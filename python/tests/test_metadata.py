"""Tests for post-hoc tagging/notes and auto-unarchive."""

import json
from pathlib import Path

import pytest

from extract import Store


@pytest.fixture
def store(tmp_path):
    config_dir = tmp_path / ".extract"
    config_dir.mkdir()
    (config_dir / "config.toml").write_text(
        '[store]\nhierarchy = "benchmark > method > variant"\n'
    )
    return Store(root=str(config_dir))


def test_tag_finished_run(store):
    exp = store.experiment({"benchmark": "cifar", "method": "ewc"})
    with exp.run(config={"lr": 0.01}) as run:
        run_id = run.id
    # Post-hoc tagging on a finished run
    r = store.get_run(run_id)
    r.tag("baseline", "v1")
    with store.lock:
        row = store._conn.execute(
            "SELECT tags FROM runs WHERE id = ?", (run_id,)
        ).fetchone()
    tags = json.loads(row["tags"])
    assert "baseline" in tags
    assert "v1" in tags


def test_note_finished_run(store):
    exp = store.experiment({"benchmark": "cifar", "method": "ewc"})
    with exp.run(config={"lr": 0.01}) as run:
        run_id = run.id
    r = store.get_run(run_id)
    r.note("good convergence")
    r.note("try lower lr next")
    with store.lock:
        row = store._conn.execute(
            "SELECT notes FROM runs WHERE id = ?", (run_id,)
        ).fetchone()
    assert "good convergence" in row["notes"]
    assert "try lower lr next" in row["notes"]


def test_auto_unarchive_ancestors(store):
    _ = store.experiment({"benchmark": "cifar", "method": "ewc", "variant": "lr_0.1"})
    _ = store.experiment({"benchmark": "cifar", "method": "ewc", "variant": "lr_0.01"})

    # Archive the method node (ewc) and its children manually
    with store.lock:
        store._conn.execute(
            "UPDATE experiments SET status = 'archived' WHERE name = 'ewc'"
        )
        store._conn.execute(
            "UPDATE experiments SET status = 'archived' WHERE name = 'lr_0.1'"
        )
        store._conn.execute(
            "UPDATE experiments SET status = 'archived' WHERE name = 'lr_0.01'"
        )
        store._conn.commit()

    # Now create a new variant under the archived ewc
    _ = store.experiment({"benchmark": "cifar", "method": "ewc", "variant": "lr_0.001"})

    with store.lock:
        # ewc should be unarchived (on the path)
        ewc = store._conn.execute(
            "SELECT status FROM experiments WHERE name = 'ewc'"
        ).fetchone()
        assert ewc["status"] == "active"

        # lr_0.1 and lr_0.01 should still be archived (not on the path)
        lr01 = store._conn.execute(
            "SELECT status FROM experiments WHERE name = 'lr_0.1'"
        ).fetchone()
        assert lr01["status"] == "archived"

        lr001 = store._conn.execute(
            "SELECT status FROM experiments WHERE name = 'lr_0.01'"
        ).fetchone()
        assert lr001["status"] == "archived"


def test_migration_idempotent(store):
    # Opening a second Store on the same path should not fail
    Store(root=str(store.root))
