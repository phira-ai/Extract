use crossterm::event::KeyEvent;
use ndarray::Array2;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Row, Table, TableState, Wrap};
use ratatui::Frame;

use crate::app::{Action, AppState, Focus, View};
use crate::artifact::load_npy_matrix;
use crate::event::AppEvent;
use crate::keys;
use crate::model::Run;
use crate::ui::chart::ChartView;
use crate::ui::heatmap::HeatmapView;
use crate::ui::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailTab {
    Scalars,
    Curves,
    Matrix,
    Info,
}

pub struct DetailPanel {
    pub active_tab: DetailTab,
    pub table_state: TableState,
    chart: ChartView,
    heatmap: HeatmapView,
    cached_matrix: Option<Array2<f64>>,
    cached_matrix_artifact_id: Option<String>,
    cached_matrix_axes: Option<(String, String)>,
    theme: Theme,
}

impl DetailPanel {
    pub fn new() -> Self {
        Self {
            active_tab: DetailTab::Scalars,
            table_state: TableState::default(),
            chart: ChartView::new(),
            heatmap: HeatmapView::new(),
            cached_matrix: None,
            cached_matrix_artifact_id: None,
            cached_matrix_axes: None,
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
        if keys::matches(key, keys::TAB) {
            self.active_tab = match self.active_tab {
                DetailTab::Scalars => DetailTab::Curves,
                DetailTab::Curves => DetailTab::Matrix,
                DetailTab::Matrix => DetailTab::Info,
                DetailTab::Info => DetailTab::Scalars,
            };
            return Action::None;
        }

        if keys::matches_shift(key, keys::TAB) {
            self.active_tab = match self.active_tab {
                DetailTab::Scalars => DetailTab::Info,
                DetailTab::Curves => DetailTab::Scalars,
                DetailTab::Matrix => DetailTab::Curves,
                DetailTab::Info => DetailTab::Matrix,
            };
            return Action::None;
        }

        if keys::matches(key, keys::BACK_ESC) {
            state.focus = Focus::Tree;
            state.current_view = View::Explorer;
            return Action::None;
        }

        // Curves tab: j/k switches between metrics
        if self.active_tab == DetailTab::Curves {
            if keys::matches(key, keys::NAV_DOWN_J) || keys::matches(key, keys::NAV_DOWN) {
                if state.selected_metric_idx + 1 < state.available_metric_names.len() {
                    state.selected_metric_idx += 1;
                    let _ = state.refresh_metric_history();
                }
                return Action::None;
            }
            if keys::matches(key, keys::NAV_UP_K) || keys::matches(key, keys::NAV_UP) {
                if state.selected_metric_idx > 0 {
                    state.selected_metric_idx -= 1;
                    let _ = state.refresh_metric_history();
                }
                return Action::None;
            }
        }

        if keys::matches(key, keys::NAV_DOWN_J) || keys::matches(key, keys::NAV_DOWN) {
            if self.active_tab == DetailTab::Scalars {
                self.table_state.select_next();
            } else {
                // Navigate between runs
                if let Some(idx) = state.selected_run {
                    if idx + 1 < state.runs.len() {
                        state.selected_run = Some(idx + 1);
                        self.load_metrics_for_selected_run(state);
                    }
                }
            }
            return Action::None;
        }

        if keys::matches(key, keys::NAV_UP_K) || keys::matches(key, keys::NAV_UP) {
            if self.active_tab == DetailTab::Scalars {
                self.table_state.select_previous();
            } else {
                // Navigate between runs
                if let Some(idx) = state.selected_run {
                    if idx > 0 {
                        state.selected_run = Some(idx - 1);
                        self.load_metrics_for_selected_run(state);
                    }
                }
            }
            return Action::None;
        }

        if keys::matches(key, keys::TOGGLE_SELECT) {
            // Mark current run for comparison
            if let Some(run) = state.selected_run.and_then(|i| state.runs.get(i)) {
                let run_id = run.id.clone();
                if state.selected_runs_for_compare.contains(&run_id) {
                    state.selected_runs_for_compare.retain(|id| id != &run_id);
                } else {
                    state.selected_runs_for_compare.push(run_id);
                }
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
        // Invalidate caches when switching runs
        self.cached_matrix = None;
        self.cached_matrix_artifact_id = None;
        self.cached_matrix_axes = None;
        state.selected_metric_idx = 0;
        let _ = state.refresh_artifacts();
        let _ = state.refresh_metric_history();
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, state: &AppState) {
        let focused = state.focus == Focus::Detail;
        let border_style = if focused {
            Style::default().fg(self.theme.border_focused)
        } else {
            Style::default().fg(self.theme.border)
        };

        let block = Block::bordered()
            .title(" Detail ")
            .border_style(border_style);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        // If no run is selected, show a message
        let selected_run = state
            .selected_run
            .and_then(|i| state.runs.get(i));

        let Some(run) = selected_run else {
            let msg = Paragraph::new("Select an experiment and run to view details.")
                .style(Style::default().fg(self.theme.accent_dim));
            frame.render_widget(msg, inner);
            return;
        };

        // Split inner into tab bar + content
        let chunks = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).split(inner);

        // Render tab bar
        self.render_tab_bar(frame, chunks[0]);

        // Render content based on active tab
        match self.active_tab {
            DetailTab::Scalars => self.render_scalars(frame, chunks[1], state),
            DetailTab::Curves => self.render_curves(frame, chunks[1], state),
            DetailTab::Matrix => self.render_matrix(frame, chunks[1], state),
            DetailTab::Info => self.render_info(frame, chunks[1], run),
        }
    }

    fn render_tab_bar(&self, frame: &mut Frame, area: Rect) {
        let tabs = [
            ("Scalars", DetailTab::Scalars),
            ("Curves", DetailTab::Curves),
            ("Matrix", DetailTab::Matrix),
            ("Info", DetailTab::Info),
        ];

        let spans: Vec<Span> = tabs
            .iter()
            .flat_map(|(label, tab)| {
                let style = if *tab == self.active_tab {
                    self.theme.tab_active
                } else {
                    self.theme.tab_inactive
                };
                vec![
                    Span::raw(" "),
                    Span::styled(format!("[{label}]"), style),
                ]
            })
            .collect();

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn render_scalars(&mut self, frame: &mut Frame, area: Rect, state: &AppState) {
        let header = Row::new(vec!["Metric", "Value", "Step"])
            .style(self.theme.header)
            .bottom_margin(1);

        let rows: Vec<Row> = state
            .metrics
            .iter()
            .map(|m| {
                Row::new(vec![
                    m.name.clone(),
                    format!("{:.6}", m.value),
                    m.step.to_string(),
                ])
            })
            .collect();

        let widths = [
            Constraint::Percentage(40),
            Constraint::Percentage(35),
            Constraint::Percentage(25),
        ];

        let table = Table::new(rows, widths)
            .header(header)
            .row_highlight_style(
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            );

        frame.render_stateful_widget(table, area, &mut self.table_state);
    }

    fn render_curves(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        if state.available_metric_names.is_empty() {
            let msg = Paragraph::new("  No scalar metrics recorded for this run.")
                .style(Style::default().fg(self.theme.accent_dim));
            frame.render_widget(msg, area);
            return;
        }

        // Split: metric selector (1 line) + chart
        let chunks = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);

        // Metric selector
        let metric_name = state
            .available_metric_names
            .get(state.selected_metric_idx)
            .map(|s| s.as_str())
            .unwrap_or("?");

        let selector = Line::from(vec![
            Span::raw("  "),
            Span::styled("\u{25c4} ", Style::default().fg(self.theme.accent_dim)),
            Span::styled(
                metric_name,
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" \u{25ba}", Style::default().fg(self.theme.accent_dim)),
            Span::styled(
                format!(
                    "  ({}/{}  j/k to switch)",
                    state.selected_metric_idx + 1,
                    state.available_metric_names.len()
                ),
                Style::default().fg(self.theme.accent_dim),
            ),
        ]);
        frame.render_widget(Paragraph::new(selector), chunks[0]);

        // Chart
        self.chart
            .render(frame, chunks[1], metric_name, &state.metric_history);
    }

    fn render_matrix(&mut self, frame: &mut Frame, area: Rect, state: &AppState) {
        // Find first matrix artifact for this run
        let matrix_artifact = state.artifacts.iter().find(|a| a.kind == "matrix");

        let Some(artifact) = matrix_artifact else {
            let msg = Paragraph::new("  No matrix artifacts for this run.")
                .style(Style::default().fg(self.theme.accent_dim));
            frame.render_widget(msg, area);
            return;
        };

        // Load matrix if not cached or if artifact changed
        let needs_load = match &self.cached_matrix_artifact_id {
            Some(id) => id != &artifact.id,
            None => true,
        };

        if needs_load {
            let path = state.store_root.join(&artifact.rel_path);
            match load_npy_matrix(&path) {
                Ok(matrix) => {
                    // Parse axes from artifact metadata
                    let axes = artifact.metadata.as_ref().and_then(|m| {
                        let parsed: serde_json::Value = serde_json::from_str(m).ok()?;
                        let axes_obj = parsed.get("axes")?;
                        let rows = axes_obj.get("rows")?.as_str()?.to_string();
                        let cols = axes_obj.get("cols")?.as_str()?.to_string();
                        Some((rows, cols))
                    });
                    self.cached_matrix = Some(matrix);
                    self.cached_matrix_artifact_id = Some(artifact.id.clone());
                    self.cached_matrix_axes = axes;
                }
                Err(e) => {
                    let msg = Paragraph::new(format!("  Error loading matrix: {e}"))
                        .style(Style::default().fg(self.theme.error));
                    frame.render_widget(msg, area);
                    return;
                }
            }
        }

        if let Some(ref matrix) = self.cached_matrix {
            let axes = self
                .cached_matrix_axes
                .as_ref()
                .map(|(r, c)| (r.as_str(), c.as_str()));
            self.heatmap
                .render(frame, area, matrix, &artifact.name, axes);
        }
    }

    fn render_info(&self, frame: &mut Frame, area: Rect, run: &Run) {
        let mut lines = Vec::new();

        lines.push(Line::from(vec![
            Span::styled("Run ID: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&run.id),
        ]));

        if let Some(ref name) = run.name {
            lines.push(Line::from(vec![
                Span::styled("Name: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(name.as_str()),
            ]));
        }

        let status_style = match run.status.as_str() {
            "running" => self.theme.status_running,
            "completed" => self.theme.status_completed,
            "failed" => self.theme.status_failed,
            _ => Style::default(),
        };
        lines.push(Line::from(vec![
            Span::styled("Status: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(run.status.as_str(), status_style),
        ]));

        lines.push(Line::from(vec![
            Span::styled("Started: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(run.started_at.as_str()),
        ]));

        if let Some(ref ended) = run.ended_at {
            lines.push(Line::from(vec![
                Span::styled("Ended: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(ended.as_str()),
            ]));
        }

        if let Some(ref hostname) = run.hostname {
            lines.push(Line::from(vec![
                Span::styled("Host: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(hostname.as_str()),
            ]));
        }

        if let Some(ref git_sha) = run.git_sha {
            lines.push(Line::from(vec![
                Span::styled("Git SHA: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(git_sha.as_str()),
            ]));
        }

        if let Some(ref tags) = run.tags {
            lines.push(Line::from(vec![
                Span::styled("Tags: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(tags.as_str()),
            ]));
        }

        if let Some(ref notes) = run.notes {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("Notes: ", Style::default().add_modifier(Modifier::BOLD)),
            ]));
            lines.push(Line::from(notes.as_str()));
        }

        if let Some(ref config) = run.config {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("Config: ", Style::default().add_modifier(Modifier::BOLD)),
            ]));
            // Pretty-print JSON config
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(config) {
                if let Ok(pretty) = serde_json::to_string_pretty(&parsed) {
                    for line in pretty.lines() {
                        lines.push(Line::from(format!("  {line}")));
                    }
                } else {
                    lines.push(Line::from(format!("  {config}")));
                }
            } else {
                lines.push(Line::from(format!("  {config}")));
            }
        }

        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        frame.render_widget(paragraph, area);
    }
}
