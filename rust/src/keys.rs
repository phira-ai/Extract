use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

// Keybinding constants
pub const QUIT: KeyCode = KeyCode::Char('q');
pub const NAV_DOWN_J: KeyCode = KeyCode::Char('j');
pub const NAV_UP_K: KeyCode = KeyCode::Char('k');
pub const NAV_DOWN: KeyCode = KeyCode::Down;
pub const NAV_UP: KeyCode = KeyCode::Up;
pub const SELECT: KeyCode = KeyCode::Enter;
pub const BACK_ESC: KeyCode = KeyCode::Esc;
pub const COMPARE: KeyCode = KeyCode::Char('c');
pub const DIFF: KeyCode = KeyCode::Char('d');
pub const REGISTRY: KeyCode = KeyCode::Char('M');
pub const PRIORITY_0: KeyCode = KeyCode::Char('0');
pub const PRIORITY_1: KeyCode = KeyCode::Char('1');
pub const PRIORITY_2: KeyCode = KeyCode::Char('2');
pub const TODOS: KeyCode = KeyCode::Char('T');
pub const LINEAGE: KeyCode = KeyCode::Char('L');
pub const ADD: KeyCode = KeyCode::Char('a');
pub const TOGGLE_SELECT: KeyCode = KeyCode::Char(' ');
pub const TAB: KeyCode = KeyCode::Tab;
pub const BACKTAB: KeyCode = KeyCode::BackTab;
pub const CYCLE_PREV: KeyCode = KeyCode::Char('h');
pub const CYCLE_NEXT: KeyCode = KeyCode::Char('l');
pub const SUMMARY_TAB: KeyCode = KeyCode::Char('S');
pub const INFO_TAB: KeyCode = KeyCode::Char('I');
pub const PANEL_1: KeyCode = KeyCode::Char('1');
pub const PANEL_2: KeyCode = KeyCode::Char('2');
pub const PANEL_3: KeyCode = KeyCode::Char('3');
pub const DELETE: KeyCode = KeyCode::Char('x');
pub const BASELINE: KeyCode = KeyCode::Char('b');
pub const YES: KeyCode = KeyCode::Char('y');
pub const COMPARE_TAB: KeyCode = KeyCode::Char('C');
pub const DIFF_TAB: KeyCode = KeyCode::Char('D');
pub const SEARCH: KeyCode = KeyCode::Char('/');
pub const HELP: KeyCode = KeyCode::Char('?');
pub const GO_TOP_G: KeyCode = KeyCode::Char('g');
pub const GO_BOTTOM: KeyCode = KeyCode::Char('G');
pub const RUN_BROWSER: KeyCode = KeyCode::Char('r');

/// Check if a key event matches a given key code (ignoring modifiers).
pub fn matches(event: &KeyEvent, code: KeyCode) -> bool {
    if code == KeyCode::BackTab {
        // crossterm sends BackTab with SHIFT modifier
        return event.code == code && event.modifiers == KeyModifiers::SHIFT;
    }
    event.code == code && event.modifiers == KeyModifiers::NONE
}

/// Check if a key event matches a given key code with shift held.
pub fn matches_shift(event: &KeyEvent, code: KeyCode) -> bool {
    event.code == code && event.modifiers == KeyModifiers::SHIFT
}
