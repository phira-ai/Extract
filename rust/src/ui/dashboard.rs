use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::{AppState, SelectionSummary};
use crate::model::{MetricAggregate, Run, ScalarMetric};
use crate::ui::theme::Theme;

pub struct Dashboard {
    theme: Theme,
}

impl Dashboard {
    pub fn new() -> Self {
        Self {
            theme: Theme::default(),
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        match &state.selection_summary {
            SelectionSummary::Root {
                total_experiments,
                total_runs,
                recent_runs,
            } => self.render_root(frame, area, *total_experiments, *total_runs, recent_runs),
            SelectionSummary::Branch {
                name,
                path,
                descendant_experiments,
                total_runs,
                runs_by_status,
                children,
                metrics,
            } => self.render_branch(
                frame,
                area,
                name,
                path,
                *descendant_experiments,
                *total_runs,
                runs_by_status,
                children,
                metrics,
            ),
            SelectionSummary::Leaf {
                name,
                runs,
                run_metrics,
                aggregate_metrics,
                unique_configs,
            } => self.render_leaf(frame, area, name, runs, run_metrics, aggregate_metrics, *unique_configs),
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
            lines.push(Line::from(Span::styled(
                "  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
                Style::default().fg(self.theme.border),
            )));

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
        descendant_experiments: i64,
        total_runs: i64,
        runs_by_status: &[(String, i64)],
        children: &[(String, i64)],
        metrics: &[MetricAggregate],
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
            lines.push(Line::from(Span::styled(
                "  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
                Style::default().fg(self.theme.border),
            )));

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

        if !metrics.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  Metrics (across all runs)",
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(Span::styled(
                "  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
                Style::default().fg(self.theme.border),
            )));
            self.append_metric_aggregates(&mut lines, metrics);
        }

        frame.render_widget(Paragraph::new(lines), area);
    }

    fn render_leaf(
        &self,
        frame: &mut Frame,
        area: Rect,
        name: &str,
        runs: &[Run],
        run_metrics: &[Vec<ScalarMetric>],
        aggregate_metrics: &[MetricAggregate],
        unique_configs: i64,
    ) {
        let run_count = runs.len();
        let config_hint = if unique_configs > 0 {
            format!(" \u{00b7} {unique_configs} unique configs")
        } else {
            String::new()
        };

        let mut lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                format!("  {name}"),
                self.theme.header,
            )),
            Line::from(format!(
                "  {run_count} {}{config_hint}",
                if run_count == 1 { "run" } else { "runs" }
            )),
        ];

        if !runs.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  Runs",
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(Span::styled(
                "  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
                Style::default().fg(self.theme.border),
            )));

            for (i, run) in runs.iter().enumerate() {
                let status_style = self.status_style(&run.status);
                let date = run.started_at.get(..10).unwrap_or(&run.started_at);

                let mut spans = vec![
                    Span::raw("  "),
                    Span::styled("\u{25cf} ", status_style),
                    Span::styled(format!("{:<11}", run.status), status_style),
                    Span::styled(
                        format!(" {date} "),
                        Style::default().fg(self.theme.accent_dim),
                    ),
                ];

                if let Some(metrics) = run_metrics.get(i) {
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

        if !aggregate_metrics.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  Summary",
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(Span::styled(
                "  \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}",
                Style::default().fg(self.theme.border),
            )));
            self.append_metric_aggregates(&mut lines, aggregate_metrics);
        }

        frame.render_widget(Paragraph::new(lines), area);
    }

    fn append_metric_aggregates(&self, lines: &mut Vec<Line<'_>>, metrics: &[MetricAggregate]) {
        for m in metrics {
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
                lines.push(Line::from(format!(
                    "  {:<14}{:.4}",
                    m.name, m.mean
                )));
            }
        }
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
