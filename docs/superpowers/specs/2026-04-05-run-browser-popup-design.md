# Run Browser Popup

## Summary

Add a centered popup triggered by `r` that lists all runs for the currently selected experiment, allowing the user to quickly navigate to a specific run. Includes inline `/` search filtering and `x` to delete runs. Also adds keystroke hint footers to all interactive popups.

## Trigger

- Key: `r` in explorer view
- Condition: current tree node is a leaf experiment with multiple runs
- If experiment has 0-1 runs, `r` is a no-op

## State

New `RunBrowserState` struct in `popup.rs`:

```rust
pub struct RunBrowserState {
    pub experiment_name: String,
    pub experiment_id: String,
    pub runs: Vec<Run>,
    pub filtered: Vec<usize>,       // indices into `runs` matching filter
    pub cursor: usize,              // position within `filtered`
    pub search_query: Option<String>, // None = not searching, Some = search active
    pub scroll_offset: usize,       // for scrolling long lists
}
```

Added to `AppState`:
```rust
pub run_browser: Option<RunBrowserState>,
```

## Rendering

- Centered rect using existing `centered_rect()` utility
- Width: 60 columns
- Height: dynamic — `min(filtered.len() + header + footer + search_line, screen_height - 4)`
- Border: rounded, accent-colored title `" {experiment_name} — runs "`
- Clear widget underneath (same pattern as RunPickerState)

### Row format

Each run row displays:
- Run name (fall back to run ID if name is None)
- Status
- `ended_at` date (or "running" if None)

Cursor row highlighted with accent background.

### Search line

When `search_query` is `Some`:
- Rendered below the title, above the run list
- Format: `/ query_` with blinking cursor
- Filtered list updates in real-time as user types

### Footer

Keystroke hints rendered as the last line inside the popup border, dimmed text:

- **Normal mode:** `j/k navigate  Enter select  / search  x delete  Esc close`
- **Search mode:** `Type to filter  Enter confirm  Esc cancel`

## Key Handling

Handled in `layout.rs` popup dispatch chain, checked after delete_confirm and before/alongside run_picker.

### Normal mode (search_query is None)

| Key | Action |
|-----|--------|
| `j` / `↓` | Move cursor down (wrap or clamp) |
| `k` / `↑` | Move cursor up (wrap or clamp) |
| `Enter` | Select run: set `current_run_index`, refresh detail, close popup |
| `/` | Enter search mode: set `search_query = Some("")` |
| `x` | Open delete confirmation for cursor run |
| `Esc` | Close popup |

### Search mode (search_query is Some)

| Key | Action |
|-----|--------|
| Printable char | Append to query, re-filter, reset cursor to 0 |
| `Backspace` | Remove last char from query, re-filter |
| `Enter` | Exit search mode (keep filter applied), return to normal mode |
| `Esc` | Clear search query, restore full run list, return to normal mode |
| `j` / `↓` | Move cursor down within filtered results |
| `k` / `↑` | Move cursor up within filtered results |

### Filtering logic

Case-insensitive substring match across:
- `run.name`
- `run.tags`
- `run.notes`
- `run.status`
- `run.config`

A run matches if any field contains the query substring.

## Integration

### Opening the popup

In `layout.rs`, when `r` is pressed in explorer view:
1. Check current tree node is a leaf experiment
2. Load runs for that experiment (already available in `app.runs`)
3. If runs.len() > 1, create `RunBrowserState` with all runs, `filtered` = all indices
4. Set `app.run_browser = Some(state)`

### Selecting a run

On `Enter`:
1. Get the run at `filtered[cursor]`
2. Set `app.current_run_index` to the index of that run in `app.runs`
3. Refresh detail panel data
4. Set `app.run_browser = None`

### Deleting a run

On `x`:
1. Get the run at `filtered[cursor]`
2. Open `DeleteConfirmState` for that run (existing flow)
3. On confirmation, delete from DB, refresh runs list, update `RunBrowserState` (remove from runs and filtered, adjust cursor)

### Dispatch priority

```
1. Search popup (/)
2. Help overlay (?)
3. Delete confirmation
4. Run browser (r)      <-- new
5. Run picker (Space)
6. Global keys (gg/G)
7. View-specific keys
```

## Keystroke Hints for Existing Popups

### Run Picker (compare, Space)

Add footer line inside border:
`j/k navigate  Space toggle  Enter confirm  Esc close`

### Delete Confirmation

Already shows `[y] confirm  [esc] cancel` — no change needed.

## Files to modify

1. **`rust/src/ui/popup.rs`** — Add `RunBrowserState`, rendering function, key handler
2. **`rust/src/app.rs`** — Add `run_browser: Option<RunBrowserState>` to `AppState`
3. **`rust/src/ui/layout.rs`** — Wire up dispatch: `r` to open, popup rendering order, key routing
4. **`rust/src/keys.rs`** — Add `RUN_BROWSER` key constant for `r`
5. **`rust/src/ui/help.rs`** — Add `r` to help overlay keybinding list
