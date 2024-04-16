use diesel::prelude::*;
use failure::ResultExt;
use time::PrimitiveDateTime;

use crate::ConnectionPool;

use super::error::*;

#[derive(Clone, Debug, Queryable)]
pub struct Factoid {
    pub name: String,
    pub idx: i32,
    pub content: String,
    pub author: String,
    pub created: PrimitiveDateTime,
}

#[derive(Insertable)]
#[diesel(table_name = factoids)]
pub struct NewFactoid<'a> {
    pub name: &'a str,
    pub idx: i32,
    pub content: &'a str,
    pub author: &'a str,
    pub created: PrimitiveDateTime,
}

// Diesel automatically defines the factoids module as public.
// We create a schema module to keep it private.
mod schema {
    diesel::table! {
        factoids (name, idx) {
            name -> Varchar,
            idx -> Integer,
            content -> Text,
            author -> Varchar,
            created -> Timestamp,
        }
    }

    diesel::table! {
        lua_values (key) {
            key -> Varchar,
            value -> Varchar,
        }
    }
}

use self::schema::{factoids, lua_values};

pub fn insert_factoid(db: &ConnectionPool, factoid: &NewFactoid) -> Result<(), FactoidError> {
    let mut conn = db.get().context(ErrorKind::NoConnection)?;

    diesel::insert_into(factoids::table)
        .values(factoid)
        .execute(&mut conn)
        .context(ErrorKind::MysqlError)?;

    Ok(())
}

pub fn get_factoid(db: &ConnectionPool, name: &str, idx: i32) -> Result<Factoid, FactoidError> {
    let mut conn = db.get().context(ErrorKind::NoConnection)?;
    Ok(factoids::table
        .find((name, idx))
        .first(&mut conn)
        .context(ErrorKind::MysqlError)?)
}

pub fn delete_factoid(db: &ConnectionPool, name: &str, idx: i32) -> Result<(), FactoidError> {
    use self::factoids::columns;

    let mut conn = db.get().context(ErrorKind::NoConnection)?;
    match diesel::delete(
        factoids::table
            .filter(columns::name.eq(name))
            .filter(columns::idx.eq(idx)),
    )
    .execute(&mut conn)
    {
        Ok(v) => {
            if v > 0 {
                Ok(())
            } else {
                Err(ErrorKind::NotFound)?
            }
        }
        Err(e) => Err(e).context(ErrorKind::MysqlError)?,
    }
}

pub fn count_factoids(db: &ConnectionPool, name: &str) -> Result<i32, FactoidError> {
    let mut conn = db.get().context(ErrorKind::NoConnection)?;

    let count: i64 = factoids::table
        .filter(factoids::columns::name.eq(name))
        .count()
        .get_result(&mut conn)
        .optional()
        .context(ErrorKind::MysqlError)?
        .unwrap_or(0);

    Ok(count as i32)
}

pub fn get_lua_value(db: &ConnectionPool, key: String) -> Result<Option<String>, FactoidError> {
    let mut conn = db.get().context(ErrorKind::NoConnection)?;

    let value: Option<String> = lua_values::table
        .filter(lua_values::columns::key.eq(key))
        .select(lua_values::columns::value)
        .first(&mut conn)
        .optional()
        .context(ErrorKind::MysqlError)?;

    Ok(value)
}

pub fn set_lua_value(db: &ConnectionPool, key: String, value: String) -> Result<(), FactoidError> {
    let mut conn = db.get().context(ErrorKind::NoConnection)?;

    diesel::replace_into(lua_values::table)
        .values((
            lua_values::columns::key.eq(key),
            lua_values::columns::value.eq(value),
        ))
        .execute(&mut conn)
        .context(ErrorKind::MysqlError)?;

    Ok(())
}
