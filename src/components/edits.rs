// SPDX-FileCopyrightText: 2026 Greg Wuller
// SPDX-License-Identifier: MIT

use gpui::{
    div, px, App, Context, Entity, EventEmitter, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, StatefulInteractiveElement as _, Styled as _, Window,
};
use gpui_component::{
    dock::{BasePanel, Panel, PanelEvent},
    v_flex, ActiveTheme as _, StyledExt as _,
};

use crate::model::composition::EditOp;
use crate::model::document::BufferDocument;

pub struct EditsPanel {
    document: Entity<BufferDocument>,
    focus_handle: FocusHandle,
}

impl EditsPanel {
    pub fn new(document: Entity<BufferDocument>, cx: &mut Context<Self>) -> Self {
        cx.observe(&document, |_, _, cx| cx.notify()).detach();
        Self {
            document,
            focus_handle: cx.focus_handle(),
        }
    }
}

impl EventEmitter<PanelEvent> for EditsPanel {}

impl Focusable for EditsPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl BasePanel for EditsPanel {
    fn panel_name(&self) -> &'static str {
        "EditsPanel"
    }

    fn closable(&self, _: &App) -> bool {
        false
    }

    fn zoomable(&self, _: &App) -> bool {
        false
    }
}

impl Panel for EditsPanel {
    fn title(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        "Edits"
    }

    fn inner_padding(&self, _: &App) -> bool {
        false
    }
}

impl Render for EditsPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let doc = self.document.read(cx);
        let composition = doc.composition.read().unwrap();
        let sample_rate = composition.sample_rate();
        let current = composition.current_edit();
        let cards: Vec<_> = composition
            .edits()
            .iter()
            .rev()
            .map(|edit| {
                (
                    edit.id,
                    edit_title(&edit.op),
                    edit_detail(&edit.op, sample_rate),
                    edit.id == current,
                    edit.id.0 > current.0,
                )
            })
            .collect();
        drop(composition);

        v_flex()
            .id("edits-list")
            .size_full()
            .gap_1()
            .overflow_y_scroll()
            .children(
                cards
                    .into_iter()
                    .map(|(id, title, detail, current, future)| {
                        let document = self.document.clone();
                        let hover_document = document.clone();
                        v_flex()
                            .id(("edit-card", id.0))
                            .w_full()
                            .flex_none()
                            .gap_0()
                            .px_1p5()
                            .py_0p5()
                            .rounded(px(4.))
                            .border_1()
                            .border_color(if current { theme.accent } else { theme.border })
                            .bg(if current {
                                theme.accent.opacity(0.25)
                            } else {
                                theme.secondary
                            })
                            .opacity(if future { 0.55 } else { 1.0 })
                            .cursor_pointer()
                            .hover(|this| this.bg(theme.secondary_hover))
                            .on_hover(cx.listener(move |_, hovered: &bool, _, cx| {
                                hover_document.update(cx, |doc, cx| {
                                    doc.set_hovered_edit(if *hovered { Some(id) } else { None });
                                    cx.notify();
                                });
                            }))
                            .on_click(cx.listener(move |_, _, _, cx| {
                                document.update(cx, |doc, cx| {
                                    doc.jump_to_edit(id);
                                    cx.notify();
                                });
                            }))
                            .child(
                                div()
                                    .text_xs()
                                    .font_semibold()
                                    .text_color(theme.foreground)
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(detail),
                            )
                    }),
            )
    }
}

pub fn edit_title(op: &EditOp) -> &'static str {
    match op {
        EditOp::Init => "Initial",
        EditOp::Cut { .. } => "Cut",
        EditOp::Copy { .. } => "Copy",
        EditOp::Paste { .. } => "Paste",
        EditOp::Remove { .. } => "Remove",
        EditOp::Delete { .. } => "Delete",
        EditOp::Trim { .. } => "Trim",
        EditOp::Move { .. } => "Move",
        EditOp::Duplicate { .. } => "Duplicate",
        EditOp::Roll { .. } => "Roll",
    }
}

pub fn edit_detail(op: &EditOp, sample_rate: u32) -> String {
    match op {
        EditOp::Init => "start of composition".into(),
        EditOp::Cut { start, len }
        | EditOp::Copy { start, len }
        | EditOp::Remove { start, len }
        | EditOp::Delete { start, len }
        | EditOp::Trim { start, len }
        | EditOp::Duplicate { start, len } => {
            format!(
                "{} · {}",
                format_stamp(*start, sample_rate),
                format_len(*len)
            )
        }
        EditOp::Paste { at, len } => {
            format!(
                "at {} · {}",
                format_stamp(*at, sample_rate),
                format_len(*len)
            )
        }
        EditOp::Move { from, len, dest } => format!(
            "{} → {} · {}",
            format_stamp(*from, sample_rate),
            format_stamp(*dest, sample_rate),
            format_len(*len)
        ),
        EditOp::Roll { at, delta } => {
            format!("at {} · {delta:+} smp", format_stamp(*at, sample_rate))
        }
    }
}

fn format_stamp(frame: u64, sample_rate: u32) -> String {
    let secs = frame as f64 / f64::from(sample_rate.max(1));
    format!("{secs:.2}s")
}

fn format_len(len: u64) -> String {
    format!("{len} smp")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_two_line_edit_cards() {
        assert_eq!(edit_title(&EditOp::Init), "Initial");
        assert_eq!(edit_detail(&EditOp::Init, 44100), "start of composition");
        assert_eq!(
            edit_detail(
                &EditOp::Cut {
                    start: 44100,
                    len: 22050
                },
                44100
            ),
            "1.00s · 22050 smp"
        );
        assert_eq!(
            edit_detail(
                &EditOp::Move {
                    from: 0,
                    len: 100,
                    dest: 44100
                },
                44100
            ),
            "0.00s → 1.00s · 100 smp"
        );
        assert_eq!(
            edit_detail(
                &EditOp::Roll {
                    at: 4410,
                    delta: -12
                },
                44100
            ),
            "at 0.10s · -12 smp"
        );
    }
}
