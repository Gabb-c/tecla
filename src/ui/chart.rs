use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Style, Stylize},
    symbols::Marker,
    text::Line,
    widgets::{Axis, Chart, Dataset, GraphType, Widget},
};

use crate::stats::{Burst, Timeline};

/// Net WPM so far, raw WPM of each interval, and the intervals with errors.
pub struct WpmChart<'a> {
    pub timeline: &'a Timeline,
}

impl WpmChart<'_> {
    /// Whether there is enough to draw: a line needs two points.
    pub fn is_drawable(&self) -> bool {
        self.timeline.samples().len() >= 2
    }
}

impl Widget for WpmChart<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let samples = self.timeline.samples();
        let [_, .., last] = samples else {
            return;
        };
        let wpm: Vec<(f64, f64)> = samples
            .iter()
            .map(|s| (s.at.as_secs_f64(), s.wpm))
            .collect();
        let bursts: Vec<Burst> = self.timeline.bursts().collect();
        let raw: Vec<(f64, f64)> = bursts
            .iter()
            .map(|b| (b.end.as_secs_f64(), b.raw_wpm))
            .collect();
        let errors: Vec<(f64, f64)> = bursts
            .iter()
            .filter(|b| b.errors > 0)
            .map(|b| (b.end.as_secs_f64(), b.raw_wpm))
            .collect();

        let end = last.at.as_secs_f64();
        let top = y_max(wpm.iter().chain(&raw).map(|&(_, y)| y));
        let datasets = vec![
            line(&raw).dark_gray(),
            line(&wpm).yellow(),
            Dataset::default()
                .marker(Marker::Dot)
                .graph_type(GraphType::Scatter)
                .red()
                .data(&errors),
        ];
        // a boxed legend would cover the end of the lines, so it goes on its own row
        let [chart_area, legend_area] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(area);
        legend().render(legend_area, buf);
        Chart::new(datasets)
            .legend_position(None)
            .x_axis(
                Axis::default()
                    .bounds([0.0, end])
                    .labels(["0s".to_owned(), format!("{end:.0}s")])
                    .style(Style::new().dark_gray()),
            )
            .y_axis(
                Axis::default()
                    .bounds([0.0, top])
                    .labels([
                        "0".to_owned(),
                        format!("{:.0}", top / 2.0),
                        format!("{top:.0}"),
                    ])
                    .style(Style::new().dark_gray()),
            )
            .render(chart_area, buf);
    }
}

fn legend() -> Line<'static> {
    Line::from(vec![
        "── wpm".yellow(),
        "   ".into(),
        "── raw".dark_gray(),
        "   ".into(),
        "• errors".red(),
    ])
    .centered()
}

fn line(data: &[(f64, f64)]) -> Dataset<'_> {
    Dataset::default()
        .marker(Marker::Braille)
        .graph_type(GraphType::Line)
        .data(data)
}

/// The highest value rounded up to a multiple of 10, so axis labels stay round.
fn y_max(values: impl IntoIterator<Item = f64>) -> f64 {
    let max = values.into_iter().fold(0.0, f64::max);
    ((max / 10.0).ceil() * 10.0).max(10.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{render, steady_timeline};

    fn rendered(timeline: &Timeline) -> String {
        render(WpmChart { timeline }, 40, 10)
    }

    #[test]
    fn shows_axis_labels_and_legend() {
        let screen = rendered(&steady_timeline(4));
        for expected in ["0s", "4s", "── wpm   ── raw   • errors"] {
            assert!(screen.contains(expected), "{expected:?} in\n{screen}");
        }
    }

    #[test]
    fn draws_nothing_under_two_samples() {
        let screen = rendered(&steady_timeline(1));
        assert!(screen.trim().is_empty(), "{screen}");
    }

    #[test]
    fn y_axis_rounds_up_to_ten() {
        assert_eq!(y_max([0.0, 41.2, 7.0]), 50.0);
    }

    #[test]
    fn y_axis_is_never_flat() {
        assert_eq!(y_max(std::iter::empty()), 10.0);
    }
}
