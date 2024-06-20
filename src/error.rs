//! Errors for `frippy` crate using `failure`.

use failure::Fail;
use log::error;

use frippy_derive::Error;

#[allow(deprecated)] //Fail derive is giving issues here on the .causes() call
pub fn log_error(e: &FrippyError) {
    let text = e
        .causes()
        .skip(1)
        .fold(format!("{}", e), |acc, err| format!("{}: {}", acc, err));
    error!("{}", text);
}

/// The main crate-wide error type.
#[derive(Copy, Clone, Eq, PartialEq, Debug, Fail, Error)]
#[error = "FrippyError"]
pub enum ErrorKind {
    /// Connection error
    #[fail(display = "A connection error occured")]
    Connection,

    /// Thread spawn error
    #[fail(display = "Failed to spawn thread")]
    ThreadSpawn,

    /// A Url error
    #[fail(display = "A Url error has occured")]
    Url,

    /// A Tell error
    #[fail(display = "A Tell error has occured")]
    Tell,

    /// A Factoid error
    #[fail(display = "A Factoid error has occured")]
    Factoid,

    /// A Quote error
    #[fail(display = "A Quote error has occured")]
    Quote,

    /// A Remind error
    #[fail(display = "A Remind error has occured")]
    Remind,

    /// A Counter error
    #[fail(display = "A Counter error has occured")]
    Counter,

    /// A Ollama error
    #[fail(display = "An Ollama error has occured")]
    Ollama,
}
