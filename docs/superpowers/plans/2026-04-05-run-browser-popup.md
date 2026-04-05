# Run Browser Popup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `r` key-triggered centered popup that lists all runs for the current experiment, with inline `/` search, `x` delete, and single-select navigation. Also add keystroke hint footers to all interactive popups.

**Architecture:** New `RunBrowserState` struct and rendering/key-handling in `popup.rs`, wired into `layout.rs` dispatch chain. Uses existing `centered_rect`, `Clear` overlay, and `DeleteConfirmState` patterns. The field `selected_run: Option<usize>` (index into `state.runs`) is set on Enter to navigate to the chosen run.

**Tech Stack:** Rust, ratatui, crossterm

---

### Task 1: Add `RunBrowserState` struct and `RUN_BROWSER` key constant

**Files:**
- Modify: `rust/src/app.rs:108-113` (after `RunPickerState`)
- Modify: `rust/src/app.rs:157-207` (add field to `AppState`)
- Modify: `rust/src/app.rs:222-271` (initialize in `AppState::new`)
- Modify: `rust/src/keys.rs:36` (add constant)

- [ ] **Step 1: Add `RunBrowserState` to `app.rs`**

After `RunPickerState` (line 113), add:

```rust
/// State for the run browser popup (r key).
pub struct RunBrowserState {
    pub experiment_name: String,
    pub experiment_id: String,
    pub runs: Vec<Run>,
    pub filtered: Vec<usize>,
    pub cursor: usize,
    pub search_query: Option<String>,
    pub scroll_offset: usize,
}

impl RunBrowserState {
    pub fn new(experiment_name: String, experiment_id: String, runs: Vec<Run>) -> Self {
        let filtered = (0..runs.len()).collect();
        Self {
            experiment_name,
            experiment_id,
            runs,
            filtered,
            cursor: 0,
            search_query: None,
            scroll_offset: 0,
        }
    }

    /// Re-filter runs based on current search query.
    pub fn apply_filter(&mut self) {
        let Some(ref query) = self.search_query else {
            self.filtered = (0..self.runs.len()).collect();
            self.cursor = 0;
            self.scroll_offset = 0;
            return;
        };
        if query.is_empty() {
            self.filtered = (0..self.runs.len()).collect();
            self.cursor = 0;
            self.scroll_offset = 0;
            return;
        }
        let q = query.to_lowercase();
        self.filtered = self.runs.iter().enumerate()
            .filter(|(_, run)| {
                let name = run.name.as_deref().unwrap_or("").to_lowercase();
                let tags = run.tags.as_deref().unwrap_or("").to_lowercase();
                let notes = run.notes.as_deref().unwrap_or("").to_lowercase();
                let status = run.status.to_lowercase();
                let config = run.config.as_deref().unwrap_or("").to_lowercase();
                name.contains(&q) || tags.contains(&q) || notes.contains(&q)
                    || status.contains(&q) || config.contains(&q)
            })
            .map(|(i, _)| i)
            .collect();
        self.cursor = 0;
        self.scroll_offset = 0;
    }
}
```

- [ ] **Step 2: Add `run_browser` field to `AppState`**

In the `AppState` struct (after `run_picker` on line 184), add:

```rust
    pub run_browser: Option<RunBrowserState>,
```

- [ ] **Step 3: Initialize `run_browser` in `AppState::new`**

In `AppState::new()`, after `run_picker: None,` (line 253), add:

```rust
            run_browser: None,
```

- [ ] **Step 4: Add `RUN_BROWSER` key constant**

In `keys.rs`, after the `GO_BOTTOM` constant (line 38), add:

```rust
pub const RUN_BROWSER: KeyCode = KeyCode::Char('r');
```

- [ ] **Step 5: Verify it compiles**

Run: `cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo check 2>&1 | tail -5`
Expected: compiles with no errors

- [ ] **Step 6: Commit**

```bash
git add rust/src/app.rs rust/src/keys.rs
git commit -m "feat: add RunBrowserState struct and RUN_BROWSER key constant"
```

---

### Task 2: Add run browser key handling in `popup.rs`

**Files:**
- Modify: `rust/src/ui/popup.rs:1-12` (imports)
- Modify: `rust/src/ui/popup.rs:91` (after `handle_run_picker_key`, add new method)

- [ ] **Step 1: Update imports in `popup.rs`**

Add `KeyCode` to the crossterm import on line 1:

```rust
use crossterm::event::{KeyCode, KeyEvent};
```

Update the app import on line 9 to include `RunBrowserState`:

```rust
use crate::app::{AppState, DeleteConfirmState, RunBrowserState, RunPickerState};
```

- [ ] **Step 2: Add `handle_run_browser_key` method to `PopupRenderer`**

After the `handle_delete_confirm_key` method (after line 101), add:

```rust
    /// Handle key events for the run browser popup.
    /// Returns true when the popup should close.
    pub fn handle_run_browser_key(&self, key: &KeyEvent, state: &mut AppState) -> bool {
        let Some(ref mut browser) = state.run_browser else {
            return false;
        };

        let is_searching = browser.search_query.is_some();

        // Search mode: handle text input
        if is_searching {
            match key.code {
                KeyCode::Esc => {
                    // Cancel search, restore full list
                    browser.search_query = None;
                    browser.filtered = (0..browser.runs.len()).collect();
                    browser.cursor = 0;
                    browser.scroll_offset = 0;
                    return false;
                }
                KeyCode::Enter => {
                    // Confirm filter, exit search mode but keep filtered results
                    browser.search_query = None;
                    return false;
                }
                KeyCode::Backspace => {
                    if let Some(ref mut q) = browser.search_query {
                        q.pop();
                    }
                    browser.apply_filter();
                    return false;
                }
                KeyCode::Char(c) => {
                    if let Some(ref mut q) = browser.search_query {
                        q.push(c);
                    }
                    browser.apply_filter();
                    return false;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if !browser.filtered.is_empty() && browser.cursor + 1 < browser.filtered.len() {
                        browser.cursor += 1;
                    }
                    return false;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if browser.cursor > 0 {
                        browser.cursor -= 1;
                    }
                    return false;
                }
                _ => return false,
            }
        }

        // Normal mode
        if keys::matches(key, keys::NAV_DOWN_J) || keys::matches(key, keys::NAV_DOWN) {
            if !browser.filtered.is_empty() && browser.cursor + 1 < browser.filtered.len() {
                browser.cursor += 1;
            }
            return false;
        }

        if keys::matches(key, keys::NAV_UP_K) || keys::matches(key, keys::NAV_UP) {
            if browser.cursor > 0 {
                browser.cursor -= 1;
            }
            return false;
        }

        if keys::matches(key, keys::SEARCH) {
            browser.search_query = Some(String::new());
            return false;
        }

        if keys::matches(key, keys::SELECT) {
            // Select the run under cursor and navigate to it
            if let Some(&run_idx) = browser.filtered.get(browser.cursor) {
                state.selected_run = Some(run_idx);
                let _ = state.load_run_preview(run_idx);
                state.metrics = state.runs.get(run_idx)
                    .map(|r| state.db.get_latest_metrics(&r.id).unwrap_or_default())
                    .unwrap_or_default();
                state.focus = crate::app::Focus::Detail;
            }
            state.run_browser = None;
            return true;
        }

        if keys::matches(key, keys::DELETE) {
            if let Some(&run_idx) = browser.filtered.get(browser.cursor) {
                if let Some(run) = browser.runs.get(run_idx) {
                    let run_id = run.id.clone();
                    let label = run.name.clone().unwrap_or_else(|| {
                        if run_id.len() > 8 {
                            run_id[run_id.len() - 8..].to_string()
                        } else {
                            run_id.clone()
                        }
                    });
                    state.delete_confirm = Some(DeleteConfirmState { run_id, label });
                }
            }
            return false;
        }

        if keys::matches(key, keys::BACK_ESC) {
            state.run_browser = None;
            return true;
        }

        false
    }
```

- [ ] **Step 3: Verify it compiles**

Run: `cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo check 2>&1 | tail -5`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add rust/src/ui/popup.rs
git commit -m "feat: add run browser key handling in popup.rs"
```

---

### Task 3: Add run browser rendering in `popup.rs`

**Files:**
- Modify: `rust/src/ui/popup.rs` (after `render_delete_confirm` method, before the closing `}` of `impl PopupRenderer`)

- [ ] **Step 1: Add `render_run_browser` method**

After the `render_delete_confirm` method, add:

```rust
    /// Render the run browser popup.
    pub fn render_run_browser(&self, frame: &mut Frame, area: Rect, browser: &RunBrowserState) {
        let is_searching = browser.search_query.is_some();
        let search_line_count: u16 = if is_searching { 1 } else { 0 };
        let footer_line_count: u16 = 1;
        let filtered_count = browser.filtered.len() as u16;

        // Height: border(2) + search(0|1) + runs + footer(1)
        let content_height = search_line_count + filtered_count + footer_line_count;
        let height = (content_height + 2).min(area.height.saturating_sub(4)).max(4);
        let width = 60u16.min(area.width.saturating_sub(4));
        let popup_area = centered_rect(width, height, area);

        frame.render_widget(Clear, popup_area);

        let title = format!(" {} — runs ", browser.experiment_name);

        let footer_spans = if is_searching {
            vec![
                Span::styled("Type", Style::default().fg(self.theme.accent)),
                Span::styled(" filter  ", Style::default().fg(self.theme.accent_dim)),
                Span::styled("Enter", Style::default().fg(self.theme.accent)),
                Span::styled(" confirm  ", Style::default().fg(self.theme.accent_dim)),
                Span::styled("Esc", Style::default().fg(self.theme.accent)),
                Span::styled(" cancel", Style::default().fg(self.theme.accent_dim)),
            ]
        } else {
            vec![
                Span::styled("j/k", Style::default().fg(self.theme.accent)),
                Span::styled(" nav  ", Style::default().fg(self.theme.accent_dim)),
                Span::styled("Enter", Style::default().fg(self.theme.accent)),
                Span::styled(" select  ", Style::default().fg(self.theme.accent_dim)),
                Span::styled("/", Style::default().fg(self.theme.accent)),
                Span::styled(" search  ", Style::default().fg(self.theme.accent_dim)),
                Span::styled("x", Style::default().fg(self.theme.accent)),
                Span::styled(" delete  ", Style::default().fg(self.theme.accent_dim)),
                Span::styled("Esc", Style::default().fg(self.theme.accent)),
                Span::styled(" close", Style::default().fg(self.theme.accent_dim)),
            ]
        };

        let block = Block::bordered()
            .title(title)
            .title_bottom(Line::from(footer_spans))
            .border_style(Style::default().fg(self.theme.accent))
            .border_set(border::ROUNDED);

        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        if inner.height == 0 || inner.width == 0 {
            return;
        }

        let mut lines: Vec<Line> = Vec::new();

        // Search input line
        if let Some(ref query) = browser.search_query {
            let prompt = Span::styled("/ ", Style::default().fg(self.theme.accent));
            let query_text = Span::raw(query.clone());
            let cursor = Span::styled("_", Style::default().add_modifier(Modifier::SLOW_BLINK));
            lines.push(Line::from(vec![prompt, query_text, cursor]));
        }

        // Compute visible window for scrolling
        let list_height = inner.height as usize - lines.len() - footer_line_count as usize;
        let scroll = if browser.cursor >= browser.scroll_offset + list_height {
            browser.cursor.saturating_sub(list_height - 1)
        } else if browser.cursor < browser.scroll_offset {
            browser.cursor
        } else {
            browser.scroll_offset
        };

        // Run rows
        for (vi, &run_idx) in browser.filtered.iter().enumerate().skip(scroll).take(list_height) {
            let run = &browser.runs[run_idx];
            let is_cursor = vi == browser.cursor;

            let name = run.name.as_deref().unwrap_or(&run.id);
            let date = run.ended_at.as_deref()
                .unwrap_or(if run.status == "running" { "running" } else { &run.started_at })
                .chars().take(19).collect::<String>();

            let status_style = match run.status.as_str() {
                "running" => self.theme.status_running,
                "completed" => self.theme.status_completed,
                "failed" => self.theme.status_failed,
                _ => Style::default(),
            };

            let line_style = if is_cursor { self.theme.selected } else { Style::default() };

            // Truncate name to fit: width - date(19) - status(~10) - padding(4)
            let max_name_len = (inner.width as usize).saturating_sub(34);
            let display_name = if name.len() > max_name_len {
                format!("{}…", &name[..max_name_len.saturating_sub(1)])
            } else {
                name.to_string()
            };

            let padding = max_name_len.saturating_sub(display_name.len());
            let line = Line::from(vec![
                Span::styled(format!(" {display_name}{}", " ".repeat(padding)), line_style),
                Span::styled(
                    format!(" {:>9} ", run.status),
                    if is_cursor { line_style } else { status_style },
                ),
                Span::styled(
                    date,
                    if is_cursor { line_style } else { Style::default().fg(self.theme.accent_dim) },
                ),
            ]);
            lines.push(line);
        }

        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, inner);
    }
```

- [ ] **Step 2: Verify it compiles**

Run: `cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo check 2>&1 | tail -5`
Expected: compiles with no errors

- [ ] **Step 3: Commit**

```bash
git add rust/src/ui/popup.rs
git commit -m "feat: add run browser rendering in popup.rs"
```

---

### Task 4: Wire up run browser in `layout.rs`

**Files:**
- Modify: `rust/src/ui/layout.rs:61-128` (event handling — add run_browser dispatch)
- Modify: `rust/src/ui/layout.rs:350-371` (rendering — add run_browser render call)
- Modify: `rust/src/ui/layout.rs:164-209` (explorer key shortcuts — add `r` binding)

- [ ] **Step 1: Add run browser key dispatch in `handle_event`**

In `handle_event`, after the `run_picker` block (lines 124-127), add the run browser dispatch:

```rust
            if state.run_browser.is_some() {
                self.popup.handle_run_browser_key(key, state);
                return Action::None;
            }
```

- [ ] **Step 2: Add `r` key binding in explorer shortcuts**

In the explorer-only key shortcuts section (after the `SEARCH` binding at lines 201-208), add:

```rust
            if keys::matches(key, keys::RUN_BROWSER) {
                // Open run browser for current leaf experiment with multiple runs
                if let Some(idx) = state.selected_experiment {
                    if let Some(exp) = state.experiments.get(idx) {
                        let has_children = state.experiments.iter()
                            .any(|e| e.parent_id.as_deref() == Some(&exp.id));
                        if !has_children && state.runs.len() > 1 {
                            let mut sorted_runs = state.runs.clone();
                            sorted_runs.sort_by(|a, b| {
                                let a_time = a.ended_at.as_deref().unwrap_or(&a.started_at);
                                let b_time = b.ended_at.as_deref().unwrap_or(&b.started_at);
                                b_time.cmp(a_time)
                            });
                            state.run_browser = Some(crate::app::RunBrowserState::new(
                                exp.name.clone(),
                                exp.id.clone(),
                                sorted_runs,
                            ));
                        }
                    }
                }
                return Action::None;
            }
```

- [ ] **Step 3: Add run browser render call**

In the `render` method, after the `run_picker` render block (lines 351-353) and before the `delete_confirm` render block, add:

```rust
        if let Some(ref browser) = state.run_browser {
            self.popup.render_run_browser(frame, area, browser);
        }
```

- [ ] **Step 4: Handle delete confirmation completing while run browser is open**

In the `handle_event` method, inside the `delete_confirm` block (around lines 102-123), after the run is deleted successfully and `state.delete_confirm = None`, add logic to refresh the run browser if it's open. Replace the existing delete_confirm block with:

```rust
            if state.delete_confirm.is_some() {
                if let Some(confirmed) = self.popup.handle_delete_confirm_key(key) {
                    if confirmed {
                        let confirm = state.delete_confirm.as_ref().unwrap();
                        let run_id = confirm.run_id.clone();
                        let label = confirm.label.clone();
                        match state.delete_run(&run_id) {
                            Ok(()) => {
                                state.notify(
                                    crate::app::NotifyLevel::Success,
                                    format!("Deleted {label}"),
                                );
                                // Refresh run browser if open
                                if let Some(ref mut browser) = state.run_browser {
                                    browser.runs.retain(|r| r.id != run_id);
                                    browser.apply_filter();
                                    if browser.cursor >= browser.filtered.len() && !browser.filtered.is_empty() {
                                        browser.cursor = browser.filtered.len() - 1;
                                    }
                                    // Close browser if 0 or 1 runs remain
                                    if browser.runs.len() <= 1 {
                                        state.run_browser = None;
                                    }
                                }
                            }
                            Err(e) => state.notify(
                                crate::app::NotifyLevel::Error,
                                format!("Delete failed: {e}"),
                            ),
                        }
                    }
                    state.delete_confirm = None;
                }
                return Action::None;
            }
```

- [ ] **Step 5: Verify it compiles**

Run: `cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo check 2>&1 | tail -5`
Expected: compiles with no errors

- [ ] **Step 6: Commit**

```bash
git add rust/src/ui/layout.rs
git commit -m "feat: wire up run browser popup in layout dispatch and rendering"
```

---

### Task 5: Add keystroke hint footer to run picker (compare popup)

**Files:**
- Modify: `rust/src/ui/popup.rs` (in `render_run_picker` method)

- [ ] **Step 1: Add `title_bottom` footer to the run picker block**

In `render_run_picker`, update the height calculation (line 105) to add 1 for the footer:

```rust
        let height = (picker.runs.len() as u16 + 5).min(area.height.saturating_sub(4));
```

Replace the block construction (lines 112-115) with:

```rust
        let title = format!(" {} — select runs ", picker.experiment_name);
        let footer_spans = vec![
            Span::styled("j/k", Style::default().fg(self.theme.accent)),
            Span::styled(" nav  ", Style::default().fg(self.theme.accent_dim)),
            Span::styled("Space", Style::default().fg(self.theme.accent)),
            Span::styled(" toggle  ", Style::default().fg(self.theme.accent_dim)),
            Span::styled("Enter", Style::default().fg(self.theme.accent)),
            Span::styled(" confirm  ", Style::default().fg(self.theme.accent_dim)),
            Span::styled("Esc", Style::default().fg(self.theme.accent)),
            Span::styled(" close", Style::default().fg(self.theme.accent_dim)),
        ];
        let block = Block::bordered()
            .title(title)
            .title_bottom(Line::from(footer_spans))
            .border_style(Style::default().fg(self.theme.accent))
            .border_set(border::ROUNDED);
```

- [ ] **Step 2: Verify it compiles**

Run: `cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo check 2>&1 | tail -5`
Expected: compiles with no errors

- [ ] **Step 3: Commit**

```bash
git add rust/src/ui/popup.rs
git commit -m "feat: add keystroke hint footer to run picker popup"
```

---

### Task 6: Update help overlay with `r` binding

**Files:**
- Modify: `rust/src/ui/help.rs:49-61` (Explorer section keybindings)

- [ ] **Step 1: Add `r` binding to Explorer section**

In the Explorer keybinding list (after the `("Space", "mark run"),` entry on line 53), add:

```rust
            ("r", "browse runs"),
```

- [ ] **Step 2: Update help overlay height**

On line 23, increase the height from `38` to `39` to accommodate the new line:

```rust
        let height = 39u16.min(area.height.saturating_sub(2));
```

- [ ] **Step 3: Verify it compiles**

Run: `cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo check 2>&1 | tail -5`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add rust/src/ui/help.rs
git commit -m "feat: add r keybinding to help overlay"
```

---

### Task 7: Build and smoke test

**Files:** None (verification only)

- [ ] **Step 1: Full build**

Run: `cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo build 2>&1 | tail -10`
Expected: compiles successfully

- [ ] **Step 2: Run clippy**

Run: `cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo clippy 2>&1 | tail -20`
Expected: no warnings or errors in our modified files
