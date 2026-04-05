use ndarray::Array2;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::ui::theme::Theme;

#[allow(dead_code)]
pub struct HeatmapView {
    theme: Theme,
}

#[allow(dead_code)]
impl HeatmapView {
    pub fn new(theme: Theme) -> Self {
        Self {
            theme,
        }
    }

    pub fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        matrix: &Array2<f64>,
        title: &str,
        axes: Option<(&str, &str)>,
    ) {
        let (rows, cols) = matrix.dim();
        if rows == 0 || cols == 0 {
            let msg = Paragraph::new("  Empty matrix.")
                .style(Style::default().fg(self.theme.accent_dim));
            frame.render_widget(msg, area);
            return;
        }

        // Find min/max for color scaling (ignoring zeros for CL lower-triangular)
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

        let mut lines: Vec<Line<'_>> = Vec::new();

        // Title line
        lines.push(Line::from(Span::styled(
            format!("  {title}"),
            Style::default()
                .fg(self.theme.accent)
                .add_modifier(Modifier::BOLD),
        )));

        // Column header
        if axes.is_some() {
            let mut header_spans = vec![Span::raw("       ")]; // row label padding
            for c in 0..cols {
                header_spans.push(Span::styled(
                    format!(" T{:<3}", c + 1),
                    Style::default().fg(self.theme.accent_dim),
                ));
            }
            lines.push(Line::from(header_spans));
        }

        // Each row
        for r in 0..rows {
            let mut spans: Vec<Span<'_>> = Vec::new();

            // Row label
            spans.push(Span::styled(
                format!("  T{:<3} ", r + 1),
                Style::default().fg(self.theme.accent_dim),
            ));

            for c in 0..cols {
                let v = matrix[[r, c]];
                if v == 0.0 {
                    // Zero cells (upper triangle in CL matrices) rendered dim
                    spans.push(Span::styled(
                        "  ·  ",
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

        // Dimensions info
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  {rows}×{cols}  range: [{vmin:.3}, {vmax:.3}]"),
            Style::default().fg(self.theme.accent_dim),
        )));

        // Truncate if we exceed available height
        let max_lines = area.height as usize;
        if lines.len() > max_lines {
            lines.truncate(max_lines.saturating_sub(1));
            lines.push(Line::from(Span::styled(
                "  ... (matrix truncated)",
                Style::default().fg(self.theme.accent_dim),
            )));
        }

        frame.render_widget(Paragraph::new(lines), area);
    }

    fn value_to_color(&self, value: f64, min: f64, max: f64) -> Color {
        let range = max - min;
        if range == 0.0 {
            return self.theme.heatmap_high;
        }
        let t = (value - min) / range; // 0.0 to 1.0

        if t < 0.5 {
            self.theme.heatmap_low
        } else if t < 0.8 {
            self.theme.heatmap_mid
        } else {
            self.theme.heatmap_high
        }
    }
}
