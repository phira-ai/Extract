mod app;
mod artifact;
mod config;
mod db;
mod event;
mod keys;
mod model;
mod ui;

use std::time::Duration;

use clap::Parser;
use event::AppEvent;

use crate::app::Action;
use crate::ui::layout::AppLayout;

#[derive(Parser)]
#[command(name = "extract-tui", about = "Extract experiment tracker TUI")]
struct Cli {
    /// Path to the .extract directory
    #[arg(short, long, default_value = ".extract")]
    store: String,
}

fn open_editor(store_root: &std::path::Path, table: &str, id: &str) -> color_eyre::Result<()> {
    assert!(table == "runs" || table == "experiments");
    let db_path = store_root.join("extract.db");
    let conn = rusqlite::Connection::open(&db_path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    let current: Option<String> = conn.query_row(
        &format!("SELECT notes FROM {table} WHERE id = ?"),
        rusqlite::params![id],
        |row| row.get(0),
    )?;

    let tmp_path = std::env::temp_dir().join(format!("extract_notes_{id}.md"));
    std::fs::write(&tmp_path, current.as_deref().unwrap_or(""))?;

    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "nvim".to_string());

    let status = std::process::Command::new(&editor)
        .arg(&tmp_path)
        .status()?;

    if status.success() {
        let new_content = std::fs::read_to_string(&tmp_path)?;
        db::Db::replace_notes(&db_path, table, id, new_content.trim_end())?;
    }

    let _ = std::fs::remove_file(&tmp_path);
    Ok(())
}

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let cli = Cli::parse();

    let db_path = std::path::Path::new(&cli.store).join("extract.db");
    let db = db::Db::open(&db_path)?;
    let mut app = app::AppState::new(db, std::path::PathBuf::from(&cli.store))?;
    let session = app::Session::load(std::path::Path::new(&cli.store));
    let mut events = event::EventHandler::new(Duration::from_millis(500));
    let mut layout = AppLayout::new(&app.config);

    // Restore detail tab from session.
    if session.detail_tab == "info" {
        layout.detail.active_tab = crate::ui::detail::DetailTab::Info;
    }

    // Restore selected run after the first render applies pending_tree_select.
    let restore_run_id = session.run_id.clone();

    // Setup terminal
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    // Main loop
    let mut first_render = true;
    loop {
        terminal.draw(|frame| {
            layout.render(frame, &mut app);
        })?;

        // After the first render, pending_tree_select has been applied by the
        // tree widget. Now load the experiment's data (runs, summary, preview)
        // which sync_selection normally does on navigation.
        if first_render {
            first_render = false;
            if let Some(ref exp_path) = session.experiment_path {
                if let Some(idx) = app.experiments.iter().position(|e| e.path == *exp_path) {
                    app.selected_experiment = Some(idx);
                    let _ = app.refresh_runs();
                    let _ = app.refresh_selection_summary();
                    // Load preview for leaf experiments.
                    let exp_id = app.experiments[idx].id.clone();
                    let has_children = app.experiments.iter()
                        .any(|e| e.parent_id.as_deref() == Some(exp_id.as_str()));
                    if !has_children {
                        let _ = app.refresh_leaf_preview();
                    }
                    // Restore selected run within the experiment.
                    if let Some(ref run_id) = restore_run_id {
                        if let Some(ri) = app.runs.iter().position(|r| r.id == *run_id) {
                            app.selected_run = Some(ri);
                            let _ = app.load_run_preview(ri);
                            app.metrics = app.db.get_latest_metrics(run_id).unwrap_or_default();
                        }
                    }
                    if session.focus == "detail" {
                        app.focus = app::Focus::Detail;
                    }
                }
            }
        }

        let event = events.next().await?;

        match &event {
            AppEvent::Tick => {
                // Only do refresh work when the DB has actually changed.
                if let Ok(v) = app.db.data_version() {
                    if v != app.last_data_version {
                        app.last_data_version = v;
                        let _ = app.refresh_live();
                    }
                }
                // Clear expired notifications (always — independent of DB state).
                app.clear_expired_notification(app.config.notifications.timeout);
            }
            AppEvent::Resize((), ()) => {
                // Terminal will re-render on next loop iteration
            }
            AppEvent::Key(_) => {}
        }

        let action = layout.handle_event(&event, &mut app);

        match action {
            Action::Quit => {
                let detail_tab = match layout.detail.active_tab {
                    crate::ui::detail::DetailTab::Summary => "summary",
                    crate::ui::detail::DetailTab::Info => "info",
                };
                app.save_session(detail_tab);
                break;
            }
            Action::Navigate(view) => {
                app.current_view = view;
            }
            Action::SuspendForEditor { table, id } => {
                // Drop the event handler to stop its background task from
                // polling stdin via crossterm — otherwise it fights with the
                // editor for terminal input and causes severe lag.
                drop(events);

                // Suspend terminal
                crossterm::terminal::disable_raw_mode()?;
                crossterm::execute!(
                    terminal.backend_mut(),
                    crossterm::terminal::LeaveAlternateScreen
                )?;

                let result = open_editor(&app.store_root, &table, &id);

                // Resume terminal
                crossterm::execute!(
                    terminal.backend_mut(),
                    crossterm::terminal::EnterAlternateScreen
                )?;
                crossterm::terminal::enable_raw_mode()?;
                terminal.clear()?;

                // Restart the event handler.
                events = event::EventHandler::new(Duration::from_millis(500));

                if let Err(e) = result {
                    app.notify(app::NotifyLevel::Error, format!("Editor failed: {e}"));
                }
            }
            Action::None => {}
        }
    }

    // Restore terminal
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;

    Ok(())
}
