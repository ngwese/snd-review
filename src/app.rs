// SPDX-FileCopyrightText: 2026 Greg Wuller
// SPDX-License-Identifier: MIT

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use cpal::Device;
use gpui::{
    div, point, prelude::FluentBuilder as _, px, size, App, AppContext as _, Bounds, Context,
    Entity, ExternalPaths, FocusHandle, Focusable, Global, InteractiveElement as _, IntoElement,
    KeyContext, Menu, MenuItem, ParentElement as _, PathPromptOptions, Pixels, Render,
    SharedString, Styled as _, TitlebarOptions, WeakEntity, Window, WindowBounds, WindowOptions,
};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    dock::{
        panel_handle, DockArea, DockEvent, DockLayout, DockPlacement, InsertTarget, NodeId,
        PaneRef, PanelId, PanelStyle,
    },
    h_flex,
    menu::AppMenuBar,
    v_flex, ActiveTheme as _, GlobalState, Icon, IconName, Root, Selectable as _, Sizable as _,
    Theme, ThemeMode, TitleBar, WindowExt as _,
};

use crate::assets::AppAssets;
use crate::commands::{
    install_keybindings, About, EditCopy, EditCut, EditDelete, EditDuplicate, EditPaste, EditRedo,
    EditRemove, EditRollLeft, EditRollRight, EditTrim, EditUndo, Open, Quit, TransportEnd,
    TransportHome, TransportLoop, TransportNext, TransportPlayPause, TransportPrevious,
    TransportStart, TransportStop, ViewExplorer, ViewFitAll, ViewFrame, ViewHistory, ViewScript,
    ViewZoomIn, ViewZoomOut,
};
use crate::components::dock_skin::{CenterTabCloseHandler, CompactDockSkin};
use crate::components::edits::EditsPanel;
use crate::components::empty_pane::EmptyPane;
use crate::components::explorer::{ExplorerEvent, ExplorerPanel};
use crate::components::repl::ReplPanel;
use crate::components::status_bar::{FileStatus, FileStatusBar};
use crate::components::waveform::{ToggleZeroCrossing, WaveformDisplay};
use crate::components::workspace::WorkspacePanel;
use crate::model::composition::Composition;
use crate::model::selection::Selection;
use crate::model::{Buffer, BufferDocument};
use crate::playback::{PlaybackSession, TransportState};
use crate::progress::ProgressState;
use crate::script::{EvalOutput, ScriptHost};
use crate::session::{DocumentId, DocumentSession};

struct OpenTarget(Entity<AppView>);

impl Global for OpenTarget {}

#[derive(Clone)]
struct DocumentViews {
    composition: Arc<RwLock<Composition>>,
    buffer: Arc<RwLock<Buffer>>,
    document: Entity<BufferDocument>,
    waveform: Entity<WaveformDisplay>,
    workspace: Entity<WorkspacePanel>,
}

pub struct AppView {
    session: DocumentSession,
    views: HashMap<DocumentId, DocumentViews>,
    dock_area: Entity<DockArea>,
    explorer: Entity<ExplorerPanel>,
    edits: Entity<EditsPanel>,
    empty_editors: Entity<EmptyPane>,
    repl: Entity<ReplPanel>,
    script: ScriptHost,
    idle_composition: Arc<RwLock<Composition>>,
    playback: PlaybackSession,
    app_menu_bar: Option<Entity<AppMenuBar>>,
    pending_opens: Arc<Mutex<Vec<PathBuf>>>,
    pending_load: Arc<Mutex<Vec<(DocumentId, u64, Result<Composition, String>)>>>,
    focus_handle: FocusHandle,
    last_progress: Option<ProgressState>,
    script_dock_size: Pixels,
    last_waveform_hover: Option<usize>,
    last_waveform_over: bool,
}

impl AppView {
    fn new(
        composition: Arc<RwLock<Composition>>,
        buffer: Arc<RwLock<Buffer>>,
        source_path: Option<PathBuf>,
        playback: PlaybackSession,
        pending_opens: Arc<Mutex<Vec<PathBuf>>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let app = cx.weak_entity();
        cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(33))
                .await;
            let still_alive = cx.update(|window, cx| {
                this.update(cx, |this, cx| {
                    this.drain_pending_opens(window, cx);
                    this.drain_pending_load(window, cx);
                    let mut dirty = false;
                    if let Some(views) = this.active_views() {
                        views.document.update(cx, |doc, cx| {
                            if this.playback.poll(doc) {
                                dirty = true;
                                cx.notify();
                            }
                        });
                        let progress = views.document.read(cx).progress.snapshot();
                        if progress != this.last_progress {
                            this.last_progress = progress;
                            dirty = true;
                            views.waveform.update(cx, |_, cx| cx.notify());
                        }
                        views.workspace.update(cx, |workspace, cx| {
                            workspace.sync_transport(
                                this.playback.transport_state(),
                                this.playback.looping(),
                                cx,
                            );
                        });
                    }
                    if dirty {
                        cx.notify();
                    }
                })
            });
            if !matches!(still_alive, Ok(Ok(()))) {
                break;
            }
        })
        .detach();

        let has_initial = source_path.is_some() || composition.read().unwrap().frames() > 0;
        let idle_composition = if has_initial {
            Arc::new(RwLock::new(Composition::new(44100, 2)))
        } else {
            composition.clone()
        };
        let mut session = DocumentSession::new();
        let mut views = HashMap::new();
        let first_workspace = if has_initial {
            let first_id = session.push(source_path);
            let first = Self::make_views(first_id, composition, buffer, app.clone(), cx);
            let workspace = first.workspace.clone();
            views.insert(first_id, first);
            Some(workspace)
        } else {
            None
        };

        let explorer = cx.new(|cx| ExplorerPanel::new(cx));
        explorer.update(cx, |explorer, _| {
            let app = app.clone();
            explorer.set_handler(Rc::new(move |event, window, cx| {
                let _ = app.update(cx, |this, cx| this.handle_explorer(event, window, cx));
            }));
        });
        let empty_editors = cx.new(|cx| {
            EmptyPane::new("EmptyEditorsPanel", "No open editors", cx)
                .with_message("Drop an audio file here or use File → Open…")
        });
        let initial_target = session.active().and_then(|id| views.get(&id).cloned());
        let edits = cx.new(|cx| EditsPanel::new(cx));
        if let Some(views) = initial_target {
            edits.update(cx, |edits, cx| {
                edits.set_target(views.document, views.waveform, cx);
            });
        }
        let transients = cx.new(|cx| EmptyPane::new("TransientsPanel", "Transients", cx));
        let markers = cx.new(|cx| EmptyPane::new("MarkersPanel", "Markers", cx));
        let labels = cx.new(|cx| EmptyPane::new("LabelsPanel", "Labels", cx));
        let script = ScriptHost::new().expect("lua runtime");
        let repl = cx.new(|cx| ReplPanel::new(window, cx));
        repl.update(cx, |repl, _| {
            let app = app.clone();
            repl.set_handler(Rc::new(move |code, window, cx| {
                let _ = app.update(cx, |this, cx| this.eval_lua(&code, window, cx));
            }));
        });
        let (dock_area, skin) = CompactDockSkin::dock_area("main-dock", Some(1), window, cx);
        let explorer_handle = panel_handle(explorer.clone());
        let center_handle = match first_workspace {
            Some(workspace) => panel_handle(workspace),
            None => panel_handle(empty_editors.clone()),
        };
        let edits_handle = panel_handle(edits.clone());
        let transients_handle = panel_handle(transients);
        let markers_handle = panel_handle(markers);
        let labels_handle = panel_handle(labels);
        dock_area.update(cx, |area, cx| {
            area.set_center(DockLayout::tabs().panel_view(center_handle, cx), window, cx);
            area.set_dock(
                DockPlacement::Left,
                DockLayout::tabs().panel_view(explorer_handle, cx),
                window,
                cx,
            );
            area.set_dock_size(DockPlacement::Left, px(220.), window, cx);
            area.set_dock_collapsible(DockPlacement::Left, true, window, cx);
            area.toggle_dock(DockPlacement::Left, window, cx);
            area.set_dock(
                DockPlacement::Right,
                DockLayout::tabs()
                    .panel_view(edits_handle, cx)
                    .panel_view(transients_handle, cx)
                    .panel_view(markers_handle, cx)
                    .panel_view(labels_handle, cx),
                window,
                cx,
            );
            area.set_dock_size(DockPlacement::Right, px(260.), window, cx);
            area.set_dock_collapsible(DockPlacement::Right, true, window, cx);
            area.toggle_dock(DockPlacement::Right, window, cx);
        });
        skin.set_panel_style(PanelStyle::TabBar, cx);
        skin.set_toggle_button_visible(false, cx);
        cx.subscribe_in(
            &dock_area,
            window,
            |this, _, event: &DockEvent, window, cx| {
                if matches!(event, DockEvent::LayoutChanged) {
                    this.sync_tabs_from_layout(window, cx);
                }
            },
        )
        .detach();

        let mut this = Self {
            session,
            views,
            dock_area,
            explorer,
            edits,
            empty_editors,
            repl,
            script,
            idle_composition,
            playback,
            app_menu_bar: (!cfg!(target_os = "macos")).then(|| AppMenuBar::new(cx)),
            pending_opens,
            pending_load: Arc::new(Mutex::new(Vec::new())),
            focus_handle: cx.focus_handle(),
            last_progress: None,
            script_dock_size: px(160.),
            last_waveform_hover: None,
            last_waveform_over: false,
        };
        this.load_init_lua(window, cx);
        this.refresh_explorer(cx);
        if let Some(id) = this.session.active() {
            this.spawn_peak_build(id, cx);
        }
        this
    }

    fn make_views(
        id: DocumentId,
        composition: Arc<RwLock<Composition>>,
        buffer: Arc<RwLock<Buffer>>,
        app: WeakEntity<Self>,
        cx: &mut Context<Self>,
    ) -> DocumentViews {
        let document = cx.new(|_| BufferDocument::with_shared(composition.clone(), buffer.clone()));
        cx.observe(&document, move |this, entity, cx| {
            if this.session.active() == Some(id) {
                this.playback.sync_from_document(entity.read(cx));
                cx.notify();
            }
        })
        .detach();
        let waveform = cx.new(|cx| WaveformDisplay::new(document.clone(), cx));
        cx.observe(&waveform, |this, waveform, cx| {
            let hover = waveform.read(cx).hover_sample();
            let over = waveform.read(cx).pointer_over();
            if this.last_waveform_hover == hover && this.last_waveform_over == over {
                return;
            }
            this.last_waveform_hover = hover;
            this.last_waveform_over = over;
            cx.notify();
        })
        .detach();
        let workspace =
            cx.new(|cx| WorkspacePanel::new(id, document.clone(), waveform.clone(), cx));
        workspace.update(cx, |workspace, _| {
            workspace.set_on_activated(Rc::new(move |id, window, cx| {
                let _ = app.update(cx, |this, cx| this.focus_document(id, window, cx));
            }));
        });
        DocumentViews {
            composition,
            buffer,
            document,
            waveform,
            workspace,
        }
    }

    fn active_views(&self) -> Option<DocumentViews> {
        let id = self.session.active()?;
        self.views.get(&id).cloned()
    }

    fn composition_title(composition: &Composition) -> SharedString {
        composition.display_name().into()
    }

    fn display_title(&self, id: DocumentId, _cx: &App) -> SharedString {
        if let Some(path) = self
            .session
            .get(id)
            .and_then(|doc| doc.source_path.as_ref())
        {
            if let Some(name) = path.file_name() {
                let name = name.to_string_lossy();
                if !name.is_empty() {
                    return name.into_owned().into();
                }
            }
        }
        self.views
            .get(&id)
            .map(|views| Self::composition_title(&views.composition.read().unwrap()))
            .unwrap_or_else(|| "snd-review".into())
    }

    fn refresh_explorer(&self, cx: &mut Context<Self>) {
        let docs: Vec<_> = self
            .session
            .documents()
            .iter()
            .map(|doc| {
                let modified = self
                    .views
                    .get(&doc.id)
                    .is_some_and(|views| views.composition.read().unwrap().edit_cursor() > 0);
                (doc.id, self.display_title(doc.id, cx), modified)
            })
            .collect();
        let active = self.session.active();
        self.explorer.update(cx, |explorer, cx| {
            explorer.set_documents(&docs, active, cx);
        });
    }

    fn update_window_title(&self, window: &mut Window, cx: &App) {
        let title = self
            .session
            .active()
            .map(|id| self.display_title(id, cx))
            .unwrap_or_else(|| "snd-review".into());
        window.set_window_title(&title);
    }

    fn center_panel_ids(&self, cx: &App) -> HashSet<u64> {
        self.dock_area
            .read(cx)
            .layout(DockPlacement::Center)
            .map(|tree| tree.panels().map(|id| id.as_u64()).collect())
            .unwrap_or_default()
    }

    fn empty_editors_id(&self) -> u64 {
        PanelId::from(self.empty_editors.entity_id()).as_u64()
    }

    fn ensure_placeholder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.center_panel_ids(cx).contains(&self.empty_editors_id()) {
            return;
        }
        let panel = self.empty_editors.clone();
        self.dock_area.update(cx, |area, cx| {
            area.add_panel(panel, DockPlacement::Center, None, window, cx);
        });
    }

    fn remove_placeholder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.session.tab_open_count() == 0 {
            return;
        }
        if !self.center_panel_ids(cx).contains(&self.empty_editors_id()) {
            return;
        }
        let panel = self.empty_editors.clone();
        self.dock_area.update(cx, |area, cx| {
            area.remove_panel(panel, window, cx);
        });
    }

    fn sync_tabs_from_layout(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let center = self.center_panel_ids(cx);
        let ids: Vec<DocumentId> = self.session.documents().iter().map(|doc| doc.id).collect();
        for id in ids {
            let open = self.views.get(&id).is_some_and(|views| {
                center.contains(&PanelId::from(views.workspace.entity_id()).as_u64())
            });
            self.session.set_tab_open(id, open);
        }
        if self.session.tab_open_count() == 0 {
            self.ensure_placeholder(window, cx);
        } else {
            self.remove_placeholder(window, cx);
        }
        self.refresh_explorer(cx);
    }

    fn apply_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(views) = self.active_views() {
            self.playback.bind_composition(views.composition);
            self.playback.sync_from_document(views.document.read(cx));
            self.edits.update(cx, |edits, cx| {
                edits.set_target(views.document, views.waveform, cx);
            });
        } else {
            self.playback
                .bind_composition(self.idle_composition.clone());
            self.playback.reload(&Buffer::empty());
            self.edits.update(cx, |edits, cx| edits.clear_target(cx));
        }
        self.refresh_explorer(cx);
        self.update_window_title(window, cx);
        cx.notify();
    }

    fn stop_playback_into_active(&mut self, cx: &mut Context<Self>) {
        if self.playback.transport_state() != TransportState::Playing {
            return;
        }
        self.sync_playback_to_document(cx);
        self.playback.stop();
        if let Some(views) = self.active_views() {
            views.workspace.update(cx, |workspace, cx| {
                workspace.sync_transport(TransportState::Stopped, self.playback.looping(), cx);
            });
        }
    }

    fn focus_document(&mut self, id: DocumentId, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(old_id) = self.session.focus(id) {
            if self.playback.transport_state() == TransportState::Playing {
                if let Some(old) = self.views.get(&old_id).cloned() {
                    old.document.update(cx, |doc, cx| {
                        self.playback.sync_document_from_playback(doc);
                        cx.notify();
                    });
                }
                self.playback.stop();
            }
            if let Some(old) = self.views.get(&old_id) {
                old.workspace.update(cx, |workspace, cx| {
                    workspace.sync_transport(TransportState::Stopped, self.playback.looping(), cx);
                });
            }
        } else {
            self.refresh_explorer(cx);
            self.update_window_title(window, cx);
            return;
        }
        self.apply_active(window, cx);
    }

    fn center_tab_slot(area: &DockArea, panel_id: PanelId) -> Option<(NodeId, usize, usize)> {
        let tree = area.layout(DockPlacement::Center)?;
        let node = tree.find_panel_node(panel_id)?;
        match tree.find_node(node)?.kind() {
            PaneRef::Tabs { panels, active_ix } => {
                let ix = panels.iter().position(|id| *id == panel_id)?;
                Some((node, ix, active_ix))
            }
            _ => None,
        }
    }

    /// Select `workspace` in the center tab bar, or add it once with a titled
    /// panel handle. Never inserts a second copy of the same panel.
    fn show_workspace_tab(
        area: &mut DockArea,
        workspace: Entity<WorkspacePanel>,
        window: &mut Window,
        cx: &mut Context<DockArea>,
    ) {
        let panel_id = PanelId::from(workspace.entity_id());
        if let Some((node, ix, active_ix)) = Self::center_tab_slot(area, panel_id) {
            if ix != active_ix {
                area.move_panel(
                    panel_id,
                    InsertTarget::Tabs {
                        node,
                        ix: Some(ix),
                        activate: true,
                    },
                    window,
                    cx,
                );
            }
            return;
        }
        area.add_panel_view(
            panel_handle(workspace),
            DockPlacement::Center,
            None,
            window,
            cx,
        );
    }

    fn ensure_tab(&mut self, id: DocumentId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(views) = self.views.get(&id).cloned() else {
            return;
        };
        self.focus_document(id, window, cx);
        let workspace = views.workspace.clone();
        self.dock_area.update(cx, |area, cx| {
            Self::show_workspace_tab(area, workspace, window, cx);
        });
        self.session.ensure_tab(id);
        self.remove_placeholder(window, cx);
    }

    fn close_tab(&mut self, id: DocumentId, window: &mut Window, cx: &mut Context<Self>) {
        if !self.session.get(id).is_some_and(|doc| doc.tab_open) {
            return;
        }
        if self.session.tab_open_count() == 1 {
            self.ensure_placeholder(window, cx);
        }
        if let Some(views) = self.views.get(&id) {
            let workspace = views.workspace.clone();
            self.dock_area.update(cx, |area, cx| {
                area.remove_panel(workspace, window, cx);
            });
        }
        self.session.close_tab(id);
        cx.notify();
    }

    fn close_center_panel(&mut self, panel_id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if panel_id == self.empty_editors_id() {
            return;
        }
        let Some(id) = self.views.iter().find_map(|(id, views)| {
            (PanelId::from(views.workspace.entity_id()).as_u64() == panel_id).then_some(*id)
        }) else {
            return;
        };
        self.close_tab(id, window, cx);
    }

    fn close_document(&mut self, id: DocumentId, window: &mut Window, cx: &mut Context<Self>) {
        if self.session.active() == Some(id) {
            self.stop_playback_into_active(cx);
        }
        if self.session.get(id).is_some_and(|doc| doc.tab_open) {
            self.close_tab(id, window, cx);
        }
        if let Some(views) = self.views.get(&id) {
            views.document.read(cx).progress.cancel();
        }
        self.session.close_document(id);
        self.views.remove(&id);
        if self.session.is_empty() {
            self.ensure_placeholder(window, cx);
        }
        self.apply_active(window, cx);
        self.refresh_explorer(cx);
        cx.notify();
    }

    fn add_document(
        &mut self,
        composition: Arc<RwLock<Composition>>,
        buffer: Arc<RwLock<Buffer>>,
        source_path: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> DocumentId {
        let app = cx.weak_entity();
        let id = self.session.push(source_path);
        let views = Self::make_views(id, composition, buffer, app, cx);
        let workspace = views.workspace.clone();
        self.views.insert(id, views);
        self.dock_area.update(cx, |area, cx| {
            Self::show_workspace_tab(area, workspace, window, cx);
        });
        self.remove_placeholder(window, cx);
        id
    }

    fn handle_explorer(
        &mut self,
        event: ExplorerEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ExplorerEvent::Activate(id) | ExplorerEvent::OpenTab(id) => {
                self.ensure_tab(id, window, cx);
            }
            ExplorerEvent::Close(id) => self.close_document(id, window, cx),
        }
    }

    fn push_loaded_composition(
        &mut self,
        id: DocumentId,
        composition: Composition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(views) = self.views.get(&id).cloned() else {
            return;
        };
        views.document.read(cx).progress.cancel();
        {
            *views.composition.write().unwrap() = composition;
        }
        {
            *views.buffer.write().unwrap() = Buffer::empty();
        }
        views
            .document
            .update(cx, |doc, _| doc.reset_for_new_buffer());
        views.waveform.update(cx, |view, cx| view.reset_view(cx));
        views.workspace.update(cx, |_, cx| cx.notify());
        if self.session.active() == Some(id) {
            let snapshot = views.buffer.read().unwrap();
            self.playback.reload(&snapshot);
            drop(snapshot);
            self.playback.sync_from_document(views.document.read(cx));
            self.update_window_title(window, cx);
        }
        self.spawn_peak_build(id, cx);
        self.refresh_explorer(cx);
        self.dock_area.update(cx, |_, cx| cx.notify());
        self.fire_loaded_script(id, window, cx);
        cx.notify();
    }

    fn toggle_edits_dock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dock_area.update(cx, |area, cx| {
            area.toggle_dock(DockPlacement::Right, window, cx);
        });
        self.sync_view_menus(cx);
        cx.notify();
    }

    fn toggle_explorer_dock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dock_area.update(cx, |area, cx| {
            area.toggle_dock(DockPlacement::Left, window, cx);
        });
        self.sync_view_menus(cx);
        cx.notify();
    }

    fn edits_dock_open(&self, cx: &App) -> bool {
        self.dock_area.read(cx).is_dock_open(DockPlacement::Right)
    }

    fn explorer_dock_open(&self, cx: &App) -> bool {
        self.dock_area.read(cx).is_dock_open(DockPlacement::Left)
    }

    fn toggle_script_dock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.script_dock_open(cx) {
            self.hide_script_dock(window, cx);
        } else {
            self.show_script_dock(window, cx);
        }
    }

    fn script_dock_open(&self, cx: &App) -> bool {
        self.dock_area.read(cx).is_dock_open(DockPlacement::Bottom)
    }

    fn sync_view_menus(&self, cx: &mut Context<Self>) {
        apply_app_menus(
            self.explorer_dock_open(cx),
            self.edits_dock_open(cx),
            self.script_dock_open(cx),
            cx,
        );
        if let Some(bar) = self.app_menu_bar.clone() {
            bar.update(cx, |bar, cx| bar.reload(cx));
        }
    }

    fn show_script_dock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let opened = !self.script_dock_open(cx);
        if opened {
            let handle = panel_handle(self.repl.clone());
            let size = self.script_dock_size;
            self.dock_area.update(cx, |area, cx| {
                area.set_dock(
                    DockPlacement::Bottom,
                    DockLayout::tabs().panel_view(handle, cx),
                    window,
                    cx,
                );
                area.set_dock_size(DockPlacement::Bottom, size, window, cx);
                area.set_dock_collapsible(DockPlacement::Bottom, false, window, cx);
            });
        }
        self.repl.focus_handle(cx).focus(window, cx);
        if opened {
            self.sync_view_menus(cx);
            cx.notify();
        }
    }

    fn hide_script_dock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.script_dock_open(cx) {
            return;
        }
        if let Some(size) = self.dock_area.read(cx).dock_size(DockPlacement::Bottom) {
            self.script_dock_size = size;
        }
        self.dock_area.update(cx, |area, cx| {
            area.remove_dock(DockPlacement::Bottom, window, cx);
        });
        self.sync_view_menus(cx);
        cx.notify();
    }

    fn load_init_lua(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let _guard = crate::script::enter(self, window, cx);
        if let Err(err) = self.script.load_init() {
            self.repl.update(cx, |repl, cx| {
                repl.append_error(&err, cx);
            });
        }
        let prints = self.script.take_prints();
        if !prints.is_empty() {
            let output = EvalOutput {
                prints,
                result: None,
                error: None,
            };
            self.repl.update(cx, |repl, cx| {
                repl.append_output(&output, cx);
            });
        }
    }

    fn eval_lua(&mut self, code: &str, window: &mut Window, cx: &mut Context<Self>) {
        let _guard = crate::script::enter(self, window, cx);
        let output = self.script.eval(code);
        self.repl.update(cx, |repl, cx| {
            repl.append_eval(code, &output, cx);
        });
    }

    fn fire_loaded_script(&mut self, id: DocumentId, window: &mut Window, cx: &mut Context<Self>) {
        let _guard = crate::script::enter(self, window, cx);
        self.script.fire_loaded(id);
        let prints = self.script.take_prints();
        if !prints.is_empty() {
            let output = EvalOutput {
                prints,
                result: None,
                error: None,
            };
            self.repl.update(cx, |repl, cx| {
                repl.append_output(&output, cx);
            });
        }
    }

    pub(crate) fn session_active(&self) -> Option<DocumentId> {
        self.session.active()
    }

    pub(crate) fn session_document_ids(&self) -> Vec<DocumentId> {
        self.session.documents().iter().map(|doc| doc.id).collect()
    }

    pub(crate) fn script_display_name(&self, id: DocumentId, cx: &App) -> Option<String> {
        Some(self.display_title(id, cx).to_string())
    }

    pub(crate) fn script_path(&self, id: DocumentId) -> Option<PathBuf> {
        self.session.get(id).and_then(|doc| doc.source_path.clone())
    }

    pub(crate) fn script_open(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<DocumentId, String> {
        self.open_path(path.clone(), window, cx);
        self.session
            .find_by_path(&path)
            .or_else(|| self.session.active())
            .ok_or_else(|| format!("failed to open {}", path.display()))
    }

    pub(crate) fn script_with_document<R>(
        &mut self,
        id: DocumentId,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut BufferDocument) -> mlua::Result<R>,
    ) -> mlua::Result<R> {
        let views = self
            .views
            .get(&id)
            .ok_or_else(|| mlua::Error::runtime("composition is not open"))?;
        views.document.update(cx, |doc, _| f(doc))
    }

    pub(crate) fn after_script_edit(
        &mut self,
        id: DocumentId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.session.active() == Some(id) {
            if let Some(views) = self.views.get(&id).cloned() {
                self.playback.sync_from_document(views.document.read(cx));
                views.document.update(cx, |_, cx| cx.notify());
                views.waveform.update(cx, |_, cx| cx.notify());
            }
        }
        self.refresh_explorer(cx);
        self.spawn_peak_build(id, cx);
        self.update_window_title(window, cx);
        cx.notify();
    }

    pub(crate) fn invoke_command(
        &mut self,
        command_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        match command_id {
            "file.open" => self.prompt_open_file(window, cx),
            "file.quit" => cx.quit(),
            "help.about" => {
                window.open_alert_dialog(cx, |alert, _, _| {
                    alert.title("About snd-review").description(format!(
                        "{}\n\nVersion {}",
                        env!("CARGO_PKG_DESCRIPTION"),
                        env!("CARGO_PKG_VERSION"),
                    ))
                });
            }
            "transport.home" => {
                self.playback.home();
                self.sync_playback_to_document(cx);
            }
            "transport.previous" => {
                self.playback.previous();
                self.sync_playback_to_document(cx);
            }
            "transport.start" => {
                self.playback.start();
                self.sync_playback_to_document(cx);
            }
            "transport.play_pause" => {
                self.playback.toggle_play_pause();
                self.sync_playback_to_document(cx);
            }
            "transport.stop" => {
                self.playback.stop();
                self.sync_playback_to_document(cx);
            }
            "transport.next" => {
                self.playback.next();
                self.sync_playback_to_document(cx);
            }
            "transport.end" => {
                self.playback.end();
                self.sync_playback_to_document(cx);
            }
            "transport.loop" => {
                self.playback.toggle_loop();
                cx.notify();
            }
            "view.fit_all" => {
                if let Some(views) = self.active_views() {
                    views.waveform.update(cx, |view, cx| view.fit(cx));
                }
            }
            "view.frame" => {
                if let Some(views) = self.active_views() {
                    views.waveform.update(cx, |view, cx| view.frame(cx));
                }
            }
            "view.zoom_in" => {
                if let Some(views) = self.active_views() {
                    views.waveform.update(cx, |view, cx| view.zoom_in(cx));
                }
            }
            "view.zoom_out" => {
                if let Some(views) = self.active_views() {
                    views.waveform.update(cx, |view, cx| view.zoom_out(cx));
                }
            }
            "view.explorer" => self.toggle_explorer_dock(window, cx),
            "view.history" => self.toggle_edits_dock(window, cx),
            "view.script" => self.toggle_script_dock(window, cx),
            "edit.undo" => self.run_edit(cx, |doc| {
                doc.edit_undo();
            }),
            "edit.redo" => self.run_edit(cx, |doc| {
                doc.edit_redo();
            }),
            "edit.cut" => self.run_edit(cx, |doc| doc.edit_cut()),
            "edit.copy" => self.run_edit(cx, |doc| doc.edit_copy()),
            "edit.paste" => self.run_edit(cx, |doc| doc.edit_paste()),
            "edit.delete" => self.run_edit(cx, |doc| doc.edit_delete()),
            "edit.remove" => self.run_edit(cx, |doc| doc.edit_remove()),
            "edit.duplicate" => self.run_edit(cx, |doc| doc.edit_duplicate()),
            "edit.trim" => self.run_edit(cx, |doc| doc.edit_trim()),
            "edit.roll_left" => self.run_edit(cx, |doc| doc.edit_roll(-1)),
            "edit.roll_right" => self.run_edit(cx, |doc| doc.edit_roll(1)),
            other => return Err(format!("unknown command `{other}`")),
        }
        Ok(())
    }

    fn run_edit(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut BufferDocument)) {
        let Some(id) = self.session.active() else {
            return;
        };
        let Some(views) = self.views.get(&id).cloned() else {
            return;
        };
        views.document.update(cx, |doc, cx| {
            f(doc);
            cx.notify();
        });
        self.playback.sync_from_document(views.document.read(cx));
        self.refresh_explorer(cx);
        self.spawn_peak_build(id, cx);
        cx.notify();
    }

    fn spawn_peak_build(&self, id: DocumentId, cx: &mut Context<Self>) {
        let Some(views) = self.views.get(&id) else {
            return;
        };
        let composition = views.composition.clone();
        if !composition.read().unwrap().needs_peak_build() {
            return;
        }
        if views.document.read(cx).progress.snapshot().is_some() {
            return;
        }
        let progress = views.document.read(cx).progress.clone();
        let epoch = progress.begin("building peaks");
        views.waveform.update(cx, |_, cx| cx.notify());
        std::thread::spawn(move || {
            let result = {
                let composition = composition.read().unwrap();
                composition.build_missing_peak_caches(Some(&progress), epoch)
            };
            match result {
                Ok(updates) => {
                    if !updates.is_empty() {
                        let mut composition = composition.write().unwrap();
                        if progress.is_epoch(epoch) {
                            composition.apply_peak_caches(updates);
                        }
                    }
                }
                Err(err) => {
                    eprintln!("failed to build peaks: {err:#}");
                }
            }
            progress.finish(epoch);
        });
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
        for path in paths {
            self.open_path(path, window, cx);
        }
    }

    fn drain_pending_load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let completed = std::mem::take(&mut *self.pending_load.lock().unwrap());
        for (id, epoch, result) in completed {
            let valid = self
                .views
                .get(&id)
                .is_some_and(|views| views.document.read(cx).progress.is_epoch(epoch));
            if !valid {
                continue;
            }
            match result {
                Ok(composition) => self.push_loaded_composition(id, composition, window, cx),
                Err(err) => {
                    if let Some(views) = self.views.get(&id) {
                        views.document.read(cx).progress.cancel();
                    }
                    self.show_load_error(&err, window, cx);
                    self.close_document(id, window, cx);
                    cx.notify();
                }
            }
        }
    }

    fn open_path(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.session.find_by_path(&path) {
            self.ensure_tab(id, window, cx);
            return;
        }
        self.stop_playback_into_active(cx);
        let composition = Arc::new(RwLock::new(Composition::new(44100, 2)));
        let buffer = Arc::new(RwLock::new(Buffer::empty()));
        let id = self.add_document(composition, buffer, Some(path.clone()), window, cx);
        self.apply_active(window, cx);
        let Some(views) = self.views.get(&id) else {
            return;
        };
        let epoch = views.document.read(cx).progress.begin("opening");
        views.waveform.update(cx, |_, cx| cx.notify());
        cx.notify();
        let pending = self.pending_load.clone();
        std::thread::spawn(move || {
            let result = Composition::load_from_path(&path).map_err(|err| format!("{err:#}"));
            pending.lock().unwrap().push((id, epoch, result));
        });
    }

    fn prompt_open_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Open".into()),
        });
        let view = cx.entity();
        cx.spawn_in(window, async move |_, cx| {
            let paths = match receiver.await {
                Ok(Ok(Some(paths))) => paths,
                _ => return,
            };
            let _ = cx.update(|window, cx| {
                view.update(cx, |this, cx| {
                    for path in paths {
                        this.open_path(path, window, cx);
                    }
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

impl AppView {
    fn app_key_context(&self, window: &mut Window, cx: &mut App) -> KeyContext {
        let mut context = KeyContext::parse("App").expect("App key context");
        let typing = window.focused_input(cx).is_some();
        let over_waveform = self
            .active_views()
            .is_some_and(|views| views.waveform.read(cx).pointer_over());
        if over_waveform && !typing {
            context.add("WaveformHover");
        }
        context
    }
}

impl Render for AppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let transport_state = self.playback.transport_state();
        let views = self.active_views();
        let meta = views
            .as_ref()
            .map(|views| format_header_meta(views.document.read(cx), transport_state))
            .unwrap_or_else(|| {
                format!(
                    "No file open  ·  {}",
                    transport_state_label(transport_state)
                )
            });
        let hover_meta = views.as_ref().and_then(|views| {
            let sample = views.waveform.read(cx).hover_sample()?;
            Some(format_hover_meta(views.document.read(cx), sample))
        });
        let file_status = views
            .as_ref()
            .and_then(|views| FileStatus::from_composition(&views.composition.read().unwrap()));
        let progress_message = views.as_ref().and_then(|views| {
            views
                .document
                .read(cx)
                .progress
                .snapshot()
                .map(|state| state.message())
        });
        let drop_highlight = theme.secondary;
        let explorer_open = self.explorer_dock_open(cx);
        let explorer_icon = if explorer_open {
            IconName::PanelLeft
        } else {
            IconName::PanelLeftOpen
        };
        let explorer_tooltip = if explorer_open {
            "Hide Explorer"
        } else {
            "Show Explorer"
        };
        let console_open = self.script_dock_open(cx);
        let console_icon = if console_open {
            IconName::PanelBottom
        } else {
            IconName::PanelBottomOpen
        };
        let console_tooltip = if console_open {
            "Hide Script"
        } else {
            "Show Script"
        };
        let edits_open = self.edits_dock_open(cx);
        let edits_icon = if edits_open {
            IconName::PanelRight
        } else {
            IconName::PanelRightOpen
        };
        let edits_tooltip = if edits_open {
            "Hide History"
        } else {
            "Show History"
        };

        div()
            .id("app-view")
            .key_context(self.app_key_context(window, cx))
            .track_focus(&self.focus_handle)
            .relative()
            .size_full()
            .drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(drop_highlight))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                for path in paths.paths() {
                    this.open_path(path.clone(), window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleZeroCrossing, _, cx| {
                if let Some(views) = this.active_views() {
                    views.document.update(cx, |doc, _| {
                        doc.toggle_zero_crossing_snap();
                    });
                    cx.notify();
                }
            }))
            .child(
                v_flex()
                    .size_full()
                    .bg(theme.background)
                    .text_color(theme.foreground)
                    .child(
                        TitleBar::new().child(
                            h_flex()
                                .id("app-title-bar-leading")
                                .h_full()
                                .items_center()
                                .gap_2()
                                .when(!cfg!(target_os = "macos"), |this| {
                                    this.child(
                                        Icon::empty()
                                            .path("icons/app-mark.svg")
                                            .with_size(px(16.))
                                            .text_color(theme.foreground),
                                    )
                                })
                                .when_some(self.app_menu_bar.clone(), |this, menu_bar| {
                                    this.child(menu_bar)
                                }),
                        ),
                    )
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
                                Button::new("toggle-explorer-dock")
                                    .ghost()
                                    .small()
                                    .icon(explorer_icon)
                                    .tooltip(explorer_tooltip)
                                    .selected(explorer_open)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.toggle_explorer_dock(window, cx);
                                    })),
                            )
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
                            })
                            .child(
                                Button::new("toggle-script-dock")
                                    .ghost()
                                    .small()
                                    .icon(console_icon)
                                    .tooltip(console_tooltip)
                                    .selected(console_open)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.toggle_script_dock(window, cx);
                                    })),
                            )
                            .child(
                                Button::new("toggle-edits-dock")
                                    .ghost()
                                    .small()
                                    .icon(edits_icon)
                                    .tooltip(edits_tooltip)
                                    .selected(edits_open)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.toggle_edits_dock(window, cx);
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .w_full()
                            .child(self.dock_area.clone()),
                    )
                    .child(FileStatusBar::new(file_status).with_progress_message(progress_message)),
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
        if let Some(views) = self.active_views() {
            views.document.update(cx, |doc, cx| {
                self.playback.sync_document_from_playback(doc);
                cx.notify();
            });
        }
        cx.notify();
    }
}

pub(crate) fn dispatch_command(command_id: &str, cx: &mut App) -> Result<(), String> {
    if command_id == "file.quit" {
        cx.quit();
        return Ok(());
    }
    let Some(view) = cx.try_global::<OpenTarget>().map(|target| target.0.clone()) else {
        return Err("application is not ready".into());
    };
    let Some(window) = cx.active_window() else {
        return Err("no active window".into());
    };
    let command_id = command_id.to_string();
    // Menu and key handlers run inside an update. Defer so path prompts and
    // nested view updates are not attempted on the same tick.
    cx.defer(move |cx| {
        let _ = window.update(cx, |_, window, cx| {
            view.update(cx, |this, cx| this.invoke_command(&command_id, window, cx))
        });
    });
    Ok(())
}

fn quit(_: &Quit, cx: &mut App) {
    let _ = crate::commands::dispatch("file.quit", cx);
}

fn open(_: &Open, cx: &mut App) {
    let _ = crate::commands::dispatch("file.open", cx);
}

fn about(_: &About, cx: &mut App) {
    let _ = crate::commands::dispatch("help.about", cx);
}

fn transport_home(_: &TransportHome, cx: &mut App) {
    let _ = crate::commands::dispatch("transport.home", cx);
}

fn transport_previous(_: &TransportPrevious, cx: &mut App) {
    let _ = crate::commands::dispatch("transport.previous", cx);
}

fn transport_start(_: &TransportStart, cx: &mut App) {
    let _ = crate::commands::dispatch("transport.start", cx);
}

fn transport_play_pause(_: &TransportPlayPause, cx: &mut App) {
    let _ = crate::commands::dispatch("transport.play_pause", cx);
}

fn transport_stop(_: &TransportStop, cx: &mut App) {
    let _ = crate::commands::dispatch("transport.stop", cx);
}

fn transport_next(_: &TransportNext, cx: &mut App) {
    let _ = crate::commands::dispatch("transport.next", cx);
}

fn transport_end(_: &TransportEnd, cx: &mut App) {
    let _ = crate::commands::dispatch("transport.end", cx);
}

fn transport_loop(_: &TransportLoop, cx: &mut App) {
    let _ = crate::commands::dispatch("transport.loop", cx);
}

fn view_fit_all(_: &ViewFitAll, cx: &mut App) {
    let _ = crate::commands::dispatch("view.fit_all", cx);
}

fn view_frame(_: &ViewFrame, cx: &mut App) {
    let _ = crate::commands::dispatch("view.frame", cx);
}

fn view_zoom_in(_: &ViewZoomIn, cx: &mut App) {
    let _ = crate::commands::dispatch("view.zoom_in", cx);
}

fn view_zoom_out(_: &ViewZoomOut, cx: &mut App) {
    let _ = crate::commands::dispatch("view.zoom_out", cx);
}

fn view_explorer(_: &ViewExplorer, cx: &mut App) {
    let _ = crate::commands::dispatch("view.explorer", cx);
}

fn view_history(_: &ViewHistory, cx: &mut App) {
    let _ = crate::commands::dispatch("view.history", cx);
}

fn view_script(_: &ViewScript, cx: &mut App) {
    let _ = crate::commands::dispatch("view.script", cx);
}

fn edit_undo(_: &EditUndo, cx: &mut App) {
    let _ = crate::commands::dispatch("edit.undo", cx);
}

fn edit_redo(_: &EditRedo, cx: &mut App) {
    let _ = crate::commands::dispatch("edit.redo", cx);
}

fn edit_cut(_: &EditCut, cx: &mut App) {
    let _ = crate::commands::dispatch("edit.cut", cx);
}

fn edit_copy(_: &EditCopy, cx: &mut App) {
    let _ = crate::commands::dispatch("edit.copy", cx);
}

fn edit_paste(_: &EditPaste, cx: &mut App) {
    let _ = crate::commands::dispatch("edit.paste", cx);
}

fn edit_delete(_: &EditDelete, cx: &mut App) {
    let _ = crate::commands::dispatch("edit.delete", cx);
}

fn edit_remove(_: &EditRemove, cx: &mut App) {
    let _ = crate::commands::dispatch("edit.remove", cx);
}

fn edit_duplicate(_: &EditDuplicate, cx: &mut App) {
    let _ = crate::commands::dispatch("edit.duplicate", cx);
}

fn edit_trim(_: &EditTrim, cx: &mut App) {
    let _ = crate::commands::dispatch("edit.trim", cx);
}

fn edit_roll_left(_: &EditRollLeft, cx: &mut App) {
    let _ = crate::commands::dispatch("edit.roll_left", cx);
}

fn edit_roll_right(_: &EditRollRight, cx: &mut App) {
    let _ = crate::commands::dispatch("edit.roll_right", cx);
}

fn app_menus(explorer: bool, history: bool, script: bool) -> Vec<Menu> {
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
            MenuItem::action("Explorer", ViewExplorer).checked(explorer),
            MenuItem::action("History", ViewHistory).checked(history),
            MenuItem::action("Script", ViewScript).checked(script),
            MenuItem::separator(),
            MenuItem::action("Zoom In", ViewZoomIn),
            MenuItem::action("Zoom Out", ViewZoomOut),
            MenuItem::action("Reset View", ViewFitAll),
        ]),
        Menu::new("Help").items([MenuItem::action("About...", About)]),
    ]
}

fn apply_app_menus(explorer: bool, history: bool, script: bool, cx: &mut App) {
    cx.set_menus(app_menus(explorer, history, script));
    let owned = app_menus(explorer, history, script)
        .into_iter()
        .map(|menu| menu.owned())
        .collect();
    GlobalState::global_mut(cx).set_app_menus(owned);
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
    cx.on_action(view_explorer);
    cx.on_action(view_history);
    cx.on_action(view_script);
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
    apply_app_menus(false, false, false, cx);
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
    let source_path = initial
        .as_ref()
        .and_then(|composition| composition.pool().first().map(|media| media.path.clone()));
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
                    ..TitleBar::title_bar_options()
                }),
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: point(px(80.), px(80.)),
                    size: size(px(1280.), px(760.)),
                })),
                #[cfg(target_os = "linux")]
                window_decorations: Some(gpui::WindowDecorations::Client),
                ..TitleBar::window_options()
            };

            cx.open_window(options, move |window, cx| {
                let view = cx.new(|cx| {
                    AppView::new(
                        shared_composition.clone(),
                        shared_buffer.clone(),
                        source_path.clone(),
                        playback,
                        pending_opens.clone(),
                        window,
                        cx,
                    )
                });
                cx.set_global(OpenTarget(view.clone()));
                let closer = view.clone();
                cx.set_global(CenterTabCloseHandler {
                    close: Rc::new(move |panel_id, window, cx| {
                        closer.update(cx, |this, cx| {
                            this.close_center_panel(panel_id, window, cx);
                        });
                    }),
                });
                window.focus(&view.focus_handle(cx), cx);
                cx.new(|cx| Root::new(view, window, cx).bg(cx.theme().background))
            })
            .expect("failed to open window");
        })
        .detach();
    });
}
