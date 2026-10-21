use ratatui::crossterm::{cursor::SetCursorStyle, execute};
use tecla::app::App;

fn main() -> eyre::Result<()> {
    ratatui::run(|terminal| {
        let result = App::new().run(terminal);
        execute!(terminal.backend_mut(), SetCursorStyle::DefaultUserShape)?;
        result
    })
}
