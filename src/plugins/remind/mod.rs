use std::marker::PhantomData;
use std::thread::{sleep, spawn};
use std::{fmt, time::Duration};

use antidote::RwLock;
use irc::client::prelude::*;

use chrono::{self, DateTime, NaiveDateTime};
use time;

use crate::FrippyClient;
use crate::{plugin::*, ConnectionPool};

pub mod database;
mod parser;
use self::database::Database;
use self::parser::CommandParser;

use self::error::*;
use crate::error::ErrorKind as FrippyErrorKind;
use crate::error::FrippyError;
use failure::ResultExt;
use log::{debug, error};

use frippy_derive::PluginName;

fn get_time() -> NaiveDateTime {
    let tm = time::now().to_timespec();
    DateTime::from_timestamp(tm.sec, 0u32).unwrap().naive_utc()
}

fn get_events(db: &ConnectionPool, in_next: chrono::Duration) -> Vec<database::Event> {
    loop {
        let before = get_time() + in_next;
        match db.get_events_before(&before) {
            Ok(events) => return events,
            Err(e) => {
                if e.kind() != ErrorKind::NotFound {
                    error!("Failed to get events: {}", e);
                }
            }
        }

        sleep(in_next.to_std().expect("Failed to convert look ahead time"));
    }
}

fn run<C: FrippyClient>(client: &C, db: ConnectionPool) {
    let look_ahead = chrono::Duration::minutes(2);

    let mut events = get_events(&db, look_ahead);

    let mut sleep_time = look_ahead
        .to_std()
        .expect("Failed to convert look ahead time");

    loop {
        let now = get_time();
        for event in events {
            if event.time <= now {
                let msg = format!("Reminder from {}: {}", event.author, event.content);
                if let Err(e) = client.send_notice(&event.receiver, &msg) {
                    error!("Failed to send reminder: {}", e);
                } else {
                    debug!("Sent reminder {:?}", event);

                    if let Some(repeat) = event.repeat {
                        let next_time = event.time + chrono::Duration::seconds(repeat);

                        if let Err(e) = db.update_event_time(event.id, &next_time) {
                            error!("Failed to update reminder: {}", e);
                        } else {
                            debug!("Updated time");
                        }
                    } else if let Err(e) = db.delete_event(event.id) {
                        error!("Failed to delete reminder: {}", e);
                    }
                }
            } else {
                let until_event = (event.time - now)
                    .to_std()
                    .expect("Failed to convert until event time");

                if until_event < sleep_time {
                    sleep_time = until_event + Duration::from_secs(1);
                }
            }
        }

        sleep(sleep_time);
        sleep_time = Duration::from_secs(120);

        events = get_events(&db, look_ahead);
    }
}

#[derive(PluginName)]
pub struct Remind<C> {
    db: ConnectionPool,
    has_reminder: RwLock<bool>,
    phantom: PhantomData<C>,
}

impl<C: FrippyClient> Remind<C> {
    pub fn new(db: ConnectionPool) -> Self {
        Remind {
            db,
            has_reminder: RwLock::new(false),
            phantom: PhantomData,
        }
    }

    fn user_cmd(&self, command: PluginCommand) -> Result<String, RemindError> {
        let parser = CommandParser::parse_target(command.tokens)?;

        self.set(&parser, &command.source)
    }

    fn me_cmd(&self, command: PluginCommand) -> Result<String, RemindError> {
        let source = command.source.clone();
        let parser = CommandParser::with_target(command.tokens, command.source)?;

        self.set(&parser, &source)
    }

    fn set(&self, parser: &CommandParser, author: &str) -> Result<String, RemindError> {
        debug!("parser: {:?}", parser);

        let target = parser.get_target();
        let time = parser.get_time(Duration::from_secs(120))?;

        let event = database::NewEvent {
            receiver: target,
            content: parser.get_message(),
            author,
            time: &time,
            repeat: parser
                .get_repeat(Duration::from_secs(300))?
                .map(|d| d.as_secs() as i64),
        };

        debug!("New event: {:?}", event);

        self.db
            .insert_event(&event)
            .map(|id| format!("Created reminder with id {} at {} UTC", id, time))
    }

    fn list(&self, user: &str) -> Result<String, RemindError> {
        let mut events = self.db.get_user_events(user)?;

        if events.is_empty() {
            Err(ErrorKind::NotFound)?;
        }

        let mut list = events.remove(0).to_string();
        for ev in events {
            list.push_str("\r\n");
            list.push_str(&ev.to_string());
        }

        Ok(list)
    }

    fn delete(&self, mut command: PluginCommand) -> Result<&str, RemindError> {
        let id = command
            .tokens
            .remove(0)
            .parse::<i64>()
            .context(ErrorKind::Parsing)?;
        let event = self.db.get_event(id).context(ErrorKind::NotFound)?;

        if event.receiver.eq_ignore_ascii_case(&command.source)
            || event.author.eq_ignore_ascii_case(&command.source)
        {
            self.db.delete_event(id).map(|()| "Successfully deleted")
        } else {
            Ok("Only the author or receiver can delete a reminder")
        }
    }

    fn help(&self) -> &str {
        "usage: remind <subcommand>\r\n\
         subcommands: user, me, list, delete, help\r\n\
         examples\r\n\
         remind user foo to sleep in 1 hour\r\n\
         remind me to leave early on 1.1 at 16:00 every week"
    }
}

impl<C: FrippyClient + 'static> Plugin for Remind<C> {
    type Client = C;
    fn execute(&self, client: &Self::Client, msg: &Message) -> ExecutionStatus {
        if let Command::JOIN(_, _, _) = msg.command {
            let mut has_reminder = self.has_reminder.write();

            if !*has_reminder {
                let events = self.db.clone();
                let client = client.clone();

                spawn(move || run(&client, events));

                *has_reminder = true;
            }
        }

        ExecutionStatus::Done
    }

    fn execute_threaded(&self, _: &Self::Client, _: &Message) -> Result<(), FrippyError> {
        panic!("Remind should not use frippy's threading")
    }

    fn command(
        &self,
        client: &Self::Client,
        mut command: PluginCommand,
    ) -> Result<(), FrippyError> {
        if command.tokens.is_empty() {
            client
                .send_notice(&command.source, ErrorKind::InvalidCommand.to_string())
                .context(FrippyErrorKind::Connection)?;
            return Ok(());
        }

        let source = command.source.clone();

        let sub_command = command.tokens.remove(0);
        let response = match sub_command.as_ref() {
            "user" => self.user_cmd(command),
            "me" => self.me_cmd(command),
            "delete" => self.delete(command).map(|s| s.to_owned()),
            "list" => self.list(&source),
            "help" => Ok(self.help().to_owned()),
            _ => Err(ErrorKind::InvalidCommand.into()),
        };

        match response {
            Ok(msg) => client
                .send_notice(&source, msg)
                .context(FrippyErrorKind::Connection)?,
            Err(e) => {
                let message = e.to_string();

                client
                    .send_notice(&source, message)
                    .context(FrippyErrorKind::Connection)?;

                Err(e).context(FrippyErrorKind::Remind)?
            }
        }

        Ok(())
    }

    fn evaluate(&self, _: &Self::Client, _: PluginCommand) -> Result<String, String> {
        Err(String::from(
            "Evaluation of commands is not implemented for remind at this time",
        ))
    }
}

impl<C: FrippyClient> fmt::Debug for Remind<C> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Remind {{ ... }}")
    }
}

pub mod error {
    use failure::Fail;
    use frippy_derive::Error;

    #[derive(Copy, Clone, Eq, PartialEq, Debug, Fail, Error)]
    #[error = "RemindError"]
    pub enum ErrorKind {
        /// Invalid command error
        #[fail(display = "Incorrect Command. Send \"remind help\" for help.")]
        InvalidCommand,

        /// Missing message error
        #[fail(display = "Reminder needs to have a description")]
        MissingMessage,

        /// Missing receiver error
        #[fail(display = "Specify who to remind")]
        MissingReceiver,

        /// Missing time error
        #[fail(display = "Reminder needs to have a time")]
        MissingTime,

        /// Invalid time error
        #[fail(display = "Could not parse time")]
        InvalidTime,

        /// Invalid date error
        #[fail(display = "Could not parse date")]
        InvalidDate,

        /// Parse error
        #[fail(display = "Could not parse integers")]
        Parsing,

        /// Ambigous time error
        #[fail(display = "Time specified is ambiguous")]
        AmbiguousTime,

        /// Time too short error
        #[fail(display = "Reminder needs to be in over 2 minutes")]
        TimeShort,

        /// Repeat time too short error
        #[fail(display = "Repeat time needs to be over 5 minutes")]
        RepeatTimeShort,

        /// Duplicate error
        #[fail(display = "Entry already exists")]
        Duplicate,

        /// Not found error
        #[fail(display = "No events found")]
        NotFound,

        /// MySQL error
        #[fail(display = "Failed to execute MySQL Query")]
        MysqlError,

        /// No connection error
        #[fail(display = "No connection to the database")]
        NoConnection,
    }
}
