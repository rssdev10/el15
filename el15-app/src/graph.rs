//! Real-time V/I/P graph using iced's native `canvas` widget.
//!
//! Uses a **single** canvas widget for all layout modes to avoid wgpu
//! "Buffer still mapped" panics when multiple canvases share a cache.
//!
//! Layout modes:
//! - Combined: all enabled traces overlaid on one chart
//! - SplitVertical: stacked sub-charts (one per enabled trace)
//! - SplitHorizontal: side-by-side sub-charts

use std::collections::VecDeque;

use chrono::{DateTime, Local};
use iced::widget::canvas::{self, Cache, Frame, Geometry, Path, Stroke};
use iced::widget::{canvas as canvas_widget, container};
use iced::{mouse, Color, Element, Length, Point, Rectangle, Size, Theme};

use crate::gui::{Message, Sample, COLOR_CURRENT, COLOR_POWER, COLOR_VOLTAGE};
use crate::settings::{GraphLayout, GraphTimeMode};

/// How many samples to display on the graph at once.
const VISIBLE_SAMPLES: usize = 600;

/// Graph margins (px).
const MARGIN_TOP: f32 = 12.0;
const MARGIN_BOTTOM: f32 = 20.0;
const MARGIN_LEFT: f32 = 55.0;
const MARGIN_RIGHT: f32 = 60.0;

/// Minimum axis ranges to avoid divide-by-zero on flat data.
const MIN_V_RANGE: f32 = 1.0;
const MIN_I_RANGE: f32 = 0.1;
const MIN_P_RANGE: f32 = 1.0;

const GRID_LINES: usize = 4;


const SUB_GAP: f32 = 4.0;

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

        let visible = get_visible(self.samples, self.time_mode, self.time_window_s, self.graph_start_time);
        if visible.is_empty() {
            draw_no_data(frame, size, axis_color);
            return;
        }

        match self.layout {
            GraphLayout::Combined => self.render_combined(frame, size, &visible, grid_color, axis_color),
            GraphLayout::SplitVertical => self.render_split_vertical(frame, size, &visible, grid_color, axis_color),
            GraphLayout::SplitHorizontal => self.render_split_horizontal(frame, size, &visible, grid_color, axis_color),
        }
    }

    fn render_combined(&self, frame: &mut Frame, size: Size, visible: &[&Sample], grid_color: Color, axis_color: Color) {
        let graph_w = (size.width - MARGIN_LEFT - MARGIN_RIGHT).max(1.0);
        let graph_h = (size.height - MARGIN_TOP - MARGIN_BOTTOM).max(1.0);
        let n = visible.len();

        draw_grid(frame, MARGIN_LEFT, MARGIN_TOP, graph_w, graph_h, grid_color, axis_color);

        // Count how many right-side axes we need to place
        let mut right_axis_index: usize = 0;

        if self.show_voltage {
            let (v_min, v_max) = auto_range(visible.iter().map(|s| s.voltage), MIN_V_RANGE);
            draw_trace(frame, visible, n, MARGIN_LEFT, MARGIN_TOP, graph_w, graph_h, |s| s.voltage, v_min, v_max, COLOR_VOLTAGE, 2.0);
            draw_y_axis(frame, MARGIN_LEFT, MARGIN_TOP, graph_h, v_min, v_max, COLOR_VOLTAGE);
        }
        if self.show_current {
            let (i_min, i_max) = auto_range(visible.iter().map(|s| s.current), MIN_I_RANGE);
            draw_trace(frame, visible, n, MARGIN_LEFT, MARGIN_TOP, graph_w, graph_h, |s| s.current, i_min, i_max, COLOR_CURRENT, 2.0);
            if !self.show_voltage {
                draw_y_axis(frame, MARGIN_LEFT, MARGIN_TOP, graph_h, i_min, i_max, COLOR_CURRENT);
            } else {
                draw_y_axis_right(frame, MARGIN_LEFT + graph_w, MARGIN_TOP, graph_h, i_min, i_max, COLOR_CURRENT, right_axis_index);
                right_axis_index += 1;
            }
        }
        if self.show_power {
            let (p_min, p_max) = auto_range(visible.iter().map(|s| s.power), MIN_P_RANGE);
            draw_trace(frame, visible, n, MARGIN_LEFT, MARGIN_TOP, graph_w, graph_h, |s| s.power, p_min, p_max, COLOR_POWER, 1.5);
            if !self.show_voltage && !self.show_current {
                draw_y_axis(frame, MARGIN_LEFT, MARGIN_TOP, graph_h, p_min, p_max, COLOR_POWER);
            } else {
                draw_y_axis_right(frame, MARGIN_LEFT + graph_w, MARGIN_TOP, graph_h, p_min, p_max, COLOR_POWER, right_axis_index);
                #[allow(unused_assignments)]
                { right_axis_index += 1; }
            }
        }

        draw_legend(frame, size, self.show_voltage, self.show_current, self.show_power);
    }

    fn render_split_vertical(&self, frame: &mut Frame, size: Size, visible: &[&Sample], grid_color: Color, axis_color: Color) {
        let traces = self.active_traces();
        if traces.is_empty() { return; }

        let total_gap = SUB_GAP * (traces.len() as f32 - 1.0).max(0.0);
        let sub_h = ((size.height - total_gap) / traces.len() as f32).max(30.0);

        for (idx, trace) in traces.iter().enumerate() {
            let y_off = idx as f32 * (sub_h + SUB_GAP);
            let graph_w = (size.width - MARGIN_LEFT - 8.0).max(1.0);
            let inner_h = (sub_h - MARGIN_TOP - MARGIN_BOTTOM).max(1.0);
            let n = visible.len();

            draw_grid(frame, MARGIN_LEFT, y_off + MARGIN_TOP, graph_w, inner_h, grid_color, axis_color);

            let (value_fn, min_range, color): (fn(&Sample) -> f32, f32, Color) = match trace {
                TraceKind::Voltage => (|s| s.voltage, MIN_V_RANGE, COLOR_VOLTAGE),
                TraceKind::Current => (|s| s.current, MIN_I_RANGE, COLOR_CURRENT),
                TraceKind::Power => (|s| s.power, MIN_P_RANGE, COLOR_POWER),
            };

            let (axis_min, axis_max) = auto_range(visible.iter().map(|s| value_fn(s)), min_range);
            draw_trace(frame, visible, n, MARGIN_LEFT, y_off + MARGIN_TOP, graph_w, inner_h, value_fn, axis_min, axis_max, color, 2.0);
            draw_y_axis(frame, MARGIN_LEFT, y_off + MARGIN_TOP, inner_h, axis_min, axis_max, color);
            draw_trace_label(frame, MARGIN_LEFT + 4.0, y_off + MARGIN_TOP + 2.0, trace.label(), color);
        }
    }

    fn render_split_horizontal(&self, frame: &mut Frame, size: Size, visible: &[&Sample], grid_color: Color, axis_color: Color) {
        let traces = self.active_traces();
        if traces.is_empty() { return; }

        let total_gap = SUB_GAP * (traces.len() as f32 - 1.0).max(0.0);
        let sub_w = ((size.width - total_gap) / traces.len() as f32).max(60.0);

        for (idx, trace) in traces.iter().enumerate() {
            let x_off = idx as f32 * (sub_w + SUB_GAP);
            let margin_l = 45.0_f32;
            let margin_r = 6.0_f32;
            let graph_w = (sub_w - margin_l - margin_r).max(1.0);
            let graph_h = (size.height - MARGIN_TOP - MARGIN_BOTTOM).max(1.0);
            let n = visible.len();

            draw_grid(frame, x_off + margin_l, MARGIN_TOP, graph_w, graph_h, grid_color, axis_color);

            let (value_fn, min_range, color): (fn(&Sample) -> f32, f32, Color) = match trace {
                TraceKind::Voltage => (|s| s.voltage, MIN_V_RANGE, COLOR_VOLTAGE),
                TraceKind::Current => (|s| s.current, MIN_I_RANGE, COLOR_CURRENT),
                TraceKind::Power => (|s| s.power, MIN_P_RANGE, COLOR_POWER),
            };

            let (axis_min, axis_max) = auto_range(visible.iter().map(|s| value_fn(s)), min_range);
            draw_trace(frame, visible, n, x_off + margin_l, MARGIN_TOP, graph_w, graph_h, value_fn, axis_min, axis_max, color, 2.0);
            draw_y_axis(frame, x_off + margin_l, MARGIN_TOP, graph_h, axis_min, axis_max, color);
            draw_trace_label(frame, x_off + margin_l + 4.0, MARGIN_TOP + 2.0, trace.label(), color);
        }
    }

    fn active_traces(&self) -> Vec<TraceKind> {
        let mut v = Vec::with_capacity(3);
        if self.show_voltage { v.push(TraceKind::Voltage); }
        if self.show_current { v.push(TraceKind::Current); }
        if self.show_power { v.push(TraceKind::Power); }
        v
    }
}

// ---- Trace enum ---------------------------------------------------------

#[derive(Clone, Copy)]
enum TraceKind { Voltage, Current, Power }

impl TraceKind {
    fn label(self) -> &'static str {
        match self {
            Self::Voltage => "V",
            Self::Current => "I",
            Self::Power => "P",
        }
    }
}

// ---- Shared drawing helpers ---------------------------------------------

fn get_visible(
    samples: &VecDeque<Sample>,
    time_mode: GraphTimeMode,
    time_window_s: u32,
    graph_start_time: Option<DateTime<Local>>,
) -> Vec<&Sample> {
    let filtered: Vec<&Sample> = match time_mode {
        GraphTimeMode::Roll => {
            let cutoff = Local::now() - chrono::Duration::seconds(time_window_s as i64);
            samples.iter().filter(|s| s.when >= cutoff).collect()
        }
        GraphTimeMode::Infinite => {
            if let Some(start) = graph_start_time {
                samples.iter().filter(|s| s.when >= start).collect()
            } else {
                samples.iter().collect()
            }
        }
    };
    let total = filtered.len();
    if total <= VISIBLE_SAMPLES {
        filtered
    } else {
        filtered.into_iter().skip(total - VISIBLE_SAMPLES).collect()
    }
}

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

fn draw_trace_label(frame: &mut Frame, x: f32, y: f32, label: &str, color: Color) {
    frame.fill_text(canvas::Text {
        content: label.into(),
        position: Point::new(x, y),
        color,
        size: 12.0.into(),
        ..Default::default()
    });
}

#[allow(clippy::too_many_arguments)]
fn draw_trace(
    frame: &mut Frame,
    visible: &[&Sample],
    n: usize,
    x_off: f32,
    y_off: f32,
    graph_w: f32,
    graph_h: f32,
    value_fn: impl Fn(&Sample) -> f32,
    axis_min: f32,
    axis_max: f32,
    color: Color,
    width: f32,
) {
    if n < 2 { return; }
    let range = (axis_max - axis_min).max(1e-9);
    let path = Path::new(|builder| {
        for (i, s) in visible.iter().enumerate() {
            let x = x_off + (i as f32 / (n - 1) as f32) * graph_w;
            let norm = ((value_fn(s) - axis_min) / range).clamp(0.0, 1.0);
            let y = y_off + graph_h - norm * graph_h;
            if i == 0 {
                builder.move_to(Point::new(x, y));
            } else {
                builder.line_to(Point::new(x, y));
            }
        }
    });
    frame.stroke(&path, Stroke::default().with_color(color).with_width(width));
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
