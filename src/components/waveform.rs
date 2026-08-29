use std::sync::Arc;

use gpui::{
    canvas, div, fill, point, px, size, Bounds, Context, DispatchPhase, Entity,
    InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement as _, PathBuilder, Pixels, Render, ScrollWheelEvent, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window,
};
use gpui_component::{
    h_flex,
    plot::scale::{Scale as _, ScaleLinear},
    v_flex, ActiveTheme as _, StyledExt as _,
};

use crate::audio;

pub trait WaveformDataProvider: Send + Sync {
    fn sample_rate(&self) -> u32;
    fn channel_count(&self) -> usize;
    fn frames(&self) -> usize;
    fn duration_secs(&self) -> f64;
    fn channel_label(&self, channel: usize) -> String;
    fn channel_samples(&self, channel: usize) -> &[f32];
    fn channel_peaks(&self, channel: usize) -> &[(f32, f32)];

    fn min_max_in_range(&self, channel: usize, start: f64, end: f64) -> (f32, f32) {
        audio::min_max_in_range(
            self.channel_samples(channel),
            self.channel_peaks(channel),
            start,
            end,
        )
    }
}

const ZOOM_FACTOR: f64 = 1.25;
const MIN_SAMPLES_PER_PIXEL: f64 = 1.0 / 50.0;
const MIN_LANE_HEIGHT: f32 = 96.0;
const MIN_THUMB: f32 = 24.0;
const SCROLLBAR_HEIGHT: f32 = 14.0;

enum Drag {
    Waveform { last_x: f32 },
    Scrollbar { grab_offset: f32 },
}

pub struct WaveformDisplay {
    provider: Arc<dyn WaveformDataProvider>,
    start_sample: f64,
    samples_per_pixel: f64,
    viewport_width: f32,
    content_origin_x: f32,
    scrollbar_origin_x: f32,
    scrollbar_width: f32,
    drag: Option<Drag>,
}

impl WaveformDisplay {
    pub fn new(provider: Arc<dyn WaveformDataProvider>) -> Self {
        let samples_per_pixel = if provider.frames() == 0 {
            1.0
        } else {
            provider.frames() as f64 / 1000.0
        };
        Self {
            provider,
            start_sample: 0.0,
            samples_per_pixel: samples_per_pixel.max(MIN_SAMPLES_PER_PIXEL),
            viewport_width: 0.0,
            content_origin_x: 0.0,
            scrollbar_origin_x: 0.0,
            scrollbar_width: 0.0,
            drag: None,
        }
    }

    pub fn zoom_in(&mut self, cx: &mut Context<Self>) {
        let anchor = self.start_sample + self.visible_samples() * 0.5;
        self.zoom_at(1.0 / ZOOM_FACTOR, anchor);
        cx.notify();
    }

    pub fn zoom_out(&mut self, cx: &mut Context<Self>) {
        let anchor = self.start_sample + self.visible_samples() * 0.5;
        self.zoom_at(ZOOM_FACTOR, anchor);
        cx.notify();
    }

    pub fn fit(&mut self, cx: &mut Context<Self>) {
        self.start_sample = 0.0;
        self.samples_per_pixel = self.max_samples_per_pixel();
        cx.notify();
    }

    pub fn visible_duration_label(&self) -> String {
        let secs = self.visible_samples() / f64::from(self.provider.sample_rate());
        format!("{secs:.2} s / view")
    }

    fn max_samples_per_pixel(&self) -> f64 {
        let width = self.viewport_width.max(1.0) as f64;
        (self.provider.frames() as f64 / width).max(MIN_SAMPLES_PER_PIXEL)
    }

    fn visible_samples(&self) -> f64 {
        self.samples_per_pixel * self.viewport_width.max(1.0) as f64
    }

    fn max_start(&self) -> f64 {
        (self.provider.frames() as f64 - self.visible_samples()).max(0.0)
    }

    fn clamp_scroll(&mut self) {
        self.samples_per_pixel = self
            .samples_per_pixel
            .clamp(MIN_SAMPLES_PER_PIXEL, self.max_samples_per_pixel());
        self.start_sample = self.start_sample.clamp(0.0, self.max_start());
    }

    fn zoom_at(&mut self, factor: f64, anchor_sample: f64) {
        let old = self.samples_per_pixel;
        if old <= 0.0 {
            return;
        }
        let pixel = (anchor_sample - self.start_sample) / old;
        self.samples_per_pixel =
            (old * factor).clamp(MIN_SAMPLES_PER_PIXEL, self.max_samples_per_pixel());
        self.start_sample = anchor_sample - pixel * self.samples_per_pixel;
        self.clamp_scroll();
    }

    fn sample_at_x(&self, x: f32) -> f64 {
        let local = (x - self.content_origin_x).max(0.0) as f64;
        self.start_sample + local * self.samples_per_pixel
    }

    fn pan_pixels(&mut self, dx: f32) {
        self.start_sample -= dx as f64 * self.samples_per_pixel;
        self.clamp_scroll();
    }

    fn set_start_from_scrollbar_x(&mut self, x: f32, grab_offset: f32) {
        let track = self.scrollbar_width.max(1.0);
        let (thumb_w, _) = scrollbar_geom(
            self.provider.frames(),
            self.start_sample,
            self.samples_per_pixel,
            track,
        );
        let max_travel = (track - thumb_w).max(1.0);
        let thumb_x = (x - self.scrollbar_origin_x - grab_offset).clamp(0.0, max_travel);
        let max_start = self.max_start();
        self.start_sample = (thumb_x as f64 / max_travel as f64) * max_start;
        self.clamp_scroll();
    }

    fn remember_viewport(&mut self, bounds: Bounds<Pixels>, cx: &mut Context<Self>) {
        let width = bounds.size.width.as_f32();
        if width <= 1.0 {
            return;
        }
        let first = self.viewport_width <= 1.0;
        let changed = (self.viewport_width - width).abs() > 0.5;
        self.viewport_width = width;
        self.content_origin_x = bounds.origin.x.as_f32();
        if first {
            self.start_sample = 0.0;
            self.samples_per_pixel = self.max_samples_per_pixel();
            cx.notify();
        } else if changed {
            self.clamp_scroll();
            cx.notify();
        }
    }

    fn remember_scrollbar(&mut self, bounds: Bounds<Pixels>) {
        self.scrollbar_width = bounds.size.width.as_f32();
        self.scrollbar_origin_x = bounds.origin.x.as_f32();
    }

    fn handle_drag_move(&mut self, x: f32, cx: &mut Context<Self>) {
        match self.drag {
            Some(Drag::Waveform { last_x }) => {
                self.pan_pixels(x - last_x);
                self.drag = Some(Drag::Waveform { last_x: x });
                cx.notify();
            }
            Some(Drag::Scrollbar { grab_offset }) => {
                self.set_start_from_scrollbar_x(x, grab_offset);
                cx.notify();
            }
            None => {}
        }
    }

    fn end_drag(&mut self, cx: &mut Context<Self>) {
        if self.drag.take().is_some() {
            cx.notify();
        }
    }
}

fn install_global_drag_listeners(entity: Entity<WaveformDisplay>, window: &mut Window) {
    window.on_mouse_event({
        let entity = entity.clone();
        move |event: &MouseMoveEvent, phase, _, cx| {
            if phase != DispatchPhase::Capture {
                return;
            }
            entity.update(cx, |this, cx| {
                this.handle_drag_move(event.position.x.as_f32(), cx);
            });
        }
    });
    window.on_mouse_event({
        let entity = entity.clone();
        move |event: &MouseUpEvent, phase, _, cx| {
            if phase != DispatchPhase::Capture || event.button != MouseButton::Left {
                return;
            }
            entity.update(cx, |this, cx| {
                this.end_drag(cx);
            });
        }
    });
}

fn scrollbar_geom(frames: usize, start: f64, spp: f64, track: f32) -> (f32, f32) {
    if frames == 0 || track <= 0.0 {
        return (track, 0.0);
    }
    let visible = (spp * track as f64).max(1.0);
    let ratio = (visible / frames as f64).clamp(0.0, 1.0) as f32;
    let thumb_w = (track * ratio).max(MIN_THUMB).min(track);
    let max_start = (frames as f64 - visible).max(0.0);
    let thumb_x = if max_start <= f64::EPSILON {
        0.0
    } else {
        (start / max_start) as f32 * (track - thumb_w)
    };
    (thumb_w, thumb_x)
}

fn channel_color(theme: &gpui_component::Theme, index: usize) -> gpui::Hsla {
    match index % 5 {
        0 => theme.chart_1,
        1 => theme.chart_2,
        2 => theme.chart_3,
        3 => theme.chart_4,
        _ => theme.chart_5,
    }
}

fn paint_lane(
    bounds: Bounds<Pixels>,
    provider: &dyn WaveformDataProvider,
    channel: usize,
    start_sample: f64,
    samples_per_pixel: f64,
    color: gpui::Hsla,
    zero_color: gpui::Hsla,
    window: &mut Window,
) {
    let width = bounds.size.width.as_f32();
    let height = bounds.size.height.as_f32();
    if width < 1.0 || height < 1.0 || channel >= provider.channel_count() {
        return;
    }

    let origin_x = bounds.origin.x.as_f32();
    let origin_y = bounds.origin.y.as_f32();
    let y_scale = ScaleLinear::new(vec![-1.0_f64, 1.0], vec![origin_y + height, origin_y]);

    if let Some(mid) = y_scale.tick(&0.0) {
        let mut builder = PathBuilder::stroke(px(1.0));
        builder.move_to(point(px(origin_x), px(mid)));
        builder.line_to(point(px(origin_x + width), px(mid)));
        if let Ok(path) = builder.build() {
            window.paint_path(path, zero_color);
        }
    }

    let samples = provider.channel_samples(channel);
    let cols = width.ceil() as usize;

    if samples_per_pixel < 1.0 {
        let mut builder = PathBuilder::stroke(px(1.2));
        let mut started = false;
        let first = start_sample.max(0.0).floor() as usize;
        let last = ((start_sample + width as f64 * samples_per_pixel).ceil() as usize)
            .min(samples.len().saturating_sub(1));
        for i in first..=last {
            let x = origin_x + ((i as f64 - start_sample) / samples_per_pixel) as f32;
            let y = y_scale
                .tick(&(samples[i] as f64))
                .unwrap_or(origin_y + height * 0.5);
            if !started {
                builder.move_to(point(px(x), px(y)));
                started = true;
            } else {
                builder.line_to(point(px(x), px(y)));
            }
        }
        if let Ok(path) = builder.build() {
            window.paint_path(path, color);
        }
        return;
    }

    for col in 0..cols {
        let bin_start = start_sample + col as f64 * samples_per_pixel;
        let bin_end = bin_start + samples_per_pixel;
        if bin_start >= samples.len() as f64 {
            break;
        }
        let (min, max) = provider.min_max_in_range(channel, bin_start, bin_end);
        let y_max = y_scale.tick(&(max as f64)).unwrap_or(origin_y);
        let y_min = y_scale.tick(&(min as f64)).unwrap_or(origin_y + height);
        let top = y_max.min(y_min);
        let bar_h = (y_max - y_min).abs().max(1.0);
        window.paint_quad(fill(
            Bounds {
                origin: point(px(origin_x + col as f32), px(top)),
                size: size(px(1.0), px(bar_h)),
            },
            color,
        ));
    }
}

fn paint_scrollbar(
    bounds: Bounds<Pixels>,
    frames: usize,
    start_sample: f64,
    samples_per_pixel: f64,
    track: gpui::Hsla,
    thumb: gpui::Hsla,
    window: &mut Window,
) {
    window.paint_quad(fill(bounds, track));
    let (thumb_w, thumb_x) = scrollbar_geom(
        frames,
        start_sample,
        samples_per_pixel,
        bounds.size.width.as_f32(),
    );
    window.paint_quad(fill(
        Bounds {
            origin: point(px(bounds.origin.x.as_f32() + thumb_x), bounds.origin.y),
            size: size(px(thumb_w), bounds.size.height),
        },
        thumb,
    ));
}

impl Render for WaveformDisplay {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let provider = self.provider.clone();
        let start_sample = self.start_sample;
        let samples_per_pixel = self.samples_per_pixel;
        let entity = cx.entity();

        v_flex()
            .size_full()
            .child(
                div()
                    .id("waveform")
                    .flex_1()
                    .w_full()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .w_full()
                            .h_full()
                            .children((0..provider.channel_count()).map(|ch| {
                                let provider = provider.clone();
                                let entity = entity.clone();
                                let color = channel_color(&theme, ch);
                                let zero = theme.border;
                                let label = provider.channel_label(ch);
                                h_flex()
                                    .id(SharedString::from(format!("lane-{ch}")))
                                    .w_full()
                                    .flex_1()
                                    .min_h(px(MIN_LANE_HEIGHT))
                                    .border_b_1()
                                    .border_color(theme.border)
                                    .child(
                                        div()
                                            .w(px(48.))
                                            .flex_none()
                                            .h_full()
                                            .items_center()
                                            .justify_center()
                                            .border_r_1()
                                            .border_color(theme.border)
                                            .text_xs()
                                            .font_semibold()
                                            .text_color(theme.muted_foreground)
                                            .child(label),
                                    )
                                    .child(
                                        canvas(
                                            {
                                                let entity = entity.clone();
                                                move |bounds, _, cx| {
                                                    entity.update(cx, |this, cx| {
                                                        this.remember_viewport(bounds, cx);
                                                    });
                                                    bounds
                                                }
                                            },
                                            {
                                                let provider = provider.clone();
                                                move |bounds, _, window, _cx| {
                                                    paint_lane(
                                                        bounds,
                                                        provider.as_ref(),
                                                        ch,
                                                        start_sample,
                                                        samples_per_pixel,
                                                        color,
                                                        zero,
                                                        window,
                                                    );
                                                }
                                            },
                                        )
                                        .flex_1()
                                        .h_full(),
                                    )
                            })),
                    )
                    .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                        let delta = event.delta.pixel_delta(px(16.));
                        let dx = delta.x.as_f32();
                        let dy = delta.y.as_f32();
                        if event.modifiers.control || event.modifiers.platform {
                            let mag = if dx.abs() > dy.abs() { dx } else { dy };
                            let factor = if mag < 0.0 {
                                1.0 / ZOOM_FACTOR
                            } else {
                                ZOOM_FACTOR
                            };
                            let anchor = this.sample_at_x(event.position.x.as_f32());
                            this.zoom_at(factor, anchor);
                        } else {
                            let pan = if dx.abs() > dy.abs() { dx } else { dy };
                            this.pan_pixels(pan);
                        }
                        cx.notify();
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _, cx| {
                            this.drag = Some(Drag::Waveform {
                                last_x: event.position.x.as_f32(),
                            });
                            cx.notify();
                        }),
                    ),
            )
            .child({
                let frames = provider.frames();
                let start = start_sample;
                let spp = samples_per_pixel;
                let track = theme.scrollbar;
                let thumb = theme.scrollbar_thumb;
                let entity = entity.clone();
                h_flex()
                    .id("h-scroll")
                    .w_full()
                    .h(px(SCROLLBAR_HEIGHT))
                    .flex_none()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .w(px(48.))
                            .h_full()
                            .flex_none()
                            .border_r_1()
                            .border_color(theme.border),
                    )
                    .child(
                        div()
                            .id("h-scroll-track")
                            .flex_1()
                            .h_full()
                            .child(
                                canvas(
                                    {
                                        let entity = entity.clone();
                                        move |bounds, _, cx| {
                                            entity.update(cx, |this, _cx| {
                                                this.remember_scrollbar(bounds);
                                            });
                                            bounds
                                        }
                                    },
                                    move |bounds, _, window, _cx| {
                                        paint_scrollbar(
                                            bounds, frames, start, spp, track, thumb, window,
                                        );
                                        install_global_drag_listeners(entity.clone(), window);
                                    },
                                )
                                .size_full(),
                            )
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                                    let x = event.position.x.as_f32();
                                    let track_w = this.scrollbar_width.max(1.0);
                                    let (thumb_w, thumb_x) = scrollbar_geom(
                                        this.provider.frames(),
                                        this.start_sample,
                                        this.samples_per_pixel,
                                        track_w,
                                    );
                                    let local = x - this.scrollbar_origin_x;
                                    let grab_offset =
                                        if local >= thumb_x && local <= thumb_x + thumb_w {
                                            local - thumb_x
                                        } else {
                                            thumb_w * 0.5
                                        };
                                    this.set_start_from_scrollbar_x(x, grab_offset);
                                    this.drag = Some(Drag::Scrollbar { grab_offset });
                                    cx.notify();
                                }),
                            ),
                    )
            })
    }
}
