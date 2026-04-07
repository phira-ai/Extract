"""Extract MCP server: read-only tool surface for LLM agents.

Invoked as `python -m extract.mcp [--store PATH]`. Runs FastMCP over
stdio by default. See docs/superpowers/specs/2026-04-07-phase6-mcp-server-design.md
for the full design.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any

try:
    from mcp.server.fastmcp import FastMCP
except ImportError:
    FastMCP = None  # type: ignore[assignment,misc]

from extract.store import Store

# Module-level state. Set by main() at startup; monkey-patched by tests.
_store: Store | None = None
mcp_server: Any = FastMCP("extract") if FastMCP else None


def _tool(fn):
    """Register a function with the FastMCP server if it's available.

    When `mcp` isn't installed, this is a no-op — the function is still
    defined at module level so unit tests can call it directly.
    """
    if mcp_server is not None:
        return mcp_server.tool()(fn)
    return fn


# ----------------------------------------------------------------------
# Shared helpers (pure, no DB access)
# ----------------------------------------------------------------------

_MIN_METRIC_PATTERNS = (
    "loss", "error", "perplexity", "mse", "mae", "rmse",
    "nll", "cer", "wer", "fid", "divergence",
)


def _row_to_dict(row) -> dict:
    """Convert a sqlite3.Row to a plain dict, parsing JSON columns."""
    d: dict = {}
    for key in row.keys():
        val = row[key]
        if key == "tags":
            d[key] = json.loads(val) if val else []
        elif key == "config":
            d[key] = json.loads(val) if val else {}
        elif key == "metadata":
            d[key] = json.loads(val) if val else None
        else:
            d[key] = val
    return d


def _label(experiment_path: str, run_name: str | None, run_id: str) -> str:
    """Build the human-readable anchor for a run."""
    tail = run_name if run_name else run_id[:8]
    return f"{experiment_path}#{tail}"


def _flatten_config(config: dict, prefix: str = "") -> dict:
    """Flatten nested dicts into dot-notation keys. Lists are leaf values."""
    result: dict = {}
    for k, v in config.items():
        key = f"{prefix}{k}"
        if isinstance(v, dict):
            result.update(_flatten_config(v, prefix=f"{key}."))
        else:
            result[key] = v
    return result


def _metric_direction(name: str) -> str:
    """Return 'min' if metric name matches a minimize pattern, else 'max'."""
    lowered = name.lower()
    for pat in _MIN_METRIC_PATTERNS:
        if pat in lowered:
            return "min"
    return "max"


def _config_diffs(runs_configs: list[tuple[str, dict]]) -> dict:
    """Return {flat_key: {run_id: value}} for keys that differ across runs.

    A key that's present in some runs but missing in others counts as a
    difference — the result shows only the runs that have the key.
    """
    _MISSING = object()
    flattened: list[tuple[str, dict]] = [
        (rid, _flatten_config(cfg or {})) for rid, cfg in runs_configs
    ]
    all_keys: set[str] = set()
    for _, flat in flattened:
        all_keys.update(flat.keys())

    result: dict = {}
    for key in all_keys:
        values = [(rid, flat.get(key, _MISSING)) for rid, flat in flattened]
        distinct = {id(v) if v is _MISSING else (type(v).__name__, repr(v))
                    for _, v in values}
        if len(distinct) > 1:
            result[key] = {rid: v for rid, v in values if v is not _MISSING}
    return result


def _clamp_limit(limit: int) -> tuple[int, bool]:
    """Clamp limit to the hard cap of 500, returning (limit, was_clamped).

    Does not validate the lower bound — callers are expected to raise
    ValueError for limit < 1 before calling this.
    """
    if limit > 500:
        return 500, True
    return limit, False


def _listing(
    items: list,
    total: int,
    limit: int,
    limit_clamped: bool = False,
) -> dict:
    """Wrap a list in the shared listing envelope.

    `total` must be the full row count from the DB (before LIMIT),
    not len(items). `truncated` is computed as `total > limit`.
    Tool implementations should run a COUNT(*) query to get `total`
    and a separate LIMIT query to get `items`, or use len(items) only
    when every row is known to be loaded.
    """
    result: dict = {
        "items": items[:limit],
        "total": total,
        "truncated": total > limit,
    }
    if limit_clamped:
        result["limit_clamped"] = True
    return result


# ----------------------------------------------------------------------
# Tools
# ----------------------------------------------------------------------


@_tool
def list_experiments(prefix: str = "", limit: int = 50) -> dict:
    """List experiments, optionally filtered by a path prefix.

    Args:
        prefix: Filter to experiments whose path starts with this (e.g.
            "cifar100/ewc"). Empty string lists all experiments.
        limit: Max number of items to return (default 50, max 500).

    Returns a listing envelope:
        {items: [{id, path, name, node_type, parent_id, n_runs}],
         total: int, truncated: bool, limit_clamped?: bool}

    Example:
        list_experiments(prefix="cifar100/")
    """
    if limit < 1:
        raise ValueError(f"limit must be >= 1 (got {limit})")
    limit, clamped = _clamp_limit(limit)

    assert _store is not None
    with _store.lock:
        if prefix:
            rows = _store._conn.execute(
                "SELECT id, path, name, node_type, parent_id "
                "FROM experiments WHERE path = ? OR path LIKE ? "
                "ORDER BY path",
                (prefix, prefix.rstrip("/") + "/%"),
            ).fetchall()
        else:
            rows = _store._conn.execute(
                "SELECT id, path, name, node_type, parent_id "
                "FROM experiments ORDER BY path"
            ).fetchall()

        items: list[dict] = []
        for row in rows:
            n_runs_row = _store._conn.execute(
                "SELECT COUNT(*) FROM runs WHERE experiment_id = ?",
                (row["id"],),
            ).fetchone()
            items.append({
                "id": row["id"],
                "path": row["path"],
                "name": row["name"],
                "node_type": row["node_type"],
                "parent_id": row["parent_id"],
                "n_runs": n_runs_row[0],
            })

    return _listing(items, total=len(items), limit=limit, limit_clamped=clamped)


@_tool
def list_runs(experiment_id: str | None = None, limit: int = 50) -> dict:
    """List runs in the store, optionally scoped to one experiment.

    Args:
        experiment_id: If provided, list runs for that experiment only.
            If omitted, list all runs newest-first.
        limit: Max rows (default 50, max 500).

    Returns a listing envelope of run rows, each with id, label,
    experiment_id, experiment_path, name, status, started_at, ended_at,
    tags, git_sha, hostname, and a config_summary {n_keys, top_level_keys}.
    Call get_run(id) for the full config.
    """
    if limit < 1:
        raise ValueError(f"limit must be >= 1 (got {limit})")
    limit, clamped = _clamp_limit(limit)

    assert _store is not None
    with _store.lock:
        if experiment_id is not None:
            exp_row = _store._conn.execute(
                "SELECT id, path FROM experiments WHERE id = ?",
                (experiment_id,),
            ).fetchone()
            if exp_row is None:
                raise ValueError(f"Experiment not found: {experiment_id!r}")
            rows = _store._conn.execute(
                "SELECT r.*, e.path AS experiment_path "
                "FROM runs r JOIN experiments e ON r.experiment_id = e.id "
                "WHERE r.experiment_id = ? ORDER BY r.started_at",
                (experiment_id,),
            ).fetchall()
        else:
            rows = _store._conn.execute(
                "SELECT r.*, e.path AS experiment_path "
                "FROM runs r JOIN experiments e ON r.experiment_id = e.id "
                "ORDER BY r.started_at DESC"
            ).fetchall()

    items: list[dict] = []
    for row in rows:
        config_dict = json.loads(row["config"]) if row["config"] else {}
        top_keys = list(config_dict.keys())
        items.append({
            "id": row["id"],
            "label": _label(row["experiment_path"], row["name"], row["id"]),
            "experiment_id": row["experiment_id"],
            "experiment_path": row["experiment_path"],
            "name": row["name"],
            "status": row["status"],
            "started_at": row["started_at"],
            "ended_at": row["ended_at"],
            "tags": json.loads(row["tags"]) if row["tags"] else [],
            "git_sha": row["git_sha"],
            "hostname": row["hostname"],
            "config_summary": {
                "n_keys": len(top_keys),
                "top_level_keys": top_keys,
            },
        })

    return _listing(items, total=len(items), limit=limit, limit_clamped=clamped)


@_tool
def list_models(name_prefix: str = "", limit: int = 50) -> dict:
    """List registered models, optionally filtered by name prefix.

    Args:
        name_prefix: If provided, return only models whose name starts
            with this string.
        limit: Max rows (default 50, max 500).

    Returns a listing envelope of model rows: id, name, version, run_id,
    framework, artifact_path, metadata, created_at.
    """
    if limit < 1:
        raise ValueError(f"limit must be >= 1 (got {limit})")
    limit, clamped = _clamp_limit(limit)

    assert _store is not None
    with _store.lock:
        if name_prefix:
            rows = _store._conn.execute(
                "SELECT * FROM models WHERE name LIKE ? ORDER BY created_at DESC",
                (name_prefix + "%",),
            ).fetchall()
        else:
            rows = _store._conn.execute(
                "SELECT * FROM models ORDER BY created_at DESC"
            ).fetchall()

    items: list[dict] = []
    for row in rows:
        items.append({
            "id": row["id"],
            "name": row["name"],
            "version": row["version"],
            "run_id": row["run_id"],
            "framework": row["framework"],
            "artifact_path": row["artifact_path"],
            "metadata": json.loads(row["metadata"]) if row["metadata"] else None,
            "created_at": row["created_at"],
        })

    return _listing(items, total=len(items), limit=limit, limit_clamped=clamped)


@_tool
def list_todos(
    scope_type: str = "global",
    scope_id: str | None = None,
    include_done: bool = False,
    limit: int = 50,
) -> dict:
    """List TODOs scoped to global, experiment, or run level.

    Args:
        scope_type: "global", "experiment", or "run".
        scope_id: Required when scope_type is "experiment" or "run";
            must be None when scope_type is "global".
        include_done: If False (default), excludes completed TODOs.
        limit: Max rows (default 50, max 500).

    Returns a listing envelope of todo rows, ordered by priority DESC
    then created_at.
    """
    valid_scopes = ("global", "experiment", "run")
    if scope_type not in valid_scopes:
        raise ValueError(
            f"scope_type must be one of: global, experiment, run "
            f"(got {scope_type!r})"
        )
    if scope_type == "global":
        if scope_id is not None:
            raise ValueError("scope_id must be None when scope_type='global'")
    else:
        if scope_id is None:
            raise ValueError(f"scope_id is required when scope_type={scope_type!r}")

    if limit < 1:
        raise ValueError(f"limit must be >= 1 (got {limit})")
    limit, clamped = _clamp_limit(limit)

    assert _store is not None
    with _store.lock:
        where = "scope_type = ?"
        params: list = [scope_type]
        if scope_id is not None:
            where += " AND scope_id = ?"
            params.append(scope_id)
        if not include_done:
            where += " AND done = 0"
        rows = _store._conn.execute(
            f"SELECT * FROM todos WHERE {where} ORDER BY priority DESC, created_at",
            params,
        ).fetchall()

    items = [dict(r) for r in rows]
    return _listing(items, total=len(items), limit=limit, limit_clamped=clamped)


@_tool
def get_run(run_id: str) -> dict:
    """Return full detail for a single run.

    Args:
        run_id: ULID of the run.

    Returns a dict with id, experiment_id, experiment_path, name, label,
    status, started_at, ended_at, hostname, git_sha, tags, notes, config
    (full parsed dict), metrics_final (last value per metric),
    metrics_available (list of metric names), run_params (string params),
    artifacts (list of artifact metadata), and todos (scoped to this run).

    Metric histories are NOT included — use compare_runs with
    include_history=True on a single run if you need them.
    """
    if not run_id:
        raise ValueError("run_id is required")

    assert _store is not None
    with _store.lock:
        row = _store._conn.execute(
            "SELECT r.*, e.path AS experiment_path "
            "FROM runs r JOIN experiments e ON r.experiment_id = e.id "
            "WHERE r.id = ?",
            (run_id,),
        ).fetchone()
        if row is None:
            raise ValueError(f"Run not found: {run_id!r}")

        # Final value per metric: the row with the largest step per metric name.
        metric_rows = _store._conn.execute(
            "SELECT name, value FROM scalar_metrics sm1 WHERE run_id = ? "
            "AND step = (SELECT MAX(step) FROM scalar_metrics sm2 "
            "            WHERE sm2.run_id = sm1.run_id AND sm2.name = sm1.name)",
            (run_id,),
        ).fetchall()
        metrics_final = {r["name"]: r["value"] for r in metric_rows}
        metrics_available = sorted(metrics_final.keys())

        param_rows = _store._conn.execute(
            "SELECT name, value FROM run_params WHERE run_id = ?",
            (run_id,),
        ).fetchall()
        run_params = {r["name"]: r["value"] for r in param_rows}

        art_rows = _store._conn.execute(
            "SELECT name, kind, step, rel_path, shape, dtype "
            "FROM artifacts WHERE run_id = ?",
            (run_id,),
        ).fetchall()
        artifacts: list[dict] = []
        for a in art_rows:
            artifacts.append({
                "name": a["name"],
                "kind": a["kind"],
                "step": a["step"],
                "rel_path": a["rel_path"],
                "shape": json.loads(a["shape"]) if a["shape"] else None,
                "dtype": a["dtype"],
            })

        todo_rows = _store._conn.execute(
            "SELECT id, content, priority, done, created_at, completed_at "
            "FROM todos WHERE scope_type = 'run' AND scope_id = ? "
            "ORDER BY priority DESC, created_at",
            (run_id,),
        ).fetchall()
        todos = [dict(t) for t in todo_rows]

    return {
        "id": row["id"],
        "experiment_id": row["experiment_id"],
        "experiment_path": row["experiment_path"],
        "name": row["name"],
        "label": _label(row["experiment_path"], row["name"], row["id"]),
        "status": row["status"],
        "started_at": row["started_at"],
        "ended_at": row["ended_at"],
        "hostname": row["hostname"],
        "git_sha": row["git_sha"],
        "tags": json.loads(row["tags"]) if row["tags"] else [],
        "notes": row["notes"] or "",
        "config": json.loads(row["config"]) if row["config"] else {},
        "metrics_final": metrics_final,
        "metrics_available": metrics_available,
        "run_params": run_params,
        "artifacts": artifacts,
        "todos": todos,
    }


def main(argv: list[str] | None = None) -> None:
    if FastMCP is None:
        print(
            "extract-tracker[mcp] extra not installed. "
            "Install with: pip install 'extract-tracker[mcp]'",
            file=sys.stderr,
        )
        sys.exit(1)

    parser = argparse.ArgumentParser(prog="extract.mcp")
    parser.add_argument("--store", default=".extract")
    args = parser.parse_args(argv)

    store_path = Path(args.store)
    if not store_path.exists():
        print(
            f"store not found: {store_path} — run training with "
            "extract-tracker first, or pass --store",
            file=sys.stderr,
        )
        sys.exit(1)

    global _store
    try:
        _store = Store(store_path)
    except Exception as e:
        print(f"failed to open store: {e}", file=sys.stderr)
        sys.exit(1)

    mcp_server.run()


if __name__ == "__main__":
    main()
