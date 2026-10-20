mod results;
mod settings;
mod typing;

use std::time::Instant;

use ratatui::{
    Frame,
    style::{Style, Stylize},
    widgets::{Block, BorderType},
};

pub use results::ResultsView;
pub use settings::SettingsView;
pub use typing::TypingView;

use crate::app::{App, Screen};
use crate::config::{Caret, ConfigFile};
use crate::engine::Limit;

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
                config: &app.config,
                now: Instant::now(),
                notice: app.config_notice.as_deref(),
            };
            if app.config.caret != Caret::Off
                && let Some(caret) = view.caret_position(area)
            {
                frame.set_cursor_position(caret);
            }
            frame.render_widget(view, area);
        }
        Screen::Settings { selected } => frame.render_widget(
            SettingsView {
                config: &app.config,
                selected: *selected,
                path: app.config_file.as_ref().map(ConfigFile::path),
                notice: app.config_notice.as_deref(),
            },
            area,
        ),
        Screen::Results(results) => frame.render_widget(
            ResultsView {
                results,
                test: test_name(app.session.limit(), app.session.words().len()),
                notice: app.notice.as_deref(),
            },
            area,
        ),
    }
}

fn test_name(limit: Limit, words: usize) -> String {
    match limit {
        Limit::Time(duration) => format!("time {}", duration.as_secs()),
        Limit::Words => format!("words {words}"),
    }
}
