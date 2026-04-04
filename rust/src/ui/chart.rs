use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::symbols;
use ratatui::widgets::{Axis, Block, Chart, Dataset, GraphType, Paragraph};
use ratatui::Frame;

use crate::model::ScalarMetric;
use crate::ui::theme::Theme;

#[allow(dead_code)]
pub struct ChartView {
    theme: Theme,
}

#[allow(dead_code)]
impl ChartView {
    pub fn new() -> Self {
        Self {
            theme: Theme::default(),
        }
    }

    pub fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        metric_name: &str,
        history: &[ScalarMetric],
    ) {
        if history.is_empty() {
            let msg = Paragraph::new("  No metric data to plot.")
                .style(Style::default().fg(self.theme.accent_dim));
            frame.render_widget(msg, area);
            return;
        }

        let data: Vec<(f64, f64)> = history
            .iter()
            .map(|m| (m.step as f64, m.value))
            .collect();

        let (x_min, x_max) = data.iter().fold((f64::MAX, f64::MIN), |(min, max), (x, _)| {
            (min.min(*x), max.max(*x))
        });
        let (y_min, y_max) = data.iter().fold((f64::MAX, f64::MIN), |(min, max), (_, y)| {
            (min.min(*y), max.max(*y))
        });

        // Add some padding to y-axis
        let y_range = y_max - y_min;
        let y_pad = if y_range > 0.0 { y_range * 0.1 } else { 0.1 };
        let y_lo = y_min - y_pad;
        let y_hi = y_max + y_pad;

        let dataset = Dataset::default()
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(self.theme.chart_line_1))
            .data(&data);

        let x_labels = vec![
            format!("{:.0}", x_min),
            format!("{:.0}", (x_min + x_max) / 2.0),
            format!("{:.0}", x_max),
        ];
        let y_labels = vec![
            format!("{:.4}", y_lo),
            format!("{:.4}", (y_lo + y_hi) / 2.0),
            format!("{:.4}", y_hi),
        ];

        let chart = Chart::new(vec![dataset])
            .block(Block::default().title(format!(" {metric_name} ")))
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

        frame.render_widget(chart, area);
    }
}
