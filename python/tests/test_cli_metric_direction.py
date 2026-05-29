from __future__ import annotations

import json
import subprocess
import sys

import extract


def test_cli_compare_uses_metrics_config_direction(tmp_path):
    root = tmp_path / ".extract"
    root.mkdir()
    (root / "config.toml").write_text(
        '[store]\nhierarchy = "benchmark > model > variant"\n\n'
        '[metrics]\nminimize = ["AF"]\nmaximize = ["AP"]\n'
    )
    store = extract.Store(root=root)
    exp = store.experiment(
        {"benchmark": "bench", "model": "model", "variant": "variant"}
    )
    with exp.run(name="low-af") as low:
        low.log(AF=1.0, AP=1.0)
    with exp.run(name="high-af") as high:
        high.log(AF=2.0, AP=2.0)
    store.close()

    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "extract",
            "runs",
            "compare",
            low.id,
            high.id,
            "--store",
            str(root),
        ],
        check=True,
        capture_output=True,
        text=True,
    )
    metrics = json.loads(result.stdout)["metrics"]

    assert metrics["AF"]["direction"] == "min"
    assert metrics["AF"]["ranking"] == [low.id, high.id]
    assert metrics["AP"]["direction"] == "max"
    assert metrics["AP"]["ranking"] == [high.id, low.id]
