use crate::desktop::colors;
use crate::widgets::button::Button;

pub struct Taskbar {
    pub button: Button,
}

impl Taskbar {
    pub fn new() -> Self {
        let _bg = colors::DEFAULT_BG;
        Self {
            button: Button::new(),
        }
    }
}
