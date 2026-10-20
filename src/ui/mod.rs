mod results;
mod typing;

use std::time::Instant;

use ratatui::{
    Frame,
    style::{Style, Stylize},
    widgets::{Block, BorderType},
};

pub use results::ResultsView;
pub use typing::TypingView;

use crate::app::{App, Screen};

/// Renders the current screen and places the terminal cursor on the typing caret.
pub fn draw(app: &App, frame: &mut Frame) {
    let border = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().dark_gray())
        .title(" tecla ".bold());
    let area = border.inner(frame.area());
    frame.render_widget(border, frame.area());
    match &app.screen {
        Screen::Typing => {
            let view = TypingView {
                session: &app.session,
                now: Instant::now(),
            };
            if let Some(caret) = view.caret_position(area) {
                frame.set_cursor_position(caret);
            }
            frame.render_widget(view, area);
        }
        Screen::Results(results) => frame.render_widget(
            ResultsView {
                results,
                notice: app.notice.as_deref(),
            },
            area,
        ),
    }
}
