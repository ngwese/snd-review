// SPDX-FileCopyrightText: 2026 Greg Wuller
// SPDX-License-Identifier: MIT

use mlua::{UserData, UserDataFields};

use crate::model::buffer::RegionId;
use crate::session::DocumentId;

use super::selection::channels_to_lua;
use super::with_document;

#[derive(Clone, Copy, Debug)]
pub struct LuaRegion {
    pub doc: DocumentId,
    pub id: RegionId,
}

impl UserData for LuaRegion {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("id", |_, this| Ok(this.id.0 as i64));
        fields.add_field_method_get("start", |lua, this| {
            with_document(lua, this.doc, |doc| {
                let region = doc
                    .buffer
                    .read()
                    .unwrap()
                    .region(this.id)
                    .cloned()
                    .ok_or_else(|| mlua::Error::runtime("region no longer exists"))?;
                Ok(region.start as i64)
            })
        });
        fields.add_field_method_get("stop", |lua, this| {
            with_document(lua, this.doc, |doc| {
                let region = doc
                    .buffer
                    .read()
                    .unwrap()
                    .region(this.id)
                    .cloned()
                    .ok_or_else(|| mlua::Error::runtime("region no longer exists"))?;
                Ok(region.end as i64)
            })
        });
        fields.add_field_method_get("channels", |lua, this| {
            with_document(lua, this.doc, |doc| {
                let region = doc
                    .buffer
                    .read()
                    .unwrap()
                    .region(this.id)
                    .cloned()
                    .ok_or_else(|| mlua::Error::runtime("region no longer exists"))?;
                channels_to_lua(lua, &region.channels)
            })
        });
        fields.add_field_method_get("label", |lua, this| {
            with_document(lua, this.doc, |doc| {
                let region = doc
                    .buffer
                    .read()
                    .unwrap()
                    .region(this.id)
                    .cloned()
                    .ok_or_else(|| mlua::Error::runtime("region no longer exists"))?;
                Ok(region.label)
            })
        });
    }
}
