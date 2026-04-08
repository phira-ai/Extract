use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Experiment {
    pub id: String,
    pub path: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub created_at: String,
    pub metadata: Option<String>,
    pub status: String,
    pub node_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub experiment_id: String,
    pub name: Option<String>,
    pub config: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub status: String,
    pub hostname: Option<String>,
    pub git_sha: Option<String>,
    pub tags: Option<String>,
    pub notes: Option<String>,
    pub total_steps: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScalarMetric {
    pub id: i64,
    pub run_id: String,
    pub step: i64,
    pub name: String,
    pub value: f64,
    pub wall_time: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    pub id: String,
    pub run_id: String,
    pub name: String,
    pub kind: String,
    pub step: Option<i64>,
    pub rel_path: String,
    pub shape: Option<String>,
    pub dtype: Option<String>,
    pub metadata: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub version: String,
    pub run_id: Option<String>,
    pub artifact_path: String,
    pub framework: Option<String>,
    pub metadata: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineageEdge {
    pub id: i64,
    pub parent_type: String,
    pub parent_id: String,
    pub child_type: String,
    pub child_id: String,
    pub relation: String,
    pub metadata: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Todo {
    pub id: String,
    pub scope_type: String,
    pub scope_id: Option<String>,
    pub content: String,
    pub done: bool,
    pub priority: i64,
    pub created_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricAggregate {
    pub name: String,
    pub mean: f64,
    pub std_dev: f64,
    pub min: f64,
    pub max: f64,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunParam {
    pub run_id: String,
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone)]
pub struct MetricRanking {
    pub metric_name: String,
    pub lower_is_better: bool,
    pub entries: Vec<(String, f64)>, // (child_name, best_value) sorted best-first
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub result_type: String, // "experiment" or "run"
    pub id: String,
    pub experiment_id: Option<String>,
    pub label: String,
    pub matched_field: String,
}

/// Returns true if lower values are better for this metric.
/// Checks config overrides first, then falls back to a name-based heuristic.
pub fn is_lower_better(metric_name: &str, config: &crate::config::MetricsConfig) -> bool {
    // Config overrides take precedence
    if config.minimize.iter().any(|m| m == metric_name) {
        return true;
    }
    if config.maximize.iter().any(|m| m == metric_name) {
        return false;
    }

    // Heuristic fallback
    let name = metric_name.to_lowercase();
    name.contains("loss")
        || name.contains("error")
        || name.contains("perplexity")
        || name.contains("mse")
        || name.contains("mae")
        || name.contains("rmse")
        || name.contains("nll")
        || name.contains("cer")
        || name.contains("wer")
        || name.contains("fid")
        || name.contains("divergence")
}
