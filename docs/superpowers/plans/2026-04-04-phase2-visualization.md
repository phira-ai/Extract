# Phase 2: Visualization — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add loss curves (Chart widget) and accuracy/confusion matrix heatmaps (Canvas widget) to the TUI detail view, loaded from artifacts on disk. Extend the detail panel with two new tabs: Curves and Matrix.

**Architecture:** The Python SDK already has `log_matrix()`, `log_timeseries()`, and `metrics.py`. The Rust TUI already has `artifact.rs` (loads .npy and .json) and `list_artifacts()` in db.rs. This plan adds: (1) a `chart.rs` component rendering scalar metric curves using ratatui's `Chart` widget, (2) a `heatmap.rs` component rendering matrix data using ratatui's `Canvas` widget with colored rectangles, (3) two new tabs in `detail.rs`, (4) artifact data loaded into `AppState`, and (5) test data generation with matrices.

**Tech Stack:** Rust, ratatui 0.30 (Chart, Canvas, Datasets), ndarray 0.16, ndarray-npy 0.9

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `rust/src/ui/chart.rs` | Create | Line chart rendering for scalar metric curves |
| `rust/src/ui/heatmap.rs` | Create | Matrix heatmap rendering via Canvas |
| `rust/src/ui/detail.rs` | Modify | Add Curves + Matrix tabs, load artifacts on run select |
| `rust/src/ui/mod.rs` | Modify | Declare `chart` and `heatmap` modules |
| `rust/src/app.rs` | Modify | Add artifact + metric history fields to AppState |
| `rust/src/ui/theme.rs` | Modify | Add heatmap gradient colors |
| `scripts/generate_test_data.py` | Modify | Add matrix and timeseries artifacts to test data |

---

### Task 1: Update Test Data Generator

**Files:**
- Modify: `scripts/generate_test_data.py`

- [ ] **Step 1: Add matrix and timeseries artifacts to test data generation**

Update `scripts/generate_test_data.py` to log accuracy matrices and loss timeseries for select runs. After the existing scalar logging, add artifact calls inside the run context managers.

For the CIFAR-100 EWC lambda_1.0 run (first run), add inside the context manager after the scalar loop:

```python
        # Log accuracy matrix (5 tasks, upper-triangular pattern for CL)
        import numpy as np
        acc_matrix = np.array([
            [0.92, 0.00, 0.00, 0.00, 0.00],
            [0.85, 0.88, 0.00, 0.00, 0.00],
            [0.78, 0.82, 0.90, 0.00, 0.00],
            [0.71, 0.75, 0.83, 0.87, 0.00],
            [0.65, 0.70, 0.78, 0.82, 0.85],
        ])
        run.log_matrix("accuracy_matrix", acc_matrix, step=49,
                       axes={"rows": "evaluated_on", "cols": "trained_up_to"})

        # Log loss timeseries artifact (same data as scalars, but as artifact file)
        steps_list = list(range(50))
        loss_values = [1.0 / (s + 1) for s in steps_list]
        run.log_timeseries("loss_curve", steps_list, loss_values)
```

For the CIFAR-100 SI c_0.5 run (third run), add similar artifacts:

```python
        acc_matrix = np.array([
            [0.88, 0.00, 0.00, 0.00, 0.00],
            [0.80, 0.84, 0.00, 0.00, 0.00],
            [0.72, 0.76, 0.86, 0.00, 0.00],
            [0.65, 0.69, 0.78, 0.83, 0.00],
            [0.58, 0.63, 0.72, 0.77, 0.80],
        ])
        run.log_matrix("accuracy_matrix", acc_matrix, step=49,
                       axes={"rows": "evaluated_on", "cols": "trained_up_to"})

        steps_list = list(range(50))
        loss_values = [1.2 / (s + 1) for s in steps_list]
        run.log_timeseries("loss_curve", steps_list, loss_values)
```

Add `import numpy as np` at the top of the file (after `import extract`).

- [ ] **Step 2: Regenerate test data**

Run: `cd /home/phil_oh/Projects/Creations/Extract && uv run --directory python python scripts/generate_test_data.py`
Expected: output shows experiments and runs, plus artifact files appear in `.extract/artifacts/`

- [ ] **Step 3: Verify artifacts on disk**

Run: `find .extract/artifacts -type f | head -20`
Expected: `.npy` and `.json` files under run-specific directories

- [ ] **Step 4: Commit**

```bash
git add scripts/generate_test_data.py .extract/
git commit -m "feat: add matrix and timeseries artifacts to test data"
```

---

### Task 2: AppState — Artifact and Metric History Fields

**Files:**
- Modify: `rust/src/app.rs`

- [ ] **Step 1: Add artifact and curve data fields to AppState**

Add these imports at the top of `app.rs`:

```rust
use crate::model::{Artifact, ...existing...};
```

Add new fields to `AppState` struct (after `metrics`):

```rust
    pub artifacts: Vec<Artifact>,
    pub metric_history: Vec<ScalarMetric>,  // full history for selected metric
    pub available_metric_names: Vec<String>,
    pub selected_metric_idx: usize,
```

Initialize them in `AppState::new()` (add to the Self construction):

```rust
            artifacts: Vec::new(),
            metric_history: Vec::new(),
            available_metric_names: Vec::new(),
            selected_metric_idx: 0,
```

- [ ] **Step 2: Add refresh_artifacts and refresh_metric_history methods**

Add to `impl AppState`:

```rust
    pub fn refresh_artifacts(&mut self) -> Result<()> {
        if let Some(run) = self.selected_run.and_then(|i| self.runs.get(i)) {
            self.artifacts = self.db.list_artifacts(&run.id)?;
        } else {
            self.artifacts.clear();
        }
        Ok(())
    }

    pub fn refresh_metric_history(&mut self) -> Result<()> {
        let Some(run) = self.selected_run.and_then(|i| self.runs.get(i)) else {
            self.metric_history.clear();
            self.available_metric_names.clear();
            return Ok(());
        };

        // Get distinct metric names for this run
        let all = self.db.get_scalar_metrics(&run.id, None)?;
        let mut names: Vec<String> = Vec::new();
        for m in &all {
            if !names.contains(&m.name) {
                names.push(m.name.clone());
            }
        }
        self.available_metric_names = names;

        // Load full history for selected metric
        if let Some(name) = self.available_metric_names.get(self.selected_metric_idx) {
            self.metric_history = self.db.get_scalar_metrics(&run.id, Some(name))?;
        } else {
            self.metric_history.clear();
        }

        Ok(())
    }
```

- [ ] **Step 3: Verify it compiles**

Run: `cd /home/phil_oh/Projects/Creations/Extract/rust && cargo check 2>&1 | tail -5`
Expected: compiles (new fields unused in some places is fine)

- [ ] **Step 4: Commit**

```bash
git add rust/src/app.rs
git commit -m "feat: add artifact and metric history fields to AppState"
```

---

### Task 3: Theme — Heatmap Gradient Colors

**Files:**
- Modify: `rust/src/ui/theme.rs`

- [ ] **Step 1: Add heatmap color fields to Theme**

Add after `tab_inactive`:

```rust
    pub heatmap_low: Color,
    pub heatmap_mid: Color,
    pub heatmap_high: Color,
    pub heatmap_zero: Color,
    pub chart_line_1: Color,
    pub chart_line_2: Color,
    pub chart_axis: Color,
```

Initialize in `Default`:

```rust
            heatmap_low: Color::Blue,
            heatmap_mid: Color::Yellow,
            heatmap_high: Color::Green,
            heatmap_zero: Color::DarkGray,
            chart_line_1: Color::Cyan,
            chart_line_2: Color::Magenta,
            chart_axis: Color::DarkGray,
```

- [ ] **Step 2: Verify it compiles**

Run: `cd /home/phil_oh/Projects/Creations/Extract/rust && cargo check 2>&1 | tail -5`

- [ ] **Step 3: Commit**

```bash
git add rust/src/ui/theme.rs
git commit -m "feat: add heatmap and chart color fields to theme"
```

---

### Task 4: Chart Component

**Files:**
- Create: `rust/src/ui/chart.rs`

- [ ] **Step 1: Create chart.rs with loss/metric curve rendering**

Create `rust/src/ui/chart.rs`:

```rust
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::symbols;
use ratatui::widgets::{Axis, Block, Chart, Dataset, GraphType};
use ratatui::Frame;

use crate::model::ScalarMetric;
use crate::ui::theme::Theme;

pub struct ChartView {
    theme: Theme,
}

impl ChartView {
    pub fn new() -> Self {
        Self {
            theme: Theme::default(),
        }
    }

    pub fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        metric_name: &str,
        history: &[ScalarMetric],
    ) {
        if history.is_empty() {
            let msg = ratatui::widgets::Paragraph::new("  No metric data to plot.")
                .style(Style::default().fg(self.theme.accent_dim));
            frame.render_widget(msg, area);
            return;
        }

        let data: Vec<(f64, f64)> = history
            .iter()
            .map(|m| (m.step as f64, m.value))
            .collect();

        let (x_min, x_max) = data.iter().fold((f64::MAX, f64::MIN), |(min, max), (x, _)| {
            (min.min(*x), max.max(*x))
        });
        let (y_min, y_max) = data.iter().fold((f64::MAX, f64::MIN), |(min, max), (_, y)| {
            (min.min(*y), max.max(*y))
        });

        // Add some padding to y-axis
        let y_range = y_max - y_min;
        let y_pad = if y_range > 0.0 { y_range * 0.1 } else { 0.1 };
        let y_lo = y_min - y_pad;
        let y_hi = y_max + y_pad;

        let dataset = Dataset::default()
            .marker(symbols::Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(self.theme.chart_line_1))
            .data(&data);

        let x_labels = vec![
            format!("{:.0}", x_min),
            format!("{:.0}", (x_min + x_max) / 2.0),
            format!("{:.0}", x_max),
        ];
        let y_labels = vec![
            format!("{:.4}", y_lo),
            format!("{:.4}", (y_lo + y_hi) / 2.0),
            format!("{:.4}", y_hi),
        ];

        let chart = Chart::new(vec![dataset])
            .block(Block::default().title(format!(" {metric_name} ")))
            .x_axis(
                Axis::default()
                    .title("step")
                    .style(Style::default().fg(self.theme.chart_axis))
                    .bounds([x_min, x_max])
                    .labels(x_labels),
            )
            .y_axis(
                Axis::default()
                    .style(Style::default().fg(self.theme.chart_axis))
                    .bounds([y_lo, y_hi])
                    .labels(y_labels),
            );

        frame.render_widget(chart, area);
    }
}
```

- [ ] **Step 2: Declare chart module in mod.rs**

In `rust/src/ui/mod.rs`, add after `pub mod dashboard;`:

```rust
pub mod chart;
```

- [ ] **Step 3: Verify it compiles**

Run: `cd /home/phil_oh/Projects/Creations/Extract/rust && cargo check 2>&1 | tail -5`

- [ ] **Step 4: Commit**

```bash
git add rust/src/ui/chart.rs rust/src/ui/mod.rs
git commit -m "feat: add chart component for metric curve rendering"
```

---

### Task 5: Heatmap Component

**Files:**
- Create: `rust/src/ui/heatmap.rs`

- [ ] **Step 1: Create heatmap.rs with matrix rendering via Canvas**

Create `rust/src/ui/heatmap.rs`:

```rust
use ndarray::Array2;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::ui::theme::Theme;

pub struct HeatmapView {
    theme: Theme,
}

impl HeatmapView {
    pub fn new() -> Self {
        Self {
            theme: Theme::default(),
        }
    }

    pub fn render(
        &self,
        frame: &mut Frame,
        area: Rect,
        matrix: &Array2<f64>,
        title: &str,
        axes: Option<(&str, &str)>,
    ) {
        let (rows, cols) = matrix.dim();
        if rows == 0 || cols == 0 {
            let msg = Paragraph::new("  Empty matrix.")
                .style(Style::default().fg(self.theme.accent_dim));
            frame.render_widget(msg, area);
            return;
        }

        // Find min/max for color scaling (ignoring zeros for CL lower-triangular)
        let mut vmin = f64::MAX;
        let mut vmax = f64::MIN;
        for &v in matrix.iter() {
            if v != 0.0 {
                vmin = vmin.min(v);
                vmax = vmax.max(v);
            }
        }
        if vmin > vmax {
            vmin = 0.0;
            vmax = 1.0;
        }

        let mut lines: Vec<Line<'_>> = Vec::new();

        // Title line
        lines.push(Line::from(Span::styled(
            format!("  {title}"),
            Style::default()
                .fg(self.theme.accent)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )));

        // Axis label
        if let Some((_row_label, col_label)) = axes {
            // Column header
            let mut header_spans = vec![Span::raw("       ")]; // row label padding
            for c in 0..cols {
                header_spans.push(Span::styled(
                    format!(" T{:<3}", c + 1),
                    Style::default().fg(self.theme.accent_dim),
                ));
            }
            lines.push(Line::from(header_spans));
        }

        // Each row
        for r in 0..rows {
            let mut spans: Vec<Span<'_>> = Vec::new();

            // Row label
            spans.push(Span::styled(
                format!("  T{:<3} ", r + 1),
                Style::default().fg(self.theme.accent_dim),
            ));

            for c in 0..cols {
                let v = matrix[[r, c]];
                if v == 0.0 {
                    // Zero cells (upper triangle in CL matrices) rendered dim
                    spans.push(Span::styled(
                        "  ·  ",
                        Style::default().fg(self.theme.heatmap_zero),
                    ));
                } else {
                    let color = self.value_to_color(v, vmin, vmax);
                    spans.push(Span::styled(
                        format!(" {:.2}", v),
                        Style::default().fg(color),
                    ));
                }
            }

            lines.push(Line::from(spans));
        }

        // Dimensions info
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  {rows}×{cols}  range: [{vmin:.3}, {vmax:.3}]"),
            Style::default().fg(self.theme.accent_dim),
        )));

        // Truncate if we exceed available height
        let max_lines = area.height as usize;
        if lines.len() > max_lines {
            lines.truncate(max_lines.saturating_sub(1));
            lines.push(Line::from(Span::styled(
                "  ... (matrix truncated)",
                Style::default().fg(self.theme.accent_dim),
            )));
        }

        frame.render_widget(Paragraph::new(lines), area);
    }

    fn value_to_color(&self, value: f64, min: f64, max: f64) -> Color {
        let range = max - min;
        if range == 0.0 {
            return self.theme.heatmap_high;
        }
        let t = (value - min) / range; // 0.0 to 1.0

        if t < 0.5 {
            self.theme.heatmap_low
        } else if t < 0.8 {
            self.theme.heatmap_mid
        } else {
            self.theme.heatmap_high
        }
    }
}
```

- [ ] **Step 2: Declare heatmap module in mod.rs**

In `rust/src/ui/mod.rs`, add after `pub mod chart;`:

```rust
pub mod heatmap;
```

- [ ] **Step 3: Verify it compiles**

Run: `cd /home/phil_oh/Projects/Creations/Extract/rust && cargo check 2>&1 | tail -5`

- [ ] **Step 4: Commit**

```bash
git add rust/src/ui/heatmap.rs rust/src/ui/mod.rs
git commit -m "feat: add heatmap component for matrix visualization"
```

---

### Task 6: Detail Panel — Add Curves and Matrix Tabs

**Files:**
- Modify: `rust/src/ui/detail.rs`

- [ ] **Step 1: Add new tab variants to DetailTab**

Update the `DetailTab` enum:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailTab {
    Scalars,
    Curves,
    Matrix,
    Info,
}
```

- [ ] **Step 2: Add chart and heatmap components to DetailPanel**

Add imports at the top of `detail.rs`:

```rust
use crate::artifact::{load_npy_matrix, load_timeseries, Timeseries};
use crate::model::{Artifact, Run};
use crate::ui::chart::ChartView;
use crate::ui::heatmap::HeatmapView;
use ndarray::Array2;
```

Update the `DetailPanel` struct:

```rust
pub struct DetailPanel {
    pub active_tab: DetailTab,
    pub table_state: TableState,
    chart: ChartView,
    heatmap: HeatmapView,
    cached_matrix: Option<Array2<f64>>,
    cached_matrix_artifact_id: Option<String>,
    cached_matrix_axes: Option<(String, String)>,
    theme: Theme,
}
```

Update `DetailPanel::new()`:

```rust
    pub fn new() -> Self {
        Self {
            active_tab: DetailTab::Scalars,
            table_state: TableState::default(),
            chart: ChartView::new(),
            heatmap: HeatmapView::new(),
            cached_matrix: None,
            cached_matrix_artifact_id: None,
            cached_matrix_axes: None,
            theme: Theme::default(),
        }
    }
```

- [ ] **Step 3: Update tab cycling in handle_key**

Replace the TAB handling in `handle_key`:

```rust
        if keys::matches(key, keys::TAB) {
            self.active_tab = match self.active_tab {
                DetailTab::Scalars => DetailTab::Curves,
                DetailTab::Curves => DetailTab::Matrix,
                DetailTab::Matrix => DetailTab::Info,
                DetailTab::Info => DetailTab::Scalars,
            };
            return Action::None;
        }
```

Add Shift+Tab for reverse cycling after the TAB block:

```rust
        if keys::matches_shift(key, keys::TAB) {
            self.active_tab = match self.active_tab {
                DetailTab::Scalars => DetailTab::Info,
                DetailTab::Curves => DetailTab::Scalars,
                DetailTab::Matrix => DetailTab::Curves,
                DetailTab::Info => DetailTab::Matrix,
            };
            return Action::None;
        }
```

- [ ] **Step 4: Add left/right metric switching for Curves tab**

In the `handle_key` method, add before the QUIT check:

```rust
        if self.active_tab == DetailTab::Curves {
            if keys::matches(key, keys::NAV_DOWN_J) || keys::matches(key, keys::NAV_DOWN) {
                if state.selected_metric_idx + 1 < state.available_metric_names.len() {
                    state.selected_metric_idx += 1;
                    let _ = state.refresh_metric_history();
                }
                return Action::None;
            }
            if keys::matches(key, keys::NAV_UP_K) || keys::matches(key, keys::NAV_UP) {
                if state.selected_metric_idx > 0 {
                    state.selected_metric_idx -= 1;
                    let _ = state.refresh_metric_history();
                }
                return Action::None;
            }
        }
```

This should go **before** the existing j/k handlers for Scalars/Info tabs. The existing j/k handlers should be wrapped in an else condition or the Curves check should return early (which it does).

- [ ] **Step 5: Update render_tab_bar to show all 4 tabs**

Replace the `render_tab_bar` method:

```rust
    fn render_tab_bar(&self, frame: &mut Frame, area: Rect) {
        let tabs = [
            ("Scalars", DetailTab::Scalars),
            ("Curves", DetailTab::Curves),
            ("Matrix", DetailTab::Matrix),
            ("Info", DetailTab::Info),
        ];

        let spans: Vec<Span> = tabs
            .iter()
            .flat_map(|(label, tab)| {
                let style = if *tab == self.active_tab {
                    self.theme.tab_active
                } else {
                    self.theme.tab_inactive
                };
                vec![
                    Span::raw(" "),
                    Span::styled(format!("[{label}]"), style),
                ]
            })
            .collect();

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }
```

- [ ] **Step 6: Update render to dispatch to all 4 tabs**

In the `render` method, update the match on active_tab:

```rust
        match self.active_tab {
            DetailTab::Scalars => self.render_scalars(frame, chunks[1], state),
            DetailTab::Curves => self.render_curves(frame, chunks[1], state),
            DetailTab::Matrix => self.render_matrix(frame, chunks[1], state),
            DetailTab::Info => self.render_info(frame, chunks[1], run),
        }
```

- [ ] **Step 7: Add render_curves method**

Add to `impl DetailPanel`:

```rust
    fn render_curves(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        if state.available_metric_names.is_empty() {
            let msg = Paragraph::new("  No scalar metrics recorded for this run.")
                .style(Style::default().fg(self.theme.accent_dim));
            frame.render_widget(msg, area);
            return;
        }

        // Split: metric selector (1 line) + chart
        let chunks = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);

        // Metric selector
        let metric_name = state
            .available_metric_names
            .get(state.selected_metric_idx)
            .map(|s| s.as_str())
            .unwrap_or("?");

        let selector = Line::from(vec![
            Span::raw("  "),
            Span::styled("◄ ", Style::default().fg(self.theme.accent_dim)),
            Span::styled(
                metric_name,
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ►", Style::default().fg(self.theme.accent_dim)),
            Span::styled(
                format!(
                    "  ({}/{}  j/k to switch)",
                    state.selected_metric_idx + 1,
                    state.available_metric_names.len()
                ),
                Style::default().fg(self.theme.accent_dim),
            ),
        ]);
        frame.render_widget(Paragraph::new(selector), chunks[0]);

        // Chart
        self.chart.render(frame, chunks[1], metric_name, &state.metric_history);
    }
```

- [ ] **Step 8: Add render_matrix method**

Add to `impl DetailPanel`:

```rust
    fn render_matrix(&mut self, frame: &mut Frame, area: Rect, state: &AppState) {
        // Find first matrix artifact for this run
        let matrix_artifact = state.artifacts.iter().find(|a| a.kind == "matrix");

        let Some(artifact) = matrix_artifact else {
            let msg = Paragraph::new("  No matrix artifacts for this run.")
                .style(Style::default().fg(self.theme.accent_dim));
            frame.render_widget(msg, area);
            return;
        };

        // Load matrix if not cached or if artifact changed
        let needs_load = match &self.cached_matrix_artifact_id {
            Some(id) => id != &artifact.id,
            None => true,
        };

        if needs_load {
            let path = state.store_root.join(&artifact.rel_path);
            match load_npy_matrix(&path) {
                Ok(matrix) => {
                    // Parse axes from artifact metadata
                    let axes = artifact.metadata.as_ref().and_then(|m| {
                        let parsed: serde_json::Value = serde_json::from_str(m).ok()?;
                        let axes_obj = parsed.get("axes")?;
                        let rows = axes_obj.get("rows")?.as_str()?.to_string();
                        let cols = axes_obj.get("cols")?.as_str()?.to_string();
                        Some((rows, cols))
                    });
                    self.cached_matrix = Some(matrix);
                    self.cached_matrix_artifact_id = Some(artifact.id.clone());
                    self.cached_matrix_axes = axes;
                }
                Err(e) => {
                    let msg = Paragraph::new(format!("  Error loading matrix: {e}"))
                        .style(Style::default().fg(self.theme.error));
                    frame.render_widget(msg, area);
                    return;
                }
            }
        }

        if let Some(ref matrix) = self.cached_matrix {
            let axes = self.cached_matrix_axes.as_ref().map(|(r, c)| (r.as_str(), c.as_str()));
            self.heatmap.render(frame, area, matrix, &artifact.name, axes);
        }
    }
```

- [ ] **Step 9: Update render signature for render_matrix**

Since `render_matrix` needs `&mut self` (for caching), change the `render` method's signature from `&mut self` to keep it as-is (it already is `&mut self`). Update the match arm for Matrix to use `self.render_matrix(frame, chunks[1], state)`.

Note: the render method already takes `&mut self`, so this works without changes.

- [ ] **Step 10: Verify it compiles**

Run: `cd /home/phil_oh/Projects/Creations/Extract/rust && cargo check 2>&1 | tail -10`

- [ ] **Step 11: Commit**

```bash
git add rust/src/ui/detail.rs
git commit -m "feat: add Curves and Matrix tabs to detail panel"
```

---

### Task 7: Wire Artifact + Metric Loading into Detail Navigation

**Files:**
- Modify: `rust/src/ui/detail.rs` (handle_key)
- Modify: `rust/src/ui/tree.rs` (sync_selection)
- Modify: `rust/src/main.rs` (tick handler)

- [ ] **Step 1: Load artifacts and metric history when entering detail view**

In `rust/src/ui/tree.rs`, find the code that transitions to Detail view (the Enter key handler that sets `state.current_view = View::Detail`). After setting the run and loading metrics, add:

```rust
let _ = state.refresh_artifacts();
let _ = state.refresh_metric_history();
```

- [ ] **Step 2: Clear matrix cache when switching runs**

In `detail.rs`, in the `load_metrics_for_selected_run` method, add cache invalidation:

```rust
    fn load_metrics_for_selected_run(&mut self, state: &mut AppState) {
        if let Some(run_idx) = state.selected_run {
            if let Some(run) = state.runs.get(run_idx) {
                state.metrics = state
                    .db
                    .get_latest_metrics(&run.id)
                    .unwrap_or_default();
            }
        }
        // Invalidate caches
        self.cached_matrix = None;
        self.cached_matrix_artifact_id = None;
        self.cached_matrix_axes = None;
        state.selected_metric_idx = 0;
        let _ = state.refresh_artifacts();
        let _ = state.refresh_metric_history();
    }
```

Note: `load_metrics_for_selected_run` currently takes `&self` — change it to `&mut self`.

- [ ] **Step 3: Update j/k in Info tab to also invalidate caches**

The existing j/k handlers in the Info tab case call `self.load_metrics_for_selected_run(state)`. Since that method now takes `&mut self`, and the caller already has `&mut self`, this should work. Make sure `handle_key` still takes `&mut self` (it already does).

- [ ] **Step 4: Verify it compiles and tests pass**

Run: `cd /home/phil_oh/Projects/Creations/Extract/rust && cargo test 2>&1 | tail -15 && cargo check 2>&1 | tail -5`

- [ ] **Step 5: Commit**

```bash
git add rust/src/ui/detail.rs rust/src/ui/tree.rs rust/src/main.rs
git commit -m "feat: wire artifact and metric history loading into navigation"
```

---

### Task 8: Build, Test, and Verify

- [ ] **Step 1: Run all Rust tests**

Run: `cd /home/phil_oh/Projects/Creations/Extract/rust && cargo test 2>&1 | tail -15`
Expected: all tests pass

- [ ] **Step 2: Run all Python tests**

Run: `cd /home/phil_oh/Projects/Creations/Extract/python && uv run pytest tests/ -v 2>&1 | tail -20`
Expected: all tests pass

- [ ] **Step 3: Full build**

Run: `cd /home/phil_oh/Projects/Creations/Extract/rust && cargo build 2>&1 | tail -10`
Expected: compiles successfully

- [ ] **Step 4: Manual TUI verification**

Run: `cd /home/phil_oh/Projects/Creations/Extract && cargo run --manifest-path rust/Cargo.toml -- --store .extract`

Verify:
1. Navigate to a leaf experiment (e.g., `lambda_1.0`), press Enter on a run
2. **Scalars tab** — metric table renders (existing behavior)
3. **Tab to Curves** — loss curve renders with Braille line chart, j/k switches between loss/accuracy
4. **Tab to Matrix** — accuracy matrix renders as colored text heatmap with row/column labels
5. **Tab to Info** — run info renders (existing behavior)
6. **Navigate to a run without artifacts** — Curves tab shows scalar chart from DB, Matrix tab shows "No matrix artifacts"
7. **Shift+Tab** — tabs cycle in reverse
8. **Esc** — returns to tree

- [ ] **Step 5: Final commit**

```bash
git add -A
git commit -m "feat: complete Phase 2 - visualization (charts and heatmaps)"
```
