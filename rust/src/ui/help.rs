use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::symbols::border;
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
        let width = 50u16.min(area.width.saturating_sub(4));
        let height = 36u16.min(area.height.saturating_sub(2));
        let popup_area = centered_rect(width, height, area);

        frame.render_widget(Clear, popup_area);

        let block = Block::bordered()
            .title(" ? Help ")
            .border_style(Style::default().fg(self.theme.accent))
            .border_set(border::ROUNDED);

        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);

        let accent_bold = Style::default()
            .fg(self.theme.accent)
            .add_modifier(Modifier::BOLD);
        let accent_dim = Style::default().fg(self.theme.accent_dim);

        let mut lines: Vec<Line> = Vec::new();

        // Explorer section
        lines.push(Line::from(Span::styled(
            " Explorer",
            accent_bold,
        )));
        for (key, desc) in &[
            ("j/k", "navigate"),
            ("Enter", "expand / select"),
            ("Space", "mark run"),
            ("c", "compare marked"),
            ("d", "diff marked"),
            ("/", "search"),
            ("1/2/3", "focus panels"),
            ("Tab", "next panel"),
        ] {
            lines.push(binding_line(key, desc, accent_bold, accent_dim));
        }

        lines.push(Line::raw(""));

        // Detail Panel section
        lines.push(Line::from(Span::styled(
            " Detail Panel",
            accent_bold,
        )));
        for (key, desc) in &[
            ("h/l", "cycle runs"),
            ("S/I", "summary / info tab"),
            ("x", "delete run"),
        ] {
            lines.push(binding_line(key, desc, accent_bold, accent_dim));
        }

        lines.push(Line::raw(""));

        // Views section
        lines.push(Line::from(Span::styled(
            " Views",
            accent_bold,
        )));
        for (key, desc) in &[
            ("M", "model registry"),
            ("T", "TODOs"),
            ("L", "lineage DAG"),
        ] {
            lines.push(binding_line(key, desc, accent_bold, accent_dim));
        }

        lines.push(Line::raw(""));

        // TODO View section
        lines.push(Line::from(Span::styled(
            " TODO View",
            accent_bold,
        )));
        for (key, desc) in &[
            ("Space", "toggle done"),
            ("a", "add"),
            ("x", "delete"),
            ("0/1/2", "priority"),
            ("A/G/E/R", "filter scope"),
        ] {
            lines.push(binding_line(key, desc, accent_bold, accent_dim));
        }

        lines.push(Line::raw(""));

        // Global section
        lines.push(Line::from(Span::styled(
            " Global",
            accent_bold,
        )));
        for (key, desc) in &[
            ("?", "toggle help"),
            ("q", "quit"),
            ("Esc", "back / close"),
        ] {
            lines.push(binding_line(key, desc, accent_bold, accent_dim));
        }

        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, inner);
    }
}

fn binding_line<'a>(
    key: &'a str,
    description: &'a str,
    key_style: Style,
    desc_style: Style,
) -> Line<'a> {
    Line::from(vec![
        Span::raw("   "),
        Span::styled(format!("{:<14}", key), key_style),
        Span::styled(description.to_string(), desc_style),
    ])
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}
