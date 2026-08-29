use std::{path::PathBuf, sync::Arc};

use gpui::{
    actions, div, point, prelude::FluentBuilder as _, px, size, App, AppContext as _, Bounds,
    Context, Entity, InteractiveElement as _, IntoElement, KeyBinding, Menu, MenuItem,
    ParentElement as _, Render, SharedString, Styled as _, TitlebarOptions, Window,
    WindowBounds, WindowOptions,
};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::AppMenuBar,
    v_flex, ActiveTheme as _, GlobalState, Root, Sizable as _, Theme, ThemeMode, WindowExt as _,
    TITLE_BAR_HEIGHT,
};

actions!(snd_review, [About, Quit]);

use crate::audio::DecodedAudio;
use crate::components::waveform::WaveformDisplay;

pub struct AppView {
    audio: Arc<DecodedAudio>,
    waveform: Entity<WaveformDisplay>,
    app_menu_bar: Option<Entity<AppMenuBar>>,
}

impl AppView {
    fn new(audio: Arc<DecodedAudio>, cx: &mut App) -> Self {
        let waveform = cx.new(|_| WaveformDisplay::new(audio.clone()));
        Self {
            audio,
            waveform,
            app_menu_bar: (!cfg!(target_os = "macos")).then(|| AppMenuBar::new(cx)),
        }
    }
}

fn format_duration(secs: f64) -> String {
    if secs.is_nan() || secs.is_infinite() {
        return "0.00s".into();
    }
    if secs < 60.0 {
        format!("{secs:.2}s")
    } else {
        let m = (secs / 60.0).floor() as u32;
        let s = secs % 60.0;
        format!("{m}m {s:05.2}s")
    }
}

impl Render for AppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let waveform = self.waveform.clone();

        let meta = format!(
            "{} Hz  ·  {} ch  ·  {}  ·  {}",
            self.audio.sample_rate,
            self.audio.channel_count(),
            format_duration(self.audio.duration_secs()),
            self.waveform.read(cx).visible_duration_label(),
        );

        div()
            .relative()
            .size_full()
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
                            .child(Button::new("zoom-out").small().label("Zoom Out").on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.waveform.update(cx, |view, cx| view.zoom_out(cx));
                                }),
                            ))
                            .child(Button::new("zoom-in").small().label("Zoom In").on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.waveform.update(cx, |view, cx| view.zoom_in(cx));
                                }),
                            ))
                            .child(
                                Button::new("zoom-fit")
                                    .small()
                                    .primary()
                                    .label("Fit")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.waveform.update(cx, |view, cx| view.fit(cx));
                                    })),
                            ),
                    )
                    .child(div().flex_1().min_h_0().w_full().child(waveform)),
            )
            .children(Root::render_dialog_layer(window, cx))
    }
}

fn quit(_: &Quit, cx: &mut App) {
    cx.quit();
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

fn app_menus() -> Vec<Menu> {
    vec![Menu::new("snd-review").items([
        MenuItem::action("About", About),
        MenuItem::separator(),
        MenuItem::action("Quit", Quit),
    ])]
}

fn install_app_menu(cx: &mut App) {
    cx.on_action(quit);
    cx.on_action(about);
    cx.bind_keys([
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-q", Quit, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("alt-f4", Quit, None),
    ]);
    cx.set_menus(app_menus());
    let owned = app_menus().into_iter().map(|menu| menu.owned()).collect();
    GlobalState::global_mut(cx).set_app_menus(owned);
    cx.activate(true);
}

pub fn run(audio: Arc<DecodedAudio>, path: PathBuf) {
    let title: SharedString = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
        .into();

    gpui_platform::application()
        .with_assets(gpui_component_assets::Assets)
        .run(move |cx| {
            gpui_component::init(cx);
            Theme::change(ThemeMode::Dark, None, cx);
            install_app_menu(cx);

            let title = title.clone();
            let audio = audio.clone();
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
                    let view = cx.new(|cx| AppView::new(audio.clone(), cx));
                    cx.new(|cx| Root::new(view, window, cx).bg(cx.theme().background))
                })
                .expect("failed to open window");
            })
            .detach();
        });
}
