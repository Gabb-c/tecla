use std::path::Path;

use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Span, Text},
    widgets::Widget,
};

use crate::config::Config;
use crate::settings::Setting;

/// The settings screen: one row per [`Setting`] grouped by section, the highlighted one
/// editable and described.
pub struct SettingsView<'a> {
    pub config: &'a Config,
    pub selected: Setting,
    /// Where changes are saved, if they are.
    pub path: Option<&'a Path>,
    pub notice: Option<&'a str>,
}

impl Widget for SettingsView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut lines = Vec::new();
        let mut section = None;
        for setting in Setting::ALL {
            let first = section != Some(setting.section());
            section = Some(setting.section());
            lines.push(self.row(setting, first));
        }
        lines.push(Line::default());
        lines.push(Line::from(self.selected.description(self.config)).italic());
        lines.push(Line::default());
        if let Some(notice) = self.notice {
            lines.push(Line::from(notice).red());
        }
        if let Some(path) = self.path {
            lines.push(Line::from(format!("saved to {}", path.display())).dark_gray());
        }

        let text = Text::from(lines).centered();
        let height = u16::try_from(text.height()).unwrap_or(u16::MAX);
        let [area] = Layout::vertical([Constraint::Length(height)])
            .flex(Flex::Center)
            .areas(area);
        text.render(area, buf);
    }
}

impl SettingsView<'_> {
    /// A setting's row, led by its section's name when it is the section's first.
    fn row(&self, setting: Setting, first_in_section: bool) -> Line<'static> {
        let selected = setting == self.selected;
        let section = if first_in_section {
            setting.section().label()
        } else {
            ""
        };
        let value = setting.value(self.config);
        let value = if selected {
            format!("‹ {value} ›")
        } else {
            format!("  {value}  ")
        };
        let value_style = match (selected, setting.is_active(self.config)) {
            (true, _) => Style::new().yellow().bold(),
            (false, true) => Style::new().yellow(),
            (false, false) => Style::new().dark_gray(),
        };
        let label_style = if selected {
            Style::new().bold()
        } else {
            Style::new().dark_gray()
        };
        Line::from(vec![
            Span::styled(
                format!("{section:>10}  "),
                Style::new().dark_gray().italic(),
            ),
            Span::styled(format!("{:>14} ", setting.label()), label_style),
            Span::styled(format!("{value:<12}"), value_style),
        ])
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;

    fn rendered(view: SettingsView<'_>) -> String {
        let mut terminal = Terminal::new(TestBackend::new(60, 24)).expect("terminal");
        terminal
            .draw(|frame| frame.render_widget(view, frame.area()))
            .expect("draw");
        let buffer = terminal.backend().buffer();
        (0..24)
            .map(|y| (0..60).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn shows_every_setting_with_its_value() {
        let config = Config::default();
        let screen = rendered(SettingsView {
            config: &config,
            selected: Setting::Mode,
            path: Some(Path::new("/cfg/tecla/config.toml")),
            notice: Some("settings not saved"),
        });

        for expected in [
            "mode ‹ time ›",
            "time   30s",
            "words   25",
            "caret   block",
            "key tips   on",
            "saved to /cfg/tecla/config.toml",
            "settings not saved",
        ] {
            assert!(screen.contains(expected), "{expected:?} in\n{screen}");
        }
    }

    #[test]
    fn labels_each_section_once() {
        let config = Config::default();
        let screen = rendered(SettingsView {
            config: &config,
            selected: Setting::Mode,
            path: None,
            notice: None,
        });

        for expected in ["test", "caret", "live stats", "difficulty", "interface"] {
            let rows = screen
                .lines()
                .filter(|line| line.trim_start().starts_with(&format!("{expected}  ")))
                .count();
            assert_eq!(rows, 1, "{expected:?} in\n{screen}");
        }
    }

    #[test]
    fn describes_the_selected_setting() {
        let config = Config::default();
        let screen = rendered(SettingsView {
            config: &config,
            selected: Setting::Confidence,
            path: None,
            notice: None,
        });

        assert!(
            screen.contains("backspace may return to a previous word"),
            "{screen}"
        );
    }
}
