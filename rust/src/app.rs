use std::path::PathBuf;

use color_eyre::Result;
use serde_json::Value as JsonValue;

use crate::artifact::TableData;
use crate::config::{self, Config};
use crate::db::Db;
use std::collections::HashMap;

use crate::model::{is_lower_better, Artifact, Experiment, MetricAggregate, MetricRanking, Run, RunParam, ScalarMetric};

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

/// Per-run data loaded for comparison.
pub struct CompareRunData {
    pub run: Run,
    pub latest_metrics: Vec<ScalarMetric>,
    pub run_params: Vec<RunParam>,
    pub config: Option<JsonValue>,
    pub metric_histories: Vec<(String, Vec<ScalarMetric>)>,
    pub tables: Vec<(String, TableData, Option<(String, String)>)>,
}

pub fn format_json_value(v: &JsonValue) -> String {
    match v {
        JsonValue::Null => "null".to_string(),
        JsonValue::Bool(b) => b.to_string(),
        JsonValue::Number(n) => n.to_string(),
        JsonValue::String(s) => s.clone(),
        JsonValue::Array(a) => format!("[{}]", a.len()),
        JsonValue::Object(o) => format!("{{{}}}", o.len()),
    }
}

impl CompareRunData {
    pub fn label(&self) -> String {
        self.run.name.clone().unwrap_or_else(|| {
            let id = &self.run.id;
            if id.len() > 8 {
                id[id.len() - 8..].to_string()
            } else {
                id.clone()
            }
        })
    }
}

/// All data needed for Compare/Diff views.
pub struct CompareData {
    pub runs: Vec<CompareRunData>,
    pub metric_names: Vec<String>,
    pub param_names: Vec<String>,
    pub config_keys: Vec<String>,
    pub table_names: Vec<String>,
    pub scroll: u16,
    pub total_lines: usize,
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
    pub run_params: Vec<RunParam>,
    pub metric_histories: Vec<(String, Vec<ScalarMetric>)>,
    pub selection_summary: SelectionSummary,
    pub summary_scroll: u16,
    pub summary_total_lines: usize,
    pub cached_table: Option<TableData>,
    pub cached_table_artifact_id: Option<String>,
    pub cached_table_axes: Option<(String, String)>,
    pub cached_table_title: Option<String>,
    pub compare_data: Option<CompareData>,
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
            run_params: Vec::new(),
            metric_histories: Vec::new(),
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
            compare_data: None,
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

    /// Load all metric histories for a given run.
    fn load_all_metric_histories(&mut self, run_id: &str) -> Result<()> {
        let all = self.db.get_scalar_metrics(run_id, None)?;
        let mut names: Vec<String> = Vec::new();
        for m in &all {
            if !names.contains(&m.name) {
                names.push(m.name.clone());
            }
        }
        self.metric_histories = names
            .into_iter()
            .map(|name| {
                let history = self
                    .db
                    .get_scalar_metrics(run_id, Some(&name))
                    .unwrap_or_default();
                (name, history)
            })
            .collect();
        Ok(())
    }

    /// Load preview data (metric history + matrix) for a leaf experiment.
    /// Uses the latest completed run, or the first run if none completed.
    pub fn refresh_leaf_preview(&mut self) -> Result<()> {
        self.summary_scroll = 0;

        if self.runs.is_empty() {
            self.metric_histories.clear();
            self.run_params.clear();
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

        // Load all metric histories and params for the preview run
        self.load_all_metric_histories(&run_id)?;
        self.run_params = self.db.list_run_params(&run_id)?;

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

    pub fn load_compare_data(&mut self) -> Result<()> {
        let ids = self.selected_runs_for_compare.clone();
        let mut runs_data = Vec::new();
        let mut seen_run_ids = Vec::new();

        for id in &ids {
            // Try as run ID first, then as experiment ID
            let run = if let Some(run) = self.db.get_run(id)? {
                run
            } else {
                let runs = self.db.list_runs(id)?;
                match runs
                    .iter()
                    .rev()
                    .find(|r| r.status == "completed")
                    .or(runs.last())
                {
                    Some(r) => r.clone(),
                    None => continue,
                }
            };

            if seen_run_ids.contains(&run.id) {
                continue;
            }
            seen_run_ids.push(run.id.clone());

            let latest_metrics = self.db.get_latest_metrics(&run.id)?;
            let run_params = self.db.list_run_params(&run.id)?;
            let config: Option<JsonValue> =
                run.config.as_ref().and_then(|c| serde_json::from_str(c).ok());

            // Load metric histories
            let all = self.db.get_scalar_metrics(&run.id, None)?;
            let mut names: Vec<String> = Vec::new();
            for m in &all {
                if !names.contains(&m.name) {
                    names.push(m.name.clone());
                }
            }
            let metric_histories: Vec<(String, Vec<ScalarMetric>)> = names
                .into_iter()
                .map(|name| {
                    let history = self
                        .db
                        .get_scalar_metrics(&run.id, Some(&name))
                        .unwrap_or_default();
                    (name, history)
                })
                .collect();

            // Load table artifacts
            let artifacts = self.db.list_artifacts(&run.id)?;
            let mut tables = Vec::new();
            for artifact in artifacts.iter().filter(|a| a.kind == "matrix") {
                let path = self.store_root.join(&artifact.rel_path);
                if let Ok(table) = crate::artifact::load_table(&path) {
                    let axes = artifact.metadata.as_ref().and_then(|m| {
                        let parsed: serde_json::Value = serde_json::from_str(m).ok()?;
                        let axes_obj = parsed.get("axes")?;
                        let rows = axes_obj.get("rows")?.as_str()?.to_string();
                        let cols = axes_obj.get("cols")?.as_str()?.to_string();
                        Some((rows, cols))
                    });
                    tables.push((artifact.name.clone(), table, axes));
                }
            }

            runs_data.push(CompareRunData {
                run,
                latest_metrics,
                run_params,
                config,
                metric_histories,
                tables,
            });
        }

        // Compute union of names
        let mut metric_names = Vec::new();
        let mut param_names = Vec::new();
        let mut config_keys = Vec::new();
        let mut table_names = Vec::new();

        for rd in &runs_data {
            for m in &rd.latest_metrics {
                if !metric_names.contains(&m.name) {
                    metric_names.push(m.name.clone());
                }
            }
            for p in &rd.run_params {
                if !param_names.contains(&p.name) {
                    param_names.push(p.name.clone());
                }
            }
            if let Some(config) = &rd.config {
                if let Some(obj) = config.as_object() {
                    for key in obj.keys() {
                        if !config_keys.contains(key) {
                            config_keys.push(key.clone());
                        }
                    }
                }
            }
            for (name, _, _) in &rd.tables {
                if !table_names.contains(name) {
                    table_names.push(name.clone());
                }
            }
        }

        metric_names.sort();
        param_names.sort();
        config_keys.sort();
        table_names.sort();

        self.compare_data = Some(CompareData {
            runs: runs_data,
            metric_names,
            param_names,
            config_keys,
            table_names,
            scroll: 0,
            total_lines: 0,
        });

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
