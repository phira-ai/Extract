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
    status      TEXT NOT NULL DEFAULT 'created',
    node_type   TEXT
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

CREATE TABLE IF NOT EXISTS hierarchy (
    level_order INTEGER NOT NULL,
    level_name  TEXT NOT NULL UNIQUE,
    PRIMARY KEY (level_order)
);
"""


def _parse_hierarchy(hierarchy_str: str) -> list[str]:
    """Parse 'benchmark > method > variant' into ['benchmark', 'method', 'variant']."""
    levels = [level.strip() for level in hierarchy_str.split(">")]
    if any(not level for level in levels):
        raise ValueError(f"Invalid hierarchy: empty level name in {hierarchy_str!r}")
    return levels


class Store:
    """Manages the .extract/ directory, SQLite database, and provides the
    top-level API for creating experiments and global TODOs."""

    def __init__(self, root: str | Path = ".extract", hierarchy: str | None = None) -> None:
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
            # Migrate existing DBs that lack node_type column
            try:
                self._conn.execute("ALTER TABLE experiments ADD COLUMN node_type TEXT")
            except sqlite3.OperationalError:
                pass  # Column already exists
            self._conn.commit()

        # Load or save hierarchy config
        existing = self._load_hierarchy()
        if hierarchy is not None:
            levels = _parse_hierarchy(hierarchy)
            if existing and existing != levels:
                raise ValueError(
                    f"Store already has hierarchy {' > '.join(existing)}, "
                    f"cannot change to {' > '.join(levels)}"
                )
            if not existing:
                self._save_hierarchy(levels)
                existing = levels
        self._hierarchy = existing

    def _load_hierarchy(self) -> list[str]:
        """Load hierarchy level names from DB, ordered."""
        with self.lock:
            rows = self._conn.execute(
                "SELECT level_name FROM hierarchy ORDER BY level_order"
            ).fetchall()
        return [r["level_name"] for r in rows]

    def _save_hierarchy(self, levels: list[str]) -> None:
        """Persist hierarchy levels to DB."""
        with self.lock:
            for i, name in enumerate(levels):
                self._conn.execute(
                    "INSERT OR REPLACE INTO hierarchy (level_order, level_name) "
                    "VALUES (?, ?)",
                    (i, name),
                )
            self._conn.commit()

    # ------------------------------------------------------------------
    # Experiments
    # ------------------------------------------------------------------

    def experiment(self, spec: dict[str, str] | str) -> Experiment:
        """Create or get an experiment.

        Args:
            spec: Either a dict mapping hierarchy levels to values
                  (e.g. {"benchmark": "cifar100", "method": "ewc"})
                  or a plain path string (legacy mode, no node_type).
        """
        if isinstance(spec, str):
            return self._experiment_by_path(spec)
        return self._experiment_by_dict(spec)

    def _experiment_by_path(self, path: str) -> Experiment:
        """Legacy: create experiment from a plain slash-delimited path."""
        parts = path.strip("/").split("/")
        parent_id: str | None = None
        exp_id = exp_path = exp_name = ""

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

    def _experiment_by_dict(self, spec: dict[str, str]) -> Experiment:
        """Create experiment from a hierarchy-keyed dict."""
        if not self._hierarchy:
            raise ValueError(
                "Cannot use dict spec without hierarchy. "
                "Initialize Store with hierarchy='level1 > level2 > ...'"
            )

        unknown = set(spec.keys()) - set(self._hierarchy)
        if unknown:
            raise ValueError(f"Unknown hierarchy levels: {unknown}")

        # Build path parts in hierarchy order, only including levels present in spec
        parts: list[tuple[str, str]] = []  # (value, level_name)
        for level_name in self._hierarchy:
            if level_name in spec:
                parts.append((spec[level_name], level_name))

        if not parts:
            raise ValueError("Spec must include at least one hierarchy level")

        parent_id: str | None = None
        exp_id = exp_path = exp_name = ""

        with self.lock:
            for i, (value, level_name) in enumerate(parts):
                partial_path = "/".join(p[0] for p in parts[: i + 1])

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
                        "INSERT INTO experiments (id, path, name, parent_id, node_type) "
                        "VALUES (?, ?, ?, ?, ?)",
                        (exp_id, partial_path, value, parent_id, level_name),
                    )
                    parent_id = exp_id
                    exp_path = partial_path
                    exp_name = value

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
