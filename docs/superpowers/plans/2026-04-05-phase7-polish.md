# Phase 7: Polish — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `/` fuzzy search, `?` help overlay, and `[theme]` config overrides to the Extract TUI.

**Architecture:** Three independent features wired into the existing layout. Search uses a centered popup with live-filtered results from a new DB query. Help is a static overlay dismissed on any key. Theme overrides extend the existing `config.toml` with a `[theme]` section that maps color names to RGB hex values, applied at `Theme` construction.

**Tech Stack:** Rust (ratatui 0.30, rusqlite 0.32), existing config.toml (toml crate)

---

## File Map

| Action | File | Responsibility |
|--------|------|----------------|
| Create | `rust/src/ui/search.rs` | Search popup: input, live results, navigation |
| Create | `rust/src/ui/help.rs` | Help overlay: keybinding reference |
| Modify | `rust/src/ui/mod.rs` | Register new modules |
| Modify | `rust/src/keys.rs` | Add SEARCH and HELP constants |
| Modify | `rust/src/db.rs` | Add `search(query)` method |
| Modify | `rust/src/app.rs` | Add search/help state fields |
| Modify | `rust/src/ui/layout.rs` | Wire search/help into event handling and rendering |
| Modify | `rust/src/ui/statusbar.rs` | Add `/` and `?` hints |
| Modify | `rust/src/config.rs` | Add `ThemeConfig` struct |
| Modify | `rust/src/ui/theme.rs` | Apply config overrides to Theme |

---

### Task 1: Search DB Query

**Files:**
- Modify: `rust/src/db.rs`

- [ ] **Step 1: Add SearchResult struct to model.rs**

In `rust/src/model.rs`, add:

```rust
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub result_type: String, // "experiment" or "run"
    pub id: String,
    pub experiment_id: Option<String>, // for runs, the parent experiment
    pub label: String,       // display name (path or run name)
    pub matched_field: String, // which field matched (path, name, tags, notes)
    pub snippet: String,     // the matched text
}
```

- [ ] **Step 2: Add search method to db.rs**

In `rust/src/db.rs`, add after the `list_todos` method:

```rust
    pub fn search(&self, query: &str) -> Result<Vec<crate::model::SearchResult>> {
        let pattern = format!("%{query}%");
        let mut results = Vec::new();

        // Search experiments by path and name
        let mut stmt = self.conn.prepare(
            "SELECT id, path, name FROM experiments WHERE path LIKE ?1 OR name LIKE ?1 LIMIT 20",
        )?;
        let rows = stmt.query_map(params![pattern], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (id, path, name) = row?;
            let matched = if name.to_lowercase().contains(&query.to_lowercase()) {
                ("name", name.clone())
            } else {
                ("path", path.clone())
            };
            results.push(crate::model::SearchResult {
                result_type: "experiment".to_string(),
                id,
                experiment_id: None,
                label: path,
                matched_field: matched.0.to_string(),
                snippet: matched.1,
            });
        }

        // Search runs by name, tags, notes
        let mut stmt = self.conn.prepare(
            "SELECT r.id, r.experiment_id, r.name, r.tags, r.notes, e.path \
             FROM runs r JOIN experiments e ON r.experiment_id = e.id \
             WHERE r.name LIKE ?1 OR r.tags LIKE ?1 OR r.notes LIKE ?1 LIMIT 20",
        )?;
        let rows = stmt.query_map(params![pattern], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?;
        for row in rows {
            let (id, exp_id, name, tags, notes, exp_path) = row?;
            let q_lower = query.to_lowercase();
            let (field, snippet) = if name.as_deref().unwrap_or("").to_lowercase().contains(&q_lower) {
                ("name", name.clone().unwrap_or_default())
            } else if tags.as_deref().unwrap_or("").to_lowercase().contains(&q_lower) {
                ("tags", tags.clone().unwrap_or_default())
            } else {
                ("notes", notes.clone().unwrap_or_default().chars().take(80).collect())
            };
            let label = name.unwrap_or_else(|| format!("{exp_path} (run)"));
            results.push(crate::model::SearchResult {
                result_type: "run".to_string(),
                id,
                experiment_id: Some(exp_id),
                label,
                matched_field: field.to_string(),
                snippet,
            });
        }

        Ok(results)
    }
```

- [ ] **Step 3: Verify it compiles**

Run: `cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo check`

- [ ] **Step 4: Commit**

```bash
git add rust/src/model.rs rust/src/db.rs
git commit -m "feat(phase7): add search query across experiments and runs"
```

---

### Task 2: Search State and Keybinding

**Files:**
- Modify: `rust/src/keys.rs`
- Modify: `rust/src/app.rs`

- [ ] **Step 1: Add key constants**

In `rust/src/keys.rs`, add:

```rust
pub const SEARCH: KeyCode = KeyCode::Char('/');
pub const HELP: KeyCode = KeyCode::Char('?');
```

- [ ] **Step 2: Add state fields to AppState**

In `rust/src/app.rs`, add a search state struct before `AppState`:

```rust
pub struct SearchState {
    pub query: String,
    pub results: Vec<crate::model::SearchResult>,
    pub cursor: usize,
}
```

Add fields to `AppState` (after `pending_tree_select`):

```rust
    pub search: Option<SearchState>,
    pub show_help: bool,
```

Initialize in `AppState::new()`:

```rust
            search: None,
            show_help: false,
```

- [ ] **Step 3: Verify it compiles**

Run: `cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo check`

- [ ] **Step 4: Commit**

```bash
git add rust/src/keys.rs rust/src/app.rs
git commit -m "feat(phase7): add search/help state and keybindings"
```

---

### Task 3: Search Popup Component

**Files:**
- Create: `rust/src/ui/search.rs`

- [ ] **Step 1: Create search.rs**

Create `rust/src/ui/search.rs`:

```rust
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{AppState, Focus, SearchState, View};
use crate::ui::theme::Theme;

pub struct SearchPopup {
    theme: Theme,
}

impl SearchPopup {
    pub fn new() -> Self {
        Self {
            theme: Theme::default(),
        }
    }

    pub fn handle_key(
        &self,
        key: &crossterm::event::KeyEvent,
        state: &mut AppState,
    ) -> bool {
        let Some(ref mut search) = state.search else {
            return false;
        };

        match key.code {
            crossterm::event::KeyCode::Esc => {
                state.search = None;
                return true;
            }
            crossterm::event::KeyCode::Enter => {
                // Navigate to selected result
                if let Some(result) = search.results.get(search.cursor).cloned() {
                    state.search = None;
                    match result.result_type.as_str() {
                        "experiment" => {
                            if let Some(idx) = state
                                .experiments
                                .iter()
                                .position(|e| e.id == result.id)
                            {
                                state.selected_experiment = Some(idx);
                                state.pending_tree_select = Some(result.id);
                                let _ = state.refresh_runs();
                                let _ = state.refresh_selection_summary();
                            }
                        }
                        "run" => {
                            if let Some(exp_id) = &result.experiment_id {
                                if let Some(idx) = state
                                    .experiments
                                    .iter()
                                    .position(|e| e.id == *exp_id)
                                {
                                    state.selected_experiment = Some(idx);
                                    state.pending_tree_select = Some(exp_id.clone());
                                    let _ = state.refresh_runs();
                                    if let Some(ri) =
                                        state.runs.iter().position(|r| r.id == result.id)
                                    {
                                        state.selected_run = Some(ri);
                                        let _ = state.load_run_preview(ri);
                                    }
                                    state.focus = Focus::Detail;
                                }
                            }
                        }
                        _ => {}
                    }
                    state.current_view = View::Explorer;
                }
                return true;
            }
            crossterm::event::KeyCode::Backspace => {
                search.query.pop();
                // Re-run search
                search.results = state
                    .db
                    .search(&search.query)
                    .unwrap_or_default();
                search.cursor = 0;
                return true;
            }
            crossterm::event::KeyCode::Down | crossterm::event::KeyCode::Tab => {
                if !search.results.is_empty()
                    && search.cursor + 1 < search.results.len()
                {
                    search.cursor += 1;
                }
                return true;
            }
            crossterm::event::KeyCode::Up => {
                search.cursor = search.cursor.saturating_sub(1);
                return true;
            }
            crossterm::event::KeyCode::Char(c) => {
                if key.modifiers == crossterm::event::KeyModifiers::NONE
                    || key.modifiers == crossterm::event::KeyModifiers::SHIFT
                {
                    search.query.push(c);
                    search.results = state
                        .db
                        .search(&search.query)
                        .unwrap_or_default();
                    search.cursor = 0;
                }
                return true;
            }
            _ => return true,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, search: &SearchState) {
        let result_count = search.results.len().min(10);
        let popup_height = (result_count as u16 + 4).min(area.height.saturating_sub(4)); // +3 border+input, +1 padding
        let popup_width = 60u16.min(area.width.saturating_sub(4));
        let x = area.x + (area.width.saturating_sub(popup_width)) / 2;
        let y = area.y + 2; // near top
        let popup_area = Rect::new(x, y, popup_width, popup_height);

        frame.render_widget(Clear, popup_area);

        let block = Block::bordered()
            .title(" / Search ")
            .border_style(Style::default().fg(self.theme.accent));
        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        if inner.height == 0 {
            return;
        }

        // Input line
        let input_area = Rect::new(inner.x, inner.y, inner.width, 1);
        let cursor = Span::styled("_", Style::default().add_modifier(Modifier::SLOW_BLINK));
        let input_line = Line::from(vec![
            Span::styled(" > ", Style::default().fg(self.theme.accent)),
            Span::raw(search.query.as_str()),
            cursor,
        ]);
        frame.render_widget(Paragraph::new(input_line), input_area);

        // Results
        let results_area = Rect::new(
            inner.x,
            inner.y + 1,
            inner.width,
            inner.height.saturating_sub(1),
        );

        if search.results.is_empty() && !search.query.is_empty() {
            let msg = Line::from(Span::styled(
                "  No results",
                Style::default().fg(self.theme.accent_dim),
            ));
            frame.render_widget(Paragraph::new(msg), results_area);
        } else {
            let lines: Vec<Line> = search
                .results
                .iter()
                .enumerate()
                .take(results_area.height as usize)
                .map(|(i, result)| {
                    let is_selected = i == search.cursor;
                    let style = if is_selected {
                        self.theme.selected
                    } else {
                        Style::default()
                    };

                    let type_tag = match result.result_type.as_str() {
                        "experiment" => "[exp]",
                        "run" => "[run]",
                        _ => "[???]",
                    };

                    let dim = if is_selected {
                        style
                    } else {
                        Style::default().fg(self.theme.accent_dim)
                    };

                    Line::from(vec![
                        Span::styled(format!(" {type_tag} "), dim),
                        Span::styled(result.label.clone(), style),
                        Span::styled(
                            format!("  ({})", result.matched_field),
                            dim,
                        ),
                    ])
                })
                .collect();

            frame.render_widget(Paragraph::new(lines), results_area);
        }
    }
}
```

- [ ] **Step 2: Commit**

```bash
git add rust/src/ui/search.rs
git commit -m "feat(phase7): add SearchPopup component"
```

---

### Task 4: Help Overlay Component

**Files:**
- Create: `rust/src/ui/help.rs`

- [ ] **Step 1: Create help.rs**

Create `rust/src/ui/help.rs`:

```rust
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

use crate::ui::theme::Theme;

pub struct HelpOverlay {
    theme: Theme,
}

impl HelpOverlay {
    pub fn new() -> Self {
        Self {
            theme: Theme::default(),
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let popup_width = 50u16.min(area.width.saturating_sub(4));
        let popup_height = 30u16.min(area.height.saturating_sub(4));
        let x = area.x + (area.width.saturating_sub(popup_width)) / 2;
        let y = area.y + (area.height.saturating_sub(popup_height)) / 2;
        let popup_area = Rect::new(x, y, popup_width, popup_height);

        frame.render_widget(Clear, popup_area);

        let block = Block::bordered()
            .title(" ? Help ")
            .border_style(Style::default().fg(self.theme.accent));
        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        let bold = Style::default()
            .fg(self.theme.accent)
            .add_modifier(Modifier::BOLD);
        let dim = Style::default().fg(self.theme.accent_dim);
        let key_style = Style::default()
            .fg(self.theme.accent)
            .add_modifier(Modifier::BOLD);

        let mut lines = Vec::new();

        let section = |title: &str| -> Line {
            Line::from(Span::styled(format!(" {title}"), bold))
        };

        let binding = |key: &str, desc: &str| -> Line {
            Line::from(vec![
                Span::styled(format!("   {key:<14}"), key_style),
                Span::styled(desc, dim),
            ])
        };

        lines.push(section("Explorer"));
        lines.push(binding("j/k", "Navigate tree"));
        lines.push(binding("Enter", "Expand / select experiment"));
        lines.push(binding("Space", "Mark run for comparison"));
        lines.push(binding("c", "Compare marked runs"));
        lines.push(binding("d", "Diff marked runs"));
        lines.push(binding("/", "Search"));
        lines.push(binding("1/2/3", "Focus tree / detail / selection"));
        lines.push(binding("Tab", "Next panel"));
        lines.push(Line::from(""));

        lines.push(section("Detail Panel"));
        lines.push(binding("h/l", "Cycle through runs"));
        lines.push(binding("S/I", "Summary / Info tab"));
        lines.push(binding("x", "Delete run"));
        lines.push(Line::from(""));

        lines.push(section("Views"));
        lines.push(binding("M", "Model registry"));
        lines.push(binding("T", "TODOs"));
        lines.push(binding("L", "Lineage DAG"));
        lines.push(Line::from(""));

        lines.push(section("TODO View"));
        lines.push(binding("Space", "Toggle done"));
        lines.push(binding("a", "Add TODO"));
        lines.push(binding("x", "Delete TODO"));
        lines.push(binding("0/1/2", "Set priority"));
        lines.push(binding("A/G/E/R", "Filter by scope"));
        lines.push(Line::from(""));

        lines.push(section("Global"));
        lines.push(binding("?", "Toggle this help"));
        lines.push(binding("q", "Quit"));
        lines.push(binding("Esc", "Back / close"));

        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, inner);
    }
}
```

- [ ] **Step 2: Commit**

```bash
git add rust/src/ui/help.rs
git commit -m "feat(phase7): add HelpOverlay component"
```

---

### Task 5: Theme Config Overrides

**Files:**
- Modify: `rust/src/config.rs`
- Modify: `rust/src/ui/theme.rs`

- [ ] **Step 1: Add ThemeConfig to config.rs**

In `rust/src/config.rs`, add before the `Config` struct:

```rust
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ThemeConfig {
    pub fg: Option<String>,
    pub bg: Option<String>,
    pub accent: Option<String>,
    pub accent_dim: Option<String>,
    pub success: Option<String>,
    pub warning: Option<String>,
    pub error: Option<String>,
    pub border: Option<String>,
    pub border_focused: Option<String>,
}
```

Add a `parse_hex_color` function:

```rust
/// Parse a hex color string like "#89b4fa" into a ratatui Color.
pub fn parse_hex_color(s: &str) -> Option<Color> {
    let s = s.strip_prefix('#').unwrap_or(s);
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}
```

Add the `theme` field to `Config`:

```rust
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub summary: SummaryConfig,
    #[serde(default)]
    pub tables: TablesConfig,
    #[serde(default)]
    pub compare: CompareConfig,
    #[serde(default)]
    pub notifications: NotificationsConfig,
    #[serde(default)]
    pub theme: ThemeConfig,
}
```

- [ ] **Step 2: Add Theme::from_config method**

In `rust/src/ui/theme.rs`, add:

```rust
use crate::config::{parse_hex_color, ThemeConfig};
```

And add a method to `Theme`:

```rust
impl Theme {
    pub fn from_config(tc: &ThemeConfig) -> Self {
        let mut t = Self::default();
        if let Some(ref c) = tc.fg { if let Some(color) = parse_hex_color(c) { t.fg = color; } }
        if let Some(ref c) = tc.bg { if let Some(color) = parse_hex_color(c) { t.bg = color; } }
        if let Some(ref c) = tc.accent {
            if let Some(color) = parse_hex_color(c) {
                t.accent = color;
                t.border_focused = color;
                t.header = Style::default().fg(color).add_modifier(Modifier::BOLD);
                t.selected = Style::default().fg(Color::Black).bg(color).add_modifier(Modifier::BOLD);
                t.tab_active = Style::default().fg(color).add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
                t.chart_line_1 = color;
            }
        }
        if let Some(ref c) = tc.accent_dim { if let Some(color) = parse_hex_color(c) { t.accent_dim = color; t.border = color; t.tab_inactive = Style::default().fg(color); t.tree_branch = Style::default().fg(color); t.chart_axis = color; } }
        if let Some(ref c) = tc.success { if let Some(color) = parse_hex_color(c) { t.success = color; t.status_completed = Style::default().fg(color); t.metric_positive = Style::default().fg(color); } }
        if let Some(ref c) = tc.warning { if let Some(color) = parse_hex_color(c) { t.warning = color; t.status_running = Style::default().fg(color).add_modifier(Modifier::BOLD); } }
        if let Some(ref c) = tc.error { if let Some(color) = parse_hex_color(c) { t.error = color; t.status_failed = Style::default().fg(color).add_modifier(Modifier::BOLD); t.metric_negative = Style::default().fg(color); } }
        if let Some(ref c) = tc.border { if let Some(color) = parse_hex_color(c) { t.border = color; } }
        if let Some(ref c) = tc.border_focused { if let Some(color) = parse_hex_color(c) { t.border_focused = color; } }
        t
    }
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo check`

- [ ] **Step 4: Commit**

```bash
git add rust/src/config.rs rust/src/ui/theme.rs
git commit -m "feat(phase7): theme config overrides with hex color support"
```

---

### Task 6: Wire Everything into Layout

**Files:**
- Modify: `rust/src/ui/mod.rs`
- Modify: `rust/src/ui/layout.rs`
- Modify: `rust/src/ui/statusbar.rs`

- [ ] **Step 1: Register modules in mod.rs**

Add to `rust/src/ui/mod.rs`:

```rust
pub mod help;
pub mod search;
```

- [ ] **Step 2: Add components to AppLayout**

In `rust/src/ui/layout.rs`, add imports:

```rust
use crate::ui::help::HelpOverlay;
use crate::ui::search::SearchPopup;
```

Add fields to `AppLayout` (after `todo_view`):

```rust
    pub search: SearchPopup,
    pub help: HelpOverlay,
```

Initialize in `new()`:

```rust
            search: SearchPopup::new(),
            help: HelpOverlay::new(),
```

- [ ] **Step 3: Wire search/help into event handling**

In `layout.rs` `handle_event()`, add search and help handling BEFORE the delete_confirm/run_picker checks (at the very top of the method, after the opening `{`):

```rust
        // Search popup intercepts all keys when active
        if state.search.is_some() {
            if let AppEvent::Key(key) = event {
                self.search.handle_key(key, state);
            }
            return Action::None;
        }

        // Help overlay dismisses on any key
        if state.show_help {
            if let AppEvent::Key(_) = event {
                state.show_help = false;
            }
            return Action::None;
        }
```

Then add `/` and `?` as global keys. In the Explorer-only key block (after the view shortcuts for M/T/L), add:

```rust
            if keys::matches(key, keys::SEARCH) {
                state.search = Some(crate::app::SearchState {
                    query: String::new(),
                    results: Vec::new(),
                    cursor: 0,
                });
                return Action::None;
            }
            if keys::matches(key, keys::HELP) {
                state.show_help = true;
                return Action::None;
            }
```

- [ ] **Step 4: Wire rendering**

In `layout.rs` `render()`, add after the notification toast rendering (at the end of the method):

```rust
        // Search popup overlay
        if let Some(ref search) = state.search {
            self.search.render(frame, area, search);
        }

        // Help overlay
        if state.show_help {
            self.help.render(frame, area);
        }
```

- [ ] **Step 5: Update statusbar**

In `rust/src/ui/statusbar.rs`, in the Explorer/Tree bindings, add before `("q", "quit")`:

```rust
                b.push(("/", "search"));
                b.push(("?", "help"));
```

- [ ] **Step 6: Verify and commit**

Run: `cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo check`
Run: `cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo test`

```bash
git add rust/src/ui/mod.rs rust/src/ui/layout.rs rust/src/ui/statusbar.rs
git commit -m "feat(phase7): wire search, help, and theme into layout"
```

---

### Task 7: Pass Theme Config Through the App

**Files:**
- Modify: `rust/src/ui/layout.rs`

Currently every component creates its own `Theme::default()`. To make theme overrides work, `AppLayout` needs to pass the config-aware theme. The simplest approach: `AppLayout::new()` accepts a `&Config` and constructs the theme once.

- [ ] **Step 1: Update AppLayout::new to accept Config**

Change `AppLayout::new()` signature and pass theme to components. Since all components store their own `Theme`, the simplest fix is to construct a `Theme` from config in `new()` and assign it:

```rust
    pub fn new(config: &crate::config::Config) -> Self {
        let theme = Theme::from_config(&config.theme);
        Self {
            tree: TreePanel::new(),
            detail: DetailPanel::new(),
            dashboard: Dashboard::new(),
            compare: CompareView::new(),
            diff: DiffView::new(),
            selection: SelectionWindow::new(),
            statusbar: StatusBar::new(),
            popup: PopupRenderer::new(),
            registry: RegistryView::new(),
            lineage: LineageView::new(),
            todo_view: TodoView::new(),
            search: SearchPopup::new(),
            help: HelpOverlay::new(),
            theme,
        }
    }
```

- [ ] **Step 2: Update main.rs to pass config**

In `rust/src/main.rs`, change the `AppLayout::new()` call to pass the config:

```rust
    let mut layout = ui::layout::AppLayout::new(&app.config);
```

- [ ] **Step 3: Verify and commit**

Run: `cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo check`
Run: `cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo test`

```bash
git add rust/src/ui/layout.rs rust/src/main.rs
git commit -m "feat(phase7): pass theme config from main through AppLayout"
```

---

### Task 8: Build, Test, Verify

- [ ] **Step 1: Run full test suite**

Run: `cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo test`

- [ ] **Step 2: Build and smoke test**

Run: `cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo build`

- [ ] **Step 3: Add a test theme config**

Append to `.extract/config.toml`:

```toml
[theme]
accent = "#89b4fa"
accent_dim = "#585b70"
success = "#a6e3a1"
warning = "#f9e2af"
error = "#f38ba8"
```

- [ ] **Step 4: Final commit**

```bash
git add -A
git commit -m "feat(phase7): search, help overlay, and theme overrides complete"
```
