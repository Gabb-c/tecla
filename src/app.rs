use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ratatui::DefaultTerminal;
use ratatui::crossterm::cursor::SetCursorStyle;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::crossterm::execute;

use crate::config::{Caret, Config, ConfigFile, Mode};
use crate::engine::{Input, Limit, Session};
use crate::event::{AppEvent, Event, EventHandler};
use crate::history::{History, Record};
use crate::settings::{Setting, Step};
use crate::stats::Results;
use crate::{ui, words};

/// Words generated up front in time mode.
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
    Settings { selected: Setting },
    Results(Results),
}

#[derive(Debug)]
pub struct App {
    running: bool,
    pub(crate) screen: Screen,
    pub(crate) session: Session,
    pub(crate) config: Config,
    /// Problem shown on the results screen, e.g. history could not be saved.
    pub(crate) notice: Option<String>,
    /// Problem loading or saving the config, shown until it is resolved.
    pub(crate) config_notice: Option<String>,
    /// Where settings are saved; `None` when they cannot be, e.g. the file failed to load.
    pub(crate) config_file: Option<ConfigFile>,
    history: Option<History>,
    events: EventHandler,
}

impl App {
    #[expect(
        clippy::new_without_default,
        reason = "reads the config and spawns the event thread"
    )]
    pub fn new() -> Self {
        let (config, config_file, config_notice) = load_config();
        Self {
            running: true,
            screen: Screen::Typing,
            session: new_session(&config),
            config,
            notice: None,
            config_notice,
            config_file,
            history: History::in_data_dir(),
            events: EventHandler::new(),
        }
    }

    /// Draws and handles events until the user quits.
    ///
    /// # Errors
    ///
    /// Returns an error if drawing or setting the cursor style fails, or the terminal stops
    /// delivering events.
    pub fn run(mut self, terminal: &mut DefaultTerminal) -> color_eyre::Result<()> {
        let mut redraw = true;
        // style last sent to the terminal
        let mut cursor = None;
        while self.running {
            if let Some(style) = cursor_style(self.config.caret, self.config.caret_blink)
                && cursor != Some(style)
            {
                execute!(terminal.backend_mut(), style)?;
                cursor = Some(style);
            }
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
            (Screen::Typing | Screen::Results(_), KeyCode::Esc) => {
                self.screen = Screen::Settings {
                    selected: Setting::Mode,
                };
            }
            (Screen::Typing, KeyCode::Tab)
            | (Screen::Results(_), KeyCode::Tab | KeyCode::Enter)
            | (Screen::Settings { .. }, KeyCode::Esc) => self.restart(),
            (Screen::Results(_) | Screen::Settings { .. }, KeyCode::Char('q')) => {
                self.events.send(AppEvent::Quit);
            }
            (Screen::Typing, _) => {
                if let Some(input) = typing_input(key) {
                    self.input(input);
                }
            }
            (&Screen::Settings { selected }, code) => self.settings_key(selected, code),
            (Screen::Results(_), _) => {}
        }
    }

    fn settings_key(&mut self, selected: Setting, code: KeyCode) {
        match code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.screen = Screen::Settings {
                    selected: selected.step(Step::Prev),
                };
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.screen = Screen::Settings {
                    selected: selected.step(Step::Next),
                };
            }
            KeyCode::Left | KeyCode::Char('h') => self.change(selected, Step::Prev),
            KeyCode::Right | KeyCode::Char('l' | ' ') | KeyCode::Enter => {
                self.change(selected, Step::Next);
            }
            _ => {}
        }
    }

    fn change(&mut self, setting: Setting, direction: Step) {
        setting.cycle(&mut self.config, direction);
        if let Some(file) = &self.config_file {
            self.config_notice = file
                .save(&self.config)
                .err()
                .map(|e| format!("settings not saved to {}: {e}", file.path().display()));
        }
    }

    fn input(&mut self, input: Input) {
        self.session.handle(input, Instant::now());
        if matches!(self.session.limit(), Limit::Time(_))
            && self.session.remaining_words() < EXTEND_THRESHOLD
        {
            self.session
                .extend(words::random_words(&mut rand::rng(), EXTRA_WORDS));
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
        let (mode, words) = match self.session.limit() {
            Limit::Time(_) => (Mode::Time, None),
            Limit::Words => (Mode::Words, Some(self.session.words().len())),
        };
        let record = Record {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            mode,
            duration_secs: results.duration.as_secs(),
            words,
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
        self.session = new_session(&self.config);
        self.notice = None;
        self.screen = Screen::Typing;
    }
}

/// The config file's settings, the file to save changes to, and any problem to report.
fn load_config() -> (Config, Option<ConfigFile>, Option<String>) {
    let Some(file) = ConfigFile::in_config_dir() else {
        let notice = "settings won't be saved: no config directory found";
        return (Config::default(), None, Some(notice.to_owned()));
    };
    match file.load() {
        Ok(config) => (config, Some(file), None),
        // saving would overwrite the user's file, so it is left alone until they fix it
        Err(e) => (
            Config::default(),
            None,
            Some(format!("{e}; settings won't be saved")),
        ),
    }
}

fn new_session(config: &Config) -> Session {
    let (count, limit) = match config.mode {
        Mode::Time => (INITIAL_WORDS, Limit::Time(Duration::from_secs(config.time))),
        Mode::Words => (config.words, Limit::Words),
    };
    Session::new(words::random_words(&mut rand::rng(), count), limit).with_rules(config.rules())
}

fn cursor_style(caret: Caret, blink: bool) -> Option<SetCursorStyle> {
    let style = match (caret, blink) {
        (Caret::Off, _) => return None,
        (Caret::Block, false) => SetCursorStyle::SteadyBlock,
        (Caret::Block, true) => SetCursorStyle::BlinkingBlock,
        (Caret::Bar, false) => SetCursorStyle::SteadyBar,
        (Caret::Bar, true) => SetCursorStyle::BlinkingBar,
        (Caret::Underline, false) => SetCursorStyle::SteadyUnderScore,
        (Caret::Underline, true) => SetCursorStyle::BlinkingUnderScore,
    };
    Some(style)
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

    /// An app that touches neither the user's config nor their history.
    fn app(config: Config) -> App {
        App {
            running: true,
            screen: Screen::Typing,
            session: new_session(&config),
            config,
            notice: None,
            config_notice: None,
            config_file: None,
            history: None,
            events: EventHandler::detached(),
        }
    }

    fn press(app: &mut App, code: KeyCode) {
        app.handle_key_event(key(code, KeyModifiers::NONE));
    }

    #[test]
    fn esc_opens_settings_and_returns_with_new_mode() {
        let mut app = app(Config::default());
        press(&mut app, KeyCode::Esc);
        assert!(matches!(
            app.screen,
            Screen::Settings {
                selected: Setting::Mode
            }
        ));

        press(&mut app, KeyCode::Right);
        assert_eq!(app.config.mode, Mode::Words);
        assert_eq!(app.session.limit(), Limit::Time(Duration::from_secs(30)));

        press(&mut app, KeyCode::Esc);
        assert!(matches!(app.screen, Screen::Typing));
        assert_eq!(app.session.limit(), Limit::Words);
        assert_eq!(app.session.words().len(), app.config.words);
    }

    #[test]
    fn settings_selection_wraps() {
        let mut app = app(Config::default());
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Up);
        assert!(matches!(
            app.screen,
            Screen::Settings {
                selected: Setting::KeyTips
            }
        ));
        press(&mut app, KeyCode::Char('j'));
        assert!(matches!(
            app.screen,
            Screen::Settings {
                selected: Setting::Mode
            }
        ));
    }

    #[test]
    fn words_mode_finishes_after_last_word() {
        let mut app = app(Config {
            mode: Mode::Words,
            words: 3,
            ..Config::default()
        });
        let target: Vec<String> = app.session.words().iter().map(|w| w.concat()).collect();
        for c in target.join(" ").chars() {
            press(&mut app, KeyCode::Char(c));
        }

        assert!(matches!(app.screen, Screen::Results(_)));
        assert_eq!(app.session.words().len(), 3);
        assert_eq!(
            app.notice.as_deref(),
            Some("history not saved: no data directory found")
        );
    }

    #[test]
    fn hidden_caret_sets_no_cursor_style() {
        assert_eq!(cursor_style(Caret::Off, true), None);
    }

    #[test]
    fn blinking_bar_sets_blinking_cursor_style() {
        assert_eq!(
            cursor_style(Caret::Bar, true),
            Some(SetCursorStyle::BlinkingBar)
        );
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
        let mut session = new_session(&Config::default());
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
