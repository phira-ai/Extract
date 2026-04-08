"""Tests for Run.curve() streaming API and Experiment.run(total_steps=)."""

from __future__ import annotations

import time

import pytest

import extract


def _bootstrap(root, hierarchy="benchmark > model > variant"):
    """Helper: create root/config.toml with the given hierarchy line."""
    root.mkdir(parents=True, exist_ok=True)
    (root / "config.toml").write_text(f'[store]\nhierarchy = "{hierarchy}"\n')


@pytest.fixture
def tmp_store(tmp_path):
    root = tmp_path / ".extract"
    _bootstrap(root)
    return extract.Store(root=root)


# ──────────────────────────────────────────────────────────────────────────
# total_steps declaration


class TestTotalStepsDeclaration:
    def test_total_steps_persisted_on_run_open(self, tmp_store):
        exp = tmp_store.experiment(
            {"benchmark": "imagenet", "model": "resnet50", "variant": "lr_0.01"}
        )
        with exp.run(config={"lr": 0.01}, total_steps=1000) as run:
            run_id = run.id

        row = tmp_store._conn.execute(
            "SELECT total_steps FROM runs WHERE id = ?", (run_id,)
        ).fetchone()
        assert row["total_steps"] == 1000

    def test_total_steps_optional_defaults_null(self, tmp_store):
        exp = tmp_store.experiment(
            {"benchmark": "imagenet", "model": "resnet50", "variant": "lr_0.01"}
        )
        with exp.run(config={"lr": 0.01}) as run:
            run_id = run.id

        row = tmp_store._conn.execute(
            "SELECT total_steps FROM runs WHERE id = ?", (run_id,)
        ).fetchone()
        assert row["total_steps"] is None
