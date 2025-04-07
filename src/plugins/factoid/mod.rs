use std::fmt;
use std::marker::PhantomData;
use std::str::FromStr;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use irc::client::prelude::*;
use mlua::prelude::*;
use mlua::HookTriggers;

use time::{self, OffsetDateTime, PrimitiveDateTime};

use crate::FrippyClient;
use crate::{plugin::*, ConnectionPool};
pub mod database;

mod utils;
use self::database::{count_factoids, delete_factoid, get_factoid, get_lua_value, insert_factoid};
use self::utils::*;
use crate::utils::Request;

use self::error::*;
use crate::error::ErrorKind as FrippyErrorKind;
use crate::error::FrippyError;
use failure::{format_err, ResultExt};

use frippy_derive::PluginName;

static LUA_SANDBOX: &str = include_str!("sandbox.lua");

#[derive(PluginName)]
pub struct Factoid<C: Client> {
    db: ConnectionPool,
    phantom: PhantomData<C>,
}

impl<C: Client> Factoid<C> {
    pub fn new(db: ConnectionPool) -> Self {
        Factoid {
            db,
            phantom: PhantomData,
        }
    }

    fn create_factoid(
        &self,
        name: &str,
        content: &str,
        author: &str,
    ) -> Result<&str, FactoidError> {
        let count = count_factoids(&self.db, name)?;
        let odt = OffsetDateTime::now_utc();

        let factoid = database::NewFactoid {
            name,
            idx: count,
            content,
            author,
            created: PrimitiveDateTime::new(odt.date(), odt.time()),
        };

        insert_factoid(&self.db, &factoid).map(|()| "Successfully added!")
    }

    fn add(&self, command: &mut PluginCommand) -> Result<&str, FactoidError> {
        if command.tokens.len() < 2 {
            Err(ErrorKind::InvalidCommand)?;
        }

        let name = command.tokens.remove(0);
        let content = command.tokens.join(" ");

        self.create_factoid(&name, &content, &command.source)
    }

    fn add_from_url(&self, command: &mut PluginCommand) -> Result<&str, FactoidError> {
        if command.tokens.len() < 2 {
            Err(ErrorKind::InvalidCommand)?;
        }

        let name = command.tokens.remove(0);
        let url = &command.tokens[0];
        let content = Request::from(url.as_ref())
            .max_kib(1024)
            .execute()
            .context(ErrorKind::Download)?;

        self.create_factoid(&name, &content, &command.source)
    }

    fn remove(&self, command: &mut PluginCommand) -> Result<&str, FactoidError> {
        if command.tokens.is_empty() {
            Err(ErrorKind::InvalidCommand)?;
        }

        let name = command.tokens.remove(0);
        let count = count_factoids(&self.db, &name)?;

        match delete_factoid(&self.db, &name, count - 1) {
            Ok(()) => Ok("Successfully removed"),
            Err(e) => Err(e)?,
        }
    }

    fn get(&self, command: &PluginCommand) -> Result<String, FactoidError> {
        let (name, idx) = match command.tokens.len() {
            0 => Err(ErrorKind::InvalidCommand)?,
            1 => {
                let name = &command.tokens[0];
                let count = count_factoids(&self.db, name)?;

                if count < 1 {
                    Err(ErrorKind::NotFound)?;
                }

                (name, count - 1)
            }
            _ => {
                let name = &command.tokens[0];
                let idx = match i32::from_str(&command.tokens[1]) {
                    Ok(i) => i,
                    Err(_) => Err(ErrorKind::InvalidCommand)?,
                };

                (name, idx)
            }
        };

        let factoid = get_factoid(&self.db, name, idx).context(ErrorKind::NotFound)?;

        let mut message = factoid.content.replace('\n', "|").replace('\r', "");
        message.truncate(512);

        Ok(format!("{}: {}", factoid.name, message))
    }

    fn info(&self, command: &PluginCommand) -> Result<String, FactoidError> {
        match command.tokens.len() {
            0 => Err(ErrorKind::InvalidCommand)?,
            1 => {
                let name = &command.tokens[0];
                let count = count_factoids(&self.db, name)?;

                Ok(match count {
                    0 => Err(ErrorKind::NotFound)?,
                    1 => format!("There is 1 version of {}", name),
                    _ => format!("There are {} versions of {}", count, name),
                })
            }
            _ => {
                let name = &command.tokens[0];
                let idx = i32::from_str(&command.tokens[1]).context(ErrorKind::InvalidIndex)?;
                let factoid = get_factoid(&self.db, name, idx)?;

                Ok(format!(
                    "{}: Added by {} at {} UTC",
                    name, factoid.author, factoid.created
                ))
            }
        }
    }

    fn exec(&self, mut command: PluginCommand) -> Result<String, FactoidError> {
        if command.tokens.is_empty() {
            Err(ErrorKind::InvalidIndex)?
        } else {
            let name = command.tokens.remove(0);
            let count = count_factoids(&self.db, &name)?;
            let factoid = get_factoid(&self.db, &name, count - 1)?;

            let content = factoid.content;
            let mut message = if let Some(stripped) = content.strip_prefix('>') {
                let content = String::from(stripped);

                if content.starts_with('>') {
                    content
                } else {
                    match self.run_lua(&name, &content, &command) {
                        Ok(v) => v,
                        Err(e) => match e {
                            LuaError::CallbackError { cause, .. } => match *cause {
                                LuaError::MemoryError(_) => {
                                    String::from("memory error: Factoid used over 1 MiB of ram")
                                }
                                _ => cause.to_string(),
                            },
                            LuaError::MemoryError(_) => {
                                String::from("memory error: Factoid used over 1 MiB of ram")
                            }
                            _ => e.to_string(),
                        },
                    }
                }
            } else {
                content
            };

            message.truncate(412);
            Ok(message.replace('\n', "|").replace('\r', ""))
        }
    }

    fn run_lua(&self, name: &str, code: &str, command: &PluginCommand) -> Result<String, LuaError> {
        let args = command
            .tokens
            .iter()
            .filter(|x| !x.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<String>>();

        let lua = Lua::new();
        // TODO Is this actually 1 Mib?
        lua.set_memory_limit(1024 * 1024)?;

        let start = Instant::now();
        // Check if the factoid timed out
        lua.set_hook(
            HookTriggers {
                every_line: true,
                ..Default::default()
            },
            move |_, _| {
                if Instant::now() - start > Duration::from_secs(30) {
                    return Err(LuaError::ExternalError(Arc::new(
                        format_err!("Factoid timed out after 30 seconds").compat(),
                    )));
                }

                // Limit the cpu usage of factoids
                thread::sleep(Duration::from_millis(1));

                Ok(LuaVmState::Continue)
            },
        );

        let globals = lua.globals();

        globals.set("factoid", code)?;
        globals.set("download", lua.create_function(download)?)?;
        let db = self.db.clone();
        globals.set(
            "persist",
            lua.create_function(move |_, (key, value)| persist(&db.clone(), key, value))?,
        )?;
        let db = self.db.clone();
        globals.set(
            "retrieve",
            lua.create_function(move |_, key| {
                get_lua_value(&db.clone(), key).map_err(|e| LuaError::external(e.to_string()))
            })?,
        )?;
        globals.set("json_decode", lua.create_function(json_decode)?)?;
        globals.set("json_encode", lua.create_function(json_encode)?)?;
        globals.set("parse_date", lua.create_function(parse_date)?)?;
        globals.set("format_timestamp", lua.create_function(format_timestamp)?)?;
        globals.set("sleep", lua.create_function(sleep)?)?;
        globals.set("args", args)?;
        globals.set("input", command.tokens.join(" "))?;
        globals.set("user", command.source.clone())?;
        globals.set("channel", command.target.clone())?;
        globals.set("output", lua.create_table()?)?;

        lua.load(LUA_SANDBOX).set_name(name).exec()?;

        let output = globals.get::<Vec<String>>("output")?;

        Ok(output.join("|"))
    }

    fn help(&self) -> &str {
        "usage: factoids <subcommand>\r\n\
         subcommands: add, fromurl, remove, get, info, exec, help"
    }
}

impl<C: FrippyClient> Plugin for Factoid<C> {
    type Client = C;
    fn execute(&self, _: &Self::Client, message: &Message) -> ExecutionStatus {
        match message.command {
            Command::PRIVMSG(_, ref content) => {
                if content.starts_with('!') {
                    ExecutionStatus::RequiresThread
                } else {
                    ExecutionStatus::Done
                }
            }
            _ => ExecutionStatus::Done,
        }
    }

    fn execute_threaded(
        &self,
        client: &Self::Client,
        message: &Message,
    ) -> Result<(), FrippyError> {
        if let Command::PRIVMSG(_, mut content) = message.command.clone() {
            content.remove(0);

            let t: Vec<String> = content.split(' ').map(ToOwned::to_owned).collect();

            let c = PluginCommand {
                source: message.source_nickname().unwrap().to_owned(),
                target: message.response_target().unwrap().to_owned(),
                tokens: t,
            };

            if let Ok(f) = self.exec(c) {
                client
                    .send_privmsg(message.response_target().unwrap(), f)
                    .context(FrippyErrorKind::Connection)?;
            }
        }

        Ok(())
    }

    fn command(
        &self,
        client: &Self::Client,
        mut command: PluginCommand,
    ) -> Result<(), FrippyError> {
        if command.tokens.is_empty() {
            client
                .send_privmsg(&command.target, "Invalid command")
                .context(FrippyErrorKind::Connection)?;

            return Ok(());
        }

        let target = command.target.clone();

        let sub_command = command.tokens.remove(0);
        let result = match sub_command.as_ref() {
            "add" => self.add(&mut command).map(|s| s.to_owned()),
            "fromurl" => self.add_from_url(&mut command).map(|s| s.to_owned()),
            "remove" => self.remove(&mut command).map(|s| s.to_owned()),
            "get" => self.get(&command),
            "info" => self.info(&command),
            "exec" => self.exec(command),
            "help" => Ok(self.help().to_owned()),
            _ => Err(ErrorKind::InvalidCommand.into()),
        };

        match result {
            Ok(m) => {
                client
                    .send_privmsg(&target, m)
                    .context(FrippyErrorKind::Connection)?;
            }
            Err(e) => {
                let message = e.to_string();
                client
                    .send_privmsg(&target, message)
                    .context(FrippyErrorKind::Connection)?;
                Err(e).context(FrippyErrorKind::Factoid)?
            }
        }

        Ok(())
    }

    fn evaluate(&self, _: &Self::Client, _: PluginCommand) -> Result<String, String> {
        Err(String::from(
            "Evaluation of commands is not implemented for Factoid at this time",
        ))
    }
}

impl<C: FrippyClient> fmt::Debug for Factoid<C> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Factoid {{ ... }}")
    }
}

pub mod error {
    use failure::Fail;
    use frippy_derive::Error;

    #[derive(Copy, Clone, Eq, PartialEq, Debug, Fail, Error)]
    #[error = "FactoidError"]
    pub enum ErrorKind {
        /// Invalid command error
        #[fail(display = "Invalid Command")]
        InvalidCommand,

        /// Invalid index error
        #[fail(display = "Invalid index")]
        InvalidIndex,

        /// Download  error
        #[fail(display = "Download failed")]
        Download,

        /// Duplicate error
        #[fail(display = "Entry already exists")]
        Duplicate,

        /// Not found error
        #[fail(display = "Factoid was not found")]
        NotFound,

        /// MySQL error
        #[fail(display = "Failed to execute MySQL Query")]
        MysqlError,

        /// No connection error
        #[fail(display = "No connection to the database")]
        NoConnection,
    }
}
