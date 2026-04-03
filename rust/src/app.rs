use std::path::PathBuf;

use color_eyre::Result;

use crate::db::Db;
use std::collections::HashMap;

use crate::model::{is_lower_better, Experiment, MetricAggregate, MetricRanking, Run, ScalarMetric};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Explorer,
    Detail,
    Compare,
    Diff,
    Registry,
    Lineage,
    TodoGlobal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Tree,
    Detail,
}

#[derive(Debug, Clone)]
pub enum Action {
    None,
    Navigate(View),
    Quit,
    Refresh,
}

pub enum SelectionSummary {
    Root {
        total_experiments: usize,
        total_runs: i64,
        recent_runs: Vec<Run>,
    },
    Branch {
        name: String,
        path: String,
        descendant_experiments: i64,
        total_runs: i64,
        runs_by_status: Vec<(String, i64)>,
        children: Vec<(String, i64)>,
        rankings: Vec<MetricRanking>,
    },
    Leaf {
        name: String,
        runs: Vec<Run>,
        run_metrics: Vec<Vec<ScalarMetric>>,
        aggregate_metrics: Vec<MetricAggregate>,
        unique_configs: i64,
    },
}

pub struct AppState {
    pub db: Db,
    pub store_root: PathBuf,
    pub current_view: View,
    pub focus: Focus,
    pub should_quit: bool,
    pub experiments: Vec<Experiment>,
    pub selected_experiment: Option<usize>,
    pub runs: Vec<Run>,
    pub selected_run: Option<usize>,
    pub selected_runs_for_compare: Vec<String>,
    pub metrics: Vec<ScalarMetric>,
    pub selection_summary: SelectionSummary,
}

impl AppState {
    pub fn new(db: Db, store_root: PathBuf) -> Result<Self> {
        let experiments = db.list_experiments()?;
        let total_runs = db.count_all_runs()?;
        let recent_runs = db.recent_runs(5)?;
        let total_experiments = experiments.len();
        Ok(Self {
            db,
            store_root,
            current_view: View::Explorer,
            focus: Focus::Tree,
            should_quit: false,
            experiments,
            selected_experiment: None,
            runs: Vec::new(),
            selected_run: None,
            selected_runs_for_compare: Vec::new(),
            metrics: Vec::new(),
            selection_summary: SelectionSummary::Root {
                total_experiments,
                total_runs,
                recent_runs,
            },
        })
    }

    pub fn refresh_experiments(&mut self) -> Result<()> {
        self.experiments = self.db.list_experiments()?;
        Ok(())
    }

    pub fn refresh_runs(&mut self) -> Result<()> {
        if let Some(idx) = self.selected_experiment {
            if let Some(exp) = self.experiments.get(idx) {
                self.runs = self.db.list_runs(&exp.id)?;
            }
        }
        Ok(())
    }

    pub fn refresh_selection_summary(&mut self) -> Result<()> {
        let Some(idx) = self.selected_experiment else {
            self.selection_summary = SelectionSummary::Root {
                total_experiments: self.experiments.len(),
                total_runs: self.db.count_all_runs()?,
                recent_runs: self.db.recent_runs(5)?,
            };
            return Ok(());
        };

        let Some(exp) = self.experiments.get(idx).cloned() else {
            return Ok(());
        };

        let has_children = self
            .experiments
            .iter()
            .any(|e| e.parent_id.as_deref() == Some(&exp.id));

        if has_children {
            // Build per-metric rankings of children
            let raw = self.db.child_best_metrics(&exp.id)?;
            let mut metric_map: HashMap<String, Vec<(String, f64, f64)>> = HashMap::new();
            for (child, metric, min_val, max_val) in raw {
                metric_map
                    .entry(metric)
                    .or_default()
                    .push((child, min_val, max_val));
            }
            let mut rankings: Vec<MetricRanking> = metric_map
                .into_iter()
                .map(|(metric_name, entries)| {
                    let lower = is_lower_better(&metric_name);
                    let mut ranked: Vec<(String, f64)> = entries
                        .iter()
                        .map(|(name, min_val, max_val)| {
                            (name.clone(), if lower { *min_val } else { *max_val })
                        })
                        .collect();
                    ranked.sort_by(|a, b| {
                        if lower {
                            a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)
                        } else {
                            b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
                        }
                    });
                    MetricRanking {
                        metric_name,
                        lower_is_better: lower,
                        entries: ranked,
                    }
                })
                .collect();
            rankings.sort_by(|a, b| a.metric_name.cmp(&b.metric_name));

            self.selection_summary = SelectionSummary::Branch {
                name: exp.name.clone(),
                path: exp.path.clone(),
                descendant_experiments: self.db.count_descendant_experiments(&exp.path)?,
                total_runs: self.db.count_runs_for_subtree(&exp.path)?,
                runs_by_status: self.db.runs_by_status_for_subtree(&exp.path)?,
                children: self.db.run_counts_by_child(&exp.id)?,
                rankings,
            };
        } else {
            let mut run_metrics = Vec::new();
            for run in &self.runs {
                let metrics = self.db.get_latest_metrics(&run.id)?;
                run_metrics.push(metrics);
            }
            self.selection_summary = SelectionSummary::Leaf {
                name: exp.name.clone(),
                runs: self.runs.clone(),
                run_metrics,
                aggregate_metrics: self.db.aggregate_final_metrics(&exp.id)?,
                unique_configs: self.db.count_unique_configs(&exp.id)?,
            };
        }

        Ok(())
    }
}
