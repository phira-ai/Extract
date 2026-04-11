use crossterm::event::KeyEvent;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::border;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

use crate::app::{format_json_value, resolve_dotted_key, Action, AppState, CompareData, Focus, View};
use crate::artifact::CellValue;
use crate::event::AppEvent;
use crate::keys;
use crate::config::MetricsConfig;
use crate::model::is_lower_better;
use crate::ui::theme::Theme;

const RUN_COLORS: [Color; 12] = [
    Color::Cyan,
    Color::Magenta,
    Color::Green,
    Color::Yellow,
    Color::Blue,
    Color::Red,
    Color::LightCyan,
    Color::LightMagenta,
    Color::LightGreen,
    Color::LightYellow,
    Color::LightBlue,
    Color::LightRed,
];

pub struct DiffView {
    theme: Theme,
}

impl DiffView {
    pub fn new(theme: Theme) -> Self {
        Self {
            theme,
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
                let max_scroll = data.total_lines.saturating_sub(data.visible_height);
                if (data.scroll as usize) < max_scroll {
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

        if keys::matches_shift(key, keys::COMPARE_TAB) {
            state.current_view = View::Compare;
            if let Some(data) = &mut state.compare_data {
                data.scroll = 0;
            }
            return Action::None;
        }

        if keys::matches(key, keys::TAB) || keys::matches(key, keys::PANEL_3) || keys::matches(key, keys::BACKTAB) {
            if !state.selected_runs_for_compare.is_empty() {
                state.focus = Focus::Selection;
            }
            return Action::None;
        }

        if keys::matches(key, keys::QUIT) {
            return Action::Quit;
        }

        Action::None
    }

    fn render_tab_bar(&self, frame: &mut Frame, area: Rect) {
        let mnemonic = Style::default()
            .fg(self.theme.accent)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);

        let spans = vec![
            Span::raw(" ["),
            Span::styled("C", mnemonic),
            Span::styled("ompare]", self.theme.tab_inactive),
            Span::raw(" ["),
            Span::styled("D", mnemonic),
            Span::styled("iff]", self.theme.tab_active),
        ];
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, state: &mut AppState) {
        let border_style = Style::default().fg(self.theme.border_focused);

        let Some(data) = &state.compare_data else {
            return;
        };

        if data.runs.len() < 2 {
            return;
        }

        let baseline_idx = state.compare_baseline.min(data.runs.len().saturating_sub(1));

        let title = {
            let baseline_label = data.runs[baseline_idx].label();
            let other_labels: Vec<String> = data
                .runs
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != baseline_idx)
                .map(|(_, rd)| rd.label())
                .collect();
            format!(" Diff: {} vs {} ", baseline_label, other_labels.join(", "))
        };

        let block = Block::bordered()
            .title(title)
            .border_style(border_style)
            .border_set(border::ROUNDED);
        let block_inner = block.inner(area);
        frame.render_widget(block, area);

        let chunks = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).split(block_inner);
        self.render_tab_bar(frame, chunks[0]);
        let inner = chunks[1];

        let (scroll, lines, total_lines) = {
            let data = state.compare_data.as_ref().unwrap();
            let scroll = data.scroll;
            let mut lines: Vec<Line<'static>> = Vec::new();

            self.build_metric_deltas(&mut lines, data, baseline_idx, inner.width, &state.config.metrics);
            self.build_config_changes(&mut lines, data, baseline_idx);
            self.build_delta_tables(&mut lines, data, baseline_idx, inner.width);

            lines.push(Line::from(""));
            let total_lines = lines.len();
            (scroll, lines, total_lines)
        };

        let paragraph = Paragraph::new(lines).scroll((scroll, 0));
        frame.render_widget(paragraph, inner);

        if let Some(data) = &mut state.compare_data {
            data.total_lines = total_lines;
            data.visible_height = inner.height as usize;
        }
    }

    fn build_metric_deltas(
        &self,
        lines: &mut Vec<Line<'static>>,
        data: &CompareData,
        baseline_idx: usize,
        available_width: u16,
        metrics_config: &MetricsConfig,
    ) {
        if data.metric_names.is_empty() {
            return;
        }

        let non_baseline: Vec<usize> = (0..data.runs.len())
            .filter(|i| *i != baseline_idx)
            .collect();

        // Check if there are any differences
        let has_numeric_diffs = data.metric_names.iter().any(|name| {
            let v_base = data.runs[baseline_idx]
                .latest_metrics
                .iter()
                .find(|m| m.name == *name)
                .map(|m| m.value);
            non_baseline.iter().any(|&i| {
                let v_other = data.runs[i]
                    .latest_metrics
                    .iter()
                    .find(|m| m.name == *name)
                    .map(|m| m.value);
                v_base != v_other
            })
        });

        if !has_numeric_diffs {
            return;
        }

        let label_width: usize = 16;
        let col_gap: usize = 5;
        let col_width: usize = 28 + col_gap;
        let indent: usize = 2;
        // baseline col + non-baseline cols; baseline always shown
        let cols_for_others = ((available_width as usize).saturating_sub(indent + label_width + col_width) / col_width).max(1);

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Metric Deltas".to_string(),
            Style::default()
                .fg(self.theme.accent)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(self.separator());

        let truncate = |s: &str| -> String {
            if s.len() <= col_width {
                format!("{:<width$}", s, width = col_width)
            } else {
                format!("{:.width$}", s, width = col_width)
            }
        };

        for chunk in non_baseline.chunks(cols_for_others) {
            // Header: baseline + this chunk
            let mut spans: Vec<Span<'static>> = Vec::new();
            spans.push(Span::raw(format!("  {:<width$}", "", width = label_width)));
            spans.push(Span::styled(
                truncate(&data.runs[baseline_idx].label()),
                Style::default().add_modifier(Modifier::BOLD),
            ));
            for &i in chunk {
                let color = RUN_COLORS[i % RUN_COLORS.len()];
                spans.push(Span::styled(
                    truncate(&data.runs[i].label()),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ));
            }
            lines.push(Line::from(spans));

            for metric_name in &data.metric_names {
                let v_base = data.runs[baseline_idx]
                    .latest_metrics
                    .iter()
                    .find(|m| m.name == *metric_name)
                    .map(|m| m.value);

                let lower_better = is_lower_better(metric_name, metrics_config);

                let mut spans: Vec<Span<'static>> = Vec::new();
                spans.push(Span::raw(format!("  {:<width$}", metric_name, width = label_width)));

                match v_base {
                    Some(base_val) => {
                        spans.push(Span::raw(format!("{:<width$.4}", base_val, width = col_width)));
                    }
                    None => {
                        spans.push(Span::raw(format!("{:<width$}", "-", width = col_width)));
                    }
                }

                for &i in chunk {
                    let v_run = data.runs[i]
                        .latest_metrics
                        .iter()
                        .find(|m| m.name == *metric_name)
                        .map(|m| m.value);

                    match (v_base, v_run) {
                        (Some(a), Some(b)) => {
                            let delta = b - a;
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

                            spans.push(Span::styled(
                                format!(
                                    "{:<width$}",
                                    format!("{:.4} \u{0394} {delta_sign}{:.4} {arrow}", b, delta),
                                    width = col_width
                                ),
                                Style::default().fg(color),
                            ));
                        }
                        (Some(_), None) => {
                            spans.push(Span::styled(
                                format!("{:<width$}", "-", width = col_width),
                                Style::default().fg(self.theme.error),
                            ));
                        }
                        (None, Some(b)) => {
                            spans.push(Span::styled(
                                format!("{:<width$.4}", b, width = col_width),
                                Style::default().fg(self.theme.success),
                            ));
                        }
                        (None, None) => {
                            spans.push(Span::raw(format!("{:<width$}", "-", width = col_width)));
                        }
                    }
                }

                lines.push(Line::from(spans));
            }

            if chunk.last() != non_baseline.last() {
                lines.push(Line::from(""));
            }
        }
    }

    fn build_config_changes(
        &self,
        lines: &mut Vec<Line<'static>>,
        data: &CompareData,
        baseline_idx: usize,
    ) {
        if data.config_keys.is_empty() {
            return;
        }

        let non_baseline: Vec<usize> = (0..data.runs.len())
            .filter(|i| *i != baseline_idx)
            .collect();

        // Only show keys that differ between baseline and at least one other run
        let mut has_changes = false;
        let mut change_lines: Vec<Line<'static>> = Vec::new();

        for key in &data.config_keys {
            let v_base = data.runs[baseline_idx]
                .config
                .as_ref()
                .and_then(|c| resolve_dotted_key(c, key))
                .map(format_json_value);

            for &i in &non_baseline {
                let v_run = data.runs[i]
                    .config
                    .as_ref()
                    .and_then(|c| resolve_dotted_key(c, key))
                    .map(format_json_value);

                if v_base == v_run {
                    continue;
                }
                has_changes = true;

                let run_label = data.runs[i].label();
                let color = RUN_COLORS[i % RUN_COLORS.len()];

                let base_str = v_base.as_deref().unwrap_or("-");
                let run_str = v_run.as_deref().unwrap_or("-");

                change_lines.push(Line::from(Span::styled(
                    format!("  {run_label}: {key}: {base_str} \u{2192} {run_str}"),
                    Style::default().fg(color),
                )));
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
                        data.runs[baseline_idx].label(),
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

