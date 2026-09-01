// SPDX-FileCopyrightText: 2026 Greg Wuller
// SPDX-License-Identifier: MIT

use mlua::{Function, UserData, UserDataFields, UserDataMethods};

use super::composition::LuaComposition;
use super::host_from_lua;

pub struct LuaApp;

impl UserData for LuaApp {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("active", |lua, _| {
            let host = host_from_lua(lua)?;
            Ok(host.active().map(|id| LuaComposition { id }))
        });
        fields.add_field_method_get("documents", |lua, _| {
            let host = host_from_lua(lua)?;
            let docs: Vec<LuaComposition> = host
                .documents()
                .into_iter()
                .map(|id| LuaComposition { id })
                .collect();
            Ok(docs)
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("open", |lua, _, path: String| {
            let host = host_from_lua(lua)?;
            host.open(&path).map(|id| LuaComposition { id })
        });
        methods.add_method("command", |lua, _, id: String| {
            host_from_lua(lua)?
                .command(&id)
                .map_err(mlua::Error::runtime)
        });
        methods.add_method("dofile", |lua, _, path: String| {
            lua.load(std::path::Path::new(&path))
                .exec()
                .map_err(mlua::Error::runtime)
        });
        methods.add_method("on", |lua, _, (event, callback): (String, Function)| {
            if event != "loaded" {
                return Err(mlua::Error::runtime(format!(
                    "unknown event `{event}`; only \"loaded\" is supported"
                )));
            }
            host_from_lua(lua)?.on_loaded(callback);
            Ok(())
        });
    }
}

pub fn bind_app(lua: &mlua::Lua) -> mlua::Result<()> {
    lua.globals().set("app", LuaApp)
}
