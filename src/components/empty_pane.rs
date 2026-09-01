// SPDX-FileCopyrightText: 2026 Greg Wuller
// SPDX-License-Identifier: MIT

use gpui::{
    div, App, Context, EventEmitter, FocusHandle, Focusable, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Window,
};
use gpui_component::{
    dock::{BasePanel, Panel, PanelEvent},
    ActiveTheme as _,
};

/// Empty dock pane used as a placeholder tab.
pub struct EmptyPane {
    name: &'static str,
    title: SharedString,
    message: Option<SharedString>,
    focus_handle: FocusHandle,
}

impl EmptyPane {
    pub fn new(name: &'static str, title: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        Self {
            name,
            title: title.into(),
            message: None,
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn with_message(mut self, message: impl Into<SharedString>) -> Self {
        self.message = Some(message.into());
        self
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        div()
            .id(self.name)
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .text_sm()
            .text_color(theme.muted_foreground)
            .children(self.message.clone())
    }
}
