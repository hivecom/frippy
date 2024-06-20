use std::marker::PhantomData;
use std::time::Duration;

use antidote::Mutex;
use irc::client::prelude::*;
use reqwest::Method;
use serde::Deserialize;
use serde::Serialize;

use crate::plugin::*;
use crate::utils::Request;
use crate::FrippyClient;

use crate::error::ErrorKind as FrippyErrorKind;
use crate::error::FrippyError;
use failure::ResultExt;

use frippy_derive::PluginName;

use self::error::OllamaError;
use self::error::*;

#[derive(PluginName, Debug)]
pub struct Ollama<C> {
    url: String,
    model: String,
    context: Mutex<Vec<u16>>,
    phantom: PhantomData<C>,
}

impl<C: FrippyClient> Ollama<C> {
    pub fn new(url: String, model: String) -> Self {
        Ollama {
            url,
            model,
            context: Mutex::new(Vec::new()),
            phantom: PhantomData,
        }
    }
}

#[derive(Debug, Serialize)]
struct OllamaRequest {
    model: String,
    prompt: String,
    context: Vec<u16>,
    stream: bool,
    options: OllamaOptions,
}

#[derive(Debug, Serialize)]
struct OllamaOptions {
    num_predict: i64,
}

#[derive(Debug, Deserialize)]
struct OllamaReply {
    response: String,
    context: Vec<u16>,
}

impl<C> Ollama<C> {
    fn prompt(&self, prompt: String) -> Result<String, OllamaError> {
        let mut context = self.context.lock();
        let body = serde_json::to_string(&OllamaRequest {
            model: self.model.clone(),
            prompt,
            context: context.clone(),
            stream: false,
            options: OllamaOptions { num_predict: 200 },
        })
        .context(ErrorKind::Serialize)?;

        let request = Request::from(self.url.clone())
            .timeout(Duration::from_secs(30))
            .method(Method::POST)
            .max_kib(1024)
            .body(body);

        let data = request.execute().context(ErrorKind::Download)?;
        let mut reply: OllamaReply = serde_json::from_str(&data).context(ErrorKind::Deserialize)?;

        *context = reply.context;

        reply.response.truncate(412);
        Ok(reply.response.replace('\n', "|").replace('\r', ""))
    }

    fn clear(&self) {
        self.context.lock().clear();
    }
}

impl<C: FrippyClient> Plugin for Ollama<C> {
    type Client = C;
    fn execute(&self, _: &Self::Client, _: &Message) -> ExecutionStatus {
        ExecutionStatus::Done
    }

    fn execute_threaded(&self, _: &Self::Client, _: &Message) -> Result<(), FrippyError> {
        panic!("Ollama should not use threading")
    }

    fn command(&self, client: &Self::Client, command: PluginCommand) -> Result<(), FrippyError> {
        if command.tokens[0] == "/clear" {
            self.clear();

            client
                .send_privmsg(command.target, "Cleared memory")
                .context(FrippyErrorKind::Connection)?;
            return Ok(());
        }

        let reply = self
            .prompt(command.tokens.join(" "))
            .context(FrippyErrorKind::Ollama)?;
        client
            .send_privmsg(command.target, reply)
            .context(FrippyErrorKind::Connection)?;

        Ok(())
    }

    fn evaluate(&self, _: &Self::Client, _: PluginCommand) -> Result<String, String> {
        Err(String::from(
            "Evaluation of commands is not implemented for ollama at this time",
        ))
    }
}

pub mod error {
    use failure::Fail;
    use frippy_derive::Error;

    /// A URL plugin error
    #[derive(Copy, Clone, Eq, PartialEq, Debug, Fail, Error)]
    #[error = "OllamaError"]
    pub enum ErrorKind {
        /// A download error
        #[fail(display = "A download error occured")]
        Download,

        /// Missing URL error
        #[fail(display = "No URL was found")]
        MissingUrl,

        /// Missing title error
        #[fail(display = "No title was found")]
        MissingTitle,

        /// Useless title error
        #[fail(display = "The titles found were not useful enough")]
        UselessTitle,

        /// Html decoding error
        #[fail(display = "Failed to decode Html characters")]
        HtmlDecoding,

        /// Deserialize error
        #[fail(display = "Failed to deserialize reply")]
        Deserialize,

        /// Serialize error
        #[fail(display = "Failed to serialize reply")]
        Serialize,
    }
}

#[cfg(test)]
mod tests {
    use crate::tests::TestClient;

    use super::*;

    #[test]
    fn prompt() {
        let ollama = Ollama::<TestClient>::new(
            "http://localhost:11434/api/generate".to_string(),
            "tinyllama".to_string(),
        );
        dbg!(ollama
            .prompt(String::from("What color is the sky?"))
            .unwrap());
        dbg!();
        dbg!(ollama.prompt(String::from("What did I just ask?")).unwrap());
        panic!()
    }
}
