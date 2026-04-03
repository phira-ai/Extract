use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::AppState;
use crate::ui::theme::Theme;

pub struct Dashboard {
    theme: Theme,
}

impl Dashboard {
    pub fn new() -> Self {
        Self {
            theme: Theme::default(),
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, state: &AppState) {
        let exp_count = state.experiments.len();
        let run_count = state.runs.len();

        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "  Extract Experiment Tracker",
                self.theme.header,
            )),
            Line::from(""),
            Line::from(format!("  Experiments: {exp_count}")),
            Line::from(format!("  Loaded runs: {run_count}")),
            Line::from(""),
            Line::from(Span::styled(
                "  Navigate the tree and press Enter to select an experiment.",
                Style::default().fg(self.theme.accent_dim),
            )),
        ];

        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, area);
    }
}
