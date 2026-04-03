use ratatui::style::{Color, Modifier, Style};

pub struct Theme {
    pub fg: Color,
    pub bg: Color,
    pub accent: Color,
    pub accent_dim: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub border: Color,
    pub border_focused: Color,
    pub header: Style,
    pub selected: Style,
    pub tree_branch: Style,
    pub metric_positive: Style,
    pub metric_negative: Style,
    pub status_running: Style,
    pub status_completed: Style,
    pub status_failed: Style,
    pub tab_active: Style,
    pub tab_inactive: Style,
    pub heatmap_low: Color,
    pub heatmap_mid: Color,
    pub heatmap_high: Color,
    pub heatmap_zero: Color,
    pub chart_line_1: Color,
    pub chart_line_2: Color,
    pub chart_axis: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            fg: Color::White,
            bg: Color::Reset,
            accent: Color::Cyan,
            accent_dim: Color::DarkGray,
            success: Color::Green,
            warning: Color::Yellow,
            error: Color::Red,
            border: Color::DarkGray,
            border_focused: Color::Cyan,
            header: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            selected: Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            tree_branch: Style::default().fg(Color::DarkGray),
            metric_positive: Style::default().fg(Color::Green),
            metric_negative: Style::default().fg(Color::Red),
            status_running: Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
            status_completed: Style::default().fg(Color::Green),
            status_failed: Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),
            tab_active: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            tab_inactive: Style::default().fg(Color::DarkGray),
            heatmap_low: Color::Blue,
            heatmap_mid: Color::Yellow,
            heatmap_high: Color::Green,
            heatmap_zero: Color::DarkGray,
            chart_line_1: Color::Cyan,
            chart_line_2: Color::Magenta,
            chart_axis: Color::DarkGray,
        }
    }
}
