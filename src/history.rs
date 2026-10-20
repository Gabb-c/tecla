//! Test results persisted as JSON Lines at `history.jsonl` in [`paths::data_dir`].

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::Mode;
use crate::paths;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    /// Seconds since the Unix epoch.
    pub timestamp: u64,
    pub mode: Mode,
    pub duration_secs: u64,
    /// Target word count in [`Mode::Words`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub words: Option<usize>,
    pub wpm: f64,
    pub raw: f64,
    pub accuracy: f64,
}

#[derive(Debug, Clone)]
pub struct History {
    path: PathBuf,
}

impl History {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// History in the XDG data directory, or `None` when no home directory is known.
    pub fn in_data_dir() -> Option<Self> {
        Some(Self::new(paths::data_dir()?.join("history.jsonl")))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends `record` as one line, creating the file and its directory if needed. The line
    /// goes out in a single write so lines from concurrent appends stay whole.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory or file cannot be created or written.
    pub fn append(&self, record: &Record) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let mut line = serde_json::to_string(record)?;
        line.push('\n');
        file.write_all(line.as_bytes())
    }

    /// Reads every record; a missing file is an empty history.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read, or [`io::ErrorKind::InvalidData`] if a
    /// line is not a valid record.
    #[cfg(test)]
    pub fn read(&self) -> io::Result<Vec<Record>> {
        use std::io::{BufRead, BufReader};

        let file = match fs::File::open(&self.path) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        BufReader::new(file)
            .lines()
            .filter(|line| line.as_ref().map_or(true, |l| !l.trim().is_empty()))
            .map(|line| Ok(serde_json::from_str(&line?)?))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TempDir;

    fn record(wpm: f64) -> Record {
        Record {
            timestamp: 1_760_000_000,
            mode: Mode::Time,
            duration_secs: 30,
            words: None,
            wpm,
            raw: wpm + 5.5,
            accuracy: 97.25,
        }
    }

    #[test]
    fn round_trip() {
        let dir = TempDir::new("round-trip");
        let history = History::new(dir.0.join("nested/history.jsonl"));

        history.append(&record(60.0)).expect("first append");
        history.append(&record(72.4)).expect("second append");

        let read = history.read().expect("read");
        assert_eq!(read, [record(60.0), record(72.4)]);
    }

    #[test]
    fn words_field_is_optional() {
        let dir = TempDir::new("words-field");
        fs::create_dir_all(&dir.0).expect("create dir");
        let history = History::new(dir.0.join("history.jsonl"));
        let old = r#"{"timestamp":1,"mode":"time","duration_secs":30,"wpm":1.0,"raw":1.0,"accuracy":1.0}"#;
        fs::write(history.path(), format!("{old}\n")).expect("write");
        let words = Record {
            mode: Mode::Words,
            words: Some(25),
            ..record(50.0)
        };
        history.append(&words).expect("append");

        let read = history.read().expect("read");
        assert_eq!(read[0].words, None);
        assert_eq!(read[1], words);
    }

    #[test]
    fn missing_file_is_empty_history() {
        let dir = TempDir::new("missing");
        let history = History::new(dir.0.join("history.jsonl"));
        assert_eq!(history.read().expect("read"), Vec::<Record>::new());
    }

    #[test]
    fn corrupt_line_is_an_error() {
        let dir = TempDir::new("corrupt");
        fs::create_dir_all(&dir.0).expect("create dir");
        let history = History::new(dir.0.join("history.jsonl"));
        fs::write(history.path(), "not json\n").expect("write");

        assert_eq!(
            history.read().expect_err("should fail").kind(),
            io::ErrorKind::InvalidData
        );
    }
}
