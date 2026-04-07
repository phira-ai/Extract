# Phase 6: MCP Server Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a read-only MCP server (`python -m extract.mcp`) that exposes 8 tools for LLM agents to inspect a `.extract/` store — experiments, runs, metrics, configs, lineage, models, and TODOs.

**Architecture:** Single-file Python module `python/src/extract/mcp.py` using `FastMCP` from the `mcp>=1.0` package (declared in the existing `[mcp]` optional extra). Stdio transport only. `--store` defaults to `.extract` relative to the server's cwd, so launching `claude` in a project folder automatically binds to that project's store. Tools are decorated Python functions that can be called directly by unit tests (monkey-patching `_store`); one stdio smoke test covers the end-to-end path.

**Tech Stack:** Python 3.10+, `mcp>=1.0`, `sqlite3` (via existing `Store`), `pytest`

**Spec:** `docs/superpowers/specs/2026-04-07-phase6-mcp-server-design.md`

---

## File Map

| Action | File | Responsibility |
|---|---|---|
| Create | `python/src/extract/mcp.py` | Single-module MCP server: FastMCP wiring, 8 tools, shared helpers, `main()` entry point |
| Create | `python/tests/test_mcp.py` | Unit tests (per-tool + cross-cutting) and one stdio smoke test |
| Modify | `PLAN.md` | Strike Phase 6 off the future work list |

No changes to `store.py`, `experiment.py`, `run.py`, `sync.py`, the Rust TUI, or the existing `extract` CLI.

---

## Conventions Used Throughout This Plan

- **Test first, then implementation, then commit.** Each task follows TDD: write a failing test, run it to confirm the failure mode, implement the minimum code to pass, run the tests, commit.
- **Run commands under `nix develop`.** Per `CLAUDE.md`, dependencies are managed via `flake.nix`. Every test invocation assumes you're inside `nix develop`.
- **Test command:** `pytest python/tests/test_mcp.py -v` (full file) or `pytest python/tests/test_mcp.py::TestName::test_method -v` (single test).
- **Module import guard.** The MCP module catches `ImportError` on `from mcp.server.fastmcp import FastMCP` and sets `FastMCP = None`. This lets the test file import `extract.mcp` and call tool functions directly even if `mcp` isn't installed; only the smoke test requires `mcp` to actually be present.
- **`_store` is a module-level variable** set by `main()` at startup. Tests monkey-patch it on the `extract.mcp` module, not via fixtures that take the tool functions as arguments.
- **Commit messages** follow the existing style (`feat: short description`, `test: ...`). Commits are small and scoped to one task.

---

## Task 1: Bootstrap Module and Test File

**Goal:** Create the skeleton — module loads, test file imports it, one trivial test passes. No tools yet.

**Files:**
- Create: `python/src/extract/mcp.py`
- Create: `python/tests/test_mcp.py`

- [ ] **Step 1: Create the minimal module**

Create `python/src/extract/mcp.py`:

```python
"""Extract MCP server: read-only tool surface for LLM agents.

Invoked as `python -m extract.mcp [--store PATH]`. Runs FastMCP over
stdio by default. See docs/superpowers/specs/2026-04-07-phase6-mcp-server-design.md
for the full design.
"""

from __future__ import annotations

import argparse
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
```

- [ ] **Step 2: Create the test file with the fixture**

Create `python/tests/test_mcp.py`:

```python
"""Tests for the MCP server tool surface."""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
from pathlib import Path

import pytest

import extract
import extract.mcp as mcp_mod


@pytest.fixture
def populated_store(tmp_path, monkeypatch):
    """Temp store with realistic data across experiments, runs, metrics,
    tags, notes, configs, todos, a model, and lineage.

    Also sets extract.mcp._store to point at this store so tool functions
    invoked during the test read from it.
    """
    root = tmp_path / ".extract"
    root.mkdir(parents=True)
    # Write config.toml so the hierarchy is registered.
    (root / "config.toml").write_text(
        '[store]\nhierarchy = "benchmark > method > variant"\n'
    )
    store = extract.Store(root=root)

    # --- Experiment 1: cifar100/ewc/lambda_1.0, two runs ---
    exp1 = store.experiment(
        {"benchmark": "cifar100", "method": "ewc", "variant": "lambda_1.0"}
    )
    with exp1.run(
        config={"lr": 0.001, "lambda": 1.0, "method": {"fisher": "diagonal"}},
        name="ewc-l1.0-a",
    ) as r1a:
        for step in range(5):
            r1a.log(step=step, loss=1.0 - 0.15 * step, accuracy=0.5 + 0.08 * step)
        r1a.log(step=0, arch="resnet18")
        r1a.tag("sweep", "production-candidate")
        r1a.note("Best lambda in sweep.")
        r1a.todo("Try lambda=0.5 next", priority=1)
    r1a_id = r1a.id

    with exp1.run(
        config={"lr": 0.0005, "lambda": 1.0, "method": {"fisher": "empirical"}},
        name="ewc-l1.0-b",
    ) as r1b:
        for step in range(5):
            r1b.log(step=step, loss=0.9 - 0.12 * step, accuracy=0.55 + 0.07 * step)
        r1b.tag("sweep")
    r1b_id = r1b.id

    # --- Experiment 2: cifar100/si/variant_a, one run ---
    exp2 = store.experiment(
        {"benchmark": "cifar100", "method": "si", "variant": "variant_a"}
    )
    with exp2.run(config={"lr": 0.01, "c": 0.1}, name="si-a") as r2:
        for step in range(5):
            r2.log(step=step, loss=1.2 - 0.10 * step, accuracy=0.45 + 0.09 * step)
        r2.tag("baseline")
    r2_id = r2.id

    # --- A model derived from r1a ---
    import shutil as _sh
    dummy_model = tmp_path / "dummy_model.pt"
    dummy_model.write_bytes(b"fake model bytes")
    # Re-open the run via a direct insert since run.register_model requires
    # an active run and we closed them above. Use the SDK via a throwaway
    # run... but simpler: insert directly.
    from ulid import ULID as _ULID
    model_id = str(_ULID())
    models_dir = root / "models" / "ewc-cifar100" / "1.0"
    models_dir.mkdir(parents=True, exist_ok=True)
    _sh.copy(dummy_model, models_dir / "dummy_model.pt")
    store._conn.execute(
        "INSERT INTO models (id, name, version, run_id, artifact_path, "
        "framework, metadata) VALUES (?, ?, ?, ?, ?, ?, ?)",
        (model_id, "ewc-cifar100", "1.0", r1a_id,
         str(models_dir / "dummy_model.pt"), "pytorch",
         json.dumps({"params": 1000})),
    )
    # Lineage: model -> r1b (r1b derived_from this model)
    store._conn.execute(
        "INSERT INTO lineage (parent_type, parent_id, child_type, child_id, relation) "
        "VALUES ('model', ?, 'run', ?, 'derived_from')",
        (model_id, r1b_id),
    )
    # Lineage: r1a -> r2 (r2 branched_from r1a)
    store._conn.execute(
        "INSERT INTO lineage (parent_type, parent_id, child_type, child_id, relation) "
        "VALUES ('run', ?, 'run', ?, 'branched_from')",
        (r1a_id, r2_id),
    )
    store._conn.commit()

    # A global todo
    store.todo("Write up results for paper", priority=2)

    monkeypatch.setattr(mcp_mod, "_store", store)

    # Expose useful IDs on the fixture for tests to reference.
    store.test_ids = {  # type: ignore[attr-defined]
        "r1a": r1a_id,
        "r1b": r1b_id,
        "r2": r2_id,
        "exp1": exp1.id,
        "exp2": exp2.id,
        "model": model_id,
    }
    yield store
    store.close()


class TestModuleLoads:
    def test_module_importable(self):
        import extract.mcp  # noqa: F401
```

- [ ] **Step 3: Run the trivial test**

```
pytest python/tests/test_mcp.py::TestModuleLoads -v
```

Expected: `TestModuleLoads::test_module_importable PASSED`.

- [ ] **Step 4: Verify the fixture works**

Add a temporary test to verify the fixture builds without errors:

```python
class TestFixture:
    def test_fixture_builds(self, populated_store):
        assert populated_store.test_ids["r1a"]
        assert populated_store.test_ids["r1b"]
        assert populated_store.test_ids["r2"]
        assert populated_store.test_ids["exp1"]
        assert populated_store.test_ids["exp2"]
```

Run:
```
pytest python/tests/test_mcp.py::TestFixture -v
```

Expected: PASSED. (If it fails, inspect the traceback — the fixture has to work before any tool test can run.)

- [ ] **Step 5: Commit**

```
git add python/src/extract/mcp.py python/tests/test_mcp.py
git commit -m "feat: bootstrap extract.mcp module and test fixture"
```

---

## Task 2: Shared Helpers

**Goal:** Implement and test the 7 pure helpers from spec §3. These have no DB dependencies and can be tested in isolation.

**Files:**
- Modify: `python/src/extract/mcp.py` (add helpers before the `main()` function)
- Modify: `python/tests/test_mcp.py` (add `TestHelpers` class)

- [ ] **Step 1: Write failing tests for all helpers**

Add to `python/tests/test_mcp.py`:

```python
class TestHelpers:
    def test_label_with_name(self):
        from extract.mcp import _label
        assert _label("a/b", "my-run", "01HZY" + "0" * 22) == "a/b#my-run"

    def test_label_without_name(self):
        from extract.mcp import _label
        assert _label("a/b", None, "01HZYABCDEF1234567890ABCD") == "a/b#01HZYABC"

    def test_label_empty_name(self):
        from extract.mcp import _label
        # Empty string should be treated same as None (fall back to id prefix).
        assert _label("a/b", "", "01HZYABCDEF1234567890ABCD") == "a/b#01HZYABC"

    def test_flatten_config_flat(self):
        from extract.mcp import _flatten_config
        assert _flatten_config({"lr": 0.001, "epochs": 10}) == {"lr": 0.001, "epochs": 10}

    def test_flatten_config_nested(self):
        from extract.mcp import _flatten_config
        assert _flatten_config({"method": {"lora_r": 8, "lora_alpha": 16}}) == {
            "method.lora_r": 8,
            "method.lora_alpha": 16,
        }

    def test_flatten_config_deep(self):
        from extract.mcp import _flatten_config
        assert _flatten_config({"a": {"b": {"c": 1}}}) == {"a.b.c": 1}

    def test_flatten_config_lists_not_flattened(self):
        from extract.mcp import _flatten_config
        # Lists are leaf values, per spec §3.
        assert _flatten_config({"layers": [64, 128, 256]}) == {"layers": [64, 128, 256]}

    def test_metric_direction_loss(self):
        from extract.mcp import _metric_direction
        assert _metric_direction("loss") == "min"
        assert _metric_direction("train_loss") == "min"
        assert _metric_direction("MSE") == "min"
        assert _metric_direction("perplexity") == "min"

    def test_metric_direction_default_max(self):
        from extract.mcp import _metric_direction
        assert _metric_direction("accuracy") == "max"
        assert _metric_direction("f1_score") == "max"
        assert _metric_direction("unknown_metric") == "max"

    def test_config_diffs_identical(self):
        from extract.mcp import _config_diffs
        pairs = [("r1", {"lr": 0.001}), ("r2", {"lr": 0.001})]
        assert _config_diffs(pairs) == {}

    def test_config_diffs_one_differ(self):
        from extract.mcp import _config_diffs
        pairs = [("r1", {"lr": 0.001, "epochs": 10}), ("r2", {"lr": 0.005, "epochs": 10})]
        assert _config_diffs(pairs) == {"lr": {"r1": 0.001, "r2": 0.005}}

    def test_config_diffs_missing_key(self):
        from extract.mcp import _config_diffs
        # One run has a key the other doesn't — that's a difference.
        pairs = [("r1", {"lr": 0.001, "lambda": 1.0}), ("r2", {"lr": 0.001})]
        result = _config_diffs(pairs)
        assert result == {"lambda": {"r1": 1.0}}

    def test_config_diffs_nested(self):
        from extract.mcp import _config_diffs
        pairs = [
            ("r1", {"method": {"fisher": "diagonal"}}),
            ("r2", {"method": {"fisher": "empirical"}}),
        ]
        assert _config_diffs(pairs) == {
            "method.fisher": {"r1": "diagonal", "r2": "empirical"}
        }

    def test_listing_not_truncated(self):
        from extract.mcp import _listing
        items = [1, 2, 3]
        assert _listing(items, total=3, limit=50) == {
            "items": [1, 2, 3],
            "total": 3,
            "truncated": False,
        }

    def test_listing_truncated(self):
        from extract.mcp import _listing
        items = list(range(10))
        result = _listing(items, total=10, limit=5)
        assert result["items"] == [0, 1, 2, 3, 4]
        assert result["total"] == 10
        assert result["truncated"] is True

    def test_listing_with_clamped(self):
        from extract.mcp import _listing
        result = _listing([1, 2], total=2, limit=500, limit_clamped=True)
        assert result["limit_clamped"] is True

    def test_clamp_limit_under(self):
        from extract.mcp import _clamp_limit
        assert _clamp_limit(50) == (50, False)
        assert _clamp_limit(500) == (500, False)

    def test_clamp_limit_over(self):
        from extract.mcp import _clamp_limit
        assert _clamp_limit(1000) == (500, True)

    def test_row_to_dict_parses_tags(self):
        from extract.mcp import _row_to_dict
        import sqlite3
        conn = sqlite3.connect(":memory:")
        conn.row_factory = sqlite3.Row
        conn.execute("CREATE TABLE t (id TEXT, tags TEXT, config TEXT, metadata TEXT)")
        conn.execute(
            "INSERT INTO t VALUES (?, ?, ?, ?)",
            ("x", '["a", "b"]', '{"lr": 0.001}', None),
        )
        row = conn.execute("SELECT * FROM t").fetchone()
        d = _row_to_dict(row)
        assert d["tags"] == ["a", "b"]
        assert d["config"] == {"lr": 0.001}
        assert d["metadata"] is None

    def test_row_to_dict_null_tags(self):
        from extract.mcp import _row_to_dict
        import sqlite3
        conn = sqlite3.connect(":memory:")
        conn.row_factory = sqlite3.Row
        conn.execute("CREATE TABLE t (id TEXT, tags TEXT, config TEXT)")
        conn.execute("INSERT INTO t VALUES (?, ?, ?)", ("x", None, None))
        row = conn.execute("SELECT * FROM t").fetchone()
        d = _row_to_dict(row)
        assert d["tags"] == []
        assert d["config"] == {}

    def test_row_to_dict_no_json_columns(self):
        from extract.mcp import _row_to_dict
        import sqlite3
        conn = sqlite3.connect(":memory:")
        conn.row_factory = sqlite3.Row
        conn.execute("CREATE TABLE t (id TEXT, name TEXT)")
        conn.execute("INSERT INTO t VALUES (?, ?)", ("x", "hello"))
        row = conn.execute("SELECT * FROM t").fetchone()
        d = _row_to_dict(row)
        assert d == {"id": "x", "name": "hello"}
```

- [ ] **Step 2: Run tests to verify they fail**

```
pytest python/tests/test_mcp.py::TestHelpers -v
```

Expected: all tests fail with `ImportError` (helpers don't exist yet).

- [ ] **Step 3: Implement the helpers in `mcp.py`**

Insert the following block between the `_tool` function and `def main(...)` in `python/src/extract/mcp.py`:

```python
# ----------------------------------------------------------------------
# Shared helpers (pure, no DB access)
# ----------------------------------------------------------------------

_MIN_METRIC_PATTERNS = (
    "loss", "error", "perplexity", "mse", "mae", "rmse",
    "nll", "cer", "wer", "fid", "divergence",
)


def _row_to_dict(row) -> dict:
    """Convert a sqlite3.Row to a plain dict, parsing JSON columns."""
    import json as _json
    d: dict = {}
    for key in row.keys():
        val = row[key]
        if key == "tags":
            d[key] = _json.loads(val) if val else []
        elif key == "config":
            d[key] = _json.loads(val) if val else {}
        elif key == "metadata":
            d[key] = _json.loads(val) if val else None
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
    """Clamp limit to the hard cap of 500, returning (clamped, was_clamped)."""
    if limit > 500:
        return 500, True
    return limit, False


def _listing(
    items: list,
    total: int,
    limit: int,
    limit_clamped: bool = False,
) -> dict:
    """Wrap a list in the shared listing envelope."""
    result: dict = {
        "items": items[:limit],
        "total": total,
        "truncated": total > limit,
    }
    if limit_clamped:
        result["limit_clamped"] = True
    return result
```

Note: `_config_diffs` uses `(type(v).__name__, repr(v))` for the distinct comparison. This avoids issues with unhashable values (like lists) and distinguishes between, say, `1` (int) and `"1"` (str) — which is the correct behavior for config diffing.

- [ ] **Step 4: Run tests to verify they pass**

```
pytest python/tests/test_mcp.py::TestHelpers -v
```

Expected: all ~20 helper tests pass.

- [ ] **Step 5: Commit**

```
git add python/src/extract/mcp.py python/tests/test_mcp.py
git commit -m "feat: shared helpers for extract.mcp"
```

---

## Task 3: Tool — `list_experiments`

**Goal:** First tool. Establishes the listing pattern. Lists experiments optionally filtered by prefix, with `n_runs` per row.

**Files:**
- Modify: `python/src/extract/mcp.py` (add `list_experiments` function)
- Modify: `python/tests/test_mcp.py` (add `TestListExperiments` class)

- [ ] **Step 1: Write failing tests**

Add to `test_mcp.py`:

```python
class TestListExperiments:
    def test_lists_all_experiments(self, populated_store):
        result = mcp_mod.list_experiments()
        assert "items" in result
        assert result["total"] >= 5  # root, benchmark, method, variant levels
        paths = [item["path"] for item in result["items"]]
        assert "cifar100" in paths
        assert "cifar100/ewc/lambda_1.0" in paths
        assert "cifar100/si/variant_a" in paths

    def test_prefix_filter(self, populated_store):
        result = mcp_mod.list_experiments(prefix="cifar100/ewc")
        paths = [item["path"] for item in result["items"]]
        assert "cifar100/ewc" in paths
        assert "cifar100/ewc/lambda_1.0" in paths
        assert "cifar100/si/variant_a" not in paths

    def test_n_runs_populated(self, populated_store):
        result = mcp_mod.list_experiments(prefix="cifar100/ewc/lambda_1.0")
        # The leaf has 2 runs (r1a, r1b).
        leaf = next(i for i in result["items"] if i["path"] == "cifar100/ewc/lambda_1.0")
        assert leaf["n_runs"] == 2
        # Branch nodes have 0 runs of their own.
        branch = next(i for i in result["items"] if i["path"] == "cifar100/ewc")
        assert branch["n_runs"] == 0

    def test_item_shape(self, populated_store):
        result = mcp_mod.list_experiments(prefix="cifar100/ewc/lambda_1.0")
        item = result["items"][0]
        assert set(item.keys()) == {"id", "path", "name", "node_type", "parent_id", "n_runs"}

    def test_listing_envelope(self, populated_store):
        result = mcp_mod.list_experiments(limit=1)
        assert len(result["items"]) == 1
        assert result["total"] >= 1
        assert result["truncated"] is True
```

- [ ] **Step 2: Run tests to verify they fail**

```
pytest python/tests/test_mcp.py::TestListExperiments -v
```

Expected: all tests fail with `AttributeError: module 'extract.mcp' has no attribute 'list_experiments'`.

- [ ] **Step 3: Implement `list_experiments`**

Insert this block into `python/src/extract/mcp.py` below the helpers section. (We'll add a `# Tools` comment header to group them.)

```python
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
```

- [ ] **Step 4: Run tests to verify they pass**

```
pytest python/tests/test_mcp.py::TestListExperiments -v
```

Expected: all 5 tests pass.

- [ ] **Step 5: Commit**

```
git add python/src/extract/mcp.py python/tests/test_mcp.py
git commit -m "feat: list_experiments MCP tool"
```

---

## Task 4: Tool — `list_runs`

**Goal:** List runs (all or for a specific experiment), with `label` and `config_summary`.

**Files:**
- Modify: `python/src/extract/mcp.py`
- Modify: `python/tests/test_mcp.py`

- [ ] **Step 1: Write failing tests**

```python
class TestListRuns:
    def test_lists_all_runs(self, populated_store):
        result = mcp_mod.list_runs()
        assert result["total"] == 3  # r1a, r1b, r2

    def test_lists_runs_for_experiment(self, populated_store):
        exp1_id = populated_store.test_ids["exp1"]
        result = mcp_mod.list_runs(experiment_id=exp1_id)
        assert result["total"] == 2
        ids = {r["id"] for r in result["items"]}
        assert populated_store.test_ids["r1a"] in ids
        assert populated_store.test_ids["r1b"] in ids

    def test_item_shape(self, populated_store):
        result = mcp_mod.list_runs()
        item = result["items"][0]
        expected_keys = {
            "id", "label", "experiment_id", "experiment_path", "name",
            "status", "started_at", "ended_at", "tags", "git_sha",
            "hostname", "config_summary",
        }
        assert set(item.keys()) == expected_keys

    def test_label_format(self, populated_store):
        result = mcp_mod.list_runs()
        item = next(i for i in result["items"] if i["name"] == "ewc-l1.0-a")
        assert item["label"] == "cifar100/ewc/lambda_1.0#ewc-l1.0-a"

    def test_label_fallback_to_id(self, populated_store, tmp_path):
        # Create a nameless run in the populated store.
        exp = populated_store.experiment(
            {"benchmark": "cifar100", "method": "ewc", "variant": "lambda_1.0"}
        )
        nameless = exp.run()
        nameless.finish()
        result = mcp_mod.list_runs(experiment_id=exp.id)
        nameless_item = next(i for i in result["items"] if i["id"] == nameless.id)
        assert nameless_item["label"].startswith("cifar100/ewc/lambda_1.0#")
        tail = nameless_item["label"].split("#", 1)[1]
        assert len(tail) == 8
        assert tail == nameless.id[:8]

    def test_config_summary_shape(self, populated_store):
        result = mcp_mod.list_runs()
        item = next(i for i in result["items"] if i["name"] == "ewc-l1.0-a")
        cs = item["config_summary"]
        assert cs["n_keys"] == 3  # lr, lambda, method (top-level)
        assert set(cs["top_level_keys"]) == {"lr", "lambda", "method"}

    def test_config_summary_empty_config(self, populated_store):
        exp = populated_store.experiment(
            {"benchmark": "cifar100", "method": "ewc", "variant": "lambda_1.0"}
        )
        nocfg = exp.run(name="no-config")
        nocfg.finish()
        result = mcp_mod.list_runs(experiment_id=exp.id)
        item = next(i for i in result["items"] if i["id"] == nocfg.id)
        assert item["config_summary"] == {"n_keys": 0, "top_level_keys": []}

    def test_unknown_experiment(self, populated_store):
        with pytest.raises(ValueError, match="Experiment not found"):
            mcp_mod.list_runs(experiment_id="not_a_real_id")

    def test_tags_parsed(self, populated_store):
        result = mcp_mod.list_runs()
        item = next(i for i in result["items"] if i["name"] == "ewc-l1.0-a")
        assert "sweep" in item["tags"]
        assert "production-candidate" in item["tags"]
```

- [ ] **Step 2: Run tests to verify they fail**

```
pytest python/tests/test_mcp.py::TestListRuns -v
```

Expected: `AttributeError: ... has no attribute 'list_runs'`.

- [ ] **Step 3: Implement `list_runs`**

Add to `python/src/extract/mcp.py` below `list_experiments`:

```python
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
    import json as _json

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
        config_dict = _json.loads(row["config"]) if row["config"] else {}
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
            "tags": _json.loads(row["tags"]) if row["tags"] else [],
            "git_sha": row["git_sha"],
            "hostname": row["hostname"],
            "config_summary": {
                "n_keys": len(top_keys),
                "top_level_keys": top_keys,
            },
        })

    return _listing(items, total=len(items), limit=limit, limit_clamped=clamped)
```

- [ ] **Step 4: Run tests to verify they pass**

```
pytest python/tests/test_mcp.py::TestListRuns -v
```

Expected: all 9 tests pass.

- [ ] **Step 5: Commit**

```
git add python/src/extract/mcp.py python/tests/test_mcp.py
git commit -m "feat: list_runs MCP tool"
```

---

## Task 5: Tool — `list_models`

**Goal:** Simple listing tool with name-prefix filtering.

**Files:**
- Modify: `python/src/extract/mcp.py`
- Modify: `python/tests/test_mcp.py`

- [ ] **Step 1: Write failing tests**

```python
class TestListModels:
    def test_lists_all_models(self, populated_store):
        result = mcp_mod.list_models()
        assert result["total"] == 1
        item = result["items"][0]
        assert item["name"] == "ewc-cifar100"
        assert item["version"] == "1.0"

    def test_name_prefix_filter(self, populated_store):
        result = mcp_mod.list_models(name_prefix="ewc")
        assert result["total"] == 1

        result = mcp_mod.list_models(name_prefix="nonexistent")
        assert result["total"] == 0

    def test_item_shape(self, populated_store):
        result = mcp_mod.list_models()
        item = result["items"][0]
        expected_keys = {
            "id", "name", "version", "run_id", "framework",
            "artifact_path", "metadata", "created_at",
        }
        assert set(item.keys()) == expected_keys
        assert item["framework"] == "pytorch"
        assert item["metadata"] == {"params": 1000}
```

- [ ] **Step 2: Run tests to verify they fail**

```
pytest python/tests/test_mcp.py::TestListModels -v
```

Expected: `AttributeError: ... has no attribute 'list_models'`.

- [ ] **Step 3: Implement `list_models`**

```python
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
    import json as _json

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
            "metadata": _json.loads(row["metadata"]) if row["metadata"] else None,
            "created_at": row["created_at"],
        })

    return _listing(items, total=len(items), limit=limit, limit_clamped=clamped)
```

- [ ] **Step 4: Run tests to verify they pass**

```
pytest python/tests/test_mcp.py::TestListModels -v
```

Expected: all 3 tests pass.

- [ ] **Step 5: Commit**

```
git add python/src/extract/mcp.py python/tests/test_mcp.py
git commit -m "feat: list_models MCP tool"
```

---

## Task 6: Tool — `list_todos`

**Goal:** Wrap `Store.list_todos` with scope validation and the `include_done` toggle.

**Files:**
- Modify: `python/src/extract/mcp.py`
- Modify: `python/tests/test_mcp.py`

- [ ] **Step 1: Write failing tests**

```python
class TestListTodos:
    def test_global_scope(self, populated_store):
        result = mcp_mod.list_todos()
        assert result["total"] >= 1
        contents = [t["content"] for t in result["items"]]
        assert "Write up results for paper" in contents

    def test_run_scope(self, populated_store):
        r1a_id = populated_store.test_ids["r1a"]
        result = mcp_mod.list_todos(scope_type="run", scope_id=r1a_id)
        assert result["total"] == 1
        assert result["items"][0]["content"] == "Try lambda=0.5 next"
        assert result["items"][0]["priority"] == 1

    def test_include_done_false_excludes_completed(self, populated_store):
        # Mark the global todo done by direct SQL.
        with populated_store.lock:
            populated_store._conn.execute(
                "UPDATE todos SET done = 1 WHERE content = 'Write up results for paper'"
            )
            populated_store._conn.commit()
        result = mcp_mod.list_todos()
        contents = [t["content"] for t in result["items"]]
        assert "Write up results for paper" not in contents

    def test_include_done_true_shows_completed(self, populated_store):
        with populated_store.lock:
            populated_store._conn.execute(
                "UPDATE todos SET done = 1 WHERE content = 'Write up results for paper'"
            )
            populated_store._conn.commit()
        result = mcp_mod.list_todos(include_done=True)
        contents = [t["content"] for t in result["items"]]
        assert "Write up results for paper" in contents

    def test_invalid_scope_type(self, populated_store):
        with pytest.raises(ValueError, match="scope_type must be one of"):
            mcp_mod.list_todos(scope_type="invalid")

    def test_scope_id_required_for_run(self, populated_store):
        with pytest.raises(ValueError, match="scope_id is required"):
            mcp_mod.list_todos(scope_type="run")

    def test_scope_id_forbidden_for_global(self, populated_store):
        with pytest.raises(ValueError, match="scope_id must be None"):
            mcp_mod.list_todos(scope_type="global", scope_id="something")

    def test_item_shape(self, populated_store):
        result = mcp_mod.list_todos()
        item = result["items"][0]
        expected_keys = {
            "id", "scope_type", "scope_id", "content", "priority",
            "done", "created_at", "completed_at",
        }
        assert set(item.keys()) == expected_keys
```

- [ ] **Step 2: Run tests to verify they fail**

```
pytest python/tests/test_mcp.py::TestListTodos -v
```

Expected: `AttributeError: ... has no attribute 'list_todos'`.

- [ ] **Step 3: Implement `list_todos`**

```python
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
```

- [ ] **Step 4: Run tests to verify they pass**

```
pytest python/tests/test_mcp.py::TestListTodos -v
```

Expected: all 8 tests pass.

- [ ] **Step 5: Commit**

```
git add python/src/extract/mcp.py python/tests/test_mcp.py
git commit -m "feat: list_todos MCP tool"
```

---

## Task 7: Tool — `get_run`

**Goal:** Full run detail including parsed config, metrics_final, run_params, artifacts, todos.

**Files:**
- Modify: `python/src/extract/mcp.py`
- Modify: `python/tests/test_mcp.py`

- [ ] **Step 1: Write failing tests**

```python
class TestGetRun:
    def test_happy_path(self, populated_store):
        r1a_id = populated_store.test_ids["r1a"]
        run = mcp_mod.get_run(r1a_id)
        assert run["id"] == r1a_id
        assert run["name"] == "ewc-l1.0-a"
        assert run["experiment_path"] == "cifar100/ewc/lambda_1.0"
        assert run["label"] == "cifar100/ewc/lambda_1.0#ewc-l1.0-a"
        assert run["status"] == "completed"

    def test_full_config_parsed(self, populated_store):
        r1a_id = populated_store.test_ids["r1a"]
        run = mcp_mod.get_run(r1a_id)
        assert run["config"]["lr"] == 0.001
        assert run["config"]["lambda"] == 1.0
        assert run["config"]["method"]["fisher"] == "diagonal"

    def test_metrics_final(self, populated_store):
        r1a_id = populated_store.test_ids["r1a"]
        run = mcp_mod.get_run(r1a_id)
        # Last step is 4; loss = 1.0 - 0.15*4 = 0.4
        assert run["metrics_final"]["loss"] == pytest.approx(0.4)
        assert run["metrics_final"]["accuracy"] == pytest.approx(0.5 + 0.08 * 4)

    def test_metrics_available_list(self, populated_store):
        r1a_id = populated_store.test_ids["r1a"]
        run = mcp_mod.get_run(r1a_id)
        assert "loss" in run["metrics_available"]
        assert "accuracy" in run["metrics_available"]

    def test_run_params(self, populated_store):
        r1a_id = populated_store.test_ids["r1a"]
        run = mcp_mod.get_run(r1a_id)
        assert run["run_params"] == {"arch": "resnet18"}

    def test_tags_and_notes(self, populated_store):
        r1a_id = populated_store.test_ids["r1a"]
        run = mcp_mod.get_run(r1a_id)
        assert "sweep" in run["tags"]
        assert "production-candidate" in run["tags"]
        assert "Best lambda in sweep." in run["notes"]

    def test_todos_for_run(self, populated_store):
        r1a_id = populated_store.test_ids["r1a"]
        run = mcp_mod.get_run(r1a_id)
        contents = [t["content"] for t in run["todos"]]
        assert "Try lambda=0.5 next" in contents

    def test_unknown_id(self, populated_store):
        with pytest.raises(ValueError, match="Run not found"):
            mcp_mod.get_run("not_a_real_id")

    def test_empty_id(self, populated_store):
        with pytest.raises(ValueError, match="run_id is required"):
            mcp_mod.get_run("")
```

- [ ] **Step 2: Run tests to verify they fail**

```
pytest python/tests/test_mcp.py::TestGetRun -v
```

Expected: `AttributeError: ... has no attribute 'get_run'`.

- [ ] **Step 3: Implement `get_run`**

```python
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
    import json as _json

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
                "shape": _json.loads(a["shape"]) if a["shape"] else None,
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
        "tags": _json.loads(row["tags"]) if row["tags"] else [],
        "notes": row["notes"] or "",
        "config": _json.loads(row["config"]) if row["config"] else {},
        "metrics_final": metrics_final,
        "metrics_available": metrics_available,
        "run_params": run_params,
        "artifacts": artifacts,
        "todos": todos,
    }
```

- [ ] **Step 4: Run tests to verify they pass**

```
pytest python/tests/test_mcp.py::TestGetRun -v
```

Expected: all 9 tests pass.

- [ ] **Step 5: Commit**

```
git add python/src/extract/mcp.py python/tests/test_mcp.py
git commit -m "feat: get_run MCP tool"
```

---

## Task 8: Tool — `search`

**Goal:** Substring query + structured filters over runs. Returns the same item shape as `list_runs`.

**Files:**
- Modify: `python/src/extract/mcp.py`
- Modify: `python/tests/test_mcp.py`

- [ ] **Step 1: Write failing tests**

```python
class TestSearch:
    def test_empty_query_no_filters_returns_all(self, populated_store):
        result = mcp_mod.search()
        assert result["total"] == 3

    def test_query_substring_name(self, populated_store):
        result = mcp_mod.search(query="ewc-l1.0")
        names = {i["name"] for i in result["items"]}
        assert names == {"ewc-l1.0-a", "ewc-l1.0-b"}

    def test_query_substring_tags(self, populated_store):
        result = mcp_mod.search(query="production")
        names = {i["name"] for i in result["items"]}
        assert names == {"ewc-l1.0-a"}

    def test_query_substring_notes(self, populated_store):
        result = mcp_mod.search(query="Best lambda")
        assert result["total"] == 1
        assert result["items"][0]["name"] == "ewc-l1.0-a"

    def test_filter_tag(self, populated_store):
        result = mcp_mod.search(filters={"tag": "sweep"})
        names = {i["name"] for i in result["items"]}
        assert names == {"ewc-l1.0-a", "ewc-l1.0-b"}

    def test_filter_status(self, populated_store):
        result = mcp_mod.search(filters={"status": "completed"})
        assert result["total"] == 3

    def test_filter_status_invalid(self, populated_store):
        with pytest.raises(ValueError, match="status must be one of"):
            mcp_mod.search(filters={"status": "bogus"})

    def test_filter_experiment_prefix(self, populated_store):
        result = mcp_mod.search(filters={"experiment_prefix": "cifar100/ewc"})
        assert result["total"] == 2
        paths = {i["experiment_path"] for i in result["items"]}
        assert paths == {"cifar100/ewc/lambda_1.0"}

    def test_filters_combine_and(self, populated_store):
        # tag=sweep AND experiment_prefix=cifar100/ewc AND status=completed
        result = mcp_mod.search(
            filters={
                "tag": "sweep",
                "experiment_prefix": "cifar100/ewc",
                "status": "completed",
            }
        )
        assert result["total"] == 2

    def test_unknown_filter(self, populated_store):
        with pytest.raises(ValueError, match="Unknown filter"):
            mcp_mod.search(filters={"not_a_filter": "x"})

    def test_item_shape_matches_list_runs(self, populated_store):
        search_item = mcp_mod.search()["items"][0]
        list_item = mcp_mod.list_runs()["items"][0]
        assert set(search_item.keys()) == set(list_item.keys())

    def test_started_after_before(self, populated_store):
        # Started_after in the future returns nothing.
        result = mcp_mod.search(filters={"started_after": "2099-01-01T00:00:00.000Z"})
        assert result["total"] == 0
```

- [ ] **Step 2: Run tests to verify they fail**

```
pytest python/tests/test_mcp.py::TestSearch -v
```

Expected: `AttributeError: ... has no attribute 'search'`.

- [ ] **Step 3: Implement `search`**

```python
_VALID_STATUS = ("running", "completed", "failed")
_VALID_FILTERS = {
    "tag", "status", "experiment_prefix", "started_after", "started_before",
}


@_tool
def search(
    query: str = "",
    filters: dict | None = None,
    limit: int = 50,
) -> dict:
    """Search runs by substring + structured filters.

    Args:
        query: Case-insensitive substring matched against run name, tags,
            and notes. Empty string means no text filter.
        filters: Optional dict of AND-combined filters. Valid keys:
            - tag: str — run must contain this tag
            - status: "running" | "completed" | "failed"
            - experiment_prefix: str — run's experiment path starts with this
            - started_after: ISO 8601 str (runs.started_at >= value)
            - started_before: ISO 8601 str (runs.started_at <= value)
        limit: Max rows (default 50, max 500).

    Returns a listing envelope of run rows in the same shape as list_runs.
    """
    import json as _json

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
            f"status must be one of: running, completed, failed "
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
        config_dict = _json.loads(row["config"]) if row["config"] else {}
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
            "tags": _json.loads(row["tags"]) if row["tags"] else [],
            "git_sha": row["git_sha"],
            "hostname": row["hostname"],
            "config_summary": {
                "n_keys": len(top_keys),
                "top_level_keys": top_keys,
            },
        })

    return _listing(items, total=len(items), limit=limit, limit_clamped=clamped)
```

Note: `search` and `list_runs` both construct the same item shape. We intentionally duplicate the ~15 lines of row-to-item conversion rather than extracting a helper — the two tools may diverge (e.g., search may add relevance scoring later) and the duplication is small. If a third caller needs the same shape, extract then.

- [ ] **Step 4: Run tests to verify they pass**

```
pytest python/tests/test_mcp.py::TestSearch -v
```

Expected: all 12 tests pass.

- [ ] **Step 5: Commit**

```
git add python/src/extract/mcp.py python/tests/test_mcp.py
git commit -m "feat: search MCP tool"
```

---

## Task 9: Tool — `compare_runs`

**Goal:** The most complex tool — metric values + rankings, optional histories, config diffs.

**Files:**
- Modify: `python/src/extract/mcp.py`
- Modify: `python/tests/test_mcp.py`

- [ ] **Step 1: Write failing tests**

```python
class TestCompareRuns:
    def test_basic_two_runs(self, populated_store):
        r1a_id = populated_store.test_ids["r1a"]
        r1b_id = populated_store.test_ids["r1b"]
        result = mcp_mod.compare_runs([r1a_id, r1b_id])

        assert len(result["runs"]) == 2
        assert {r["id"] for r in result["runs"]} == {r1a_id, r1b_id}
        assert "loss" in result["metrics"]
        assert "accuracy" in result["metrics"]

    def test_metric_direction(self, populated_store):
        r1a_id = populated_store.test_ids["r1a"]
        r1b_id = populated_store.test_ids["r1b"]
        result = mcp_mod.compare_runs([r1a_id, r1b_id])
        assert result["metrics"]["loss"]["direction"] == "min"
        assert result["metrics"]["accuracy"]["direction"] == "max"

    def test_ranking_for_min_metric(self, populated_store):
        # r1a loss final = 0.4; r1b loss final = 0.9 - 0.12*4 = 0.42
        # Direction = min, so r1a ranks first.
        r1a_id = populated_store.test_ids["r1a"]
        r1b_id = populated_store.test_ids["r1b"]
        result = mcp_mod.compare_runs([r1a_id, r1b_id])
        assert result["metrics"]["loss"]["ranking"][0] == r1a_id
        assert result["metrics"]["loss"]["ranking"][1] == r1b_id

    def test_values_final_per_run(self, populated_store):
        r1a_id = populated_store.test_ids["r1a"]
        r1b_id = populated_store.test_ids["r1b"]
        result = mcp_mod.compare_runs([r1a_id, r1b_id])
        vals = result["metrics"]["loss"]["values"]
        assert vals[r1a_id] == pytest.approx(0.4)
        assert vals[r1b_id] == pytest.approx(0.42)

    def test_history_omitted_by_default(self, populated_store):
        r1a_id = populated_store.test_ids["r1a"]
        r1b_id = populated_store.test_ids["r1b"]
        result = mcp_mod.compare_runs([r1a_id, r1b_id])
        assert "history" not in result["metrics"]["loss"]

    def test_history_included_when_requested(self, populated_store):
        r1a_id = populated_store.test_ids["r1a"]
        r1b_id = populated_store.test_ids["r1b"]
        result = mcp_mod.compare_runs([r1a_id, r1b_id], include_history=True)
        hist = result["metrics"]["loss"]["history"]
        assert r1a_id in hist
        assert r1b_id in hist
        # 5 steps logged per run.
        assert len(hist[r1a_id]) == 5
        # Each entry is [step, value].
        assert hist[r1a_id][0] == [0, pytest.approx(1.0)]

    def test_config_diffs_only_differing_keys(self, populated_store):
        r1a_id = populated_store.test_ids["r1a"]
        r1b_id = populated_store.test_ids["r1b"]
        result = mcp_mod.compare_runs([r1a_id, r1b_id])
        diffs = result["config_diffs"]
        # lr differs: 0.001 vs 0.0005
        assert "lr" in diffs
        # lambda is the same (1.0 in both) — should NOT be present
        assert "lambda" not in diffs
        # method.fisher differs: diagonal vs empirical
        assert "method.fisher" in diffs
        assert diffs["method.fisher"][r1a_id] == "diagonal"
        assert diffs["method.fisher"][r1b_id] == "empirical"

    def test_three_runs(self, populated_store):
        ids = [
            populated_store.test_ids["r1a"],
            populated_store.test_ids["r1b"],
            populated_store.test_ids["r2"],
        ]
        result = mcp_mod.compare_runs(ids)
        assert len(result["runs"]) == 3

    def test_too_few_runs(self, populated_store):
        with pytest.raises(ValueError, match="at least 2 run_ids"):
            mcp_mod.compare_runs([populated_store.test_ids["r1a"]])

    def test_too_many_runs(self, populated_store):
        with pytest.raises(ValueError, match="at most 10"):
            mcp_mod.compare_runs(["id"] * 11)

    def test_unknown_run_id(self, populated_store):
        r1a_id = populated_store.test_ids["r1a"]
        with pytest.raises(ValueError, match="Run not found"):
            mcp_mod.compare_runs([r1a_id, "not_a_real_id"])
```

- [ ] **Step 2: Run tests to verify they fail**

```
pytest python/tests/test_mcp.py::TestCompareRuns -v
```

Expected: `AttributeError: ... has no attribute 'compare_runs'`.

- [ ] **Step 3: Implement `compare_runs`**

```python
@_tool
def compare_runs(run_ids: list[str], include_history: bool = False) -> dict:
    """Compare 2-10 runs: final metric values, rankings, and config diffs.

    Args:
        run_ids: List of 2-10 run ULIDs.
        include_history: If True, include per-metric [(step, value), ...]
            histories for every run. Off by default to keep payloads bounded.

    Returns:
        {
          runs: [{id, label, experiment_path, status}],
          metrics: {
            name: {
              direction: "min" | "max",
              values: {run_id: final_value},
              ranking: [best_run_id, ..., worst_run_id],
              history: {run_id: [[step, value], ...]}  # only if include_history
            }
          },
          config_diffs: {flat_key: {run_id: value}}  # only differing keys
        }
    """
    import json as _json

    if len(run_ids) < 2:
        raise ValueError(f"compare_runs requires at least 2 run_ids (got {len(run_ids)})")
    if len(run_ids) > 10:
        raise ValueError(
            f"compare_runs supports at most 10 runs per call (got {len(run_ids)})"
        )

    assert _store is not None
    runs_out: list[dict] = []
    configs: list[tuple[str, dict]] = []
    metric_values: dict[str, dict[str, float]] = {}  # name -> {run_id: final_val}
    metric_history: dict[str, dict[str, list[list]]] = {}

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

            runs_out.append({
                "id": row["id"],
                "label": _label(row["experiment_path"], row["name"], row["id"]),
                "experiment_path": row["experiment_path"],
                "status": row["status"],
            })
            cfg = _json.loads(row["config"]) if row["config"] else {}
            configs.append((rid, cfg))

            # Final per-metric value.
            metric_rows = _store._conn.execute(
                "SELECT name, value FROM scalar_metrics sm1 WHERE run_id = ? "
                "AND step = (SELECT MAX(step) FROM scalar_metrics sm2 "
                "            WHERE sm2.run_id = sm1.run_id AND sm2.name = sm1.name)",
                (rid,),
            ).fetchall()
            for mr in metric_rows:
                metric_values.setdefault(mr["name"], {})[rid] = mr["value"]

            # Full history if requested.
            if include_history:
                hist_rows = _store._conn.execute(
                    "SELECT name, step, value FROM scalar_metrics "
                    "WHERE run_id = ? ORDER BY name, step",
                    (rid,),
                ).fetchall()
                for h in hist_rows:
                    metric_history.setdefault(h["name"], {}).setdefault(rid, []).append(
                        [h["step"], h["value"]]
                    )

    # Build the metrics dict.
    metrics_out: dict[str, dict] = {}
    for name, vals in metric_values.items():
        direction = _metric_direction(name)
        reverse = (direction == "max")
        ranking = [rid for rid, _ in sorted(
            vals.items(), key=lambda kv: kv[1], reverse=reverse
        )]
        entry = {
            "direction": direction,
            "values": vals,
            "ranking": ranking,
        }
        if include_history and name in metric_history:
            entry["history"] = metric_history[name]
        metrics_out[name] = entry

    return {
        "runs": runs_out,
        "metrics": metrics_out,
        "config_diffs": _config_diffs(configs),
    }
```

- [ ] **Step 4: Run tests to verify they pass**

```
pytest python/tests/test_mcp.py::TestCompareRuns -v
```

Expected: all 11 tests pass.

- [ ] **Step 5: Commit**

```
git add python/src/extract/mcp.py python/tests/test_mcp.py
git commit -m "feat: compare_runs MCP tool"
```

---

## Task 10: Tool — `get_lineage`

**Goal:** BFS walk of the lineage DAG from a given node, in the requested direction(s), up to a depth cap.

**Files:**
- Modify: `python/src/extract/mcp.py`
- Modify: `python/tests/test_mcp.py`

- [ ] **Step 1: Write failing tests**

The fixture has these edges:
- `model(ewc-cifar100@1.0) --derived_from--> run(r1b)`
- `run(r1a) --branched_from--> run(r2)`

```python
class TestGetLineage:
    def test_descendants_of_model(self, populated_store):
        model_id = populated_store.test_ids["model"]
        r1b_id = populated_store.test_ids["r1b"]
        result = mcp_mod.get_lineage(
            node_type="model", node_id=model_id, direction="descendants"
        )
        assert result["root"]["id"] == model_id
        assert result["root"]["type"] == "model"
        ids = {n["id"] for n in result["nodes"]}
        assert r1b_id in ids
        # At least one edge connects model -> run
        edge = next(
            e for e in result["edges"]
            if e["parent_type"] == "model" and e["child_id"] == r1b_id
        )
        assert edge["relation"] == "derived_from"

    def test_ancestors_of_run(self, populated_store):
        r1b_id = populated_store.test_ids["r1b"]
        model_id = populated_store.test_ids["model"]
        result = mcp_mod.get_lineage(
            node_type="run", node_id=r1b_id, direction="ancestors"
        )
        ids = {n["id"] for n in result["nodes"]}
        assert model_id in ids

    def test_both_direction(self, populated_store):
        r1a_id = populated_store.test_ids["r1a"]
        r2_id = populated_store.test_ids["r2"]
        result = mcp_mod.get_lineage(
            node_type="run", node_id=r1a_id, direction="both"
        )
        ids = {n["id"] for n in result["nodes"]}
        assert r2_id in ids  # descendant via branched_from

    def test_depth_cap(self, populated_store):
        r1a_id = populated_store.test_ids["r1a"]
        result = mcp_mod.get_lineage(
            node_type="run", node_id=r1a_id, direction="descendants", depth=1
        )
        # Only immediate descendants (depth=1). Should include r2.
        r2_id = populated_store.test_ids["r2"]
        ids = {n["id"] for n in result["nodes"]}
        assert r2_id in ids

    def test_root_shape(self, populated_store):
        r1a_id = populated_store.test_ids["r1a"]
        result = mcp_mod.get_lineage(
            node_type="run", node_id=r1a_id
        )
        assert set(result["root"].keys()) == {"type", "id", "label"}
        assert result["root"]["label"] == "cifar100/ewc/lambda_1.0#ewc-l1.0-a"

    def test_model_label_format(self, populated_store):
        model_id = populated_store.test_ids["model"]
        result = mcp_mod.get_lineage(
            node_type="model", node_id=model_id, direction="descendants"
        )
        assert result["root"]["label"] == "ewc-cifar100@1.0"

    def test_invalid_direction(self, populated_store):
        with pytest.raises(ValueError, match="direction must be one of"):
            mcp_mod.get_lineage(
                node_type="run", node_id="x", direction="sideways"
            )

    def test_invalid_node_type(self, populated_store):
        with pytest.raises(ValueError, match="node_type must be one of"):
            mcp_mod.get_lineage(node_type="widget", node_id="x")

    def test_depth_out_of_range(self, populated_store):
        with pytest.raises(ValueError, match="depth must be between"):
            mcp_mod.get_lineage(node_type="run", node_id="x", depth=10)
        with pytest.raises(ValueError, match="depth must be between"):
            mcp_mod.get_lineage(node_type="run", node_id="x", depth=0)

    def test_unknown_root(self, populated_store):
        with pytest.raises(ValueError, match="Run not found"):
            mcp_mod.get_lineage(node_type="run", node_id="not_a_real_id")
```

- [ ] **Step 2: Run tests to verify they fail**

```
pytest python/tests/test_mcp.py::TestGetLineage -v
```

Expected: `AttributeError: ... has no attribute 'get_lineage'`.

- [ ] **Step 3: Implement `get_lineage`**

```python
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
            pretty = {"run": "Run", "experiment": "Experiment", "model": "Model"}[node_type]
            raise ValueError(f"{pretty} not found: {node_id!r}")

        visited: set[tuple[str, str]] = {(node_type, node_id)}
        edges_out: list[dict] = []
        frontier: list[tuple[str, str]] = [(node_type, node_id)]

        for _hop in range(depth):
            next_frontier: list[tuple[str, str]] = []
            for (nt, nid) in frontier:
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
            key = (e["parent_type"], e["parent_id"],
                   e["child_type"], e["child_id"], e["relation"])
            if key not in seen_edges:
                seen_edges.add(key)
                unique_edges.append(e)

        nodes_out: list[dict] = []
        for (nt, nid) in visited:
            if (nt, nid) == (node_type, node_id):
                continue  # root is emitted separately
            label = _lookup_node_label(_store._conn, nt, nid)
            nodes_out.append({"type": nt, "id": nid, "label": label or ""})

    return {
        "root": {"type": node_type, "id": node_id, "label": root_label},
        "nodes": nodes_out,
        "edges": unique_edges,
    }
```

- [ ] **Step 4: Run tests to verify they pass**

```
pytest python/tests/test_mcp.py::TestGetLineage -v
```

Expected: all 10 tests pass.

- [ ] **Step 5: Commit**

```
git add python/src/extract/mcp.py python/tests/test_mcp.py
git commit -m "feat: get_lineage MCP tool"
```

---

## Task 11: Cross-Cutting Tests

**Goal:** Test behaviors that span tools — listing envelope consistency, limit_clamped flag, label stability, error-message catalog.

**Files:**
- Modify: `python/tests/test_mcp.py`

- [ ] **Step 1: Add `TestListingEnvelope`**

```python
class TestListingEnvelope:
    @pytest.mark.parametrize("tool_name,kwargs", [
        ("list_experiments", {}),
        ("list_runs", {}),
        ("list_models", {}),
        ("list_todos", {}),
        ("search", {}),
    ])
    def test_envelope_shape(self, populated_store, tool_name, kwargs):
        fn = getattr(mcp_mod, tool_name)
        result = fn(**kwargs)
        assert "items" in result
        assert "total" in result
        assert "truncated" in result
        assert isinstance(result["items"], list)
        assert isinstance(result["total"], int)
        assert isinstance(result["truncated"], bool)

    def test_truncation_flag_when_limited(self, populated_store):
        result = mcp_mod.list_runs(limit=1)
        assert len(result["items"]) == 1
        assert result["total"] >= 2
        assert result["truncated"] is True

    def test_not_truncated_when_under_limit(self, populated_store):
        result = mcp_mod.list_runs(limit=50)
        assert result["truncated"] is False
```

- [ ] **Step 2: Add `TestLimitClamp`**

```python
class TestLimitClamp:
    def test_clamp_flag_set(self, populated_store):
        result = mcp_mod.list_runs(limit=1000)
        assert result.get("limit_clamped") is True
        assert len(result["items"]) <= 500

    def test_no_clamp_flag_under_cap(self, populated_store):
        result = mcp_mod.list_runs(limit=100)
        assert "limit_clamped" not in result

    def test_limit_negative_errors(self, populated_store):
        with pytest.raises(ValueError, match="limit must be"):
            mcp_mod.list_runs(limit=0)
        with pytest.raises(ValueError, match="limit must be"):
            mcp_mod.list_runs(limit=-5)
```

- [ ] **Step 3: Add `TestLabelStability`**

```python
class TestLabelStability:
    def test_same_label_across_tools(self, populated_store):
        r1a_id = populated_store.test_ids["r1a"]
        r1b_id = populated_store.test_ids["r1b"]

        list_label = next(
            i["label"] for i in mcp_mod.list_runs()["items"] if i["id"] == r1a_id
        )
        get_label = mcp_mod.get_run(r1a_id)["label"]
        compare_label = next(
            r["label"] for r in mcp_mod.compare_runs([r1a_id, r1b_id])["runs"]
            if r["id"] == r1a_id
        )
        search_label = next(
            i["label"] for i in mcp_mod.search(query="ewc-l1.0-a")["items"]
            if i["id"] == r1a_id
        )

        assert list_label == get_label == compare_label == search_label
        assert list_label == "cifar100/ewc/lambda_1.0#ewc-l1.0-a"
```

- [ ] **Step 4: Add `TestErrorMessages`**

```python
class TestErrorMessages:
    """Regression protection for agent-facing error strings."""

    def test_get_run_empty_id(self, populated_store):
        with pytest.raises(ValueError, match=r"run_id is required"):
            mcp_mod.get_run("")

    def test_get_run_unknown(self, populated_store):
        with pytest.raises(ValueError, match=r"Run not found: 'nope'"):
            mcp_mod.get_run("nope")

    def test_list_runs_unknown_experiment(self, populated_store):
        with pytest.raises(ValueError, match=r"Experiment not found: 'nope'"):
            mcp_mod.list_runs(experiment_id="nope")

    def test_compare_runs_too_few(self, populated_store):
        with pytest.raises(
            ValueError,
            match=r"compare_runs requires at least 2 run_ids \(got 1\)",
        ):
            mcp_mod.compare_runs(["only_one"])

    def test_compare_runs_too_many(self, populated_store):
        with pytest.raises(
            ValueError,
            match=r"compare_runs supports at most 10 runs per call \(got 11\)",
        ):
            mcp_mod.compare_runs(["x"] * 11)

    def test_list_todos_bad_scope(self, populated_store):
        with pytest.raises(
            ValueError,
            match=r"scope_type must be one of: global, experiment, run \(got 'bad'\)",
        ):
            mcp_mod.list_todos(scope_type="bad")

    def test_search_bad_filter(self, populated_store):
        with pytest.raises(
            ValueError,
            match=(
                r"Unknown filter: 'nope'\. "
                r"Valid filters: tag, status, experiment_prefix, "
                r"started_after, started_before"
            ),
        ):
            mcp_mod.search(filters={"nope": "x"})

    def test_search_bad_status(self, populated_store):
        with pytest.raises(
            ValueError,
            match=r"status must be one of: running, completed, failed \(got 'bogus'\)",
        ):
            mcp_mod.search(filters={"status": "bogus"})

    def test_get_lineage_bad_node_type(self, populated_store):
        with pytest.raises(
            ValueError,
            match=r"node_type must be one of: experiment, run, model \(got 'widget'\)",
        ):
            mcp_mod.get_lineage(node_type="widget", node_id="x")

    def test_get_lineage_bad_direction(self, populated_store):
        with pytest.raises(
            ValueError,
            match=r"direction must be one of: ancestors, descendants, both \(got 'up'\)",
        ):
            mcp_mod.get_lineage(node_type="run", node_id="x", direction="up")

    def test_get_lineage_bad_depth(self, populated_store):
        with pytest.raises(ValueError, match=r"depth must be between 1 and 5 \(got 7\)"):
            mcp_mod.get_lineage(node_type="run", node_id="x", depth=7)
```

- [ ] **Step 5: Run all cross-cutting tests**

```
pytest python/tests/test_mcp.py::TestListingEnvelope python/tests/test_mcp.py::TestLimitClamp python/tests/test_mcp.py::TestLabelStability python/tests/test_mcp.py::TestErrorMessages -v
```

Expected: all tests pass.

- [ ] **Step 6: Commit**

```
git add python/tests/test_mcp.py
git commit -m "test: cross-cutting MCP tests for envelope, clamping, labels, errors"
```

---

## Task 12: Smoke Test (subprocess + stdio)

**Goal:** One end-to-end test that spawns `python -m extract.mcp` as a subprocess, connects via `mcp.client.stdio`, calls `list_experiments`, and confirms a non-empty response. Covers the entry point, arg parsing, Store open, FastMCP stdio transport, and tool dispatch.

**Files:**
- Modify: `python/tests/test_mcp.py`

- [ ] **Step 1: Check if `mcp` client library is importable**

```
python -c "from mcp.client.stdio import stdio_client; print('ok')"
```

If this prints `ok`, the smoke test will run. If it fails with ImportError, the smoke test will skip with a clear reason.

- [ ] **Step 2: Add the smoke test class**

Add to `python/tests/test_mcp.py`:

```python
class TestServerSmoke:
    def test_server_boots_and_lists_experiments(self, tmp_path):
        """Spawn the real server as a subprocess and round-trip a tool call."""
        pytest.importorskip("mcp.client.stdio")

        import asyncio
        from mcp import ClientSession, StdioServerParameters
        from mcp.client.stdio import stdio_client

        # Build a minimal store on disk (the subprocess reads its own).
        root = tmp_path / ".extract"
        root.mkdir()
        (root / "config.toml").write_text(
            '[store]\nhierarchy = "benchmark > method > variant"\n'
        )
        store = extract.Store(root=root)
        exp = store.experiment(
            {"benchmark": "cifar100", "method": "ewc", "variant": "v1"}
        )
        with exp.run(config={"lr": 0.001}, name="smoke") as r:
            r.log(step=0, loss=1.0)
        store.close()

        async def run_client() -> dict:
            server_params = StdioServerParameters(
                command=sys.executable,
                args=["-m", "extract.mcp", "--store", str(root)],
            )
            async with stdio_client(server_params) as (read, write):
                async with ClientSession(read, write) as session:
                    await session.initialize()
                    result = await session.call_tool(
                        "list_experiments", {}
                    )
                    return result

        result = asyncio.run(run_client())
        # FastMCP wraps the tool return value in content blocks.
        # We just need to assert the call didn't error and got content.
        assert result.isError is False
        assert result.content  # non-empty content list
        # Content[0] is typically a TextContent with JSON text.
        text_content = result.content[0].text
        parsed = json.loads(text_content)
        assert parsed["total"] >= 1
```

- [ ] **Step 3: Run the smoke test**

```
pytest python/tests/test_mcp.py::TestServerSmoke -v
```

Expected: `test_server_boots_and_lists_experiments PASSED` if `mcp` is installed, `SKIPPED (could not import 'mcp.client.stdio')` otherwise.

If it errors with an API mismatch (e.g. `call_tool` signature differs or `result.isError` has a different name), read the actual `mcp` package version installed and adjust the client code to match. The test's intent is: spawn subprocess → initialize session → call `list_experiments` → assert success. Whatever the current `mcp.client` API is, the test should do that.

- [ ] **Step 4: Run the entire test file**

```
pytest python/tests/test_mcp.py -v
```

Expected: all tests pass (smoke test skipped if `mcp` isn't installed; everything else green).

- [ ] **Step 5: Commit**

```
git add python/tests/test_mcp.py
git commit -m "test: stdio smoke test for extract.mcp server"
```

---

## Task 13: Strike Phase 6 off `PLAN.md`

**Goal:** Remove the "Phase 6: MCP Server" section from `PLAN.md` since it's now implemented.

**Files:**
- Modify: `PLAN.md`

- [ ] **Step 1: Read current PLAN.md**

```
cat PLAN.md
```

- [ ] **Step 2: Remove the Phase 6 section**

Delete these three lines (and the surrounding blank line so the file is clean):

```markdown
## Phase 6: MCP Server

- `python -m extract.mcp` exposing tools for LLM agents
- Tools: list_experiments, list_runs, get_run, compare_runs, search, create_todo, list_todos, log_metrics, get_lineage, list_models
```

The remaining file should start with `# Extract — Future Work` and then jump directly to `## Beyond: High Value`.

- [ ] **Step 3: Commit**

```
git add PLAN.md
git commit -m "docs: remove Phase 6 from PLAN.md (MCP server implemented)"
```

---

## Final Verification

- [ ] **Step 1: Run the full test suite**

```
pytest python/tests/ -v
```

Expected: all tests in `test_mcp.py` pass. `test_hierarchy.py` may have pre-existing failures unrelated to this work (its `hierarchy=` kwarg on `Store` is stale API).

- [ ] **Step 2: Manual smoke against a real store**

Assuming you have a populated `.extract/` in the project root:

```
python -m extract.mcp --store .extract
```

The server should start and wait on stdin. Send an MCP `initialize` message (or just Ctrl-C — it should shut down cleanly).

- [ ] **Step 3: Verify no accidental changes to other files**

```
git status
```

Expected: clean working tree. Only `python/src/extract/mcp.py`, `python/tests/test_mcp.py`, and `PLAN.md` should have been touched across all commits.

- [ ] **Step 4: Verify the commit log tells the story**

```
git log --oneline -20
```

Expected: ~13 new commits, each scoped to one task:
```
docs: remove Phase 6 from PLAN.md (MCP server implemented)
test: stdio smoke test for extract.mcp server
test: cross-cutting MCP tests for envelope, clamping, labels, errors
feat: get_lineage MCP tool
feat: compare_runs MCP tool
feat: search MCP tool
feat: get_run MCP tool
feat: list_todos MCP tool
feat: list_models MCP tool
feat: list_runs MCP tool
feat: list_experiments MCP tool
feat: shared helpers for extract.mcp
feat: bootstrap extract.mcp module and test fixture
docs: Phase 6 MCP server design spec
```
