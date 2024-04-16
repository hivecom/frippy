use std::fmt;
use std::marker::PhantomData;

use irc::client::prelude::*;

use crate::plugin::*;
use crate::ConnectionPool;
use crate::FrippyClient;
pub mod database;
use self::database::Database;

use self::error::*;
use crate::error::ErrorKind as FrippyErrorKind;
use crate::error::FrippyError;
use failure::ResultExt;

use frippy_derive::PluginName;

#[derive(PluginName)]
pub struct Counter<C: Client> {
    db: ConnectionPool,
    phantom: PhantomData<C>,
}

impl<C: Client> Counter<C> {
    pub fn new(db: ConnectionPool) -> Self {
        Self {
            db,
            phantom: PhantomData,
        }
    }

    fn get(&self, name: &str) -> Result<String, CounterError> {
        self.db.get_count(name).map(|c| c.to_string())
    }

    fn add(&self, name: &str) -> Result<String, CounterError> {
        self.db.add(name).map(|c| c.to_string())
    }

    fn subtract(&self, name: &str) -> Result<String, CounterError> {
        self.db.subtract(name).map(|c| c.to_string())
    }
}

impl<C: FrippyClient> Plugin for Counter<C> {
    type Client = C;
    fn execute(&self, _: &Self::Client, message: &Message) -> ExecutionStatus {
        if let Command::PRIVMSG(_, content) = message.command.clone() {
            if content.contains(' ')
                || content.len() <= 2
                || !content.is_char_boundary(content.len() - 2)
            {
                return ExecutionStatus::Done;
            }
            if ["++", "--", "=="].contains(&&content[content.len() - 2..]) {
                return ExecutionStatus::RequiresThread;
            }
        }

        ExecutionStatus::Done
    }

    fn execute_threaded(
        &self,
        client: &Self::Client,
        message: &Message,
    ) -> Result<(), FrippyError> {
        if let Command::PRIVMSG(_, content) = message.command.clone() {
            let (name, end) = content.split_at(content.len() - 2);
            let count = match end {
                "++" => self.add(name),
                "--" => self.subtract(name),
                "==" => self.get(name),
                _ => unreachable!("execute checks this already"),
            }
            .context(FrippyErrorKind::Counter)?;

            client
                .send_privmsg(message.response_target().unwrap_or(""), count)
                .context(FrippyErrorKind::Connection)?;
        }

        Ok(())
    }

    fn command(&self, client: &Self::Client, command: PluginCommand) -> Result<(), FrippyError> {
        client
            .send_privmsg(
                command.target,
                "This Plugin does not implement any commands.",
            )
            .context(FrippyErrorKind::Connection)?;

        Ok(())
    }

    fn evaluate(&self, _: &Self::Client, _: PluginCommand) -> Result<String, String> {
        Err(String::from(
            "Evaluation of commands is not implemented for Factoid at this time",
        ))
    }
}

impl<C: FrippyClient> fmt::Debug for Counter<C> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Counter {{ ... }}")
    }
}

pub mod error {
    use failure::Fail;
    use frippy_derive::Error;

    #[derive(Copy, Clone, Eq, PartialEq, Debug, Fail, Error)]
    #[error = "CounterError"]
    pub enum ErrorKind {
        /// MySQL error
        #[fail(display = "Failed to execute MySQL Query")]
        MysqlError,

        /// No connection error
        #[fail(display = "No connection to the database")]
        NoConnection,
    }
}
