use std::marker::PhantomData;
use std::time::Duration;

use irc::client::prelude::*;

use lazy_static::lazy_static;
use regex::Regex;

use crate::plugin::*;
use crate::utils::Request;
use crate::FrippyClient;

use self::error::*;
use crate::error::ErrorKind as FrippyErrorKind;
use crate::error::FrippyError;
use failure::Fail;
use failure::ResultExt;
use log::debug;

use frippy_derive::PluginName;

lazy_static! {
    static ref URL_RE: Regex = Regex::new(r"(^|\s)(https?://\S+)").unwrap();
    static ref WORD_RE: Regex = Regex::new(r"(\w+)").unwrap();
}

#[derive(PluginName, Debug)]
pub struct UrlTitles<C> {
    max_kib: usize,
    phantom: PhantomData<C>,
}

#[derive(Clone, Debug)]
struct Title(String, Option<usize>);

impl From<String> for Title {
    fn from(title: String) -> Self {
        Title(title, None)
    }
}

impl From<Title> for String {
    fn from(title: Title) -> Self {
        title.0
    }
}

impl Title {
    fn find_by_delimiters(body: &str, delimiters: [&str; 3]) -> Result<Self, UrlError> {
        let title = body
            .find(delimiters[0])
            .map(|tag| {
                body[tag..]
                    .find(delimiters[1])
                    .map(|offset| tag + offset + delimiters[1].len())
                    .map(|start| {
                        body[start..]
                            .find(delimiters[2])
                            .map(|offset| start + offset)
                            .map(|end| &body[start..end])
                    })
            })
            .and_then(|s| s.and_then(|s| s))
            .ok_or(ErrorKind::MissingTitle)?;

        debug!("Found title {:?} with delimiters {:?}", title, delimiters);

        htmlescape::decode_html(title)
            .map(|t| t.into())
            .map_err(|_| ErrorKind::HtmlDecoding.into())
    }

    fn find_ogtitle(body: &str) -> Result<Self, UrlError> {
        Self::find_by_delimiters(body, ["property=\"og:title\"", "content=\"", "\""])
    }

    fn find_title(body: &str) -> Result<Self, UrlError> {
        Self::find_by_delimiters(body, ["<title", ">", "</title>"])
    }

    // TODO Improve logic
    fn get_usefulness(self, url: &str) -> Self {
        let mut usefulness = 0;
        for word in WORD_RE.find_iter(&self.0) {
            let w = word.as_str().to_lowercase();
            if w.len() > 2 && !url.to_lowercase().contains(&w) {
                usefulness += 1;
            }
        }

        Title(self.0, Some(usefulness))
    }

    pub fn usefulness(&self) -> usize {
        self.1.expect("Usefulness should be calculated already")
    }

    fn clean_up(self) -> Self {
        Title(self.0.trim().replace(['\n', '\r'], "|"), self.1)
    }

    pub fn find_clean_ogtitle(body: &str, url: &str) -> Result<Self, UrlError> {
        let title = Self::find_ogtitle(body)?;
        Ok(title.get_usefulness(url).clean_up())
    }

    pub fn find_clean_title(body: &str, url: &str) -> Result<Self, UrlError> {
        let title = Self::find_title(body)?;
        Ok(title.get_usefulness(url).clean_up())
    }
}

impl<C: FrippyClient> UrlTitles<C> {
    /// If a file is larger than `max_kib` KiB the download is stopped
    pub fn new(max_kib: usize) -> Self {
        UrlTitles {
            max_kib,
            phantom: PhantomData,
        }
    }

    fn grep_url<'a>(&self, msg: &'a str) -> Option<&'a str> {
        let captures = URL_RE.captures(msg)?;
        debug!("Url captures: {:?}", captures);

        Some(captures.get(2)?.as_str())
    }

    fn url(&self, text: &str) -> Result<String, UrlError> {
        let url = self.grep_url(text).ok_or(ErrorKind::MissingUrl)?;

        let request = Request::from(url)
            .max_kib(self.max_kib)
            .timeout(Duration::from_secs(5));

        let body = request.execute().context(ErrorKind::Download)?;

        let title = Title::find_clean_title(&body, url);
        let og_title = Title::find_clean_ogtitle(&body, url);

        let title = match (title, og_title) {
            (Ok(title), Ok(og_title)) => {
                if title.usefulness() > og_title.usefulness() {
                    title
                } else {
                    og_title
                }
            }
            (Ok(title), _) => title,
            (_, Ok(title)) => title,
            (Err(e), _) => Err(e)?,
        };

        if title.usefulness() == 0 {
            Err(ErrorKind::UselessTitle)?;
        }

        Ok(title.into())
    }
}

impl<C: FrippyClient> Plugin for UrlTitles<C> {
    type Client = C;
    fn execute(&self, _: &Self::Client, message: &Message) -> ExecutionStatus {
        match message.command {
            Command::PRIVMSG(_, ref msg) => {
                if URL_RE.is_match(msg) {
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
        if let Command::PRIVMSG(_, ref content) = message.command {
            let title = self.url(content).context(FrippyErrorKind::Url)?;
            let response = format!("[URL] {}", title);

            client
                .send_privmsg(message.response_target().unwrap(), response)
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

    fn evaluate(&self, _: &Self::Client, command: PluginCommand) -> Result<String, String> {
        self.url(&command.tokens[0])
            .map_err(|e| e.cause().unwrap().to_string())
    }
}

pub mod error {
    use failure::Fail;
    use frippy_derive::Error;

    /// A URL plugin error
    #[derive(Copy, Clone, Eq, PartialEq, Debug, Fail, Error)]
    #[error = "UrlError"]
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
    }
}
