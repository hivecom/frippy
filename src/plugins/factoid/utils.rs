use std::collections::HashMap;
use std::str::FromStr;
use std::thread;
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::{self, Value as SerdeValue};

use mlua::Error as LuaError;
use mlua::Error::RuntimeError;
use mlua::{Lua, Value as LuaValue};

use crate::utils::error::ErrorKind::Connection;
use crate::utils::Url;
use crate::ConnectionPool;

use failure::Fail;

use super::database::set_lua_value;

pub fn sleep(_: &Lua, dur: u64) -> Result<(), LuaError> {
    thread::sleep(Duration::from_millis(dur));
    Ok(())
}

pub fn persist(db: &ConnectionPool, key: String, value: String) -> Result<(), LuaError> {
    if key.len() > 32 {
        return Err(LuaError::external("Key can't be longer than 32 characters"));
    }
    if value.len() > 5120 {
        return Err(LuaError::external(
            "Value can't be longer than 5120 characters",
        ));
    }

    set_lua_value(db, key, value).map_err(|e| LuaError::external(e.to_string()))?;

    Ok(())
}

pub fn download(url: String, headers: Option<HashMap<String, String>>) -> Result<String, LuaError> {
    let mut url = Url::from(url).max_kib(1024);

    if let Some(headers) = headers {
        let mut header_map = HeaderMap::new();

        for (key, value) in headers {
            let parsed_key = HeaderName::from_str(&key).map_err(LuaError::external)?;
            let parsed_value = HeaderValue::from_str(&value).map_err(LuaError::external)?;
            header_map.insert(parsed_key, parsed_value);
        }

        url = url.headers(header_map);
    }

    match url.request() {
        Ok(v) => Ok(v),
        Err(e) => {
            let error = match e.kind() {
                Connection => e.cause().unwrap().to_string(),
                _ => e.to_string(),
            };

            Err(RuntimeError(format!(
                "Failed to download {} - {}",
                url.as_str(),
                error
            )))
        }
    }
}

fn convert_serde_value(
    lua: &Lua,
    sval: SerdeValue,
    max_recurs: usize,
) -> Result<LuaValue, LuaError> {
    if max_recurs == 0 {
        return Err(RuntimeError(String::from(
            "Reached max recursion level - json is nested too deep",
        )));
    }

    let lval = match sval {
        SerdeValue::Null => LuaValue::Nil,
        SerdeValue::Bool(b) => LuaValue::Boolean(b),
        SerdeValue::String(s) => LuaValue::String(lua.create_string(&s)?),
        SerdeValue::Number(n) => {
            if n.is_f64() {
                let f = n.as_f64().ok_or_else(|| {
                    RuntimeError(String::from("Failed to convert number into double"))
                })?;

                LuaValue::Number(f)
            } else {
                let i = n.as_i64().ok_or_else(|| {
                    RuntimeError(String::from("Failed to convert number into integer"))
                })?;

                LuaValue::Integer(i)
            }
        }
        SerdeValue::Array(arr) => {
            let table = lua.create_table()?;
            for (i, val) in arr.into_iter().enumerate() {
                table.set(i + 1, convert_serde_value(lua, val, max_recurs - 1)?)?;
            }

            LuaValue::Table(table)
        }
        SerdeValue::Object(obj) => {
            let table = lua.create_table()?;
            for (key, val) in obj {
                table.set(key, convert_serde_value(lua, val, max_recurs - 1)?)?;
            }

            LuaValue::Table(table)
        }
    };

    Ok(lval)
}

pub fn json_decode(lua: &Lua, json: String) -> Result<LuaValue, LuaError> {
    let ser_val: SerdeValue =
        serde_json::from_str(&json).map_err(|e| RuntimeError(e.to_string()))?;

    convert_serde_value(lua, ser_val, 25)
}

fn convert_lua_value(lua: &Lua, lval: LuaValue, max_recurs: usize) -> Result<SerdeValue, LuaError> {
    if max_recurs == 0 {
        return Err(RuntimeError(String::from(
            "Reached max recursion level - table is nested too deep",
        )));
    }

    let sval = match lval {
        LuaValue::Nil
        | LuaValue::Thread(_)
        | LuaValue::Function(_)
        | LuaValue::UserData(_)
        | LuaValue::LightUserData(_) => SerdeValue::Null,
        LuaValue::Error(e) => SerdeValue::String(e.to_string()),
        LuaValue::Boolean(b) => SerdeValue::Bool(b),
        LuaValue::String(s) => SerdeValue::String(s.to_string_lossy().to_string()),
        LuaValue::Integer(i) => SerdeValue::Number(serde_json::value::Number::from(i)),
        LuaValue::Number(n) => match serde_json::value::Number::from_f64(n) {
            Some(n) => SerdeValue::Number(n),
            None => SerdeValue::Null,
        },
        LuaValue::Table(t) => {
            let mut map = serde_json::Map::new();
            for pair in t.pairs::<LuaValue, LuaValue>() {
                let (k, v) = pair?;
                map.insert(k.to_string()?, convert_lua_value(lua, v, max_recurs - 1)?);
            }

            SerdeValue::Object(map)
        }
    };

    Ok(sval)
}

pub fn json_encode(lua: &Lua, lua_value: LuaValue) -> Result<String, LuaError> {
    convert_lua_value(lua, lua_value, 25)
        .map(|v| v.to_string())
        .map_err(|e| RuntimeError(e.to_string()))
}
