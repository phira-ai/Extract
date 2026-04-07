"""Tests for the MCP server tool surface."""

from __future__ import annotations

import json
import shutil
import subprocess
import sys
from pathlib import Path
from ulid import ULID

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
    dummy_model = tmp_path / "dummy_model.pt"
    dummy_model.write_bytes(b"fake model bytes")
    # Re-open the run via a direct insert since run.register_model requires
    # an active run and we closed them above. Use the SDK via a throwaway
    # run... but simpler: insert directly.
    model_id = str(ULID())
    models_dir = root / "models" / "ewc-cifar100" / "1.0"
    models_dir.mkdir(parents=True, exist_ok=True)
    shutil.copy(dummy_model, models_dir / "dummy_model.pt")
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


class TestFixture:
    def test_fixture_builds(self, populated_store):
        assert populated_store.test_ids["r1a"]
        assert populated_store.test_ids["r1b"]
        assert populated_store.test_ids["r2"]
        assert populated_store.test_ids["exp1"]
        assert populated_store.test_ids["exp2"]
        assert populated_store.test_ids["model"]


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

    def test_config_diffs_list_values(self):
        from extract.mcp import _config_diffs
        # Lists are leaf values — they should survive the distinct-value
        # comparison without crashing (lists are unhashable).
        pairs = [("r1", {"layers": [64, 128]}), ("r2", {"layers": [64, 256]})]
        assert _config_diffs(pairs) == {"layers": {"r1": [64, 128], "r2": [64, 256]}}

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
        # Query with the branch prefix so both the branch and its descendants appear.
        result = mcp_mod.list_experiments(prefix="cifar100/ewc")
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

    def test_label_fallback_to_id(self, populated_store):
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
