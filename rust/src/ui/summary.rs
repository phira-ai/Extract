use ndarray::Array2;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Chart, Dataset, GraphType, Paragraph, Widget};
use ratatui::Frame;

use crate::config::SummarySection;
use crate::model::{MetricAggregate, Run, ScalarMetric};
use crate::ui::theme::Theme;

/// All data needed to render a summary panel.
pub struct SummaryData<'a> {
    pub name: &'a str,
    pub runs: &'a [Run],
    pub run_metrics: &'a [Vec<ScalarMetric>],
    pub aggregate_metrics: &'a [MetricAggregate],
    pub unique_configs: i64,
    pub metric_history: &'a [ScalarMetric],
    pub metric_name: Option<&'a str>,
    pub matrix: Option<&'a Array2<f64>>,
    pub matrix_title: Option<&'a str>,
    pub matrix_axes: Option<(&'a str, &'a str)>,
}

pub struct SummaryRenderer {
    theme: Theme,
}

impl SummaryRenderer {
    pub fn new() -> Self {
        Self {
            theme: Theme::default(),
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
    ) -> usize {
        let mut lines: Vec<Line<'static>> = Vec::new();

        // Header (always first)
        self.build_header(&mut lines, data);

        // Configurable sections
        for section in sections {
            match section {
                SummarySection::Runs => self.build_runs(&mut lines, data),
                SummarySection::Metrics => self.build_metrics(&mut lines, data),
                SummarySection::Curves => {
                    self.build_curves(&mut lines, data, area.width.saturating_sub(4))
                }
                SummarySection::Matrix => self.build_matrix(&mut lines, data),
            }
        }

        // Trailing blank line
        lines.push(Line::from(""));

        let total_lines = lines.len();
        let visible_height = area.height as usize;

        let paragraph = Paragraph::new(lines).scroll((scroll_offset, 0));
        frame.render_widget(paragraph, area);

        // Scroll indicators
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

            let mut spans = vec![
                Span::raw("  ".to_string()),
                Span::styled("\u{25cf} ".to_string(), status_style),
                Span::styled(format!("{:<11}", run.status), status_style),
                Span::styled(
                    format!(" {date} "),
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
        if data.aggregate_metrics.is_empty() {
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
    }

    fn build_curves(&self, lines: &mut Vec<Line<'static>>, data: &SummaryData, width: u16) {
        if data.metric_history.is_empty() {
            return;
        }

        let chart_height: u16 = 10;

        lines.push(Line::from(""));

        let label = data.metric_name.unwrap_or("metric");
        lines.push(Line::from(Span::styled(
            format!("  {label}"),
            Style::default()
                .fg(self.theme.accent)
                .add_modifier(Modifier::BOLD),
        )));

        let chart_lines = self.render_chart_to_lines(data.metric_history, width, chart_height);
        lines.extend(chart_lines);
    }

    fn render_chart_to_lines(
        &self,
        history: &[ScalarMetric],
        width: u16,
        height: u16,
    ) -> Vec<Line<'static>> {
        let points: Vec<(f64, f64)> = history
            .iter()
            .map(|m| (m.step as f64, m.value))
            .collect();

        let (x_min, x_max) = points
            .iter()
            .fold((f64::MAX, f64::MIN), |(min, max), (x, _)| {
                (min.min(*x), max.max(*x))
            });
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

        // Render to offscreen buffer
        let rect = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(rect);
        Widget::render(chart, rect, &mut buf);

        // Extract styled lines from buffer
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

    fn build_matrix(&self, lines: &mut Vec<Line<'static>>, data: &SummaryData) {
        let Some(matrix) = data.matrix else {
            return;
        };

        let (rows, cols) = matrix.dim();
        if rows == 0 || cols == 0 {
            return;
        }

        let mut vmin = f64::MAX;
        let mut vmax = f64::MIN;
        for &v in matrix.iter() {
            if v != 0.0 {
                vmin = vmin.min(v);
                vmax = vmax.max(v);
            }
        }
        if vmin > vmax {
            vmin = 0.0;
            vmax = 1.0;
        }

        lines.push(Line::from(""));
        let title = data.matrix_title.unwrap_or("Matrix");
        lines.push(Line::from(Span::styled(
            format!("  {title}"),
            Style::default()
                .fg(self.theme.accent)
                .add_modifier(Modifier::BOLD),
        )));

        // Column header
        if data.matrix_axes.is_some() {
            let mut header_spans: Vec<Span<'static>> = vec![Span::raw("       ".to_string())];
            for c in 0..cols {
                header_spans.push(Span::styled(
                    format!(" T{:<3}", c + 1),
                    Style::default().fg(self.theme.accent_dim),
                ));
            }
            lines.push(Line::from(header_spans));
        }

        for r in 0..rows {
            let mut spans: Vec<Span<'static>> = Vec::new();
            spans.push(Span::styled(
                format!("  T{:<3} ", r + 1),
                Style::default().fg(self.theme.accent_dim),
            ));

            for c in 0..cols {
                let v = matrix[[r, c]];
                if v == 0.0 {
                    spans.push(Span::styled(
                        "  \u{00b7}  ".to_string(),
                        Style::default().fg(self.theme.heatmap_zero),
                    ));
                } else {
                    let color = self.value_to_color(v, vmin, vmax);
                    spans.push(Span::styled(
                        format!(" {:.2}", v),
                        Style::default().fg(color),
                    ));
                }
            }

            lines.push(Line::from(spans));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  {rows}\u{00d7}{cols}  range: [{vmin:.3}, {vmax:.3}]"),
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

    fn value_to_color(&self, value: f64, min: f64, max: f64) -> Color {
        let range = max - min;
        if range == 0.0 {
            return self.theme.heatmap_high;
        }
        let t = (value - min) / range;
        if t < 0.5 {
            self.theme.heatmap_low
        } else if t < 0.8 {
            self.theme.heatmap_mid
        } else {
            self.theme.heatmap_high
        }
    }
}
