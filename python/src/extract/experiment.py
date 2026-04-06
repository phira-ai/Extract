"""Experiment: a hierarchical namespace node that contains runs."""

from __future__ import annotations

import json
import socket
import subprocess
from typing import TYPE_CHECKING

from ulid import ULID

if TYPE_CHECKING:
    from extract.store import Store

from extract.run import Run


def _git_sha() -> str | None:
    """Try to get the current git HEAD sha, return None on failure."""
    try:
        result = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            capture_output=True,
            text=True,
            timeout=5,
        )
        if result.returncode == 0:
            return result.stdout.strip()
    except (FileNotFoundError, subprocess.TimeoutExpired):
        pass
    return None


class Experiment:
    """Represents an experiment (a node in the hierarchical namespace)."""

    def __init__(self, store: Store, id: str, path: str, name: str) -> None:
        self._store = store
        self._id = id
        self._path = path
        self._name = name

    @property
    def id(self) -> str:
        return self._id

    @property
    def path(self) -> str:
        return self._path

    @property
    def name(self) -> str:
        return self._name

    def run(self, config: dict | None = None, name: str | None = None) -> Run:
        """Create a new run for this experiment and return it as a context manager."""
        run_id = str(ULID())
        hostname = socket.gethostname()
        git_sha = _git_sha()
        config_json = json.dumps(config) if config is not None else None

        with self._store.lock:
            # Auto-suffix duplicate names within this experiment.
            if name is not None:
                row = self._store._conn.execute(
                    "SELECT COUNT(*) FROM runs WHERE experiment_id = ? AND name = ?",
                    (self._id, name),
                ).fetchone()
                if row[0] > 0:
                    # Find the next available suffix.
                    row2 = self._store._conn.execute(
                        "SELECT COUNT(*) FROM runs WHERE experiment_id = ? AND name LIKE ?",
                        (self._id, f"{name}_%"),
                    ).fetchone()
                    name = f"{name}_{row[0] + row2[0]}"

            self._store._conn.execute(
                "INSERT INTO runs (id, experiment_id, name, config, status, "
                "hostname, git_sha, tags) VALUES (?, ?, ?, ?, 'running', ?, ?, '[]')",
                (run_id, self._id, name, config_json, hostname, git_sha),
            )
            self._store._conn.commit()

        return Run(
            store=self._store,
            experiment_id=self._id,
            run_id=run_id,
        )

    def list_runs(self) -> list[dict]:
        """List all runs for this experiment."""
        with self._store.lock:
            rows = self._store._conn.execute(
                "SELECT * FROM runs WHERE experiment_id = ? ORDER BY started_at",
                (self._id,),
            ).fetchall()
        return [dict(r) for r in rows]

    def __repr__(self) -> str:
        return f"Experiment(path={self._path!r}, name={self._name!r})"
