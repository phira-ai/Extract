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
    /// Chart width as percentage of panel width (1-100, default 80).
    #[serde(default = "default_curve_width")]
    pub curve_width: u8,
}

impl Default for CompareConfig {
    fn default() -> Self {
        Self {
            sections: default_compare_sections(),
            curve_width: default_curve_width(),
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
pub struct ThemeConfig {
    pub fg: Option<String>,
    pub bg: Option<String>,
    pub accent: Option<String>,
    pub accent_dim: Option<String>,
    pub success: Option<String>,
    pub warning: Option<String>,
    pub error: Option<String>,
    pub border: Option<String>,
    pub border_focused: Option<String>,
}

/// Parse a hex color string like "#89b4fa" into a ratatui Color.
pub fn parse_hex_color(s: &str) -> Option<Color> {
    let s = s.strip_prefix('#').unwrap_or(s);
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct MetricsConfig {
    /// Metrics where lower values are better (e.g. "forgetting_rate").
    #[serde(default)]
    pub minimize: Vec<String>,
    /// Metrics where higher values are better (e.g. "custom_score").
    #[serde(default)]
    pub maximize: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct InfoConfig {
    /// Glob patterns for which config keys to show (e.g. ["method.*", "task.num_train_epochs"]).
    /// If empty, all keys are shown.
    #[serde(default)]
    pub fields: Vec<String>,
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
    #[serde(default)]
    pub theme: ThemeConfig,
    #[serde(default)]
    pub metrics: MetricsConfig,
    #[serde(default)]
    pub info: InfoConfig,
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

/// Check if a dotted config key matches a glob pattern.
///
/// Dots are treated as path separators so that glob semantics apply per segment:
/// - `*` matches within a single segment (e.g. `method.*` matches `method.name` but not `method.a.b`)
/// - `**` matches across segments (e.g. `method.**` matches `method.a.b`)
/// - `{a,b}` alternation, `?` single char, and character classes work as expected.
///
/// Negation: patterns starting with `!` exclude matching keys.
pub fn key_matches_glob(key: &str, pattern: &str) -> bool {
    let (negate, pat) = if let Some(rest) = pattern.strip_prefix('!') {
        (true, rest)
    } else {
        (false, pattern)
    };
    // Translate dots to slashes so glob_match treats them as path separators.
    let key_path = key.replace('.', "/");
    let pat_path = pat.replace('.', "/");
    let matched = glob_match::glob_match(&pat_path, &key_path);
    if negate { !matched } else { matched }
}

/// Check if a key passes a list of field filter patterns.
/// Empty filters = pass everything. Negation patterns (`!foo`) are AND'd with positive matches.
pub fn key_passes_filters(key: &str, filters: &[String]) -> bool {
    if filters.is_empty() {
        return true;
    }
    let positive: Vec<&str> = filters.iter().map(|s| s.as_str()).filter(|s| !s.starts_with('!')).collect();
    let negative: Vec<&str> = filters.iter()
        .filter_map(|s| s.strip_prefix('!'))
        .collect();

    // Must match at least one positive pattern (if any exist).
    let included = positive.is_empty() || positive.iter().any(|pat| key_matches_glob(key, pat));
    // Must not match any negation pattern (checked without the ! prefix).
    let excluded = negative.iter().any(|pat| key_matches_glob(key, pat));
    included && !excluded
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

    #[test]
    fn test_parse_info_fields() {
        let content = r#"
[info]
fields = ["method.*", "task.num_train_epochs"]
"#;
        let config: Config = toml::from_str(content).expect("Failed to parse config");
        assert_eq!(config.info.fields, vec!["method.*", "task.num_train_epochs"]);
    }

    #[test]
    fn test_info_fields_default_empty() {
        let config: Config = toml::from_str("").expect("Failed to parse empty config");
        assert!(config.info.fields.is_empty());
    }

    #[test]
    fn test_key_matches_glob() {
        // Exact match
        assert!(key_matches_glob("task.num_train_epochs", "task.num_train_epochs"));
        assert!(!key_matches_glob("task.num_train_epochs", "task.num_train"));

        // * matches single segment only
        assert!(key_matches_glob("method.name", "method.*"));
        assert!(key_matches_glob("method.lora_r", "method.*"));
        assert!(!key_matches_glob("method.deep.nested", "method.*"));
        assert!(!key_matches_glob("model.name", "method.*"));

        // ** matches across segments
        assert!(key_matches_glob("method.deep.nested", "method.**"));
        assert!(key_matches_glob("method.name", "method.**"));

        // Leading wildcard
        assert!(key_matches_glob("method.name", "*.name"));
        assert!(key_matches_glob("model.name", "*.name"));
        assert!(!key_matches_glob("method.deep.name", "*.name"));
        assert!(key_matches_glob("method.deep.name", "**.name"));

        // Infix wildcard in segment
        assert!(key_matches_glob("method.lora_r", "method.lora_*"));
        assert!(key_matches_glob("method.lora_alpha", "method.lora_*"));
        assert!(!key_matches_glob("method.name", "method.lora_*"));

        // ? single char
        assert!(key_matches_glob("method.lora_r", "method.lora_?"));
        assert!(!key_matches_glob("method.lora_alpha", "method.lora_?"));

        // {a,b} alternation
        assert!(key_matches_glob("method.name", "{method,model}.name"));
        assert!(key_matches_glob("model.name", "{method,model}.name"));
        assert!(!key_matches_glob("task.name", "{method,model}.name"));

        // Negation
        assert!(!key_matches_glob("method.name", "!method.*"));
        assert!(key_matches_glob("model.name", "!method.*"));
    }

    #[test]
    fn test_key_passes_filters() {
        // Empty filters = pass all
        assert!(key_passes_filters("anything", &[]));

        // Positive only
        let filters = vec!["method.*".to_string(), "task.num_train_epochs".to_string()];
        assert!(key_passes_filters("method.name", &filters));
        assert!(key_passes_filters("task.num_train_epochs", &filters));
        assert!(!key_passes_filters("model.name", &filters));

        // Positive + negation
        let filters = vec!["method.**".to_string(), "!method.parent".to_string()];
        assert!(key_passes_filters("method.name", &filters));
        assert!(key_passes_filters("method.lora.alpha", &filters));
        assert!(!key_passes_filters("method.parent", &filters));

        // Negation only (no positive = include all, then exclude)
        let filters = vec!["!method.parent".to_string()];
        assert!(key_passes_filters("method.name", &filters));
        assert!(!key_passes_filters("method.parent", &filters));
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
