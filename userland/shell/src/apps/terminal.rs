use crate::desktop::colors;
use crate::ui::window::Window;

pub struct Terminal {
    pub window: Window,
}

impl Terminal {
    pub fn new() -> Self {
        let _bg = colors::DEFAULT_BG;
        Self {
            window: Window::new(),
        }
    }
}
