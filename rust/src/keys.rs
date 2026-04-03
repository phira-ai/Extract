use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

// Keybinding constants
pub const QUIT: KeyCode = KeyCode::Char('q');
pub const NAV_DOWN_J: KeyCode = KeyCode::Char('j');
pub const NAV_UP_K: KeyCode = KeyCode::Char('k');
pub const NAV_DOWN: KeyCode = KeyCode::Down;
pub const NAV_UP: KeyCode = KeyCode::Up;
pub const SELECT: KeyCode = KeyCode::Enter;
pub const BACK_ESC: KeyCode = KeyCode::Esc;
pub const BACK_BACKSPACE: KeyCode = KeyCode::Backspace;
pub const COMPARE: KeyCode = KeyCode::Char('c');
pub const DIFF: KeyCode = KeyCode::Char('d');
pub const MODELS: KeyCode = KeyCode::Char('m');
pub const TODOS: KeyCode = KeyCode::Char('t');
pub const LINEAGE: KeyCode = KeyCode::Char('l');
pub const TOGGLE_SELECT: KeyCode = KeyCode::Char(' ');
pub const SEARCH: KeyCode = KeyCode::Char('/');
pub const HELP: KeyCode = KeyCode::Char('?');
pub const TAB: KeyCode = KeyCode::Tab;

/// Check if a key event matches a given key code (ignoring modifiers).
pub fn matches(event: &KeyEvent, code: KeyCode) -> bool {
    event.code == code && event.modifiers == KeyModifiers::NONE
}

/// Check if a key event matches a given key code with shift held.
pub fn matches_shift(event: &KeyEvent, code: KeyCode) -> bool {
    event.code == code && event.modifiers == KeyModifiers::SHIFT
}
