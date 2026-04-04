use std::path::Path;

use ratatui::style::Color;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SummarySection {
    Runs,
    Metrics,
    Curves,
    Tables,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SummaryConfig {
    pub sections: Vec<SummarySection>,
    /// Chart width as percentage of panel width (1-100, default 80).
    #[serde(default = "default_curve_width")]
    pub curve_width: u8,
    /// Smooth curves with Catmull-Rom interpolation (default false).
    #[serde(default)]
    pub curve_smooth: bool,
}

fn default_curve_width() -> u8 {
    80
}

impl Default for SummaryConfig {
    fn default() -> Self {
        Self {
            sections: vec![
                SummarySection::Runs,
                SummarySection::Metrics,
                SummarySection::Tables,
                SummarySection::Curves,
            ],
            curve_width: default_curve_width(),
            curve_smooth: false,
        }
    }
}

/// A single highlight rule for table cells.
/// Rules are evaluated in order; first match wins.
#[derive(Debug, Clone, Deserialize)]
pub struct HighlightRule {
    /// Exact value match. Takes precedence over min/max.
    pub eq: Option<f64>,
    /// Minimum value (inclusive). Applies to float and int cells.
    pub min: Option<f64>,
    /// Maximum value (exclusive). Applies to float and int cells.
    pub max: Option<f64>,
    /// Substring match for string cells.
    pub pattern: Option<String>,
    /// Color name: "red", "green", "yellow", "blue", "cyan", "magenta", "white", "darkgray",
    /// or "none"/"reset" for default terminal color (effectively hides highlighting).
    pub color: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TablesConfig {
    /// Ordered highlight rules for table cell coloring.
    #[serde(default)]
    pub highlight: Vec<HighlightRule>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub summary: SummaryConfig,
    #[serde(default)]
    pub tables: TablesConfig,
}

/// Parse a color name string into a ratatui Color.
pub fn parse_color(name: &str) -> Color {
    match name.to_lowercase().as_str() {
        "none" | "reset" | "default" => Color::Reset,
        "orange" => Color::LightRed,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "cyan" => Color::Cyan,
        "magenta" => Color::Magenta,
        "white" => Color::White,
        "black" => Color::Black,
        "darkgray" | "dark_gray" => Color::DarkGray,
        "lightred" | "light_red" => Color::LightRed,
        "lightgreen" | "light_green" => Color::LightGreen,
        "lightyellow" | "light_yellow" => Color::LightYellow,
        "lightblue" | "light_blue" => Color::LightBlue,
        "lightcyan" | "light_cyan" => Color::LightCyan,
        "lightmagenta" | "light_magenta" => Color::LightMagenta,
        _ => Color::Reset, // fallback to terminal default
    }
}

pub fn load_config(store_root: &Path) -> Config {
    let config_path = store_root.join("config.toml");
    if config_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            if let Ok(config) = toml::from_str(&content) {
                return config;
            }
        }
    }
    Config::default()
}
