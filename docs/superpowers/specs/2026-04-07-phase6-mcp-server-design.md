# Phase 6: MCP Server — Design Spec

## Overview

Expose Extract's read-only data model to LLM agents via an MCP (Model Context Protocol) server. Agents running in Claude Code, Claude Desktop, or any MCP-capable host can inspect experiments, runs, metrics, configs, lineage, models, and TODOs from a project's `.extract/` store using a fixed set of 8 tools.

**Scope:** read-only in v1. No tools that mutate the store (no `create_todo`, `log_metrics`, `tag_run`, etc.) — agents explore and reason; humans produce data. Write tools are deferred until the read surface is proven.

**Transport:** stdio only. `python -m extract.mcp` is launched as a subprocess by the MCP host and inherits its cwd, so the default `--store .extract` resolves to the host's workdir — launching `claude` in `/path/to/project` automatically connects to `/path/to/project/.extract`.

**Tech Stack:** Python 3.10+, `mcp>=1.0` (via the `[mcp]` optional extra already declared in `pyproject.toml:18`), SQLite via the existing `Store` class.

---

## 1. Architecture & Entry Point

### Module layout

**One new file:** `python/src/extract/mcp.py`. Single module, mirrors the existing `store.py` / `run.py` / `sync.py` pattern. Expected size ~400 lines.

No package-level `extract/mcp/` directory. If the file grows past ~400 lines we can split later, but preemptive splitting for 8 small tools is over-engineering.

### Dependency

`mcp>=1.0` is already listed in `[project.optional-dependencies]` as the `mcp` extra in `pyproject.toml:17-18`. Install with:

```
pip install 'extract-tracker[mcp]'
```

The module imports `mcp` at the top; if the import fails, `main()` catches `ImportError`, prints a clear install hint to stderr, and exits 1. The rest of the `extract` package never imports `extract.mcp`, so missing `mcp` never affects `extract tui` or the core SDK.

### Entry point

```
python -m extract.mcp [--store PATH]
```

Invoked by the module's `if __name__ == "__main__"` guard. Deliberately **not** wired into the existing `extract` CLI (`__main__.py`) — MCP servers are spawned by hosts, never invoked interactively, and keeping the entry points separate avoids pulling the optional `mcp` import into every `extract tui` invocation.

### CLI arguments

| Flag | Default | Description |
|---|---|---|
| `--store PATH` | `.extract` | Path to the `.extract/` directory. Relative paths resolve against the server's cwd (= MCP host's cwd). |

No other flags in v1. No `--transport`, no `--log-level`, no `--read-only` (it's always read-only). Keep the surface minimal.

### Store discovery behavior

Because `--store` defaults to the relative path `.extract`, the server connects to whichever store exists in its cwd:

1. User runs `claude` in `/path/to/MyProject`. Claude Code's process cwd is `/path/to/MyProject`.
2. Claude Code spawns the MCP server: `python -m extract.mcp`. The subprocess inherits its parent's cwd.
3. The server resolves `--store .extract` → `/path/to/MyProject/.extract`.

This holds for MCP servers registered globally (`~/.claude.json`) *and* for project-local (`.mcp.json`) registrations — both launch with the Claude host's cwd. Users who want a specific absolute path can pass `--store /abs/path` in their MCP config args.

### Store lifecycle

`main()` constructs one `Store(args.store)` instance before server startup and assigns it to a module-level `_store` variable that every tool function closes over. Single long-lived read-only `Store` is safe: SQLite WAL mode allows concurrent readers even while a training job is writing.

The `Store` is never closed — the process lifetime *is* the store lifetime. Shutdown happens when the MCP host disconnects stdin.

### Server shape

```python
# python/src/extract/mcp.py
from __future__ import annotations

import argparse
import sys
from pathlib import Path

try:
    from mcp.server.fastmcp import FastMCP
except ImportError:
    FastMCP = None  # handled in main()

from extract.store import Store

_store: Store | None = None
mcp_server = FastMCP("extract") if FastMCP else None

@mcp_server.tool()
def list_experiments(prefix: str = "", limit: int = 50) -> dict: ...

# ... 7 more tools ...

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
            f"store not found: {store_path} — run training with extract-tracker first, "
            "or pass --store",
            file=sys.stderr,
        )
        sys.exit(1)

    global _store
    try:
        _store = Store(store_path)
    except Exception as e:
        print(f"failed to open store: {e}", file=sys.stderr)
        sys.exit(1)

    mcp_server.run()  # stdio transport by default

if __name__ == "__main__":
    main()
```

All stdout is reserved for MCP protocol messages; every human-readable error goes to stderr.

---

## 2. Tool Schemas

Eight read-only tools. All `run_id` / `experiment_id` values are ULIDs — agents copy them from listing responses rather than trying to type them. Every listing tool shares the same response envelope (see §3).

### Shared conventions

- **Listing envelope:** `{items: [...], total: int, truncated: bool}`. `truncated = total > len(items)`.
- **Default `limit` = 50, hard cap = 500.** `limit > 500` silently clamps to 500 and adds `"limit_clamped": true` to the response (rationale: an agent that asked for 10000 shouldn't have to guess the cap to recover).
- **`label` field** on every run-returning row: `"{experiment_path}#{run_name}"`, or `"{experiment_path}#{id[:8]}"` if the run has no name.
- **Timestamps** are ISO 8601 strings, exactly as stored in the DB.

### 1. `list_experiments(prefix: str = "", limit: int = 50) -> dict`

Wraps `Store.list_experiments(prefix)` in `store.py:328`. Filters by optional path prefix.

Item shape:
```json
{
  "id": "...",
  "path": "cifar100/ewc/lambda_1.0",
  "name": "lambda_1.0",
  "node_type": "variant",
  "parent_id": "...",
  "n_runs": 3
}
```

`n_runs` is a per-experiment `COUNT(*)` query on `runs`. One extra round-trip per experiment, but experiment lists are typically small (tens, not thousands) and the count helps the agent decide which nodes actually have data.

### 2. `list_runs(experiment_id: str | None = None, limit: int = 50) -> dict`

If `experiment_id` is provided: runs for that experiment, ordered by `started_at`. If omitted: all runs in the store, ordered by `started_at DESC` (newest first).

Item shape:
```json
{
  "id": "01HZY...",
  "label": "cifar100/ewc/lambda_1.0#ewc-l1.0",
  "experiment_id": "...",
  "experiment_path": "cifar100/ewc/lambda_1.0",
  "name": "ewc-l1.0",
  "status": "completed",
  "started_at": "2026-04-05T12:34:56.789Z",
  "ended_at": "2026-04-05T13:22:01.456Z",
  "tags": ["sweep", "production-candidate"],
  "git_sha": "a1b2c3d...",
  "hostname": "gpu-node-03",
  "config_summary": {
    "n_keys": 12,
    "top_level_keys": ["lr", "lambda", "epochs", "method", "model"]
  }
}
```

`config_summary` is a cheap two-field summary, not lossy truncation — the full config is a `get_run(id)` call away. It exists because listing 50 runs with full configs would be wasteful of context tokens. All other fields are returned untruncated.

`n_keys` counts **top-level** keys only (nested dicts contribute 1), matching the semantics of `top_level_keys`. This gives the agent a quick "how structured is this config" signal without implying a flattened key count.

### 3. `get_run(run_id: str) -> dict`

Full run detail for one run. No listing envelope — returns the run dict directly.

Return shape:
```json
{
  "id": "...",
  "experiment_id": "...",
  "experiment_path": "cifar100/ewc/lambda_1.0",
  "name": "ewc-l1.0",
  "label": "cifar100/ewc/lambda_1.0#ewc-l1.0",
  "status": "completed",
  "started_at": "...",
  "ended_at": "...",
  "hostname": "...",
  "git_sha": "...",
  "tags": [...],
  "notes": "...",
  "config": { /* full parsed config dict */ },
  "metrics_final": {
    "loss": 0.321,
    "accuracy": 0.876
  },
  "metrics_available": ["loss", "accuracy", "lr"],
  "run_params": {
    "arch": "resnet18",
    "optimizer": "adam"
  },
  "artifacts": [
    {"name": "confusion_matrix", "kind": "matrix", "step": null,
     "rel_path": "artifacts/.../matrices/confusion_matrix.npy",
     "shape": [10, 10], "dtype": "float64"}
  ],
  "todos": [
    {"id": "...", "content": "Try lambda=0.5 next", "priority": 1,
     "done": 0, "created_at": "..."}
  ]
}
```

**Explicitly absent:** full metric histories. The last value per metric is in `metrics_final`; the list of available names is in `metrics_available`. Full histories are retrievable via `compare_runs([run_id], include_history=True)`, or we add a dedicated `get_metric_history` tool in v2 if demand is real. This keeps `get_run` payloads bounded regardless of run length.

### 4. `compare_runs(run_ids: list[str], include_history: bool = False) -> dict`

Takes 2–10 run IDs. Returns metrics with rankings, plus config diffs.

Return shape:
```json
{
  "runs": [
    {"id": "...", "label": "...", "experiment_path": "...", "status": "completed"}
  ],
  "metrics": {
    "loss": {
      "direction": "min",
      "values": {
        "01HZY...": 0.321,
        "01HZZ...": 0.289
      },
      "ranking": ["01HZZ...", "01HZY..."],
      "history": {
        "01HZY...": [[0, 2.3], [1, 1.8], [2, 1.5]],
        "01HZZ...": [[0, 2.4], [1, 1.7], [2, 1.4]]
      }
    }
  },
  "config_diffs": {
    "lr": {"01HZY...": 0.001, "01HZZ...": 0.005},
    "lambda": {"01HZY...": 1.0, "01HZZ...": 0.5}
  }
}
```

- `direction` comes from the shared `_metric_direction()` helper (§3).
- `ranking` is the run IDs sorted best-to-worst by final value according to `direction`.
- `values` contains the last recorded value per metric per run.
- `history` is only present when `include_history=True`; its value is a list of `[step, value]` pairs per run.
- `config_diffs` uses the flat dot-notation key format (§3) and only emits keys where at least two runs have different values. Identical keys are omitted.

**Constraints:**
- `len(run_ids) < 2` → `ValueError("compare_runs requires at least 2 run_ids")`
- `len(run_ids) > 10` → `ValueError("compare_runs supports at most 10 runs per call (got N)")`
- Any unknown run_id → `ValueError(f"Run not found: {run_id}")`

### 5. `search(query: str = "", filters: dict | None = None, limit: int = 50) -> dict`

Substring + filter search over runs. Same item shape as `list_runs`.

**`query`:** case-insensitive `LIKE '%query%'` against `runs.name`, `runs.tags`, `runs.notes`. Empty string → no text filter.

**`filters`** (all optional, all AND-combined):

| Key | Type | Semantics |
|---|---|---|
| `tag` | `str` | Run's `tags` JSON contains this tag |
| `status` | `"running" \| "completed" \| "failed"` | Exact status match |
| `experiment_prefix` | `str` | Run's `experiment_path` starts with this |
| `started_after` | ISO 8601 `str` | `runs.started_at >= value` |
| `started_before` | ISO 8601 `str` | `runs.started_at <= value` |

Empty `query` + empty `filters` is valid and equivalent to "list most recent N runs." Unknown filter keys → `ValueError` that lists the valid set.

### 6. `list_todos(scope_type: str = "global", scope_id: str | None = None, include_done: bool = False) -> dict`

Wraps `Store.list_todos()`. `scope_type ∈ {"global", "experiment", "run"}`. When `scope_type != "global"`, `scope_id` is required (the experiment ID or run ID to scope to); when `scope_type == "global"`, `scope_id` must be None.

`include_done=False` (default) filters to `done = 0`.

Item shape:
```json
{
  "id": "...",
  "scope_type": "run",
  "scope_id": "...",
  "content": "Try lambda=0.5 next",
  "priority": 1,
  "done": 0,
  "created_at": "...",
  "completed_at": null
}
```

Ordered by `priority DESC, created_at ASC`, matching the existing Python SDK behavior.

### 7. `get_lineage(node_type: str, node_id: str, direction: str = "both", depth: int = 2) -> dict`

Walks the `lineage` table as a DAG.

- `node_type ∈ {"experiment", "run", "model"}`
- `direction ∈ {"ancestors", "descendants", "both"}`
- `depth` capped at `[1, 5]`, default 2

Flat node + edge representation (nested trees choke on diamonds):

```json
{
  "root": {"type": "run", "id": "...", "label": "cifar100/ewc#ewc-l1.0"},
  "nodes": [
    {"type": "model", "id": "...", "label": "baseline-cifar100@v0.9"},
    {"type": "run", "id": "...", "label": "cifar100/ewc#ewc-l0.5"}
  ],
  "edges": [
    {"parent_type": "model", "parent_id": "...",
     "child_type": "run", "child_id": "...", "relation": "derived_from"}
  ]
}
```

Label format by type:
- `run`: `{experiment_path}#{name or id[:8]}`
- `experiment`: `{path}`
- `model`: `{name}@{version}`

Algorithm: BFS from the root up to `depth` hops, in the requested direction(s), collecting visited nodes and traversed edges. Deduplicate nodes by `(type, id)`.

### 8. `list_models(name_prefix: str = "", limit: int = 50) -> dict`

Optional name filter. Ordered by `created_at DESC`.

Item shape:
```json
{
  "id": "...",
  "name": "ewc-cifar100",
  "version": "1.0",
  "run_id": "...",
  "framework": "pytorch",
  "artifact_path": "/abs/path/to/models/ewc-cifar100/1.0/model.pt",
  "metadata": {"params": 11200000, "task": "cifar100"},
  "created_at": "..."
}
```

---

## 3. Shared Helpers

All helpers live at the top of `mcp.py` as private functions. None are exported.

### `_row_to_dict(row)`
Converts a `sqlite3.Row` to a plain `dict`, parsing JSON string columns into Python values. Only parses columns that are actually present on the row (via `row.keys()`):
- `tags` (TEXT) → `list[str]` (`[]` for NULL)
- `config` (TEXT) → `dict` (`{}` for NULL)
- `metadata` (TEXT) → `dict | None`
- Timestamps stay as strings (no datetime parsing — agents can parse if needed, and keeping the DB representation avoids a serialization round-trip)

### `_label(experiment_path: str, run_name: str | None, run_id: str) -> str`
```python
def _label(experiment_path: str, run_name: str | None, run_id: str) -> str:
    tail = run_name if run_name else run_id[:8]
    return f"{experiment_path}#{tail}"
```
Every tool returning a run uses this. Single source of truth for the human-readable anchor.

### `_flatten_config(config: dict, prefix: str = "") -> dict`
Flattens nested dicts into dot-notation keys:
```python
{"method": {"lora_r": 8, "lora_alpha": 16}}
# becomes
{"method.lora_r": 8, "method.lora_alpha": 16}
```
Matches the dot-notation the TUI's compare/diff view already uses (DOC.md:364). Consistency across TUI and MCP keeps mental models aligned.

**Lists are not flattened.** `{"layers": [64, 128, 256]}` becomes `{"layers": [64, 128, 256]}`, not `{"layers.0": 64, ...}`. Lists are leaf values for the purposes of diffing, matching TUI behavior.

### `_metric_direction(name: str) -> str`
Returns `"min"` or `"max"` based on substring match against the metric name (lowercased):

Minimize patterns (identical to DOC.md:461): `loss`, `error`, `perplexity`, `mse`, `mae`, `rmse`, `nll`, `cer`, `wer`, `fid`, `divergence`. Everything else returns `"max"`.

**Explicit non-goal for v1:** reading `[metrics]` overrides from `config.toml`. The TUI reads them; the MCP server won't, yet. Rationale: adding TOML parsing to the MCP module pulls `tomllib` in for one feature that's trivial to add in v2 once we know which tools actually need config-driven direction. The hardcoded heuristics cover the common case.

### `_config_diffs(runs_configs: list[tuple[str, dict]]) -> dict`
Takes a list of `(run_id, config_dict)` pairs (configs already parsed from JSON). Flattens each config, takes the union of keys, and returns only keys where at least two runs have different values, keyed directly by `run_id`:
```python
def _config_diffs(runs_configs: list[tuple[str, dict]]) -> dict:
    """Returns {flat_key: {run_id: value}} for keys where values differ across runs."""
```
Keyed by run_id directly so `compare_runs` can return the result without a second pass. Missing keys (when one run has a key another doesn't) are included in the result and show up only under the runs that have them.

### `_listing(items: list, total: int, limit: int, limit_clamped: bool = False) -> dict`
```python
def _listing(items, total, limit, limit_clamped=False):
    result = {"items": items[:limit], "total": total, "truncated": total > limit}
    if limit_clamped:
        result["limit_clamped"] = True
    return result
```
Every listing tool calls this.

### `_clamp_limit(limit: int) -> tuple[int, bool]`
```python
def _clamp_limit(limit: int) -> tuple[int, bool]:
    if limit > 500:
        return 500, True
    return limit, False
```
One place that implements the silent clamp.

---

## 4. Error Handling

### Propagation model

All tool-visible errors raise `ValueError`. FastMCP automatically converts raised exceptions into MCP `isError: true` responses with the exception's string form as the error text. No custom exception types, no error-code scheme — just strings crafted to be actionable.

Unexpected exceptions (bugs) propagate as-is. FastMCP reports them as internal errors, which surfaces the stack trace in the host — appropriate for development, and the agent sees enough to know "something is broken, don't retry blindly."

### Validation pattern

Every tool opens with cheap validation before touching the DB:

```python
@mcp_server.tool()
def get_run(run_id: str) -> dict:
    if not run_id:
        raise ValueError("run_id is required")
    row = _store._conn.execute(
        "SELECT ... FROM runs WHERE id = ?", (run_id,)
    ).fetchone()
    if row is None:
        raise ValueError(f"Run not found: {run_id!r}")
    # ... build response ...
```

### Error message catalog

Every error message follows the same pattern: **name what was wrong, show the offending value, list valid options when the valid set is small and fixed**.

| Condition | Tool(s) | Message |
|---|---|---|
| Empty `run_id` | get_run, compare_runs, get_lineage | `"run_id is required"` |
| Unknown run_id | get_run, compare_runs, get_lineage | `f"Run not found: {run_id!r}"` |
| Empty `experiment_id` | list_runs, get_lineage | `"experiment_id is required"` |
| Unknown experiment_id | list_runs, get_lineage | `f"Experiment not found: {experiment_id!r}"` |
| `compare_runs` < 2 ids | compare_runs | `f"compare_runs requires at least 2 run_ids (got {len(ids)})"` |
| `compare_runs` > 10 ids | compare_runs | `f"compare_runs supports at most 10 runs per call (got {len(ids)})"` |
| Invalid `scope_type` | list_todos | `f"scope_type must be one of: global, experiment, run (got {val!r})"` |
| `scope_id` required but missing | list_todos | `f"scope_id is required when scope_type={scope_type!r}"` |
| `scope_id` given for global scope | list_todos | `"scope_id must be None when scope_type='global'"` |
| Invalid `status` filter | search | `f"status must be one of: running, completed, failed (got {val!r})"` |
| Invalid `direction` | get_lineage | `f"direction must be one of: ancestors, descendants, both (got {val!r})"` |
| Invalid `node_type` | get_lineage | `f"node_type must be one of: experiment, run, model (got {val!r})"` |
| Depth out of range | get_lineage | `f"depth must be between 1 and 5 (got {depth})"` |
| Unknown filter key | search | `f"Unknown filter: {key!r}. Valid filters: tag, status, experiment_prefix, started_after, started_before"` |
| Malformed ISO 8601 | search | `f"{field} must be ISO 8601 (got {val!r})"` |
| Negative `limit` | all listings | `f"limit must be >= 1 (got {limit})"` |

### Clamping vs erroring

`limit > 500` silently clamps and adds `"limit_clamped": true` to the response. Rationale: if the agent asked for 10000 and we raised, it now has to guess the cap to recover. Silent clamp + a flag is lower-friction, discoverable, and non-destructive.

### Startup errors

Three failure modes at `main()` time, before accepting MCP calls:

1. **Missing `mcp` package.** Top-level `try: from mcp.server.fastmcp import FastMCP except ImportError: FastMCP = None`. `main()` checks `FastMCP is None`, prints `"extract-tracker[mcp] extra not installed. Install with: pip install 'extract-tracker[mcp]'"` to stderr, exits 1.
2. **Store path doesn't exist.** `Store()`'s default is to *create* a missing directory, which would silently produce an empty store. We don't want that here. `main()` checks `Path(args.store).exists()` first and exits 1 with `f"store not found: {store_path} — run training with extract-tracker first, or pass --store"`.
3. **DB open / migration failure.** `Store()` raises; we catch, print `f"failed to open store: {e}"` to stderr, exit 1.

All startup errors go to stderr with exit code 1. Stdout is reserved for MCP protocol frames.

### Explicit non-goals

- **No logging framework.** If we need it later, add `logging` calls then.
- **No retry logic.** Read-only SQLite over WAL doesn't block on writers; transient failures aren't a real failure mode.
- **No request-id tracing.** Single-client local servers don't need it.

---

## 5. Testing

### Strategy

**Primary: unit tests against the tool functions directly.** FastMCP-decorated functions are still regular Python functions — we call them as functions with a fixture-built `_store` and assert on the returned dicts. No subprocess, no stdio round-trip, no MCP client library. Fast and follows the same pattern as `python/tests/test_hierarchy.py`.

**Secondary: one stdio smoke test.** Spawns `python -m extract.mcp --store <tmp>` as a subprocess, connects with `mcp.client.stdio`, calls `list_experiments` once, confirms non-empty response, shuts down. Covers the entry point, arg parsing, Store open, FastMCP stdio transport, and tool dispatch in one test. Skipped (`pytest.skip`) if the `mcp` package isn't importable in the test environment.

### New file

`python/tests/test_mcp.py`. One file. Fixtures at the top, one `Test*` class per tool, plus `TestServerSmoke`.

### Fixture

```python
@pytest.fixture
def populated_store(tmp_path, monkeypatch):
    """Temp store with realistic data, with extract.mcp._store pointed at it."""
    store = Store(tmp_path / ".extract")
    # ... build hierarchy, 2-3 experiments, 3-4 runs with metrics/tags/notes/configs,
    #     a TODO, a model, a lineage edge — enough diversity to exercise every tool
    import extract.mcp as mcp_mod
    monkeypatch.setattr(mcp_mod, "_store", store)
    yield store
```

One fixture reused by every unit test. Keeps setup cost low and isolates breakage.

### Per-tool coverage

| Tool | Cases |
|---|---|
| `list_experiments` | no-prefix, prefix filter, `n_runs` correctness |
| `list_runs` | by experiment, all-runs, `config_summary` shape, `label` format |
| `get_run` | happy path, unknown id raises, full config round-trip, `metrics_final` correctness, `metrics_available` matches, artifacts present |
| `compare_runs` | 2 runs, 3 runs, `include_history=True` adds history, `config_diffs` only has differing keys, ranking correctness for both directions, < 2 ids errors, > 10 ids errors |
| `search` | `query` substring, `tag` filter, `status` filter, `experiment_prefix` filter, AND semantics across filters, unknown filter key errors |
| `list_todos` | global scope, run scope, `include_done=True` shows completed, `scope_id` validation |
| `get_lineage` | ancestors, descendants, both, depth cap enforced, DAG diamond (two paths → one node), unknown root errors |
| `list_models` | no prefix, `name_prefix` filter, full payload shape |

### Cross-cutting tests

- **`TestListingEnvelope`** — one parametrized test across all listing tools. Calls with `limit=1` on a store that has >1 matching rows; asserts `{items: [_], total: N, truncated: True}` with `N > 1`.
- **`TestLimitClamp`** — one test passing `limit=1000`; asserts `limit_clamped=True` and `len(items) <= 500`.
- **`TestLabelStability`** — one test asserting the same run's `label` is byte-identical across `list_runs`, `get_run`, `compare_runs`, and `search`. Regression protection for the `_label()` helper.
- **`TestErrorMessages`** — parametrized test covering every row in §4's error message catalog. Calls each tool with the bad input, asserts `ValueError` with the expected text. Cheap insurance that refactors don't mangle agent-facing strings.

### Smoke test

```python
class TestServerSmoke:
    def test_server_boots_and_lists_experiments(self, tmp_path):
        # Build populated store on disk (without fixture monkeypatch,
        # since we're spawning a subprocess that reads its own)
        store = Store(tmp_path / ".extract")
        # ... minimal fixture: one experiment, one run ...
        store.close()

        # Spawn `python -m extract.mcp --store <tmp_path/.extract>`
        # Connect via mcp.client.stdio
        # Call list_experiments()
        # Assert non-empty items list
        # Close cleanly
```

One test. If it passes, every layer from entry point down to tool dispatch works end-to-end.

### Running tests

```
nix develop
pytest python/tests/test_mcp.py
```

Per `CLAUDE.md`, dependencies are managed via `flake.nix` + `nix develop`. The smoke test requires the `mcp` package; if unavailable, it `pytest.skip`s with a clear reason and the unit tests still run (because unit tests monkey-patch `_store` and don't import `mcp.server.fastmcp` directly — the decorated functions are still just Python functions after import).

**Caveat:** if `mcp` is not installed at all, `import extract.mcp` itself fails, and all tests in the file skip. That's expected and correct.

### What we're not testing

- **MCP protocol compliance beyond the smoke test.** FastMCP is a tested dependency.
- **Concurrent read + TUI/trainer write.** SQLite WAL handles it; integration tests against the Rust side are out of scope for this phase.
- **Installation of the `[mcp]` extra on fresh envs.** Packaging concern, not a code concern.

---

## Summary

- **One new file:** `python/src/extract/mcp.py` (~400 lines)
- **One new test file:** `python/tests/test_mcp.py`
- **Zero changes** to `store.py`, `experiment.py`, `run.py`, `sync.py`, or the Rust TUI.
- **No changes** to the existing `extract` CLI; MCP lives at its own `python -m extract.mcp` entry point.
- **Dependency:** `mcp>=1.0` via the pre-existing `[mcp]` optional extra.
- **Surface:** 8 read-only tools, stdio transport, `--store PATH` default `.extract` (relative to MCP host's cwd).
