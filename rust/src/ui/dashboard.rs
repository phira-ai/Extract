use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::{AppState, SelectionSummary};
use crate::model::{MetricRanking, Run};
use crate::ui::summary::{SummaryData, SummaryRenderer};
use crate::ui::theme::Theme;

pub struct Dashboard {
    theme: Theme,
    summary: SummaryRenderer,
}

impl Dashboard {
    pub fn new() -> Self {
        Self {
            theme: Theme::default(),
            summary: SummaryRenderer::new(),
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, state: &mut AppState) {
        match &state.selection_summary {
            SelectionSummary::Root {
                total_experiments,
                total_runs,
                recent_runs,
            } => self.render_root(frame, area, *total_experiments, *total_runs, recent_runs),
            SelectionSummary::Branch {
                name,
                path,
                child_type,
                descendant_experiments,
                total_runs,
                runs_by_status,
                children,
                rankings,
            } => self.render_branch(
                frame,
                area,
                name,
                path,
                child_type.as_deref(),
                *descendant_experiments,
                *total_runs,
                runs_by_status,
                children,
                rankings,
            ),
            SelectionSummary::Leaf {
                name,
                runs,
                run_metrics,
                aggregate_metrics,
                unique_configs,
            } => {
                let data = SummaryData {
                    name,
                    runs,
                    run_metrics,
                    aggregate_metrics,
                    unique_configs: *unique_configs,
                    metric_history: &state.metric_history,
                    metric_name: state.available_metric_names.first().map(|s| s.as_str()),
                    matrix: state.cached_matrix.as_ref(),
                    matrix_title: state.cached_matrix_title.as_deref(),
                    matrix_axes: state
                        .cached_matrix_axes
                        .as_ref()
                        .map(|(r, c)| (r.as_str(), c.as_str())),
                };
                let total = self.summary.render(
                    frame,
                    area,
                    &data,
                    &state.config.summary.sections,
                    state.summary_scroll,
                );
                state.summary_total_lines = total;
            }
        }
    }

    fn render_root(
        &self,
        frame: &mut Frame,
        area: Rect,
        total_experiments: usize,
        total_runs: i64,
        recent_runs: &[Run],
    ) {
        let mut lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "  Extract Experiment Tracker",
                self.theme.header,
            )),
            Line::from(""),
            Line::from(format!("  Experiments: {total_experiments}")),
            Line::from(format!("  Total runs:  {total_runs}")),
        ];

        if !recent_runs.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  Recent Activity",
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(self.separator());

            for run in recent_runs {
                let status_style = self.status_style(&run.status);
                let date = run.started_at.get(..10).unwrap_or(&run.started_at);
                let name = run.name.as_deref().unwrap_or(
                    run.id.get(..8).unwrap_or(&run.id),
                );
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(format!("{:<11}", run.status), status_style),
                    Span::raw(format!(" {:<16}", name)),
                    Span::styled(date, Style::default().fg(self.theme.accent_dim)),
                ]));
            }
        }

        if total_runs == 0 {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  Navigate the tree and press Enter to select an experiment.",
                Style::default().fg(self.theme.accent_dim),
            )));
        }

        frame.render_widget(Paragraph::new(lines), area);
    }

    fn render_branch(
        &self,
        frame: &mut Frame,
        area: Rect,
        _name: &str,
        path: &str,
        child_type: Option<&str>,
        descendant_experiments: i64,
        total_runs: i64,
        runs_by_status: &[(String, i64)],
        children: &[(String, i64)],
        rankings: &[MetricRanking],
    ) {
        let mut lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                format!("  {path}"),
                self.theme.header,
            )),
            Line::from(""),
            Line::from(format!(
                "  {descendant_experiments} experiments \u{00b7} {total_runs} runs"
            )),
        ];

        if !runs_by_status.is_empty() {
            let status_parts: Vec<String> = runs_by_status
                .iter()
                .map(|(status, count)| format!("{status}: {count}"))
                .collect();
            lines.push(Line::from(format!("  {}", status_parts.join("  "))));
        }

        if !children.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  Children",
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(self.separator());

            for (child_name, run_count) in children {
                let run_label = if *run_count == 1 { "run" } else { "runs" };
                lines.push(Line::from(vec![
                    Span::raw(format!("  {:<28}", child_name)),
                    Span::styled(
                        format!("{run_count} {run_label}"),
                        Style::default().fg(self.theme.accent_dim),
                    ),
                ]));
            }
        }

        if !rankings.is_empty() {
            lines.push(Line::from(""));
            let ranking_title = match child_type {
                Some(t) => format!("  Rankings ({t}s)"),
                None => "  Rankings".to_string(),
            };
            lines.push(Line::from(Span::styled(
                ranking_title,
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(self.separator());

            for ranking in rankings {
                let arrow = if ranking.lower_is_better {
                    "\u{2193}" // ↓
                } else {
                    "\u{2191}" // ↑
                };
                lines.push(Line::from(Span::styled(
                    format!("  {} {arrow}", ranking.metric_name),
                    Style::default().add_modifier(Modifier::BOLD),
                )));
                for (rank, (name, value)) in ranking.entries.iter().enumerate() {
                    let rank_num = rank + 1;
                    let style = if rank == 0 {
                        self.theme.status_completed // green for best
                    } else {
                        Style::default()
                    };
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("    {rank_num}. "),
                            Style::default().fg(self.theme.accent_dim),
                        ),
                        Span::styled(format!("{:<24}", name), style),
                        Span::raw(format!("{:.4}", value)),
                    ]));
                }
                lines.push(Line::from(""));
            }
        }

        frame.render_widget(Paragraph::new(lines), area);
    }

    fn separator(&self) -> Line<'static> {
        Line::from(Span::styled(
            "  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
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
