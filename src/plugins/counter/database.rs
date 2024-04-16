use diesel::prelude::*;
use failure::ResultExt;

use crate::ConnectionPool;

use super::error::*;

pub trait Database: Send + Sync {
    fn add(&self, name: &str) -> Result<i64, CounterError>;
    fn subtract(&self, name: &str) -> Result<i64, CounterError>;
    fn get_count(&self, name: &str) -> Result<i64, CounterError>;
}

// Diesel automatically defines the counts module as public.
// We create a schema module to keep it private.
mod schema {
    diesel::table! {
        counts (name) {
            name -> Varchar,
            count -> Bigint,
        }
    }
}

use self::schema::counts;

impl Database for ConnectionPool {
    fn add(&self, name: &str) -> Result<i64, CounterError> {
        let mut conn = self.get().context(ErrorKind::NoConnection)?;
        match counts::table
            .find(name)
            .select(counts::columns::count)
            .first(&mut conn)
        {
            Ok(mut count) => {
                count += 1;
                diesel::update(counts::table.filter(counts::columns::name.eq(name)))
                    .set(counts::columns::count.eq(count))
                    .execute(&mut conn)
                    .context(ErrorKind::MysqlError)?;

                Ok(count)
            }
            Err(e) => match e {
                diesel::result::Error::NotFound => {
                    diesel::insert_into(counts::table)
                        .values((counts::columns::name.eq(name), counts::columns::count.eq(1)))
                        .execute(&mut conn)
                        .context(ErrorKind::MysqlError)?;

                    Ok(1)
                }
                _ => Err(e).context(ErrorKind::MysqlError)?,
            },
        }
    }

    fn subtract(&self, name: &str) -> Result<i64, CounterError> {
        let mut conn = self.get().context(ErrorKind::NoConnection)?;
        match counts::table
            .find(name)
            .select(counts::columns::count)
            .first(&mut conn)
        {
            Ok(mut count) => {
                count -= 1;
                diesel::update(counts::table.filter(counts::columns::name.eq(name)))
                    .set(counts::columns::count.eq(count))
                    .execute(&mut conn)
                    .context(ErrorKind::MysqlError)?;

                Ok(count)
            }
            Err(e) => match e {
                diesel::result::Error::NotFound => {
                    diesel::insert_into(counts::table)
                        .values((
                            counts::columns::name.eq(name),
                            counts::columns::count.eq(-1),
                        ))
                        .execute(&mut conn)
                        .context(ErrorKind::MysqlError)?;

                    Ok(-1)
                }
                _ => Err(e).context(ErrorKind::MysqlError)?,
            },
        }
    }

    fn get_count(&self, name: &str) -> Result<i64, CounterError> {
        let mut conn = self.get().context(ErrorKind::NoConnection)?;

        match counts::table
            .find(name)
            .select(counts::columns::count)
            .first(&mut conn)
        {
            Ok(count) => Ok(count),
            Err(e) => match e {
                diesel::result::Error::NotFound => Ok(0),
                _ => Err(e).context(ErrorKind::MysqlError)?,
            },
        }
    }
}
