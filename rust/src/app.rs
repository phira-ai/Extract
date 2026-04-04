use std::path::PathBuf;

use color_eyre::Result;

use crate::artifact::TableData;
use crate::config::{self, Config};
use crate::db::Db;
use std::collections::HashMap;

use crate::model::{is_lower_better, Artifact, Experiment, MetricAggregate, MetricRanking, Run, ScalarMetric};

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
        child_type: Option<String>,
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
    pub hierarchy: Vec<String>,
    pub config: Config,
    pub current_view: View,
    pub focus: Focus,
    pub should_quit: bool,
    pub experiments: Vec<Experiment>,
    pub selected_experiment: Option<usize>,
    pub runs: Vec<Run>,
    pub selected_run: Option<usize>,
    pub selected_runs_for_compare: Vec<String>,
    pub metrics: Vec<ScalarMetric>,
    pub artifacts: Vec<Artifact>,
    pub metric_history: Vec<ScalarMetric>,
    pub available_metric_names: Vec<String>,
    pub selected_metric_idx: usize,
    pub selection_summary: SelectionSummary,
    pub summary_scroll: u16,
    pub summary_total_lines: usize,
    pub cached_table: Option<TableData>,
    pub cached_table_artifact_id: Option<String>,
    pub cached_table_axes: Option<(String, String)>,
    pub cached_table_title: Option<String>,
}

impl AppState {
    pub fn new(db: Db, store_root: PathBuf) -> Result<Self> {
        let experiments = db.list_experiments()?;
        let total_runs = db.count_all_runs()?;
        let recent_runs = db.recent_runs(5)?;
        let total_experiments = experiments.len();
        let hierarchy = db.list_hierarchy()?;
        let config = config::load_config(&store_root);
        Ok(Self {
            db,
            store_root,
            hierarchy,
            config,
            current_view: View::Explorer,
            focus: Focus::Tree,
            should_quit: false,
            experiments,
            selected_experiment: None,
            runs: Vec::new(),
            selected_run: None,
            selected_runs_for_compare: Vec::new(),
            metrics: Vec::new(),
            artifacts: Vec::new(),
            metric_history: Vec::new(),
            available_metric_names: Vec::new(),
            selected_metric_idx: 0,
            selection_summary: SelectionSummary::Root {
                total_experiments,
                total_runs,
                recent_runs,
            },
            summary_scroll: 0,
            summary_total_lines: 0,
            cached_table: None,
            cached_table_artifact_id: None,
            cached_table_axes: None,
            cached_table_title: None,
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

    pub fn refresh_artifacts(&mut self) -> Result<()> {
        if let Some(run) = self.selected_run.and_then(|i| self.runs.get(i)) {
            self.artifacts = self.db.list_artifacts(&run.id)?;
        } else {
            self.artifacts.clear();
        }
        Ok(())
    }

    pub fn refresh_metric_history(&mut self) -> Result<()> {
        let Some(run) = self.selected_run.and_then(|i| self.runs.get(i)) else {
            self.metric_history.clear();
            self.available_metric_names.clear();
            return Ok(());
        };

        // Get distinct metric names for this run
        let all = self.db.get_scalar_metrics(&run.id, None)?;
        let mut names: Vec<String> = Vec::new();
        for m in &all {
            if !names.contains(&m.name) {
                names.push(m.name.clone());
            }
        }
        self.available_metric_names = names;

        // Load full history for selected metric
        if let Some(name) = self.available_metric_names.get(self.selected_metric_idx) {
            self.metric_history = self.db.get_scalar_metrics(&run.id, Some(name))?;
        } else {
            self.metric_history.clear();
        }

        Ok(())
    }

    /// Load preview data (metric history + matrix) for a leaf experiment.
    /// Uses the latest completed run, or the first run if none completed.
    pub fn refresh_leaf_preview(&mut self) -> Result<()> {
        self.summary_scroll = 0;

        if self.runs.is_empty() {
            self.metric_history.clear();
            self.available_metric_names.clear();
            self.artifacts.clear();
            self.cached_table = None;
            self.cached_table_artifact_id = None;
            return Ok(());
        }

        // Pick a preview run: latest completed, or first
        let preview_run = self
            .runs
            .iter()
            .rev()
            .find(|r| r.status == "completed")
            .or(self.runs.first());

        let Some(run) = preview_run else {
            return Ok(());
        };
        let run_id = run.id.clone();

        // Load metric names and history for first metric
        let all = self.db.get_scalar_metrics(&run_id, None)?;
        let mut names: Vec<String> = Vec::new();
        for m in &all {
            if !names.contains(&m.name) {
                names.push(m.name.clone());
            }
        }
        self.available_metric_names = names;
        self.selected_metric_idx = 0;

        if let Some(name) = self.available_metric_names.first() {
            self.metric_history = self.db.get_scalar_metrics(&run_id, Some(name))?;
        } else {
            self.metric_history.clear();
        }

        // Load artifacts and cache first table (matrix artifact)
        self.artifacts = self.db.list_artifacts(&run_id)?;
        self.load_first_table()?;

        Ok(())
    }

    fn load_first_table(&mut self) -> Result<()> {
        let table_artifact = self.artifacts.iter().find(|a| a.kind == "matrix");

        let Some(artifact) = table_artifact else {
            self.cached_table = None;
            self.cached_table_artifact_id = None;
            self.cached_table_axes = None;
            self.cached_table_title = None;
            return Ok(());
        };

        if self.cached_table_artifact_id.as_deref() == Some(&artifact.id) {
            return Ok(());
        }

        let path = self.store_root.join(&artifact.rel_path);
        match crate::artifact::load_table(&path) {
            Ok(table) => {
                let axes = artifact.metadata.as_ref().and_then(|m| {
                    let parsed: serde_json::Value = serde_json::from_str(m).ok()?;
                    let axes_obj = parsed.get("axes")?;
                    let rows = axes_obj.get("rows")?.as_str()?.to_string();
                    let cols = axes_obj.get("cols")?.as_str()?.to_string();
                    Some((rows, cols))
                });
                self.cached_table = Some(table);
                self.cached_table_artifact_id = Some(artifact.id.clone());
                self.cached_table_title = Some(artifact.name.clone());
                self.cached_table_axes = axes;
            }
            Err(_) => {
                self.cached_table = None;
                self.cached_table_artifact_id = None;
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
            let child_type = self
                .experiments
                .iter()
                .filter(|e| e.parent_id.as_deref() == Some(&exp.id))
                .find_map(|e| e.node_type.clone());
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
                child_type,
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
