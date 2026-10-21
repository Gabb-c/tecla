//! Which key does what on each screen; the single source for dispatch, hints and help.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::settings::Step;

/// A screen, as far as its keys and place in the navigation are concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Typing,
    Menu,
    Results,
    Settings,
    History,
}

/// A screen that the menu and the results open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Settings,
    History,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Starts a new test with fresh words.
    Restart,
    /// Goes up one level.
    Back,
    Quit,
    Open(Target),
    Help,
    /// Moves the settings selection.
    Select(Step),
    /// Changes the selected setting's value.
    Change(Step),
    /// Changes the next test's mode; only before the test starts.
    CycleMode,
    /// Changes the next test's length; only before the test starts.
    CycleLength(Step),
}

impl Action {
    /// Whether the action only applies until the test starts.
    pub fn is_next_test(self) -> bool {
        matches!(self, Self::CycleMode | Self::CycleLength(_))
    }
}

/// How a binding is shown in the footer and the help overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hint {
    pub keys: &'static str,
    pub label: &'static str,
}

/// The keys on one view that trigger one action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub keys: &'static [KeyCode],
    pub action: Action,
    /// `None` for a key already covered by another binding's hint.
    pub hint: Option<Hint>,
}

const fn bind(
    keys: &'static [KeyCode],
    action: Action,
    hint: (&'static str, &'static str),
) -> Binding {
    Binding {
        keys,
        action,
        hint: Some(Hint {
            keys: hint.0,
            label: hint.1,
        }),
    }
}

const fn silent(keys: &'static [KeyCode], action: Action) -> Binding {
    Binding {
        keys,
        action,
        hint: None,
    }
}

const RESTART: Binding = bind(&[KeyCode::Tab], Action::Restart, ("tab", "new test"));
const BACK: Binding = bind(&[KeyCode::Esc], Action::Back, ("esc", "back"));
const QUIT: Binding = bind(&[KeyCode::Char('q')], Action::Quit, ("q", "quit"));
const HELP: Binding = bind(&[KeyCode::Char('?')], Action::Help, ("?", "keys"));
const SETTINGS: Binding = bind(
    &[KeyCode::Char('s')],
    Action::Open(Target::Settings),
    ("s", "settings"),
);
const HISTORY: Binding = bind(
    &[KeyCode::Char('h')],
    Action::Open(Target::History),
    ("h", "history"),
);

const TYPING: &[Binding] = &[
    RESTART,
    bind(&[KeyCode::Esc], Action::Back, ("esc", "menu")),
    bind(
        &[KeyCode::Right],
        Action::CycleLength(Step::Next),
        ("←→", "length"),
    ),
    silent(&[KeyCode::Left], Action::CycleLength(Step::Prev)),
    bind(
        &[KeyCode::Up, KeyCode::Down],
        Action::CycleMode,
        ("↑↓", "mode"),
    ),
];

const MENU: &[Binding] = &[SETTINGS, HISTORY, RESTART, HELP, QUIT, BACK];

const RESULTS: &[Binding] = &[
    bind(
        &[KeyCode::Tab, KeyCode::Enter],
        Action::Restart,
        ("tab", "new test"),
    ),
    HISTORY,
    SETTINGS,
    silent(&[KeyCode::Esc], Action::Back),
    HELP,
    QUIT,
];

const SETTINGS_KEYS: &[Binding] = &[
    bind(
        &[KeyCode::Down, KeyCode::Char('j')],
        Action::Select(Step::Next),
        ("↑↓", "select"),
    ),
    silent(
        &[KeyCode::Up, KeyCode::Char('k')],
        Action::Select(Step::Prev),
    ),
    bind(
        &[
            KeyCode::Right,
            KeyCode::Char('l'),
            KeyCode::Char(' '),
            KeyCode::Enter,
        ],
        Action::Change(Step::Next),
        ("←→", "change"),
    ),
    silent(
        &[KeyCode::Left, KeyCode::Char('h')],
        Action::Change(Step::Prev),
    ),
    BACK,
    RESTART,
    HELP,
    QUIT,
];

const HISTORY_KEYS: &[Binding] = &[
    BACK,
    bind(
        &[KeyCode::Tab, KeyCode::Enter],
        Action::Restart,
        ("tab", "new test"),
    ),
    HELP,
    QUIT,
];

/// The bindings of `view`, in hint order; the first one holding a key wins.
pub fn bindings(view: View) -> &'static [Binding] {
    match view {
        View::Typing => TYPING,
        View::Menu => MENU,
        View::Results => RESULTS,
        View::Settings => SETTINGS_KEYS,
        View::History => HISTORY_KEYS,
    }
}

/// The action bound to `key` on `view`; ctrl and alt combinations are never bound.
pub fn action(view: View, key: KeyEvent) -> Option<Action> {
    if key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        return None;
    }
    bindings(view)
        .iter()
        .find(|binding| binding.keys.contains(&key.code))
        .map(|binding| binding.action)
}

/// The hinted bindings of `view`, in table order.
pub fn hints(view: View) -> impl Iterator<Item = (Action, Hint)> {
    bindings(view)
        .iter()
        .filter_map(|binding| Some((binding.action, binding.hint?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [View; 5] = [
        View::Typing,
        View::Menu,
        View::Results,
        View::Settings,
        View::History,
    ];

    fn press(view: View, code: KeyCode, modifiers: KeyModifiers) -> Option<Action> {
        action(view, KeyEvent::new(code, modifiers))
    }

    #[test]
    fn text_is_unbound_while_typing() {
        for c in ['a', 'q', 's', 'h', '?', ' '] {
            assert_eq!(
                press(View::Typing, KeyCode::Char(c), KeyModifiers::NONE),
                None
            );
        }
    }

    #[test]
    fn ctrl_and_alt_combinations_are_unbound() {
        assert_eq!(
            press(View::Menu, KeyCode::Char('q'), KeyModifiers::CONTROL),
            None
        );
        assert_eq!(press(View::Results, KeyCode::Tab, KeyModifiers::ALT), None);
    }

    #[test]
    fn shifted_keys_are_bound() {
        assert_eq!(
            press(View::Results, KeyCode::Char('?'), KeyModifiers::SHIFT),
            Some(Action::Help)
        );
    }

    #[test]
    fn tab_starts_a_new_test_everywhere() {
        for view in ALL {
            assert_eq!(
                press(view, KeyCode::Tab, KeyModifiers::NONE),
                Some(Action::Restart),
                "{view:?}"
            );
        }
    }

    #[test]
    fn esc_goes_back_everywhere() {
        for view in ALL {
            assert_eq!(
                press(view, KeyCode::Esc, KeyModifiers::NONE),
                Some(Action::Back),
                "{view:?}"
            );
        }
    }

    #[test]
    fn every_view_has_hints() {
        for view in ALL {
            assert_ne!(hints(view).count(), 0, "{view:?}");
        }
    }

    #[test]
    fn no_key_is_bound_twice_in_a_view() {
        for view in ALL {
            let keys: Vec<KeyCode> = bindings(view)
                .iter()
                .flat_map(|binding| binding.keys.iter().copied())
                .collect();
            for (i, key) in keys.iter().enumerate() {
                assert!(!keys[i + 1..].contains(key), "{key:?} twice in {view:?}");
            }
        }
    }
}
