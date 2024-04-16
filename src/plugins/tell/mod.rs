use std::marker::PhantomData;

use chrono::DateTime;
use irc::client::data::User;
use irc::client::prelude::*;

use humantime::format_duration;
use itertools::Itertools;
use std::time::Duration;
use time;

use crate::plugin::*;
use crate::ConnectionPool;
use crate::FrippyClient;

use self::error::*;
use crate::error::ErrorKind as FrippyErrorKind;
use crate::error::FrippyError;
use failure::Fail;
use failure::ResultExt;
use log::{debug, trace};

use frippy_derive::PluginName;

pub mod database;
use self::database::Database;

#[derive(PluginName)]
pub struct Tell<C> {
    db: ConnectionPool,
    phantom: PhantomData<C>,
}

impl<C: FrippyClient> Tell<C> {
    pub fn new(db: ConnectionPool) -> Self {
        Tell {
            db,
            phantom: PhantomData,
        }
    }

    fn tell_command(&self, client: &C, command: PluginCommand) -> Result<String, TellError> {
        if command.tokens.len() < 2 {
            return Ok(self.invalid_command().to_owned());
        }

        let mut online = Vec::new();

        let receivers = command.tokens[0]
            .split(',')
            .filter(|&s| !s.is_empty())
            .unique()
            .collect::<Vec<_>>();
        let sender = command.source;

        let mut no_receiver = true;
        for receiver in &receivers {
            if receiver.eq_ignore_ascii_case(client.current_nickname())
                || receiver.eq_ignore_ascii_case(&sender)
            {
                if !online.contains(&receiver) {
                    online.push(receiver);
                }
                continue;
            }

            let channels = client
                .list_channels()
                .expect("The irc crate should not be compiled with the \"nochanlists\" feature");

            let find_receiver = |option: Option<Vec<User>>| {
                option.and_then(|users| {
                    users
                        .into_iter()
                        .find(|user| user.get_nickname().eq_ignore_ascii_case(receiver))
                })
            };

            if channels
                .iter()
                .map(|channel| client.list_users(channel))
                .map(find_receiver)
                .any(|option| option.is_some())
                && !online.contains(&receiver)
            {
                // online.push(receiver);
            }

            let tm = time::now().to_timespec();
            let message = command.tokens[1..].join(" ");
            let tell = database::NewTellMessage {
                sender: &sender,
                receiver: &receiver.to_lowercase(),
                time: DateTime::from_timestamp(tm.sec, 0u32).unwrap().naive_utc(),
                message: &message,
            };

            debug!("Saving tell for {:?}", receiver);
            self.db.insert_tell(&tell)?;
            no_receiver = false;
        }

        let resp = if no_receiver {
            String::from("Invalid receiver.")
        } else {
            format!("Sending tell to {}.", receivers.join(", "))
        };

        Ok(resp)
    }

    fn on_namelist(&self, client: &C, channel: &str) -> Result<(), FrippyError> {
        let receivers = self.db.get_receivers().context(FrippyErrorKind::Tell)?;

        if let Some(users) = client.list_users(channel) {
            debug!("Outstanding tells for {:?}", receivers);

            for receiver in users
                .iter()
                .map(|u| u.get_nickname())
                .filter(|u| receivers.iter().any(|r| r == &u.to_lowercase()))
            {
                self.send_tells(client, receiver, channel)?;
            }
        }

        Ok(())
    }

    fn send_tells(&self, client: &C, receiver: &str, channel: &str) -> Result<(), FrippyError> {
        trace!("Checking {} for tells", receiver);

        if client.current_nickname() == receiver {
            return Ok(());
        }

        let tell_messages = match self.db.get_tells(&receiver.to_lowercase()) {
            Ok(t) => t,
            Err(e) => {
                // This warning only occurs if frippy is built without a database
                #[allow(unreachable_patterns)]
                return match e.kind() {
                    ErrorKind::NotFound => Ok(()),
                    _ => Err(e.context(FrippyErrorKind::Tell))?,
                };
            }
        };

        for tell in tell_messages {
            let now = Duration::new(time::now().to_timespec().sec as u64, 0);
            let dur = now - Duration::new(tell.time.and_utc().timestamp() as u64, 0);
            let human_dur = format_duration(dur);

            let message = format!(
                "{}, {} sent you a tell {} ago: {}",
                receiver, tell.sender, human_dur, tell.message
            );

            client
                .send_privmsg(channel, &message)
                .context(FrippyErrorKind::Connection)?;

            debug!(
                "Sent {:?} from {:?} to {:?} in channel {:?}",
                tell.message, tell.sender, receiver, channel,
            );
        }

        self.db
            .delete_tells(&receiver.to_lowercase())
            .context(FrippyErrorKind::Tell)?;

        Ok(())
    }

    fn invalid_command(&self) -> &str {
        "Incorrect Command. \
         Send \"tell help\" for help."
    }

    fn help(&self) -> &str {
        "Used to send messages to offline users which they will receive when they come online.\r\n
         usage: tell user message\r\n\
         example: tell Foobar Hello!"
    }
}

impl<C: FrippyClient> Plugin for Tell<C> {
    type Client = C;
    fn execute(&self, client: &Self::Client, message: &Message) -> ExecutionStatus {
        let source = message.source_nickname();
        let target = message.response_target();

        let res = match message.command {
            Command::JOIN(_, _, _) => self.send_tells(client, source.unwrap(), target.unwrap()),
            Command::NICK(ref nick) => self.send_tells(client, nick, nick),
            Command::PRIVMSG(_, _) => self.send_tells(client, source.unwrap(), target.unwrap()),
            Command::Response(resp, ref chan_info, _) => {
                if resp == Response::RPL_NAMREPLY {
                    debug!("NAMREPLY info: {:?}", chan_info);

                    self.on_namelist(client, &chan_info[chan_info.len() - 1])
                } else {
                    Ok(())
                }
            }
            _ => Ok(()),
        };

        match res {
            Ok(_) => ExecutionStatus::Done,
            Err(e) => ExecutionStatus::Err(e),
        }
    }

    fn execute_threaded(&self, _: &Self::Client, _: &Message) -> Result<(), FrippyError> {
        panic!("Tell should not use threading")
    }

    fn command(&self, client: &Self::Client, command: PluginCommand) -> Result<(), FrippyError> {
        if command.tokens.is_empty() {
            client
                .send_privmsg(&command.target, self.invalid_command())
                .context(FrippyErrorKind::Connection)?;
            return Ok(());
        }

        let target = command.target.clone();

        match command.tokens[0].as_ref() {
            "help" => client
                .send_privmsg(&target, self.help())
                .context(FrippyErrorKind::Connection),
            _ => match self.tell_command(client, command) {
                Ok(msg) => client
                    .send_privmsg(&target, msg)
                    .context(FrippyErrorKind::Connection),
                Err(e) => client
                    .send_privmsg(&target, e.to_string())
                    .context(FrippyErrorKind::Connection),
            },
        }?;

        Ok(())
    }

    fn evaluate(&self, _: &Self::Client, _: PluginCommand) -> Result<String, String> {
        Err(String::from("This Plugin does not implement any commands."))
    }
}

use std::fmt;
impl<C: FrippyClient> fmt::Debug for Tell<C> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Tell {{ ... }}")
    }
}

pub mod error {
    use failure::Fail;
    use frippy_derive::Error;

    #[derive(Copy, Clone, Eq, PartialEq, Debug, Fail, Error)]
    #[error = "TellError"]
    pub enum ErrorKind {
        /// Not found command error
        #[fail(display = "Tell was not found")]
        NotFound,

        /// MySQL error
        #[fail(display = "Failed to execute MySQL Query")]
        MysqlError,

        /// No connection error
        #[fail(display = "No connection to the database")]
        NoConnection,
    }
}
