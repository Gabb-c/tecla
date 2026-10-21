mod chart;
mod history;
mod popup;
mod results;
mod settings;
mod typing;

use std::time::Instant;

use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType},
};

pub use history::HistoryView;
pub use popup::KeysPopup;
pub use results::ResultsView;
pub use settings::SettingsView;
pub use typing::TypingView;

use crate::app::{App, Finished, Screen};
use crate::config::{Caret, ConfigFile};
use crate::history::Test;
use crate::keymap::{self, View};

/// The screens listed in the border, each covering one or more views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Test,
    History,
    Settings,
}

impl Tab {
    const ALL: [Self; 3] = [Self::Test, Self::History, Self::Settings];

    fn of(view: View) -> Self {
        match view {
            View::Typing | View::Menu | View::Results => Self::Test,
            View::History => Self::History,
            View::Settings => Self::Settings,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Test => "test",
            Self::History => "history",
            Self::Settings => "settings",
        }
    }
}

/// Renders the current screen and places the terminal cursor on the typing caret.
pub fn draw(app: &App, frame: &mut Frame) {
    let view = app.screen.view();
    let mut border = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().dark_gray())
        .title(" tecla ".bold())
        .title(tabs(view).right_aligned());
    // the menu already lists its keys
    let show_hints = match view {
        View::Typing => app.config.key_tips,
        View::Menu => false,
        View::Results | View::Settings | View::History => true,
    };
    if show_hints {
        border = border.title_bottom(hints(view, app.session.is_started()).centered());
    }
    let area = border.inner(frame.area());
    frame.render_widget(border, frame.area());
    match &app.screen {
        Screen::Typing => draw_typing(app, frame, area, true),
        Screen::Menu => {
            draw_typing(app, frame, area, false);
            frame.buffer_mut().set_style(area, Style::new().dim());
            frame.render_widget(
                KeysPopup {
                    title: " menu ",
                    view: View::Menu,
                },
                area,
            );
        }
        Screen::Settings { selected, .. } => frame.render_widget(
            SettingsView {
                config: &app.config,
                selected: *selected,
                path: app.config_file.as_ref().map(ConfigFile::path),
                notice: app.config_notice.as_deref(),
            },
            area,
        ),
        Screen::Results(Finished { results, pb }) => frame.render_widget(
            ResultsView {
                results,
                timeline: &app.timeline,
                test: Test::of(&app.session),
                pb: *pb,
                notice: app.notice.as_deref(),
            },
            area,
        ),
        Screen::History { summaries, .. } => frame.render_widget(
            HistoryView {
                summaries,
                current: Test::of(&app.session),
                notice: app.history_notice.as_deref(),
            },
            area,
        ),
    }
    if app.help {
        frame.render_widget(
            KeysPopup {
                title: " keys ",
                view,
            },
            area,
        );
    }
}

fn draw_typing(app: &App, frame: &mut Frame, area: Rect, caret: bool) {
    let view = TypingView {
        session: &app.session,
        config: &app.config,
        lengths: &app.lengths,
        now: Instant::now(),
        notice: app.config_notice.as_deref(),
    };
    if let Some(position) = view.render_with_caret(area, frame.buffer_mut())
        && caret
        && app.config.caret != Caret::Off
    {
        frame.set_cursor_position(position);
    }
}

/// The screens, with the one `view` belongs to highlighted.
fn tabs(view: View) -> Line<'static> {
    let current = Tab::of(view);
    let mut spans = vec![Span::raw(" ")];
    for (i, tab) in Tab::ALL.into_iter().enumerate() {
        if i > 0 {
            spans.push(" · ".dark_gray());
        }
        spans.push(if tab == current {
            tab.label().yellow().bold()
        } else {
            tab.label().dark_gray()
        });
    }
    spans.push(Span::raw(" "));
    Line::from(spans)
}

/// The keys of `view`; the next test's mode and length only change before it starts.
fn hints(view: View, started: bool) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    let hints = keymap::hints(view).filter(|(action, _)| !(started && action.is_next_test()));
    for (i, (_, hint)) in hints.enumerate() {
        if i > 0 {
            spans.push(" · ".dark_gray());
        }
        spans.push(Span::raw(hint.keys));
        spans.push(format!(" {}", hint.label).dark_gray());
    }
    spans.push(Span::raw(" "));
    Line::from(spans)
}

/// A `width` by `height` area centered in `area`, shrunk to fit.
fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let [area] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(area);
    let [area] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(area);
    area
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(line: &Line) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn hints_come_from_the_bindings() {
        assert_eq!(
            text(&hints(View::History, false)),
            " esc back · tab new test · ? keys · q quit "
        );
    }

    #[test]
    fn typing_hints_drop_next_test_keys_once_started() {
        assert_eq!(
            text(&hints(View::Typing, true)),
            " tab new test · esc menu "
        );
    }

    #[test]
    fn typing_hints_show_next_test_keys_before_start() {
        assert!(text(&hints(View::Typing, false)).contains("←→ length · ↑↓ mode"));
    }

    #[test]
    fn tabs_highlight_the_current_screen() {
        let line = tabs(View::Settings);
        let settings = line
            .spans
            .iter()
            .find(|span| span.content == "settings")
            .expect("settings tab");
        assert_eq!(settings.style, Style::new().yellow().bold());
    }

    #[test]
    fn results_belong_to_the_test_tab() {
        let line = tabs(View::Results);
        let test = line
            .spans
            .iter()
            .find(|span| span.content == "test")
            .expect("test tab");
        assert_eq!(test.style, Style::new().yellow().bold());
    }
}
