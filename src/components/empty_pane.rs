// SPDX-FileCopyrightText: 2026 Greg Wuller
// SPDX-License-Identifier: MIT

use gpui::{
    div, App, Context, EventEmitter, FocusHandle, Focusable, InteractiveElement as _, IntoElement,
    Render, SharedString, Styled as _, Window,
};
use gpui_component::dock::{BasePanel, Panel, PanelEvent};

/// Empty right-dock pane used as a placeholder tab.
pub struct EmptyPane {
    name: &'static str,
    title: SharedString,
    focus_handle: FocusHandle,
}

impl EmptyPane {
    pub fn new(name: &'static str, title: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            name,
            title: title.into(),
            focus_handle: cx.focus_handle(),
        }
    }
}

impl EventEmitter<PanelEvent> for EmptyPane {}

impl Focusable for EmptyPane {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for EmptyPane {
    fn panel_name(&self) -> &'static str {
        self.name
    }

    fn closable(&self, _: &App) -> bool {
        false
    }

    fn zoomable(&self, _: &App) -> bool {
        false
    }
}

impl Panel for EmptyPane {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.title.clone()
    }

    fn inner_padding(&self, _: &App) -> bool {
        false
    }
}

impl Render for EmptyPane {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().id(self.name).size_full()
    }
}
