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
use crate::history::{self, History, Pb, Record, Summary, Test};
use crate::keymap::{self, Action, Target, View};
use crate::settings::{Lengths, Setting, Step};
use crate::stats::{Results, Timeline};
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

/// A finished test, as shown on the results screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Finished {
    pub results: Results,
    /// `None` when the result was not saved.
    pub pb: Option<Pb>,
}

/// The screen that settings and history go back to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Origin {
    Menu,
    Results(Finished),
}

impl Origin {
    fn screen(self) -> Screen {
        match self {
            Self::Menu => Screen::Menu,
            Self::Results(finished) => Screen::Results(finished),
        }
    }
}

#[derive(Debug)]
pub enum Screen {
    Typing,
    /// Drawn over the typing screen; leaving it starts a new test.
    Menu,
    Settings {
        selected: Setting,
        from: Origin,
    },
    Results(Finished),
    History {
        summaries: Vec<Summary>,
        from: Origin,
    },
}

impl Screen {
    pub fn view(&self) -> View {
        match self {
            Self::Typing => View::Typing,
            Self::Menu => View::Menu,
            Self::Settings { .. } => View::Settings,
            Self::Results(_) => View::Results,
            Self::History { .. } => View::History,
        }
    }
}

#[derive(Debug)]
pub struct App {
    running: bool,
    pub(crate) screen: Screen,
    pub(crate) session: Session,
    /// Samples of `session` for the results chart.
    pub(crate) timeline: Timeline,
    pub(crate) config: Config,
    /// Problem shown on the results screen, e.g. history could not be saved.
    pub(crate) notice: Option<String>,
    /// Problem loading or saving the config, shown until it is resolved.
    pub(crate) config_notice: Option<String>,
    /// Where settings are saved; `None` when they cannot be, e.g. the file failed to load.
    pub(crate) config_file: Option<ConfigFile>,
    history: Option<History>,
    /// Every saved result, oldest first; `None` when the history file exists but could not be
    /// read.
    pub(crate) records: Option<Vec<Record>>,
    /// Problem reading the history, shown on the history screen.
    pub(crate) history_notice: Option<String>,
    /// Whether the key help is drawn over the screen.
    pub(crate) help: bool,
    pub(crate) lengths: Lengths,
    events: EventHandler,
}

impl App {
    #[expect(
        clippy::new_without_default,
        reason = "reads the config and spawns the event thread"
    )]
    pub fn new() -> Self {
        let (config, config_file, config_notice) = load_config();
        let history = History::in_data_dir();
        let (records, history_notice) = load_history(history.as_ref());
        Self {
            running: true,
            screen: Screen::Typing,
            session: new_session(&config),
            timeline: Timeline::default(),
            lengths: Lengths::new(&config),
            config,
            notice: None,
            config_notice,
            config_file,
            history,
            records,
            history_notice,
            help: false,
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
        if self.help {
            self.help = false;
            return;
        }
        if let Some(action) = keymap::action(self.screen.view(), key) {
            self.apply(action);
        } else if matches!(self.screen, Screen::Typing)
            && let Some(input) = typing_input(key)
        {
            self.input(input);
        }
    }

    fn apply(&mut self, action: Action) {
        if action.is_next_test() && self.session.is_started() {
            return;
        }
        match action {
            Action::Restart => self.restart(),
            Action::Back => self.back(),
            Action::Quit => self.events.send(AppEvent::Quit),
            Action::Open(target) => self.open(target),
            Action::Help => self.help = true,
            Action::Select(direction) => {
                if let Screen::Settings { selected, .. } = &mut self.screen {
                    *selected = selected.step(direction);
                }
            }
            Action::Change(direction) => {
                if let Screen::Settings { selected, .. } = self.screen {
                    self.change(selected, direction);
                }
            }
            Action::CycleMode => self.change_next_test(Setting::Mode, Step::Next),
            Action::CycleLength(direction) => {
                let length = match self.config.mode {
                    Mode::Time => Setting::Time,
                    Mode::Words => Setting::Words,
                };
                self.change_next_test(length, direction);
            }
        }
    }

    fn back(&mut self) {
        match self.screen {
            Screen::Typing => self.screen = Screen::Menu,
            // the test behind the menu may have run on, and settings may have changed
            Screen::Menu | Screen::Results(_) => self.restart(),
            Screen::Settings { from, .. } | Screen::History { from, .. } => {
                self.screen = from.screen();
            }
        }
    }

    /// Opens `target` from the results, or else from the menu.
    fn open(&mut self, target: Target) {
        let from = match self.screen {
            Screen::Results(finished) => Origin::Results(finished),
            _ => Origin::Menu,
        };
        self.screen = match target {
            Target::Settings => Screen::Settings {
                selected: Setting::Mode,
                from,
            },
            Target::History => Screen::History {
                summaries: history::summarize(self.records.as_deref().unwrap_or_default()),
                from,
            },
        };
    }

    /// Changes `setting` from the typing screen.
    fn change_next_test(&mut self, setting: Setting, direction: Step) {
        self.change(setting, direction);
        self.restart();
    }

    fn change(&mut self, setting: Setting, direction: Step) {
        setting.cycle(&mut self.config, &self.lengths, direction);
        if let Some(file) = &self.config_file {
            self.config_notice = file
                .save(&self.config)
                .err()
                .map(|e| format!("settings not saved to {}: {e}", file.path().display()));
        }
    }

    fn input(&mut self, input: Input) {
        let now = Instant::now();
        // before the input, so a keystroke past a second boundary lands in the next second
        self.timeline.observe(&self.session, now);
        self.session.handle(input, now);
        if matches!(self.session.limit(), Limit::Time(_))
            && self.session.remaining_words() < EXTEND_THRESHOLD
        {
            self.session
                .extend(words::random_words(&mut rand::rng(), EXTRA_WORDS));
        }
        self.finish_if_done();
    }

    fn tick(&mut self) {
        let now = Instant::now();
        self.session.tick(now);
        self.timeline.observe(&self.session, now);
        self.finish_if_done();
    }

    fn finish_if_done(&mut self) {
        if matches!(self.screen, Screen::Typing) && self.session.is_finished() {
            self.timeline.finish(&self.session);
            let results = Results::from_session(&self.session, &self.timeline);
            let record = self.record(&results);
            let pb = match self.save(&record) {
                Ok(()) => self.add_record(record),
                Err(e) => {
                    self.notice = Some(e.to_string());
                    None
                }
            };
            self.screen = Screen::Results(Finished { results, pb });
        }
    }

    /// Adds a saved record and compares it with the previous best, unless the history could
    /// not be read and so the best is unknown.
    fn add_record(&mut self, record: Record) -> Option<Pb> {
        let records = self.records.as_mut()?;
        let pb = history::pb(records, &record);
        records.push(record);
        Some(pb)
    }

    fn record(&self, results: &Results) -> Record {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        Record::new(Test::of(&self.session), timestamp, results)
    }

    fn save(&self, record: &Record) -> Result<(), SaveError> {
        let history = self.history.as_ref().ok_or(SaveError::NoDataDir)?;
        history.append(record).map_err(|source| SaveError::Write {
            path: history.path().to_owned(),
            source,
        })
    }

    fn restart(&mut self) {
        self.session = new_session(&self.config);
        self.timeline = Timeline::default();
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

/// The saved results and any problem reading them.
fn load_history(history: Option<&History>) -> (Option<Vec<Record>>, Option<String>) {
    let Some(history) = history else {
        return (Some(Vec::new()), Some(SaveError::NoDataDir.to_string()));
    };
    match history.read() {
        Ok(loaded) => {
            let notice = loaded.first_error.map(|first| {
                format!(
                    "{} unreadable lines skipped in {} ({first})",
                    loaded.skipped,
                    history.path().display()
                )
            });
            (Some(loaded.records), notice)
        }
        Err(e) => {
            let notice = format!("history not loaded from {}: {e}", history.path().display());
            (None, Some(notice))
        }
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
    use crate::test_util::TempDir;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    /// An app that touches neither the user's config nor their history.
    fn app(config: Config) -> App {
        App {
            running: true,
            screen: Screen::Typing,
            session: new_session(&config),
            timeline: Timeline::default(),
            lengths: Lengths::new(&config),
            config,
            notice: None,
            config_notice: None,
            config_file: None,
            history: None,
            records: Some(Vec::new()),
            history_notice: None,
            help: false,
            events: EventHandler::detached(),
        }
    }

    /// An app saving to a fresh history in `dir`.
    fn app_with_history(config: Config, dir: &TempDir) -> App {
        let history = History::new(dir.0.join("history.jsonl"));
        App {
            history: Some(history),
            ..app(config)
        }
    }

    fn three_words() -> Config {
        Config {
            mode: Mode::Words,
            words: 3,
            ..Config::default()
        }
    }

    /// Types the session's words without mistakes, finishing a words test.
    fn type_all(app: &mut App) {
        let target: Vec<String> = app.session.words().iter().map(|w| w.concat()).collect();
        for c in target.join(" ").chars() {
            press(app, KeyCode::Char(c));
        }
    }

    fn pb(app: &App) -> Option<Pb> {
        match app.screen {
            Screen::Results(Finished { pb, .. }) => pb,
            ref screen => panic!("not on results: {screen:?}"),
        }
    }

    fn press(app: &mut App, code: KeyCode) {
        app.handle_key_event(key(code, KeyModifiers::NONE));
    }

    #[test]
    fn esc_opens_menu() {
        let mut app = app(Config::default());
        press(&mut app, KeyCode::Esc);
        assert!(matches!(app.screen, Screen::Menu));
    }

    #[test]
    fn settings_from_menu_return_to_menu_then_new_mode_test() {
        let mut app = app(Config::default());
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Char('s'));
        press(&mut app, KeyCode::Right);
        assert_eq!(app.config.mode, Mode::Words);

        press(&mut app, KeyCode::Esc);
        assert!(matches!(app.screen, Screen::Menu));

        press(&mut app, KeyCode::Esc);
        assert!(matches!(app.screen, Screen::Typing));
        assert_eq!(app.session.limit(), Limit::Words);
        assert_eq!(app.session.words().len(), app.config.words);
    }

    #[test]
    fn settings_selection_wraps() {
        let mut app = app(Config::default());
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Char('s'));
        press(&mut app, KeyCode::Up);
        assert!(matches!(
            app.screen,
            Screen::Settings {
                selected: Setting::KeyTips,
                ..
            }
        ));
        press(&mut app, KeyCode::Char('j'));
        assert!(matches!(
            app.screen,
            Screen::Settings {
                selected: Setting::Mode,
                ..
            }
        ));
    }

    #[test]
    fn history_opens_from_menu_before_any_test() {
        let mut app = app(Config::default());
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Char('h'));
        assert!(matches!(
            app.screen,
            Screen::History {
                from: Origin::Menu,
                ..
            }
        ));
    }

    #[test]
    fn esc_from_history_returns_to_menu() {
        let mut app = app(Config::default());
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Char('h'));

        press(&mut app, KeyCode::Esc);
        assert!(matches!(app.screen, Screen::Menu));
    }

    #[test]
    fn leaving_menu_starts_a_new_test() {
        let mut app = app(Config::default());
        press(&mut app, KeyCode::Char('a'));
        press(&mut app, KeyCode::Esc);

        press(&mut app, KeyCode::Esc);
        assert!(matches!(app.screen, Screen::Typing));
        assert!(!app.session.is_started());
    }

    #[test]
    fn tab_from_settings_starts_a_test() {
        let mut app = app(Config::default());
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Char('s'));

        press(&mut app, KeyCode::Tab);
        assert!(matches!(app.screen, Screen::Typing));
    }

    #[test]
    fn esc_from_settings_returns_to_same_results() {
        let mut app = app(three_words());
        type_all(&mut app);
        let Screen::Results(finished) = app.screen else {
            panic!("not on results: {:?}", app.screen);
        };
        press(&mut app, KeyCode::Char('s'));

        press(&mut app, KeyCode::Esc);
        assert!(matches!(app.screen, Screen::Results(f) if f == finished));
    }

    #[test]
    fn esc_from_results_starts_a_new_test() {
        let mut app = app(three_words());
        type_all(&mut app);

        press(&mut app, KeyCode::Esc);
        assert!(matches!(app.screen, Screen::Typing));
    }

    #[test]
    fn right_arrow_before_start_lengthens_the_test() {
        let mut app = app(Config::default());
        press(&mut app, KeyCode::Right);

        assert_eq!(app.session.limit(), Limit::Time(Duration::from_secs(60)));
    }

    #[test]
    fn right_arrow_before_start_saves_the_config() {
        let dir = TempDir::new("quick-length");
        let file = ConfigFile::new(dir.0.join("config.toml"));
        let mut app = App {
            config_file: Some(file.clone()),
            ..app(Config::default())
        };
        press(&mut app, KeyCode::Right);

        assert_eq!(file.load().map(|config| config.time).ok(), Some(60));
    }

    #[test]
    fn arrows_after_start_are_ignored() {
        let mut app = app(Config::default());
        press(&mut app, KeyCode::Char('a'));
        press(&mut app, KeyCode::Right);
        press(&mut app, KeyCode::Down);

        assert_eq!(app.config, Config::default());
        assert!(app.session.is_started());
    }

    #[test]
    fn custom_length_survives_quick_round_trip() {
        let mut app = app(Config {
            mode: Mode::Words,
            words: 40,
            ..Config::default()
        });
        press(&mut app, KeyCode::Right);
        press(&mut app, KeyCode::Left);

        assert_eq!(app.config.words, 40);
    }

    #[test]
    fn down_arrow_before_start_switches_mode() {
        let mut app = app(Config::default());
        press(&mut app, KeyCode::Down);

        assert_eq!(app.session.limit(), Limit::Words);
    }

    #[test]
    fn help_closes_on_next_key_without_acting() {
        let mut app = app(three_words());
        type_all(&mut app);
        press(&mut app, KeyCode::Char('?'));
        assert!(app.help);

        press(&mut app, KeyCode::Char('h'));
        assert!(!app.help);
        assert!(matches!(app.screen, Screen::Results(_)));
    }

    #[test]
    fn words_mode_finishes_after_last_word() {
        let mut app = app(three_words());
        type_all(&mut app);

        assert!(matches!(app.screen, Screen::Results(_)));
        assert_eq!(app.session.words().len(), 3);
        assert_eq!(
            app.notice.as_deref(),
            Some("history not saved: no data directory found")
        );
    }

    #[test]
    fn first_saved_test_is_new_pb() {
        let dir = TempDir::new("first-pb");
        let mut app = app_with_history(three_words(), &dir);
        type_all(&mut app);

        assert_eq!(pb(&app), Some(Pb::New));
    }

    #[test]
    fn saved_test_is_added_to_records() {
        let dir = TempDir::new("added");
        let mut app = app_with_history(three_words(), &dir);
        type_all(&mut app);

        assert_eq!(app.records.map(|r| r.len()), Some(1));
    }

    #[test]
    fn unsaved_test_shows_no_pb() {
        let mut app = app(three_words());
        type_all(&mut app);

        assert_eq!(pb(&app), None);
    }

    #[test]
    fn unreadable_history_shows_no_pb() {
        let dir = TempDir::new("unreadable");
        let mut app = app_with_history(three_words(), &dir);
        app.records = None;
        type_all(&mut app);

        assert_eq!(pb(&app), None);
    }

    #[test]
    fn h_opens_history() {
        let mut app = app(three_words());
        type_all(&mut app);

        press(&mut app, KeyCode::Char('h'));
        assert!(matches!(app.screen, Screen::History { .. }));
    }

    #[test]
    fn esc_from_history_returns_to_same_results() {
        let mut app = app(three_words());
        type_all(&mut app);
        let Screen::Results(finished) = app.screen else {
            panic!("not on results: {:?}", app.screen);
        };
        press(&mut app, KeyCode::Char('h'));

        press(&mut app, KeyCode::Esc);
        assert!(matches!(app.screen, Screen::Results(f) if f == finished));
    }

    /// A history file of two lines that are not records.
    fn corrupt_history(dir: &TempDir) -> History {
        std::fs::create_dir_all(&dir.0).expect("create dir");
        let history = History::new(dir.0.join("history.jsonl"));
        std::fs::write(history.path(), "bad\nworse\n").expect("write");
        history
    }

    #[test]
    fn load_history_keeps_records_when_lines_skipped() {
        let dir = TempDir::new("skipped-records");
        let (records, _) = load_history(Some(&corrupt_history(&dir)));
        assert_eq!(records, Some(Vec::new()));
    }

    #[test]
    fn load_history_reports_skipped_lines() {
        let dir = TempDir::new("skipped-notice");
        let (_, notice) = load_history(Some(&corrupt_history(&dir)));
        assert!(
            notice
                .as_deref()
                .is_some_and(|n| n.starts_with("2 unreadable lines skipped")),
            "{notice:?}"
        );
    }

    #[test]
    fn load_history_without_data_dir_reports_notice() {
        let (_, notice) = load_history(None);
        assert_eq!(
            notice.as_deref(),
            Some("history not saved: no data directory found")
        );
    }

    #[test]
    fn finishing_samples_timeline() {
        let mut app = app(three_words());
        type_all(&mut app);

        assert_ne!(app.timeline.samples(), []);
    }

    #[test]
    fn restart_clears_timeline() {
        let mut app = app(three_words());
        type_all(&mut app);

        press(&mut app, KeyCode::Tab);
        assert_eq!(app.timeline.samples(), []);
    }

    #[test]
    fn saved_record_has_consistency_of_results() {
        let dir = TempDir::new("consistency");
        let mut app = app_with_history(three_words(), &dir);
        type_all(&mut app);
        let Screen::Results(Finished { results, .. }) = app.screen else {
            panic!("not on results: {:?}", app.screen);
        };

        let saved = app.records.and_then(|r| r.last().map(|r| r.consistency));
        assert_eq!(saved, Some(results.consistency));
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

        let results = Screen::Results(Finished {
            results: Results::from_session(&session, &Timeline::default()),
            pb: None,
        });
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
