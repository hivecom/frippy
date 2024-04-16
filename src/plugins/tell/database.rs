use diesel::mysql::MysqlConnection;
use diesel::prelude::*;
use diesel::r2d2::ConnectionManager;
use r2d2::Pool;

use chrono::NaiveDateTime;

use failure::ResultExt;

use super::error::*;

#[derive(Queryable, PartialEq, Clone, Debug)]
pub struct TellMessage {
    pub id: i64,
    pub sender: String,
    pub receiver: String,
    pub time: NaiveDateTime,
    pub message: String,
}

#[derive(Insertable)]
#[diesel(table_name = tells)]
pub struct NewTellMessage<'a> {
    pub sender: &'a str,
    pub receiver: &'a str,
    pub time: NaiveDateTime,
    pub message: &'a str,
}

pub trait Database: Send + Sync {
    fn insert_tell(&self, tell: &NewTellMessage) -> Result<(), TellError>;
    fn get_tells(&self, receiver: &str) -> Result<Vec<TellMessage>, TellError>;
    fn get_receivers(&self) -> Result<Vec<String>, TellError>;
    fn delete_tells(&self, receiver: &str) -> Result<(), TellError>;
}

// Diesel automatically defines the tells module as public.
// We create a schema module to keep it private.
mod schema {
    diesel::table! {
        tells (id) {
            id -> Bigint,
            sender -> Varchar,
            receiver -> Varchar,
            time -> Timestamp,
            message -> Varchar,
        }
    }
}

use self::schema::tells;

impl Database for Pool<ConnectionManager<MysqlConnection>> {
    fn insert_tell(&self, tell: &NewTellMessage) -> Result<(), TellError> {
        let mut conn = self.get().expect("Failed to get connection");
        diesel::insert_into(tells::table)
            .values(tell)
            .execute(&mut conn)
            .context(ErrorKind::MysqlError)?;

        Ok(())
    }

    fn get_tells(&self, receiver: &str) -> Result<Vec<TellMessage>, TellError> {
        use self::tells::columns;

        let mut conn = self.get().context(ErrorKind::NoConnection)?;
        let result = tells::table
            .filter(columns::receiver.eq(receiver))
            .order(columns::time.asc())
            .load::<TellMessage>(&mut conn)
            .context(ErrorKind::MysqlError)?;

        Ok(result)
    }

    fn get_receivers(&self) -> Result<Vec<String>, TellError> {
        use self::tells::columns;

        let mut conn = self.get().context(ErrorKind::NoConnection)?;
        let result = tells::table
            .select(columns::receiver)
            .load::<String>(&mut conn)
            .context(ErrorKind::MysqlError)?;

        Ok(result)
    }

    fn delete_tells(&self, receiver: &str) -> Result<(), TellError> {
        use self::tells::columns;

        let mut conn = self.get().context(ErrorKind::NoConnection)?;
        diesel::delete(tells::table.filter(columns::receiver.eq(receiver)))
            .execute(&mut conn)
            .context(ErrorKind::MysqlError)?;
        Ok(())
    }
}
