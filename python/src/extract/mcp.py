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

import tomli
from extract.store import Store
from mcp.server.fastmcp import FastMCP

# Module-level state. Set by main() at startup; monkey-patched by tests.
_store: Store | None = None
mcp_server = FastMCP("extract")


def _tool(fn):
    """Register a function with the FastMCP server."""
    return mcp_server.tool()(fn)


# ----------------------------------------------------------------------
# Shared helpers (pure, no DB access)
# ----------------------------------------------------------------------

_MIN_METRIC_PATTERNS = (
    "loss",
    "error",
    "perplexity",
    "mse",
    "mae",
    "rmse",
    "nll",
    "cer",
    "wer",
    "fid",
    "divergence",
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


def _metric_direction(
    name: str,
    overrides: tuple[set[str], set[str]] | None = None,
) -> str:
    """Return 'min' or 'max' using config overrides, then heuristics."""
    if overrides is not None:
        minimize, maximize = overrides
        if name in minimize:
            return "min"
        if name in maximize:
            return "max"

    lowered = name.lower()
    for pat in _MIN_METRIC_PATTERNS:
        if pat in lowered:
            return "min"
    return "max"


def _metric_direction_overrides(store: Store) -> tuple[set[str], set[str]]:
    """Read [metrics] minimize/maximize overrides from config.toml."""
    config_path = store.root / "config.toml"
    if not config_path.exists():
        return set(), set()

    with config_path.open("rb") as f:
        data = tomli.load(f)

    metrics = data.get("metrics", {})
    if not isinstance(metrics, dict):
        return set(), set()

    minimize = metrics.get("minimize", [])
    maximize = metrics.get("maximize", [])
    return (
        (
            {m for m in minimize if isinstance(m, str)}
            if isinstance(minimize, list)
            else set()
        ),
        (
            {m for m in maximize if isinstance(m, str)}
            if isinstance(maximize, list)
            else set()
        ),
    )


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
        distinct = {
            id(v) if v is _MISSING else (type(v).__name__, repr(v)) for _, v in values
        }
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
def list_experiments(
    prefix: str = "",
    limit: int = 50,
    include_archived: bool = False,
) -> dict:
    """List experiments, optionally filtered by a path prefix.

    Args:
        prefix: Filter to experiments whose path starts with this (e.g.
            "cifar100/ewc"). Empty string lists all experiments.
        limit: Max number of items to return (default 50, max 500).
        include_archived: If True, include archived experiments and count
            archived runs in n_runs. Default False.

    Returns a listing envelope:
        {items: [{id, path, name, node_type, parent_id, n_runs}],
         total: int, truncated: bool, limit_clamped?: bool}

    Example:
        list_experiments(prefix="cifar100/")
    """
    if limit < 1:
        raise ValueError(f"limit must be >= 1 (got {limit})")
    limit, clamped = _clamp_limit(limit)

    status_filter = "" if include_archived else " AND status != 'archived'"
    run_filter = "" if include_archived else " AND status != 'archived'"

    assert _store is not None
    with _store.lock:
        if prefix:
            rows = _store._conn.execute(
                "SELECT id, path, name, node_type, parent_id "
                f"FROM experiments WHERE (path = ? OR path LIKE ?){status_filter} "
                "ORDER BY path",
                (prefix, prefix.rstrip("/") + "/%"),
            ).fetchall()
        else:
            rows = _store._conn.execute(
                "SELECT id, path, name, node_type, parent_id "
                f"FROM experiments WHERE 1=1{status_filter} ORDER BY path"
            ).fetchall()

        items: list[dict] = []
        for row in rows:
            n_runs_row = _store._conn.execute(
                f"SELECT COUNT(*) FROM runs WHERE experiment_id = ?{run_filter}",
                (row["id"],),
            ).fetchone()
            items.append(
                {
                    "id": row["id"],
                    "path": row["path"],
                    "name": row["name"],
                    "node_type": row["node_type"],
                    "parent_id": row["parent_id"],
                    "n_runs": n_runs_row[0],
                }
            )

    return _listing(items, total=len(items), limit=limit, limit_clamped=clamped)


@_tool
def list_runs(
    experiment_id: str | None = None,
    limit: int = 50,
    include_archived: bool = False,
) -> dict:
    """List runs in the store, optionally scoped to one experiment.

    Args:
        experiment_id: If provided, list runs for that experiment only.
            If omitted, list all runs newest-first.
        limit: Max rows (default 50, max 500).
        include_archived: If True, include archived runs. Default False.

    Returns a listing envelope of run rows, each with id, label,
    experiment_id, experiment_path, name, status, started_at, ended_at,
    tags, git_sha, hostname, and a config_summary {n_keys, top_level_keys}.
    Call get_run(id) for the full config.
    """
    if limit < 1:
        raise ValueError(f"limit must be >= 1 (got {limit})")
    limit, clamped = _clamp_limit(limit)

    status_filter = "" if include_archived else " AND r.status != 'archived'"

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
                f"WHERE r.experiment_id = ?{status_filter} ORDER BY r.started_at",
                (experiment_id,),
            ).fetchall()
        else:
            rows = _store._conn.execute(
                "SELECT r.*, e.path AS experiment_path "
                "FROM runs r JOIN experiments e ON r.experiment_id = e.id "
                f"WHERE 1=1{status_filter} "
                "ORDER BY r.started_at DESC"
            ).fetchall()

    items: list[dict] = []
    for row in rows:
        config_dict = json.loads(row["config"]) if row["config"] else {}
        top_keys = list(config_dict.keys())
        items.append(
            {
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
            }
        )

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
        items.append(
            {
                "id": row["id"],
                "name": row["name"],
                "version": row["version"],
                "run_id": row["run_id"],
                "framework": row["framework"],
                "artifact_path": row["artifact_path"],
                "metadata": json.loads(row["metadata"]) if row["metadata"] else None,
                "created_at": row["created_at"],
            }
        )

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

    Streaming curve histories are NOT included — call
    compare_runs([this_run, another_run], include_curves=True) if you need
    per-step curve data (compare_runs always takes 2+ run ids).
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

        # Headline value per metric — one row per (run_id, name) after the
        # step= removal from Run.log().
        metric_rows = _store._conn.execute(
            "SELECT name, value FROM scalar_metrics WHERE run_id = ?",
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
            artifacts.append(
                {
                    "name": a["name"],
                    "kind": a["kind"],
                    "step": a["step"],
                    "rel_path": a["rel_path"],
                    "shape": json.loads(a["shape"]) if a["shape"] else None,
                    "dtype": a["dtype"],
                }
            )

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


_VALID_STATUS = {"running", "completed", "failed", "archived"}
_VALID_FILTERS = {
    "tag",
    "status",
    "experiment_prefix",
    "started_after",
    "started_before",
}


@_tool
def search(
    query: str = "",
    filters: dict | None = None,
    limit: int = 50,
    include_archived: bool = False,
) -> dict:
    """Search runs by substring + structured filters.

    Args:
        query: Case-insensitive substring matched against run name, tags,
            and notes. Empty string means no text filter.
        filters: Optional dict of AND-combined filters. Valid keys:
            - tag: str — run must contain this tag
            - status: "running" | "completed" | "failed" | "archived"
            - experiment_prefix: str — run's experiment path starts with this
            - started_after: ISO 8601 str (runs.started_at >= value)
            - started_before: ISO 8601 str (runs.started_at <= value)
        limit: Max rows (default 50, max 500).
        include_archived: If True, include archived runs. Default False.

    Returns a listing envelope of run rows in the same shape as list_runs.
    """
    if limit < 1:
        raise ValueError(f"limit must be >= 1 (got {limit})")
    limit, clamped = _clamp_limit(limit)

    filters = filters or {}
    unknown = set(filters.keys()) - _VALID_FILTERS
    if unknown:
        raise ValueError(
            f"Unknown filter: {next(iter(unknown))!r}. "
            f"Valid filters: tag, status, experiment_prefix, "
            f"started_after, started_before"
        )

    if "status" in filters and filters["status"] not in _VALID_STATUS:
        raise ValueError(
            f"status must be one of: running, completed, failed, archived "
            f"(got {filters['status']!r})"
        )

    clauses: list[str] = []
    params: list = []

    if query:
        q = f"%{query}%"
        clauses.append(
            "(LOWER(r.name) LIKE LOWER(?) OR "
            "LOWER(COALESCE(r.tags, '')) LIKE LOWER(?) OR "
            "LOWER(COALESCE(r.notes, '')) LIKE LOWER(?))"
        )
        params += [q, q, q]

    if "tag" in filters:
        # tags is a JSON array; match a quoted tag within it.
        clauses.append("r.tags LIKE ?")
        params.append(f'%"{filters["tag"]}"%')

    if "status" in filters:
        clauses.append("r.status = ?")
        params.append(filters["status"])

    if "experiment_prefix" in filters:
        pfx = filters["experiment_prefix"]
        clauses.append("(e.path = ? OR e.path LIKE ?)")
        params += [pfx, pfx.rstrip("/") + "/%"]

    if "started_after" in filters:
        clauses.append("r.started_at >= ?")
        params.append(filters["started_after"])

    if "started_before" in filters:
        clauses.append("r.started_at <= ?")
        params.append(filters["started_before"])

    if not include_archived:
        clauses.append("r.status != 'archived'")

    where_sql = (" WHERE " + " AND ".join(clauses)) if clauses else ""

    assert _store is not None
    with _store.lock:
        rows = _store._conn.execute(
            f"SELECT r.*, e.path AS experiment_path "
            f"FROM runs r JOIN experiments e ON r.experiment_id = e.id"
            f"{where_sql} ORDER BY r.started_at DESC",
            params,
        ).fetchall()

    items: list[dict] = []
    for row in rows:
        config_dict = json.loads(row["config"]) if row["config"] else {}
        top_keys = list(config_dict.keys())
        items.append(
            {
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
            }
        )

    return _listing(items, total=len(items), limit=limit, limit_clamped=clamped)


@_tool
def compare_runs(run_ids: list[str], include_curves: bool = False) -> dict:
    """Compare 2-10 runs: final headline metric values, rankings, config diffs,
    and optionally per-step streaming curve data.

    Args:
        run_ids: List of 2-10 run ULIDs.
        include_curves: If True, include per-metric per-run `[[step, value], ...]`
            histories from the `curve_points` table (populated by `run.curve()`
            during training). Off by default to keep payloads bounded.

    Returns:
        {
          runs: [{id, label, experiment_path, status}],
          metrics: {
            name: {
              direction: "min" | "max",
              values: {run_id: final_value},
              ranking: [best_run_id, ..., worst_run_id],
            }
          },
          curves: {                                         # only if include_curves
            name: {run_id: [[step, value], ...]}
          },
          config_diffs: {flat_key: {run_id: value}}
        }
    """
    if len(run_ids) < 2:
        raise ValueError(
            f"compare_runs requires at least 2 run_ids (got {len(run_ids)})"
        )
    if len(run_ids) > 10:
        raise ValueError(
            f"compare_runs supports at most 10 runs per call (got {len(run_ids)})"
        )

    assert _store is not None
    runs_out: list[dict] = []
    configs: list[tuple[str, dict]] = []
    metric_values: dict[str, dict[str, float]] = {}  # name -> {run_id: final_val}
    curves_out: dict[str, dict[str, list[list]]] = (
        {}
    )  # name -> {run_id: [[step, value], ...]}

    with _store.lock:
        for rid in run_ids:
            row = _store._conn.execute(
                "SELECT r.*, e.path AS experiment_path "
                "FROM runs r JOIN experiments e ON r.experiment_id = e.id "
                "WHERE r.id = ?",
                (rid,),
            ).fetchone()
            if row is None:
                raise ValueError(f"Run not found: {rid!r}")

            runs_out.append(
                {
                    "id": row["id"],
                    "label": _label(row["experiment_path"], row["name"], row["id"]),
                    "experiment_path": row["experiment_path"],
                    "status": row["status"],
                }
            )
            cfg = json.loads(row["config"]) if row["config"] else {}
            configs.append((rid, cfg))

            # Headline values — after the step= removal from Run.log(),
            # scalar_metrics has at most one row per (run_id, name) at step=0.
            metric_rows = _store._conn.execute(
                "SELECT name, value FROM scalar_metrics WHERE run_id = ?",
                (rid,),
            ).fetchall()
            for mr in metric_rows:
                metric_values.setdefault(mr["name"], {})[rid] = mr["value"]

            # Streaming curve histories if requested.
            if include_curves:
                curve_rows = _store._conn.execute(
                    "SELECT name, step, value FROM curve_points "
                    "WHERE run_id = ? ORDER BY name, step",
                    (rid,),
                ).fetchall()
                for c in curve_rows:
                    curves_out.setdefault(c["name"], {}).setdefault(rid, []).append(
                        [c["step"], c["value"]]
                    )

    # Build the metrics dict.
    direction_overrides = _metric_direction_overrides(_store)
    metrics_out: dict[str, dict] = {}
    for name, vals in metric_values.items():
        direction = _metric_direction(name, direction_overrides)
        reverse = direction == "max"
        ranking = [
            rid
            for rid, _ in sorted(vals.items(), key=lambda kv: kv[1], reverse=reverse)
        ]
        metrics_out[name] = {
            "direction": direction,
            "values": vals,
            "ranking": ranking,
        }

    result: dict = {
        "runs": runs_out,
        "metrics": metrics_out,
        "config_diffs": _config_diffs(configs),
    }
    if include_curves:
        result["curves"] = curves_out
    return result


_VALID_NODE_TYPES = ("experiment", "run", "model")
_VALID_DIRECTIONS = ("ancestors", "descendants", "both")


def _lookup_node_label(conn, node_type: str, node_id: str) -> str | None:
    """Return the label for a node, or None if the node doesn't exist."""
    if node_type == "run":
        row = conn.execute(
            "SELECT r.name, r.id, e.path AS path "
            "FROM runs r JOIN experiments e ON r.experiment_id = e.id "
            "WHERE r.id = ?",
            (node_id,),
        ).fetchone()
        if row is None:
            return None
        return _label(row["path"], row["name"], row["id"])
    elif node_type == "experiment":
        row = conn.execute(
            "SELECT path FROM experiments WHERE id = ?", (node_id,)
        ).fetchone()
        if row is None:
            return None
        return row["path"]
    elif node_type == "model":
        row = conn.execute(
            "SELECT name, version FROM models WHERE id = ?", (node_id,)
        ).fetchone()
        if row is None:
            return None
        return f"{row['name']}@{row['version']}"
    return None


@_tool
def get_lineage(
    node_type: str,
    node_id: str,
    direction: str = "both",
    depth: int = 2,
) -> dict:
    """Walk the lineage DAG from a given node.

    Args:
        node_type: "experiment", "run", or "model".
        node_id: ULID of the node.
        direction: "ancestors", "descendants", or "both" (default).
        depth: BFS hop cap, between 1 and 5 (default 2).

    Returns a flat graph:
        {
          root: {type, id, label},
          nodes: [{type, id, label}],   # discovered, excluding root
          edges: [{parent_type, parent_id, child_type, child_id, relation}]
        }
    """
    if node_type not in _VALID_NODE_TYPES:
        raise ValueError(
            f"node_type must be one of: experiment, run, model (got {node_type!r})"
        )
    if direction not in _VALID_DIRECTIONS:
        raise ValueError(
            f"direction must be one of: ancestors, descendants, both "
            f"(got {direction!r})"
        )
    if depth < 1 or depth > 5:
        raise ValueError(f"depth must be between 1 and 5 (got {depth})")
    if not node_id:
        raise ValueError(f"{node_type}_id is required")

    assert _store is not None
    with _store.lock:
        root_label = _lookup_node_label(_store._conn, node_type, node_id)
        if root_label is None:
            pretty = {"run": "Run", "experiment": "Experiment", "model": "Model"}[
                node_type
            ]
            raise ValueError(f"{pretty} not found: {node_id!r}")

        visited: set[tuple[str, str]] = {(node_type, node_id)}
        edges_out: list[dict] = []
        frontier: list[tuple[str, str]] = [(node_type, node_id)]

        for _hop in range(depth):
            next_frontier: list[tuple[str, str]] = []
            for nt, nid in frontier:
                # Descendants: I am the parent.
                if direction in ("descendants", "both"):
                    rows = _store._conn.execute(
                        "SELECT parent_type, parent_id, child_type, child_id, relation "
                        "FROM lineage WHERE parent_type = ? AND parent_id = ?",
                        (nt, nid),
                    ).fetchall()
                    for e in rows:
                        edges_out.append(dict(e))
                        key = (e["child_type"], e["child_id"])
                        if key not in visited:
                            visited.add(key)
                            next_frontier.append(key)
                # Ancestors: I am the child.
                if direction in ("ancestors", "both"):
                    rows = _store._conn.execute(
                        "SELECT parent_type, parent_id, child_type, child_id, relation "
                        "FROM lineage WHERE child_type = ? AND child_id = ?",
                        (nt, nid),
                    ).fetchall()
                    for e in rows:
                        edges_out.append(dict(e))
                        key = (e["parent_type"], e["parent_id"])
                        if key not in visited:
                            visited.add(key)
                            next_frontier.append(key)
            frontier = next_frontier
            if not frontier:
                break

        # Dedupe edges in case descendants/ancestors both found the same one.
        seen_edges: set[tuple] = set()
        unique_edges: list[dict] = []
        for e in edges_out:
            key = (
                e["parent_type"],
                e["parent_id"],
                e["child_type"],
                e["child_id"],
                e["relation"],
            )
            if key not in seen_edges:
                seen_edges.add(key)
                unique_edges.append(e)

        nodes_out: list[dict] = []
        for nt, nid in visited:
            if (nt, nid) == (node_type, node_id):
                continue  # root is emitted separately
            label = _lookup_node_label(_store._conn, nt, nid)
            nodes_out.append({"type": nt, "id": nid, "label": label or ""})

    return {
        "root": {"type": node_type, "id": node_id, "label": root_label},
        "nodes": nodes_out,
        "edges": unique_edges,
    }


def main(argv: list[str] | None = None) -> None:
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
