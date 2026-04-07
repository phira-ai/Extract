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
