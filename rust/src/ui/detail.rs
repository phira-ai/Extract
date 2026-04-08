use crossterm::event::KeyEvent;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::symbols::border;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{Action, AppState, Focus, SelectionSummary, View};
use crate::event::AppEvent;
use crate::keys;
use crate::model::Run;
use crate::ui::summary::{SummaryData, SummaryRenderer};
use crate::ui::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailTab {
    Summary,
    Info,
}

pub struct DetailPanel {
    pub active_tab: DetailTab,
    summary: SummaryRenderer,
    theme: Theme,
}

impl DetailPanel {
    pub fn new(theme: Theme) -> Self {
        Self {
            active_tab: DetailTab::Summary,
            summary: SummaryRenderer::new(theme),
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
        // S/I switch detail tabs
        if keys::matches_shift(key, keys::SUMMARY_TAB) {
            self.active_tab = DetailTab::Summary;
            return Action::None;
        }
        if keys::matches_shift(key, keys::INFO_TAB) {
            self.active_tab = DetailTab::Info;
            return Action::None;
        }

        // Tab → next panel: Selection (if marked) or Tree
        if keys::matches(key, keys::TAB) {
            if !state.selected_runs_for_compare.is_empty() {
                state.focus = Focus::Selection;
            } else {
                state.focus = Focus::Tree;
            }
            return Action::None;
        }

        // Shift-Tab → previous panel: Tree
        if keys::matches(key, keys::BACKTAB) {
            state.focus = Focus::Tree;
            return Action::None;
        }

        if keys::matches(key, keys::BACK_ESC) {
            state.focus = Focus::Tree;
            state.current_view = View::Explorer;
            return Action::None;
        }

        // Summary tab: j/k scrolls
        if self.active_tab == DetailTab::Summary {
            if keys::matches(key, keys::NAV_DOWN_J) || keys::matches(key, keys::NAV_DOWN) {
                let max_scroll = state.summary_total_lines.saturating_sub(state.summary_visible_height);
                if (state.summary_scroll as usize) < max_scroll {
                    state.summary_scroll += 1;
                }
                return Action::None;
            }
            if keys::matches(key, keys::NAV_UP_K) || keys::matches(key, keys::NAV_UP) {
                state.summary_scroll = state.summary_scroll.saturating_sub(1);
                return Action::None;
            }
        }

        // Info tab: j/k scrolls
        if self.active_tab == DetailTab::Info {
            if keys::matches(key, keys::NAV_DOWN_J) || keys::matches(key, keys::NAV_DOWN) {
                let max_scroll = state.info_total_lines.saturating_sub(state.info_visible_height);
                if (state.info_scroll as usize) < max_scroll {
                    state.info_scroll += 1;
                }
                return Action::None;
            }
            if keys::matches(key, keys::NAV_UP_K) || keys::matches(key, keys::NAV_UP) {
                state.info_scroll = state.info_scroll.saturating_sub(1);
                return Action::None;
            }
        }

        if keys::matches(key, keys::CYCLE_NEXT) {
            if let Some(idx) = state.selected_run {
                if idx + 1 < state.runs.len() {
                    state.selected_run = Some(idx + 1);
                    let _ = state.load_run_preview(idx + 1);
                    self.load_metrics_for_selected_run(state);
                }
            }
            return Action::None;
        }

        if keys::matches(key, keys::CYCLE_PREV) {
            if let Some(idx) = state.selected_run {
                if idx > 0 {
                    state.selected_run = Some(idx - 1);
                    let _ = state.load_run_preview(idx - 1);
                    self.load_metrics_for_selected_run(state);
                }
            }
            return Action::None;
        }

        if keys::matches(key, keys::COMPARE) {
            if state.selected_runs_for_compare.len() >= 2 {
                if state.load_compare_data().is_ok() {
                    state.current_view = View::Compare;
                    return Action::Navigate(View::Compare);
                }
            }
            return Action::None;
        }

        if keys::matches(key, keys::DIFF) {
            if state.selected_runs_for_compare.len() >= 2 {
                if state.load_compare_data().is_ok() {
                    state.current_view = View::Diff;
                    return Action::Navigate(View::Diff);
                }
            }
            return Action::None;
        }

        if keys::matches(key, keys::DELETE) {
            if let Some(run) = state.selected_run.and_then(|i| state.runs.get(i)) {
                let run_id = run.id.clone();
                let label = run.name.clone().unwrap_or_else(|| {
                    if run_id.len() > 8 {
                        run_id[run_id.len() - 8..].to_string()
                    } else {
                        run_id.clone()
                    }
                });
                state.delete_confirm = Some(crate::app::DeleteConfirmState {
                    target: crate::app::DeleteTarget::Run { run_id },
                    label,
                });
            }
            return Action::None;
        }

        if keys::matches(key, keys::QUIT) {
            return Action::Quit;
        }

        Action::None
    }

    fn load_metrics_for_selected_run(&mut self, state: &mut AppState) {
        if let Some(run_idx) = state.selected_run {
            if let Some(run) = state.runs.get(run_idx) {
                state.metrics = state
                    .db
                    .get_latest_metrics(&run.id)
                    .unwrap_or_default();
            }
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, state: &mut AppState) {
        let focused = state.focus == Focus::Detail;
        let border_style = if focused {
            Style::default().fg(self.theme.border_focused)
        } else {
            Style::default().fg(self.theme.border)
        };

        let run_indicator = if focused && state.runs.len() > 1 {
            format!(
                " run {}/{} ",
                state.selected_run.map(|i| i + 1).unwrap_or(0),
                state.runs.len()
            )
        } else {
            String::new()
        };

        let block = Block::bordered()
            .title(format!(" 2 Detail{run_indicator}"))
            .border_style(border_style)
            .border_set(border::ROUNDED);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let selected_run = state.selected_run.and_then(|i| state.runs.get(i).cloned());

        let Some(run) = selected_run else {
            let msg = Paragraph::new("Select an experiment and run to view details.")
                .style(Style::default().fg(self.theme.accent_dim));
            frame.render_widget(msg, inner);
            return;
        };

        if focused {
            // Split inner into tab bar + content
            let chunks = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).split(inner);
            self.render_tab_bar(frame, chunks[0]);
            match self.active_tab {
                DetailTab::Summary => self.render_summary(frame, chunks[1], state),
                DetailTab::Info => self.render_info(frame, chunks[1], &run, state),
            }
        } else {
            match self.active_tab {
                DetailTab::Summary => self.render_summary(frame, inner, state),
                DetailTab::Info => self.render_info(frame, inner, &run, state),
            }
        }
    }

    fn render_tab_bar(&self, frame: &mut Frame, area: Rect) {
        let mnemonic = Style::default()
            .fg(self.theme.accent)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);

        let mut spans: Vec<Span> = Vec::new();

        // Summary tab
        let sum_style = if self.active_tab == DetailTab::Summary {
            self.theme.tab_active
        } else {
            self.theme.tab_inactive
        };
        spans.push(Span::raw(" ["));
        spans.push(Span::styled("S", mnemonic));
        spans.push(Span::styled("ummary]", sum_style));

        // Info tab
        let info_style = if self.active_tab == DetailTab::Info {
            self.theme.tab_active
        } else {
            self.theme.tab_inactive
        };
        spans.push(Span::raw(" ["));
        spans.push(Span::styled("I", mnemonic));
        spans.push(Span::styled("nfo]", info_style));

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn render_summary(&self, frame: &mut Frame, area: Rect, state: &mut AppState) {
        // Build SummaryData from the SelectionSummary::Leaf
        let (name, runs, run_metrics, aggregate_metrics, unique_configs) =
            match &state.selection_summary {
                SelectionSummary::Leaf {
                    name,
                    runs,
                    run_metrics,
                    aggregate_metrics,
                    unique_configs,
                } => (
                    name.clone(),
                    runs.clone(),
                    run_metrics.clone(),
                    aggregate_metrics.clone(),
                    *unique_configs,
                ),
                _ => return,
            };

        // Resolve the preview run's total_steps for the chart x-axis pin.
        // Per-run detail view uses state.selected_run; leaf preview falls back
        // to the same picker as reload_leaf_preview_data so the pinned axis
        // matches the loaded data.
        let preview_total_steps = if let Some(idx) = state.selected_run {
            state.runs.get(idx).and_then(|r| r.total_steps)
        } else {
            state.leaf_preview_run().and_then(|r| r.total_steps)
        };

        let data = SummaryData {
            name: &name,
            runs: &runs,
            run_metrics: &run_metrics,
            aggregate_metrics: &aggregate_metrics,
            unique_configs,
            run_params: &state.run_params,
            metric_histories: &state.metric_histories,
            table: state.cached_table.as_ref(),
            table_title: state.cached_table_title.as_deref(),
            table_axes: state
                .cached_table_axes
                .as_ref()
                .map(|(r, c)| (r.as_str(), c.as_str())),
            preview_total_steps,
        };

        let sections = state.config.summary.sections.clone();
        let total = self.summary.render(
            frame,
            area,
            &data,
            &sections,
            state.summary_scroll,
            state.config.summary.curve_width,
            state.config.summary.curve_height,
            state.config.summary.curve_smooth,
            &state.config.tables,
        );
        state.summary_total_lines = total;
        state.summary_visible_height = area.height as usize;
    }

    fn render_info(&self, frame: &mut Frame, area: Rect, run: &Run, state: &mut AppState) {
        use crate::config::key_passes_filters;

        let mut lines = Vec::new();

        // Build metadata rows as aligned key-value pairs.
        let mut meta: Vec<(&str, String, Option<Style>)> = Vec::new();
        meta.push(("Run ID", run.id.clone(), None));
        if let Some(ref name) = run.name {
            meta.push(("Name", name.clone(), None));
        }
        let status_style = match run.status.as_str() {
            "running" => Some(self.theme.status_running),
            "completed" => Some(self.theme.status_completed),
            "failed" => Some(self.theme.status_failed),
            _ => None,
        };
        meta.push(("Status", run.status.clone(), status_style));
        meta.push(("Started", run.started_at.clone(), None));
        if let Some(ref ended) = run.ended_at {
            meta.push(("Ended", ended.clone(), None));
        }
        if let Some(ref hostname) = run.hostname {
            meta.push(("Host", hostname.clone(), None));
        }
        if let Some(ref git_sha) = run.git_sha {
            meta.push(("Git SHA", git_sha.clone(), None));
        }
        if let Some(ref tags) = run.tags {
            meta.push(("Tags", tags.clone(), None));
        }

        let meta_key_width = meta.iter().map(|(k, _, _)| k.len()).max().unwrap_or(4);
        for (label, value, val_style) in &meta {
            let val_span = if let Some(s) = val_style {
                Span::styled(value.clone(), *s)
            } else {
                Span::raw(value.clone())
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {:<width$}  ", label, width = meta_key_width),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                val_span,
            ]));
        }

        if let Some(ref notes) = run.notes {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Notes: ",
                Style::default().add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(notes.clone()));
        }

        if let Some(ref config) = run.config {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Config",
                Style::default().add_modifier(Modifier::BOLD),
            )));
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(config) {
                if let Some(obj) = parsed.as_object() {
                    // Flatten nested config into (dotted_key, leaf_value) pairs.
                    let mut flat: Vec<(String, String)> = Vec::new();
                    fn flatten(
                        prefix: &str,
                        value: &serde_json::Value,
                        out: &mut Vec<(String, String)>,
                    ) {
                        match value {
                            serde_json::Value::Object(map) => {
                                for (k, v) in map {
                                    let key = if prefix.is_empty() {
                                        k.clone()
                                    } else {
                                        format!("{prefix}.{k}")
                                    };
                                    flatten(&key, v, out);
                                }
                            }
                            serde_json::Value::String(s) => {
                                out.push((prefix.to_string(), s.clone()));
                            }
                            serde_json::Value::Null => {
                                out.push((prefix.to_string(), "null".to_string()));
                            }
                            serde_json::Value::Bool(b) => {
                                out.push((prefix.to_string(), b.to_string()));
                            }
                            serde_json::Value::Number(n) => {
                                out.push((prefix.to_string(), n.to_string()));
                            }
                            serde_json::Value::Array(arr) => {
                                out.push((
                                    prefix.to_string(),
                                    serde_json::to_string(arr).unwrap_or_default(),
                                ));
                            }
                        }
                    }
                    for (k, v) in obj {
                        flatten(k, v, &mut flat);
                    }

                    // Apply field filter from config.
                    let filters = &state.config.info.fields;
                    flat.retain(|(k, _)| key_passes_filters(k, filters));

                    let key_width =
                        flat.iter().map(|(k, _)| k.len()).max().unwrap_or(8).max(4);
                    for (k, val_str) in &flat {
                        lines.push(Line::from(vec![
                            Span::styled(
                                format!("  {:<width$}  ", k, width = key_width),
                                Style::default().fg(self.theme.accent_dim),
                            ),
                            Span::raw(val_str.clone()),
                        ]));
                    }
                } else {
                    lines.push(Line::from(format!("  {config}")));
                }
            } else {
                lines.push(Line::from(format!("  {config}")));
            }
        }

        state.info_total_lines = lines.len();
        state.info_visible_height = area.height as usize;
        let paragraph = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((state.info_scroll, 0));
        frame.render_widget(paragraph, area);
    }
}
