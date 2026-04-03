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
