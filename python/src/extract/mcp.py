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
            # Include: exact prefix match, all descendants, and all ancestors
            # (so the full branch context is returned).
            clean = prefix.rstrip("/")
            rows = _store._conn.execute(
                "SELECT id, path, name, node_type, parent_id "
                "FROM experiments "
                "WHERE path = ? OR path LIKE ? OR ? LIKE (path || '/%') "
                "ORDER BY path",
                (clean, clean + "/%", clean),
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
