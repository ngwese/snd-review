// SPDX-FileCopyrightText: 2026 Greg Wuller
// SPDX-License-Identifier: MIT

use mlua::{Lua, UserData, UserDataFields, Value};

use crate::model::buffer::ChannelScope;
use crate::model::selection::Selection;

#[derive(Clone, Debug)]
pub struct LuaSelection {
    pub kind: String,
    pub start: Option<i64>,
    pub stop: Option<i64>,
    pub channels: ChannelScope,
}

impl LuaSelection {
    pub fn from_selection(selection: &Selection) -> Self {
        match selection {
            Selection::None => Self {
                kind: "none".into(),
                start: None,
                stop: None,
                channels: ChannelScope::all(),
            },
            Selection::Position(pos) => Self {
                kind: "position".into(),
                start: Some(pos.sample as i64),
                stop: Some(pos.sample as i64),
                channels: pos.channels.clone(),
            },
            Selection::Region {
                start,
                end,
                channels,
                ..
            } => Self {
                kind: "region".into(),
                start: Some(*start as i64),
                stop: Some(*end as i64),
                channels: channels.clone(),
            },
        }
    }
}

impl UserData for LuaSelection {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("kind", |_, this| Ok(this.kind.clone()));
        fields.add_field_method_get("start", |_, this| Ok(this.start));
        fields.add_field_method_get("stop", |_, this| Ok(this.stop));
        fields.add_field_method_get("channels", |lua, this| channels_to_lua(lua, &this.channels));
    }
}

pub fn channels_from_lua(_lua: &Lua, value: Value) -> mlua::Result<ChannelScope> {
    match value {
        Value::Nil => Ok(ChannelScope::all()),
        Value::String(s) => {
            let text = s.to_str()?.to_string();
            if text == "all" {
                Ok(ChannelScope::all())
            } else {
                Err(mlua::Error::runtime(format!(
                    "channels must be \"all\" or an array, got {text:?}"
                )))
            }
        }
        Value::Table(table) => {
            let mut channels = Vec::new();
            for pair in table.sequence_values::<i64>() {
                let index = pair?;
                if index < 0 {
                    return Err(mlua::Error::runtime("channel indices must be non-negative"));
                }
                channels.push(index as usize);
            }
            if channels.is_empty() {
                Ok(ChannelScope::all())
            } else {
                Ok(ChannelScope::Channels(channels))
            }
        }
        other => Err(mlua::Error::runtime(format!(
            "channels must be \"all\" or an array, got {}",
            other.type_name()
        ))),
    }
}

pub fn channels_to_lua(lua: &Lua, scope: &ChannelScope) -> mlua::Result<Value> {
    match scope {
        ChannelScope::AllChannels => Ok(Value::String(lua.create_string("all")?)),
        ChannelScope::Channels(channels) => {
            let table = lua.create_table()?;
            for (i, channel) in channels.iter().enumerate() {
                table.set(i + 1, *channel as i64)?;
            }
            Ok(Value::Table(table))
        }
    }
}

pub fn optional_i64(value: Value) -> mlua::Result<Option<i64>> {
    match value {
        Value::Nil => Ok(None),
        Value::Integer(n) => Ok(Some(n)),
        Value::Number(n) => Ok(Some(n as i64)),
        other => Err(mlua::Error::runtime(format!(
            "expected integer, got {}",
            other.type_name()
        ))),
    }
}
