use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Style, Stylize},
    text::Line,
    widgets::{Block, BorderType, Clear, Padding, Paragraph, Widget},
};

use super::centered;
use crate::keymap::{self, View};

const KEYS_WIDTH: usize = 6;

/// A box over the screen listing the keys of a view; the menu and the key help.
pub struct KeysPopup {
    pub title: &'static str,
    pub view: View,
}

impl Widget for KeysPopup {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let lines: Vec<Line> = keymap::hints(self.view)
            .map(|(_, hint)| {
                Line::from(vec![
                    format!("{:<KEYS_WIDTH$}", hint.keys).yellow().bold(),
                    hint.label.into(),
                ])
            })
            .collect();
        let width = lines.iter().map(Line::width).max().unwrap_or_default();
        // two columns of padding and the border on each side
        let width = u16::try_from(width + 6).unwrap_or(u16::MAX);
        let height = u16::try_from(lines.len() + 2).unwrap_or(u16::MAX);
        let area = centered(area, width, height);
        Clear.render(area, buf);
        Paragraph::new(lines)
            .block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .border_style(Style::new().dark_gray())
                    .title(self.title.bold())
                    .padding(Padding::horizontal(2)),
            )
            .render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::render;

    #[test]
    fn lists_the_views_keys() {
        let screen = render(
            KeysPopup {
                title: " keys ",
                view: View::Settings,
            },
            50,
            14,
        );

        for expected in ["keys", "↑↓    select", "←→    change", "q     quit"] {
            assert!(screen.contains(expected), "{expected:?} in\n{screen}");
        }
    }

    #[test]
    fn menu_lists_where_to_go() {
        let screen = render(
            KeysPopup {
                title: " menu ",
                view: View::Menu,
            },
            50,
            14,
        );

        for expected in ["s     settings", "h     history", "esc   back"] {
            assert!(screen.contains(expected), "{expected:?} in\n{screen}");
        }
    }
}
