"""Sync: transfer .extract/ stores between machines via rsync or tar archives.

Merge strategy: experiments are matched by *path* (not ULID), so independently
created stores with the same hierarchy merge cleanly. Runs, metrics, and
artifacts use ULIDs so they never collide.
"""

from __future__ import annotations

import os
import shutil
import sqlite3
import subprocess
import tarfile
import tempfile
from pathlib import Path


class SyncError(Exception):
    pass


class SyncLock:
    """Context manager for .extract/sync.lock to prevent concurrent syncs."""

    def __init__(self, root: Path) -> None:
        self.lock_path = root / "sync.lock"

    def __enter__(self) -> SyncLock:
        if not self.lock_path.parent.exists():
            raise SyncError(f"Store directory does not exist: {self.lock_path.parent}")
        if self.lock_path.exists():
            pid = self.lock_path.read_text().strip()
            raise SyncError(
                f"Sync already in progress (lock: {self.lock_path}, pid={pid}). "
                "If stale, remove the lock file manually."
            )
        self.lock_path.write_text(str(os.getpid()))
        return self

    def __exit__(self, *exc: object) -> None:
        self.lock_path.unlink(missing_ok=True)


def checkpoint_wal(root: Path) -> None:
    """Fold WAL into main DB file so rsync transfers a single consistent file."""
    db_path = root / "extract.db"
    if not db_path.exists():
        return
    conn = sqlite3.connect(str(db_path))
    try:
        conn.execute("PRAGMA wal_checkpoint(TRUNCATE)")
    finally:
        conn.close()


def merge_db(src_path: Path, dst_path: Path) -> dict[str, int]:
    """Merge src DB into dst DB, matching experiments by path.

    - Experiments with the same path are unified (no duplicates in the tree).
    - Runs and child rows are remapped to the destination's experiment IDs.
    - Run ULIDs are globally unique, so INSERT OR IGNORE handles dedup.

    Returns a dict of {table: rows_added}.
    """
    src = sqlite3.connect(str(src_path))
    src.row_factory = sqlite3.Row
    dst = sqlite3.connect(str(dst_path))
    dst.row_factory = sqlite3.Row
    stats: dict[str, int] = {}

    try:
        dst.execute("PRAGMA foreign_keys=OFF")

        # --- hierarchy (INSERT OR IGNORE, no remapping) ---
        for row in src.execute("SELECT * FROM hierarchy").fetchall():
            dst.execute(
                "INSERT OR IGNORE INTO hierarchy (level_order, level_name) VALUES (?, ?)",
                (row["level_order"], row["level_name"]),
            )

        # --- experiments: match by path, build id remapping ---
        exp_remap: dict[str, str] = {}  # src_id → dst_id

        # Index destination experiments by path
        dst_exp_by_path: dict[str, str] = {}
        for row in dst.execute("SELECT id, path FROM experiments").fetchall():
            dst_exp_by_path[row["path"]] = row["id"]

        src_exps = src.execute(
            "SELECT * FROM experiments ORDER BY path"  # parents before children
        ).fetchall()

        new_exps = 0
        for row in src_exps:
            src_id = row["id"]
            path = row["path"]
            if path in dst_exp_by_path:
                # Experiment exists — remap, don't insert
                exp_remap[src_id] = dst_exp_by_path[path]
            else:
                # New experiment — remap parent_id if needed, then insert
                parent_id = row["parent_id"]
                if parent_id and parent_id in exp_remap:
                    parent_id = exp_remap[parent_id]
                dst.execute(
                    "INSERT INTO experiments "
                    "(id, path, name, parent_id, created_at, metadata, status, node_type) "
                    "VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                    (src_id, path, row["name"], parent_id,
                     row["created_at"], row["metadata"], row["status"], row["node_type"]),
                )
                exp_remap[src_id] = src_id  # keeps its own ID
                dst_exp_by_path[path] = src_id
                new_exps += 1
        stats["experiments"] = new_exps

        # --- runs: remap experiment_id ---
        src_runs = src.execute("SELECT * FROM runs").fetchall()
        new_runs = 0
        for row in src_runs:
            exp_id = exp_remap.get(row["experiment_id"], row["experiment_id"])
            r = dst.execute("SELECT 1 FROM runs WHERE id = ?", (row["id"],)).fetchone()
            if r is not None:
                continue  # already exists
            dst.execute(
                "INSERT INTO runs "
                "(id, experiment_id, name, config, started_at, ended_at, "
                "status, hostname, git_sha, tags, notes, total_steps) "
                "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                (row["id"], exp_id, row["name"], row["config"],
                 row["started_at"], row["ended_at"], row["status"],
                 row["hostname"], row["git_sha"], row["tags"], row["notes"],
                 row["total_steps"]),
            )
            new_runs += 1
        stats["runs"] = new_runs

        # --- AUTOINCREMENT tables: exclude integer PK, let dst assign new ids ---
        for table in ("scalar_metrics", "run_params", "curve_points"):
            cols_info = dst.execute(f"PRAGMA table_info({table})").fetchall()
            cols = [c[1] for c in cols_info if c[1] != "id"]
            col_list = ", ".join(cols)
            placeholders = ", ".join("?" * len(cols))

            before = dst.execute(f"SELECT count(*) FROM {table}").fetchone()[0]
            for row in src.execute(f"SELECT * FROM {table}").fetchall():
                vals = tuple(row[c] for c in cols)
                dst.execute(
                    f"INSERT OR IGNORE INTO {table} ({col_list}) VALUES ({placeholders})",
                    vals,
                )
            after = dst.execute(f"SELECT count(*) FROM {table}").fetchone()[0]
            stats[table] = after - before

        # --- artifacts: ULID TEXT PK, keep id intact ---
        before = dst.execute("SELECT count(*) FROM artifacts").fetchone()[0]
        for row in src.execute("SELECT * FROM artifacts").fetchall():
            dst.execute(
                "INSERT OR IGNORE INTO artifacts "
                "(id, run_id, name, kind, step, rel_path, shape, dtype, metadata, created_at) "
                "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                (row["id"], row["run_id"], row["name"], row["kind"], row["step"],
                 row["rel_path"], row["shape"], row["dtype"], row["metadata"],
                 row["created_at"]),
            )
        stats["artifacts"] = dst.execute("SELECT count(*) FROM artifacts").fetchone()[0] - before

        # --- models (ULID PK, run_id FK — no remapping needed) ---
        before = dst.execute("SELECT count(*) FROM models").fetchone()[0]
        for row in src.execute("SELECT * FROM models").fetchall():
            dst.execute(
                "INSERT OR IGNORE INTO models "
                "(id, name, version, run_id, artifact_path, framework, metadata, created_at) "
                "VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                (row["id"], row["name"], row["version"], row["run_id"],
                 row["artifact_path"], row["framework"], row["metadata"], row["created_at"]),
            )
        stats["models"] = dst.execute("SELECT count(*) FROM models").fetchone()[0] - before

        # --- lineage (remap experiment IDs in parent/child refs) ---
        before = dst.execute("SELECT count(*) FROM lineage").fetchone()[0]
        for row in src.execute("SELECT * FROM lineage").fetchall():
            parent_id = row["parent_id"]
            child_id = row["child_id"]
            if row["parent_type"] == "experiment" and parent_id in exp_remap:
                parent_id = exp_remap[parent_id]
            if row["child_type"] == "experiment" and child_id in exp_remap:
                child_id = exp_remap[child_id]
            dst.execute(
                "INSERT OR IGNORE INTO lineage "
                "(parent_type, parent_id, child_type, child_id, relation, metadata, created_at) "
                "VALUES (?, ?, ?, ?, ?, ?, ?)",
                (row["parent_type"], parent_id, row["child_type"], child_id,
                 row["relation"], row["metadata"], row["created_at"]),
            )
        stats["lineage"] = dst.execute("SELECT count(*) FROM lineage").fetchone()[0] - before

        # --- todos (remap experiment-scoped scope_id) ---
        before = dst.execute("SELECT count(*) FROM todos").fetchone()[0]
        for row in src.execute("SELECT * FROM todos").fetchall():
            scope_id = row["scope_id"]
            if row["scope_type"] == "experiment" and scope_id and scope_id in exp_remap:
                scope_id = exp_remap[scope_id]
            dst.execute(
                "INSERT OR IGNORE INTO todos "
                "(id, scope_type, scope_id, content, done, priority, created_at, completed_at) "
                "VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                (row["id"], row["scope_type"], scope_id, row["content"],
                 row["done"], row["priority"], row["created_at"], row["completed_at"]),
            )
        stats["todos"] = dst.execute("SELECT count(*) FROM todos").fetchone()[0] - before

        dst.commit()
        dst.execute("PRAGMA foreign_keys=ON")
    finally:
        src.close()
        dst.close()
    return stats


def _merge_artifacts(src_dir: Path, dst_dir: Path) -> None:
    """Copy artifact files from src to dst, skipping existing files."""
    if not src_dir.exists():
        return
    dst_dir.mkdir(parents=True, exist_ok=True)
    for item in src_dir.iterdir():
        dst_item = dst_dir / item.name
        if item.is_dir():
            _merge_artifacts(item, dst_item)
        elif not dst_item.exists():
            shutil.copy2(item, dst_item)


def _run_rsync(src: str, dst: str) -> None:
    """Run rsync -avz and raise on failure."""
    result = subprocess.run(
        ["rsync", "-avz", src, dst],
        capture_output=True, text=True,
    )
    if result.returncode != 0:
        raise SyncError(f"rsync failed:\n{result.stderr}")


def push(root: Path, remote: str) -> None:
    """Push local .extract/ to a remote path via rsync over SSH."""
    root = Path(root)
    with SyncLock(root):
        checkpoint_wal(root)
        _run_rsync(str(root) + "/", remote.rstrip("/") + "/")


def pull(root: Path, remote: str) -> dict[str, int]:
    """Pull remote .extract/ into local store via rsync + DB merge."""
    root = Path(root)
    root.mkdir(parents=True, exist_ok=True)
    (root / "artifacts").mkdir(exist_ok=True)
    (root / "models").mkdir(exist_ok=True)

    with SyncLock(root):
        with tempfile.TemporaryDirectory() as tmp:
            tmp_root = Path(tmp) / ".extract"
            tmp_root.mkdir()
            # Rsync remote into temp directory
            _run_rsync(remote.rstrip("/") + "/", str(tmp_root) + "/")
            # Merge DB
            checkpoint_wal(tmp_root)
            stats = merge_db(tmp_root / "extract.db", root / "extract.db")
            # Merge artifacts and models
            _merge_artifacts(tmp_root / "artifacts", root / "artifacts")
            _merge_artifacts(tmp_root / "models", root / "models")
    return stats


def export_archive(root: Path, output: Path) -> None:
    """Create a tar.gz archive of .extract/ for manual transfer."""
    root = Path(root)
    output = Path(output)
    with SyncLock(root):
        checkpoint_wal(root)
        with tarfile.open(output, "w:gz") as tar:
            tar.add(str(root), arcname=".extract")


def import_archive(archive: Path, root: Path) -> dict[str, int]:
    """Merge a tar.gz archive into an existing .extract/ store."""
    archive = Path(archive)
    root = Path(root)
    root.mkdir(parents=True, exist_ok=True)
    (root / "artifacts").mkdir(exist_ok=True)
    (root / "models").mkdir(exist_ok=True)

    with SyncLock(root):
        with tempfile.TemporaryDirectory() as tmp:
            tmp_root = Path(tmp) / ".extract"
            # Extract archive into temp directory
            with tarfile.open(archive, "r:gz") as tar:
                tar.extractall(path=tmp, filter="data")
            # Merge DB
            checkpoint_wal(tmp_root)
            stats = merge_db(tmp_root / "extract.db", root / "extract.db")
            # Merge artifacts and models
            _merge_artifacts(tmp_root / "artifacts", root / "artifacts")
            _merge_artifacts(tmp_root / "models", root / "models")
    return stats
