use crossterm::event::KeyEvent;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

use crate::app::{format_json_value, Action, AppState, CompareData, Focus, View};
use crate::artifact::CellValue;
use crate::event::AppEvent;
use crate::keys;
use crate::model::is_lower_better;
use crate::ui::theme::Theme;

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
            self.build_delta_tables(&mut lines, data);

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

    fn build_delta_tables(&self, lines: &mut Vec<Line<'static>>, data: &CompareData) {
        if data.table_names.is_empty() {
            return;
        }

        let cell_width = 8;

        for table_name in &data.table_names {
            let t1 = data.runs[0]
                .tables
                .iter()
                .find(|(n, _, _)| n == table_name);
            let t2 = data.runs[1]
                .tables
                .iter()
                .find(|(n, _, _)| n == table_name);

            let (Some((_, table1, _)), Some((_, table2, _))) = (t1, t2) else {
                continue;
            };

            if table1.rows != table2.rows || table1.cols != table2.cols {
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

            // Column headers
            let mut header_spans: Vec<Span<'static>> = vec![Span::raw("       ".to_string())];
            for c in 0..table1.cols {
                header_spans.push(Span::styled(
                    format!("{:>width$}", format!("C{}", c + 1), width = cell_width),
                    Style::default().fg(self.theme.accent_dim),
                ));
            }
            lines.push(Line::from(header_spans));

            // Delta rows
            for r in 0..table1.rows {
                let mut spans: Vec<Span<'static>> = vec![Span::styled(
                    format!("  R{:<3} ", r + 1),
                    Style::default().fg(self.theme.accent_dim),
                )];

                for c in 0..table1.cols {
                    let v1 = table1.values[r][c].as_f64();
                    let v2 = table2.values[r][c].as_f64();

                    match (v1, v2) {
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
                                format!("{:>width$}", CellValue::Float(f64::NAN).display(cell_width), width = cell_width),
                                Style::default().fg(self.theme.accent_dim),
                            ));
                        }
                    }
                }
                lines.push(Line::from(spans));
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

