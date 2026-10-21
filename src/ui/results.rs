use std::fmt::Display;
use std::time::Duration;

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Flex, Layout, Rect},
    style::Stylize,
    text::{Line, Text},
    widgets::Widget,
};

use super::chart::WpmChart;
use crate::history::{Pb, Test};
use crate::stats::{Results, Timeline};

/// Fewest rows worth drawing the chart in; shorter terminals show the stats alone.
const CHART_MIN: u16 = 6;
const CHART_MAX: u16 = 14;
const CHART_WIDTH: u16 = 72;

/// The results screen shown after a test ends.
pub struct ResultsView<'a> {
    pub results: &'a Results,
    pub timeline: &'a Timeline,
    pub test: Test,
    /// `None` hides the personal best, e.g. when the result was not saved.
    pub pb: Option<Pb>,
    /// Problem worth telling the user about, e.g. history could not be saved.
    pub notice: Option<&'a str>,
}

impl Widget for ResultsView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let stats = self.stats();
        let footer = self.footer();
        let chart = WpmChart {
            timeline: self.timeline,
        };
        let stats_h = height(&stats);
        let footer_h = height(&footer);
        // one blank row above the chart and one above the footer
        let room = area.height.saturating_sub(stats_h + footer_h + 2);
        let chart_h = if chart.is_drawable() && room >= CHART_MIN {
            room.min(CHART_MAX)
        } else {
            0
        };
        let [stats_area, _, chart_area, _, footer_area] = Layout::vertical([
            Constraint::Length(stats_h),
            Constraint::Length(u16::from(chart_h > 0)),
            Constraint::Length(chart_h),
            Constraint::Length(1),
            Constraint::Length(footer_h),
        ])
        .flex(Flex::Center)
        .areas(area);
        let [chart_area] = Layout::horizontal([Constraint::Length(CHART_WIDTH)])
            .flex(Flex::Center)
            .areas(chart_area);

        stats.render(stats_area, buf);
        chart.render(chart_area, buf);
        footer.render(footer_area, buf);
    }
}

impl<'a> ResultsView<'a> {
    fn stats(&self) -> Text<'static> {
        let r = self.results;
        let mut lines = vec![
            stat("test", self.test),
            stat("wpm", format!("{:.0}", r.net_wpm)),
        ];
        match self.pb {
            Some(Pb::New) => lines.push(Line::from("new pb!").green().bold()),
            Some(Pb::Standing(best)) => lines.push(stat("pb", format!("{best:.0}"))),
            None => {}
        }
        let consistency = r
            .consistency
            .map_or_else(|| "-".to_owned(), |c| format!("{c:.0}%"));
        lines.extend([
            stat("raw", format!("{:.0}", r.raw_wpm)),
            stat("acc", format!("{:.1}%", r.accuracy)),
            stat("consistency", consistency),
            stat("correct", r.correct_chars),
            stat("incorrect", r.incorrect_chars),
            stat("extra", r.extra_chars),
            stat("missed", r.missed_chars),
            stat("time", seconds(r.duration)),
        ]);
        Text::from(lines).centered()
    }

    fn footer(&self) -> Text<'a> {
        self.notice
            .map_or_else(Text::default, |notice| Line::from(notice).red().into())
            .centered()
    }
}

fn height(text: &Text) -> u16 {
    u16::try_from(text.height()).unwrap_or(u16::MAX)
}

/// Whole seconds when exact, as time tests are, else one decimal.
fn seconds(duration: Duration) -> String {
    if duration.subsec_millis() == 0 {
        format!("{}s", duration.as_secs())
    } else {
        format!("{:.1}s", duration.as_secs_f64())
    }
}

fn stat(label: &str, value: impl Display) -> Line<'static> {
    Line::from(vec![
        format!("{label:>11} ").dark_gray(),
        format!("{value:<9}").yellow().bold(),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{render, steady_timeline};

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
            consistency: Some(78.4),
        }
    }

    fn rendered(view: ResultsView<'_>) -> String {
        render(view, 60, 20)
    }

    #[test]
    fn shows_every_stat() {
        let results = results();
        let screen = rendered(ResultsView {
            results: &results,
            timeline: &Timeline::default(),
            test: Test::Time(30),
            pb: None,
            notice: None,
        });

        for expected in [
            "test time 30",
            "wpm 62",
            "raw 70",
            "acc 95.3%",
            "correct 150",
            "incorrect 4",
            "extra 2",
            "missed 1",
            "time 30s",
            "consistency 78%",
        ] {
            assert!(screen.contains(expected), "{expected:?} in\n{screen}");
        }
    }

    #[test]
    fn shows_notice() {
        let results = results();
        let screen = rendered(ResultsView {
            results: &results,
            timeline: &Timeline::default(),
            test: Test::Time(30),
            pb: None,
            notice: Some("history not saved"),
        });

        assert!(screen.contains("history not saved"), "{screen}");
    }

    #[test]
    fn shows_new_pb() {
        let results = results();
        let screen = rendered(ResultsView {
            results: &results,
            timeline: &Timeline::default(),
            test: Test::Time(30),
            pb: Some(Pb::New),
            notice: None,
        });

        assert!(screen.contains("new pb!"), "{screen}");
    }

    #[test]
    fn shows_standing_pb() {
        let results = results();
        let screen = rendered(ResultsView {
            results: &results,
            timeline: &Timeline::default(),
            test: Test::Time(30),
            pb: Some(Pb::Standing(87.4)),
            notice: None,
        });

        assert!(screen.contains("pb 87"), "{screen}");
    }

    #[test]
    fn undefined_consistency_shows_dash() {
        let results = Results {
            consistency: None,
            ..results()
        };
        let screen = rendered(ResultsView {
            results: &results,
            timeline: &Timeline::default(),
            test: Test::Time(30),
            pb: None,
            notice: None,
        });

        assert!(screen.contains("consistency -"), "{screen}");
    }

    #[test]
    fn shows_chart_when_tall() {
        let results = results();
        let timeline = steady_timeline(30);
        let screen = render(
            ResultsView {
                results: &results,
                timeline: &timeline,
                test: Test::Time(30),
                pb: None,
                notice: None,
            },
            80,
            30,
        );

        assert!(screen.contains("30s"), "{screen}");
    }

    /// A full 30 second test's results, with a notice, in a terminal too short for the chart.
    fn rendered_short() -> String {
        let results = results();
        let timeline = steady_timeline(30);
        render(
            ResultsView {
                results: &results,
                timeline: &timeline,
                test: Test::Time(30),
                pb: None,
                notice: Some("history not saved"),
            },
            80,
            18,
        )
    }

    #[test]
    fn hides_chart_when_short() {
        let screen = rendered_short();
        assert!(!screen.contains("errors"), "{screen}");
    }

    #[test]
    fn keeps_footer_when_short() {
        let screen = rendered_short();
        assert!(screen.contains("history not saved"), "{screen}");
    }

    #[test]
    fn fractional_time_shows_a_decimal() {
        assert_eq!(seconds(Duration::from_secs(30)), "30s");
        assert_eq!(seconds(Duration::from_millis(12_430)), "12.4s");
    }
}
