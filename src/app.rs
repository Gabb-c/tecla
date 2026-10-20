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
/// Words left after the current one before more are appended.
const EXTEND_THRESHOLD: usize = 20;

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
        let mut redraw = true;
        while self.running {
            if redraw {
                terminal.draw(|frame| ui::draw(&self, frame))?;
            }
            redraw = self.handle_events()?;
        }
        Ok(())
    }

    /// Handles the next event and returns whether the screen needs redrawing.
    fn handle_events(&mut self) -> color_eyre::Result<bool> {
        let redraw = match self.events.next()? {
            Event::Tick => {
                // checked before ticking so the tick that ends the test still redraws
                let redraw = tick_needs_redraw(&self.screen, &self.session);
                self.tick();
                redraw
            }
            Event::Crossterm(ratatui::crossterm::event::Event::Key(key))
                if key.kind == KeyEventKind::Press =>
            {
                self.handle_key_event(key);
                true
            }
            Event::Crossterm(_) => true,
            Event::App(AppEvent::Quit) => {
                self.running = false;
                false
            }
            Event::Error(error) => return Err(error),
        };
        Ok(redraw)
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
            (Screen::Typing, _) => {
                if let Some(input) = typing_input(key) {
                    self.input(input);
                }
            }
            (Screen::Results(_), _) => {}
        }
    }

    fn input(&mut self, input: Input) {
        self.session.handle(input, Instant::now());
        if self.session.remaining_words() < EXTEND_THRESHOLD {
            let more = words::random_words(&mut rand::rng(), EXTRA_WORDS).join(" ");
            self.session.extend(&more);
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

/// Only a running timer changes the screen between key presses.
fn tick_needs_redraw(screen: &Screen, session: &Session) -> bool {
    matches!(screen, Screen::Typing) && session.is_started()
}

fn typing_input(key: KeyEvent) -> Option<Input> {
    let ctrl_or_alt = key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
    match key.code {
        KeyCode::Backspace if ctrl_or_alt => Some(Input::DeleteWord),
        KeyCode::Backspace => Some(Input::Backspace),
        // many terminals send ctrl+backspace as ctrl+h
        KeyCode::Char('h' | 'w') if key.modifiers == KeyModifiers::CONTROL => {
            Some(Input::DeleteWord)
        }
        KeyCode::Char(c) if is_text(key.modifiers) => Some(Input::Char(c)),
        _ => None,
    }
}

fn is_text(modifiers: KeyModifiers) -> bool {
    let ctrl_alt = KeyModifiers::CONTROL | KeyModifiers::ALT;
    // windows reports AltGr as Ctrl+Alt
    !modifiers.intersects(ctrl_alt) || modifiers.contains(ctrl_alt)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn delete_word_shortcuts() {
        for event in [
            key(KeyCode::Backspace, KeyModifiers::CONTROL),
            key(KeyCode::Backspace, KeyModifiers::ALT),
            key(KeyCode::Char('h'), KeyModifiers::CONTROL),
            key(KeyCode::Char('w'), KeyModifiers::CONTROL),
        ] {
            assert_eq!(typing_input(event), Some(Input::DeleteWord), "{event:?}");
        }
    }

    #[test]
    fn plain_keys_map_to_text_and_backspace() {
        assert_eq!(
            typing_input(key(KeyCode::Backspace, KeyModifiers::NONE)),
            Some(Input::Backspace)
        );
        assert_eq!(
            typing_input(key(KeyCode::Char('w'), KeyModifiers::NONE)),
            Some(Input::Char('w'))
        );
        assert_eq!(
            typing_input(key(KeyCode::Char('x'), KeyModifiers::CONTROL)),
            None
        );
    }

    #[test]
    fn ticks_redraw_only_while_timer_runs() {
        let mut session = new_session();
        assert!(!tick_needs_redraw(&Screen::Typing, &session));

        session.handle(Input::Char('a'), Instant::now());
        assert!(tick_needs_redraw(&Screen::Typing, &session));

        let results = Screen::Results(Results::from_session(&session));
        assert!(!tick_needs_redraw(&results, &session));
    }

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
