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

/// Recursively collect dotted key paths from a JSON object (e.g. "optimizer.lr").
fn collect_dotted_keys(value: &JsonValue, prefix: &str, keys: &mut Vec<String>) {
    if let Some(obj) = value.as_object() {
        for (k, v) in obj {
            let full_key = if prefix.is_empty() {
                k.clone()
            } else {
                format!("{}.{}", prefix, k)
            };
            if v.is_object() {
                collect_dotted_keys(v, &full_key, keys);
            } else if !keys.contains(&full_key) {
                keys.push(full_key);
            }
        }
    }
}

/// Resolve a dotted key path (e.g. "optimizer.lr") through nested JSON objects.
pub fn resolve_dotted_key<'a>(value: &'a JsonValue, key: &str) -> Option<&'a JsonValue> {
    let mut current = value;
    for part in key.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

/// Disambiguate run labels by prepending minimal distinguishing path segments.
/// `paths` contains the experiment path segments for each run, `names` contains the base labels.
pub fn disambiguate_labels(paths: &[Vec<String>], names: &[String]) -> Vec<String> {
    let n = names.len();
    let mut result = names.to_vec();

    // Find duplicate labels and disambiguate by prepending path segments
    for i in 0..n {
        let mut depth = 0;
        loop {
            let has_dup = (0..n).any(|j| j != i && result[j] == result[i]);
            if !has_dup {
                break;
            }
            depth += 1;
            let path = &paths[i];
            if depth > path.len() {
                break;
            }
            // Prepend segments from the end of the path
            let prefix = path[path.len().saturating_sub(depth)..].join("/");
            result[i] = format!("{}/{}", prefix, names[i]);
        }
    }

    result
}

impl CompareRunData {
    pub fn label(&self) -> String {
        if let Some(ref name) = self.run.name {
            return format!("{}/{}", self.experiment_name, name);
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
    pub curve_names: Vec<String>,
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
    pub search_query: Option<String>,
    pub filtered: Vec<usize>,
    pub scroll_offset: usize,
}

impl RunPickerState {
    pub fn new(experiment_name: String, runs: Vec<Run>, selected: Vec<String>) -> Self {
        let filtered = (0..runs.len()).collect();
        Self {
            experiment_name,
            runs,
            selected,
            cursor: 0,
            search_query: None,
            filtered,
            scroll_offset: 0,
        }
    }

    /// Re-filter runs based on current search query.
    pub fn apply_filter(&mut self) {
        let Some(ref query) = self.search_query else {
            self.filtered = (0..self.runs.len()).collect();
            self.cursor = 0;
            self.scroll_offset = 0;
            return;
        };
        if query.is_empty() {
            self.filtered = (0..self.runs.len()).collect();
            self.cursor = 0;
            self.scroll_offset = 0;
            return;
        }
        let q = query.to_lowercase();
        self.filtered = self.runs.iter().enumerate()
            .filter(|(_, run)| {
                let name = run.name.as_deref().unwrap_or("").to_lowercase();
                let tags = run.tags.as_deref().unwrap_or("").to_lowercase();
                let notes = run.notes.as_deref().unwrap_or("").to_lowercase();
                let status = run.status.to_lowercase();
                let config = run.config.as_deref().unwrap_or("").to_lowercase();
                name.contains(&q) || tags.contains(&q) || notes.contains(&q)
                    || status.contains(&q) || config.contains(&q)
            })
            .map(|(i, _)| i)
            .collect();
        self.cursor = 0;
        self.scroll_offset = 0;
    }
}

/// State for the run browser popup (r key).
pub struct RunBrowserState {
    pub experiment_name: String,
    pub runs: Vec<Run>,
    pub filtered: Vec<usize>,
    pub cursor: usize,
    pub search_query: Option<String>,
    pub scroll_offset: usize,
}

impl RunBrowserState {
    pub fn new(experiment_name: String, _experiment_id: String, runs: Vec<Run>) -> Self {
        let filtered = (0..runs.len()).collect();
        Self {
            experiment_name,
            runs,
            filtered,
            cursor: 0,
            search_query: None,
            scroll_offset: 0,
        }
    }

    /// Re-filter runs based on current search query.
    pub fn apply_filter(&mut self) {
        let Some(ref query) = self.search_query else {
            self.filtered = (0..self.runs.len()).collect();
            self.cursor = 0;
            self.scroll_offset = 0;
            return;
        };
        if query.is_empty() {
            self.filtered = (0..self.runs.len()).collect();
            self.cursor = 0;
            self.scroll_offset = 0;
            return;
        }
        let q = query.to_lowercase();
        self.filtered = self.runs.iter().enumerate()
            .filter(|(_, run)| {
                let name = run.name.as_deref().unwrap_or("").to_lowercase();
                let tags = run.tags.as_deref().unwrap_or("").to_lowercase();
                let notes = run.notes.as_deref().unwrap_or("").to_lowercase();
                let status = run.status.to_lowercase();
                let config = run.config.as_deref().unwrap_or("").to_lowercase();
                name.contains(&q) || tags.contains(&q) || notes.contains(&q)
                    || status.contains(&q) || config.contains(&q)
            })
            .map(|(i, _)| i)
            .collect();
        self.cursor = 0;
        self.scroll_offset = 0;
    }
}

/// What kind of entity is being deleted.
#[derive(Debug, Clone)]
pub enum DeleteTarget {
    Run { run_id: String },
    Experiment { experiment_id: String },
}

/// State for the delete confirmation popup.
pub struct DeleteConfirmState {
    pub target: DeleteTarget,
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
    pub info_scroll: u16,
    pub info_total_lines: usize,
    pub info_visible_height: usize,
    pub cached_table: Option<TableData>,
    pub cached_table_artifact_id: Option<String>,
    pub cached_table_axes: Option<(String, String)>,
    pub cached_table_title: Option<String>,
    pub compare_data: Option<CompareData>,
    pub compare_baseline: usize,
    pub marked_experiment_ids: std::collections::HashSet<String>,
    pub selection_cursor: usize,
    pub run_picker: Option<RunPickerState>,
    pub run_browser: Option<RunBrowserState>,
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
    /// SQLite data_version watermark — used to skip tick refresh work when
    /// the database hasn't changed since the last tick.
    pub last_data_version: i64,
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
            info_scroll: 0,
            info_total_lines: 0,
            info_visible_height: 0,
            cached_table: None,
            cached_table_artifact_id: None,
            cached_table_axes: None,
            cached_table_title: None,
            compare_data: None,
            compare_baseline: 0,
            marked_experiment_ids: std::collections::HashSet::new(),
            selection_cursor: 0,
            run_picker: None,
            run_browser: None,
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
            last_data_version: 0,
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

    /// The run whose data populates `metric_histories` in the leaf-preview path:
    /// the latest completed run, or the first run if none have completed.
    /// Used by the leaf-preview loader and by chart-axis-pinning code that
    /// needs to match the loaded run exactly.
    pub fn leaf_preview_run(&self) -> Option<&Run> {
        self.runs
            .iter()
            .rev()
            .find(|r| r.status == "completed")
            .or(self.runs.first())
    }

    /// Load curve data for a given run.
    /// Reads from the curve_points table (populated by run.curve() in the SDK).
    /// Headline metrics from run.log() live in scalar_metrics and are surfaced
    /// elsewhere — they don't appear in metric_histories.
    fn load_all_metric_histories(&mut self, run_id: &str) -> Result<()> {
        self.metric_histories.clear();

        let names = self.db.list_curve_names(run_id)?;
        for name in names {
            let points = self.db.list_curve_points(run_id, &name)?;
            if points.is_empty() {
                continue;
            }
            let history: Vec<ScalarMetric> = points
                .into_iter()
                .map(|(step, value, wall_time)| ScalarMetric {
                    id: 0,
                    run_id: run_id.to_string(),
                    step,
                    name: name.clone(),
                    value,
                    wall_time,
                })
                .collect();
            self.metric_histories.push((name, history));
        }

        Ok(())
    }

    /// Hard reload — used when the user navigates to a new leaf experiment.
    /// Resets scroll positions to the top.
    pub fn refresh_leaf_preview(&mut self) -> Result<()> {
        self.summary_scroll = 0;
        self.info_scroll = 0;
        self.reload_leaf_preview_data()
    }

    /// Soft reload — used on data_version tick. Preserves scroll positions
    /// so the leaf summary view doesn't jump to the top while training
    /// writes new data.
    pub fn reload_leaf_preview_data(&mut self) -> Result<()> {
        if self.runs.is_empty() {
            self.metric_histories.clear();
            self.run_params.clear();
            self.artifacts.clear();
            self.cached_table = None;
            self.cached_table_artifact_id = None;
            return Ok(());
        }

        let Some(run) = self.leaf_preview_run() else {
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

    /// Hard reload — used when the user enters compare view via `c`.
    /// Resets scroll position to the top.
    pub fn load_compare_data(&mut self) -> Result<()> {
        self.reload_compare_data()?;
        if let Some(ref mut data) = self.compare_data {
            data.scroll = 0;
        }
        Ok(())
    }

    /// Soft reload — used on data_version tick. Preserves the user's scroll
    /// position in the compare view while curves continue to grow.
    pub fn reload_compare_data(&mut self) -> Result<()> {
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

            // Load artifacts (timeseries + tables)
            let artifacts = self.db.list_artifacts(&run.id)?;

            // Load curve data from curve_points table
            let mut metric_histories: Vec<(String, Vec<ScalarMetric>)> = Vec::new();
            let curve_names = self.db.list_curve_names(&run.id)?;
            for name in curve_names {
                let points = self.db.list_curve_points(&run.id, &name)?;
                if points.is_empty() {
                    continue;
                }
                let history: Vec<ScalarMetric> = points
                    .into_iter()
                    .map(|(step, value, wall_time)| ScalarMetric {
                        id: 0,
                        run_id: run.id.clone(),
                        step,
                        name: name.clone(),
                        value,
                        wall_time,
                    })
                    .collect();
                metric_histories.push((name, history));
            }

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
        let mut curve_names = Vec::new();

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
                collect_dotted_keys(config, "", &mut config_keys);
            }
            for (name, _, _) in &rd.tables {
                if !table_names.contains(name) {
                    table_names.push(name.clone());
                }
            }
            for (name, _) in &rd.metric_histories {
                if !curve_names.contains(name) {
                    curve_names.push(name.clone());
                }
            }
        }

        metric_names.sort();
        param_names.sort();
        config_keys.retain(|k| config::key_passes_filters(k, &self.config.info.fields));
        config_keys.sort();
        table_names.sort();
        curve_names.sort();

        // Preserve the user's scroll position across soft reloads.
        // (The hard wrapper resets scroll back to 0 explicitly.)
        let preserved_scroll = self.compare_data.as_ref().map(|d| d.scroll).unwrap_or(0);
        let preserved_visible_height = self.compare_data.as_ref().map(|d| d.visible_height).unwrap_or(0);

        self.compare_data = Some(CompareData {
            runs: runs_data,
            metric_names,
            param_names,
            config_keys,
            table_names,
            curve_names,
            scroll: preserved_scroll,
            total_lines: 0,
            visible_height: preserved_visible_height,
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
                    let lower = is_lower_better(&metric_name, &self.config.metrics);
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

    /// Single entry point for live refresh, called by the tick loop when
    /// PRAGMA data_version has incremented. Re-runs every visible query
    /// using the SOFT loaders so user scroll positions are preserved.
    pub fn refresh_live(&mut self) -> Result<()> {
        self.refresh_experiments()?;
        if self.selected_experiment.is_some() {
            self.refresh_runs()?;
        }
        self.refresh_selection_summary()?;
        // Detail panel — soft reload (preserves scroll).
        if let Some(idx) = self.selected_run {
            self.reload_run_preview_data(idx)?;
        } else if let Some(exp_idx) = self.selected_experiment {
            // Leaf preview path: only fires if the selected experiment is a leaf.
            let is_leaf = if let Some(exp) = self.experiments.get(exp_idx) {
                let exp_id = exp.id.clone();
                !self.experiments.iter().any(|e| e.parent_id.as_deref() == Some(exp_id.as_str()))
            } else {
                false
            };
            if is_leaf {
                self.reload_leaf_preview_data()?;
            }
        }
        // Compare view — soft reload (preserves scroll), only if active.
        if self.compare_data.is_some() {
            self.reload_compare_data()?;
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

    pub fn delete_experiment(&mut self, experiment_id: &str) -> Result<()> {
        let db_path = self.store_root.join("extract.db");
        let deleted_run_ids = crate::db::Db::delete_experiment(&db_path, experiment_id)?;

        // Remove artifact files for all deleted runs
        for run_id in &deleted_run_ids {
            let artifacts_dir = self.store_root.join("artifacts").join(run_id);
            if artifacts_dir.exists() {
                let _ = std::fs::remove_dir_all(&artifacts_dir);
            }
        }

        // Remove deleted runs from compare selection
        self.selected_runs_for_compare
            .retain(|id| !deleted_run_ids.contains(id));
        if self.compare_baseline >= self.selected_runs_for_compare.len()
            && !self.selected_runs_for_compare.is_empty()
        {
            self.compare_baseline = 0;
        }
        self.refresh_marked_experiments();

        // Refresh experiments and runs
        let _ = self.refresh_experiments();
        self.selected_experiment = None;
        self.selected_run = None;
        self.runs.clear();
        self.metrics.clear();

        Ok(())
    }

    /// Hard reload — used on user navigation (cycle to a new run, etc.).
    /// Resets scroll positions to the top.
    pub fn load_run_preview(&mut self, run_idx: usize) -> Result<()> {
        self.summary_scroll = 0;
        self.info_scroll = 0;
        self.reload_run_preview_data(run_idx)
    }

    /// Soft reload — used on data_version tick. Preserves scroll positions
    /// so the user's view doesn't jump to the top every 500ms while training
    /// writes new curve points.
    pub fn reload_run_preview_data(&mut self, run_idx: usize) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    /// Bootstrap a temp store directory with a minimal schema and seed data.
    /// Returns the tempdir (keep it alive for the test duration) and the
    /// path to the `extract.db` file inside it.
    fn setup_store() -> (tempfile::TempDir, std::path::PathBuf) {
        let tmp = tempfile::TempDir::new().unwrap();
        let db_path = tmp.path().join("extract.db");

        // Schema + seed via a writable connection.
        let writer = Connection::open(&db_path).unwrap();
        writer.execute_batch("PRAGMA journal_mode=WAL").unwrap();
        writer
            .execute_batch(include_str!("../../schema/migrations/001_init.sql"))
            .unwrap();
        writer
            .execute_batch(include_str!("../../schema/migrations/002_experiment_metadata.sql"))
            .unwrap();
        writer
            .execute_batch(
                "INSERT INTO hierarchy VALUES (0, 'benchmark');
                 INSERT INTO experiments VALUES ('e1', 'a', 'a', NULL, '2026-01-01T00:00:00Z', NULL, 'active', 'benchmark', NULL, NULL);
                 INSERT INTO runs VALUES ('r1', 'e1', 'run1', NULL, '2026-01-01T00:00:00Z', NULL, 'running', NULL, NULL, '[]', NULL, 10);
                 INSERT INTO curve_points VALUES ('r1', 'loss', 0, 1.0, 0.0);",
            )
            .unwrap();
        drop(writer);

        (tmp, db_path)
    }

    /// refresh_live is the single entry point for the data_version-gated
    /// tick refresh. This test verifies four properties end-to-end:
    ///   1. After an external write, the new curve data is visible in
    ///      AppState::metric_histories.
    ///   2. The user's scroll position (summary_scroll / info_scroll) is
    ///      preserved across refresh_live — that's the whole point of the
    ///      hard/soft loader split.
    ///   3. PRAGMA data_version ticks when another connection commits, which
    ///      is what the tick loop's gate relies on.
    ///   4. Calling refresh_live twice in a row with no intervening writes is
    ///      a no-op on the data (idempotent).
    #[test]
    fn test_refresh_live_streams_curves_and_preserves_scroll() {
        let (_tmp, db_path) = setup_store();

        // Open the read-only Db as the TUI does.
        let db = Db::open(&db_path).unwrap();
        let store_root = db_path.parent().unwrap().to_path_buf();
        let mut state = AppState::new(db, store_root).unwrap();

        // Navigate to the leaf experiment and the run within it — this is
        // what the user does when pressing Enter on a tree leaf.
        state.selected_experiment = Some(0);
        state.refresh_runs().unwrap();
        assert_eq!(state.runs.len(), 1, "fixture has one seed run");
        state.selected_run = Some(0);
        state.load_run_preview(0).unwrap();

        // Precondition: the seed has a single curve point for "loss".
        let loss_before = state
            .metric_histories
            .iter()
            .find(|(n, _)| n == "loss")
            .expect("seed curve point should be loaded");
        assert_eq!(loss_before.1.len(), 1);

        // User scrolls down. refresh_live must preserve this across ticks.
        state.summary_scroll = 42;
        state.info_scroll = 17;

        let v_before = state.db.data_version().unwrap();

        // Simulate the SDK training loop appending curve points via a
        // second writable connection.
        let writer = Connection::open(&db_path).unwrap();
        writer
            .execute_batch(
                "INSERT INTO curve_points VALUES ('r1', 'loss', 1, 0.8, 0.5);
                 INSERT INTO curve_points VALUES ('r1', 'loss', 2, 0.6, 1.0);",
            )
            .unwrap();
        drop(writer);

        // Property 3: data_version ticks — this is what main.rs gates on.
        let v_after = state.db.data_version().unwrap();
        assert!(
            v_after > v_before,
            "data_version should increment after external write (before={v_before}, after={v_after})"
        );

        // Simulate the tick loop: call refresh_live.
        state.refresh_live().unwrap();

        // Property 1: new curve data is visible.
        let loss_after = state
            .metric_histories
            .iter()
            .find(|(n, _)| n == "loss")
            .expect("loss history should still be loaded");
        assert_eq!(
            loss_after.1.len(),
            3,
            "refresh_live should pull in the two new curve points"
        );
        assert_eq!(loss_after.1[2].step, 2);
        assert!((loss_after.1[2].value - 0.6).abs() < 1e-9);

        // Property 2: scroll positions preserved (the whole point of the
        // hard/soft loader split in refresh_live).
        assert_eq!(
            state.summary_scroll, 42,
            "summary_scroll must survive refresh_live"
        );
        assert_eq!(
            state.info_scroll, 17,
            "info_scroll must survive refresh_live"
        );

        // Property 4: idempotent — calling refresh_live again with no new
        // writes changes nothing observable.
        state.refresh_live().unwrap();
        let loss_again = state
            .metric_histories
            .iter()
            .find(|(n, _)| n == "loss")
            .unwrap();
        assert_eq!(loss_again.1.len(), 3);
        assert_eq!(state.summary_scroll, 42);
    }
}
