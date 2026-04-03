"""Store: manages the .extract/ directory, SQLite database, and migrations."""

from __future__ import annotations

import json
import sqlite3
import threading
from pathlib import Path

from ulid import ULID

from extract.experiment import Experiment

_SCHEMA = """\
PRAGMA journal_mode=WAL;
PRAGMA foreign_keys=ON;

CREATE TABLE IF NOT EXISTS experiments (
    id          TEXT PRIMARY KEY,
    path        TEXT NOT NULL,
    name        TEXT NOT NULL,
    parent_id   TEXT REFERENCES experiments(id),
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    metadata    TEXT,
    status      TEXT NOT NULL DEFAULT 'created'
);

CREATE INDEX IF NOT EXISTS idx_experiments_path      ON experiments(path);
CREATE INDEX IF NOT EXISTS idx_experiments_parent_id ON experiments(parent_id);

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
    notes         TEXT
);

CREATE INDEX IF NOT EXISTS idx_runs_experiment_id ON runs(experiment_id);

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
"""


class Store:
    """Manages the .extract/ directory, SQLite database, and provides the
    top-level API for creating experiments and global TODOs."""

    def __init__(self, root: str | Path = ".extract") -> None:
        self.root = Path(root)
        self.root.mkdir(parents=True, exist_ok=True)
        (self.root / "artifacts").mkdir(exist_ok=True)
        (self.root / "models").mkdir(exist_ok=True)

        self.lock = threading.Lock()

        db_path = self.root / "extract.db"
        self._conn = sqlite3.connect(str(db_path), check_same_thread=False)
        self._conn.row_factory = sqlite3.Row

        # Run migrations (embedded schema uses IF NOT EXISTS, safe to re-run)
        with self.lock:
            self._conn.executescript(_SCHEMA)

    # ------------------------------------------------------------------
    # Experiments
    # ------------------------------------------------------------------

    def experiment(self, path: str) -> Experiment:
        """Create or get an experiment by path, auto-creating hierarchy nodes.

        For path "a/b/c", creates experiments with paths "a", "a/b", "a/b/c"
        with proper parent_id linkage (like mkdir -p).
        """
        parts = path.strip("/").split("/")
        parent_id: str | None = None

        with self.lock:
            for i in range(len(parts)):
                partial_path = "/".join(parts[: i + 1])
                name = parts[i]

                row = self._conn.execute(
                    "SELECT id, path, name FROM experiments WHERE path = ?",
                    (partial_path,),
                ).fetchone()

                if row is not None:
                    parent_id = row["id"]
                    exp_id, exp_path, exp_name = row["id"], row["path"], row["name"]
                else:
                    exp_id = str(ULID())
                    self._conn.execute(
                        "INSERT INTO experiments (id, path, name, parent_id) "
                        "VALUES (?, ?, ?, ?)",
                        (exp_id, partial_path, name, parent_id),
                    )
                    parent_id = exp_id
                    exp_path = partial_path
                    exp_name = name

            self._conn.commit()

        return Experiment(store=self, id=exp_id, path=exp_path, name=exp_name)

    def list_experiments(self, prefix: str = "") -> list[Experiment]:
        """List experiments, optionally filtered by path prefix."""
        with self.lock:
            if prefix:
                rows = self._conn.execute(
                    "SELECT id, path, name FROM experiments "
                    "WHERE path = ? OR path LIKE ?",
                    (prefix, prefix.rstrip("/") + "/%"),
                ).fetchall()
            else:
                rows = self._conn.execute(
                    "SELECT id, path, name FROM experiments"
                ).fetchall()

        return [
            Experiment(store=self, id=r["id"], path=r["path"], name=r["name"])
            for r in rows
        ]

    # ------------------------------------------------------------------
    # TODOs
    # ------------------------------------------------------------------

    def todo(self, content: str, priority: int = 0) -> None:
        """Create a global TODO."""
        todo_id = str(ULID())
        with self.lock:
            self._conn.execute(
                "INSERT INTO todos (id, scope_type, content, priority) "
                "VALUES (?, 'global', ?, ?)",
                (todo_id, content, priority),
            )
            self._conn.commit()

    def list_todos(
        self, scope_type: str = "global", scope_id: str | None = None
    ) -> list[dict]:
        """List TODOs, filtered by scope."""
        with self.lock:
            if scope_id is not None:
                rows = self._conn.execute(
                    "SELECT * FROM todos WHERE scope_type = ? AND scope_id = ? "
                    "ORDER BY priority DESC, created_at",
                    (scope_type, scope_id),
                ).fetchall()
            else:
                rows = self._conn.execute(
                    "SELECT * FROM todos WHERE scope_type = ? "
                    "ORDER BY priority DESC, created_at",
                    (scope_type,),
                ).fetchall()

        return [dict(r) for r in rows]

    # ------------------------------------------------------------------
    # Lifecycle
    # ------------------------------------------------------------------

    def close(self) -> None:
        """Close the database connection."""
        with self.lock:
            self._conn.close()
