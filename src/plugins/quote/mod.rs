use std::collections::HashMap;
use std::fmt;
use std::marker::PhantomData;
use std::ops::Deref;
use std::str::FromStr;

use antidote::Mutex;
use chrono::DateTime;
use irc::client::prelude::*;
use rand::{rng, Rng};
use time;
use time::OffsetDateTime;

pub mod database;
pub mod randomizer;
use self::database::Database;
use self::randomizer::RandomIndex;

use crate::plugin::*;
use crate::ConnectionPool;
use crate::FrippyClient;

use self::error::*;
use crate::error::ErrorKind as FrippyErrorKind;
use crate::error::FrippyError;
use failure::ResultExt;

use frippy_derive::PluginName;

#[derive(Clone)]
enum PreviousCommand {
    Get,
    GetUser(String, Option<i32>),
    Search(String, i32),
    SearchUser(String, String, i32),
}

#[derive(PluginName)]
pub struct Quote<C: Client> {
    db: ConnectionPool,
    previous_map: Mutex<HashMap<String, PreviousCommand>>,
    phantom: PhantomData<C>,
    random_index: Mutex<RandomIndex>,
}

impl<C: Client> Quote<C> {
    pub fn new(db: ConnectionPool) -> Self {
        let random_index = RandomIndex::new();

        Quote {
            db,
            previous_map: Mutex::new(HashMap::new()),
            phantom: PhantomData,
            random_index: Mutex::new(random_index),
        }
    }

    fn create_quote(
        &self,
        quotee: &str,
        channel: &str,
        content: &str,
        author: &str,
    ) -> Result<&str, QuoteError> {
        let count = self.db.count_user_quotes(quotee, channel)?;
        let odt = OffsetDateTime::now_utc();

        let quote = database::NewQuote {
            quotee,
            channel,
            idx: count + 1,
            content,
            author,
            created: DateTime::from_timestamp(odt.unix_timestamp(), 0u32)
                .expect("fails after death of universe")
                .naive_utc(),
        };

        let response = self
            .db
            .insert_quote(&quote)
            .map(|()| "Successfully added!")?;

        Ok(response)
    }

    fn add(&self, command: &mut PluginCommand) -> Result<&str, QuoteError> {
        if command.tokens.len() < 2 {
            Err(ErrorKind::InvalidCommand)?;
        }

        if command.target == command.source {
            Err(ErrorKind::PrivateMessageNotAllowed)?;
        }

        let quotee = command.tokens.remove(0);
        let channel = &command.target;
        let content = command.tokens.join(" ");

        self.create_quote(&quotee, channel, &content, &command.source)
    }

    fn get(&self, command: &PluginCommand) -> Result<String, QuoteError> {
        let tokens = command
            .tokens
            .iter()
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>();
        let quotee = &tokens.first();
        let channel = &command.target;

        match quotee {
            Some(quotee) => {
                let idx = match tokens.get(1) {
                    Some(s) => Some(i32::from_str(s).context(ErrorKind::InvalidIndex)?),
                    None => None,
                };

                self.get_user(quotee, channel, idx)
            }
            None => self.get_random(channel),
        }
    }

    fn get_user(
        &self,
        quotee: &str,
        channel: &str,
        idx: Option<i32>,
    ) -> Result<String, QuoteError> {
        let count = self.db.count_user_quotes(quotee, channel)?;
        if count < 1 {
            Err(ErrorKind::NotFound)?;
        }

        let mut idx = if let Some(idx) = idx {
            self.previous_map.lock().insert(
                channel.to_owned(),
                PreviousCommand::GetUser(quotee.to_owned(), Some(idx)),
            );

            idx
        } else {
            self.previous_map.lock().insert(
                channel.to_owned(),
                PreviousCommand::GetUser(quotee.to_owned(), None),
            );

            rng().random_range(1..count + 1)
        };

        if idx < 0 {
            idx += count + 1;
        }

        let quote = self
            .db
            .get_user_quote(quotee, channel, idx)
            .context(ErrorKind::NotFound)?;

        let response = format!(
            "\"{}\" - {}[{}/{}]",
            quote.content, quote.quotee, quote.idx, count
        );

        Ok(response)
    }

    fn get_random(&self, channel: &str) -> Result<String, QuoteError> {
        let count = self.db.count_channel_quotes(channel)?;

        if count < 1 {
            Err(ErrorKind::NotFound)?;
        }
        self.previous_map
            .lock()
            .insert(channel.to_owned(), PreviousCommand::Get);

        let mut binding = self.random_index.lock();
        binding.init(count);
        let idx = binding.get().expect("Randomizer should be initialized");

        let quote = self
            .db
            .get_channel_quote(channel, *idx)
            .context(ErrorKind::NotFound)?;

        Ok(format!(
            "\"{}\" - {}[{}]",
            quote.content, quote.quotee, quote.idx
        ))
    }

    fn search(&self, command: &mut PluginCommand) -> Result<String, QuoteError> {
        if command.tokens.len() < 2 {
            Err(ErrorKind::InvalidCommand)?;
        }

        let channel = &command.target;
        match command.tokens.remove(0).deref() {
            "user" => {
                let user = command.tokens.remove(0);

                if command.tokens.is_empty() {
                    Err(ErrorKind::InvalidCommand)?;
                }

                let query = command.tokens.join(" ");
                self.search_user(&user, channel, &query, 0)
            }
            "channel" => {
                if command.tokens.is_empty() {
                    Err(ErrorKind::InvalidCommand)?;
                }

                let query = command.tokens.join(" ");
                self.search_channel(channel, &query, 0)
            }
            _ => Err(ErrorKind::InvalidCommand.into()),
        }
    }

    fn next(&self, channel: String) -> Result<String, QuoteError> {
        let previous = self
            .previous_map
            .lock()
            .get(&channel)
            .cloned()
            .ok_or(ErrorKind::NoPrevious)?;

        match previous {
            PreviousCommand::Get => self.get_random(&channel),
            PreviousCommand::GetUser(user, idx) => {
                let idx = idx.map(|idx| if idx < 0 { idx - 1 } else { idx + 1 });

                self.get_user(&user, &channel, idx)
            }
            PreviousCommand::Search(query, offset) => {
                self.search_channel(&channel, &query, offset + 1)
            }
            PreviousCommand::SearchUser(user, query, offset) => {
                self.search_user(&user, &channel, &query, offset + 1)
            }
        }
    }

    fn search_user(
        &self,
        user: &str,
        channel: &str,
        query: &str,
        offset: i32,
    ) -> Result<String, QuoteError> {
        self.previous_map.lock().insert(
            channel.to_owned(),
            PreviousCommand::SearchUser(user.to_owned(), query.to_owned(), offset),
        );

        let quote = self
            .db
            .search_user_quote(query, user, channel, offset)
            .context(ErrorKind::NotFound)?;

        let response = format!("\"{}\" - {}[{}]", quote.content, quote.quotee, quote.idx);

        Ok(response)
    }

    fn search_channel(
        &self,
        channel: &str,
        query: &str,
        offset: i32,
    ) -> Result<String, QuoteError> {
        self.previous_map.lock().insert(
            channel.to_owned(),
            PreviousCommand::Search(query.to_owned(), offset),
        );

        let quote = self
            .db
            .search_channel_quote(query, channel, offset)
            .context(ErrorKind::NotFound)?;

        let response = format!("\"{}\" - {}[{}]", quote.content, quote.quotee, quote.idx);

        Ok(response)
    }

    fn info(&self, command: &PluginCommand) -> Result<String, QuoteError> {
        let tokens = command
            .tokens
            .iter()
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>();
        match tokens.len() {
            0 => {
                let channel = &command.target;
                let count = self.db.count_channel_quotes(channel)?;

                Ok(match count {
                    0 => Err(ErrorKind::NotFound)?,
                    1 => format!("1 quote was saved in {}", channel),
                    _ => format!("{} quotes were saved in {}", count, channel),
                })
            }
            1 => {
                let quotee = &command.tokens[0];
                let channel = &command.target;
                let count = self.db.count_user_quotes(quotee, channel)?;

                Ok(match count {
                    0 => Err(ErrorKind::NotFound)?,
                    1 => format!("{} has 1 quote", quotee),
                    _ => format!("{} has {} quotes", quotee, count),
                })
            }
            _ => {
                let quotee = &command.tokens[0];
                let channel = &command.target;
                let idx = i32::from_str(&command.tokens[1]).context(ErrorKind::InvalidIndex)?;

                let idx = if idx < 0 {
                    self.db.count_user_quotes(quotee, channel)? + idx + 1
                } else {
                    idx
                };

                let quote = self
                    .db
                    .get_user_quote(quotee, channel, idx)
                    .context(ErrorKind::NotFound)?;

                Ok(format!(
                    "{}'s quote was added by {} at {} UTC",
                    quote.quotee, quote.author, quote.created
                ))
            }
        }
    }

    fn help(&self) -> &str {
        "usage: quote <subcommand>\r\n\
         subcommands:\r\n \
         add USER QUOTE                    Add a new quote\r\n \
         get [user] [index]                Get a random or specific quote\r\n \
         search user/channel [name] QUERY  Search the quote of either the current channel or a user\r\n \
         next                              Go to next quote from the previous search/get\r\n \
         info USER [index]                 Get all available information about a quote\r\n \
         help                              Get this message"
    }
}

impl<C: FrippyClient> Plugin for Quote<C> {
    type Client = C;
    fn execute(&self, _: &Self::Client, _: &Message) -> ExecutionStatus {
        ExecutionStatus::Done
    }

    fn execute_threaded(&self, _: &Self::Client, _: &Message) -> Result<(), FrippyError> {
        panic!("Quotes should not use threading")
    }

    fn command(
        &self,
        client: &Self::Client,
        mut command: PluginCommand,
    ) -> Result<(), FrippyError> {
        if command.tokens.is_empty() {
            client
                .send_privmsg(&command.target, ErrorKind::InvalidCommand.to_string())
                .context(FrippyErrorKind::Connection)?;

            return Ok(());
        }

        let target = command.target.clone();

        let sub_command = command.tokens.remove(0).to_lowercase();
        let result = match sub_command.as_ref() {
            "add" => self.add(&mut command).map(|s| s.to_owned()),
            "get" => self.get(&command),
            "search" => self.search(&mut command),
            "next" => self.next(command.target),
            "info" => self.info(&command),
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
                Err(e).context(FrippyErrorKind::Quote)?
            }
        }

        Ok(())
    }

    fn evaluate(&self, _: &Self::Client, _: PluginCommand) -> Result<String, String> {
        Err(String::from(
            "Evaluation of commands is not implemented for Quote at this time",
        ))
    }
}

impl<C: FrippyClient> fmt::Debug for Quote<C> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Quote {{ ... }}")
    }
}

pub mod error {
    use failure::Fail;
    use frippy_derive::Error;

    #[derive(Copy, Clone, Eq, PartialEq, Debug, Fail, Error)]
    #[error = "QuoteError"]
    pub enum ErrorKind {
        /// Invalid command error
        #[fail(display = "Incorrect command. Send \"quote help\" for help")]
        InvalidCommand,

        /// Invalid index error
        #[fail(display = "Invalid index")]
        InvalidIndex,

        /// No previous command error
        #[fail(display = "No previous command was found for this channel")]
        NoPrevious,

        /// Private message error
        #[fail(display = "You can only add quotes in channel messages")]
        PrivateMessageNotAllowed,

        /// Download  error
        #[fail(display = "Download failed")]
        Download,

        /// Duplicate error
        #[fail(display = "Entry already exists")]
        Duplicate,

        /// Not found error
        #[fail(display = "Quote was not found")]
        NotFound,

        /// MySQL error
        #[fail(display = "Failed to execute MySQL Query")]
        MysqlError,

        /// No connection error
        #[fail(display = "No connection to the database")]
        NoConnection,
    }
}
