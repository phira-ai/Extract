#!/usr/bin/env python3
"""Generate .extract/ test data with the typed hierarchy API."""

import shutil
import sys
from pathlib import Path

# Add the Python SDK to the path
sys.path.insert(0, str(Path(__file__).parent.parent / "python" / "src"))

import numpy as np

import extract

STORE_ROOT = Path(__file__).parent.parent / ".extract"

def main():
    # Clean existing data
    if STORE_ROOT.exists():
        shutil.rmtree(STORE_ROOT)

    store = extract.Store(root=STORE_ROOT, hierarchy="benchmark > method > variant")

    # --- CIFAR-100 experiments ---

    # EWC variants
    with store.experiment({"benchmark": "cifar100", "method": "ewc", "variant": "lambda_1.0"}).run(
        config={"lr": 0.001, "lambda": 1.0}
    ) as run:
        for step in range(50):
            run.log(step=step, loss=1.0 / (step + 1), accuracy=0.5 + 0.35 * (step / 49))

        # Log accuracy matrix (5 tasks, lower-triangular pattern for CL)
        acc_matrix = np.array([
            [0.92, 0.00, 0.00, 0.00, 0.00],
            [0.85, 0.88, 0.00, 0.00, 0.00],
            [0.78, 0.82, 0.90, 0.00, 0.00],
            [0.71, 0.75, 0.83, 0.87, 0.00],
            [0.65, 0.70, 0.78, 0.82, 0.85],
        ])
        run.log_table("accuracy_matrix", acc_matrix, step=49,
                       axes={"rows": "evaluated_on", "cols": "trained_up_to"})

        # Log loss timeseries artifact
        steps_list = list(range(50))
        loss_values = [1.0 / (s + 1) for s in steps_list]
        run.log_timeseries("loss_curve", steps_list, loss_values)

    with store.experiment({"benchmark": "cifar100", "method": "ewc", "variant": "online_ewc"}).run(
        config={"lr": 0.001, "lambda": 1.0, "online": True}
    ) as run:
        for step in range(50):
            run.log(step=step, loss=0.9 / (step + 1), accuracy=0.5 + 0.33 * (step / 49))

    # SI
    with store.experiment({"benchmark": "cifar100", "method": "si", "variant": "c_0.5"}).run(
        config={"lr": 0.001, "c": 0.5}
    ) as run:
        for step in range(50):
            run.log(step=step, loss=1.2 / (step + 1), accuracy=0.5 + 0.30 * (step / 49))

        acc_matrix = np.array([
            [0.88, 0.00, 0.00, 0.00, 0.00],
            [0.80, 0.84, 0.00, 0.00, 0.00],
            [0.72, 0.76, 0.86, 0.00, 0.00],
            [0.65, 0.69, 0.78, 0.83, 0.00],
            [0.58, 0.63, 0.72, 0.77, 0.80],
        ])
        run.log_table("accuracy_matrix", acc_matrix, step=49,
                       axes={"rows": "evaluated_on", "cols": "trained_up_to"})

        steps_list = list(range(50))
        loss_values = [1.2 / (s + 1) for s in steps_list]
        run.log_timeseries("loss_curve", steps_list, loss_values)

    # Replay
    with store.experiment({"benchmark": "cifar100", "method": "replay", "variant": "buffer_500"}).run(
        config={"lr": 0.001, "buffer_size": 500}
    ) as run:
        for step in range(50):
            run.log(step=step, loss=0.8 / (step + 1), accuracy=0.5 + 0.32 * (step / 49))

    # --- TinyImageNet experiments ---

    with store.experiment({"benchmark": "tinyimagenet", "method": "replay", "variant": "buffer_1000"}).run(
        config={"lr": 0.0005, "buffer_size": 1000}
    ) as run:
        for step in range(30):
            run.log(step=step, loss=1.5 / (step + 1), accuracy=0.4 + 0.25 * (step / 29))

    with store.experiment({"benchmark": "tinyimagenet", "method": "ewc", "variant": "lambda_0.5"}).run(
        config={"lr": 0.0005, "lambda": 0.5}
    ) as run:
        for step in range(30):
            run.log(step=step, loss=1.8 / (step + 1), accuracy=0.4 + 0.20 * (step / 29))

    store.close()
    print(f"Generated test data at {STORE_ROOT}")
    print(f"  Hierarchy: benchmark > method > variant")

    # Verify
    import sqlite3
    conn = sqlite3.connect(str(STORE_ROOT / "extract.db"))
    conn.row_factory = sqlite3.Row
    exps = conn.execute("SELECT path, node_type FROM experiments ORDER BY path").fetchall()
    print(f"  Experiments: {len(exps)}")
    for e in exps:
        print(f"    {e['path']:<40} {e['node_type'] or ''}")
    runs = conn.execute("SELECT COUNT(*) FROM runs").fetchone()[0]
    print(f"  Runs: {runs}")
    conn.close()


if __name__ == "__main__":
    main()
