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
    # Clean existing data but preserve config.toml (except ensure hierarchy is set)
    config_path = STORE_ROOT / "config.toml"
    saved_config = config_path.read_text() if config_path.exists() else None
    if STORE_ROOT.exists():
        shutil.rmtree(STORE_ROOT)
    STORE_ROOT.mkdir(parents=True, exist_ok=True)
    if saved_config is not None:
        # Uncomment hierarchy if it was commented out
        import re
        saved_config = re.sub(
            r'^#\s*hierarchy\s*=',
            'hierarchy =',
            saved_config,
            flags=re.MULTILINE,
        )
        # Add [store] section with hierarchy if missing entirely
        if "hierarchy" not in saved_config:
            saved_config = '[store]\nhierarchy = "benchmark > model > variant"\n\n' + saved_config
        config_path.write_text(saved_config)
    else:
        config_path.write_text('[store]\nhierarchy = "benchmark > model > variant"\n')

    store = extract.Store(root=STORE_ROOT)

    # --- ImageNet experiments ---

    # ResNet50 variants
    with store.experiment({"benchmark": "imagenet", "model": "resnet50", "variant": "lr_0.01"}).run(
        config={"lr": 0.001, "weight_decay": 0.0}
    ) as run:
        # Categorical params logged alongside numeric metrics
        run.log(step=0, arch="resnet18", fisher_label="empirical")
        for step in range(50):
            run.log(step=step, loss=1.0 / (step + 1), accuracy=0.5 + 0.35 * (step / 49))

        # Log accuracy matrix (5 tasks, lower-triangular fill pattern)
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

    # Second run for lr_0.01 with different hyperparams (for comparison testing)
    with store.experiment({"benchmark": "imagenet", "model": "resnet50", "variant": "lr_0.01"}).run(
        config={"lr": 0.0005, "weight_decay": 0.0}
    ) as run:
        run.log(step=0, arch="resnet18", fisher_label="diagonal")
        for step in range(50):
            run.log(step=step, loss=0.95 / (step + 1), accuracy=0.5 + 0.38 * (step / 49))

        acc_matrix = np.array([
            [0.94, 0.00, 0.00, 0.00, 0.00],
            [0.87, 0.91, 0.00, 0.00, 0.00],
            [0.80, 0.84, 0.92, 0.00, 0.00],
            [0.74, 0.78, 0.85, 0.89, 0.00],
            [0.68, 0.73, 0.80, 0.84, 0.88],
        ])
        run.log_table("accuracy_matrix", acc_matrix, step=49,
                       axes={"rows": "evaluated_on", "cols": "trained_up_to"})

    with store.experiment({"benchmark": "imagenet", "model": "resnet50", "variant": "lr_0.005"}).run(
        config={"lr": 0.001, "weight_decay": 0.0, "use_momentum": True}
    ) as run:
        for step in range(50):
            run.log(step=step, loss=0.9 / (step + 1), accuracy=0.5 + 0.33 * (step / 49))

    # ViT Base
    with store.experiment({"benchmark": "imagenet", "model": "vit_base", "variant": "lr_0.001"}).run(
        config={"lr": 0.001, "weight_decay": 0.01}
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

    # ConvNeXt
    with store.experiment({"benchmark": "cifar10", "model": "convnext", "variant": "bs_64"}).run(
        config={"lr": 0.001, "batch_size": 500}
    ) as run:
        for step in range(50):
            run.log(step=step, loss=0.8 / (step + 1), accuracy=0.5 + 0.32 * (step / 49))

    # --- CIFAR10 experiments ---

    with store.experiment({"benchmark": "cifar10", "model": "convnext", "variant": "bs_128"}).run(
        config={"lr": 0.0005, "batch_size": 1000}
    ) as run:
        for step in range(30):
            run.log(step=step, loss=1.5 / (step + 1), accuracy=0.4 + 0.25 * (step / 29))

    with store.experiment({"benchmark": "cifar10", "model": "resnet50", "variant": "lr_0.005"}).run(
        config={"lr": 0.0005, "weight_decay": 0.01}
    ) as run:
        for step in range(30):
            run.log(step=step, loss=1.8 / (step + 1), accuracy=0.4 + 0.20 * (step / 29))

    # --- Phase 5: Models, Lineage, TODOs ---

    from ulid import ULID

    # Retrieve runs for model registration
    resnet_lr_runs = store.experiment(
        {"benchmark": "imagenet", "model": "resnet50", "variant": "lr_0.01"}
    ).list_runs()
    vit_runs = store.experiment(
        {"benchmark": "imagenet", "model": "vit_base", "variant": "lr_0.001"}
    ).list_runs()

    run_resnet_v1 = resnet_lr_runs[0]["id"] if resnet_lr_runs else None
    run_resnet_v2 = resnet_lr_runs[1]["id"] if len(resnet_lr_runs) > 1 else None
    run_vit_v1  = vit_runs[0]["id"] if vit_runs else None

    # Register 3 models via raw SQL
    model_resnet_v1  = str(ULID())
    model_resnet_v2  = str(ULID())
    model_vit_v1   = str(ULID())

    store._conn.executemany(
        "INSERT OR IGNORE INTO models (id, name, version, run_id, artifact_path, framework) "
        "VALUES (?, ?, ?, ?, ?, ?)",
        [
            (model_resnet_v1, "resnet-imagenet", "1.0", run_resnet_v1,
             "models/resnet-imagenet-v1.0.pt", "pytorch"),
            (model_resnet_v2, "resnet-imagenet", "2.0", run_resnet_v2,
             "models/resnet-imagenet-v2.0.pt", "pytorch"),
            (model_vit_v1,  "vit-default",  "1.0", run_vit_v1,
             "models/vit-default-v1.0.pt",  "pytorch"),
        ],
    )

    # Lineage edges
    lineage_rows = [
        # resnet v2.0 fine_tuned_from v1.0
        ("model", model_resnet_v1, "model", model_resnet_v2, "fine_tuned_from"),
        # vit v1.0 branched_from resnet v1.0
        ("model", model_resnet_v1, "model", model_vit_v1,  "branched_from"),
    ]
    if run_resnet_v1:
        lineage_rows.append(("run", run_resnet_v1, "model", model_resnet_v1, "produced"))
    if run_resnet_v2:
        lineage_rows.append(("run", run_resnet_v2, "model", model_resnet_v2, "produced"))
    if run_vit_v1:
        lineage_rows.append(("run", run_vit_v1,  "model", model_vit_v1,  "produced"))

    store._conn.executemany(
        "INSERT OR IGNORE INTO lineage "
        "(parent_type, parent_id, child_type, child_id, relation) "
        "VALUES (?, ?, ?, ?, ?)",
        lineage_rows,
    )
    store._conn.commit()

    # Global TODOs
    store.todo("Tune ResNet50 learning rate on ImageNet", priority=2)
    store.todo("Run ablation: batch size comparison", priority=1)
    store.todo("Add ViT with cosine LR schedule", priority=0)

    # Experiment-scoped TODOs via raw SQL
    resnet_exp = store.experiment({"benchmark": "imagenet", "model": "resnet50", "variant": "lr_0.01"})
    vit_exp  = store.experiment({"benchmark": "imagenet", "model": "vit_base", "variant": "lr_0.001"})

    store._conn.executemany(
        "INSERT INTO todos (id, scope_type, scope_id, content, priority) "
        "VALUES (?, 'experiment', ?, ?, ?)",
        [
            (str(ULID()), resnet_exp.id, "Compare v1.0 vs v2.0 final accuracy", 1),
            (str(ULID()), vit_exp.id,  "Profile memory usage on validation split", 0),
        ],
    )
    store._conn.commit()

    store.close()
    print(f"Generated test data at {STORE_ROOT}")
    print(f"  Hierarchy: benchmark > model > variant")

    # Verify
    import sqlite3
    conn = sqlite3.connect(str(STORE_ROOT / "extract.db"))
    conn.row_factory = sqlite3.Row

    # Leaf experiments = nodes with no children (i.e. not referenced as parent_id)
    leaves = conn.execute(
        "SELECT e.path, e.node_type FROM experiments e "
        "WHERE NOT EXISTS (SELECT 1 FROM experiments c WHERE c.parent_id = e.id) "
        "ORDER BY e.path"
    ).fetchall()
    runs    = conn.execute("SELECT COUNT(*) FROM runs").fetchone()[0]
    models  = conn.execute("SELECT COUNT(*) FROM models").fetchone()[0]
    lineage = conn.execute("SELECT COUNT(*) FROM lineage").fetchone()[0]
    todos   = conn.execute("SELECT COUNT(*) FROM todos").fetchone()[0]

    print(f"  Experiments: {len(leaves)}, Runs: {runs}, Models: {models}, Lineage edges: {lineage}, TODOs: {todos}")
    for e in leaves:
        run_count = conn.execute(
            "SELECT COUNT(*) FROM runs r JOIN experiments x ON r.experiment_id = x.id "
            "WHERE x.path = ?", (e['path'],)
        ).fetchone()[0]
        print(f"    {e['path']:<45} [{run_count} runs]")
    conn.close()


if __name__ == "__main__":
    main()
