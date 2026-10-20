//! Per-user directories, following the XDG base directory spec on every platform.
//!
//! Without the XDG variable, config and data fall back to `~/.config` and `~/.local/share` on
//! Linux and macOS, and both to `%APPDATA%` on Windows.

use std::ffi::OsString;
use std::path::PathBuf;

use directories::BaseDirs;

const APP_DIR: &str = "tecla";

/// tecla's config directory, or `None` when no home directory is known.
pub fn config_dir() -> Option<PathBuf> {
    app_dir("XDG_CONFIG_HOME", ".config")
}

/// tecla's data directory, or `None` when no home directory is known.
pub fn data_dir() -> Option<PathBuf> {
    app_dir("XDG_DATA_HOME", ".local/share")
}

fn app_dir(xdg_var: &str, home_fallback: &str) -> Option<PathBuf> {
    let fallback = BaseDirs::new().map(|dirs| {
        if cfg!(windows) {
            dirs.config_dir().to_owned()
        } else {
            dirs.home_dir().join(home_fallback)
        }
    });
    Some(xdg_home(std::env::var_os(xdg_var), fallback)?.join(APP_DIR))
}

/// `xdg` when set to an absolute path, as the XDG spec requires, else `fallback`.
fn xdg_home(xdg: Option<OsString>, fallback: Option<PathBuf>) -> Option<PathBuf> {
    xdg.map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xdg_home_must_be_absolute() {
        let fallback = Some(PathBuf::from("fallback"));
        let absolute = std::env::temp_dir();

        assert_eq!(
            xdg_home(Some(absolute.clone().into()), fallback.clone()),
            Some(absolute)
        );
        assert_eq!(
            xdg_home(Some("relative".into()), fallback.clone()),
            fallback
        );
        assert_eq!(xdg_home(None, fallback.clone()), fallback);
        assert_eq!(xdg_home(None, None), None);
    }
}
