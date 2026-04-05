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

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let cli = Cli::parse();

    let db_path = std::path::Path::new(&cli.store).join("extract.db");
    let db = db::Db::open(&db_path)?;
    let mut app = app::AppState::new(db, std::path::PathBuf::from(&cli.store))?;
    let mut events = event::EventHandler::new(Duration::from_millis(500));
    let mut layout = AppLayout::new(&app.config);

    // Setup terminal
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    // Main loop
    loop {
        terminal.draw(|frame| {
            layout.render(frame, &mut app);
        })?;

        let event = events.next().await?;

        match &event {
            AppEvent::Tick => {
                // Periodically refresh data from DB
                let _ = app.refresh_experiments();
                if app.selected_experiment.is_some() {
                    let _ = app.refresh_runs();
                }
                let _ = app.refresh_selection_summary();
                // Clear expired notifications
                app.clear_expired_notification(app.config.notifications.timeout);
            }
            AppEvent::Resize((), ()) => {
                // Terminal will re-render on next loop iteration
            }
            AppEvent::Key(_) => {}
        }

        let action = layout.handle_event(&event, &mut app);

        match action {
            Action::Quit => break,
            Action::Navigate(view) => {
                app.current_view = view;
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
