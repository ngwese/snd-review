// SPDX-FileCopyrightText: 2026 Greg Wuller
// SPDX-License-Identifier: MIT

use mlua::{FromLua, Lua, Table, UserData, UserDataFields, UserDataMethods, Value};

use crate::model::buffer::{ChannelScope, RegionId};
use crate::model::document::BufferDocument;
use crate::session::DocumentId;

use super::region::LuaRegion;
use super::selection::{channels_from_lua, optional_i64, LuaSelection};
use super::{host_from_lua, with_document};

#[derive(Clone, Copy, Debug)]
pub struct LuaComposition {
    pub id: DocumentId,
}

impl FromLua for LuaComposition {
    fn from_lua(value: Value, _: &Lua) -> mlua::Result<Self> {
        match value {
            Value::UserData(data) => data.borrow::<Self>().map(|this| *this),
            _ => Err(mlua::Error::runtime("expected a composition")),
        }
    }
}

impl UserData for LuaComposition {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("name", |lua, this| {
            let host = host_from_lua(lua)?;
            host.display_name(this.id)
                .ok_or_else(|| mlua::Error::runtime("composition is not open"))
        });
        fields.add_field_method_get("path", |lua, this| {
            let host = host_from_lua(lua)?;
            Ok(host.path(this.id).map(|path| path.display().to_string()))
        });
        fields.add_field_method_get("frames", |lua, this| {
            with_document(lua, this.id, |doc| Ok(doc.frames() as i64))
        });
        fields.add_field_method_get("sample_rate", |lua, this| {
            with_document(lua, this.id, |doc| Ok(doc.sample_rate() as i64))
        });
        fields.add_field_method_get("channels", |lua, this| {
            with_document(lua, this.id, |doc| {
                Ok(doc.composition.read().unwrap().channel_count() as i64)
            })
        });
        fields.add_field_method_get("duration", |lua, this| {
            with_document(lua, this.id, |doc| {
                Ok(doc.composition.read().unwrap().duration_secs())
            })
        });
        fields.add_field_method_get("selection", |lua, this| {
            with_document(lua, this.id, |doc| {
                Ok(LuaSelection::from_selection(&doc.selection))
            })
        });
        fields.add_field_method_set("selection", |lua, this, value: Value| {
            with_document(lua, this.id, |doc| {
                apply_selection(lua, doc, value)?;
                Ok(())
            })?;
            after_edit(lua, this.id)
        });
        fields.add_field_method_get("position", |lua, this| {
            with_document(lua, this.id, |doc| {
                Ok(doc.current_position.as_ref().map(|pos| pos.sample as i64))
            })
        });
        fields.add_field_method_set("position", |lua, this, sample: i64| {
            with_document(lua, this.id, |doc| {
                let sample = sample.max(0) as usize;
                doc.set_position(sample, ChannelScope::all());
                Ok(())
            })?;
            after_edit(lua, this.id)
        });
        fields.add_field_method_get("regions", |lua, this| {
            with_document(lua, this.id, |doc| {
                let regions: Vec<LuaRegion> = doc
                    .buffer
                    .read()
                    .unwrap()
                    .regions
                    .iter()
                    .map(|region| LuaRegion {
                        doc: this.id,
                        id: region.id,
                    })
                    .collect();
                Ok(regions)
            })
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method(
            "select",
            |lua, this, (start, stop, channels): (i64, i64, Value)| {
                let channels = channels_from_lua(lua, channels)?;
                with_document(lua, this.id, |doc| {
                    doc.select_range(start.max(0) as usize, stop.max(0) as usize, channels);
                    Ok(())
                })?;
                after_edit(lua, this.id)
            },
        );
        methods.add_method("select_all", |lua, this, ()| {
            with_document(lua, this.id, |doc| {
                doc.select_all();
                Ok(())
            })?;
            after_edit(lua, this.id)
        });
        methods.add_method("clear_selection", |lua, this, ()| {
            with_document(lua, this.id, |doc| {
                doc.clear_selection();
                Ok(())
            })?;
            after_edit(lua, this.id)
        });
        methods.add_method("add_region", |lua, this, spec: Table| {
            let start: i64 = spec.get("start")?;
            let stop: i64 = spec.get("stop")?;
            let channels = channels_from_lua(lua, spec.get("channels")?)?;
            let label: Option<String> = spec.get("label")?;
            let id = with_document(lua, this.id, |doc| {
                Ok(doc.add_labeled_region(
                    start.max(0) as usize,
                    stop.max(0) as usize,
                    channels,
                    label,
                ))
            })?;
            after_edit(lua, this.id)?;
            Ok(LuaRegion { doc: this.id, id })
        });
        methods.add_method("remove_region", |lua, this, id: i64| {
            let removed = with_document(lua, this.id, |doc| {
                Ok(doc.remove_region(RegionId(id.max(0) as u64)))
            })?;
            after_edit(lua, this.id)?;
            Ok(removed)
        });
        methods.add_method("undo", |lua, this, ()| {
            edit(lua, this.id, |doc| doc.edit_undo())
        });
        methods.add_method("redo", |lua, this, ()| {
            edit(lua, this.id, |doc| doc.edit_redo())
        });
        methods.add_method("cut", |lua, this, ()| {
            edit(lua, this.id, |doc| {
                doc.edit_cut();
                true
            })
        });
        methods.add_method("copy", |lua, this, ()| {
            edit(lua, this.id, |doc| {
                doc.edit_copy();
                true
            })
        });
        methods.add_method("paste", |lua, this, ()| {
            edit(lua, this.id, |doc| {
                doc.edit_paste();
                true
            })
        });
        methods.add_method("delete", |lua, this, ()| {
            edit(lua, this.id, |doc| {
                doc.edit_delete();
                true
            })
        });
        methods.add_method("remove", |lua, this, ()| {
            edit(lua, this.id, |doc| {
                doc.edit_remove();
                true
            })
        });
        methods.add_method("duplicate", |lua, this, ()| {
            edit(lua, this.id, |doc| {
                doc.edit_duplicate();
                true
            })
        });
        methods.add_method("trim", |lua, this, ()| {
            edit(lua, this.id, |doc| {
                doc.edit_trim();
                true
            })
        });
        methods.add_method("roll", |lua, this, delta: i64| {
            edit(lua, this.id, |doc| {
                doc.edit_roll(delta);
                true
            })
        });
    }
}

fn edit<R>(lua: &Lua, id: DocumentId, f: impl FnOnce(&mut BufferDocument) -> R) -> mlua::Result<R> {
    let result = with_document(lua, id, |doc| Ok(f(doc)))?;
    after_edit(lua, id)?;
    Ok(result)
}

fn after_edit(lua: &Lua, id: DocumentId) -> mlua::Result<()> {
    host_from_lua(lua)?.after_edit(id)
}

fn apply_selection(lua: &Lua, doc: &mut BufferDocument, value: Value) -> mlua::Result<()> {
    match value {
        Value::Nil => {
            doc.clear_selection();
            Ok(())
        }
        Value::UserData(data) => {
            let selection = data.borrow::<LuaSelection>()?;
            apply_lua_selection(doc, &selection)
        }
        Value::Table(table) => {
            let kind: String = table.get("kind").unwrap_or_else(|_| "region".into());
            let start = optional_i64(table.get("start")?)?;
            let stop = optional_i64(table.get("stop")?)?;
            let channels = channels_from_lua(lua, table.get("channels")?)?;
            apply_lua_selection(
                doc,
                &LuaSelection {
                    kind,
                    start,
                    stop,
                    channels,
                },
            )
        }
        other => Err(mlua::Error::runtime(format!(
            "selection must be a table or userdata, got {}",
            other.type_name()
        ))),
    }
}

fn apply_lua_selection(doc: &mut BufferDocument, selection: &LuaSelection) -> mlua::Result<()> {
    match selection.kind.as_str() {
        "none" => {
            doc.clear_selection();
            Ok(())
        }
        "position" => {
            let sample = selection
                .start
                .ok_or_else(|| mlua::Error::runtime("position selection needs start"))?;
            doc.set_position(sample.max(0) as usize, selection.channels.clone());
            Ok(())
        }
        "region" => {
            let start = selection
                .start
                .ok_or_else(|| mlua::Error::runtime("region selection needs start"))?;
            let stop = selection.stop.unwrap_or(start);
            doc.select_range(
                start.max(0) as usize,
                stop.max(0) as usize,
                selection.channels.clone(),
            );
            Ok(())
        }
        kind => Err(mlua::Error::runtime(format!(
            "unknown selection kind {kind}"
        ))),
    }
}
