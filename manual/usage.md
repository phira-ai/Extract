# Usage

Extract tracks experiments in a project-local `.extract/` store. Use the Python SDK inside training scripts and the Rust TUI in another terminal.

## Initialize a store

```bash
extract init
```

Choose a hierarchy such as:

```text
benchmark > model > variant
```

This creates `.extract/config.toml`.

## Log a run

```python
from extract import Store

store = Store()
exp = store.experiment({
    "benchmark": "imagenet",
    "model": "resnet50",
    "variant": "lr_0.01",
})

with exp.run(config={"lr": 0.01}, name="seed-1", total_steps=1000) as run:
    for step in range(1000):
        loss, acc = train_step(...)
        run.curve(step=step, train_loss=loss, train_acc=acc)

    run.log(final_loss=loss, final_acc=acc)
```

`total_steps` lets the TUI pin chart x-axes from the start.

## Experiments

`Store.experiment()` accepts a dict keyed by your configured hierarchy.

```python
exp = store.experiment({
    "benchmark": "mmlu",
    "model": "llama-3.2-1b",
    "variant": "lora-r16",
})
```

Keys must be hierarchy levels. Values must not skip levels.

## Metrics

Use two metric APIs:

```python
run.curve(step=step, train_loss=loss, eval_acc=acc)
run.log(best_acc=best_acc, final_loss=final_loss)
```

- `curve()` stores dense time-series in `curve_points`; TUI live charts read these.
- `log()` stores headline metrics in `scalar_metrics`; summaries, rankings, and comparisons read these.

## Params

String values passed to `log()` become run-level categorical params.

```python
run.log(optimizer="adamw", schedule="cosine")
```

## Artifacts

```python
run.log_table("confusion_matrix", matrix)
run.log_text("notes", "Calibration improved after warmup.")
```

Tables are saved as `.npy`; text artifacts are saved as Markdown.

## Tags and notes

```python
run.tag("baseline", "production-candidate")
run.note("Good accuracy, but slower convergence.")
```

The TUI can edit tags and append notes after runs finish.

## Models

```python
run.register_model(
    name="resnet50",
    version="v1",
    path="checkpoints/best.pt",
    metadata={"dataset": "imagenet"},
    framework="pytorch",
)
```

Model files are copied into `.extract/models/{name}/{version}/`.

## Lineage

```python
run.derived_from(run=parent_run_id)
run.derived_from(model="resnet50", version="v1")
run.branched_from(experiment=experiment_id)
```

Open lineage view with `L` in the TUI.

## TODOs

```python
store.todo("Rerun baseline with seed 2", priority=1)
run.todo("Inspect confusion matrix", priority=2)
```

Open TODO view with `T` in the TUI.
