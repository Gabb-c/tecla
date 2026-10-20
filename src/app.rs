use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::engine::{Input, Session};
use crate::event::{AppEvent, Event, EventHandler};
use crate::history::{History, Mode, Record};
use crate::stats::Results;
use crate::{ui, words};

const TEST_DURATION: Duration = Duration::from_secs(30);
const INITIAL_WORDS: usize = 100;
const EXTRA_WORDS: usize = 50;
/// Untyped graphemes left before more words are appended.
const EXTEND_THRESHOLD: usize = 100;

#[derive(Debug, thiserror::Error)]
enum SaveError {
    #[error("history not saved: no data directory found")]
    NoDataDir,
    #[error("history not saved to {}: {source}", .path.display())]
    Write { path: PathBuf, source: io::Error },
}

#[derive(Debug)]
pub enum Screen {
    Typing,
    Results(Results),
}

#[derive(Debug)]
pub struct App {
    running: bool,
    pub(crate) screen: Screen,
    pub(crate) session: Session,
    /// Problem shown on the results screen, e.g. history could not be saved.
    pub(crate) notice: Option<String>,
    history: Option<History>,
    events: EventHandler,
}

impl App {
    pub fn new() -> Self {
        Self {
            running: true,
            screen: Screen::Typing,
            session: new_session(),
            notice: None,
            history: History::in_data_dir(),
            events: EventHandler::new(),
        }
    }

    /// Draws and handles events until the user quits.
    ///
    /// # Errors
    ///
    /// Returns an error if drawing fails or the terminal stops delivering events.
    pub fn run(mut self, terminal: &mut DefaultTerminal) -> color_eyre::Result<()> {
        while self.running {
            terminal.draw(|frame| ui::draw(&self, frame))?;
            self.handle_events()?;
        }
        Ok(())
    }

    fn handle_events(&mut self) -> color_eyre::Result<()> {
        match self.events.next()? {
            Event::Tick => self.tick(),
            Event::Crossterm(ratatui::crossterm::event::Event::Key(key))
                if key.kind == KeyEventKind::Press =>
            {
                self.handle_key_event(key);
            }
            Event::Crossterm(_) => {}
            Event::App(AppEvent::Quit) => self.running = false,
            Event::Error(error) => return Err(error),
        }
        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.events.send(AppEvent::Quit);
            return;
        }
        match (&self.screen, key.code) {
            (_, KeyCode::Esc) => self.events.send(AppEvent::Quit),
            (Screen::Typing, KeyCode::Tab)
            | (Screen::Results(_), KeyCode::Tab | KeyCode::Enter) => self.restart(),
            (Screen::Typing, KeyCode::Backspace) => self.input(Input::Backspace),
            (Screen::Typing, KeyCode::Char(c)) if is_text(key.modifiers) => {
                self.input(Input::Char(c));
            }
            _ => {}
        }
    }

    fn input(&mut self, input: Input) {
        self.session.handle(input, Instant::now());
        if self.session.target().len() - self.session.cursor() < EXTEND_THRESHOLD {
            let more = words::random_words(&mut rand::rng(), EXTRA_WORDS).join(" ");
            self.session.extend(&format!(" {more}"));
        }
        self.finish_if_done();
    }

    fn tick(&mut self) {
        self.session.tick(Instant::now());
        self.finish_if_done();
    }

    fn finish_if_done(&mut self) {
        if matches!(self.screen, Screen::Typing) && self.session.is_finished() {
            let results = Results::from_session(&self.session);
            self.notice = self.save(&results).err().map(|e| e.to_string());
            self.screen = Screen::Results(results);
        }
    }

    fn save(&self, results: &Results) -> Result<(), SaveError> {
        let history = self.history.as_ref().ok_or(SaveError::NoDataDir)?;
        let record = Record {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            mode: Mode::Time,
            duration_secs: results.duration.as_secs(),
            wpm: results.net_wpm,
            raw: results.raw_wpm,
            accuracy: results.accuracy,
        };
        history.append(&record).map_err(|source| SaveError::Write {
            path: history.path().to_owned(),
            source,
        })
    }

    fn restart(&mut self) {
        self.session = new_session();
        self.notice = None;
        self.screen = Screen::Typing;
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

fn new_session() -> Session {
    let text = words::random_words(&mut rand::rng(), INITIAL_WORDS).join(" ");
    Session::new(&text, TEST_DURATION)
}

fn is_text(modifiers: KeyModifiers) -> bool {
    let ctrl_alt = KeyModifiers::CONTROL | KeyModifiers::ALT;
    // windows reports AltGr as Ctrl+Alt
    !modifiers.intersects(ctrl_alt) || modifiers.contains(ctrl_alt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unmodified_and_shifted_keys_are_text() {
        assert!(is_text(KeyModifiers::NONE));
        assert!(is_text(KeyModifiers::SHIFT));
    }

    #[test]
    fn ctrl_or_alt_alone_is_not_text() {
        assert!(!is_text(KeyModifiers::CONTROL));
        assert!(!is_text(KeyModifiers::ALT));
        assert!(!is_text(KeyModifiers::CONTROL | KeyModifiers::SHIFT));
    }

    #[test]
    fn altgr_reported_as_ctrl_alt_is_text() {
        assert!(is_text(KeyModifiers::CONTROL | KeyModifiers::ALT));
        assert!(is_text(
            KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT
        ));
    }
}
