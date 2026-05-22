# TUI Guide

Launch:

```bash
extract tui
extract tui --store path/to/.extract
```

The TUI reads SQLite while training scripts write. Visible live runs show a `● LIVE` badge, and charts update when SQLite `data_version` changes.

## Common workflow

1. Start training script.
2. Run `extract tui` in another terminal.
3. Navigate to a leaf experiment.
4. Watch curves fill in as `run.curve(...)` writes points.
5. Mark runs with `Space`.
6. Press `c` to compare or `d` to diff configs.

## Navigation

| Key | Action |
|-----|--------|
| `j` / `k` | Move down / up |
| `gg` / `G` | Jump to top / bottom |
| `Enter` | Expand / select |
| `h` / `l` | Cycle panels, same as Tab / Shift+Tab |
| `Tab` / `Shift+Tab` | Cycle panels |
| `1` / `2` / `3` | Focus Tree / Detail / Selection panel |
| `/` | Search experiments and runs |
| `?` | Help overlay |
| `q` | Quit |

## Experiment tree

| Key | Action |
|-----|--------|
| `Left` | Go to parent node |
| `Right` | Go to first child / enter leaf |
| `Space` | Mark run for comparison |

## Detail panel

| Key | Action |
|-----|--------|
| `Left` / `Right` | Cycle through runs |
| `S` | Summary tab |
| `I` | Info tab |

## Actions

| Key | Action |
|-----|--------|
| `t` | Edit tags on Summary tab |
| `n` | Append note |
| `Ctrl+E` | Edit notes in `$EDITOR` |
| `Shift+F` | Mark run failed |
| `Shift+C` | Mark run completed |
| `Shift+A` | Archive run / experiment |
| `Shift+U` | Unarchive |
| `Shift+H` | Toggle show archived |

## Runs and comparison

| Key | Action |
|-----|--------|
| `r` | Open run browser |
| `c` | Compare marked runs |
| `d` | Diff marked runs by config |
| `x` | Delete run |

## Views

| Key | Action |
|-----|--------|
| `M` | Model registry |
| `T` | TODOs |
| `L` | Lineage DAG |

## TODO view

| Key | Action |
|-----|--------|
| `Space` | Toggle done |
| `a` | Add TODO |
| `x` | Delete TODO |
| `0` / `1` / `2` | Set priority: low / mid / high |
| `A` / `G` / `E` / `R` | Filter: All / Global / Experiment / Run |
