use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Flex, Layout, Rect},
    style::Stylize,
    text::{Line, Text},
    widgets::Widget,
};

use crate::history::{RECENT, Summary, Test};

/// The history screen: one row of saved results per [`Test`].
pub struct HistoryView<'a> {
    pub summaries: &'a [Summary],
    /// The test just run, highlighted.
    pub current: Test,
    /// Problem reading the history.
    pub notice: Option<&'a str>,
}

impl Widget for HistoryView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut lines = Vec::with_capacity(self.summaries.len() + 4);
        if self.summaries.is_empty() {
            lines.push(Line::from("no tests yet").dark_gray());
        } else {
            let avg = format!("avg{RECENT}");
            lines.push(row("test", "tests", "best", &avg, "last").dark_gray());
            lines.extend(self.summaries.iter().map(|s| {
                let line = row(
                    &s.test.to_string(),
                    &s.count.to_string(),
                    &format!("{:.0}", s.best),
                    &format!("{:.0}", s.recent_avg),
                    &format!("{:.0}", s.last),
                );
                if s.test == self.current {
                    line.yellow().bold()
                } else {
                    line
                }
            }));
        }
        if let Some(notice) = self.notice {
            lines.push(Line::default());
            lines.push(Line::from(notice).red());
        }

        let text = Text::from(lines).centered();
        let height = u16::try_from(text.height()).unwrap_or(u16::MAX);
        let [area] = Layout::vertical([Constraint::Length(height)])
            .flex(Flex::Center)
            .areas(area);
        text.render(area, buf);
    }
}

fn row(test: &str, count: &str, best: &str, avg: &str, last: &str) -> Line<'static> {
    Line::from(format!("{test:<10}{count:>6}{best:>6}{avg:>7}{last:>6}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::render;

    fn rendered(view: HistoryView<'_>) -> String {
        render(view, 50, 12)
    }

    fn summary(test: Test, best: f64) -> Summary {
        Summary {
            test,
            count: 12,
            best,
            recent_avg: best - 10.4,
            last: best - 5.0,
        }
    }

    #[test]
    fn shows_row_per_test() {
        let summaries = [
            summary(Test::Time(30), 87.0),
            summary(Test::Words(25), 92.0),
        ];
        let screen = rendered(HistoryView {
            summaries: &summaries,
            current: Test::Time(30),
            notice: None,
        });

        for expected in [
            "test       tests  best  avg10  last",
            "time 30       12    87     77    82",
            "words 25      12    92     82    87",
        ] {
            assert!(screen.contains(expected), "{expected:?} in\n{screen}");
        }
    }

    #[test]
    fn shows_empty_state() {
        let screen = rendered(HistoryView {
            summaries: &[],
            current: Test::Time(30),
            notice: None,
        });

        assert!(screen.contains("no tests yet"), "{screen}");
    }

    #[test]
    fn shows_notice() {
        let screen = rendered(HistoryView {
            summaries: &[],
            current: Test::Time(30),
            notice: Some("2 unreadable lines skipped"),
        });

        assert!(screen.contains("2 unreadable lines skipped"), "{screen}");
    }
}
