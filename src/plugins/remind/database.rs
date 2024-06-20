use std::fmt;

use chrono::NaiveDateTime;
use diesel::prelude::*;
use failure::ResultExt;

use super::error::*;
use crate::ConnectionPool;

static LAST_ID_SQL: &str = "SELECT LAST_INSERT_ID()";

#[derive(Queryable, Clone, Debug)]
pub struct Event {
    pub id: i64,
    pub receiver: String,
    pub content: String,
    pub author: String,
    pub time: NaiveDateTime,
    pub repeat: Option<i64>,
}

impl fmt::Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}: {} reminds {} to \"{}\" at {}",
            self.id, self.author, self.receiver, self.content, self.time
        )
    }
}

#[derive(Insertable, Debug)]
#[diesel(table_name = events)]
pub struct NewEvent<'a> {
    pub receiver: &'a str,
    pub content: &'a str,
    pub author: &'a str,
    pub time: &'a NaiveDateTime,
    pub repeat: Option<i64>,
}

pub trait Database: Send + Sync {
    fn insert_event(&self, event: &NewEvent) -> Result<i64, RemindError>;
    fn update_event_time(&self, id: i64, time: &NaiveDateTime) -> Result<(), RemindError>;
    fn get_events_before(&self, time: &NaiveDateTime) -> Result<Vec<Event>, RemindError>;
    fn get_user_events(&self, user: &str) -> Result<Vec<Event>, RemindError>;
    fn get_event(&self, id: i64) -> Result<Event, RemindError>;
    fn delete_event(&self, id: i64) -> Result<(), RemindError>;
}

mod schema {
    diesel::table! {
        events (id) {
            id -> Bigint,
            receiver -> Varchar,
            content -> Text,
            author -> Varchar,
            time -> Timestamp,
            repeat -> Nullable<Bigint>,
        }
    }
}

use self::schema::events;

impl Database for ConnectionPool {
    fn insert_event(&self, event: &NewEvent) -> Result<i64, RemindError> {
        use diesel::{dsl::sql, sql_types::Bigint};
        let mut conn = self.get().context(ErrorKind::NoConnection)?;

        diesel::insert_into(events::table)
            .values(event)
            .execute(&mut conn)
            .context(ErrorKind::MysqlError)?;

        let id = sql::<Bigint>(LAST_ID_SQL)
            .get_result(&mut conn)
            .context(ErrorKind::MysqlError)?;

        Ok(id)
    }

    fn update_event_time(&self, id: i64, time: &NaiveDateTime) -> Result<(), RemindError> {
        use self::events::columns;
        let mut conn = self.get().context(ErrorKind::NoConnection)?;

        match diesel::update(events::table.filter(columns::id.eq(id)))
            .set(columns::time.eq(time))
            .execute(&mut conn)
        {
            Ok(0) => Err(ErrorKind::NotFound)?,
            Ok(_) => Ok(()),
            Err(e) => Err(e).context(ErrorKind::MysqlError)?,
        }
    }

    fn get_events_before(&self, time: &NaiveDateTime) -> Result<Vec<Event>, RemindError> {
        use self::events::columns;
        let mut conn = self.get().context(ErrorKind::NoConnection)?;

        Ok(events::table
            .filter(columns::time.lt(time))
            .load::<Event>(&mut conn)
            .context(ErrorKind::MysqlError)?)
    }

    fn get_user_events(&self, user: &str) -> Result<Vec<Event>, RemindError> {
        use self::events::columns;
        let mut conn = self.get().context(ErrorKind::NoConnection)?;

        Ok(events::table
            .filter(columns::receiver.eq(user))
            .load::<Event>(&mut conn)
            .context(ErrorKind::MysqlError)?)
    }

    fn get_event(&self, id: i64) -> Result<Event, RemindError> {
        let mut conn = self.get().context(ErrorKind::NoConnection)?;

        Ok(events::table
            .find(id)
            .first(&mut conn)
            .context(ErrorKind::MysqlError)?)
    }

    fn delete_event(&self, id: i64) -> Result<(), RemindError> {
        use self::events::columns;

        let mut conn = self.get().context(ErrorKind::NoConnection)?;
        match diesel::delete(events::table.filter(columns::id.eq(id))).execute(&mut conn) {
            Ok(0) => Err(ErrorKind::NotFound)?,
            Ok(_) => Ok(()),
            Err(e) => Err(e).context(ErrorKind::MysqlError)?,
        }
    }
}
