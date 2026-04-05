use std::path::PathBuf;
use std::time::Instant;

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
    Selection,
}

#[derive(Debug, Clone)]
pub enum Action {
    None,
    Navigate(View),
    Quit,
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
    pub experiment_name: String,
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
        if let Some(ref name) = self.run.name {
            return name.clone();
        }
        self.experiment_name.clone()
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
    pub visible_height: usize,
}

/// State for the run picker popup.
pub struct RunPickerState {
    pub experiment_name: String,
    pub runs: Vec<Run>,
    pub selected: Vec<String>,
    pub cursor: usize,
}

/// State for the delete confirmation popup.
pub struct DeleteConfirmState {
    pub run_id: String,
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyLevel {
    Success,
    Warn,
    Error,
}

pub struct Notification {
    pub message: String,
    pub level: NotifyLevel,
    pub created_at: Instant,
}

#[derive(Debug, Clone)]
pub struct LineageNode {
    pub entity_type: String,
    pub entity_id: String,
    pub label: String,
    pub layer: usize,
    pub x: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoFilter {
    All,
    Global,
    Experiment,
    Run,
}

pub struct SearchState {
    pub query: String,
    pub results: Vec<crate::model::SearchResult>,
    pub cursor: usize,
}

pub struct AppState {
    pub db: Db,
    pub store_root: PathBuf,
    pub config: Config,
    pub current_view: View,
    pub focus: Focus,
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
    pub summary_visible_height: usize,
    pub cached_table: Option<TableData>,
    pub cached_table_artifact_id: Option<String>,
    pub cached_table_axes: Option<(String, String)>,
    pub cached_table_title: Option<String>,
    pub compare_data: Option<CompareData>,
    pub compare_baseline: usize,
    pub marked_experiment_ids: std::collections::HashSet<String>,
    pub selection_cursor: usize,
    pub run_picker: Option<RunPickerState>,
    pub delete_confirm: Option<DeleteConfirmState>,
    pub notification: Option<Notification>,
    // Phase 5: Registry, Lineage, TODOs
    pub models: Vec<crate::model::Model>,
    pub registry_cursor: usize,
    pub lineage_edges: Vec<crate::model::LineageEdge>,
    pub lineage_nodes: Vec<LineageNode>,
    pub lineage_cursor: usize,
    pub todos: Vec<crate::model::Todo>,
    pub todo_cursor: usize,
    pub todo_input: Option<String>,
    pub todo_filter: TodoFilter,
    /// Chosen scope for the next TODO add (set by picker or directly)
    pub todo_add_scope: Option<(String, Option<String>)>,
    /// Scope picker for adding scoped TODOs
    pub todo_scope_picker: Option<TodoScopePicker>,
    /// When set, tree panel should open ancestors and select this experiment path.
    pub pending_tree_select: Option<String>,
    pub search: Option<SearchState>,
    pub show_help: bool,
    /// True when `g` was pressed once, waiting for second `g` to go to top.
    pub g_pending: bool,
}

pub struct TodoScopePicker {
    pub items: Vec<(String, String)>, // (id, label)
    pub cursor: usize,
    pub scope_type: String, // "experiment" or "run"
}

impl AppState {
    pub fn new(db: Db, store_root: PathBuf) -> Result<Self> {
        let experiments = db.list_experiments()?;
        let total_runs = db.count_all_runs()?;
        let recent_runs = db.recent_runs(5)?;
        let total_experiments = db.count_leaf_experiments()?;
        let config = config::load_config(&store_root);
        Ok(Self {
            db,
            store_root,
            config,
            current_view: View::Explorer,
            focus: Focus::Tree,
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
            summary_visible_height: 0,
            cached_table: None,
            cached_table_artifact_id: None,
            cached_table_axes: None,
            cached_table_title: None,
            compare_data: None,
            compare_baseline: 0,
            marked_experiment_ids: std::collections::HashSet::new(),
            selection_cursor: 0,
            run_picker: None,
            delete_confirm: None,
            notification: None,
            models: Vec::new(),
            registry_cursor: 0,
            lineage_edges: Vec::new(),
            lineage_nodes: Vec::new(),
            lineage_cursor: 0,
            todos: Vec::new(),
            todo_cursor: 0,
            todo_input: None,
            todo_filter: TodoFilter::All,
            todo_add_scope: None,
            todo_scope_picker: None,
            pending_tree_select: None,
            search: None,
            show_help: false,
            g_pending: false,
        })
    }

    pub fn notify(&mut self, level: NotifyLevel, message: impl Into<String>) {
        self.notification = Some(Notification {
            message: message.into(),
            level,
            created_at: Instant::now(),
        });
    }

    pub fn clear_expired_notification(&mut self, timeout_secs: u64) {
        if let Some(ref notif) = self.notification {
            if notif.created_at.elapsed().as_secs() >= timeout_secs {
                self.notification = None;
            }
        }
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

            let experiment_name = self
                .db
                .get_experiment(&run.experiment_id)?
                .map(|e| e.name.clone())
                .unwrap_or_else(|| {
                    let id = &run.id;
                    if id.len() > 8 { id[id.len() - 8..].to_string() } else { id.clone() }
                });

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
                experiment_name,
                latest_metrics,
                run_params,
                config,
                metric_histories,
                tables,
            });
        }

        // Add #N suffix for runs sharing the same experiment_name
        let mut name_counts: HashMap<String, usize> = HashMap::new();
        for rd in &runs_data {
            *name_counts.entry(rd.experiment_name.clone()).or_default() += 1;
        }
        let mut name_indices: HashMap<String, usize> = HashMap::new();
        for rd in &mut runs_data {
            let count = name_counts[&rd.experiment_name];
            if count > 1 {
                let idx = name_indices.entry(rd.experiment_name.clone()).or_insert(0);
                *idx += 1;
                rd.experiment_name = format!("{} #{}", rd.experiment_name, idx);
            }
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
            visible_height: 0,
        });

        Ok(())
    }

    pub fn refresh_selection_summary(&mut self) -> Result<()> {
        let Some(idx) = self.selected_experiment else {
            self.selection_summary = SelectionSummary::Root {
                total_experiments: self.db.count_leaf_experiments()?,
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

    pub fn refresh_marked_experiments(&mut self) {
        self.marked_experiment_ids.clear();
        for run_id in &self.selected_runs_for_compare {
            if let Ok(Some(run)) = self.db.get_run(run_id) {
                self.marked_experiment_ids.insert(run.experiment_id.clone());
            }
        }
    }

    pub fn delete_run(&mut self, run_id: &str) -> Result<()> {
        let db_path = self.store_root.join("extract.db");
        crate::db::Db::delete_run(&db_path, run_id)?;

        // Remove artifact files
        let artifacts_dir = self.store_root.join("artifacts").join(run_id);
        if artifacts_dir.exists() {
            let _ = std::fs::remove_dir_all(&artifacts_dir);
        }

        // Remove from compare selection
        self.selected_runs_for_compare.retain(|id| id != run_id);
        if self.compare_baseline >= self.selected_runs_for_compare.len()
            && !self.selected_runs_for_compare.is_empty()
        {
            self.compare_baseline = 0;
        }
        self.refresh_marked_experiments();

        // Refresh runs list
        let _ = self.refresh_runs();
        if self.runs.is_empty() {
            self.selected_run = None;
        } else if let Some(idx) = self.selected_run {
            if idx >= self.runs.len() {
                self.selected_run = Some(self.runs.len() - 1);
            }
        }

        Ok(())
    }

    pub fn load_run_preview(&mut self, run_idx: usize) -> Result<()> {
        self.summary_scroll = 0;
        let Some(run) = self.runs.get(run_idx) else {
            return Ok(());
        };
        let run_id = run.id.clone();

        self.load_all_metric_histories(&run_id)?;
        self.run_params = self.db.list_run_params(&run_id)?;
        self.artifacts = self.db.list_artifacts(&run_id)?;
        self.load_first_table()?;
        Ok(())
    }

    /// Build the path of experiment IDs from root to the given experiment.
    pub fn experiment_id_path(&self, experiment_id: &str) -> Vec<String> {
        let mut path = Vec::new();
        let mut current_id = Some(experiment_id.to_string());
        while let Some(id) = current_id {
            path.push(id.clone());
            current_id = self.experiments.iter()
                .find(|e| e.id == id)
                .and_then(|e| e.parent_id.clone());
        }
        path.reverse();
        path
    }

    pub fn load_registry_data(&mut self) -> Result<()> {
        self.models = self.db.list_models()?;
        self.registry_cursor = 0;
        Ok(())
    }

    pub fn load_lineage_data(&mut self) -> Result<()> {
        self.lineage_edges = self.db.list_all_lineage()?;
        self.build_lineage_graph();
        self.lineage_cursor = 0;
        Ok(())
    }

    pub fn load_todo_data(&mut self) -> Result<()> {
        let (scope_type, scope_id): (Option<&str>, Option<&str>) = match self.todo_filter {
            TodoFilter::All => (None, None),
            TodoFilter::Global => (Some("global"), None),
            TodoFilter::Experiment => (Some("experiment"), None),
            TodoFilter::Run => (Some("run"), None),
        };
        self.todos = self.db.list_todos(scope_type, scope_id)?;
        if !self.todos.is_empty() && self.todo_cursor >= self.todos.len() {
            self.todo_cursor = self.todos.len() - 1;
        }
        Ok(())
    }

    fn build_lineage_graph(&mut self) {
        use std::collections::{HashMap, HashSet, VecDeque};

        let mut node_set: Vec<(String, String)> = Vec::new();
        let mut seen: HashSet<(String, String)> = HashSet::new();
        for edge in &self.lineage_edges {
            let parent = (edge.parent_type.clone(), edge.parent_id.clone());
            let child = (edge.child_type.clone(), edge.child_id.clone());
            if seen.insert(parent.clone()) {
                node_set.push(parent);
            }
            if seen.insert(child.clone()) {
                node_set.push(child);
            }
        }

        if node_set.is_empty() {
            self.lineage_nodes.clear();
            return;
        }

        let node_idx: HashMap<(String, String), usize> = node_set
            .iter()
            .enumerate()
            .map(|(i, n)| (n.clone(), i))
            .collect();
        let n = node_set.len();
        let mut children_of: Vec<Vec<usize>> = vec![vec![]; n];
        let mut in_degree: Vec<usize> = vec![0; n];

        for edge in &self.lineage_edges {
            let pi = node_idx[&(edge.parent_type.clone(), edge.parent_id.clone())];
            let ci = node_idx[&(edge.child_type.clone(), edge.child_id.clone())];
            children_of[pi].push(ci);
            in_degree[ci] += 1;
        }

        let mut queue: VecDeque<usize> = VecDeque::new();
        let mut layer: Vec<usize> = vec![0; n];
        for i in 0..n {
            if in_degree[i] == 0 {
                queue.push_back(i);
            }
        }
        while let Some(u) = queue.pop_front() {
            for &v in &children_of[u] {
                layer[v] = layer[v].max(layer[u] + 1);
                in_degree[v] -= 1;
                if in_degree[v] == 0 {
                    queue.push_back(v);
                }
            }
        }

        let max_layer = *layer.iter().max().unwrap_or(&0);
        let mut layer_counts: Vec<usize> = vec![0; max_layer + 1];

        let mut nodes: Vec<LineageNode> = Vec::new();
        for (i, (etype, eid)) in node_set.iter().enumerate() {
            let label = match etype.as_str() {
                "model" => {
                    self.db.get_model(eid).ok().flatten()
                        .map(|m| format!("{} v{}", m.name, m.version))
                        .unwrap_or_else(|| format!("model:{}", &eid[eid.len().saturating_sub(8)..]))
                }
                "run" => {
                    self.db.get_run(eid).ok().flatten()
                        .and_then(|r| r.name.or_else(|| {
                            self.db.get_experiment(&r.experiment_id).ok().flatten()
                                .map(|e| e.name)
                        }))
                        .unwrap_or_else(|| format!("run:{}", &eid[eid.len().saturating_sub(8)..]))
                }
                "experiment" => {
                    self.db.get_experiment(eid).ok().flatten()
                        .map(|e| e.name)
                        .unwrap_or_else(|| format!("exp:{}", &eid[eid.len().saturating_sub(8)..]))
                }
                _ => format!("{}:{}", etype, &eid[eid.len().saturating_sub(8)..]),
            };

            let l = layer[i];
            let x_pos = layer_counts[l] as f64;
            layer_counts[l] += 1;

            nodes.push(LineageNode {
                entity_type: etype.clone(),
                entity_id: eid.clone(),
                label,
                layer: l,
                x: x_pos,
            });
        }

        for l in 0..=max_layer {
            let count = layer_counts[l];
            if count == 0 { continue; }
            let offset = -(count as f64 - 1.0) / 2.0;
            for node in &mut nodes {
                if node.layer == l {
                    node.x += offset;
                }
            }
        }

        self.lineage_nodes = nodes;
    }
}
