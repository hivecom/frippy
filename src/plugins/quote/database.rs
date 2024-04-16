use diesel::mysql::MysqlConnection;
use diesel::prelude::*;
use diesel::r2d2::ConnectionManager;
use failure::ResultExt;
use r2d2::Pool;

use chrono::NaiveDateTime;

use super::error::*;

#[derive(Queryable, Clone, Debug)]
pub struct Quote {
    pub quotee: String,
    pub channel: String,
    pub idx: i32,
    pub content: String,
    pub author: String,
    pub created: NaiveDateTime,
}

#[derive(Insertable)]
#[diesel(table_name = quotes)]
pub struct NewQuote<'a> {
    pub quotee: &'a str,
    pub channel: &'a str,
    pub idx: i32,
    pub content: &'a str,
    pub author: &'a str,
    pub created: NaiveDateTime,
}

pub trait Database: Send + Sync {
    fn insert_quote(&self, quote: &NewQuote) -> Result<(), QuoteError>;
    fn get_user_quote(&self, quotee: &str, channel: &str, idx: i32) -> Result<Quote, QuoteError>;
    fn get_channel_quote(&self, channel: &str, idx: i32) -> Result<Quote, QuoteError>;
    fn count_user_quotes(&self, quotee: &str, channel: &str) -> Result<i32, QuoteError>;
    fn count_channel_quotes(&self, channel: &str) -> Result<i32, QuoteError>;
    fn search_user_quote(
        &self,
        query: &str,
        quotee: &str,
        channel: &str,
        offset: i32,
    ) -> Result<Quote, QuoteError>;

    fn search_channel_quote(
        &self,
        query: &str,
        channel: &str,
        offset: i32,
    ) -> Result<Quote, QuoteError>;
}

// Diesel automatically defines the quotes module as public.
// We create a schema module to keep it private.
mod schema {
    diesel::table! {
        quotes (quotee, channel, idx) {
            quotee -> Varchar,
            channel -> Varchar,
            idx -> Integer,
            content -> Text,
            author -> Varchar,
            created -> Timestamp,
        }
    }
}

use self::schema::quotes;

impl Database for Pool<ConnectionManager<MysqlConnection>> {
    fn insert_quote(&self, quote: &NewQuote) -> Result<(), QuoteError> {
        let mut conn = self.get().context(ErrorKind::NoConnection)?;
        diesel::insert_into(quotes::table)
            .values(quote)
            .execute(&mut conn)
            .context(ErrorKind::MysqlError)?;

        Ok(())
    }

    fn get_user_quote(&self, quotee: &str, channel: &str, idx: i32) -> Result<Quote, QuoteError> {
        let mut conn = self.get().context(ErrorKind::NoConnection)?;
        let quote = quotes::table
            .find((quotee, channel, idx))
            .first(&mut conn)
            .context(ErrorKind::MysqlError)?;

        Ok(quote)
    }

    fn get_channel_quote(&self, channel: &str, idx: i32) -> Result<Quote, QuoteError> {
        let mut conn = self.get().context(ErrorKind::NoConnection)?;
        let quote = quotes::table
            .filter(quotes::columns::channel.eq(channel))
            .offset(idx as i64 - 1)
            .first(&mut conn)
            .context(ErrorKind::MysqlError)?;

        Ok(quote)
    }

    fn count_user_quotes(&self, quotee: &str, channel: &str) -> Result<i32, QuoteError> {
        let mut conn = self.get().context(ErrorKind::NoConnection)?;
        let count: Result<i64, _> = quotes::table
            .filter(quotes::columns::quotee.eq(quotee))
            .filter(quotes::columns::channel.eq(channel))
            .count()
            .get_result(&mut conn);

        match count {
            Ok(c) => Ok(c as i32),
            Err(diesel::NotFound) => Ok(0),
            Err(e) => Err(e).context(ErrorKind::MysqlError)?,
        }
    }

    fn count_channel_quotes(&self, channel: &str) -> Result<i32, QuoteError> {
        let mut conn = self.get().context(ErrorKind::NoConnection)?;
        let count: Result<i64, _> = quotes::table
            .filter(quotes::columns::channel.eq(channel))
            .count()
            .get_result(&mut conn);

        match count {
            Ok(c) => Ok(c as i32),
            Err(diesel::NotFound) => Ok(0),
            Err(e) => Err(e).context(ErrorKind::MysqlError)?,
        }
    }

    fn search_user_quote(
        &self,
        query: &str,
        quotee: &str,
        channel: &str,
        offset: i32,
    ) -> Result<Quote, QuoteError> {
        let mut conn = self.get().context(ErrorKind::NoConnection)?;
        let quote = quotes::table
            .filter(quotes::columns::channel.eq(channel))
            .filter(quotes::columns::quotee.eq(quotee))
            .filter(quotes::columns::content.like(&format!("%{}%", query)))
            .offset(offset as i64)
            .first(&mut conn)
            .context(ErrorKind::MysqlError)?;

        Ok(quote)
    }

    fn search_channel_quote(
        &self,
        query: &str,
        channel: &str,
        offset: i32,
    ) -> Result<Quote, QuoteError> {
        let mut conn = self.get().context(ErrorKind::NoConnection)?;
        let quote = quotes::table
            .filter(quotes::columns::channel.eq(channel))
            .filter(quotes::columns::content.like(&format!("%{}%", query)))
            .offset(offset as i64)
            .first(&mut conn)
            .context(ErrorKind::MysqlError)?;

        Ok(quote)
    }
}
