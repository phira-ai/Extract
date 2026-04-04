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
    #[serde(default = "default_sections")]
    pub sections: Vec<SummarySection>,
    /// Chart width as percentage of panel width (1-100, default 80).
    #[serde(default = "default_curve_width")]
    pub curve_width: u8,
    /// Smooth curves with Catmull-Rom interpolation (default false).
    #[serde(default)]
    pub curve_smooth: bool,
}

fn default_sections() -> Vec<SummarySection> {
    vec![
        SummarySection::Runs,
        SummarySection::Metrics,
        SummarySection::Tables,
        SummarySection::Curves,
    ]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CompareSection {
    Pivot,
    Config,
    Tables,
    Curves,
}

fn default_compare_sections() -> Vec<CompareSection> {
    vec![
        CompareSection::Pivot,
        CompareSection::Config,
        CompareSection::Tables,
        CompareSection::Curves,
    ]
}

#[derive(Debug, Clone, Deserialize)]
pub struct CompareConfig {
    #[serde(default = "default_compare_sections")]
    pub sections: Vec<CompareSection>,
}

impl Default for CompareConfig {
    fn default() -> Self {
        Self {
            sections: default_compare_sections(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct NotificationsConfig {
    #[serde(default = "default_notification_timeout")]
    pub timeout: u64,
}

fn default_notification_timeout() -> u64 {
    3
}

impl Default for NotificationsConfig {
    fn default() -> Self {
        Self {
            timeout: default_notification_timeout(),
        }
    }
}

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
}

/// Parse a color name string into a ratatui Color.
pub fn parse_color(name: &str) -> Color {
    match name.to_lowercase().as_str() {
        "none" | "reset" | "default" => Color::Reset,
        "orange" => Color::Rgb(255, 165, 0),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_config_toml() {
        let content = r#"
[summary]
curve_smooth = true

[tables]
[[tables.highlight]]
eq = 0.00
color = "none"

[[tables.highlight]]
min = 0.7
color = "red"

[[tables.highlight]]
min = 0.5
max = 0.7
color = "orange"

[[tables.highlight]]
min = 0.3
max = 0.5
color = "yellow"

[[tables.highlight]]
max = 0.3
color = "white"
"#;
        let config: Config = toml::from_str(content).expect("Failed to parse config");
        assert!(config.summary.curve_smooth);
        assert_eq!(config.tables.highlight.len(), 5);

        let r0 = &config.tables.highlight[0];
        assert_eq!(r0.eq, Some(0.0));
        assert_eq!(r0.color, "none");

        let r1 = &config.tables.highlight[1];
        assert_eq!(r1.min, Some(0.7));
        assert_eq!(r1.max, None);
        assert_eq!(r1.color, "red");

        // Check color parsing
        assert_eq!(parse_color("red"), Color::Red);
        assert_eq!(parse_color("orange"), Color::Rgb(255, 165, 0));
        assert_eq!(parse_color("none"), Color::Reset);

        // Default compare config
        assert_eq!(
            config.compare.sections,
            vec![
                CompareSection::Pivot,
                CompareSection::Config,
                CompareSection::Tables,
                CompareSection::Curves,
            ]
        );
    }

    #[test]
    fn test_parse_compare_config() {
        let content = r#"
[compare]
sections = ["pivot", "curves"]
"#;
        let config: Config = toml::from_str(content).expect("Failed to parse config");
        assert_eq!(
            config.compare.sections,
            vec![CompareSection::Pivot, CompareSection::Curves]
        );
    }
}

pub fn load_config(store_root: &Path) -> Config {
    let config_path = store_root.join("config.toml");
    if config_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            match toml::from_str::<Config>(&content) {
                Ok(config) => return config,
                Err(e) => eprintln!("Warning: failed to parse {}: {e}", config_path.display()),
            }
        }
    }
    Config::default()
}
