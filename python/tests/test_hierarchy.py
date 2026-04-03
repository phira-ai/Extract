"""Tests for hierarchy config and dict-based experiment creation."""

import sqlite3
import tempfile
from pathlib import Path

import pytest

import extract


@pytest.fixture
def tmp_store(tmp_path):
    """Create a Store with a standard hierarchy in a temp directory."""
    return extract.Store(
        root=tmp_path / ".extract",
        hierarchy="benchmark > method > variant",
    )


class TestHierarchyConfig:
    def test_hierarchy_stored_in_db(self, tmp_store):
        levels = tmp_store._load_hierarchy()
        assert levels == ["benchmark", "method", "variant"]

    def test_hierarchy_persists_across_opens(self, tmp_path):
        root = tmp_path / ".extract"
        extract.Store(root=root, hierarchy="benchmark > method > variant")
        store2 = extract.Store(root=root)
        assert store2._hierarchy == ["benchmark", "method", "variant"]

    def test_hierarchy_mismatch_raises(self, tmp_path):
        root = tmp_path / ".extract"
        extract.Store(root=root, hierarchy="benchmark > method > variant")
        with pytest.raises(ValueError, match="cannot change"):
            extract.Store(root=root, hierarchy="method > benchmark")

    def test_no_hierarchy_legacy_mode(self, tmp_path):
        store = extract.Store(root=tmp_path / ".extract")
        assert store._hierarchy == []
        # Legacy string path still works
        exp = store.experiment("a/b/c")
        assert exp.path == "a/b/c"

    def test_parse_hierarchy_strips_whitespace(self):
        from extract.store import _parse_hierarchy
        assert _parse_hierarchy("  a > b  >c ") == ["a", "b", "c"]

    def test_parse_hierarchy_empty_level_raises(self):
        from extract.store import _parse_hierarchy
        with pytest.raises(ValueError, match="empty level"):
            _parse_hierarchy("a > > b")


class TestDictExperiment:
    def test_creates_path_in_hierarchy_order(self, tmp_store):
        # Dict keys in arbitrary order, path follows hierarchy config
        exp = tmp_store.experiment({
            "variant": "lambda_1.0",
            "benchmark": "cifar100",
            "method": "ewc",
        })
        assert exp.path == "cifar100/ewc/lambda_1.0"
        assert exp.name == "lambda_1.0"

    def test_ancestors_have_correct_node_type(self, tmp_store):
        tmp_store.experiment({
            "benchmark": "cifar100",
            "method": "ewc",
            "variant": "lambda_1.0",
        })
        conn = tmp_store._conn
        rows = conn.execute(
            "SELECT path, node_type FROM experiments ORDER BY path"
        ).fetchall()
        types = {r["path"]: r["node_type"] for r in rows}
        assert types["cifar100"] == "benchmark"
        assert types["cifar100/ewc"] == "method"
        assert types["cifar100/ewc/lambda_1.0"] == "variant"

    def test_partial_spec_creates_partial_hierarchy(self, tmp_store):
        exp = tmp_store.experiment({
            "benchmark": "cifar100",
            "method": "ewc",
        })
        assert exp.path == "cifar100/ewc"
        assert exp.name == "ewc"

    def test_reuses_existing_ancestors(self, tmp_store):
        tmp_store.experiment({"benchmark": "cifar100", "method": "ewc", "variant": "v1"})
        tmp_store.experiment({"benchmark": "cifar100", "method": "ewc", "variant": "v2"})
        conn = tmp_store._conn
        count = conn.execute("SELECT COUNT(*) FROM experiments WHERE path = 'cifar100'").fetchone()[0]
        assert count == 1  # cifar100 created only once

    def test_unknown_level_raises(self, tmp_store):
        with pytest.raises(ValueError, match="Unknown hierarchy levels"):
            tmp_store.experiment({"benchmark": "cifar100", "dataset": "oops"})

    def test_dict_without_hierarchy_raises(self, tmp_path):
        store = extract.Store(root=tmp_path / ".extract")
        with pytest.raises(ValueError, match="without hierarchy"):
            store.experiment({"benchmark": "cifar100"})

    def test_empty_spec_raises(self, tmp_store):
        with pytest.raises(ValueError, match="at least one"):
            tmp_store.experiment({})

    def test_runs_work_on_dict_experiments(self, tmp_store):
        exp = tmp_store.experiment({
            "benchmark": "cifar100",
            "method": "ewc",
            "variant": "lambda_1.0",
        })
        with exp.run(config={"lr": 0.001}) as run:
            run.log(step=0, loss=0.5, accuracy=0.7)

        runs = exp.list_runs()
        assert len(runs) == 1
        assert runs[0]["status"] == "completed"
