//! Test results persisted as JSON Lines at `history.jsonl` in [`paths::data_dir`].

use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::Mode;
use crate::engine::{Limit, Session};
use crate::paths;
use crate::stats::Results;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consistency: Option<f64>,
}

impl Record {
    pub fn new(test: Test, timestamp: u64, results: &Results) -> Self {
        let (mode, words) = match test {
            Test::Time(_) => (Mode::Time, None),
            Test::Words(words) => (Mode::Words, Some(words)),
        };
        Self {
            timestamp,
            mode,
            duration_secs: results.duration.as_secs(),
            words,
            wpm: results.net_wpm,
            raw: results.raw_wpm,
            accuracy: results.accuracy,
            consistency: results.consistency,
        }
    }

    /// The kind of test this record is for; `None` for a words record missing its count.
    pub fn test(&self) -> Option<Test> {
        match self.mode {
            Mode::Time => Some(Test::Time(self.duration_secs)),
            Mode::Words => self.words.map(Test::Words),
        }
    }
}

/// A mode and its length; records of the same test are compared with each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Test {
    /// Seconds.
    Time(u64),
    /// Target word count.
    Words(usize),
}

impl Test {
    /// The test `session` runs.
    pub fn of(session: &Session) -> Self {
        match session.limit() {
            Limit::Time(duration) => Self::Time(duration.as_secs()),
            Limit::Words => Self::Words(session.words().len()),
        }
    }
}

impl fmt::Display for Test {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Time(secs) => write!(f, "time {secs}"),
            Self::Words(words) => write!(f, "words {words}"),
        }
    }
}

/// Records read from the history file.
#[derive(Debug, Default, PartialEq)]
pub struct Loaded {
    pub records: Vec<Record>,
    /// Lines that were not valid records.
    pub skipped: usize,
    /// Why the first skipped line was not valid, with its line number.
    pub first_error: Option<String>,
}

/// How a finished test compares with the best saved result for the same [`Test`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pb {
    New,
    /// The previous best wpm, which still stands.
    Standing(f64),
}

/// Results recorded for one [`Test`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Summary {
    pub test: Test,
    pub count: usize,
    pub best: f64,
    /// Mean wpm of the last [`RECENT`] tests.
    pub recent_avg: f64,
    pub last: f64,
}

/// Tests averaged in [`Summary::recent_avg`].
pub const RECENT: usize = 10;

/// How `record` compares with the best of `records` for the same test.
pub fn pb(records: &[Record], record: &Record) -> Pb {
    match record.test().and_then(|test| best(records, test)) {
        Some(best) if record.wpm <= best => Pb::Standing(best),
        _ => Pb::New,
    }
}

/// The highest wpm recorded for `test`.
fn best(records: &[Record], test: Test) -> Option<f64> {
    records
        .iter()
        .filter(|r| r.test() == Some(test))
        .map(|r| r.wpm)
        .max_by(f64::total_cmp)
}

/// One summary per test, ordered time before words, then by length.
pub fn summarize(records: &[Record]) -> Vec<Summary> {
    let mut by_test: BTreeMap<Test, Vec<f64>> = BTreeMap::new();
    for record in records {
        if let Some(test) = record.test() {
            by_test.entry(test).or_default().push(record.wpm);
        }
    }
    by_test
        .into_iter()
        .map(|(test, wpms)| summary(test, &wpms))
        .collect()
}

/// `wpms` in the order they were recorded; never empty, as [`summarize`] only groups
/// records it has.
fn summary(test: Test, wpms: &[f64]) -> Summary {
    let recent = &wpms[wpms.len().saturating_sub(RECENT)..];
    Summary {
        test,
        count: wpms.len(),
        best: wpms.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        recent_avg: recent.iter().sum::<f64>() / recent.len() as f64,
        last: wpms[wpms.len() - 1],
    }
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

    /// Reads every valid record, counting lines that are not; a missing file is an empty
    /// history.
    ///
    /// # Errors
    ///
    /// Returns an error if the file exists but cannot be read.
    pub fn read(&self) -> io::Result<Loaded> {
        let file = match fs::File::open(&self.path) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Loaded::default()),
            Err(e) => return Err(e),
        };
        let mut loaded = Loaded::default();
        // bytes, not `lines`, so a line of invalid UTF-8 is skipped like any other bad line
        for (index, line) in BufReader::new(file).split(b'\n').enumerate() {
            let line = line?;
            if line.trim_ascii().is_empty() {
                continue;
            }
            match serde_json::from_slice(&line) {
                Ok(record) => loaded.records.push(record),
                Err(e) => {
                    loaded.skipped += 1;
                    loaded
                        .first_error
                        .get_or_insert_with(|| format!("line {}: {e}", index + 1));
                }
            }
        }
        Ok(loaded)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

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
            consistency: Some(81.5),
        }
    }

    #[test]
    fn round_trip() {
        let dir = TempDir::new("round-trip");
        let history = History::new(dir.0.join("nested/history.jsonl"));

        history.append(&record(60.0)).expect("first append");
        history.append(&record(72.4)).expect("second append");

        let read = history.read().expect("read");
        assert_eq!(read.records, [record(60.0), record(72.4)]);
    }

    /// A line written before `words` and `consistency` existed, read back.
    fn read_old_record() -> Record {
        let dir = TempDir::new("old-record");
        fs::create_dir_all(&dir.0).expect("create dir");
        let history = History::new(dir.0.join("history.jsonl"));
        let old = r#"{"timestamp":1,"mode":"time","duration_secs":30,"wpm":1.0,"raw":1.0,"accuracy":1.0}"#;
        fs::write(history.path(), format!("{old}\n")).expect("write");
        history.read().expect("read").records.remove(0)
    }

    #[test]
    fn missing_words_defaults_to_none() {
        assert_eq!(read_old_record().words, None);
    }

    #[test]
    fn missing_consistency_defaults_to_none() {
        assert_eq!(read_old_record().consistency, None);
    }

    #[test]
    fn words_record_round_trips() {
        let dir = TempDir::new("words-record");
        let history = History::new(dir.0.join("history.jsonl"));
        let words = Record {
            mode: Mode::Words,
            words: Some(25),
            ..record(50.0)
        };
        history.append(&words).expect("append");

        assert_eq!(history.read().expect("read").records, [words]);
    }

    #[test]
    fn missing_file_is_empty_history() {
        let dir = TempDir::new("missing");
        let history = History::new(dir.0.join("history.jsonl"));
        assert_eq!(history.read().expect("read"), Loaded::default());
    }

    /// A history of an invalid JSON line, an invalid UTF-8 line and one valid record, read back.
    fn read_corrupt() -> Loaded {
        let dir = TempDir::new("corrupt");
        fs::create_dir_all(&dir.0).expect("create dir");
        let history = History::new(dir.0.join("history.jsonl"));
        let valid = serde_json::to_string(&record(60.0)).expect("serialize");
        fs::write(
            history.path(),
            [b"not json\n".as_slice(), b"\xff\n", valid.as_bytes()].concat(),
        )
        .expect("write");
        history.read().expect("read")
    }

    #[test]
    fn read_keeps_valid_records_past_corrupt_lines() {
        assert_eq!(read_corrupt().records, [record(60.0)]);
    }

    #[test]
    fn read_counts_corrupt_lines() {
        assert_eq!(read_corrupt().skipped, 2);
    }

    #[test]
    fn read_keeps_first_parse_error_line() {
        let first_error = read_corrupt().first_error;
        assert!(
            first_error
                .as_deref()
                .is_some_and(|e| e.starts_with("line 1: ")),
            "{first_error:?}"
        );
    }

    #[test]
    fn words_record_without_count_has_no_test() {
        let record = Record {
            mode: Mode::Words,
            ..record(50.0)
        };
        assert_eq!(record.test(), None);
    }

    #[test]
    fn time_test_displays_seconds() {
        assert_eq!(Test::Time(30).to_string(), "time 30");
    }

    #[test]
    fn words_test_displays_word_count() {
        assert_eq!(Test::Words(25).to_string(), "words 25");
    }

    fn results(net_wpm: f64) -> Results {
        Results {
            net_wpm,
            raw_wpm: net_wpm + 5.0,
            accuracy: 97.0,
            correct_chars: 0,
            incorrect_chars: 0,
            extra_chars: 0,
            missed_chars: 0,
            duration: Duration::from_secs(30),
            consistency: Some(80.0),
        }
    }

    #[test]
    fn new_time_record_keeps_its_test() {
        let record = Record::new(Test::Time(30), 1, &results(60.0));
        assert_eq!(record.test(), Some(Test::Time(30)));
    }

    #[test]
    fn new_words_record_keeps_its_test() {
        let record = Record::new(Test::Words(25), 1, &results(60.0));
        assert_eq!(record.test(), Some(Test::Words(25)));
    }

    fn words_record(words: usize, wpm: f64) -> Record {
        Record {
            mode: Mode::Words,
            words: Some(words),
            ..record(wpm)
        }
    }

    #[test]
    fn best_ignores_other_tests() {
        let records = [record(60.0), words_record(25, 90.0), record(70.0)];
        assert_eq!(best(&records, Test::Time(30)), Some(70.0));
    }

    #[test]
    fn best_of_untried_test_is_none() {
        assert_eq!(best(&[record(60.0)], Test::Time(60)), None);
    }

    #[test]
    fn first_record_of_a_test_is_new_pb() {
        assert_eq!(pb(&[words_record(25, 90.0)], &record(60.0)), Pb::New);
    }

    #[test]
    fn faster_record_is_new_pb() {
        assert_eq!(pb(&[record(60.0)], &record(70.0)), Pb::New);
    }

    #[test]
    fn slower_record_leaves_best_standing() {
        assert_eq!(pb(&[record(60.0)], &record(50.0)), Pb::Standing(60.0));
    }

    #[test]
    fn tying_record_leaves_best_standing() {
        assert_eq!(pb(&[record(60.0)], &record(60.0)), Pb::Standing(60.0));
    }

    #[test]
    fn summarize_groups_and_sorts_by_test() {
        let records = [words_record(25, 80.0), record(60.0), record(70.0)];
        let tests: Vec<Test> = summarize(&records).iter().map(|s| s.test).collect();
        assert_eq!(tests, [Test::Time(30), Test::Words(25)]);
    }

    #[test]
    fn summarize_counts_records() {
        let records = [record(60.0), record(80.0), record(70.0)];
        assert_eq!(summarize(&records)[0].count, 3);
    }

    #[test]
    fn summarize_finds_best() {
        let records = [record(60.0), record(80.0), record(70.0)];
        assert_eq!(summarize(&records)[0].best, 80.0);
    }

    #[test]
    fn summarize_keeps_last() {
        let records = [record(60.0), record(80.0), record(70.0)];
        assert_eq!(summarize(&records)[0].last, 70.0);
    }

    #[test]
    fn summarize_averages_only_last_ten() {
        let mut records = vec![record(0.0); 5];
        records.extend(vec![record(50.0); RECENT]);
        assert_eq!(summarize(&records)[0].recent_avg, 50.0);
    }
}
