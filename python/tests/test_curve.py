"""Tests for Run.curve() streaming API and Experiment.run(total_steps=)."""

from __future__ import annotations

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


# ──────────────────────────────────────────────────────────────────────────
# Run.curve() streaming API


def _make_run(store, total_steps=None):
    exp = store.experiment(
        {"benchmark": "imagenet", "model": "resnet50", "variant": "lr_0.01"}
    )
    return exp.run(config={"lr": 0.01}, total_steps=total_steps)


class TestRunCurveBasic:
    def test_curve_writes_to_curve_points_table(self, tmp_store):
        run = _make_run(tmp_store, total_steps=10)
        with run as r:
            r.curve(step=0, train_loss=1.0)
            r.curve(step=1, train_loss=0.8)

        rows = tmp_store._conn.execute(
            "SELECT step, name, value FROM curve_points WHERE run_id = ? ORDER BY step",
            (run.id,),
        ).fetchall()
        assert len(rows) == 2
        assert rows[0]["step"] == 0
        assert rows[0]["name"] == "train_loss"
        assert rows[0]["value"] == 1.0
        assert rows[1]["value"] == 0.8

    def test_curve_supports_multiple_metrics_per_step(self, tmp_store):
        run = _make_run(tmp_store, total_steps=10)
        with run as r:
            r.curve(step=0, train_loss=1.0, accuracy=0.5)

        rows = tmp_store._conn.execute(
            "SELECT name, value FROM curve_points WHERE run_id = ? ORDER BY name",
            (run.id,),
        ).fetchall()
        assert len(rows) == 2
        assert rows[0]["name"] == "accuracy"
        assert rows[0]["value"] == 0.5
        assert rows[1]["name"] == "train_loss"
        assert rows[1]["value"] == 1.0

    def test_curve_does_not_pollute_scalar_metrics(self, tmp_store):
        """The whole point of the split: curve() data must NOT appear in scalar_metrics."""
        run = _make_run(tmp_store, total_steps=10)
        with run as r:
            r.curve(step=0, train_loss=1.0)
            r.curve(step=1, train_loss=0.8)

        rows = tmp_store._conn.execute(
            "SELECT * FROM scalar_metrics WHERE run_id = ?", (run.id,)
        ).fetchall()
        assert len(rows) == 0

    def test_log_does_not_pollute_curve_points(self, tmp_store):
        """And vice versa — log() must NOT appear in curve_points."""
        run = _make_run(tmp_store, total_steps=10)
        with run as r:
            r.log(Cl=0.7, Fgt=0.1)

        rows = tmp_store._conn.execute(
            "SELECT * FROM curve_points WHERE run_id = ?", (run.id,)
        ).fetchall()
        assert len(rows) == 0

    def test_curve_rejects_string_values(self, tmp_store):
        run = _make_run(tmp_store, total_steps=10)
        with run as r:
            with pytest.raises(TypeError, match="numeric"):
                r.curve(step=0, label="not a number")

    def test_curve_rejects_bool_values(self, tmp_store):
        """bool is a subclass of int — must be rejected explicitly."""
        run = _make_run(tmp_store, total_steps=10)
        with run as r:
            with pytest.raises(TypeError, match="numeric"):
                r.curve(step=0, finished=True)

    def test_curve_after_finish_raises(self, tmp_store):
        run = _make_run(tmp_store, total_steps=10)
        with run as r:
            r.curve(step=0, loss=1.0)
        with pytest.raises(RuntimeError, match="finished"):
            run.curve(step=1, loss=0.9)


class TestRunCurveBuffering:
    def test_curve_flushes_at_threshold(self, tmp_store):
        """Buffer should flush automatically once it hits _CURVE_FLUSH_THRESHOLD."""
        from extract import run as run_mod

        run = _make_run(tmp_store, total_steps=100)
        # Bypass the context manager so we can inspect the buffer state
        # without triggering the on-exit flush.
        try:
            # Write threshold-1 points; nothing should be flushed yet.
            for i in range(run_mod._CURVE_FLUSH_THRESHOLD - 1):
                run.curve(step=i, loss=float(i))
            count_before = tmp_store._conn.execute(
                "SELECT COUNT(*) FROM curve_points WHERE run_id = ?", (run.id,)
            ).fetchone()[0]
            assert count_before == 0

            # One more point — should trigger a flush.
            run.curve(step=run_mod._CURVE_FLUSH_THRESHOLD - 1, loss=99.0)
            count_after = tmp_store._conn.execute(
                "SELECT COUNT(*) FROM curve_points WHERE run_id = ?", (run.id,)
            ).fetchone()[0]
            assert count_after == run_mod._CURVE_FLUSH_THRESHOLD
        finally:
            run.finish()

    def test_curve_finish_flushes_remaining(self, tmp_store):
        run = _make_run(tmp_store, total_steps=10)
        with run as r:
            r.curve(step=0, loss=1.0)
            r.curve(step=1, loss=0.9)
            # No automatic flush yet (below threshold).
            count_before = tmp_store._conn.execute(
                "SELECT COUNT(*) FROM curve_points WHERE run_id = ?", (r.id,)
            ).fetchone()[0]
            assert count_before == 0
        # After finish (context exit), everything should be flushed.
        count_after = tmp_store._conn.execute(
            "SELECT COUNT(*) FROM curve_points WHERE run_id = ?", (run.id,)
        ).fetchone()[0]
        assert count_after == 2

    def test_curve_wall_clock_flush(self, tmp_store, monkeypatch):
        """Sparse logging should still flush within the wall-clock window."""
        from extract import run as run_mod

        # Use a fake clock so the test is deterministic and instant.
        fake_now = [1000.0]
        monkeypatch.setattr(run_mod.time, "monotonic", lambda: fake_now[0])

        run = _make_run(tmp_store, total_steps=1000)
        try:
            run.curve(step=0, loss=1.0)
            count = tmp_store._conn.execute(
                "SELECT COUNT(*) FROM curve_points WHERE run_id = ?", (run.id,)
            ).fetchone()[0]
            assert count == 0  # below threshold, not yet flushed

            # Advance the clock past the wall-clock window.
            fake_now[0] += run_mod._CURVE_FLUSH_INTERVAL_SEC + 0.1
            run.curve(step=1, loss=0.9)

            count = tmp_store._conn.execute(
                "SELECT COUNT(*) FROM curve_points WHERE run_id = ?", (run.id,)
            ).fetchone()[0]
            assert count == 2  # both points now flushed
        finally:
            run.finish()


class TestRunLogNoStep:
    def test_log_rejects_step_kwarg(self, tmp_store):
        """Passing step= to log() must raise TypeError with a migration message."""
        run = _make_run(tmp_store)
        with run as r:
            with pytest.raises(TypeError, match="no longer accepts a 'step' argument"):
                r.log(step=0, final_acc=0.9)

    def test_log_overwrites_on_relog(self, tmp_store):
        """Re-logging the same metric name overwrites the previous value."""
        run = _make_run(tmp_store)
        with run as r:
            r.log(final_acc=0.5)
            r.log(final_acc=0.9)

        rows = tmp_store._conn.execute(
            "SELECT step, value FROM scalar_metrics WHERE run_id = ? AND name = 'final_acc'",
            (run.id,),
        ).fetchall()
        assert len(rows) == 1
        assert rows[0]["step"] == 0
        assert rows[0]["value"] == 0.9

    def test_log_multiple_metrics_at_once(self, tmp_store):
        """log() should handle multiple kwargs in a single call."""
        run = _make_run(tmp_store)
        with run as r:
            r.log(Cl=0.7, Fgt=0.1, final_loss=0.3)

        rows = tmp_store._conn.execute(
            "SELECT name, value FROM scalar_metrics WHERE run_id = ? ORDER BY name",
            (run.id,),
        ).fetchall()
        names = [r["name"] for r in rows]
        assert names == ["Cl", "Fgt", "final_loss"]
