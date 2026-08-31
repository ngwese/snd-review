// SPDX-FileCopyrightText: 2026 Greg Wuller
// SPDX-License-Identifier: MIT

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use cpal::Device;
use gpui::{
    div, point, prelude::FluentBuilder as _, px, size, App, AppContext as _, Bounds, Context,
    Entity, ExternalPaths, FocusHandle, Focusable, Global, InteractiveElement as _, IntoElement,
    Menu, MenuItem, ParentElement as _, PathPromptOptions, Render, SharedString, Styled as _,
    TitlebarOptions, Window, WindowBounds, WindowOptions,
};
use gpui_component::{
    h_flex, menu::AppMenuBar, v_flex, ActiveTheme as _, GlobalState, Root, Theme, ThemeMode,
    WindowExt as _, TITLE_BAR_HEIGHT,
};

use crate::assets::AppAssets;
use crate::commands::{
    install_keybindings, About, EditCopy, EditCut, EditDelete, EditDuplicate, EditPaste, EditRedo,
    EditRemove, EditRollLeft, EditRollRight, EditTrim, EditUndo, Open, Quit, TransportEnd,
    TransportHome, TransportLoop, TransportNext, TransportPlayPause, TransportPrevious,
    TransportStart, TransportStop, ViewFitAll, ViewFrame, ViewZoomIn, ViewZoomOut,
};
use crate::components::status_bar::{FileStatus, FileStatusBar};
use crate::components::transport::Transport;
use crate::components::waveform::{ToggleZeroCrossing, WaveformDisplay};
use crate::model::composition::Composition;
use crate::model::selection::Selection;
use crate::model::{Buffer, BufferDocument};
use crate::playback::{PlaybackSession, TransportState};

struct OpenTarget(Entity<AppView>);

impl Global for OpenTarget {}

pub struct AppView {
    composition: Arc<RwLock<Composition>>,
    buffer: Arc<RwLock<Buffer>>,
    device: Device,
    document: Entity<BufferDocument>,
    waveform: Entity<WaveformDisplay>,
    playback: PlaybackSession,
    app_menu_bar: Option<Entity<AppMenuBar>>,
    pending_opens: Arc<Mutex<Vec<PathBuf>>>,
    focus_handle: FocusHandle,
}

impl AppView {
    fn new(
        composition: Arc<RwLock<Composition>>,
        buffer: Arc<RwLock<Buffer>>,
        device: Device,
        document: Entity<BufferDocument>,
        playback: PlaybackSession,
        pending_opens: Arc<Mutex<Vec<PathBuf>>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(&document, |this, _, cx| {
            this.playback.sync_from_document(this.document.read(cx));
            cx.notify();
        })
        .detach();

        let document_for_poll = document.clone();
        cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(33))
                .await;
            let still_alive = cx.update(|window, cx| {
                this.update(cx, |this, cx| {
                    this.drain_pending_opens(window, cx);
                    let mut dirty = false;
                    this.document.update(cx, |doc, cx| {
                        if this.playback.poll(doc) {
                            dirty = true;
                            cx.notify();
                        }
                    });
                    if dirty {
                        cx.notify();
                    }
                })
            });
            if !matches!(still_alive, Ok(Ok(()))) {
                break;
            }
            let _ = &document_for_poll;
        })
        .detach();

        let waveform = cx.new(|cx| WaveformDisplay::new(document.clone(), cx));
        cx.observe(&waveform, |_, _, cx| cx.notify()).detach();
        Self {
            composition,
            buffer,
            device,
            document,
            waveform,
            playback,
            app_menu_bar: (!cfg!(target_os = "macos")).then(|| AppMenuBar::new(cx)),
            pending_opens,
            focus_handle: cx.focus_handle(),
        }
    }

    fn composition_title(composition: &Composition) -> SharedString {
        composition
            .pool()
            .first()
            .and_then(|m| m.path.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "snd-review".into())
            .into()
    }

    fn load_composition(
        &mut self,
        composition: Composition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let title = Self::composition_title(&composition);
        {
            *self.composition.write().unwrap() = composition;
        }
        {
            *self.buffer.write().unwrap() = Buffer::empty();
        }
        let snapshot = self.buffer.read().unwrap();
        if let Err(err) = self.playback.reload(&self.device, &snapshot) {
            eprintln!("failed to reload playback: {err:#}");
        }
        drop(snapshot);

        self.document
            .update(cx, |doc, _| doc.reset_for_new_buffer());
        self.playback.sync_from_document(self.document.read(cx));
        self.waveform.update(cx, |view, cx| view.reset_view(cx));

        window.set_window_title(&title);
        cx.notify();
    }

    fn show_load_error(&self, message: &str, window: &mut Window, cx: &mut Context<Self>) {
        let message = message.to_string();
        window.open_alert_dialog(cx, move |alert, _, _| {
            alert
                .title("Failed to open file")
                .description(message.clone())
        });
    }

    fn drain_pending_opens(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = std::mem::take(&mut *self.pending_opens.lock().unwrap());
        if let Some(path) = paths.into_iter().next() {
            self.open_path(path, window, cx);
        }
    }

    fn open_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let view = cx.entity();
        cx.spawn_in(window, async move |_, window| {
            let result = std::thread::spawn(move || Composition::load_from_path(&path)).join();
            let _ = window.update(|window, cx| {
                view.update(cx, |this, cx| match result {
                    Ok(Ok(composition)) => this.load_composition(composition, window, cx),
                    Ok(Err(err)) => this.show_load_error(&format!("{err:#}"), window, cx),
                    Err(_) => this.show_load_error("failed to load file", window, cx),
                });
            });
        })
        .detach();
    }

    fn prompt_open_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open".into()),
        });
        let view = cx.entity();
        cx.spawn_in(window, async move |_, window| {
            let paths = match receiver.await {
                Ok(Ok(Some(paths))) => paths,
                _ => return,
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let result = std::thread::spawn(move || Composition::load_from_path(&path)).join();
            let _ = window.update(|window, cx| {
                view.update(cx, |this, cx| match result {
                    Ok(Ok(composition)) => this.load_composition(composition, window, cx),
                    Ok(Err(err)) => this.show_load_error(&format!("{err:#}"), window, cx),
                    Err(_) => this.show_load_error("failed to load file", window, cx),
                });
            });
        })
        .detach();
    }
}

fn format_secs(secs: f64) -> String {
    if secs.is_nan() || secs.is_infinite() {
        return "0.000000s".into();
    }
    format!("{secs:.6}s")
}

fn transport_state_label(state: TransportState) -> &'static str {
    match state {
        TransportState::Stopped => "Stopped",
        TransportState::Playing => "Playing",
        TransportState::Paused => "Paused",
    }
}

fn format_hover_meta(doc: &BufferDocument, sample: usize) -> String {
    format!(
        "hover {}  ·  {sample} smp",
        format_secs(doc.sample_to_secs(sample))
    )
}

fn format_header_meta(doc: &BufferDocument, transport: TransportState) -> String {
    if !doc.is_loaded() {
        return format!("No file open  ·  {}", transport_state_label(transport));
    }

    let mut parts = vec![transport_state_label(transport).to_string()];

    if let Some(pos) = &doc.current_position {
        parts.push(format!(
            "pos {}",
            format_secs(doc.sample_to_secs(pos.sample))
        ));
    }

    if let Selection::Region { start, end, .. } = &doc.selection {
        if end > start {
            let start_secs = doc.sample_to_secs(*start);
            let end_secs = doc.sample_to_secs(*end);
            let len_samples = end - start + 1;
            let len_secs = len_samples as f64 / f64::from(doc.sample_rate().max(1));
            parts.push(format!(
                "region {}–{}",
                format_secs(start_secs),
                format_secs(end_secs)
            ));
            parts.push(format!("len {}", format_secs(len_secs)));
            parts.push(format!("{len_samples} smp"));
        }
    }

    parts.join("  ·  ")
}

impl Render for AppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let waveform = self.waveform.clone();
        let doc = self.document.read(cx);
        let transport_state = self.playback.transport_state();
        let looping = self.playback.looping();

        let hover_sample = self.waveform.read(cx).hover_sample();
        let meta = format_header_meta(doc, transport_state);
        let hover_meta = hover_sample.map(|sample| format_hover_meta(doc, sample));
        let file_status = FileStatus::from_composition(&doc.composition.read().unwrap());
        let drop_highlight = theme.secondary;

        div()
            .id("app-view")
            .key_context("App")
            .track_focus(&self.focus_handle)
            .relative()
            .size_full()
            .drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(drop_highlight))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                if let Some(path) = paths.paths().first() {
                    this.open_path(path.clone(), window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleZeroCrossing, _, cx| {
                this.document
                    .update(cx, |doc, _| doc.toggle_zero_crossing_snap());
                cx.notify();
            }))
            .child(
                v_flex()
                    .size_full()
                    .bg(theme.background)
                    .text_color(theme.foreground)
                    .when_some(self.app_menu_bar.clone(), |this, menu_bar| {
                        this.child(
                            h_flex()
                                .id("app-menu-bar-row")
                                .w_full()
                                .h(TITLE_BAR_HEIGHT)
                                .flex_none()
                                .items_center()
                                .px_2()
                                .border_b_1()
                                .border_color(theme.border)
                                .bg(theme.title_bar)
                                .child(menu_bar),
                        )
                    })
                    .child(
                        h_flex()
                            .w_full()
                            .flex_none()
                            .px_3()
                            .py_2()
                            .gap_2()
                            .items_center()
                            .border_b_1()
                            .border_color(theme.border)
                            .bg(theme.title_bar)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .whitespace_nowrap()
                                    .overflow_hidden()
                                    .child(meta),
                            )
                            .when_some(hover_meta, |this, hover_meta| {
                                this.child(
                                    div()
                                        .flex_none()
                                        .text_sm()
                                        .text_color(theme.muted_foreground)
                                        .whitespace_nowrap()
                                        .child(hover_meta),
                                )
                            }),
                    )
                    .child(div().flex_1().min_h_0().w_full().child(waveform))
                    .child(Transport::new(transport_state, looping))
                    .child(FileStatusBar::new(file_status)),
            )
            .children(Root::render_dialog_layer(window, cx))
    }
}

impl Focusable for AppView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl AppView {
    fn sync_playback_to_document(&mut self, cx: &mut Context<Self>) {
        self.document.update(cx, |doc, cx| {
            self.playback.sync_document_from_playback(doc);
            cx.notify();
        });
        cx.notify();
    }
}

fn quit(_: &Quit, cx: &mut App) {
    cx.quit();
}

fn open(_: &Open, cx: &mut App) {
    let Some(view) = cx.try_global::<OpenTarget>().map(|target| target.0.clone()) else {
        return;
    };
    let Some(window) = cx.active_window() else {
        return;
    };
    cx.defer(move |cx| {
        let _ = window.update(cx, |_, window, cx| {
            view.update(cx, |this, cx| {
                this.prompt_open_file(window, cx);
            });
        });
    });
}

fn about(_: &About, cx: &mut App) {
    let Some(window) = cx.active_window().and_then(|w| w.downcast::<Root>()) else {
        return;
    };
    cx.defer(move |cx| {
        let _ = window.update(cx, |_, window, cx| {
            window.defer(cx, |window, cx| {
                window.open_alert_dialog(cx, |alert, _, _| {
                    alert.title("About snd-review").description(format!(
                        "{}\n\nVersion {}",
                        env!("CARGO_PKG_DESCRIPTION"),
                        env!("CARGO_PKG_VERSION"),
                    ))
                });
            });
        });
    });
}

fn with_app_view(cx: &mut App, f: impl FnOnce(&mut AppView, &mut Context<AppView>)) {
    let Some(view) = cx.try_global::<OpenTarget>().map(|target| target.0.clone()) else {
        return;
    };
    view.update(cx, f);
}

fn transport_home(_: &TransportHome, cx: &mut App) {
    with_app_view(cx, |this, cx| {
        this.playback.home();
        this.sync_playback_to_document(cx);
    });
}

fn transport_previous(_: &TransportPrevious, cx: &mut App) {
    with_app_view(cx, |this, cx| {
        this.playback.previous();
        this.sync_playback_to_document(cx);
    });
}

fn transport_start(_: &TransportStart, cx: &mut App) {
    with_app_view(cx, |this, cx| {
        this.playback.start();
        this.sync_playback_to_document(cx);
    });
}

fn transport_play_pause(_: &TransportPlayPause, cx: &mut App) {
    with_app_view(cx, |this, cx| {
        this.playback.toggle_play_pause();
        this.sync_playback_to_document(cx);
    });
}

fn transport_stop(_: &TransportStop, cx: &mut App) {
    with_app_view(cx, |this, cx| {
        this.playback.stop();
        this.sync_playback_to_document(cx);
    });
}

fn transport_next(_: &TransportNext, cx: &mut App) {
    with_app_view(cx, |this, cx| {
        this.playback.next();
        this.sync_playback_to_document(cx);
    });
}

fn transport_end(_: &TransportEnd, cx: &mut App) {
    with_app_view(cx, |this, cx| {
        this.playback.end();
        this.sync_playback_to_document(cx);
    });
}

fn transport_loop(_: &TransportLoop, cx: &mut App) {
    with_app_view(cx, |this, cx| {
        this.playback.toggle_loop();
        cx.notify();
    });
}

fn view_fit_all(_: &ViewFitAll, cx: &mut App) {
    with_app_view(cx, |this, cx| {
        this.waveform.update(cx, |view, cx| view.fit(cx));
    });
}

fn view_frame(_: &ViewFrame, cx: &mut App) {
    with_app_view(cx, |this, cx| {
        this.waveform.update(cx, |view, cx| view.frame(cx));
    });
}

fn view_zoom_in(_: &ViewZoomIn, cx: &mut App) {
    with_app_view(cx, |this, cx| {
        this.waveform.update(cx, |view, cx| view.zoom_in(cx));
    });
}

fn view_zoom_out(_: &ViewZoomOut, cx: &mut App) {
    with_app_view(cx, |this, cx| {
        this.waveform.update(cx, |view, cx| view.zoom_out(cx));
    });
}

fn with_edit(cx: &mut App, f: impl FnOnce(&mut BufferDocument)) {
    with_app_view(cx, |this, cx| {
        this.document.update(cx, |doc, cx| {
            f(doc);
            cx.notify();
        });
        this.playback.sync_from_document(this.document.read(cx));
        cx.notify();
    });
}

fn edit_undo(_: &EditUndo, cx: &mut App) {
    with_edit(cx, |doc| {
        doc.edit_undo();
    });
}

fn edit_redo(_: &EditRedo, cx: &mut App) {
    with_edit(cx, |doc| {
        doc.edit_redo();
    });
}

fn edit_cut(_: &EditCut, cx: &mut App) {
    with_edit(cx, |doc| doc.edit_cut());
}

fn edit_copy(_: &EditCopy, cx: &mut App) {
    with_edit(cx, |doc| doc.edit_copy());
}

fn edit_paste(_: &EditPaste, cx: &mut App) {
    with_edit(cx, |doc| doc.edit_paste());
}

fn edit_delete(_: &EditDelete, cx: &mut App) {
    with_edit(cx, |doc| doc.edit_delete());
}

fn edit_remove(_: &EditRemove, cx: &mut App) {
    with_edit(cx, |doc| doc.edit_remove());
}

fn edit_duplicate(_: &EditDuplicate, cx: &mut App) {
    with_edit(cx, |doc| doc.edit_duplicate());
}

fn edit_trim(_: &EditTrim, cx: &mut App) {
    with_edit(cx, |doc| doc.edit_trim());
}

fn edit_roll_left(_: &EditRollLeft, cx: &mut App) {
    with_edit(cx, |doc| doc.edit_roll(-1));
}

fn edit_roll_right(_: &EditRollRight, cx: &mut App) {
    with_edit(cx, |doc| doc.edit_roll(1));
}

fn app_menus() -> Vec<Menu> {
    vec![
        Menu::new("File").items([
            MenuItem::action("Open...", Open),
            MenuItem::separator(),
            MenuItem::action("Quit", Quit),
        ]),
        Menu::new("Edit").items([
            MenuItem::action("Undo", EditUndo),
            MenuItem::action("Redo", EditRedo),
            MenuItem::separator(),
            MenuItem::action("Cut", EditCut),
            MenuItem::action("Copy", EditCopy),
            MenuItem::action("Paste", EditPaste),
            MenuItem::separator(),
            MenuItem::action("Delete", EditDelete),
            MenuItem::action("Remove", EditRemove),
            MenuItem::action("Duplicate", EditDuplicate),
            MenuItem::action("Trim to Selection", EditTrim),
            MenuItem::separator(),
            MenuItem::action("Roll Source Left", EditRollLeft),
            MenuItem::action("Roll Source Right", EditRollRight),
        ]),
        Menu::new("View").items([
            MenuItem::action("Zoom In", ViewZoomIn),
            MenuItem::action("Zoom Out", ViewZoomOut),
            MenuItem::action("Reset View", ViewFitAll),
        ]),
        Menu::new("Help").items([MenuItem::action("About...", About)]),
    ]
}

fn install_app_menu(cx: &mut App) {
    cx.on_action(open);
    cx.on_action(quit);
    cx.on_action(about);
    cx.on_action(transport_home);
    cx.on_action(transport_previous);
    cx.on_action(transport_start);
    cx.on_action(transport_play_pause);
    cx.on_action(transport_stop);
    cx.on_action(transport_next);
    cx.on_action(transport_end);
    cx.on_action(transport_loop);
    cx.on_action(view_fit_all);
    cx.on_action(view_frame);
    cx.on_action(view_zoom_in);
    cx.on_action(view_zoom_out);
    cx.on_action(edit_undo);
    cx.on_action(edit_redo);
    cx.on_action(edit_cut);
    cx.on_action(edit_copy);
    cx.on_action(edit_paste);
    cx.on_action(edit_delete);
    cx.on_action(edit_remove);
    cx.on_action(edit_duplicate);
    cx.on_action(edit_trim);
    cx.on_action(edit_roll_left);
    cx.on_action(edit_roll_right);
    install_keybindings(cx);
    cx.set_menus(app_menus());
    let owned = app_menus().into_iter().map(|menu| menu.owned()).collect();
    GlobalState::global_mut(cx).set_app_menus(owned);
    cx.activate(true);
}

fn path_from_open_url(url: &str) -> Option<PathBuf> {
    let decoded = if let Some(rest) = url.strip_prefix("file://") {
        let path = if rest.starts_with('/') {
            rest
        } else {
            let slash = rest.find('/')?;
            &rest[slash..]
        };
        percent_decode(path)?
    } else if url.starts_with('/') {
        url.to_owned()
    } else {
        return None;
    };
    Some(PathBuf::from(decoded))
}

fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return None;
            }
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

pub fn run(initial: Option<Composition>, device: Device) {
    let composition = initial.unwrap_or_else(|| Composition::new(44100, 2));
    let title = AppView::composition_title(&composition);

    let shared_composition = Arc::new(RwLock::new(composition));
    let shared_buffer = Arc::new(RwLock::new(Buffer::empty()));
    let playback = PlaybackSession::open(&device, shared_composition.clone())
        .expect("failed to open audio playback device");
    let pending_opens = Arc::new(Mutex::new(Vec::<PathBuf>::new()));

    let app = gpui_platform::application().with_assets(AppAssets);
    app.on_open_urls({
        let pending_opens = pending_opens.clone();
        move |urls| {
            let mut pending = pending_opens.lock().unwrap();
            for url in urls {
                if let Some(path) = path_from_open_url(&url) {
                    pending.push(path);
                }
            }
        }
    });
    app.run(move |cx| {
        gpui_component::init(cx);
        Theme::change(ThemeMode::Dark, None, cx);
        install_app_menu(cx);

        let title = title.clone();
        let pending_opens = pending_opens.clone();
        cx.spawn(async move |cx| {
            let options = WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some(title.clone()),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: point(px(80.), px(80.)),
                    size: size(px(1280.), px(760.)),
                })),
                ..Default::default()
            };

            cx.open_window(options, move |window, cx| {
                let document = cx.new(|_| {
                    BufferDocument::with_shared(shared_composition.clone(), shared_buffer.clone())
                });
                let view = cx.new(|cx| {
                    AppView::new(
                        shared_composition.clone(),
                        shared_buffer.clone(),
                        device,
                        document,
                        playback,
                        pending_opens.clone(),
                        window,
                        cx,
                    )
                });
                cx.set_global(OpenTarget(view.clone()));
                window.focus(&view.focus_handle(cx), cx);
                cx.new(|cx| Root::new(view, window, cx).bg(cx.theme().background))
            })
            .expect("failed to open window");
        })
        .detach();
    });
}
