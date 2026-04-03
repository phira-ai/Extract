mod app;
mod artifact;
mod db;
mod event;
mod keys;
mod model;

use std::time::Duration;

use clap::Parser;
use event::AppEvent;

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
    let mut _app = app::AppState::new(db, std::path::PathBuf::from(&cli.store))?;
    let mut events = event::EventHandler::new(Duration::from_millis(500));

    // Setup terminal
    crossterm::terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;

    // Main loop (placeholder - UI rendering comes in Task 4)
    loop {
        terminal.draw(|frame| {
            let area = frame.area();
            let block = ratatui::widgets::Block::default()
                .title(" Extract ")
                .borders(ratatui::widgets::Borders::ALL);
            frame.render_widget(block, area);
        })?;

        match events.next().await? {
            AppEvent::Key(key) => {
                if key.code == crossterm::event::KeyCode::Char('q') {
                    break;
                }
            }
            AppEvent::Tick => {
                // Will refresh data periodically
            }
            AppEvent::Resize(_, _) => {}
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
