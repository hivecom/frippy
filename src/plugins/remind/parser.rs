use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime};
use humantime::parse_duration;
use std::time::Duration;
use time;

use super::error::*;
use failure::ResultExt;
use log::debug;

#[derive(Default, Debug)]
pub struct CommandParser {
    on_date: Option<String>,
    at_time: Option<String>,
    in_duration: Option<String>,
    every_time: Option<String>,
    target: String,
    message: Option<String>,
}

#[derive(PartialEq, Clone, Copy)]
enum ParseState {
    None,
    On,
    At,
    In,
    Every,
    Msg,
}

impl CommandParser {
    pub fn parse_target(mut tokens: Vec<String>) -> Result<Self, RemindError> {
        let mut parser = CommandParser::default();

        if tokens.is_empty() {
            Err(ErrorKind::MissingReceiver)?;
        }

        parser.target = tokens.remove(0);

        parser.parse_tokens(tokens)
    }

    pub fn with_target(tokens: Vec<String>, target: String) -> Result<Self, RemindError> {
        let parser = CommandParser {
            target,
            ..Default::default()
        };

        parser.parse_tokens(tokens)
    }

    fn parse_tokens(mut self, tokens: Vec<String>) -> Result<Self, RemindError> {
        let mut state = ParseState::None;
        let mut cur_str = String::new();

        for token in tokens {
            let next_state = match token.as_ref() {
                "on" => ParseState::On,
                "at" => ParseState::At,
                "in" => ParseState::In,
                "every" => ParseState::Every,
                "to" => ParseState::Msg,
                _ => {
                    if !cur_str.is_empty() {
                        cur_str.push(' ');
                    }
                    cur_str.push_str(&token);
                    state
                }
            };

            if next_state != state {
                if state != ParseState::None {
                    self = self.add_string_by_state(state, cur_str)?;
                    cur_str = String::new();
                }

                state = next_state;
            }
        }

        self = self.add_string_by_state(state, cur_str)?;

        if self.message.is_none() {
            return Err(ErrorKind::MissingMessage.into());
        }

        if self.in_duration.is_some() && self.at_time.is_some()
            || self.in_duration.is_some() && self.on_date.is_some()
        {
            return Err(ErrorKind::AmbiguousTime.into());
        }

        if self.in_duration.is_none() && self.at_time.is_none() && self.on_date.is_none() {
            return Err(ErrorKind::MissingTime.into());
        }

        Ok(self)
    }

    fn add_string_by_state(self, state: ParseState, string: String) -> Result<Self, RemindError> {
        use self::ParseState::*;
        let string = Some(string);
        match state {
            On if self.on_date.is_none() => Ok(CommandParser {
                on_date: string,
                ..self
            }),
            At if self.at_time.is_none() => Ok(CommandParser {
                at_time: string,
                ..self
            }),
            In if self.in_duration.is_none() => Ok(CommandParser {
                in_duration: string,
                ..self
            }),
            Msg if self.message.is_none() => Ok(CommandParser {
                message: string,
                ..self
            }),
            Every if self.every_time.is_none() => Ok(CommandParser {
                every_time: string,
                ..self
            }),
            _ => Err(ErrorKind::MissingMessage.into()),
        }
    }

    fn parse_date(&self, str_date: &str) -> Result<NaiveDate, RemindError> {
        let nums = str_date
            .split('.')
            .map(|s| s.parse::<u32>())
            .collect::<Result<Vec<_>, _>>()
            .context(ErrorKind::InvalidDate)?;

        if 2 > nums.len() || nums.len() > 3 {
            return Err(ErrorKind::InvalidDate.into());
        }

        let day = nums[0];
        let month = nums[1];

        let parse_date = match nums.get(2) {
            Some(year) => {
                NaiveDate::from_ymd_opt(*year as i32, month, day).ok_or(ErrorKind::InvalidDate)?
            }
            None => {
                let now = time::now();
                let date = NaiveDate::from_ymd_opt(now.tm_year + 1900, month, day)
                    .ok_or(ErrorKind::InvalidDate)?;
                if date
                    .succ_opt()
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
                    .and_utc()
                    .timestamp()
                    < now.to_timespec().sec
                {
                    NaiveDate::from_ymd_opt(now.tm_year + 1901, month, day)
                        .ok_or(ErrorKind::InvalidDate)?
                } else {
                    date
                }
            }
        };

        Ok(parse_date)
    }

    fn parse_time(&self, str_time: &str) -> Result<NaiveTime, RemindError> {
        let nums = str_time
            .split(':')
            .map(|s| s.parse::<u32>())
            .collect::<Result<Vec<_>, _>>()
            .context(ErrorKind::InvalidTime)?;

        if 2 != nums.len() {
            return Err(ErrorKind::InvalidTime.into());
        }

        let hour = nums[0];
        let minute = nums[1];

        Ok(NaiveTime::from_hms_opt(hour, minute, 0).unwrap())
    }

    pub fn get_time(&self, min_dur: Duration) -> Result<NaiveDateTime, RemindError> {
        if let Some(ref str_duration) = self.in_duration {
            let duration = parse_duration(str_duration).context(ErrorKind::InvalidTime)?;

            if duration < min_dur {
                return Err(ErrorKind::TimeShort.into());
            }

            let tm = time::now().to_timespec();
            return Ok(
                DateTime::from_timestamp(tm.sec + duration.as_secs() as i64, 0u32)
                    .expect("fails after death of universe")
                    .naive_utc(),
            );
        }

        let mut date = None;
        if let Some(ref str_date) = self.on_date {
            date = Some(self.parse_date(str_date)?);
        }

        if let Some(ref str_time) = self.at_time {
            let time = self.parse_time(str_time)?;

            if let Some(date) = date {
                Ok(date.and_time(time))
            } else {
                let now = time::now();
                let today = NaiveDate::from_ymd_opt(
                    now.tm_year + 1900,
                    now.tm_mon as u32 + 1,
                    now.tm_mday as u32,
                )
                .ok_or(ErrorKind::InvalidDate)?;

                let time_today = today.and_time(time);

                if time_today.and_utc().timestamp() < now.to_timespec().sec {
                    debug!("tomorrow");

                    Ok(today.succ_opt().unwrap().and_time(time))
                } else {
                    debug!("today");

                    Ok(time_today)
                }
            }
        } else {
            Ok(date
                .expect("At this point date has to be set")
                .and_hms_opt(0, 0, 0)
                .unwrap())
        }
    }

    pub fn get_repeat(&self, min_dur: Duration) -> Result<Option<Duration>, RemindError> {
        if let Some(mut words) = self.every_time.clone() {
            if !words.chars().next().unwrap().is_ascii_digit() {
                words.insert(0, '1');
            }
            let dur = parse_duration(&words).context(ErrorKind::InvalidTime)?;

            if dur < min_dur {
                return Err(ErrorKind::RepeatTimeShort.into());
            }

            Ok(Some(dur))
        } else {
            Ok(None)
        }
    }

    pub fn get_target(&self) -> &str {
        &self.target
    }

    pub fn get_message(&self) -> &str {
        self.message.as_ref().expect("Has to be set")
    }
}
