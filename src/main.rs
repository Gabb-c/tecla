use ratatui::crossterm::{cursor::SetCursorStyle, execute};
use tecla::app::App;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    ratatui::run(|terminal| {
        let result = App::new().run(terminal);
        execute!(terminal.backend_mut(), SetCursorStyle::DefaultUserShape)?;
        result
    })
}
