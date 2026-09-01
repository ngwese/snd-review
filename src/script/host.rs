// SPDX-FileCopyrightText: 2026 Greg Wuller
// SPDX-License-Identifier: MIT

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, RwLock};

use mlua::{Function, Lua, MultiValue, Value};

use crate::model::composition::Composition;
use crate::model::document::BufferDocument;
use crate::model::Buffer;
use crate::session::DocumentId;

use super::access;
use super::app::bind_app;
use super::composition::LuaComposition;

#[derive(Clone, Debug)]
pub struct EvalOutput {
    pub prints: Vec<String>,
    pub result: Option<String>,
    pub error: Option<String>,
}

struct HostInner {
    prints: Vec<String>,
    loaded: Vec<Function>,
    test: Option<Rc<RefCell<TestWorld>>>,
}

#[derive(Clone)]
pub struct HostHandle {
    inner: Rc<RefCell<HostInner>>,
}

pub struct ScriptHost {
    lua: Lua,
    handle: HostHandle,
}

pub struct TestWorld {
    pub docs: HashMap<DocumentId, BufferDocument>,
    pub paths: HashMap<DocumentId, Option<PathBuf>>,
    pub names: HashMap<DocumentId, String>,
    pub active: Option<DocumentId>,
    next_id: u64,
}

impl TestWorld {
    pub fn new() -> Self {
        Self {
            docs: HashMap::new(),
            paths: HashMap::new(),
            names: HashMap::new(),
            active: None,
            next_id: 0,
        }
    }

    pub fn push(
        &mut self,
        composition: Composition,
        buffer: Buffer,
        name: impl Into<String>,
        path: Option<PathBuf>,
    ) -> DocumentId {
        self.next_id += 1;
        let id = DocumentId(self.next_id);
        let composition = Arc::new(RwLock::new(composition));
        let buffer = Arc::new(RwLock::new(buffer));
        let document = BufferDocument::with_shared(composition, buffer);
        self.docs.insert(id, document);
        self.paths.insert(id, path);
        self.names.insert(id, name.into());
        self.active = Some(id);
        id
    }
}

impl ScriptHost {
    pub fn new() -> mlua::Result<Self> {
        Self::with_test(None)
    }

    pub fn for_test(world: Rc<RefCell<TestWorld>>) -> mlua::Result<Self> {
        Self::with_test(Some(world))
    }

    fn with_test(test: Option<Rc<RefCell<TestWorld>>>) -> mlua::Result<Self> {
        let lua = Lua::new();
        let handle = HostHandle {
            inner: Rc::new(RefCell::new(HostInner {
                prints: Vec::new(),
                loaded: Vec::new(),
                test,
            })),
        };
        lua.set_app_data(handle.clone());
        bind_app(&lua)?;
        install_print(&lua, handle.clone())?;
        Ok(Self { lua, handle })
    }

    pub fn eval(&mut self, code: &str) -> EvalOutput {
        self.handle.inner.borrow_mut().prints.clear();
        let result = eval_repl(&self.lua, code);
        let prints = std::mem::take(&mut self.handle.inner.borrow_mut().prints);
        match result {
            Ok(values) => EvalOutput {
                prints,
                result: stringify_values(&self.lua, values),
                error: None,
            },
            Err(err) => EvalOutput {
                prints,
                result: None,
                error: Some(err.to_string()),
            },
        }
    }

    pub fn load_init(&mut self) -> Result<(), String> {
        let Some(dir) = crate::commands::user_config_dir() else {
            return Ok(());
        };
        let path = dir.join("init.lua");
        if !path.is_file() {
            return Ok(());
        }
        self.lua
            .load(path)
            .exec()
            .map_err(|err| format!("init.lua: {err}"))
    }

    pub fn fire_loaded(&self, id: DocumentId) {
        let hooks = self.handle.inner.borrow().loaded.clone();
        let handle = LuaComposition { id };
        for hook in hooks {
            if let Err(err) = hook.call::<()>(handle) {
                self.handle
                    .inner
                    .borrow_mut()
                    .prints
                    .push(format!("loaded hook error: {err}"));
            }
        }
    }

    pub fn take_prints(&self) -> Vec<String> {
        std::mem::take(&mut self.handle.inner.borrow_mut().prints)
    }
}

impl HostHandle {
    pub fn active(&self) -> Option<DocumentId> {
        if let Some(test) = &self.inner.borrow().test {
            return test.borrow().active;
        }
        access::with_view(|view, _, _| view.session_active())
            .ok()
            .flatten()
    }

    pub fn documents(&self) -> Vec<DocumentId> {
        if let Some(test) = &self.inner.borrow().test {
            return test.borrow().docs.keys().copied().collect();
        }
        access::with_view(|view, _, _| view.session_document_ids()).unwrap_or_default()
    }

    pub fn display_name(&self, id: DocumentId) -> Option<String> {
        if let Some(test) = &self.inner.borrow().test {
            return test.borrow().names.get(&id).cloned();
        }
        access::with_view(|view, _, cx| view.script_display_name(id, cx)).ok()?
    }

    pub fn path(&self, id: DocumentId) -> Option<PathBuf> {
        if let Some(test) = &self.inner.borrow().test {
            return test.borrow().paths.get(&id).cloned().flatten();
        }
        access::with_view(|view, _, _| view.script_path(id))
            .ok()
            .flatten()
    }

    pub fn open(&self, path: &str) -> mlua::Result<DocumentId> {
        if self.inner.borrow().test.is_some() {
            return Err(mlua::Error::runtime(
                "app:open is not available in script tests",
            ));
        }
        let path = PathBuf::from(path);
        access::with_view(|view, window, cx| view.script_open(path, window, cx))
            .map_err(mlua::Error::runtime)?
            .map_err(mlua::Error::runtime)
    }

    pub fn command(&self, id: &str) -> Result<(), String> {
        crate::commands::validate_command_id(id)?;
        if self.inner.borrow().test.is_some() {
            return Ok(());
        }
        match access::with_view(|view, window, cx| view.invoke_command(id, window, cx)) {
            Ok(result) => result,
            Err(err) => Err(err),
        }
    }

    pub fn on_loaded(&self, callback: Function) {
        self.inner.borrow_mut().loaded.push(callback);
    }

    pub fn after_edit(&self, id: DocumentId) -> mlua::Result<()> {
        if self.inner.borrow().test.is_some() {
            return Ok(());
        }
        access::with_view(|view, window, cx| view.after_script_edit(id, window, cx))
            .map_err(mlua::Error::runtime)
    }

    pub fn with_document<R>(
        &self,
        id: DocumentId,
        f: impl FnOnce(&mut BufferDocument) -> mlua::Result<R>,
    ) -> mlua::Result<R> {
        if let Some(test) = &self.inner.borrow().test {
            let mut world = test.borrow_mut();
            let doc = world
                .docs
                .get_mut(&id)
                .ok_or_else(|| mlua::Error::runtime("composition is not open"))?;
            return f(doc);
        }
        access::with_view(|view, _, cx| view.script_with_document(id, cx, f))
            .map_err(mlua::Error::runtime)?
    }
}

pub fn host_from_lua(lua: &Lua) -> mlua::Result<HostHandle> {
    lua.app_data_ref::<HostHandle>()
        .map(|handle| handle.clone())
        .ok_or_else(|| mlua::Error::runtime("script host is not bound"))
}

pub fn with_document<R>(
    lua: &Lua,
    id: DocumentId,
    f: impl FnOnce(&mut BufferDocument) -> mlua::Result<R>,
) -> mlua::Result<R> {
    host_from_lua(lua)?.with_document(id, f)
}

fn install_print(lua: &Lua, handle: HostHandle) -> mlua::Result<()> {
    let print = lua.create_function(move |lua, args: MultiValue| {
        let text = stringify_values(lua, args).unwrap_or_default();
        handle.inner.borrow_mut().prints.push(text);
        Ok(())
    })?;
    lua.globals().set("print", print)?;
    Ok(())
}

fn eval_repl(lua: &Lua, code: &str) -> mlua::Result<MultiValue> {
    let trimmed = code.trim();
    if trimmed.is_empty() {
        return Ok(MultiValue::new());
    }
    match lua
        .load(&format!("return {trimmed}"))
        .set_name("=repl")
        .eval::<MultiValue>()
    {
        Ok(values) => Ok(values),
        Err(_) => lua.load(trimmed).set_name("=repl").eval::<MultiValue>(),
    }
}

fn stringify_values(lua: &Lua, values: MultiValue) -> Option<String> {
    if values.is_empty() {
        return None;
    }
    let tostring: Function = match lua.globals().get("tostring") {
        Ok(f) => f,
        Err(_) => return None,
    };
    let mut parts = Vec::new();
    for value in values {
        if matches!(value, Value::Nil) && parts.is_empty() {
            continue;
        }
        match tostring.call::<String>(value) {
            Ok(text) => parts.push(text),
            Err(_) => parts.push("<unprintable>".into()),
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\t"))
    }
}
