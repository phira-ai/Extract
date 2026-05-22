# Configuration

Edit `.extract/config.toml`. `extract init` writes the initial file.

## Store setup

```toml
[store]
hierarchy = "benchmark > model > variant"
```

Hierarchy levels define how experiments are created from dict specs:

```python
store.experiment({
    "benchmark": "imagenet",
    "model": "resnet50",
    "variant": "lr_0.01",
})
```

## Summary tab

Controls the Detail panel Summary tab (`S`).

```toml
[summary]
sections = ["runs", "metrics", "tables", "curves"]
curve_width = 80       # chart width as % of panel
# curve_height = 10    # chart height in lines; default auto-scales by metric count
curve_smooth = false
```

## Info tab and config fields

Controls the Detail panel Info tab (`I`) and config sections in Compare/Diff views.
Nested configs are flattened with dot notation, e.g. `model.lora_r`, `task.num_train_epochs`.

```toml
[info]
fields = ["model.*", "task.num_train_epochs"]
time_format = "%Y-%m-%d %H:%M:%S"
```

Glob syntax:

- `*` matches one segment.
- `**` matches multiple segments.
- `?` matches one character.
- `{a,b}` matches alternatives.
- Prefix with `!` to exclude, e.g. `["model.**", "!model.parent"]`.

Empty `fields` means show all.

## Compare view

Controls Compare view (`c` with marked runs).

```toml
[compare]
sections = ["pivot", "config", "tables", "curves"]
curve_width = 50
# curve_height = 10
```

## Metric interpretation

```toml
[metrics]
minimize = ["forgetting_rate"]
maximize = ["custom_score"]
order = "alpha"
```

Unlisted metrics use name heuristics. For example, names containing `loss` are minimized.

`order` supports:

- `alpha`
- `rev_alpha`
- explicit order such as `"accuracy > loss > f1"`

## Table highlighting

First matching rule wins.

```toml
[[tables.highlight]]
min = 0.7
color = "red"
```

Rule fields:

- `eq` — exact match
- `min` — inclusive lower bound
- `max` — exclusive upper bound
- `pattern` — substring match
- `color` — named color or hex

Colors: `red`, `green`, `yellow`, `blue`, `cyan`, `magenta`, `white`, `orange`, `none`, or hex such as `#ff6600`.

## Tags

Predefine tags and colors:

```toml
[[tags.definitions]]
name = "baseline"
color = "blue"

[[tags.definitions]]
name = "production"
color = "#a6e3a1"

[[tags.definitions]]
name = "deprecated"
color = "red"
```

Press `t` in the TUI to open the tag picker. Type to fuzzy-filter, Enter to toggle, or create new tags.

## Theme

```toml
[theme]
fg = "#cdd6f4"
bg = "#1e1e2e"
accent = "#89b4fa"
accent_dim = "#585b70"
success = "#a6e3a1"
warning = "#f9e2af"
error = "#f38ba8"
border = "#585b70"
border_focused = "#89b4fa"

[notifications]
timeout = 3
```
