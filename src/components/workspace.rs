// SPDX-FileCopyrightText: 2026 Greg Wuller
// SPDX-License-Identifier: MIT

use gpui::{
    div, App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement,
    ParentElement as _, Render, Styled as _, Window,
};
use gpui_component::{
    dock::{BasePanel, Panel, PanelEvent},
    v_flex,
};

use crate::components::transport::Transport;
use crate::components::waveform::WaveformDisplay;
use crate::model::document::BufferDocument;
use crate::playback::TransportState;

pub struct WorkspacePanel {
    document: Entity<BufferDocument>,
    waveform: Entity<WaveformDisplay>,
    transport_state: TransportState,
    looping: bool,
    focus_handle: FocusHandle,
}

impl WorkspacePanel {
    pub fn new(
        document: Entity<BufferDocument>,
        waveform: Entity<WaveformDisplay>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&document, |_, _, cx| cx.notify()).detach();
        cx.observe(&waveform, |_, _, cx| cx.notify()).detach();
        Self {
            document,
            waveform,
            transport_state: TransportState::Stopped,
            looping: false,
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn sync_transport(&mut self, state: TransportState, looping: bool, cx: &mut Context<Self>) {
        if self.transport_state == state && self.looping == looping {
            return;
        }
        self.transport_state = state;
        self.looping = looping;
        cx.notify();
    }
}

impl EventEmitter<PanelEvent> for WorkspacePanel {}

impl Focusable for WorkspacePanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for WorkspacePanel {
    fn panel_name(&self) -> &'static str {
        "WorkspacePanel"
    }

    fn closable(&self, _: &App) -> bool {
        false
    }

    fn zoomable(&self, _: &App) -> bool {
        false
    }
}

impl Panel for WorkspacePanel {
    fn title(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.document
            .read(cx)
            .composition
            .read()
            .unwrap()
            .display_name()
    }

    fn tab_name(&self, cx: &App) -> Option<gpui::SharedString> {
        Some(
            self.document
                .read(cx)
                .composition
                .read()
                .unwrap()
                .display_name()
                .into(),
        )
    }

    fn inner_padding(&self, _: &App) -> bool {
        false
    }
}

impl Render for WorkspacePanel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(self.waveform.clone()),
            )
            .child(Transport::new(self.transport_state, self.looping))
    }
}
