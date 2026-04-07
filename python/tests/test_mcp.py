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
