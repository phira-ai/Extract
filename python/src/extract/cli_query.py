"""JSON CLI adapter for Extract read-only query tools.

This module reuses the existing MCP tool implementations so the command-line
surface and MCP surface share one result contract while the query core is
factored out in a later cleanup.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any

from extract.store import Store


def _add_common(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "--store", default=".extract", help="Path to .extract/ directory"
    )
    parser.add_argument(
        "--format",
        choices=("json",),
        default="json",
        help="Output format (default: json)",
    )


def add_query_parsers(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    """Register read-only query subcommands on the top-level CLI parser."""
    experiments = sub.add_parser("experiments", help="Read experiments from a store")
    exp_sub = experiments.add_subparsers(dest="query_action")
    exp_list = exp_sub.add_parser("list", help="List experiments")
    _add_common(exp_list)
    exp_list.add_argument("--prefix", default="", help="Experiment path prefix")
    exp_list.add_argument("--limit", type=int, default=50, help="Max rows (max 500)")
    exp_list.add_argument("--include-archived", action="store_true")
    exp_list.set_defaults(_extract_query="list_experiments")

    runs = sub.add_parser("runs", help="Read runs from a store")
    runs_sub = runs.add_subparsers(dest="query_action")
    runs_list = runs_sub.add_parser("list", help="List runs")
    _add_common(runs_list)
    runs_list.add_argument(
        "--experiment-id", default=None, help="Scope to one experiment id"
    )
    runs_list.add_argument("--limit", type=int, default=50, help="Max rows (max 500)")
    runs_list.add_argument("--include-archived", action="store_true")
    runs_list.set_defaults(_extract_query="list_runs")

    runs_get = runs_sub.add_parser("get", help="Get full run detail")
    _add_common(runs_get)
    runs_get.add_argument("run_id", help="Run ULID")
    runs_get.set_defaults(_extract_query="get_run")

    runs_compare = runs_sub.add_parser("compare", help="Compare 2-10 runs")
    _add_common(runs_compare)
    runs_compare.add_argument("run_ids", nargs="+", help="Run ULIDs")
    runs_compare.add_argument("--include-curves", action="store_true")
    runs_compare.set_defaults(_extract_query="compare_runs")

    search = sub.add_parser("search", help="Search runs")
    _add_common(search)
    search.add_argument("--query", default="", help="Case-insensitive text search")
    search.add_argument("--tag", default=None, help="Require tag")
    search.add_argument("--status", default=None, help="Require status")
    search.add_argument(
        "--experiment-prefix", default=None, help="Experiment path prefix"
    )
    search.add_argument(
        "--started-after", default=None, help="ISO timestamp lower bound"
    )
    search.add_argument(
        "--started-before", default=None, help="ISO timestamp upper bound"
    )
    search.add_argument("--limit", type=int, default=50, help="Max rows (max 500)")
    search.add_argument("--include-archived", action="store_true")
    search.set_defaults(_extract_query="search")

    todos = sub.add_parser("todos", help="Read TODOs from a store")
    todos_sub = todos.add_subparsers(dest="query_action")
    todos_list = todos_sub.add_parser("list", help="List TODOs")
    _add_common(todos_list)
    todos_list.add_argument(
        "--scope-type",
        default="global",
        choices=("global", "experiment", "run"),
        help="TODO scope",
    )
    todos_list.add_argument(
        "--scope-id", default=None, help="Required for experiment/run scope"
    )
    todos_list.add_argument("--include-done", action="store_true")
    todos_list.add_argument("--limit", type=int, default=50, help="Max rows (max 500)")
    todos_list.set_defaults(_extract_query="list_todos")

    lineage = sub.add_parser("lineage", help="Read lineage graph from a store")
    lineage_sub = lineage.add_subparsers(dest="query_action")
    lineage_get = lineage_sub.add_parser("get", help="Walk lineage DAG")
    _add_common(lineage_get)
    lineage_get.add_argument("node_type", choices=("experiment", "run", "model"))
    lineage_get.add_argument("node_id")
    lineage_get.add_argument(
        "--direction",
        default="both",
        choices=("ancestors", "descendants", "both"),
    )
    lineage_get.add_argument("--depth", type=int, default=2)
    lineage_get.set_defaults(_extract_query="get_lineage")

    models = sub.add_parser("models", help="Read model registry from a store")
    models_sub = models.add_subparsers(dest="query_action")
    models_list = models_sub.add_parser("list", help="List registered models")
    _add_common(models_list)
    models_list.add_argument("--name-prefix", default="", help="Model name prefix")
    models_list.add_argument("--limit", type=int, default=50, help="Max rows (max 500)")
    models_list.set_defaults(_extract_query="list_models")


def _json_error(code: str, message: str) -> str:
    return json.dumps(
        {"error": {"code": code, "message": message}}, indent=2, sort_keys=True
    )


def _build_filters(args: argparse.Namespace) -> dict[str, str]:
    filters: dict[str, str] = {}
    for attr, key in (
        ("tag", "tag"),
        ("status", "status"),
        ("experiment_prefix", "experiment_prefix"),
        ("started_after", "started_after"),
        ("started_before", "started_before"),
    ):
        value = getattr(args, attr, None)
        if value is not None:
            filters[key] = value
    return filters


def _call_query(args: argparse.Namespace) -> Any:
    import extract.mcp as mcp_mod

    action = args._extract_query
    if action == "list_experiments":
        return mcp_mod.list_experiments(
            prefix=args.prefix,
            limit=args.limit,
            include_archived=args.include_archived,
        )
    if action == "list_runs":
        return mcp_mod.list_runs(
            experiment_id=args.experiment_id,
            limit=args.limit,
            include_archived=args.include_archived,
        )
    if action == "get_run":
        return mcp_mod.get_run(args.run_id)
    if action == "compare_runs":
        return mcp_mod.compare_runs(args.run_ids, include_curves=args.include_curves)
    if action == "search":
        return mcp_mod.search(
            query=args.query,
            filters=_build_filters(args),
            limit=args.limit,
            include_archived=args.include_archived,
        )
    if action == "list_todos":
        return mcp_mod.list_todos(
            scope_type=args.scope_type,
            scope_id=args.scope_id,
            include_done=args.include_done,
            limit=args.limit,
        )
    if action == "get_lineage":
        return mcp_mod.get_lineage(
            node_type=args.node_type,
            node_id=args.node_id,
            direction=args.direction,
            depth=args.depth,
        )
    if action == "list_models":
        return mcp_mod.list_models(name_prefix=args.name_prefix, limit=args.limit)
    raise RuntimeError(f"unknown query action: {action}")


def run_query_command(args: argparse.Namespace) -> int | None:
    """Run a query command if args selects one; else return None."""
    if not hasattr(args, "_extract_query"):
        return None

    store_path = Path(args.store)
    if not store_path.exists():
        print(
            _json_error(
                "store_not_found",
                f"store not found: {store_path} — run training with extract-tracker first, or pass --store",
            ),
            file=sys.stdout,
        )
        return 2

    import extract.mcp as mcp_mod

    store: Store | None = None
    try:
        store = Store(store_path)
        mcp_mod._store = store
        result = _call_query(args)
        print(json.dumps(result, indent=2, sort_keys=True))
        return 0
    except ValueError as exc:
        print(_json_error("invalid_request", str(exc)), file=sys.stdout)
        return 1
    except Exception as exc:  # pragma: no cover - defensive CLI boundary
        print(_json_error("internal_error", str(exc)), file=sys.stdout)
        return 70
    finally:
        mcp_mod._store = None
        if store is not None:
            store.close()
