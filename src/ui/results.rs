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
            stat("extra", r.extra_chars),
            stat("missed", r.missed_chars),
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ratatui::{Terminal, backend::TestBackend};

    use super::*;

    fn results() -> Results {
        Results {
            net_wpm: 61.6,
            raw_wpm: 70.2,
            accuracy: 95.3,
            correct_chars: 150,
            incorrect_chars: 4,
            extra_chars: 2,
            missed_chars: 1,
            duration: Duration::from_secs(30),
        }
    }

    fn rendered(view: ResultsView<'_>) -> String {
        let mut terminal = Terminal::new(TestBackend::new(40, 15)).expect("terminal");
        terminal
            .draw(|frame| frame.render_widget(view, frame.area()))
            .expect("draw");
        let buffer = terminal.backend().buffer();
        (0..15)
            .map(|y| (0..40).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn shows_every_stat() {
        let results = results();
        let screen = rendered(ResultsView {
            results: &results,
            notice: None,
        });

        for expected in [
            "wpm 62",
            "raw 70",
            "acc 95.3%",
            "correct 150",
            "incorrect 4",
            "extra 2",
            "missed 1",
            "time 30s",
        ] {
            assert!(screen.contains(expected), "{expected:?} in\n{screen}");
        }
    }

    #[test]
    fn shows_notice() {
        let results = results();
        let screen = rendered(ResultsView {
            results: &results,
            notice: Some("history not saved"),
        });

        assert!(screen.contains("history not saved"), "{screen}");
    }
}
