use log::info;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    Normal,
    Insert,
    Command,
    Hint,
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Mode::Normal => write!(f, "NORMAL"),
            Mode::Insert => write!(f, "INSERT"),
            Mode::Command => write!(f, "COMMAND"),
            Mode::Hint => write!(f, "HINT"),
        }
    }
}

pub type ModeState = Rc<RefCell<Mode>>;
pub type HintBuffer = Rc<RefCell<String>>;
pub type NewTabFlag = Rc<RefCell<bool>>;

pub fn new_mode_state() -> ModeState {
    info!("initializing mode state with Normal mode");
    Rc::new(RefCell::new(Mode::Normal))
}

pub fn new_hint_buffer() -> HintBuffer {
    Rc::new(RefCell::new(String::new()))
}

pub fn new_tab_flag() -> NewTabFlag {
    Rc::new(RefCell::new(false))
}

pub fn set_mode(state: &ModeState, mode: Mode) {
    info!("mode transition: {} -> {}", *state.borrow(), mode);
    *state.borrow_mut() = mode;
}

pub fn current_mode(state: &ModeState) -> Mode {
    *state.borrow()
}
