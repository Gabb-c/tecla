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

/// The settings screen: one row per [`Setting`], the highlighted one editable.
pub struct SettingsView<'a> {
    pub config: &'a Config,
    pub selected: Setting,
    /// Where changes are saved, if they are.
    pub path: Option<&'a Path>,
    pub notice: Option<&'a str>,
}

impl Widget for SettingsView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut lines: Vec<Line> = Setting::ALL
            .into_iter()
            .map(|setting| self.row(setting, setting == self.selected))
            .collect();
        lines.push(Line::default());
        if let Some(notice) = self.notice {
            lines.push(Line::from(notice).red());
        }
        if let Some(path) = self.path {
            lines.push(Line::from(format!("saved to {}", path.display())).dark_gray());
        }
        lines.push(Line::from("↑↓ select · ←→ change · esc back · q quit").dark_gray());

        let text = Text::from(lines).centered();
        let height = u16::try_from(text.height()).unwrap_or(u16::MAX);
        let [area] = Layout::vertical([Constraint::Length(height)])
            .flex(Flex::Center)
            .areas(area);
        text.render(area, buf);
    }
}

impl SettingsView<'_> {
    fn row(&self, setting: Setting, selected: bool) -> Line<'static> {
        let value = format!(" {:<10}", setting.value(self.config));
        let value_style = match (selected, setting.is_active(self.config)) {
            (true, _) => Style::new().black().on_yellow().bold(),
            (false, true) => Style::new().yellow(),
            (false, false) => Style::new().dark_gray(),
        };
        let label = format!("{:>14} ", setting.label());
        let label_style = if selected {
            Style::new().bold()
        } else {
            Style::new().dark_gray()
        };
        Line::from(vec![
            Span::styled(label, label_style),
            Span::styled(value, value_style),
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
            "mode  time",
            "time  30s",
            "words  25",
            "caret  block",
            "key tips  on",
            "saved to /cfg/tecla/config.toml",
            "settings not saved",
        ] {
            assert!(screen.contains(expected), "{expected:?} in\n{screen}");
        }
    }
}
