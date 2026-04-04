# Phase 5: Registry, Lineage, TODOs — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add three new full-screen TUI views — Model Registry (`R`), Lineage DAG (`L`), and TODOs (`T`) — completing Phase 5 of Extract's master plan.

**Architecture:** Each view follows the existing full-screen pattern (like Compare/Diff): data is loaded into `AppState` on entry, rendered by a dedicated component, and cleared on exit. The Python SDK already has `register_model()`, `derived_from()`, `branched_from()`, `todo()`, and `list_todos()`. The Rust DB layer has `list_models()`, `get_lineage()`, and `list_todos()`. Phase 5 is primarily Rust TUI work: three new `ui/` modules, keybinding wiring, state management, and test data generation.

**Tech Stack:** Rust (ratatui 0.30, rusqlite 0.32), Python SDK (existing), ANSI 16-color theme

---

## File Map

| Action | File | Responsibility |
|--------|------|----------------|
| Create | `rust/src/ui/registry.rs` | Model registry table view |
| Create | `rust/src/ui/lineage.rs` | DAG visualization with Canvas |
| Create | `rust/src/ui/todo.rs` | TODO list with inline add/toggle |
| Modify | `rust/src/ui/mod.rs` | Register new modules |
| Modify | `rust/src/keys.rs` | Update `MODELS`→`R`, `TODOS`→`T`, `LINEAGE`→`L` (uppercase) |
| Modify | `rust/src/app.rs` | Add state fields + data loading for registry/lineage/todos |
| Modify | `rust/src/ui/layout.rs` | Wire new views into event routing and rendering |
| Modify | `rust/src/ui/statusbar.rs` | Show R/L/T hints in Explorer status bar |
| Modify | `rust/src/db.rs` | Add `list_all_lineage()`, `toggle_todo()`, `add_todo()` write queries |
| Modify | `scripts/generate_test_data.py` | Add models, lineage edges, and TODOs to test data |

---

### Task 1: Update Keybindings and Add Test Data

**Files:**
- Modify: `rust/src/keys.rs:14-16`
- Modify: `scripts/generate_test_data.py`

- [ ] **Step 1: Update key constants to uppercase**

In `rust/src/keys.rs`, change the three view keys to uppercase (shifted) to avoid conflicting with `h/l` cycling and `t` tab navigation:

```rust
pub const REGISTRY: KeyCode = KeyCode::Char('R');
pub const TODOS: KeyCode = KeyCode::Char('T');
pub const LINEAGE: KeyCode = KeyCode::Char('L');
```

Remove the old `MODELS` constant. Also add a key for adding a new TODO:

```rust
pub const ADD: KeyCode = KeyCode::Char('a');
```

- [ ] **Step 2: Add models, lineage, and TODOs to test data**

In `scripts/generate_test_data.py`, after the existing experiment creation (before `store.close()`), add:

```python
    # --- Models, Lineage, TODOs (Phase 5 test data) ---

    # Register models from completed runs
    # Re-open experiments to get run references
    ewc_exp = store.experiment({"benchmark": "cifar100", "method": "ewc", "variant": "lambda_1.0"})
    ewc_runs = store.list_runs(ewc_exp.id)

    # Register a model from the first EWC run
    if ewc_runs:
        first_run_id = ewc_runs[0]["id"]
        store._conn.execute(
            "INSERT OR IGNORE INTO models (id, name, version, run_id, artifact_path, framework, metadata, created_at) "
            "VALUES (?, ?, ?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
            (str(__import__('ulid').ULID()), "ewc-cifar100", "1.0", first_run_id,
             "checkpoints/ewc_best.pt", "pytorch",
             '{"params": 11200000, "task": "cifar100"}'),
        )
        store._conn.commit()
        model1_id = store._conn.execute(
            "SELECT id FROM models WHERE name='ewc-cifar100' AND version='1.0'"
        ).fetchone()[0]

        # Second version from second run
        if len(ewc_runs) > 1:
            second_run_id = ewc_runs[1]["id"]
            store._conn.execute(
                "INSERT OR IGNORE INTO models (id, name, version, run_id, artifact_path, framework, metadata, created_at) "
                "VALUES (?, ?, ?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
                (str(__import__('ulid').ULID()), "ewc-cifar100", "2.0", second_run_id,
                 "checkpoints/ewc_best_v2.pt", "pytorch",
                 '{"params": 11200000, "task": "cifar100"}'),
            )
            store._conn.commit()

    # SI model
    si_exp = store.experiment({"benchmark": "cifar100", "method": "si", "variant": "c_0.5"})
    si_runs = store.list_runs(si_exp.id)
    if si_runs:
        store._conn.execute(
            "INSERT OR IGNORE INTO models (id, name, version, run_id, artifact_path, framework, metadata, created_at) "
            "VALUES (?, ?, ?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%fZ','now'))",
            (str(__import__('ulid').ULID()), "si-cifar100", "1.0", si_runs[0]["id"],
             "checkpoints/si_best.pt", "pytorch",
             '{"params": 11200000, "task": "cifar100"}'),
        )
        store._conn.commit()

    # Lineage: ewc v2.0 derived_from ewc v1.0
    store._conn.execute(
        "INSERT OR IGNORE INTO lineage (parent_type, parent_id, child_type, child_id, relation) "
        "VALUES ('model', ?, 'model', ?, 'fine_tuned')",
        (model1_id, store._conn.execute(
            "SELECT id FROM models WHERE name='ewc-cifar100' AND version='2.0'"
        ).fetchone()[0]),
    )

    # Lineage: si derived_from ewc v1.0 (hypothetical branching)
    si_model_id = store._conn.execute(
        "SELECT id FROM models WHERE name='si-cifar100' AND version='1.0'"
    ).fetchone()[0]
    store._conn.execute(
        "INSERT OR IGNORE INTO lineage (parent_type, parent_id, child_type, child_id, relation) "
        "VALUES ('model', ?, 'model', ?, 'branched_from')",
        (model1_id, si_model_id),
    )

    # Lineage: run→model edges (run produced model)
    if ewc_runs:
        store._conn.execute(
            "INSERT OR IGNORE INTO lineage (parent_type, parent_id, child_type, child_id, relation) "
            "VALUES ('run', ?, 'model', ?, 'produced')",
            (ewc_runs[0]["id"], model1_id),
        )

    store._conn.commit()

    # TODOs
    store.todo("Try lambda=0.5 and compare with lambda=1.0", priority=2)
    store.todo("Run full CL benchmark suite on TinyImageNet", priority=1)
    store.todo("Write up EWC vs SI comparison results")

    # Scoped TODOs on the EWC experiment
    ewc_exp_id = ewc_exp.id
    from ulid import ULID as _ULID
    for content, prio in [
        ("Tune fisher estimation steps", 1),
        ("Compare online vs batch EWC", 0),
    ]:
        store._conn.execute(
            "INSERT INTO todos (id, scope_type, scope_id, content, done, priority) VALUES (?, 'experiment', ?, ?, 0, ?)",
            (str(_ULID()), ewc_exp_id, content, prio),
        )
    store._conn.commit()
```

- [ ] **Step 3: Verify test data generates correctly**

Run: `cd /home/phil_oh/Projects/Creations/Extract && nix develop --command python scripts/generate_test_data.py`

Expected: Script completes without errors, prints summary including experiments and runs.

Then verify models/lineage/todos exist:

```bash
sqlite3 .extract/extract.db "SELECT name, version, framework FROM models; SELECT count(*) FROM lineage; SELECT count(*) FROM todos;"
```

Expected: 3 models, 3+ lineage edges, 5 TODOs.

- [ ] **Step 4: Commit**

```bash
git add rust/src/keys.rs scripts/generate_test_data.py
git commit -m "feat(phase5): update keybindings R/T/L, add models/lineage/todos test data"
```

---

### Task 2: DB Layer — Write Queries for TODO Toggle and Add

**Files:**
- Modify: `rust/src/db.rs:580-648` (after `list_todos`)

The TUI needs to toggle todo completion and add new todos. These require writable DB connections (like `delete_run`).

- [ ] **Step 1: Add `list_all_lineage` query**

After the existing `get_lineage` method in `rust/src/db.rs` (~line 332), add:

```rust
    pub fn list_all_lineage(&self) -> Result<Vec<LineageEdge>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, parent_type, parent_id, child_type, child_id, relation, metadata, created_at \
             FROM lineage ORDER BY created_at",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(LineageEdge {
                id: row.get(0)?,
                parent_type: row.get(1)?,
                parent_id: row.get(2)?,
                child_type: row.get(3)?,
                child_id: row.get(4)?,
                relation: row.get(5)?,
                metadata: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;
        let mut edges = Vec::new();
        for row in rows {
            edges.push(row?);
        }
        Ok(edges)
    }
```

- [ ] **Step 2: Add `get_model` query**

After `list_models` (~line 304), add:

```rust
    pub fn get_model(&self, id: &str) -> Result<Option<Model>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, version, run_id, artifact_path, framework, metadata, created_at \
             FROM models WHERE id = ?",
        )?;
        let mut rows = stmt.query_map(params![id], |row| {
            Ok(Model {
                id: row.get(0)?,
                name: row.get(1)?,
                version: row.get(2)?,
                run_id: row.get(3)?,
                artifact_path: row.get(4)?,
                framework: row.get(5)?,
                metadata: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }
```

- [ ] **Step 3: Add static `toggle_todo` and `add_todo` write methods**

After the existing `delete_run` method (end of `impl Db` block, before `#[cfg(test)]`), add:

```rust
    /// Toggle a todo's done status. Opens a writable connection.
    pub fn toggle_todo(db_path: &Path, todo_id: &str) -> Result<bool> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let done: i64 = conn.query_row(
            "SELECT done FROM todos WHERE id = ?",
            params![todo_id],
            |row| row.get(0),
        )?;
        let new_done = if done != 0 { 0 } else { 1 };
        let completed_at = if new_done == 1 {
            Some(chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
        } else {
            None
        };
        conn.execute(
            "UPDATE todos SET done = ?, completed_at = ? WHERE id = ?",
            params![new_done, completed_at, todo_id],
        )?;
        Ok(new_done == 1)
    }

    /// Add a new global todo. Opens a writable connection.
    pub fn add_todo(db_path: &Path, content: &str, priority: i64) -> Result<()> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let id = format!("{}", ulid::Ulid::new());
        conn.execute(
            "INSERT INTO todos (id, scope_type, content, done, priority) VALUES (?, 'global', ?, 0, ?)",
            params![id, content, priority],
        )?;
        Ok(())
    }
```

- [ ] **Step 4: Add `ulid` and `chrono` to Cargo.toml**

The `chrono` crate is already in Cargo.toml. Add `ulid`:

```toml
ulid = "1"
```

- [ ] **Step 5: Add tests for new queries**

In the `#[cfg(test)]` block of `rust/src/db.rs`, add:

```rust
    #[test]
    fn test_list_all_lineage() {
        let db = test_db();
        // Insert some lineage edges
        db.conn.execute_batch(
            "INSERT INTO lineage (parent_type, parent_id, child_type, child_id, relation) \
             VALUES ('run', 'r1', 'model', 'm1', 'produced');
             INSERT INTO lineage (parent_type, parent_id, child_type, child_id, relation) \
             VALUES ('model', 'm1', 'model', 'm2', 'fine_tuned');",
        ).unwrap();
        let edges = db.list_all_lineage().unwrap();
        assert_eq!(edges.len(), 2);
        assert_eq!(edges[0].relation, "produced");
        assert_eq!(edges[1].relation, "fine_tuned");
    }

    #[test]
    fn test_list_models() {
        let db = test_db();
        db.conn.execute_batch(
            "INSERT INTO models (id, name, version, run_id, artifact_path, framework) \
             VALUES ('m1', 'test-model', '1.0', 'r1', 'path/to/model', 'pytorch');
             INSERT INTO models (id, name, version, run_id, artifact_path, framework) \
             VALUES ('m2', 'test-model', '2.0', 'r2', 'path/to/model2', 'pytorch');",
        ).unwrap();
        let models = db.list_models().unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].name, "test-model");
        assert_eq!(models[0].version, "1.0");
        assert_eq!(models[1].version, "2.0");
    }

    #[test]
    fn test_list_todos() {
        let db = test_db();
        db.conn.execute_batch(
            "INSERT INTO todos (id, scope_type, content, done, priority) \
             VALUES ('t1', 'global', 'First todo', 0, 2);
             INSERT INTO todos (id, scope_type, content, done, priority) \
             VALUES ('t2', 'global', 'Second todo', 1, 1);
             INSERT INTO todos (id, scope_type, scope_id, content, done, priority) \
             VALUES ('t3', 'experiment', 'e_b', 'Exp todo', 0, 0);",
        ).unwrap();

        // All todos
        let all = db.list_todos(None, None).unwrap();
        assert_eq!(all.len(), 3);
        // Ordered by priority DESC: t1 (2), t2 (1), t3 (0)
        assert_eq!(all[0].id, "t1");

        // Global only
        let global = db.list_todos(Some("global"), None).unwrap();
        assert_eq!(global.len(), 2);

        // Experiment-scoped
        let exp = db.list_todos(Some("experiment"), Some("e_b")).unwrap();
        assert_eq!(exp.len(), 1);
        assert_eq!(exp[0].content, "Exp todo");
    }
```

- [ ] **Step 6: Verify tests pass**

Run: `cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo test`

Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add rust/src/db.rs rust/Cargo.toml
git commit -m "feat(phase5): add list_all_lineage, get_model, toggle_todo, add_todo queries"
```

---

### Task 3: AppState — Registry, Lineage, TODO State Fields and Loaders

**Files:**
- Modify: `rust/src/app.rs:135-166` (AppState struct)
- Modify: `rust/src/app.rs:168-212` (AppState::new)

- [ ] **Step 1: Add state fields for the three views**

In `rust/src/app.rs`, add these fields to `AppState` (after `notification` at line 165):

```rust
    // Phase 5: Registry, Lineage, TODOs
    pub models: Vec<crate::model::Model>,
    pub registry_cursor: usize,
    pub lineage_edges: Vec<crate::model::LineageEdge>,
    pub lineage_nodes: Vec<LineageNode>,
    pub lineage_cursor: usize,
    pub todos: Vec<crate::model::Todo>,
    pub todo_cursor: usize,
    pub todo_input: Option<String>,
    pub todo_filter: TodoFilter,
```

Add the supporting types before `AppState`:

```rust
#[derive(Debug, Clone)]
pub struct LineageNode {
    pub entity_type: String,
    pub entity_id: String,
    pub label: String,
    pub layer: usize,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoFilter {
    All,
    Global,
    Experiment,
    Run,
}
```

- [ ] **Step 2: Initialize the new fields in `AppState::new()`**

In the `Ok(Self { ... })` block, add after `notification: None,`:

```rust
            models: Vec::new(),
            registry_cursor: 0,
            lineage_edges: Vec::new(),
            lineage_nodes: Vec::new(),
            lineage_cursor: 0,
            todos: Vec::new(),
            todo_cursor: 0,
            todo_input: None,
            todo_filter: TodoFilter::All,
```

- [ ] **Step 3: Add data loading methods**

After the `load_run_preview` method (~line 647), add:

```rust
    pub fn load_registry_data(&mut self) -> Result<()> {
        self.models = self.db.list_models()?;
        self.registry_cursor = 0;
        Ok(())
    }

    pub fn load_lineage_data(&mut self) -> Result<()> {
        self.lineage_edges = self.db.list_all_lineage()?;
        self.build_lineage_graph();
        self.lineage_cursor = 0;
        Ok(())
    }

    fn build_lineage_graph(&mut self) {
        use std::collections::{HashMap, HashSet, VecDeque};

        // Collect unique nodes from edges
        let mut node_set: Vec<(String, String)> = Vec::new();
        let mut seen: HashSet<(String, String)> = HashSet::new();
        for edge in &self.lineage_edges {
            let parent = (edge.parent_type.clone(), edge.parent_id.clone());
            let child = (edge.child_type.clone(), edge.child_id.clone());
            if seen.insert(parent.clone()) {
                node_set.push(parent);
            }
            if seen.insert(child.clone()) {
                node_set.push(child);
            }
        }

        if node_set.is_empty() {
            self.lineage_nodes.clear();
            return;
        }

        // Build adjacency for topological sort (parent→child)
        let node_idx: HashMap<(String, String), usize> = node_set
            .iter()
            .enumerate()
            .map(|(i, n)| (n.clone(), i))
            .collect();
        let n = node_set.len();
        let mut children_of: Vec<Vec<usize>> = vec![vec![]; n];
        let mut in_degree: Vec<usize> = vec![0; n];

        for edge in &self.lineage_edges {
            let pi = node_idx[&(edge.parent_type.clone(), edge.parent_id.clone())];
            let ci = node_idx[&(edge.child_type.clone(), edge.child_id.clone())];
            children_of[pi].push(ci);
            in_degree[ci] += 1;
        }

        // Kahn's algorithm for topological order + layer assignment
        let mut queue: VecDeque<usize> = VecDeque::new();
        let mut layer: Vec<usize> = vec![0; n];
        for i in 0..n {
            if in_degree[i] == 0 {
                queue.push_back(i);
            }
        }
        while let Some(u) = queue.pop_front() {
            for &v in &children_of[u] {
                layer[v] = layer[v].max(layer[u] + 1);
                in_degree[v] -= 1;
                if in_degree[v] == 0 {
                    queue.push_back(v);
                }
            }
        }

        // Assign x positions within each layer
        let max_layer = *layer.iter().max().unwrap_or(&0);
        let mut layer_counts: Vec<usize> = vec![0; max_layer + 1];

        // Resolve labels
        let mut nodes: Vec<LineageNode> = Vec::new();
        for (i, (etype, eid)) in node_set.iter().enumerate() {
            let label = match etype.as_str() {
                "model" => {
                    self.db.get_model(eid).ok().flatten()
                        .map(|m| format!("{} v{}", m.name, m.version))
                        .unwrap_or_else(|| format!("model:{}", &eid[eid.len().saturating_sub(8)..]))
                }
                "run" => {
                    self.db.get_run(eid).ok().flatten()
                        .and_then(|r| r.name.or_else(|| {
                            self.db.get_experiment(&r.experiment_id).ok().flatten()
                                .map(|e| e.name)
                        }))
                        .unwrap_or_else(|| format!("run:{}", &eid[eid.len().saturating_sub(8)..]))
                }
                "experiment" => {
                    self.db.get_experiment(eid).ok().flatten()
                        .map(|e| e.name)
                        .unwrap_or_else(|| format!("exp:{}", &eid[eid.len().saturating_sub(8)..]))
                }
                _ => format!("{}:{}", etype, &eid[eid.len().saturating_sub(8)..]),
            };

            let l = layer[i];
            let x_pos = layer_counts[l] as f64;
            layer_counts[l] += 1;

            nodes.push(LineageNode {
                entity_type: etype.clone(),
                entity_id: eid.clone(),
                label,
                layer: l,
                x: x_pos,
                y: l as f64,
            });
        }

        // Center each layer horizontally
        for l in 0..=max_layer {
            let count = layer_counts[l];
            if count == 0 { continue; }
            let offset = -(count as f64 - 1.0) / 2.0;
            for node in &mut nodes {
                if node.layer == l {
                    node.x += offset;
                }
            }
        }

        self.lineage_nodes = nodes;
    }

    pub fn load_todo_data(&mut self) -> Result<()> {
        let (scope_type, scope_id) = match self.todo_filter {
            TodoFilter::All => (None, None),
            TodoFilter::Global => (Some("global"), None),
            TodoFilter::Experiment => (Some("experiment"), None),
            TodoFilter::Run => (Some("run"), None),
        };
        self.todos = self.db.list_todos(scope_type, scope_id)?;
        if self.todo_cursor >= self.todos.len() && !self.todos.is_empty() {
            self.todo_cursor = self.todos.len() - 1;
        }
        Ok(())
    }
```

- [ ] **Step 4: Build and verify**

Run: `cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo check`

Expected: Compiles with no errors (warnings about unused fields are fine).

- [ ] **Step 5: Commit**

```bash
git add rust/src/app.rs
git commit -m "feat(phase5): add AppState fields and loaders for registry, lineage, todos"
```

---

### Task 4: Registry View

**Files:**
- Create: `rust/src/ui/registry.rs`

- [ ] **Step 1: Create the registry view component**

Create `rust/src/ui/registry.rs`:

```rust
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Row, Table};
use ratatui::Frame;

use crate::app::{Action, AppState, Focus, View};
use crate::event::AppEvent;
use crate::keys;
use crate::ui::theme::Theme;

pub struct RegistryView {
    theme: Theme,
}

impl RegistryView {
    pub fn new() -> Self {
        Self {
            theme: Theme::default(),
        }
    }

    pub fn handle_event(&mut self, event: &AppEvent, state: &mut AppState) -> Action {
        match event {
            AppEvent::Key(key) => self.handle_key(key, state),
            _ => Action::None,
        }
    }

    fn handle_key(&mut self, key: &crossterm::event::KeyEvent, state: &mut AppState) -> Action {
        if keys::matches(key, keys::BACK_ESC) {
            state.current_view = View::Explorer;
            state.focus = Focus::Tree;
            return Action::None;
        }
        if keys::matches(key, keys::QUIT) {
            return Action::Quit;
        }
        if keys::matches(key, keys::NAV_DOWN_J) || keys::matches(key, keys::NAV_DOWN) {
            if !state.models.is_empty() {
                state.registry_cursor = (state.registry_cursor + 1).min(state.models.len() - 1);
            }
            return Action::None;
        }
        if keys::matches(key, keys::NAV_UP_K) || keys::matches(key, keys::NAV_UP) {
            state.registry_cursor = state.registry_cursor.saturating_sub(1);
            return Action::None;
        }
        // Enter: navigate to the linked run
        if keys::matches(key, keys::SELECT) {
            if let Some(model) = state.models.get(state.registry_cursor) {
                if let Some(ref run_id) = model.run_id {
                    // Find the run's experiment and select it
                    if let Ok(Some(run)) = state.db.get_run(run_id) {
                        if let Some(idx) = state.experiments.iter().position(|e| e.id == run.experiment_id) {
                            state.selected_experiment = Some(idx);
                            let _ = state.refresh_runs();
                            if let Some(run_idx) = state.runs.iter().position(|r| r.id == *run_id) {
                                state.selected_run = Some(run_idx);
                                let _ = state.load_run_preview(run_idx);
                            }
                            state.current_view = View::Explorer;
                            state.focus = Focus::Detail;
                        }
                    }
                }
            }
            return Action::None;
        }
        // L: view lineage for selected model
        if keys::matches_shift(key, keys::LINEAGE) {
            if let Some(model) = state.models.get(state.registry_cursor) {
                state.lineage_edges = state.db.get_lineage("model", &model.id).unwrap_or_default();
                // Also load transitive edges for full graph
                let _ = state.load_lineage_data();
                state.current_view = View::Lineage;
            }
            return Action::None;
        }
        Action::None
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let border_style = Style::default().fg(self.theme.border_focused);
        let block = Block::bordered()
            .title(" R Models ")
            .border_style(border_style);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if state.models.is_empty() {
            let msg = ratatui::widgets::Paragraph::new("No models registered. Use run.register_model() in the Python SDK.")
                .style(Style::default().fg(self.theme.accent_dim));
            frame.render_widget(msg, inner);
            return;
        }

        let header = Row::new(vec![
            Cell::from("Name").style(self.theme.header),
            Cell::from("Version").style(self.theme.header),
            Cell::from("Framework").style(self.theme.header),
            Cell::from("Run").style(self.theme.header),
            Cell::from("Path").style(self.theme.header),
            Cell::from("Created").style(self.theme.header),
        ]);

        let rows: Vec<Row> = state
            .models
            .iter()
            .enumerate()
            .map(|(i, model)| {
                let style = if i == state.registry_cursor {
                    self.theme.selected
                } else {
                    Style::default()
                };

                let run_label = model.run_id.as_deref()
                    .and_then(|rid| {
                        state.db.get_run(rid).ok().flatten()
                            .and_then(|r| r.name.or_else(|| {
                                state.db.get_experiment(&r.experiment_id).ok().flatten()
                                    .map(|e| e.name)
                            }))
                    })
                    .unwrap_or_else(|| model.run_id.as_deref().map(|id| {
                        if id.len() > 8 { id[id.len()-8..].to_string() } else { id.to_string() }
                    }).unwrap_or_default());

                let created = &model.created_at[..10.min(model.created_at.len())];

                Row::new(vec![
                    Cell::from(model.name.clone()),
                    Cell::from(model.version.clone()),
                    Cell::from(model.framework.clone().unwrap_or_default()),
                    Cell::from(run_label),
                    Cell::from(model.artifact_path.clone()),
                    Cell::from(created.to_string()),
                ])
                .style(style)
            })
            .collect();

        let widths = [
            Constraint::Percentage(20),
            Constraint::Percentage(10),
            Constraint::Percentage(12),
            Constraint::Percentage(18),
            Constraint::Percentage(25),
            Constraint::Percentage(15),
        ];

        let table = Table::new(rows, widths)
            .header(header.style(Style::default().add_modifier(Modifier::BOLD)))
            .row_highlight_style(self.theme.selected);

        frame.render_widget(table, inner);
    }
}
```

- [ ] **Step 2: Build and verify**

Run: `cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo check`

Expected: Compiles (registry.rs won't be wired in yet, so it's just checked for syntax).

- [ ] **Step 3: Commit**

```bash
git add rust/src/ui/registry.rs
git commit -m "feat(phase5): add RegistryView component"
```

---

### Task 5: TODO View

**Files:**
- Create: `rust/src/ui/todo.rs`

- [ ] **Step 1: Create the TODO view component**

Create `rust/src/ui/todo.rs`:

```rust
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

use crate::app::{Action, AppState, Focus, NotifyLevel, TodoFilter, View};
use crate::event::AppEvent;
use crate::keys;
use crate::ui::theme::Theme;

pub struct TodoView {
    theme: Theme,
}

impl TodoView {
    pub fn new() -> Self {
        Self {
            theme: Theme::default(),
        }
    }

    pub fn handle_event(&mut self, event: &AppEvent, state: &mut AppState) -> Action {
        match event {
            AppEvent::Key(key) => self.handle_key(key, state),
            _ => Action::None,
        }
    }

    fn handle_key(&mut self, key: &crossterm::event::KeyEvent, state: &mut AppState) -> Action {
        // Text input mode
        if let Some(ref mut input) = state.todo_input {
            match key.code {
                crossterm::event::KeyCode::Enter => {
                    let content = input.clone();
                    state.todo_input = None;
                    if !content.trim().is_empty() {
                        let db_path = state.store_root.join("extract.db");
                        match crate::db::Db::add_todo(&db_path, content.trim(), 0) {
                            Ok(()) => {
                                state.notify(NotifyLevel::Success, "TODO added");
                                let _ = state.load_todo_data();
                            }
                            Err(e) => state.notify(NotifyLevel::Error, format!("Failed: {e}")),
                        }
                    }
                    return Action::None;
                }
                crossterm::event::KeyCode::Esc => {
                    state.todo_input = None;
                    return Action::None;
                }
                crossterm::event::KeyCode::Backspace => {
                    input.pop();
                    return Action::None;
                }
                crossterm::event::KeyCode::Char(c) => {
                    input.push(c);
                    return Action::None;
                }
                _ => return Action::None,
            }
        }

        // Normal mode
        if keys::matches(key, keys::BACK_ESC) {
            state.current_view = View::Explorer;
            state.focus = Focus::Tree;
            return Action::None;
        }
        if keys::matches(key, keys::QUIT) {
            return Action::Quit;
        }
        if keys::matches(key, keys::NAV_DOWN_J) || keys::matches(key, keys::NAV_DOWN) {
            if !state.todos.is_empty() {
                state.todo_cursor = (state.todo_cursor + 1).min(state.todos.len() - 1);
            }
            return Action::None;
        }
        if keys::matches(key, keys::NAV_UP_K) || keys::matches(key, keys::NAV_UP) {
            state.todo_cursor = state.todo_cursor.saturating_sub(1);
            return Action::None;
        }
        // Space: toggle done
        if keys::matches(key, keys::TOGGLE_SELECT) {
            if let Some(todo) = state.todos.get(state.todo_cursor) {
                let todo_id = todo.id.clone();
                let db_path = state.store_root.join("extract.db");
                match crate::db::Db::toggle_todo(&db_path, &todo_id) {
                    Ok(_) => {
                        let _ = state.load_todo_data();
                    }
                    Err(e) => state.notify(NotifyLevel::Error, format!("Toggle failed: {e}")),
                }
            }
            return Action::None;
        }
        // a: add new TODO
        if keys::matches(key, keys::ADD) {
            state.todo_input = Some(String::new());
            return Action::None;
        }
        // Tab: cycle filter
        if keys::matches(key, keys::TAB) {
            state.todo_filter = match state.todo_filter {
                TodoFilter::All => TodoFilter::Global,
                TodoFilter::Global => TodoFilter::Experiment,
                TodoFilter::Experiment => TodoFilter::Run,
                TodoFilter::Run => TodoFilter::All,
            };
            let _ = state.load_todo_data();
            return Action::None;
        }
        Action::None
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let border_style = Style::default().fg(self.theme.border_focused);
        let filter_label = match state.todo_filter {
            TodoFilter::All => "All",
            TodoFilter::Global => "Global",
            TodoFilter::Experiment => "Experiment",
            TodoFilter::Run => "Run",
        };
        let block = Block::bordered()
            .title(format!(" T TODOs [{filter_label}] "))
            .border_style(border_style);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Split: todo list + optional input line
        let has_input = state.todo_input.is_some();
        let chunks = if has_input {
            Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(inner)
        } else {
            Layout::vertical([Constraint::Min(0)]).split(inner)
        };

        let list_area = chunks[0];

        if state.todos.is_empty() {
            let msg = Paragraph::new("No TODOs. Press 'a' to add one.")
                .style(Style::default().fg(self.theme.accent_dim));
            frame.render_widget(msg, list_area);
        } else {
            let lines: Vec<Line> = state
                .todos
                .iter()
                .enumerate()
                .map(|(i, todo)| {
                    let is_selected = i == state.todo_cursor;
                    let checkbox = if todo.done { "[x]" } else { "[ ]" };
                    let priority_indicator = match todo.priority {
                        p if p >= 2 => "!! ",
                        1 => "!  ",
                        _ => "   ",
                    };
                    let scope_label = match todo.scope_type.as_str() {
                        "global" => String::new(),
                        "experiment" => {
                            let name = todo.scope_id.as_deref()
                                .and_then(|id| state.db.get_experiment(id).ok().flatten().map(|e| e.name))
                                .unwrap_or_else(|| "?".to_string());
                            format!(" [exp:{name}]")
                        }
                        "run" => {
                            let name = todo.scope_id.as_deref()
                                .and_then(|id| state.db.get_run(id).ok().flatten().and_then(|r| r.name))
                                .unwrap_or_else(|| "?".to_string());
                            format!(" [run:{name}]")
                        }
                        other => format!(" [{other}]"),
                    };

                    let base_style = if is_selected {
                        self.theme.selected
                    } else if todo.done {
                        Style::default().fg(self.theme.accent_dim)
                    } else {
                        Style::default()
                    };

                    let priority_style = if is_selected {
                        self.theme.selected
                    } else if todo.priority >= 2 {
                        Style::default().fg(self.theme.error).add_modifier(Modifier::BOLD)
                    } else if todo.priority == 1 {
                        Style::default().fg(self.theme.warning)
                    } else {
                        base_style
                    };

                    Line::from(vec![
                        Span::styled(format!(" {priority_indicator}"), priority_style),
                        Span::styled(format!("{checkbox} "), base_style),
                        Span::styled(todo.content.clone(), base_style),
                        Span::styled(scope_label, Style::default().fg(self.theme.accent_dim)),
                    ])
                })
                .collect();

            let paragraph = Paragraph::new(lines);
            frame.render_widget(paragraph, list_area);
        }

        // Input line
        if has_input {
            let input_area = chunks[1];
            let input_text = state.todo_input.as_deref().unwrap_or("");
            let input_line = Line::from(vec![
                Span::styled(" > ", Style::default().fg(self.theme.accent).add_modifier(Modifier::BOLD)),
                Span::raw(input_text),
                Span::styled("_", Style::default().fg(self.theme.accent).add_modifier(Modifier::SLOW_BLINK)),
            ]);
            frame.render_widget(Paragraph::new(input_line), input_area);
        }
    }
}
```

- [ ] **Step 2: Build and verify**

Run: `cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo check`

Expected: Compiles.

- [ ] **Step 3: Commit**

```bash
git add rust/src/ui/todo.rs
git commit -m "feat(phase5): add TodoView component with toggle, add, and filter"
```

---

### Task 6: Lineage DAG View

**Files:**
- Create: `rust/src/ui/lineage.rs`

- [ ] **Step 1: Create the lineage view with Canvas-based DAG rendering**

Create `rust/src/ui/lineage.rs`:

```rust
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::symbols::Marker;
use ratatui::widgets::canvas::{Canvas, Line as CanvasLine, Points};
use ratatui::widgets::Block;
use ratatui::Frame;

use crate::app::{Action, AppState, Focus, View};
use crate::event::AppEvent;
use crate::keys;
use crate::ui::theme::Theme;

const NODE_SPACING_X: f64 = 30.0;
const NODE_SPACING_Y: f64 = 12.0;

pub struct LineageView {
    theme: Theme,
}

impl LineageView {
    pub fn new() -> Self {
        Self {
            theme: Theme::default(),
        }
    }

    pub fn handle_event(&mut self, event: &AppEvent, state: &mut AppState) -> Action {
        match event {
            AppEvent::Key(key) => self.handle_key(key, state),
            _ => Action::None,
        }
    }

    fn handle_key(&mut self, key: &crossterm::event::KeyEvent, state: &mut AppState) -> Action {
        if keys::matches(key, keys::BACK_ESC) {
            state.current_view = View::Explorer;
            state.focus = Focus::Tree;
            return Action::None;
        }
        if keys::matches(key, keys::QUIT) {
            return Action::Quit;
        }
        // Navigate between nodes
        if keys::matches(key, keys::NAV_DOWN_J) || keys::matches(key, keys::NAV_DOWN) {
            if !state.lineage_nodes.is_empty() {
                state.lineage_cursor = (state.lineage_cursor + 1).min(state.lineage_nodes.len() - 1);
            }
            return Action::None;
        }
        if keys::matches(key, keys::NAV_UP_K) || keys::matches(key, keys::NAV_UP) {
            state.lineage_cursor = state.lineage_cursor.saturating_sub(1);
            return Action::None;
        }
        // Enter: navigate to the entity
        if keys::matches(key, keys::SELECT) {
            if let Some(node) = state.lineage_nodes.get(state.lineage_cursor) {
                match node.entity_type.as_str() {
                    "run" => {
                        if let Ok(Some(run)) = state.db.get_run(&node.entity_id) {
                            if let Some(idx) = state.experiments.iter().position(|e| e.id == run.experiment_id) {
                                state.selected_experiment = Some(idx);
                                let _ = state.refresh_runs();
                                if let Some(ri) = state.runs.iter().position(|r| r.id == node.entity_id) {
                                    state.selected_run = Some(ri);
                                    let _ = state.load_run_preview(ri);
                                }
                                state.current_view = View::Explorer;
                                state.focus = Focus::Detail;
                            }
                        }
                    }
                    "model" => {
                        let _ = state.load_registry_data();
                        if let Some(idx) = state.models.iter().position(|m| m.id == node.entity_id) {
                            state.registry_cursor = idx;
                        }
                        state.current_view = View::Registry;
                    }
                    _ => {}
                }
            }
            return Action::None;
        }
        Action::None
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let border_style = Style::default().fg(self.theme.border_focused);
        let block = Block::bordered()
            .title(" L Lineage ")
            .border_style(border_style);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if state.lineage_nodes.is_empty() {
            let msg = ratatui::widgets::Paragraph::new(
                "No lineage data. Use run.derived_from() or run.branched_from() in the Python SDK.",
            )
            .style(Style::default().fg(self.theme.accent_dim));
            frame.render_widget(msg, inner);
            return;
        }

        // Compute canvas bounds
        let (min_x, max_x, min_y, max_y) = self.compute_bounds(state);

        // Build node index for edge drawing
        let node_positions: Vec<(f64, f64)> = state
            .lineage_nodes
            .iter()
            .map(|n| (n.x * NODE_SPACING_X, -(n.y * NODE_SPACING_Y)))
            .collect();

        let canvas = Canvas::default()
            .marker(Marker::Braille)
            .x_bounds([min_x - NODE_SPACING_X, max_x + NODE_SPACING_X])
            .y_bounds([min_y - NODE_SPACING_Y, max_y + NODE_SPACING_Y])
            .paint(|ctx| {
                // Draw edges
                for edge in &state.lineage_edges {
                    let parent_pos = state.lineage_nodes.iter()
                        .position(|n| n.entity_type == edge.parent_type && n.entity_id == edge.parent_id);
                    let child_pos = state.lineage_nodes.iter()
                        .position(|n| n.entity_type == edge.child_type && n.entity_id == edge.child_id);

                    if let (Some(pi), Some(ci)) = (parent_pos, child_pos) {
                        let (px, py) = node_positions[pi];
                        let (cx, cy) = node_positions[ci];
                        ctx.draw(&CanvasLine {
                            x1: px,
                            y1: py,
                            x2: cx,
                            y2: cy,
                            color: ratatui::style::Color::DarkGray,
                        });
                    }
                }

                // Draw nodes
                for (i, node) in state.lineage_nodes.iter().enumerate() {
                    let (nx, ny) = node_positions[i];
                    let is_selected = i == state.lineage_cursor;

                    let color = if is_selected {
                        ratatui::style::Color::White
                    } else {
                        match node.entity_type.as_str() {
                            "experiment" => ratatui::style::Color::Blue,
                            "run" => ratatui::style::Color::Green,
                            "model" => ratatui::style::Color::Yellow,
                            _ => ratatui::style::Color::White,
                        }
                    };

                    // Draw node marker
                    ctx.draw(&Points {
                        coords: &[(nx, ny)],
                        color,
                    });

                    // Draw label
                    let label = if node.label.len() > 20 {
                        format!("{}...", &node.label[..17])
                    } else {
                        node.label.clone()
                    };
                    ctx.print(nx + 1.0, ny, ratatui::text::Line::from(
                        ratatui::text::Span::styled(label, Style::default().fg(color)),
                    ));
                }
            });

        frame.render_widget(canvas, inner);

        // Draw legend + selected node info at bottom
        if let Some(node) = state.lineage_nodes.get(state.lineage_cursor) {
            let info = format!(
                " {} | {} | {}",
                node.entity_type, node.label, node.entity_id
            );
            let info_area = Rect::new(
                inner.x,
                inner.y + inner.height.saturating_sub(1),
                inner.width,
                1,
            );
            let info_widget = ratatui::widgets::Paragraph::new(info)
                .style(Style::default().fg(self.theme.accent).add_modifier(Modifier::BOLD));
            frame.render_widget(info_widget, info_area);
        }
    }

    fn compute_bounds(&self, state: &AppState) -> (f64, f64, f64, f64) {
        let mut min_x = f64::MAX;
        let mut max_x = f64::MIN;
        let mut min_y = f64::MAX;
        let mut max_y = f64::MIN;

        for node in &state.lineage_nodes {
            let x = node.x * NODE_SPACING_X;
            let y = -(node.y * NODE_SPACING_Y);
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }

        (min_x, max_x, min_y, max_y)
    }
}
```

- [ ] **Step 2: Build and verify**

Run: `cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo check`

Expected: Compiles.

- [ ] **Step 3: Commit**

```bash
git add rust/src/ui/lineage.rs
git commit -m "feat(phase5): add LineageView with Canvas Sugiyama DAG"
```

---

### Task 7: Wire Everything into mod.rs, layout.rs, and statusbar.rs

**Files:**
- Modify: `rust/src/ui/mod.rs:1-13`
- Modify: `rust/src/ui/layout.rs`
- Modify: `rust/src/ui/statusbar.rs`

- [ ] **Step 1: Register new modules in mod.rs**

In `rust/src/ui/mod.rs`, add after line 6 (`pub mod diff;`):

```rust
pub mod lineage;
```

And add after line 8 (`pub mod popup;`):

```rust
pub mod registry;
```

And add after line 10 (`pub mod statusbar;`):

```rust
pub mod todo;
```

- [ ] **Step 2: Add new components to AppLayout**

In `rust/src/ui/layout.rs`, add the imports:

```rust
use crate::ui::lineage::LineageView;
use crate::ui::registry::RegistryView;
use crate::ui::todo::TodoView;
```

Add fields to `AppLayout` struct (after `popup: PopupRenderer,`):

```rust
    pub registry: RegistryView,
    pub lineage: LineageView,
    pub todo_view: TodoView,
```

Initialize in `new()` (after `popup: PopupRenderer::new(),`):

```rust
            registry: RegistryView::new(),
            lineage: LineageView::new(),
            todo_view: TodoView::new(),
```

- [ ] **Step 3: Wire event handling for full-screen views**

In `layout.rs` `handle_event()`, expand the match for full-screen views (currently lines 113-117):

Replace:

```rust
        // Route to full-screen views first
        match state.current_view {
            View::Compare => return self.compare.handle_event(event, state),
            View::Diff => return self.diff.handle_event(event, state),
            _ => {}
        }
```

With:

```rust
        // Route to full-screen views first
        match state.current_view {
            View::Compare => return self.compare.handle_event(event, state),
            View::Diff => return self.diff.handle_event(event, state),
            View::Registry => return self.registry.handle_event(event, state),
            View::Lineage => return self.lineage.handle_event(event, state),
            View::TodoGlobal => return self.todo_view.handle_event(event, state),
            _ => {}
        }
```

- [ ] **Step 4: Add R/T/L global keys in Explorer focus**

In `layout.rs` `handle_event()`, after the global panel shortcuts block (after line 96 `}`), add:

```rust
            // View shortcuts
            if keys::matches_shift(key, keys::REGISTRY) {
                let _ = state.load_registry_data();
                state.current_view = View::Registry;
                return Action::None;
            }
            if keys::matches_shift(key, keys::TODOS) {
                let _ = state.load_todo_data();
                state.current_view = View::TodoGlobal;
                return Action::None;
            }
            if keys::matches_shift(key, keys::LINEAGE) {
                let _ = state.load_lineage_data();
                state.current_view = View::Lineage;
                return Action::None;
            }
```

- [ ] **Step 5: Wire rendering for new views**

In `layout.rs` `render()`, add cases for the new views alongside Compare/Diff (after the `View::Diff` block):

```rust
            View::Registry => {
                self.registry.render(frame, main_area, state);
                self.statusbar.render(frame, status_area, state);
            }
            View::Lineage => {
                self.lineage.render(frame, main_area, state);
                self.statusbar.render(frame, status_area, state);
            }
            View::TodoGlobal => {
                self.todo_view.render(frame, main_area, state);
                self.statusbar.render(frame, status_area, state);
            }
```

- [ ] **Step 6: Update statusbar with R/T/L hints**

In `rust/src/ui/statusbar.rs`, update the Explorer/Tree bindings (inside the `(View::Explorer, Focus::Tree)` match arm) to include the new view keys. After the existing bindings and before `b.push(("Tab", "detail"))`, add:

```rust
                b.push(("R", "models"));
                b.push(("T", "todos"));
                b.push(("L", "lineage"));
```

Also add a status bar case for the new views. Replace the catch-all `_ => vec![("q", "quit"), ("Esc", "back")],` with:

```rust
            (View::Registry, _) => vec![
                ("Esc", "back"),
                ("j/k", "navigate"),
                ("Enter", "go to run"),
                ("L", "lineage"),
                ("q", "quit"),
            ],
            (View::Lineage, _) => vec![
                ("Esc", "back"),
                ("j/k", "navigate"),
                ("Enter", "go to entity"),
                ("q", "quit"),
            ],
            (View::TodoGlobal, _) => vec![
                ("Esc", "back"),
                ("j/k", "navigate"),
                ("Space", "toggle"),
                ("a", "add"),
                ("Tab", "filter"),
                ("q", "quit"),
            ],
            _ => vec![("q", "quit"), ("Esc", "back")],
```

- [ ] **Step 7: Build and verify**

Run: `cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo check`

Expected: Compiles with no errors.

- [ ] **Step 8: Commit**

```bash
git add rust/src/ui/mod.rs rust/src/ui/layout.rs rust/src/ui/statusbar.rs
git commit -m "feat(phase5): wire registry, lineage, todo views into layout and statusbar"
```

---

### Task 8: Full Build, Test Data, and Manual Verification

**Files:** None new — integration verification only.

- [ ] **Step 1: Run full test suite**

Run: `cd /home/phil_oh/Projects/Creations/Extract/rust && nix develop --command cargo test`

Expected: All existing + new tests pass.

- [ ] **Step 2: Regenerate test data with Phase 5 content**

Run: `cd /home/phil_oh/Projects/Creations/Extract && nix develop --command python scripts/generate_test_data.py`

Verify models, lineage, and TODOs:

```bash
sqlite3 .extract/extract.db "SELECT name, version FROM models;"
sqlite3 .extract/extract.db "SELECT parent_type, relation, child_type FROM lineage;"
sqlite3 .extract/extract.db "SELECT scope_type, content, done FROM todos;"
```

- [ ] **Step 3: Launch TUI and test all three views**

Run: `cd /home/phil_oh/Projects/Creations/Extract && nix develop --command cargo run --manifest-path rust/Cargo.toml -- --store .extract`

Test:
1. Press `R` → Registry view shows models table. Press `j/k` to navigate. Press `Esc` to return.
2. Press `T` → TODO view shows 5 TODOs. Press `Space` to toggle one. Press `a`, type a new TODO, press `Enter`. Press `Tab` to cycle filters. Press `Esc` to return.
3. Press `L` → Lineage view shows DAG with model nodes and edges. Press `j/k` to navigate nodes. Press `Esc` to return.
4. In Registry, press `Enter` on a model → navigates to the linked run in Detail view.

- [ ] **Step 4: Fix any issues found during manual testing**

- [ ] **Step 5: Final commit**

```bash
git add -A
git commit -m "feat(phase5): registry, lineage, and TODO views complete"
```
