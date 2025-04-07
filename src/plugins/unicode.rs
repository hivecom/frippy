use std::marker::PhantomData;

use irc::client::prelude::*;

use crate::plugin::*;
use crate::FrippyClient;

use crate::error::ErrorKind as FrippyErrorKind;
use crate::error::FrippyError;
use failure::Fail;

use frippy_derive::PluginName;

#[derive(PluginName, Default, Debug)]
pub struct Unicode<C> {
    phantom: PhantomData<C>,
}

impl<C: FrippyClient> Unicode<C> {
    pub fn new() -> Unicode<C> {
        Unicode {
            phantom: PhantomData,
        }
    }

    fn get_name(&self, symbol: char) -> String {
        match unicode_names2::name(symbol) {
            Some(sym) => sym.to_string().to_lowercase(),
            None => String::from("UNKNOWN"),
        }
    }

    fn format_response(&self, content: &str) -> String {
        let character = content
            .chars()
            .next()
            .expect("content contains at least one character");

        let mut buf = [0; 4];

        let bytes = character
            .encode_utf8(&mut buf)
            .as_bytes()
            .iter()
            .map(|b| format!("{:#x}", b))
            .collect::<Vec<String>>();

        let name = self.get_name(character);

        if bytes.len() > 1 {
            format!(
                "{} is '{}' | UTF-8: {2:#x} ({2}), Bytes: [{3}]",
                character,
                name,
                character as u32,
                bytes.join(",")
            )
        } else {
            format!(
                "{} is '{}' | UTF-8: {2:#x} ({2})",
                character, name, character as u32
            )
        }
    }
}

impl<C: FrippyClient> Plugin for Unicode<C> {
    type Client = C;

    fn execute(&self, _: &Self::Client, _: &Message) -> ExecutionStatus {
        ExecutionStatus::Done
    }

    fn execute_threaded(&self, _: &Self::Client, _: &Message) -> Result<(), FrippyError> {
        panic!("Unicode should not use threading")
    }

    fn command(&self, client: &Self::Client, command: PluginCommand) -> Result<(), FrippyError> {
        let token = match command.tokens.iter().find(|t| !t.is_empty()) {
            Some(t) => t,
            None => {
                let msg = "No non-space character was found.";

                if let Err(e) = client.send_privmsg(command.target, msg) {
                    Err(e.context(FrippyErrorKind::Connection))?;
                }

                return Ok(());
            }
        };

        if let Err(e) = client.send_privmsg(command.target, self.format_response(token)) {
            Err(e.context(FrippyErrorKind::Connection))?;
        }

        Ok(())
    }

    fn evaluate(&self, _: &Self::Client, command: PluginCommand) -> Result<String, String> {
        let tokens = command.tokens;

        if tokens.is_empty() {
            return Err(String::from("No non-space character was found."));
        }

        Ok(self.format_response(&tokens[0]))
    }
}
