"""Run: a single execution within an experiment, used as a context manager."""

from __future__ import annotations

import json
import shutil
import time
import traceback
from pathlib import Path
from typing import TYPE_CHECKING

import numpy as np
from ulid import ULID

from extract.metrics import save_npy, save_text

if TYPE_CHECKING:
    from extract.store import Store

_FLUSH_THRESHOLD = 100              # scalar_metrics (headline)
_CURVE_FLUSH_THRESHOLD = 10         # curve_points (streaming) — smaller for live UX
_CURVE_FLUSH_INTERVAL_SEC = 2.0     # wall-clock fallback for slow training loops


class Run:
    """Represents a single run within an experiment.

    Use as a context manager to automatically flush metrics and record
    the run's final status on exit.
    """

    def __init__(self, store: Store, experiment_id: str, run_id: str) -> None:
        self._store = store
        self._experiment_id = experiment_id
        self._id = run_id
        self._start_time = time.time()
        self._finished = False
        # Headline (scalar_metrics) buffer.
        self._buffer: list[tuple[str, int, str, float, float]] = []  # (run_id, step, name, value, wall_time)
        # Streaming-curve (curve_points) buffer + wall-clock flush bookkeeping.
        # Tuple order matches curve_points column order: (run_id, name, step, value, wall_time)
        self._curve_buffer: list[tuple[str, str, int, float, float]] = []
        self._curve_last_flush: float = time.monotonic()

    @property
    def id(self) -> str:
        return self._id

    # ------------------------------------------------------------------
    # Context manager
    # ------------------------------------------------------------------

    def __enter__(self) -> Run:
        return self

    def __exit__(self, exc_type, exc_val, exc_tb) -> None:
        status = "failed" if exc_type is not None else "completed"
        self.finish(status=status)
        return None

    # ------------------------------------------------------------------
    # Lifecycle
    # ------------------------------------------------------------------

    def finish(self, status: str = "completed") -> None:
        """Flush metrics and finalize the run.

        Idempotent — safe to call multiple times.
        """
        if self._finished:
            return
        self._finished = True
        self._flush()
        self._flush_curves()
        with self._store.lock:
            self._store._conn.execute(
                "UPDATE runs SET ended_at = strftime('%Y-%m-%dT%H:%M:%fZ','now'), "
                "status = ? WHERE id = ?",
                (status, self._id),
            )
            self._store._conn.commit()

    def _check_active(self) -> None:
        if self._finished:
            raise RuntimeError(f"Run {self._id} is already finished")

    # ------------------------------------------------------------------
    # Scalar metrics
    # ------------------------------------------------------------------

    def log(self, step: int, **kwargs: float | int | str) -> None:
        """Log metrics at a given step.

        Numeric values (int, float) are stored as time-series scalar metrics.
        String values are stored as run-level categorical parameters.
        """
        self._check_active()
        wall_time = time.time() - self._start_time
        for name, value in kwargs.items():
            if isinstance(value, str):
                self._log_param(name, value)
            else:
                self._buffer.append((self._id, step, name, float(value), wall_time))

        if len(self._buffer) >= _FLUSH_THRESHOLD:
            self._flush()

    def curve(self, step: int, **kwargs: float | int) -> None:
        """Log streaming-curve points at a given step.

        Unlike `log()`, curve points are stored in a separate table that the
        TUI's chart panel reads but headline-summary queries do not. Use this
        for high-frequency training values (per-step loss, accuracy) that
        should drive a live chart but should NOT clutter the run summary.

        Numeric values only — strings raise TypeError. Buffered and flushed
        in batches of `_CURVE_FLUSH_THRESHOLD` or after `_CURVE_FLUSH_INTERVAL_SEC`
        seconds, whichever comes first.
        """
        self._check_active()
        wall_time = time.time() - self._start_time
        for name, value in kwargs.items():
            if isinstance(value, bool) or not isinstance(value, (int, float)):
                raise TypeError(
                    f"curve() values must be numeric, got {type(value).__name__} for {name!r}"
                )
            self._curve_buffer.append((self._id, name, step, float(value), wall_time))

        # Threshold flush.
        if len(self._curve_buffer) >= _CURVE_FLUSH_THRESHOLD:
            self._flush_curves()
            return
        # Wall-clock flush — keeps slow training loops feeling live in the TUI.
        if time.monotonic() - self._curve_last_flush >= _CURVE_FLUSH_INTERVAL_SEC:
            self._flush_curves()

    def _log_param(self, name: str, value: str) -> None:
        """Store a categorical/string parameter for this run."""
        with self._store.lock:
            self._store._conn.execute(
                "INSERT OR REPLACE INTO run_params (run_id, name, value) "
                "VALUES (?, ?, ?)",
                (self._id, name, value),
            )
            self._store._conn.commit()

    def _flush(self) -> None:
        """Flush the scalar metrics buffer to the database."""
        if not self._buffer:
            return

        with self._store.lock:
            self._store._conn.executemany(
                "INSERT OR REPLACE INTO scalar_metrics "
                "(run_id, step, name, value, wall_time) VALUES (?, ?, ?, ?, ?)",
                self._buffer,
            )
            self._store._conn.commit()

        self._buffer.clear()

    def _flush_curves(self) -> None:
        """Flush the streaming-curve buffer to the database."""
        if not self._curve_buffer:
            return

        with self._store.lock:
            self._store._conn.executemany(
                "INSERT OR REPLACE INTO curve_points "
                "(run_id, name, step, value, wall_time) VALUES (?, ?, ?, ?, ?)",
                self._curve_buffer,
            )
            self._store._conn.commit()

        self._curve_buffer.clear()
        self._curve_last_flush = time.monotonic()

    # ------------------------------------------------------------------
    # Artifact helpers
    # ------------------------------------------------------------------

    def _artifact_dir(self, kind: str) -> Path:
        return self._store.root / "artifacts" / self._id / kind

    def log_table(
        self,
        name: str,
        data: np.ndarray,
        step: int | None = None,
        axes: dict | None = None,
    ) -> None:
        """Save a matrix as a .npy artifact."""
        self._check_active()
        suffix = f"_step_{step}" if step is not None else ""
        filename = f"{name}{suffix}.npy"
        rel_dir = Path("artifacts") / self._id / "matrices"
        rel_path = rel_dir / filename
        abs_path = self._store.root / rel_path

        save_npy(data, abs_path)

        metadata = {}
        if axes is not None:
            metadata["axes"] = axes

        artifact_id = str(ULID())
        with self._store.lock:
            self._store._conn.execute(
                "INSERT INTO artifacts "
                "(id, run_id, name, kind, step, rel_path, shape, dtype, metadata) "
                "VALUES (?, ?, ?, 'matrix', ?, ?, ?, ?, ?)",
                (
                    artifact_id,
                    self._id,
                    name,
                    step,
                    str(rel_path),
                    json.dumps(list(data.shape)),
                    str(data.dtype),
                    json.dumps(metadata) if metadata else None,
                ),
            )
            self._store._conn.commit()

    def log_text(self, name: str, content: str) -> None:
        """Save text content as a markdown artifact."""
        self._check_active()
        rel_dir = Path("artifacts") / self._id / "text"
        rel_path = rel_dir / f"{name}.md"
        abs_path = self._store.root / rel_path

        save_text(content, abs_path)

        artifact_id = str(ULID())
        with self._store.lock:
            self._store._conn.execute(
                "INSERT INTO artifacts "
                "(id, run_id, name, kind, rel_path) VALUES (?, ?, ?, 'text', ?)",
                (artifact_id, self._id, name, str(rel_path)),
            )
            self._store._conn.commit()

    # ------------------------------------------------------------------
    # Tags and notes
    # ------------------------------------------------------------------

    def tag(self, *tags: str) -> None:
        """Append tags to this run."""
        self._check_active()
        with self._store.lock:
            row = self._store._conn.execute(
                "SELECT tags FROM runs WHERE id = ?", (self._id,)
            ).fetchone()
            existing = json.loads(row["tags"]) if row["tags"] else []
            existing.extend(tags)
            self._store._conn.execute(
                "UPDATE runs SET tags = ? WHERE id = ?",
                (json.dumps(existing), self._id),
            )
            self._store._conn.commit()

    def note(self, content: str) -> None:
        """Set or append to this run's notes."""
        self._check_active()
        with self._store.lock:
            row = self._store._conn.execute(
                "SELECT notes FROM runs WHERE id = ?", (self._id,)
            ).fetchone()
            existing = row["notes"] or ""
            updated = (existing + "\n" + content).strip()
            self._store._conn.execute(
                "UPDATE runs SET notes = ? WHERE id = ?",
                (updated, self._id),
            )
            self._store._conn.commit()

    # ------------------------------------------------------------------
    # TODOs
    # ------------------------------------------------------------------

    def todo(self, content: str, priority: int = 0) -> None:
        """Create a TODO scoped to this run."""
        self._check_active()
        todo_id = str(ULID())
        with self._store.lock:
            self._store._conn.execute(
                "INSERT INTO todos (id, scope_type, scope_id, content, priority) "
                "VALUES (?, 'run', ?, ?, ?)",
                (todo_id, self._id, content, priority),
            )
            self._store._conn.commit()

    # ------------------------------------------------------------------
    # Models
    # ------------------------------------------------------------------

    def register_model(
        self,
        name: str,
        version: str,
        path: str,
        metadata: dict | None = None,
        framework: str = "pytorch",
    ) -> None:
        """Register a model version, copying it to the models directory."""
        self._check_active()
        dest_dir = self._store.root / "models" / name / version
        dest_dir.mkdir(parents=True, exist_ok=True)

        src = Path(path)
        if src.is_dir():
            dest = dest_dir / src.name
            if dest.exists():
                shutil.rmtree(dest)
            shutil.copytree(src, dest)
        else:
            shutil.copy2(src, dest_dir / src.name)

        artifact_path = str(dest_dir / src.name)
        model_id = str(ULID())
        with self._store.lock:
            self._store._conn.execute(
                "INSERT INTO models "
                "(id, name, version, run_id, artifact_path, framework, metadata) "
                "VALUES (?, ?, ?, ?, ?, ?, ?)",
                (
                    model_id,
                    name,
                    version,
                    self._id,
                    artifact_path,
                    framework,
                    json.dumps(metadata) if metadata else None,
                ),
            )
            self._store._conn.commit()

    # ------------------------------------------------------------------
    # Lineage
    # ------------------------------------------------------------------

    def derived_from(
        self,
        run: str | None = None,
        model: str | None = None,
        version: str | None = None,
    ) -> None:
        """Record that this run is derived from another run or model."""
        self._check_active()
        with self._store.lock:
            if run is not None:
                self._store._conn.execute(
                    "INSERT OR IGNORE INTO lineage "
                    "(parent_type, parent_id, child_type, child_id, relation) "
                    "VALUES ('run', ?, 'run', ?, 'derived_from')",
                    (run, self._id),
                )
            if model is not None:
                # Look up the model by name+version if version provided, else by id
                if version is not None:
                    row = self._store._conn.execute(
                        "SELECT id FROM models WHERE name = ? AND version = ?",
                        (model, version),
                    ).fetchone()
                    model_id = row["id"] if row else model
                else:
                    model_id = model
                self._store._conn.execute(
                    "INSERT OR IGNORE INTO lineage "
                    "(parent_type, parent_id, child_type, child_id, relation) "
                    "VALUES ('model', ?, 'run', ?, 'derived_from')",
                    (model_id, self._id),
                )
            self._store._conn.commit()

    def branched_from(
        self,
        experiment: str | None = None,
        run: str | None = None,
    ) -> None:
        """Record that this run branched from an experiment or another run."""
        self._check_active()
        with self._store.lock:
            if experiment is not None:
                self._store._conn.execute(
                    "INSERT OR IGNORE INTO lineage "
                    "(parent_type, parent_id, child_type, child_id, relation) "
                    "VALUES ('experiment', ?, 'run', ?, 'branched_from')",
                    (experiment, self._id),
                )
            if run is not None:
                self._store._conn.execute(
                    "INSERT OR IGNORE INTO lineage "
                    "(parent_type, parent_id, child_type, child_id, relation) "
                    "VALUES ('run', ?, 'run', ?, 'branched_from')",
                    (run, self._id),
                )
            self._store._conn.commit()

    def __repr__(self) -> str:
        return f"Run(id={self._id!r})"
