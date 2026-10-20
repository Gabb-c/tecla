mod results;
mod typing;

use std::time::Instant;

use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

pub use results::ResultsView;
pub use typing::TypingView;

use crate::app::{App, Screen};

impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        match &self.screen {
            Screen::Typing => TypingView {
                session: &self.session,
                now: Instant::now(),
            }
            .render(area, buf),
            Screen::Results(results) => ResultsView {
                results,
                notice: self.notice.as_deref(),
            }
            .render(area, buf),
        }
    }
}
