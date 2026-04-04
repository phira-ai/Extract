use crossterm::event::KeyEvent;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Block, Chart, Dataset, GraphType, Paragraph, Widget};
use ratatui::Frame;

use crate::app::{format_json_value, Action, AppState, CompareData, Focus, View};
use crate::config::{parse_color, CompareSection};
use crate::event::AppEvent;
use crate::keys;
use crate::ui::summary::match_highlight_rule;
use crate::ui::theme::Theme;

const RUN_COLORS: [Color; 6] = [
    Color::Cyan,
    Color::Magenta,
    Color::Green,
    Color::Yellow,
    Color::Blue,
    Color::Red,
];

pub struct CompareView {
    theme: Theme,
}

impl CompareView {
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

        if keys::matches_shift(key, keys::DIFF_TAB) {
            if state.selected_runs_for_compare.len() >= 2 {
                state.current_view = View::Diff;
                if let Some(data) = &mut state.compare_data {
                    data.scroll = 0;
                }
                return Action::None;
            }
            return Action::None;
        }

        if keys::matches(key, keys::TAB) {
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
            Span::styled("ompare]", self.theme.tab_active),
            Span::raw(" ["),
            Span::styled("D", mnemonic),
            Span::styled("iff]", self.theme.tab_inactive),
        ];
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, state: &mut AppState) {
        let border_style = Style::default().fg(self.theme.border_focused);

        let Some(data) = &state.compare_data else {
            return;
        };

        let title = if data.runs.len() == 2 {
            format!(
                " Compare: {} vs {} ",
                data.runs[0].label(),
                data.runs[1].label()
            )
        } else {
            let labels: Vec<String> = data.runs.iter().map(|r| r.label()).collect();
            format!(" Compare: {} ", labels.join(" vs "))
        };

        let block = Block::bordered()
            .title(title)
            .border_style(border_style);
        let block_inner = block.inner(area);
        frame.render_widget(block, area);

        let chunks = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).split(block_inner);
        self.render_tab_bar(frame, chunks[0]);
        let inner = chunks[1];

        // Build lines from compare data (immutable borrow of state.compare_data)
        let (scroll, lines, total_lines) = {
            let data = state.compare_data.as_ref().unwrap();
            let scroll = data.scroll;
            let mut lines: Vec<Line<'static>> = Vec::new();

            let sections = state.config.compare.sections.clone();
            for section in &sections {
                match section {
                    CompareSection::Pivot => self.build_pivot_table(&mut lines, data),
                    CompareSection::Config => self.build_config_section(&mut lines, data),
                    CompareSection::Tables => self.build_tables_section(
                        &mut lines,
                        data,
                        &state.config.tables,
                        inner.width,
                    ),
                    CompareSection::Curves => {
                        let chart_width = ((inner.width as f32)
                            * (state.config.summary.curve_width.min(100) as f32 / 100.0))
                            as u16;
                        self.build_overlay_charts(
                            &mut lines,
                            data,
                            chart_width.max(20),
                        )
                    }
                }
            }

            lines.push(Line::from(""));
            let total_lines = lines.len();
            (scroll, lines, total_lines)
        };

        // Render with scroll
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

        // Update total_lines
        if let Some(data) = &mut state.compare_data {
            data.total_lines = total_lines;
        }
    }

    fn build_pivot_table(&self, lines: &mut Vec<Line<'static>>, data: &CompareData) {
        if data.metric_names.is_empty() && data.param_names.is_empty() {
            return;
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Pivot Table".to_string(),
            Style::default()
                .fg(self.theme.accent)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(self.separator());

        let label_width = 16;
        let col_width = 14;

        // Column headers
        let mut header_spans = vec![Span::raw(format!("  {:<label_width$}", ""))];
        for (i, rd) in data.runs.iter().enumerate() {
            let color = RUN_COLORS[i % RUN_COLORS.len()];
            header_spans.push(Span::styled(
                format!("{:<col_width$}", rd.label()),
                Style::default()
                    .fg(color)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        lines.push(Line::from(header_spans));

        // Numeric metrics
        for metric_name in &data.metric_names {
            let mut spans = vec![Span::raw(format!("  {:<label_width$}", metric_name))];
            let values: Vec<Option<f64>> = data
                .runs
                .iter()
                .map(|rd| {
                    rd.latest_metrics
                        .iter()
                        .find(|m| m.name == *metric_name)
                        .map(|m| m.value)
                })
                .collect();

            // Check if values differ
            let all_same = values.windows(2).all(|w| match (w[0], w[1]) {
                (Some(a), Some(b)) => (a - b).abs() < f64::EPSILON,
                (None, None) => true,
                _ => false,
            });

            for (i, val) in values.iter().enumerate() {
                let text = match val {
                    Some(v) => format!("{:<col_width$.4}", v),
                    None => format!("{:<col_width$}", "-"),
                };
                let style = if !all_same {
                    Style::default().fg(self.theme.warning)
                } else {
                    Style::default()
                };
                let _ = i;
                spans.push(Span::styled(text, style));
            }
            lines.push(Line::from(spans));
        }

        // Categorical params
        for param_name in &data.param_names {
            let mut spans = vec![Span::raw(format!("  {:<label_width$}", param_name))];
            let values: Vec<String> = data
                .runs
                .iter()
                .map(|rd| {
                    rd.run_params
                        .iter()
                        .find(|p| p.name == *param_name)
                        .map(|p| p.value.clone())
                        .unwrap_or_else(|| "-".to_string())
                })
                .collect();

            let all_same = values.windows(2).all(|w| w[0] == w[1]);

            for val in &values {
                let text = format!("{:<col_width$}", val);
                let style = if !all_same {
                    Style::default().fg(self.theme.accent)
                } else {
                    Style::default().fg(self.theme.accent_dim)
                };
                spans.push(Span::styled(text, style));
            }
            lines.push(Line::from(spans));
        }
    }

    fn build_config_section(&self, lines: &mut Vec<Line<'static>>, data: &CompareData) {
        if data.config_keys.is_empty() {
            return;
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Config".to_string(),
            Style::default()
                .fg(self.theme.accent)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(self.separator());

        let label_width = 16;
        let col_width = 14;

        // Column headers
        let mut header_spans = vec![Span::raw(format!("  {:<label_width$}", ""))];
        for (i, rd) in data.runs.iter().enumerate() {
            let color = RUN_COLORS[i % RUN_COLORS.len()];
            header_spans.push(Span::styled(
                format!("{:<col_width$}", rd.label()),
                Style::default()
                    .fg(color)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        lines.push(Line::from(header_spans));

        for key in &data.config_keys {
            let mut spans = vec![Span::raw(format!("  {:<label_width$}", key))];

            let values: Vec<String> = data
                .runs
                .iter()
                .map(|rd| {
                    rd.config
                        .as_ref()
                        .and_then(|c| c.get(key))
                        .map(|v| format_json_value(v))
                        .unwrap_or_else(|| "-".to_string())
                })
                .collect();

            let all_same = values.windows(2).all(|w| w[0] == w[1]);

            for val in &values {
                let text = format!("{:<col_width$}", val);
                let style = if !all_same {
                    Style::default().fg(self.theme.warning)
                } else {
                    Style::default()
                };
                spans.push(Span::styled(text, style));
            }
            lines.push(Line::from(spans));
        }
    }

    fn build_tables_section(
        &self,
        lines: &mut Vec<Line<'static>>,
        data: &CompareData,
        tables_config: &crate::config::TablesConfig,
        available_width: u16,
    ) {
        if data.table_names.is_empty() {
            return;
        }

        let cell_width: usize = 6;
        let row_label_w: usize = 6; // "  R{r} "
        let gap: usize = 3;

        for table_name in &data.table_names {
            // Collect (run_index, &TableData) pairs for runs that have this artifact
            let run_tables: Vec<(usize, &crate::artifact::TableData)> = data
                .runs
                .iter()
                .enumerate()
                .filter_map(|(i, rd)| {
                    rd.tables
                        .iter()
                        .find(|(n, _, _)| n == table_name)
                        .map(|(_, table, _)| (i, table))
                })
                .collect();

            if run_tables.is_empty() {
                continue;
            }

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("  {table_name}"),
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(self.separator());

            // Compute table width based on max cols across runs
            let max_cols = run_tables.iter().map(|(_, t)| t.cols).max().unwrap_or(0);
            let table_w = row_label_w + max_cols * cell_width;

            // Compute tables_per_row
            let avail = available_width as usize;
            let tables_per_row = if table_w + gap <= avail {
                ((avail + gap) / (table_w + gap)).min(run_tables.len()).max(1)
            } else {
                1
            };

            // Render in chunks
            for chunk in run_tables.chunks(tables_per_row) {
                // Run label headers side-by-side
                let mut header_spans: Vec<Span<'static>> = Vec::new();
                for (ci, &(run_idx, _)) in chunk.iter().enumerate() {
                    if ci > 0 {
                        header_spans.push(Span::raw(" ".repeat(gap)));
                    }
                    let color = RUN_COLORS[run_idx % RUN_COLORS.len()];
                    let label = data.runs[run_idx].label();
                    header_spans.push(Span::styled(
                        format!("  {:<width$}", label, width = table_w.saturating_sub(2)),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ));
                }
                lines.push(Line::from(header_spans));

                // Row-by-row rendering
                let max_rows = chunk.iter().map(|(_, t)| t.rows).max().unwrap_or(0);
                for r in 0..max_rows {
                    let mut spans: Vec<Span<'static>> = Vec::new();
                    for (ci, &(_, table)) in chunk.iter().enumerate() {
                        if ci > 0 {
                            spans.push(Span::raw(" ".repeat(gap)));
                        }
                        if r < table.rows {
                            spans.push(Span::styled(
                                format!("  R{:<3}", r + 1),
                                Style::default().fg(self.theme.accent_dim),
                            ));
                            for c in 0..table.cols {
                                let cell = &table.values[r][c];
                                let color_name =
                                    match_highlight_rule(cell, &tables_config.highlight);
                                if color_name == "transparent" {
                                    spans.push(Span::raw(" ".repeat(cell_width)));
                                } else {
                                    let display = cell.display(cell_width);
                                    spans.push(Span::styled(
                                        display,
                                        Style::default().fg(parse_color(color_name)),
                                    ));
                                }
                            }
                        } else {
                            // Pad empty space for tables with fewer rows
                            spans.push(Span::raw(" ".repeat(table_w)));
                        }
                    }
                    lines.push(Line::from(spans));
                }
                lines.push(Line::from(""));
            }
        }
    }

    fn build_overlay_charts(
        &self,
        lines: &mut Vec<Line<'static>>,
        data: &CompareData,
        width: u16,
    ) {
        if data.metric_names.is_empty() {
            return;
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Curves (overlay)".to_string(),
            Style::default()
                .fg(self.theme.accent)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(self.separator());

        // Legend
        let mut legend_spans: Vec<Span<'static>> = vec![Span::raw("  ".to_string())];
        for (i, rd) in data.runs.iter().enumerate() {
            let color = RUN_COLORS[i % RUN_COLORS.len()];
            if i > 0 {
                legend_spans.push(Span::raw("  ".to_string()));
            }
            legend_spans.push(Span::styled(
                format!("\u{2500}\u{2500} {}", rd.label()),
                Style::default().fg(color),
            ));
        }
        lines.push(Line::from(legend_spans));

        let chart_height: u16 = match data.metric_names.len() {
            1 => 12,
            2 => 10,
            3 => 8,
            _ => 6,
        };

        for metric_name in &data.metric_names {
            // Collect data points for each run
            let mut all_points: Vec<(Vec<(f64, f64)>, Color)> = Vec::new();
            let mut has_data = false;

            for (i, rd) in data.runs.iter().enumerate() {
                let color = RUN_COLORS[i % RUN_COLORS.len()];
                if let Some((_, history)) = rd
                    .metric_histories
                    .iter()
                    .find(|(n, _)| n == metric_name)
                {
                    if !history.is_empty() {
                        has_data = true;
                    }
                    let points: Vec<(f64, f64)> =
                        history.iter().map(|m| (m.step as f64, m.value)).collect();
                    all_points.push((points, color));
                }
            }

            if !has_data {
                continue;
            }

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("  {metric_name}"),
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            )));

            let chart_lines =
                self.render_overlay_chart_to_lines(&all_points, width.max(20), chart_height);
            lines.extend(chart_lines);
        }
    }

    fn render_overlay_chart_to_lines(
        &self,
        runs_data: &[(Vec<(f64, f64)>, Color)],
        width: u16,
        height: u16,
    ) -> Vec<Line<'static>> {
        // Compute global bounds
        let mut x_min = f64::MAX;
        let mut x_max = f64::MIN;
        let mut y_min = f64::MAX;
        let mut y_max = f64::MIN;

        for (data, _) in runs_data {
            for &(x, y) in data {
                x_min = x_min.min(x);
                x_max = x_max.max(x);
                y_min = y_min.min(y);
                y_max = y_max.max(y);
            }
        }

        if x_min >= x_max {
            x_max = x_min + 1.0;
        }

        let y_range = y_max - y_min;
        let y_pad = if y_range > 0.0 { y_range * 0.1 } else { 0.1 };
        let y_lo = y_min - y_pad;
        let y_hi = y_max + y_pad;

        let datasets: Vec<Dataset> = runs_data
            .iter()
            .map(|(data, color)| {
                Dataset::default()
                    .marker(symbols::Marker::Braille)
                    .graph_type(GraphType::Line)
                    .style(Style::default().fg(*color))
                    .data(data)
            })
            .collect();

        let x_labels = vec![format!("{:.0}", x_min), format!("{:.0}", x_max)];
        let y_labels = vec![format!("{:.3}", y_lo), format!("{:.3}", y_hi)];

        let chart = Chart::new(datasets)
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
            let mut spans: Vec<Span<'static>> = vec![Span::raw("  ".to_string())];
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

    fn separator(&self) -> Line<'static> {
        Line::from(Span::styled(
            "  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}"
                .to_string(),
            Style::default().fg(self.theme.border),
        ))
    }
}

