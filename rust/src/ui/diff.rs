use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

use crate::app::{format_json_value, Action, AppState, CompareData, Focus, View};
use crate::artifact::CellValue;
use crate::event::AppEvent;
use crate::keys;
use crate::model::is_lower_better;
use crate::ui::theme::Theme;

const RUN_COLORS: [Color; 6] = [
    Color::Cyan,
    Color::Magenta,
    Color::Green,
    Color::Yellow,
    Color::Blue,
    Color::Red,
];

pub struct DiffView {
    theme: Theme,
}

impl DiffView {
    pub fn new() -> Self {
        Self {
            theme: Theme::default(),
        }
    }

    pub fn handle_event(&mut self, event: &AppEvent, state: &mut AppState) -> Action {
        match event {
            AppEvent::Key(key) => self.handle_key(key, state),
            _ => Action::None,
        }
    }

    fn handle_key(&mut self, key: &KeyEvent, state: &mut AppState) -> Action {
        if keys::matches(key, keys::BACK_ESC) {
            state.current_view = View::Explorer;
            state.focus = Focus::Tree;
            state.compare_data = None;
            return Action::None;
        }

        if keys::matches(key, keys::NAV_DOWN_J) || keys::matches(key, keys::NAV_DOWN) {
            if let Some(data) = &mut state.compare_data {
                if (data.scroll as usize) + 1 < data.total_lines {
                    data.scroll += 1;
                }
            }
            return Action::None;
        }

        if keys::matches(key, keys::NAV_UP_K) || keys::matches(key, keys::NAV_UP) {
            if let Some(data) = &mut state.compare_data {
                data.scroll = data.scroll.saturating_sub(1);
            }
            return Action::None;
        }

        if keys::matches(key, keys::QUIT) {
            return Action::Quit;
        }

        Action::None
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, state: &mut AppState) {
        let border_style = Style::default().fg(self.theme.border_focused);

        let Some(data) = &state.compare_data else {
            return;
        };

        if data.runs.len() < 2 {
            return;
        }

        let title = format!(
            " Diff: {} \u{2192} {} ",
            data.runs[0].label(),
            data.runs[1].label()
        );

        let block = Block::bordered()
            .title(title)
            .border_style(border_style);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let (scroll, lines, total_lines) = {
            let data = state.compare_data.as_ref().unwrap();
            let scroll = data.scroll;
            let mut lines: Vec<Line<'static>> = Vec::new();

            self.build_metric_deltas(&mut lines, data);
            self.build_config_changes(&mut lines, data);
            self.build_delta_tables(&mut lines, data, 0, inner.width);

            lines.push(Line::from(""));
            let total_lines = lines.len();
            (scroll, lines, total_lines)
        };

        let paragraph = Paragraph::new(lines).scroll((scroll, 0));
        frame.render_widget(paragraph, inner);

        // Scroll indicators
        let visible_height = inner.height as usize;
        if total_lines > visible_height {
            if scroll > 0 {
                let hint = Paragraph::new(Line::from(Span::styled(
                    " \u{25b2} more above",
                    Style::default().fg(self.theme.accent_dim),
                )));
                frame.render_widget(hint, Rect::new(inner.x, inner.y, inner.width, 1));
            }
            let at_bottom = scroll as usize + visible_height >= total_lines;
            if !at_bottom {
                let hint = Paragraph::new(Line::from(Span::styled(
                    " \u{25bc} more below",
                    Style::default().fg(self.theme.accent_dim),
                )));
                let y = inner.y + inner.height.saturating_sub(1);
                frame.render_widget(hint, Rect::new(inner.x, y, inner.width, 1));
            }
        }

        if let Some(data) = &mut state.compare_data {
            data.total_lines = total_lines;
        }
    }

    fn build_metric_deltas(&self, lines: &mut Vec<Line<'static>>, data: &CompareData) {
        if data.metric_names.is_empty() {
            return;
        }

        let has_numeric_diffs = data.metric_names.iter().any(|name| {
            let v1 = data.runs[0]
                .latest_metrics
                .iter()
                .find(|m| m.name == *name)
                .map(|m| m.value);
            let v2 = data.runs[1]
                .latest_metrics
                .iter()
                .find(|m| m.name == *name)
                .map(|m| m.value);
            v1 != v2
        });

        if !has_numeric_diffs {
            return;
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Metric Deltas".to_string(),
            Style::default()
                .fg(self.theme.accent)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(self.separator());

        for metric_name in &data.metric_names {
            let v1 = data.runs[0]
                .latest_metrics
                .iter()
                .find(|m| m.name == *metric_name)
                .map(|m| m.value);
            let v2 = data.runs[1]
                .latest_metrics
                .iter()
                .find(|m| m.name == *metric_name)
                .map(|m| m.value);

            match (v1, v2) {
                (Some(a), Some(b)) => {
                    let delta = b - a;
                    let lower_better = is_lower_better(metric_name);
                    let is_improvement = if lower_better {
                        delta < 0.0
                    } else {
                        delta > 0.0
                    };

                    let (color, arrow) = if delta.abs() < f64::EPSILON {
                        (self.theme.accent_dim, " ")
                    } else if is_improvement {
                        (self.theme.success, if delta > 0.0 { "\u{2191}" } else { "\u{2193}" })
                    } else {
                        (self.theme.error, if delta > 0.0 { "\u{2191}" } else { "\u{2193}" })
                    };

                    let delta_sign = if delta > 0.0 { "+" } else { "" };

                    lines.push(Line::from(vec![
                        Span::raw(format!("  {:<14}", metric_name)),
                        Span::raw(format!("{:.4} \u{2192} {:.4}", a, b)),
                        Span::styled(
                            format!("    \u{0394} {delta_sign}{:.4}  {arrow}", delta),
                            Style::default().fg(color),
                        ),
                    ]));
                }
                (Some(a), None) => {
                    lines.push(Line::from(vec![
                        Span::raw(format!("  {:<14}", metric_name)),
                        Span::styled(
                            format!("{:.4} \u{2192} -", a),
                            Style::default().fg(self.theme.error),
                        ),
                    ]));
                }
                (None, Some(b)) => {
                    lines.push(Line::from(vec![
                        Span::raw(format!("  {:<14}", metric_name)),
                        Span::styled(
                            format!("- \u{2192} {:.4}", b),
                            Style::default().fg(self.theme.success),
                        ),
                    ]));
                }
                (None, None) => {}
            }
        }
    }

    fn build_config_changes(&self, lines: &mut Vec<Line<'static>>, data: &CompareData) {
        if data.config_keys.is_empty() {
            return;
        }

        // Only show keys that differ
        let mut has_changes = false;
        let mut change_lines: Vec<Line<'static>> = Vec::new();

        for key in &data.config_keys {
            let v1 = data.runs[0]
                .config
                .as_ref()
                .and_then(|c| c.get(key))
                .map(format_json_value);
            let v2 = data.runs[1]
                .config
                .as_ref()
                .and_then(|c| c.get(key))
                .map(format_json_value);

            if v1 == v2 {
                continue;
            }
            has_changes = true;

            match (v1, v2) {
                (Some(a), Some(b)) => {
                    change_lines.push(Line::from(Span::styled(
                        format!("  - {key}: {a}"),
                        Style::default().fg(self.theme.error),
                    )));
                    change_lines.push(Line::from(Span::styled(
                        format!("  + {key}: {b}"),
                        Style::default().fg(self.theme.success),
                    )));
                }
                (Some(a), None) => {
                    change_lines.push(Line::from(Span::styled(
                        format!("  - {key}: {a}"),
                        Style::default().fg(self.theme.error),
                    )));
                }
                (None, Some(b)) => {
                    change_lines.push(Line::from(Span::styled(
                        format!("  + {key}: {b}"),
                        Style::default().fg(self.theme.success),
                    )));
                }
                (None, None) => {}
            }
        }

        if !has_changes {
            return;
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Config Changes".to_string(),
            Style::default()
                .fg(self.theme.accent)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(self.separator());
        lines.extend(change_lines);
    }

    fn build_delta_tables(
        &self,
        lines: &mut Vec<Line<'static>>,
        data: &CompareData,
        baseline_idx: usize,
        available_width: u16,
    ) {
        if data.table_names.is_empty() {
            return;
        }

        let cell_width: usize = 8;
        let row_label_w: usize = 7; // "  R{r}  "
        let gap: usize = 3;

        for table_name in &data.table_names {
            // Get baseline table
            let baseline_table = data.runs[baseline_idx]
                .tables
                .iter()
                .find(|(n, _, _)| n == table_name)
                .map(|(_, t, _)| t);

            let Some(baseline) = baseline_table else {
                continue;
            };

            // Collect non-baseline runs with matching dimensions
            let delta_runs: Vec<(usize, &crate::artifact::TableData)> = data
                .runs
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != baseline_idx)
                .filter_map(|(i, rd)| {
                    rd.tables
                        .iter()
                        .find(|(n, _, _)| n == table_name)
                        .and_then(|(_, t, _)| {
                            if t.rows == baseline.rows && t.cols == baseline.cols {
                                Some((i, t))
                            } else {
                                None
                            }
                        })
                })
                .collect();

            if delta_runs.is_empty() {
                continue;
            }

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("  Delta: {table_name}"),
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(self.separator());

            let table_w = row_label_w + baseline.cols * cell_width;
            let avail = available_width as usize;
            let tables_per_row = if table_w + gap <= avail {
                ((avail + gap) / (table_w + gap)).min(delta_runs.len()).max(1)
            } else {
                1
            };

            let baseline_label = data.runs[baseline_idx].label();

            for chunk in delta_runs.chunks(tables_per_row) {
                // Run label headers side-by-side
                let mut header_spans: Vec<Span<'static>> = Vec::new();
                for (ci, &(run_idx, _)) in chunk.iter().enumerate() {
                    if ci > 0 {
                        header_spans.push(Span::raw(" ".repeat(gap)));
                    }
                    let color = RUN_COLORS[run_idx % RUN_COLORS.len()];
                    let label = format!(
                        "{} - {}",
                        data.runs[run_idx].label(),
                        baseline_label,
                    );
                    header_spans.push(Span::styled(
                        format!("  {:<width$}", label, width = table_w.saturating_sub(2)),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ));
                }
                lines.push(Line::from(header_spans));

                // Column headers side-by-side
                let mut col_header_spans: Vec<Span<'static>> = Vec::new();
                for (ci, _) in chunk.iter().enumerate() {
                    if ci > 0 {
                        col_header_spans.push(Span::raw(" ".repeat(gap)));
                    }
                    col_header_spans.push(Span::raw(" ".repeat(row_label_w)));
                    for c in 0..baseline.cols {
                        col_header_spans.push(Span::styled(
                            format!("{:>width$}", format!("C{}", c + 1), width = cell_width),
                            Style::default().fg(self.theme.accent_dim),
                        ));
                    }
                }
                lines.push(Line::from(col_header_spans));

                // Delta rows
                for r in 0..baseline.rows {
                    let mut spans: Vec<Span<'static>> = Vec::new();
                    for (ci, &(_, table)) in chunk.iter().enumerate() {
                        if ci > 0 {
                            spans.push(Span::raw(" ".repeat(gap)));
                        }
                        spans.push(Span::styled(
                            format!("  R{:<3} ", r + 1),
                            Style::default().fg(self.theme.accent_dim),
                        ));

                        for c in 0..baseline.cols {
                            let v_base = baseline.values[r][c].as_f64();
                            let v_run = table.values[r][c].as_f64();

                            match (v_base, v_run) {
                                (Some(a), Some(b)) => {
                                    let delta = b - a;
                                    let color = if delta.abs() < f64::EPSILON {
                                        self.theme.accent_dim
                                    } else if delta > 0.0 {
                                        self.theme.success
                                    } else {
                                        self.theme.error
                                    };
                                    let sign = if delta > 0.0 { "+" } else { "" };
                                    spans.push(Span::styled(
                                        format!(
                                            "{:>width$}",
                                            format!("{sign}{:.2}", delta),
                                            width = cell_width
                                        ),
                                        Style::default().fg(color),
                                    ));
                                }
                                _ => {
                                    spans.push(Span::styled(
                                        format!(
                                            "{:>width$}",
                                            CellValue::Float(f64::NAN).display(cell_width),
                                            width = cell_width
                                        ),
                                        Style::default().fg(self.theme.accent_dim),
                                    ));
                                }
                            }
                        }
                    }
                    lines.push(Line::from(spans));
                }
                lines.push(Line::from(""));
            }
        }
    }

    fn separator(&self) -> Line<'static> {
        Line::from(Span::styled(
            "  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}"
                .to_string(),
            Style::default().fg(self.theme.border),
        ))
    }
}

