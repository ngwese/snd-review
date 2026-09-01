// SPDX-FileCopyrightText: 2026 Greg Wuller
// SPDX-License-Identifier: MIT

mod access;
mod app;
mod composition;
mod host;
mod region;
mod selection;

pub use access::{enter, try_invoke_command};
pub use host::{host_from_lua, with_document, EvalOutput, ScriptHost, TestWorld};

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use crate::model::composition::Composition;
    use crate::model::Buffer;

    use super::*;

    fn test_host() -> (ScriptHost, Rc<RefCell<TestWorld>>) {
        let world = Rc::new(RefCell::new(TestWorld::new()));
        let composition = Composition::new(44100, 2);
        world
            .borrow_mut()
            .push(composition, Buffer::empty(), "fixture", None);
        let host = ScriptHost::for_test(world.clone()).expect("lua");
        (host, world)
    }

    #[test]
    fn eval_returns_expression_results() {
        let (mut host, _) = test_host();
        let out = host.eval("1 + 2");
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(out.result.as_deref(), Some("3"));
    }

    #[test]
    fn print_is_captured() {
        let (mut host, _) = test_host();
        let out = host.eval(r#"print("hello")"#);
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(out.prints, vec!["hello".to_string()]);
    }

    #[test]
    fn unknown_command_is_an_error() {
        let (mut host, _) = test_host();
        let out = host.eval(r#"app:command("not.a.command")"#);
        assert!(
            out.error
                .as_deref()
                .is_some_and(|err| err.contains("unknown command")),
            "{:?}",
            out.error
        );
    }

    #[test]
    fn selection_and_named_region_round_trip() {
        let (mut host, world) = test_host();
        let out = host.eval(
            r#"
            local c = app.active
            c:select(0, 100)
            local region = c:add_region({ start = 10, stop = 20, label = "intro" })
            return c.selection.kind, c.selection.start, region.label, region.start, region.stop
            "#,
        );
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(out.result.as_deref(), Some("region\t0\tintro\t10\t20"));
        let world = world.borrow();
        let id = world.active.unwrap();
        let doc = world.docs.get(&id).unwrap();
        assert_eq!(
            doc.buffer.read().unwrap().regions[0].label.as_deref(),
            Some("intro"),
        );
    }

    #[test]
    fn known_command_id_is_accepted() {
        let (mut host, _) = test_host();
        let out = host.eval(r#"app:command("edit.trim")"#);
        assert!(out.error.is_none(), "{:?}", out.error);
    }
}
