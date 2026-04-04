pub mod chart;
pub mod compare;
pub mod dashboard;
pub mod detail;
pub mod diff;
pub mod heatmap;
pub mod layout;
pub mod statusbar;
pub mod summary;
pub mod theme;
pub mod tree;

use ratatui::layout::Rect;
use ratatui::Frame;

use crate::app::{Action, AppState};
use crate::event::AppEvent;

pub trait Component {
    fn handle_event(&mut self, event: &AppEvent, state: &mut AppState) -> Action;
    fn render(&self, frame: &mut Frame, area: Rect, state: &AppState);
}
