use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Chart, Dataset, GraphType, Paragraph, Widget};
use ratatui::Frame;

use crate::artifact::{CellValue, TableData};
use crate::config::{parse_color, HighlightRule, SummarySection, TablesConfig};
use crate::model::{MetricAggregate, Run, RunParam, ScalarMetric};
use crate::ui::theme::Theme;

/// Compute `(x_min, x_max)` for a curve chart with optional declared total_steps.
///
/// `x_min` is always `0.0`. `x_max` is `total_steps - 1` when declared (and > 0),
/// extended to `observed_x_max` on overflow. When `total_steps` is None, falls
/// back to `observed_x_max`. Both branches floor at `1.0` to keep the chart
/// widget happy in the degenerate "single point at step 0" case.
pub(crate) fn pinned_x_bounds(observed_x_max: f64, total_steps: Option<i64>) -> (f64, f64) {
    let x_max = match total_steps.filter(|n| *n > 0) {
        Some(n) => ((n - 1) as f64).max(observed_x_max).max(1.0),
        None => observed_x_max.max(1.0),
    };
    (0.0, x_max)
}

/// All data needed to render a summary panel.
pub struct SummaryData<'a> {
    pub name: &'a str,
    pub runs: &'a [Run],
    pub run_metrics: &'a [Vec<ScalarMetric>],
    pub aggregate_metrics: &'a [MetricAggregate],
    pub unique_configs: i64,
    pub run_params: &'a [RunParam],
    pub metric_histories: &'a [(String, Vec<ScalarMetric>)],
    pub table: Option<&'a TableData>,
    pub table_title: Option<&'a str>,
    pub table_axes: Option<(&'a str, &'a str)>,
    /// If set, the curve chart's X axis is pinned at [0, total_steps - 1]
    /// (extending if observed steps overflow). If None, falls back to the
    /// legacy auto-fit-to-max-step behavior.
    pub preview_total_steps: Option<i64>,
    pub tags: Option<&'a str>,
    pub tag_edit: Option<&'a str>,
}

pub struct SummaryRenderer {
    theme: Theme,
}

impl SummaryRenderer {
    pub fn new(theme: Theme) -> Self {
        Self {
            theme,
        }
    }

    /// Render the summary panel. Returns total line count for scroll tracking.
    pub fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        data: &SummaryData,
        sections: &[SummarySection],
        scroll_offset: u16,
        curve_width_pct: u8,
        curve_height: Option<u16>,
        curve_smooth: bool,
        tables_config: &TablesConfig,
    ) -> usize {
        let mut lines: Vec<Line<'static>> = Vec::new();

        self.build_header(&mut lines, data);

        for section in sections {
            match section {
                SummarySection::Runs => self.build_runs(&mut lines, data),
                SummarySection::Metrics => self.build_metrics(&mut lines, data),
                SummarySection::Curves => {
                    let chart_width =
                        ((area.width as f32) * (curve_width_pct.min(100) as f32 / 100.0)) as u16;
                    self.build_curves(&mut lines, data, chart_width.max(20), curve_height, curve_smooth);
                }
                SummarySection::Tables => {
                    self.build_tables(&mut lines, data, tables_config);
                }
            }
        }

        lines.push(Line::from(""));

        let total_lines = lines.len();
        let visible_height = area.height as usize;

        let paragraph = Paragraph::new(lines).scroll((scroll_offset, 0));
        frame.render_widget(paragraph, area);

        if total_lines > visible_height {
            if scroll_offset > 0 {
                let hint = Paragraph::new(Line::from(Span::styled(
                    " \u{25b2} more above",
                    Style::default().fg(self.theme.accent_dim),
                )));
                frame.render_widget(hint, Rect::new(area.x, area.y, area.width, 1));
            }
            let at_bottom = scroll_offset as usize + visible_height >= total_lines;
            if !at_bottom {
                let hint = Paragraph::new(Line::from(Span::styled(
                    " \u{25bc} more below",
                    Style::default().fg(self.theme.accent_dim),
                )));
                let y = area.y + area.height.saturating_sub(1);
                frame.render_widget(hint, Rect::new(area.x, y, area.width, 1));
            }
        }

        total_lines
    }

    fn build_header(&self, lines: &mut Vec<Line<'static>>, data: &SummaryData) {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  {}", data.name),
            self.theme.header,
        )));

        let run_count = data.runs.len();
        let config_hint = if data.unique_configs > 0 {
            format!(" \u{00b7} {} unique configs", data.unique_configs)
        } else {
            String::new()
        };
        lines.push(Line::from(format!(
            "  {} {}{config_hint}",
            run_count,
            if run_count == 1 { "run" } else { "runs" }
        )));

        // Tags row (editable or display)
        if let Some(edit_text) = data.tag_edit {
            lines.push(Line::from(vec![
                Span::styled("  Tags: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(
                    edit_text.to_string(),
                    Style::default().fg(self.theme.accent),
                ),
                Span::styled("_", Style::default().add_modifier(Modifier::SLOW_BLINK)),
            ]));
        } else if let Some(tags_json) = data.tags {
            if let Ok(tags) = serde_json::from_str::<Vec<String>>(tags_json) {
                if !tags.is_empty() {
                    // Cycle through distinct background colors for tag chips.
                    let tag_colors = [
                        Color::Magenta,
                        Color::Blue,
                        Color::Cyan,
                        Color::Green,
                        Color::Yellow,
                    ];
                    let mut tag_spans: Vec<Span<'static>> = vec![Span::raw("  ")];
                    for (i, tag) in tags.iter().enumerate() {
                        if i > 0 {
                            tag_spans.push(Span::raw(" "));
                        }
                        let bg = tag_colors[i % tag_colors.len()];
                        tag_spans.push(Span::styled(
                            format!(" {} ", tag),
                            Style::default().fg(Color::Black).bg(bg).add_modifier(Modifier::BOLD),
                        ));
                    }
                    lines.push(Line::from(tag_spans));
                }
            }
        }
    }

    fn build_runs(&self, lines: &mut Vec<Line<'static>>, data: &SummaryData) {
        if data.runs.is_empty() {
            return;
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Runs".to_string(),
            Style::default()
                .fg(self.theme.accent)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(self.separator());

        for (i, run) in data.runs.iter().enumerate() {
            let status_style = self.status_style(&run.status);
            let date = run.started_at.get(..10).unwrap_or(&run.started_at);
            let label = run.name.clone().unwrap_or_else(|| {
                let id = &run.id;
                if id.len() > 8 { id[id.len() - 8..].to_string() } else { id.clone() }
            });

            let mut spans = vec![
                Span::raw("  ".to_string()),
                Span::raw(format!("{:<12}", label)),
                Span::styled(" \u{25cf} ".to_string(), status_style),
                Span::styled(
                    format!("{date} "),
                    Style::default().fg(self.theme.accent_dim),
                ),
            ];

            if let Some(metrics) = data.run_metrics.get(i) {
                let metric_strs: Vec<String> = metrics
                    .iter()
                    .take(3)
                    .map(|m| format!("{}: {:.3}", m.name, m.value))
                    .collect();
                if !metric_strs.is_empty() {
                    spans.push(Span::raw(format!(" {}", metric_strs.join("  "))));
                }
            }

            lines.push(Line::from(spans));
        }
    }

    fn build_metrics(&self, lines: &mut Vec<Line<'static>>, data: &SummaryData) {
        if data.aggregate_metrics.is_empty() && data.run_params.is_empty() {
            return;
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Summary".to_string(),
            Style::default()
                .fg(self.theme.accent)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(self.separator());

        for m in data.aggregate_metrics {
            if m.count > 1 {
                lines.push(Line::from(vec![
                    Span::raw(format!("  {:<14}", m.name)),
                    Span::raw(format!("mean: {:<8.4}", m.mean)),
                    Span::styled(
                        format!("\u{00b1}{:<8.4}", m.std_dev),
                        Style::default().fg(self.theme.accent_dim),
                    ),
                    Span::styled(
                        format!("[{:.4}, {:.4}]", m.min, m.max),
                        Style::default().fg(self.theme.accent_dim),
                    ),
                ]));
            } else {
                lines.push(Line::from(format!("  {:<14}{:.4}", m.name, m.mean)));
            }
        }

        // Categorical/string parameters
        for p in data.run_params {
            lines.push(Line::from(vec![
                Span::raw(format!("  {:<14}", p.name)),
                Span::styled(
                    p.value.clone(),
                    Style::default().fg(self.theme.accent),
                ),
            ]));
        }
    }

    fn build_curves(
        &self,
        lines: &mut Vec<Line<'static>>,
        data: &SummaryData,
        width: u16,
        height_override: Option<u16>,
        smooth: bool,
    ) {
        if data.metric_histories.is_empty() {
            return;
        }

        // Use configured height or auto-scale based on number of metrics
        let chart_height: u16 = height_override.unwrap_or_else(|| match data.metric_histories.len() {
            1 => 12,
            2 => 10,
            3 => 8,
            _ => 6,
        });

        for (name, history) in data.metric_histories {
            if history.is_empty() {
                continue;
            }

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("  {name}"),
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            )));

            let chart_lines = self.render_chart_to_lines(
                history,
                width,
                chart_height,
                smooth,
                data.preview_total_steps,
            );
            lines.extend(chart_lines);
        }
    }

    fn render_chart_to_lines(
        &self,
        history: &[ScalarMetric],
        width: u16,
        height: u16,
        smooth: bool,
        total_steps: Option<i64>,
    ) -> Vec<Line<'static>> {
        let raw_points: Vec<(f64, f64)> = history
            .iter()
            .map(|m| (m.step as f64, m.value))
            .collect();

        let points = if smooth && raw_points.len() >= 3 {
            catmull_rom_interpolate(&raw_points, (raw_points.len() * 4).max(100))
        } else {
            raw_points
        };

        // Defensive: caller should already filter empty histories, but if a
        // future call site forgets, return early rather than rendering a
        // degenerate [0, 1] x-range chart with no data.
        if points.is_empty() {
            return Vec::new();
        }

        let observed_max_x = points
            .iter()
            .map(|(x, _)| *x)
            .fold(f64::MIN, f64::max);

        let (x_min, x_max) = pinned_x_bounds(observed_max_x, total_steps);
        let (y_min, y_max) = points
            .iter()
            .fold((f64::MAX, f64::MIN), |(min, max), (_, y)| {
                (min.min(*y), max.max(*y))
            });

        let y_range = y_max - y_min;
        let y_pad = if y_range > 0.0 { y_range * 0.1 } else { 0.1 };
        let y_lo = y_min - y_pad;
        let y_hi = y_max + y_pad;

        let dataset = Dataset::default()
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(self.theme.chart_line_1))
            .data(&points);

        let x_labels = vec![format!("{:.0}", x_min), format!("{:.0}", x_max)];
        let y_labels = vec![format!("{:.3}", y_lo), format!("{:.3}", y_hi)];

        let chart = Chart::new(vec![dataset])
            .x_axis(
                Axis::default()
                    .title("step")
                    .style(Style::default().fg(self.theme.chart_axis))
                    .bounds([x_min, x_max])
                    .labels(x_labels),
            )
            .y_axis(
                Axis::default()
                    .style(Style::default().fg(self.theme.chart_axis))
                    .bounds([y_lo, y_hi])
                    .labels(y_labels),
            );

        let rect = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(rect);
        Widget::render(chart, rect, &mut buf);

        let mut result = Vec::new();
        for y in 0..height {
            let mut spans: Vec<Span<'static>> = Vec::new();
            spans.push(Span::raw("  ".to_string()));

            let mut current_style = Style::default();
            let mut current_text = String::new();

            for x in 0..width {
                let cell = &buf[(x, y)];
                let cell_style = Style::default()
                    .fg(cell.fg)
                    .bg(cell.bg)
                    .add_modifier(cell.modifier);

                if cell_style == current_style {
                    current_text.push_str(cell.symbol());
                } else {
                    if !current_text.is_empty() {
                        spans.push(Span::styled(current_text.clone(), current_style));
                    }
                    current_style = cell_style;
                    current_text = cell.symbol().to_string();
                }
            }
            if !current_text.is_empty() {
                spans.push(Span::styled(current_text, current_style));
            }

            result.push(Line::from(spans));
        }

        result
    }

    fn build_tables(
        &self,
        lines: &mut Vec<Line<'static>>,
        data: &SummaryData,
        tables_config: &TablesConfig,
    ) {
        let Some(table) = data.table else {
            return;
        };

        if table.rows == 0 || table.cols == 0 {
            return;
        }

        lines.push(Line::from(""));
        let title = data.table_title.unwrap_or("Table");
        lines.push(Line::from(Span::styled(
            format!("  {title}"),
            Style::default()
                .fg(self.theme.accent)
                .add_modifier(Modifier::BOLD),
        )));

        // Determine cell display width based on content
        let cell_width = 6;

        // Column header
        if data.table_axes.is_some() {
            let mut header_spans: Vec<Span<'static>> = vec![Span::raw("       ".to_string())];
            for c in 0..table.cols {
                header_spans.push(Span::styled(
                    format!("{:>width$}", format!("C{}", c + 1), width = cell_width),
                    Style::default().fg(self.theme.accent_dim),
                ));
            }
            lines.push(Line::from(header_spans));
        }

        // Rows
        for r in 0..table.rows {
            let mut spans: Vec<Span<'static>> = Vec::new();
            spans.push(Span::styled(
                format!("  R{:<3} ", r + 1),
                Style::default().fg(self.theme.accent_dim),
            ));

            for c in 0..table.cols {
                let cell = &table.values[r][c];
                let color_name = match_highlight_rule(cell, &tables_config.highlight);
                if color_name == "transparent" {
                    spans.push(Span::raw(" ".repeat(cell_width)));
                } else {
                    let display = cell.display(cell_width);
                    spans.push(Span::styled(display, Style::default().fg(parse_color(color_name))));
                }
            }

            lines.push(Line::from(spans));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  {}\u{00d7}{}", table.rows, table.cols),
            Style::default().fg(self.theme.accent_dim),
        )));
    }

    fn separator(&self) -> Line<'static> {
        Line::from(Span::styled(
            "  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}".to_string(),
            Style::default().fg(self.theme.border),
        ))
    }

    fn status_style(&self, status: &str) -> Style {
        match status {
            "running" => self.theme.status_running,
            "completed" => self.theme.status_completed,
            "failed" => self.theme.status_failed,
            _ => Style::default(),
        }
    }
}

/// Catmull-Rom spline interpolation.
/// Generates `num_points` evenly spaced points along the spline that passes
/// through all input points. Requires at least 3 input points.
fn catmull_rom_interpolate(points: &[(f64, f64)], num_points: usize) -> Vec<(f64, f64)> {
    let n = points.len();
    if n < 3 {
        return points.to_vec();
    }

    let x_min = points.first().unwrap().0;
    let x_max = points.last().unwrap().0;
    let x_range = x_max - x_min;
    if x_range == 0.0 {
        return points.to_vec();
    }

    let mut result = Vec::with_capacity(num_points);

    for i in 0..num_points {
        let t_global = i as f64 / (num_points - 1) as f64;
        let x = x_min + t_global * x_range;

        // Find the segment this x falls into
        let mut seg = 0;
        for j in 0..n - 1 {
            if x >= points[j].0 && x <= points[j + 1].0 {
                seg = j;
                break;
            }
        }
        // Clamp to last segment
        if x > points[n - 1].0 {
            seg = n - 2;
        }

        // Four control points: p0, p1, p2, p3
        let p0 = if seg > 0 { points[seg - 1] } else { points[seg] };
        let p1 = points[seg];
        let p2 = points[seg + 1];
        let p3 = if seg + 2 < n {
            points[seg + 2]
        } else {
            points[seg + 1]
        };

        // Local t within segment [p1, p2]
        let seg_range = p2.0 - p1.0;
        let t = if seg_range > 0.0 {
            ((x - p1.0) / seg_range).clamp(0.0, 1.0)
        } else {
            0.0
        };

        let t2 = t * t;
        let t3 = t2 * t;

        // Catmull-Rom basis (tau=0.5)
        let y = 0.5
            * ((2.0 * p1.1)
                + (-p0.1 + p2.1) * t
                + (2.0 * p0.1 - 5.0 * p1.1 + 4.0 * p2.1 - p3.1) * t2
                + (-p0.1 + 3.0 * p1.1 - 3.0 * p2.1 + p3.1) * t3);

        // Clamp to segment endpoints to prevent overshoot
        let seg_min = p1.1.min(p2.1);
        let seg_max = p1.1.max(p2.1);
        result.push((x, y.clamp(seg_min, seg_max)));
    }

    result
}

/// Match a cell value against highlight rules. Returns the color name string.
/// Special values: "transparent" (render invisible), "none"/"reset" (default terminal color).
pub(crate) fn match_highlight_rule<'a>(cell: &CellValue, rules: &'a [HighlightRule]) -> &'a str {
    if rules.is_empty() {
        return "reset";
    }

    let numeric = cell.as_f64();

    for rule in rules {
        // Numeric matching
        if let Some(val) = numeric {
            // Exact match takes precedence
            if let Some(eq) = rule.eq {
                if (val - eq).abs() < f64::EPSILON {
                    return &rule.color;
                }
                continue;
            }
            let min_ok = rule.min.map_or(true, |min| val >= min);
            let max_ok = rule.max.map_or(true, |max| val < max);
            if min_ok && max_ok && rule.pattern.is_none() {
                return &rule.color;
            }
        }

        // Pattern matching (for future string support)
        if let Some(ref pattern) = rule.pattern {
            let text = match cell {
                CellValue::Float(v) => format!("{v}"),
                CellValue::Int(v) => format!("{v}"),
            };
            if text.contains(pattern.as_str()) {
                return &rule.color;
            }
        }
    }

    "reset"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HighlightRule;

    fn test_rules() -> Vec<HighlightRule> {
        vec![
            HighlightRule { eq: Some(0.0), min: None, max: None, pattern: None, color: "transparent".into() },
            HighlightRule { eq: None, min: Some(0.7), max: None, pattern: None, color: "red".into() },
            HighlightRule { eq: None, min: Some(0.5), max: Some(0.7), pattern: None, color: "orange".into() },
            HighlightRule { eq: None, min: Some(0.3), max: Some(0.5), pattern: None, color: "yellow".into() },
            HighlightRule { eq: None, min: None, max: Some(0.3), pattern: None, color: "white".into() },
        ]
    }

    #[test]
    fn test_pinned_x_bounds_no_declared_total() {
        // Auto-fit branch: x_max should match observed_max_x.
        assert_eq!(pinned_x_bounds(10.0, None), (0.0, 10.0));
    }

    #[test]
    fn test_pinned_x_bounds_declared_larger_than_observed() {
        // Declared 1000 with observed 5 → x_max = 999.
        assert_eq!(pinned_x_bounds(5.0, Some(1000)), (0.0, 999.0));
    }

    #[test]
    fn test_pinned_x_bounds_observed_overflow() {
        // Declared 1000 but training overshot to step 1500 → x_max = 1500.
        assert_eq!(pinned_x_bounds(1500.0, Some(1000)), (0.0, 1500.0));
    }

    #[test]
    fn test_pinned_x_bounds_zero_or_negative_declared_ignored() {
        // total_steps=0 should be ignored, falling back to observed.
        assert_eq!(pinned_x_bounds(10.0, Some(0)), (0.0, 10.0));
        assert_eq!(pinned_x_bounds(10.0, Some(-1)), (0.0, 10.0));
    }

    #[test]
    fn test_pinned_x_bounds_degenerate_single_point_at_zero() {
        // Single point at step 0, no declaration → x_max clamped to 1.0
        // to prevent empty [0, 0] range that would break the chart widget.
        assert_eq!(pinned_x_bounds(0.0, None), (0.0, 1.0));
    }

    #[test]
    fn test_highlight_exact_zero() {
        let rules = test_rules();
        assert_eq!(match_highlight_rule(&CellValue::Float(0.0), &rules), "transparent");
    }

    #[test]
    fn test_highlight_high_value() {
        let rules = test_rules();
        assert_eq!(match_highlight_rule(&CellValue::Float(0.92), &rules), "red");
    }

    #[test]
    fn test_highlight_mid_value() {
        let rules = test_rules();
        assert_eq!(match_highlight_rule(&CellValue::Float(0.65), &rules), "orange");
    }

    #[test]
    fn test_highlight_low_value() {
        let rules = test_rules();
        assert_eq!(match_highlight_rule(&CellValue::Float(0.4), &rules), "yellow");
    }

    #[test]
    fn test_highlight_very_low() {
        let rules = test_rules();
        assert_eq!(match_highlight_rule(&CellValue::Float(0.2), &rules), "white");
    }
}
