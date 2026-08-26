//! Real-time V/I/P graph using iced's native `canvas` widget.
//!
//! Uses a **single** canvas widget for all layout modes to avoid wgpu
//! "Buffer still mapped" panics when multiple canvases share a cache.
//!
//! Layout modes:
//! - Combined: all enabled traces overlaid on one chart
//! - SplitVertical: stacked sub-charts (one per enabled trace)
//! - SplitHorizontal: side-by-side sub-charts
//!
//! # Data ownership
//!
//! The graph owns no data.  It renders a *view* of `AppState::samples` — the
//! same buffer CSV export writes out.  Roll and Infinite differ only in which
//! slice of that buffer they show and over what time domain they draw it, so
//! toggling between them mid-run never gains or loses a sample.
//!
//! The one thing that removes data from view is the **Clear** button, which
//! sets a view epoch (`graph_start_time`).  That epoch is honoured identically
//! in both modes.

use std::collections::VecDeque;

use chrono::{DateTime, Duration, Local};
use iced::widget::canvas::{self, Cache, Frame, Geometry, Path, Stroke};
use iced::widget::{canvas as canvas_widget, container};
use iced::{mouse, Color, Element, Length, Point, Rectangle, Size, Theme};

use crate::gui::{Message, Sample, COLOR_CURRENT, COLOR_POWER, COLOR_VOLTAGE};
use crate::settings::{GraphLayout, GraphTimeMode};

/// Graph margins (px).
const MARGIN_TOP: f32 = 12.0;
const MARGIN_BOTTOM: f32 = 30.0;
const MARGIN_LEFT: f32 = 55.0;
const MARGIN_RIGHT: f32 = 60.0;

/// Minimum axis ranges to avoid divide-by-zero on flat data.
const MIN_V_RANGE: f32 = 1.0;
const MIN_I_RANGE: f32 = 0.1;
const MIN_P_RANGE: f32 = 1.0;

const GRID_LINES: usize = 4;

const SUB_GAP: f32 = 4.0;

/// A pause longer than `GAP_FACTOR` poll intervals breaks the polyline, so a
/// disconnect or a paused log reads as a gap instead of a straight line drawn
/// through time when no measurement existed.
const GAP_FACTOR: i64 = 5;

/// Floor for the gap threshold, so ordinary BLE jitter at a fast poll rate does
/// not shred the trace into fragments.
const GAP_FLOOR_MS: i64 = 1_500;

/// Below this width the x axis is labelled at its ends only.
const X_AXIS_FULL_LABELS_W: f32 = 320.0;

/// Build the graph panel element with configurable layout and trace visibility.
#[allow(clippy::too_many_arguments)]
pub fn view_configurable<'a>(
    samples: &'a VecDeque<Sample>,
    cache: &'a Cache,
    layout: GraphLayout,
    show_voltage: bool,
    show_current: bool,
    show_power: bool,
    time_mode: GraphTimeMode,
    time_window_s: u32,
    graph_start_time: Option<DateTime<Local>>,
    poll_interval_ms: u64,
) -> Element<'a, Message> {
    let chart = canvas_widget(GraphCanvas {
        samples,
        cache,
        layout,
        show_voltage,
        show_current,
        show_power,
        time_mode,
        time_window_s,
        graph_start_time,
        poll_interval_ms,
    })
    .width(Length::Fill)
    .height(Length::Fill);

    container(chart)
        .padding(4)
        .style(container::bordered_box)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

// ---- Single canvas that handles all layout modes ------------------------

struct GraphCanvas<'a> {
    samples: &'a VecDeque<Sample>,
    cache: &'a Cache,
    layout: GraphLayout,
    show_voltage: bool,
    show_current: bool,
    show_power: bool,
    time_mode: GraphTimeMode,
    time_window_s: u32,
    graph_start_time: Option<DateTime<Local>>,
    poll_interval_ms: u64,
}

impl<'a> canvas::Program<Message> for GraphCanvas<'a> {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let bg = theme.palette().background;
        let is_dark = (bg.r + bg.g + bg.b) / 3.0 < 0.5;

        // Background is drawn outside the cache so theme changes are reflected
        // immediately without waiting for a cache invalidation.
        let mut bg_frame = Frame::new(renderer, bounds.size());
        bg_frame.fill_rectangle(Point::ORIGIN, bounds.size(), bg);
        let background = bg_frame.into_geometry();

        let geom = self.cache.draw(renderer, bounds.size(), |frame| {
            self.render(frame, bounds.size(), is_dark);
        });
        vec![background, geom]
    }
}

impl<'a> GraphCanvas<'a> {
    fn render(&self, frame: &mut Frame, size: Size, is_dark: bool) {
        // Background is handled by the caller (drawn outside the cache).
        let grid_color = if is_dark {
            Color::from_rgba(1.0, 1.0, 1.0, 0.10)
        } else {
            Color::from_rgba(0.0, 0.0, 0.0, 0.15)
        };
        let axis_color = if is_dark {
            Color::from_rgba(1.0, 1.0, 1.0, 0.55)
        } else {
            Color::from_rgba(0.0, 0.0, 0.0, 0.55)
        };

        let now = Local::now();
        let cutoff = visible_cutoff(now, self.time_mode, self.time_window_s, self.graph_start_time);
        let visible = get_visible(self.samples, cutoff);

        // Roll always has a domain (the window itself) even with no data in it,
        // but there is nothing to draw, so fall through to the placeholder.
        if visible.is_empty() {
            draw_no_data(frame, size, axis_color);
            return;
        }

        let (t_min_ms, span_ms) =
            time_domain(&visible, now, self.time_mode, self.time_window_s, cutoff);
        let gap_ms = gap_threshold_ms(self.poll_interval_ms);

        match self.layout {
            GraphLayout::Combined => {
                self.render_combined(frame, size, &visible, t_min_ms, span_ms, gap_ms, grid_color, axis_color)
            }
            GraphLayout::SplitVertical => {
                self.render_split_vertical(frame, size, &visible, t_min_ms, span_ms, gap_ms, grid_color, axis_color)
            }
            GraphLayout::SplitHorizontal => {
                self.render_split_horizontal(frame, size, &visible, t_min_ms, span_ms, gap_ms, grid_color, axis_color)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_combined(
        &self,
        frame: &mut Frame,
        size: Size,
        visible: &Visible<'_>,
        t_min_ms: i64,
        span_ms: f64,
        gap_ms: i64,
        grid_color: Color,
        axis_color: Color,
    ) {
        let ctx = PlotCtx {
            x_off: MARGIN_LEFT,
            y_off: MARGIN_TOP,
            graph_w: (size.width - MARGIN_LEFT - MARGIN_RIGHT).max(1.0),
            graph_h: (size.height - MARGIN_TOP - MARGIN_BOTTOM).max(1.0),
            t_min_ms,
            span_ms,
            gap_ms,
        };

        draw_grid(frame, ctx.x_off, ctx.y_off, ctx.graph_w, ctx.graph_h, grid_color, axis_color);
        draw_x_axis(frame, &ctx, self.time_mode, axis_color);

        // Count how many right-side axes we need to place
        let mut right_axis_index: usize = 0;

        if self.show_voltage {
            let (v_min, v_max) = auto_range(visible.iter().map(|s| s.voltage), MIN_V_RANGE);
            draw_trace(frame, visible, &ctx, |s| s.voltage, (v_min, v_max), COLOR_VOLTAGE, 2.0);
            draw_y_axis(frame, ctx.x_off, ctx.y_off, ctx.graph_h, v_min, v_max, COLOR_VOLTAGE);
        }
        if self.show_current {
            let (i_min, i_max) = auto_range(visible.iter().map(|s| s.current), MIN_I_RANGE);
            draw_trace(frame, visible, &ctx, |s| s.current, (i_min, i_max), COLOR_CURRENT, 2.0);
            if !self.show_voltage {
                draw_y_axis(frame, ctx.x_off, ctx.y_off, ctx.graph_h, i_min, i_max, COLOR_CURRENT);
            } else {
                draw_y_axis_right(frame, ctx.x_off + ctx.graph_w, ctx.y_off, ctx.graph_h, i_min, i_max, COLOR_CURRENT, right_axis_index);
                right_axis_index += 1;
            }
        }
        if self.show_power {
            let (p_min, p_max) = auto_range(visible.iter().map(|s| s.power), MIN_P_RANGE);
            draw_trace(frame, visible, &ctx, |s| s.power, (p_min, p_max), COLOR_POWER, 1.5);
            if !self.show_voltage && !self.show_current {
                draw_y_axis(frame, ctx.x_off, ctx.y_off, ctx.graph_h, p_min, p_max, COLOR_POWER);
            } else {
                draw_y_axis_right(frame, ctx.x_off + ctx.graph_w, ctx.y_off, ctx.graph_h, p_min, p_max, COLOR_POWER, right_axis_index);
                #[allow(unused_assignments)]
                {
                    right_axis_index += 1;
                }
            }
        }

        draw_legend(frame, size, self.show_voltage, self.show_current, self.show_power);
    }

    #[allow(clippy::too_many_arguments)]
    fn render_split_vertical(
        &self,
        frame: &mut Frame,
        size: Size,
        visible: &Visible<'_>,
        t_min_ms: i64,
        span_ms: f64,
        gap_ms: i64,
        grid_color: Color,
        axis_color: Color,
    ) {
        let traces = self.active_traces();
        if traces.is_empty() {
            return;
        }

        let total_gap = SUB_GAP * (traces.len() as f32 - 1.0).max(0.0);
        let sub_h = ((size.height - total_gap) / traces.len() as f32).max(30.0);
        let last_idx = traces.len() - 1;

        for (idx, trace) in traces.iter().enumerate() {
            let ctx = PlotCtx {
                x_off: MARGIN_LEFT,
                y_off: idx as f32 * (sub_h + SUB_GAP) + MARGIN_TOP,
                graph_w: (size.width - MARGIN_LEFT - 8.0).max(1.0),
                graph_h: (sub_h - MARGIN_TOP - MARGIN_BOTTOM).max(1.0),
                t_min_ms,
                span_ms,
                gap_ms,
            };

            draw_grid(frame, ctx.x_off, ctx.y_off, ctx.graph_w, ctx.graph_h, grid_color, axis_color);
            // Stacked charts share one time domain — label it once, at the bottom.
            if idx == last_idx {
                draw_x_axis(frame, &ctx, self.time_mode, axis_color);
            }

            let (value_fn, min_range, color) = trace.spec();
            let (axis_min, axis_max) = auto_range(visible.iter().map(value_fn), min_range);
            draw_trace(frame, visible, &ctx, value_fn, (axis_min, axis_max), color, 2.0);
            draw_y_axis(frame, ctx.x_off, ctx.y_off, ctx.graph_h, axis_min, axis_max, color);
            draw_trace_label(frame, ctx.x_off + 4.0, ctx.y_off + 2.0, trace.label(), color);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn render_split_horizontal(
        &self,
        frame: &mut Frame,
        size: Size,
        visible: &Visible<'_>,
        t_min_ms: i64,
        span_ms: f64,
        gap_ms: i64,
        grid_color: Color,
        axis_color: Color,
    ) {
        let traces = self.active_traces();
        if traces.is_empty() {
            return;
        }

        let total_gap = SUB_GAP * (traces.len() as f32 - 1.0).max(0.0);
        let sub_w = ((size.width - total_gap) / traces.len() as f32).max(60.0);

        for (idx, trace) in traces.iter().enumerate() {
            let margin_l = 45.0_f32;
            let margin_r = 6.0_f32;
            let ctx = PlotCtx {
                x_off: idx as f32 * (sub_w + SUB_GAP) + margin_l,
                y_off: MARGIN_TOP,
                graph_w: (sub_w - margin_l - margin_r).max(1.0),
                graph_h: (size.height - MARGIN_TOP - MARGIN_BOTTOM).max(1.0),
                t_min_ms,
                span_ms,
                gap_ms,
            };

            draw_grid(frame, ctx.x_off, ctx.y_off, ctx.graph_w, ctx.graph_h, grid_color, axis_color);
            draw_x_axis(frame, &ctx, self.time_mode, axis_color);

            let (value_fn, min_range, color) = trace.spec();
            let (axis_min, axis_max) = auto_range(visible.iter().map(value_fn), min_range);
            draw_trace(frame, visible, &ctx, value_fn, (axis_min, axis_max), color, 2.0);
            draw_y_axis(frame, ctx.x_off, ctx.y_off, ctx.graph_h, axis_min, axis_max, color);
            draw_trace_label(frame, ctx.x_off + 4.0, ctx.y_off + 2.0, trace.label(), color);
        }
    }

    fn active_traces(&self) -> Vec<TraceKind> {
        let mut v = Vec::with_capacity(3);
        if self.show_voltage {
            v.push(TraceKind::Voltage);
        }
        if self.show_current {
            v.push(TraceKind::Current);
        }
        if self.show_power {
            v.push(TraceKind::Power);
        }
        v
    }
}

// ---- Trace enum ---------------------------------------------------------

#[derive(Clone, Copy)]
enum TraceKind {
    Voltage,
    Current,
    Power,
}

impl TraceKind {
    fn label(self) -> &'static str {
        match self {
            Self::Voltage => "V",
            Self::Current => "I",
            Self::Power => "P",
        }
    }

    /// Value accessor, minimum axis span and colour for this trace.
    fn spec(self) -> (fn(&Sample) -> f32, f32, Color) {
        match self {
            Self::Voltage => (|s| s.voltage, MIN_V_RANGE, COLOR_VOLTAGE),
            Self::Current => (|s| s.current, MIN_I_RANGE, COLOR_CURRENT),
            Self::Power => (|s| s.power, MIN_P_RANGE, COLOR_POWER),
        }
    }
}

// ---- Plot geometry ------------------------------------------------------

/// Everything a trace needs to map (time, value) onto the canvas.
///
/// Bundled into a struct rather than threaded through as loose arguments —
/// `draw_trace` would otherwise take a dozen parameters.
#[derive(Clone, Copy, Debug)]
struct PlotCtx {
    x_off: f32,
    y_off: f32,
    graph_w: f32,
    graph_h: f32,
    /// Left edge of the time domain, in epoch milliseconds.
    t_min_ms: i64,
    /// Width of the time domain in milliseconds; always >= 1.0.
    span_ms: f64,
    /// Sample spacing above which the polyline is broken.
    gap_ms: i64,
}

impl PlotCtx {
    /// Fraction of the domain at which `t_ms` falls, clamped to `0..=1`.
    fn frac_of(&self, t_ms: i64) -> f64 {
        (((t_ms - self.t_min_ms) as f64) / self.span_ms).clamp(0.0, 1.0)
    }

    /// Canvas x for an epoch-millisecond timestamp.
    fn x_of(&self, t_ms: i64) -> f32 {
        self.x_off + self.frac_of(t_ms) as f32 * self.graph_w
    }
}

fn gap_threshold_ms(poll_interval_ms: u64) -> i64 {
    (poll_interval_ms as i64).saturating_mul(GAP_FACTOR).max(GAP_FLOOR_MS)
}

// ---- Visible-slice selection --------------------------------------------

/// Oldest timestamp the graph may draw, or `None` for "everything retained".
///
/// `graph_start_time` is the epoch set by the **Clear** button.  It is honoured
/// in *both* modes deliberately: it is the single, mode-independent answer to
/// "which samples has the user asked to stop seeing".  Because Roll additionally
/// bounds by its window, switching Roll -> Infinite mid-run always reveals at
/// least as much as Roll showed, and never less.
fn visible_cutoff(
    now: DateTime<Local>,
    time_mode: GraphTimeMode,
    time_window_s: u32,
    graph_start_time: Option<DateTime<Local>>,
) -> Option<DateTime<Local>> {
    match time_mode {
        GraphTimeMode::Roll => {
            let window_start = now - Duration::seconds(time_window_s as i64);
            Some(match graph_start_time {
                Some(epoch) if epoch > window_start => epoch,
                _ => window_start,
            })
        }
        GraphTimeMode::Infinite => graph_start_time,
    }
}

/// A re-iterable borrow of the retained samples at or after the view cutoff.
///
/// Deliberately *not* a `Vec<&Sample>`: with a 24 h buffer the visible slice
/// runs to hundreds of thousands of samples, and materialising a pointer vector
/// on every redraw would churn megabytes per frame to no purpose.  The buffer is
/// append-only, so a start index is the whole of the state needed.
#[derive(Clone, Copy)]
struct Visible<'a> {
    samples: &'a VecDeque<Sample>,
    start: usize,
}

impl<'a> Visible<'a> {
    fn iter(&self) -> std::collections::vec_deque::Iter<'a, Sample> {
        self.samples.range(self.start..)
    }

    fn len(&self) -> usize {
        self.samples.len() - self.start
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn first(&self) -> Option<&'a Sample> {
        self.samples.get(self.start)
    }

    /// The visible slice always runs to the end of the buffer.
    fn last(&self) -> Option<&'a Sample> {
        self.samples.back()
    }
}

/// Borrow the samples at or after `cutoff`.
///
/// The buffer is append-only in timestamp order, so the cut point is found by
/// binary search rather than by scanning — this runs on every redraw.
fn get_visible(samples: &VecDeque<Sample>, cutoff: Option<DateTime<Local>>) -> Visible<'_> {
    let start = match cutoff {
        None => 0,
        Some(c) => samples.partition_point(|s| s.when < c),
    };
    Visible { samples, start }
}

/// The time domain to draw over, as `(t_min_ms, span_ms)`.
///
/// In Roll the domain is the *window*, anchored to now — not the extent of the
/// data that happens to be in it.  A half-filled window therefore draws on the
/// right-hand half of the canvas instead of stretching to fill it, which is what
/// makes changing the window size visible.
fn time_domain(
    visible: &Visible<'_>,
    now: DateTime<Local>,
    time_mode: GraphTimeMode,
    time_window_s: u32,
    cutoff: Option<DateTime<Local>>,
) -> (i64, f64) {
    let (t_min, t_max) = match time_mode {
        GraphTimeMode::Roll => {
            let start = cutoff.unwrap_or(now - Duration::seconds(time_window_s as i64));
            // Guard against a sample stamped slightly ahead of `now`.
            let end = visible.last().map_or(now, |s| s.when.max(now));
            (start, end)
        }
        // Infinite: the domain is exactly the retained span, so the plot holds
        // still when data stops arriving instead of creeping leftwards.
        GraphTimeMode::Infinite => (
            visible.first().map_or(now, |s| s.when),
            visible.last().map_or(now, |s| s.when),
        ),
    };
    let t_min_ms = t_min.timestamp_millis();
    let span_ms = (t_max.timestamp_millis() - t_min_ms) as f64;
    (t_min_ms, if span_ms >= 1.0 { span_ms } else { 1000.0 })
}

// ---- Decimation ---------------------------------------------------------

/// One pixel column's worth of samples, reduced to its extremes.
#[derive(Clone, Copy)]
struct Bucket {
    col: usize,
    lo: (f32, f32),
    hi: (f32, f32),
    lo_t: i64,
    hi_t: i64,
}

impl Bucket {
    fn new(col: usize, x: f32, v: f32, t: i64) -> Self {
        Self { col, lo: (x, v), hi: (x, v), lo_t: t, hi_t: t }
    }

    fn add(&mut self, x: f32, v: f32, t: i64) {
        if v < self.lo.1 {
            self.lo = (x, v);
            self.lo_t = t;
        }
        if v > self.hi.1 {
            self.hi = (x, v);
            self.hi_t = t;
        }
    }

    /// Emit the column's extremes in timestamp order.
    fn flush_into(&self, out: &mut Vec<(f32, f32)>) {
        if self.lo_t == self.hi_t {
            out.push(self.lo);
        } else if self.lo_t < self.hi_t {
            out.push(self.lo);
            out.push(self.hi);
        } else {
            out.push(self.hi);
            out.push(self.lo);
        }
    }
}

/// Reduce `visible` to at most two points per horizontal pixel, keeping each
/// column's minimum and maximum.
///
/// This replaces the previous "draw only the newest N samples" cap, which
/// silently discarded history: with truncation, a window wider than N samples
/// simply showed nothing extra.  Min/max decimation keeps the whole span on
/// screen at a bounded vertex count and preserves single-sample transients that
/// stride-sampling would step over.
///
/// The returned polylines are split wherever consecutive samples are more than
/// `ctx.gap_ms` apart, so a disconnect is drawn as a gap, not as a line.
fn decimate(
    visible: &Visible<'_>,
    ctx: &PlotCtx,
    value_fn: &impl Fn(&Sample) -> f32,
) -> Vec<Vec<(f32, f32)>> {
    let cols = ctx.graph_w.max(1.0) as usize;
    let mut segments: Vec<Vec<(f32, f32)>> = Vec::new();
    let mut seg: Vec<(f32, f32)> = Vec::new();
    let mut bucket: Option<Bucket> = None;
    let mut prev_t: Option<i64> = None;

    for s in visible.iter() {
        let t = s.when.timestamp_millis();
        let broken = prev_t.is_some_and(|p| t - p > ctx.gap_ms);
        prev_t = Some(t);

        if broken {
            if let Some(b) = bucket.take() {
                b.flush_into(&mut seg);
            }
            if !seg.is_empty() {
                segments.push(std::mem::take(&mut seg));
            }
        }

        let frac = ctx.frac_of(t);
        let col = ((frac * cols as f64) as usize).min(cols - 1);
        let x = ctx.x_off + frac as f32 * ctx.graph_w;
        let v = value_fn(s);

        match bucket.as_mut() {
            Some(b) if b.col == col => b.add(x, v, t),
            _ => {
                if let Some(b) = bucket.take() {
                    b.flush_into(&mut seg);
                }
                bucket = Some(Bucket::new(col, x, v, t));
            }
        }
    }

    if let Some(b) = bucket.take() {
        b.flush_into(&mut seg);
    }
    if !seg.is_empty() {
        segments.push(seg);
    }
    segments
}

// ---- Shared drawing helpers ---------------------------------------------

fn draw_no_data(frame: &mut Frame, size: Size, axis_color: Color) {
    frame.fill_text(canvas::Text {
        content: "Waiting for data…".into(),
        position: Point::new(size.width / 2.0 - 60.0, size.height / 2.0),
        color: axis_color,
        size: 14.0.into(),
        ..Default::default()
    });
}

fn draw_grid(frame: &mut Frame, x_off: f32, y_off: f32, graph_w: f32, graph_h: f32, grid_color: Color, axis_color: Color) {
    let stroke = Stroke::default().with_color(grid_color).with_width(0.5);
    for i in 0..=GRID_LINES {
        let frac = i as f32 / GRID_LINES as f32;
        let y = y_off + frac * graph_h;
        let h_line = Path::line(
            Point::new(x_off, y),
            Point::new(x_off + graph_w, y),
        );
        frame.stroke(&h_line, stroke);
        let x = x_off + frac * graph_w;
        let v_line = Path::line(
            Point::new(x, y_off),
            Point::new(x, y_off + graph_h),
        );
        frame.stroke(&v_line, stroke);
    }
    let border = Path::rectangle(
        Point::new(x_off, y_off),
        Size::new(graph_w, graph_h),
    );
    frame.stroke(&border, Stroke::default().with_color(axis_color).with_width(1.0));
}

fn draw_y_axis(frame: &mut Frame, x_off: f32, y_off: f32, graph_h: f32, min: f32, max: f32, color: Color) {
    for i in 0..=GRID_LINES {
        let frac = i as f32 / GRID_LINES as f32;
        let val = max - frac * (max - min);
        let y = y_off + frac * graph_h;
        frame.fill_text(canvas::Text {
            content: format!("{:.2}", val),
            position: Point::new(x_off - 10.0, y - 6.0),
            color,
            size: 10.0.into(),
            align_x: iced::alignment::Horizontal::Right.into(),
            ..Default::default()
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_y_axis_right(frame: &mut Frame, x_off: f32, y_off: f32, graph_h: f32, min: f32, max: f32, color: Color, index: usize) {
    let offset = 4.0 + index as f32 * 30.0;
    for i in 0..=GRID_LINES {
        let frac = i as f32 / GRID_LINES as f32;
        let val = max - frac * (max - min);
        let y = y_off + frac * graph_h;
        frame.fill_text(canvas::Text {
            content: format!("{:.2}", val),
            position: Point::new(x_off + offset, y - 6.0),
            color,
            size: 10.0.into(),
            ..Default::default()
        });
    }
}

/// Label the time domain under the plot.
///
/// Roll is labelled relative to now (`-60s` … `0`) because the window is what
/// the user set; Infinite is labelled with wall-clock times because its span is
/// whatever has been recorded.  Without these labels the two modes are visually
/// indistinguishable, which is what made the window setting look inert.
fn draw_x_axis(frame: &mut Frame, ctx: &PlotCtx, time_mode: GraphTimeMode, color: Color) {
    let ticks = if ctx.graph_w >= X_AXIS_FULL_LABELS_W { GRID_LINES } else { 1 };
    let y = ctx.y_off + ctx.graph_h + 4.0;
    for i in 0..=ticks {
        let frac = i as f64 / ticks as f64;
        let t_ms = ctx.t_min_ms + (ctx.span_ms * frac) as i64;
        let content = match time_mode {
            GraphTimeMode::Roll => {
                let behind_ms = (ctx.span_ms * (1.0 - frac)) as i64;
                fmt_offset(behind_ms / 1000)
            }
            GraphTimeMode::Infinite => DateTime::from_timestamp_millis(t_ms)
                .map(|t| t.with_timezone(&Local).format("%H:%M:%S").to_string())
                .unwrap_or_default(),
        };
        let (x, align) = if i == 0 {
            (ctx.x_off, iced::alignment::Horizontal::Left)
        } else if i == ticks {
            (ctx.x_off + ctx.graph_w, iced::alignment::Horizontal::Right)
        } else {
            (ctx.x_of(t_ms), iced::alignment::Horizontal::Center)
        };
        frame.fill_text(canvas::Text {
            content,
            position: Point::new(x, y),
            color,
            size: 9.0.into(),
            align_x: align.into(),
            ..Default::default()
        });
    }
}

/// Compact "how long ago" label, e.g. `-1h02m`, `-2m05s`, `-45s`, `0`.
fn fmt_offset(secs: i64) -> String {
    if secs <= 0 {
        "0".to_string()
    } else if secs >= 3600 {
        format!("-{}h{:02}m", secs / 3600, (secs % 3600) / 60)
    } else if secs >= 60 {
        format!("-{}m{:02}s", secs / 60, secs % 60)
    } else {
        format!("-{secs}s")
    }
}

fn draw_trace_label(frame: &mut Frame, x: f32, y: f32, label: &str, color: Color) {
    frame.fill_text(canvas::Text {
        content: label.into(),
        position: Point::new(x, y),
        color,
        size: 12.0.into(),
        ..Default::default()
    });
}

fn draw_trace(
    frame: &mut Frame,
    visible: &Visible<'_>,
    ctx: &PlotCtx,
    value_fn: impl Fn(&Sample) -> f32,
    axis: (f32, f32),
    color: Color,
    width: f32,
) {
    if visible.len() < 2 {
        return;
    }
    let (axis_min, axis_max) = axis;
    let range = (axis_max - axis_min).max(1e-9);
    let y_of = |v: f32| {
        let norm = ((v - axis_min) / range).clamp(0.0, 1.0);
        ctx.y_off + ctx.graph_h - norm * ctx.graph_h
    };
    let stroke = Stroke::default().with_color(color).with_width(width);

    for seg in decimate(visible, ctx, &value_fn) {
        // A run isolated between two gaps has no line to draw — mark it so a
        // lone reading after a dropout is still visible.
        if seg.len() == 1 {
            let (x, v) = seg[0];
            frame.fill(&Path::circle(Point::new(x, y_of(v)), width * 0.6), color);
            continue;
        }
        let path = Path::new(|builder| {
            for (i, &(x, v)) in seg.iter().enumerate() {
                let p = Point::new(x, y_of(v));
                if i == 0 {
                    builder.move_to(p);
                } else {
                    builder.line_to(p);
                }
            }
        });
        frame.stroke(&path, stroke);
    }
}

fn draw_legend(frame: &mut Frame, size: Size, show_v: bool, show_i: bool, show_p: bool) {
    let y = size.height - 6.0;
    let mut x = MARGIN_LEFT;
    let entries: &[(&str, Color, bool)] = &[
        ("V", COLOR_VOLTAGE, show_v),
        ("I", COLOR_CURRENT, show_i),
        ("P", COLOR_POWER, show_p),
    ];
    for &(label, color, visible) in entries {
        if !visible { continue; }
        let line = Path::line(Point::new(x, y), Point::new(x + 14.0, y));
        frame.stroke(&line, Stroke::default().with_color(color).with_width(2.0));
        frame.fill_text(canvas::Text {
            content: label.into(),
            position: Point::new(x + 17.0, y - 5.0),
            color,
            size: 11.0.into(),
            ..Default::default()
        });
        x += 40.0;
    }
}

/// Auto-range with 5% padding, enforcing a minimum span.
fn auto_range(values: impl Iterator<Item = f32>, min_span: f32) -> (f32, f32) {
    let mut lo = f32::MAX;
    let mut hi = f32::MIN;
    for v in values {
        if v < lo { lo = v; }
        if v > hi { hi = v; }
    }
    if lo > hi {
        return (0.0, min_span);
    }
    let span = (hi - lo).max(min_span);
    let pad = span * 0.05;
    (lo - pad, hi + pad)
}

// ---- tests --------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_at(when: DateTime<Local>, voltage: f32) -> Sample {
        Sample {
            when,
            voltage,
            current: 1.0,
            power: voltage,
            resistance: voltage,
            temperature: 25.0,
            runtime_s: 0,
            mode: "CC".into(),
            load_on: true,
        }
    }

    /// `n` samples ending at `now`, spaced `step_ms` apart.
    fn buffer(now: DateTime<Local>, n: usize, step_ms: i64) -> VecDeque<Sample> {
        (0..n)
            .map(|i| {
                let back = (n - 1 - i) as i64 * step_ms;
                sample_at(now - Duration::milliseconds(back), i as f32)
            })
            .collect()
    }

    fn ctx_for(t_min_ms: i64, span_ms: f64, graph_w: f32) -> PlotCtx {
        PlotCtx {
            x_off: 0.0,
            y_off: 0.0,
            graph_w,
            graph_h: 100.0,
            t_min_ms,
            span_ms,
            gap_ms: 1_000,
        }
    }

    // ---- visible slice ---------------------------------------------------

    #[test]
    fn roll_keeps_only_the_window() {
        let now = Local::now();
        // 10 minutes of samples, one per second.
        let samples = buffer(now, 600, 1_000);
        let cutoff = visible_cutoff(now, GraphTimeMode::Roll, 60, None);
        let visible = get_visible(&samples, cutoff);
        // 60 s window at 1 Hz: 61 samples including both boundaries.
        assert!((60..=61).contains(&visible.len()), "got {}", visible.len());
    }

    #[test]
    fn roll_boundary_sample_is_included() {
        let now = Local::now();
        let mut samples = VecDeque::new();
        // Exactly on the cutoff — must be kept, the filter is inclusive.
        samples.push_back(sample_at(now - Duration::seconds(60), 1.0));
        samples.push_back(sample_at(now, 2.0));
        let cutoff = visible_cutoff(now, GraphTimeMode::Roll, 60, None);
        assert_eq!(get_visible(&samples, cutoff).len(), 2);
    }

    #[test]
    fn infinite_shows_the_whole_buffer_without_a_clear() {
        let now = Local::now();
        let samples = buffer(now, 5_000, 200);
        let cutoff = visible_cutoff(now, GraphTimeMode::Infinite, 60, None);
        assert!(cutoff.is_none());
        assert_eq!(get_visible(&samples, cutoff).len(), 5_000);
    }

    /// The regression this whole change exists for: the old code truncated the
    /// visible slice to the newest 600 samples, so Infinite could never show
    /// more than ~2 minutes of a CAP run.
    #[test]
    fn infinite_is_not_capped_at_a_fixed_sample_count() {
        let now = Local::now();
        let samples = buffer(now, 50_000, 200);
        let visible = get_visible(&samples, None);
        assert_eq!(visible.len(), 50_000);
    }

    #[test]
    fn switching_modes_never_loses_samples() {
        let now = Local::now();
        let samples = buffer(now, 3_000, 200); // 10 minutes at 5 Hz

        let roll = get_visible(&samples, visible_cutoff(now, GraphTimeMode::Roll, 60, None));
        let infinite = get_visible(&samples, visible_cutoff(now, GraphTimeMode::Infinite, 60, None));

        // Infinite is a superset of Roll, and the buffer itself is untouched.
        assert!(infinite.len() > roll.len());
        assert_eq!(infinite.len(), samples.len());
        assert!(roll.iter().all(|s| infinite.iter().any(|i| i.when == s.when)));
    }

    // ---- the clear epoch -------------------------------------------------

    #[test]
    fn clear_is_the_only_thing_that_hides_data_in_infinite() {
        let now = Local::now();
        let samples = buffer(now, 600, 1_000); // 10 minutes at 1 Hz

        // No clear pressed: everything retained is visible, whatever the window.
        for window in [5_u32, 60, 3600, 86_400] {
            let cutoff = visible_cutoff(now, GraphTimeMode::Infinite, window, None);
            assert_eq!(get_visible(&samples, cutoff).len(), samples.len());
        }

        // Clear pressed 60 s ago: only samples after the epoch remain visible.
        let epoch = now - Duration::seconds(60);
        let cutoff = visible_cutoff(now, GraphTimeMode::Infinite, 60, Some(epoch));
        let visible = get_visible(&samples, cutoff);
        assert!(visible.len() < samples.len());
        assert!(visible.iter().all(|s| s.when >= epoch));
    }

    #[test]
    fn clear_epoch_applies_in_roll_mode_too() {
        let now = Local::now();
        let samples = buffer(now, 600, 1_000);
        let epoch = now - Duration::seconds(10);

        // Epoch is newer than the window start, so it wins.
        let cutoff = visible_cutoff(now, GraphTimeMode::Roll, 60, Some(epoch));
        assert_eq!(cutoff, Some(epoch));
        assert!(get_visible(&samples, cutoff).iter().all(|s| s.when >= epoch));
    }

    #[test]
    fn old_clear_epoch_does_not_widen_the_roll_window() {
        let now = Local::now();
        let epoch = now - Duration::seconds(600);
        // Epoch is older than the window start — the window still bounds the view.
        let cutoff = visible_cutoff(now, GraphTimeMode::Roll, 60, Some(epoch)).unwrap();
        assert!(cutoff > epoch);
        assert!((now - cutoff).num_seconds() <= 61);
    }

    // ---- time domain -----------------------------------------------------

    #[test]
    fn roll_domain_is_the_window_not_the_data() {
        let now = Local::now();
        // Only 10 s of data in a 60 s window.
        let samples = buffer(now, 10, 1_000);
        let visible = get_visible(&samples, None);
        let cutoff = visible_cutoff(now, GraphTimeMode::Roll, 60, None);
        let (t_min_ms, span_ms) = time_domain(&visible, now, GraphTimeMode::Roll, 60, cutoff);

        // Domain spans the full 60 s window...
        assert!((span_ms - 60_000.0).abs() < 1_500.0, "span {span_ms}");
        // ...so the oldest sample sits in the right-hand portion of the canvas,
        // rather than being stretched to the left edge as the old index-based
        // mapping did.
        let ctx = ctx_for(t_min_ms, span_ms, 600.0);
        let first_x = ctx.x_of(visible.first().unwrap().when.timestamp_millis());
        assert!(first_x > 400.0, "first sample at x={first_x}, expected right side");
    }

    #[test]
    fn wider_roll_window_moves_the_data_right() {
        let now = Local::now();
        let samples = buffer(now, 30, 1_000); // 30 s of data
        let visible = get_visible(&samples, None);

        let x_at = |window: u32| {
            let cutoff = visible_cutoff(now, GraphTimeMode::Roll, window, None);
            let (t_min_ms, span_ms) = time_domain(&visible, now, GraphTimeMode::Roll, window, cutoff);
            ctx_for(t_min_ms, span_ms, 600.0).x_of(visible.first().unwrap().when.timestamp_millis())
        };

        // Changing the window must actually change the picture — under the old
        // index-based x mapping both of these were identical.
        assert!(x_at(600) > x_at(60), "{} vs {}", x_at(600), x_at(60));
    }

    #[test]
    fn infinite_domain_spans_the_retained_data() {
        let now = Local::now();
        let samples = buffer(now, 100, 1_000); // 99 s span
        let visible = get_visible(&samples, None);
        let (_, span_ms) = time_domain(&visible, now, GraphTimeMode::Infinite, 60, None);
        assert!((span_ms - 99_000.0).abs() < 100.0, "span {span_ms}");
    }

    #[test]
    fn degenerate_domain_does_not_divide_by_zero() {
        let now = Local::now();
        let samples: VecDeque<Sample> = std::iter::once(sample_at(now, 1.0)).collect();
        let visible = get_visible(&samples, None);
        let (t_min_ms, span_ms) = time_domain(&visible, now, GraphTimeMode::Infinite, 60, None);
        assert!(span_ms >= 1.0);
        let ctx = ctx_for(t_min_ms, span_ms, 600.0);
        assert!(ctx.x_of(now.timestamp_millis()).is_finite());
    }

    // ---- decimation ------------------------------------------------------

    #[test]
    fn decimation_bounds_vertex_count() {
        let now = Local::now();
        let samples = buffer(now, 100_000, 100);
        let visible = get_visible(&samples, None);
        let span = (visible.last().unwrap().when - visible.first().unwrap().when).num_milliseconds() as f64;
        let ctx = PlotCtx {
            gap_ms: i64::MAX, // one continuous run
            ..ctx_for(visible.first().unwrap().when.timestamp_millis(), span, 800.0)
        };

        let segs = decimate(&visible, &ctx, &|s: &Sample| s.voltage);
        let total: usize = segs.iter().map(|s| s.len()).sum();
        assert_eq!(segs.len(), 1);
        // At most min + max per pixel column.
        assert!(total <= 2 * 800, "emitted {total} points for 800 px");
        assert!(total > 100, "decimation collapsed the trace to {total} points");
    }

    #[test]
    fn decimation_preserves_a_single_sample_spike() {
        let now = Local::now();
        let mut samples = buffer(now, 10_000, 100);
        // Flatten everything, then plant one spike in the middle.
        for s in samples.iter_mut() {
            s.voltage = 1.0;
        }
        samples[5_000].voltage = 42.0;

        let visible = get_visible(&samples, None);
        let span = (visible.last().unwrap().when - visible.first().unwrap().when).num_milliseconds() as f64;
        let ctx = PlotCtx {
            gap_ms: i64::MAX,
            ..ctx_for(visible.first().unwrap().when.timestamp_millis(), span, 400.0)
        };

        let segs = decimate(&visible, &ctx, &|s: &Sample| s.voltage);
        let peak = segs
            .iter()
            .flatten()
            .map(|&(_, v)| v)
            .fold(f32::MIN, f32::max);
        // Truncation or stride-sampling would drop this; min/max keeps it.
        assert_eq!(peak, 42.0);
    }

    #[test]
    fn decimation_passes_sparse_data_through_unchanged() {
        let now = Local::now();
        let samples = buffer(now, 50, 200);
        let visible = get_visible(&samples, None);
        let span = (visible.last().unwrap().when - visible.first().unwrap().when).num_milliseconds() as f64;
        let ctx = ctx_for(visible.first().unwrap().when.timestamp_millis(), span, 800.0);

        let segs = decimate(&visible, &ctx, &|s: &Sample| s.voltage);
        let total: usize = segs.iter().map(|s| s.len()).sum();
        assert_eq!(total, 50, "fewer samples than pixels must not be altered");
    }

    #[test]
    fn gaps_break_the_polyline() {
        let now = Local::now();
        let mut samples = VecDeque::new();
        for i in 0..10 {
            samples.push_back(sample_at(now - Duration::seconds(100 - i), i as f32));
        }
        // 30 s dropout, then data resumes.
        for i in 0..10 {
            samples.push_back(sample_at(now - Duration::seconds(60 - i), i as f32));
        }
        let visible = get_visible(&samples, None);
        let span = (visible.last().unwrap().when - visible.first().unwrap().when).num_milliseconds() as f64;
        let ctx = ctx_for(visible.first().unwrap().when.timestamp_millis(), span, 800.0);

        let segs = decimate(&visible, &ctx, &|s: &Sample| s.voltage);
        assert_eq!(segs.len(), 2, "dropout must not be drawn as a straight line");
    }

    #[test]
    fn gap_threshold_has_a_floor() {
        // A 50 ms poll must not break the trace on ordinary BLE jitter.
        assert_eq!(gap_threshold_ms(50), GAP_FLOOR_MS);
        assert_eq!(gap_threshold_ms(1_000), 5_000);
    }

    #[test]
    fn offset_labels_are_compact() {
        assert_eq!(fmt_offset(0), "0");
        assert_eq!(fmt_offset(45), "-45s");
        assert_eq!(fmt_offset(125), "-2m05s");
        assert_eq!(fmt_offset(3_720), "-1h02m");
    }
}
