"""Tests for Store hierarchy loading and dict-spec experiment creation.

These tests use the post-init SDK contract:
  - Store(root) requires root/config.toml with [store] hierarchy
  - There is no Store(hierarchy=...) constructor kwarg; bootstrap is via
    extract init (or test fixtures pre-writing config.toml)
  - The legacy path-string API on store.experiment() is gone
"""

from __future__ import annotations

import pytest

import extract
from extract.store import MissingHierarchyError, _parse_hierarchy


def _bootstrap(root, hierarchy="benchmark > model > variant"):
    """Helper: create root/config.toml with the given hierarchy line."""
    root.mkdir(parents=True, exist_ok=True)
    (root / "config.toml").write_text(f'[store]\nhierarchy = "{hierarchy}"\n')


@pytest.fixture
def tmp_store(tmp_path):
    """Create a Store with the standard hierarchy in a temp directory."""
    root = tmp_path / ".extract"
    _bootstrap(root)
    return extract.Store(root=root)


# ──────────────────────────────────────────────────────────────────────────
# Hierarchy loading


class TestHierarchyConfig:
    def test_hierarchy_loaded_from_config(self, tmp_store):
        assert tmp_store._hierarchy == ["benchmark", "model", "variant"]

    def test_hierarchy_persists_in_db(self, tmp_path):
        root = tmp_path / ".extract"
        _bootstrap(root)
        extract.Store(root=root)  # First open writes hierarchy table

        store2 = extract.Store(root=root)
        assert store2._load_hierarchy() == ["benchmark", "model", "variant"]

    def test_hierarchy_mismatch_raises(self, tmp_path):
        root = tmp_path / ".extract"
        _bootstrap(root, "benchmark > model > variant")
        extract.Store(root=root)  # First open populates DB

        # Now corrupt the config to a different hierarchy
        (root / "config.toml").write_text(
            '[store]\nhierarchy = "model > benchmark"\n'
        )
        with pytest.raises(ValueError, match="mismatch"):
            extract.Store(root=root)

    def test_parse_hierarchy_strips_whitespace(self):
        assert _parse_hierarchy("  a > b  >c ") == ["a", "b", "c"]

    def test_parse_hierarchy_empty_level_raises(self):
        with pytest.raises(ValueError, match="empty level"):
            _parse_hierarchy("a > > b")


# ──────────────────────────────────────────────────────────────────────────
# Dict-spec experiment creation


class TestDictExperiment:
    def test_creates_path_in_hierarchy_order(self, tmp_store):
        # Dict keys in arbitrary order, path follows hierarchy config
        exp = tmp_store.experiment({
            "variant": "lr_0.01",
            "benchmark": "imagenet",
            "model": "resnet50",
        })
        assert exp.path == "imagenet/resnet50/lr_0.01"
        assert exp.name == "lr_0.01"

    def test_ancestors_have_correct_node_type(self, tmp_store):
        tmp_store.experiment({
            "benchmark": "imagenet",
            "model": "resnet50",
            "variant": "lr_0.01",
        })
        rows = tmp_store._conn.execute(
            "SELECT path, node_type FROM experiments ORDER BY path"
        ).fetchall()
        types = {r["path"]: r["node_type"] for r in rows}
        assert types["imagenet"] == "benchmark"
        assert types["imagenet/resnet50"] == "model"
        assert types["imagenet/resnet50/lr_0.01"] == "variant"

    def test_partial_spec_creates_partial_hierarchy(self, tmp_store):
        exp = tmp_store.experiment({
            "benchmark": "imagenet",
            "model": "resnet50",
        })
        assert exp.path == "imagenet/resnet50"
        assert exp.name == "resnet50"

    def test_reuses_existing_ancestors(self, tmp_store):
        tmp_store.experiment({"benchmark": "imagenet", "model": "resnet50", "variant": "v1"})
        tmp_store.experiment({"benchmark": "imagenet", "model": "resnet50", "variant": "v2"})
        count = tmp_store._conn.execute(
            "SELECT COUNT(*) FROM experiments WHERE path = 'imagenet'"
        ).fetchone()[0]
        assert count == 1  # imagenet created only once

    def test_skipped_level_raises(self, tmp_store):
        with pytest.raises(ValueError, match="Cannot skip"):
            tmp_store.experiment({"benchmark": "imagenet", "variant": "lr_0.01"})

    def test_unknown_level_raises(self, tmp_store):
        with pytest.raises(ValueError, match="Unknown hierarchy levels"):
            tmp_store.experiment({"benchmark": "imagenet", "dataset": "oops"})

    def test_empty_spec_raises(self, tmp_store):
        with pytest.raises(ValueError, match="at least one"):
            tmp_store.experiment({})

    def test_runs_work_on_dict_experiments(self, tmp_store):
        exp = tmp_store.experiment({
            "benchmark": "imagenet",
            "model": "resnet50",
            "variant": "lr_0.01",
        })
        with exp.run(config={"lr": 0.001}) as run:
            run.log(loss=0.5, accuracy=0.7)

        runs = exp.list_runs()
        assert len(runs) == 1
        assert runs[0]["status"] == "completed"


# ──────────────────────────────────────────────────────────────────────────
# Hard requirement: Store() must have config.toml


class TestMissingHierarchy:
    def test_raises_without_config_toml(self, tmp_path):
        store_root = tmp_path / ".extract"
        store_root.mkdir()
        # No config.toml at all
        with pytest.raises(MissingHierarchyError, match="config.toml"):
            extract.Store(root=store_root)

    def test_raises_with_empty_store_section(self, tmp_path):
        store_root = tmp_path / ".extract"
        store_root.mkdir()
        (store_root / "config.toml").write_text("[store]\n")
        with pytest.raises(MissingHierarchyError):
            extract.Store(root=store_root)

    def test_raises_with_no_store_section(self, tmp_path):
        store_root = tmp_path / ".extract"
        store_root.mkdir()
        (store_root / "config.toml").write_text("[other]\nkey = 1\n")
        with pytest.raises(MissingHierarchyError):
            extract.Store(root=store_root)

    def test_error_message_mentions_path(self, tmp_path):
        store_root = tmp_path / ".extract"
        store_root.mkdir()
        with pytest.raises(MissingHierarchyError) as exc_info:
            extract.Store(root=store_root)
        assert str(store_root) in str(exc_info.value)
        assert "extract init" in str(exc_info.value)


# ──────────────────────────────────────────────────────────────────────────
# Legacy API removal regressions


class TestLegacyAPIRemoved:
    def test_path_string_api_raises(self, tmp_store):
        # The legacy "string spec" branch is gone
        with pytest.raises((TypeError, AttributeError)):
            tmp_store.experiment("foo/bar/baz")  # type: ignore[arg-type]

    def test_experiment_by_path_method_gone(self):
        # The private method is removed entirely
        from extract.store import Store
        assert not hasattr(Store, "_experiment_by_path")

    def test_alter_table_migration_gone(self):
        """Grep store.py source for the dropped runtime migration."""
        from pathlib import Path
        src = Path(__file__).parent.parent / "src" / "extract" / "store.py"
        content = src.read_text()
        assert "ADD COLUMN node_type" not in content
