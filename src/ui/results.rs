use std::fmt::Display;

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Flex, Layout, Rect},
    style::Stylize,
    text::{Line, Text},
    widgets::Widget,
};

use crate::stats::Results;

/// The results screen shown after a test ends.
pub struct ResultsView<'a> {
    pub results: &'a Results,
    /// Problem worth telling the user about, e.g. history could not be saved.
    pub notice: Option<&'a str>,
}

impl Widget for ResultsView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let r = self.results;
        let mut lines = vec![
            stat("wpm", format!("{:.0}", r.net_wpm)),
            stat("raw", format!("{:.0}", r.raw_wpm)),
            stat("acc", format!("{:.1}%", r.accuracy)),
            stat("correct", r.correct_chars),
            stat("incorrect", r.incorrect_chars),
            stat("time", format!("{}s", r.duration.as_secs())),
            Line::default(),
        ];
        if let Some(notice) = self.notice {
            lines.push(Line::from(notice).red());
        }
        lines.push(Line::from("tab/enter restart · esc quit").dark_gray());

        let text = Text::from(lines).centered();
        let height = u16::try_from(text.height()).unwrap_or(u16::MAX);
        let [area] = Layout::vertical([Constraint::Length(height)])
            .flex(Flex::Center)
            .areas(area);
        text.render(area, buf);
    }
}

fn stat(label: &str, value: impl Display) -> Line<'static> {
    Line::from(vec![
        format!("{label:>9} ").dark_gray(),
        format!("{value:<9}").yellow().bold(),
    ])
}
