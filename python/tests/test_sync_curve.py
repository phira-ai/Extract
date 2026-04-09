"""Round-trip tests: curve_points and total_steps must propagate through extract sync."""

from __future__ import annotations

import pytest

import extract
from extract.sync import merge_db


def _bootstrap(root):
    root.mkdir(parents=True, exist_ok=True)
    (root / "config.toml").write_text(
        '[store]\nhierarchy = "benchmark > model > variant"\n'
    )


@pytest.fixture
def two_stores(tmp_path):
    """Create two empty stores. Returns (src_root, dst_root) Paths."""
    src_root = tmp_path / "src" / ".extract"
    dst_root = tmp_path / "dst" / ".extract"
    _bootstrap(src_root)
    _bootstrap(dst_root)
    # Open and close both stores so the schema is created and the DB files exist on disk.
    extract.Store(root=src_root).close()
    extract.Store(root=dst_root).close()
    return src_root, dst_root


class TestSyncCurvePoints:
    def test_curve_points_round_trip(self, two_stores):
        src_root, dst_root = two_stores

        # Write some curves into the source.
        src = extract.Store(root=src_root)
        try:
            exp = src.experiment(
                {"benchmark": "imagenet", "model": "resnet50", "variant": "lr_0.01"}
            )
            with exp.run(config={"lr": 0.01}, total_steps=5, name="src-run") as run:
                for s in range(5):
                    run.curve(step=s, loss=1.0 - 0.1 * s)
                run.log(accuracy=0.9)  # also a headline metric
        finally:
            src.close()

        # Merge src into dst.
        stats = merge_db(src_root / "extract.db", dst_root / "extract.db")

        # Verify curves landed in dst.
        dst = extract.Store(root=dst_root)
        try:
            rows = dst._conn.execute(
                "SELECT name, step, value FROM curve_points ORDER BY step"
            ).fetchall()
            assert len(rows) == 5
            assert rows[0]["name"] == "loss"
            assert rows[0]["step"] == 0
            assert rows[0]["value"] == pytest.approx(1.0)
            assert rows[4]["value"] == pytest.approx(0.6)

            # Headline scalar_metrics row also propagated.
            scalars = dst._conn.execute(
                "SELECT name, value FROM scalar_metrics"
            ).fetchall()
            assert len(scalars) == 1
            assert scalars[0]["name"] == "accuracy"
        finally:
            dst.close()

        # Stats dict should report the new table.
        assert stats.get("curve_points", 0) == 5

    def test_total_steps_round_trip(self, two_stores):
        """The runs INSERT in sync.py must include total_steps in its column list."""
        src_root, dst_root = two_stores

        src = extract.Store(root=src_root)
        try:
            exp = src.experiment(
                {"benchmark": "imagenet", "model": "resnet50", "variant": "lr_0.01"}
            )
            with exp.run(config={"lr": 0.01}, total_steps=1234, name="src-run") as run:
                pass
        finally:
            src.close()

        merge_db(src_root / "extract.db", dst_root / "extract.db")

        dst = extract.Store(root=dst_root)
        try:
            row = dst._conn.execute(
                "SELECT total_steps FROM runs WHERE name = 'src-run'"
            ).fetchone()
            assert row is not None
            assert row["total_steps"] == 1234
        finally:
            dst.close()
