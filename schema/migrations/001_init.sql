PRAGMA journal_mode=WAL;
PRAGMA foreign_keys=ON;

-- experiments: hierarchical namespace for grouping runs
CREATE TABLE IF NOT EXISTS experiments (
    id          TEXT PRIMARY KEY,
    path        TEXT NOT NULL,
    name        TEXT NOT NULL,
    parent_id   TEXT REFERENCES experiments(id),
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    metadata    TEXT,
    status      TEXT NOT NULL DEFAULT 'created',
    node_type   TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_experiments_path      ON experiments(path);
CREATE INDEX IF NOT EXISTS idx_experiments_parent_id ON experiments(parent_id);

-- runs: a single execution within an experiment
CREATE TABLE IF NOT EXISTS runs (
    id            TEXT PRIMARY KEY,
    experiment_id TEXT NOT NULL REFERENCES experiments(id),
    name          TEXT,
    config        TEXT,
    started_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    ended_at      TEXT,
    status        TEXT NOT NULL DEFAULT 'running',
    hostname      TEXT,
    git_sha       TEXT,
    tags          TEXT,
    notes         TEXT,
    total_steps   INTEGER
);

CREATE INDEX IF NOT EXISTS idx_runs_experiment_id ON runs(experiment_id);

-- scalar_metrics: time-series numeric values logged during a run
CREATE TABLE IF NOT EXISTS scalar_metrics (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id    TEXT    NOT NULL REFERENCES runs(id),
    step      INTEGER NOT NULL,
    name      TEXT    NOT NULL,
    value     REAL    NOT NULL,
    wall_time REAL,
    UNIQUE(run_id, name, step)
);

CREATE INDEX IF NOT EXISTS idx_scalar_metrics_run_name ON scalar_metrics(run_id, name);

-- curve_points: streaming curve data for live chart updates
CREATE TABLE IF NOT EXISTS curve_points (
    run_id    TEXT    NOT NULL REFERENCES runs(id),
    name      TEXT    NOT NULL,
    step      INTEGER NOT NULL,
    value     REAL    NOT NULL,
    wall_time REAL,
    UNIQUE(run_id, name, step)
);

CREATE INDEX IF NOT EXISTS idx_curve_points_run_name_step
    ON curve_points(run_id, name, step);

-- artifacts: files or blobs associated with a run
CREATE TABLE IF NOT EXISTS artifacts (
    id         TEXT PRIMARY KEY,
    run_id     TEXT NOT NULL REFERENCES runs(id),
    name       TEXT NOT NULL,
    kind       TEXT NOT NULL,
    step       INTEGER,
    rel_path   TEXT NOT NULL,
    shape      TEXT,
    dtype      TEXT,
    metadata   TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

CREATE INDEX IF NOT EXISTS idx_artifacts_run_id ON artifacts(run_id);

-- models: versioned model registry entries
CREATE TABLE IF NOT EXISTS models (
    id            TEXT PRIMARY KEY,
    name          TEXT NOT NULL,
    version       TEXT NOT NULL,
    run_id        TEXT REFERENCES runs(id),
    artifact_path TEXT NOT NULL,
    framework     TEXT,
    metadata      TEXT,
    created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    UNIQUE(name, version)
);

-- lineage: directed edges between experiments, runs, and models
CREATE TABLE IF NOT EXISTS lineage (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    parent_type TEXT NOT NULL,
    parent_id   TEXT NOT NULL,
    child_type  TEXT NOT NULL,
    child_id    TEXT NOT NULL,
    relation    TEXT NOT NULL,
    metadata    TEXT,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    UNIQUE(parent_type, parent_id, child_type, child_id, relation)
);

CREATE INDEX IF NOT EXISTS idx_lineage_child  ON lineage(child_type, child_id);
CREATE INDEX IF NOT EXISTS idx_lineage_parent ON lineage(parent_type, parent_id);

-- hierarchy: user-defined level ordering for typed experiment nodes
CREATE TABLE IF NOT EXISTS hierarchy (
    level_order INTEGER NOT NULL,
    level_name  TEXT NOT NULL UNIQUE,
    PRIMARY KEY (level_order)
);

-- run_params: categorical/string key-value attributes for a run
CREATE TABLE IF NOT EXISTS run_params (
    id     INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT NOT NULL REFERENCES runs(id),
    name   TEXT NOT NULL,
    value  TEXT NOT NULL,
    UNIQUE(run_id, name)
);

CREATE INDEX IF NOT EXISTS idx_run_params_run_id ON run_params(run_id);

-- todos: task notes scoped to global, an experiment, or a run
CREATE TABLE IF NOT EXISTS todos (
    id           TEXT PRIMARY KEY,
    scope_type   TEXT    NOT NULL,
    scope_id     TEXT,
    content      TEXT    NOT NULL,
    done         INTEGER NOT NULL DEFAULT 0,
    priority     INTEGER NOT NULL DEFAULT 0,
    created_at   TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    completed_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_todos_scope ON todos(scope_type, scope_id);
