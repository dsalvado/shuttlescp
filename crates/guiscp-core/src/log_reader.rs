//! Following a growing log file, like `tail -f`: each call returns only the complete
//! lines added since the previous one. Front ends poll it and display the text.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::logging::LogLevel;

/// How much of the end of an existing file is returned by the first read.
const INITIAL_TAIL_BYTES: u64 = 64 * 1024;

/// Reader position in a log file.
#[derive(Debug)]
pub struct LogReader {
    path: PathBuf,
    /// Bytes of the file already returned (or skipped).
    offset: u64,
    /// The last read ended mid-line: the start of that line, waiting for the rest.
    partial: Vec<u8>,
}

impl LogReader {
    /// Start following `path`, beginning with its last [`INITIAL_TAIL_BYTES`] so a long
    /// log does not flood the display.
    ///
    /// # Arguments
    ///
    /// * `path` - Log file to follow (it may not exist yet)
    pub fn new(path: PathBuf) -> Self {
        let offset = std::fs::metadata(&path)
            .map_or(0, |meta| meta.len().saturating_sub(INITIAL_TAIL_BYTES));
        Self {
            path,
            offset,
            partial: Vec::new(),
        }
    }

    /// The file being followed.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Read whatever was added to the file since the last call.
    ///
    /// # Returns
    ///
    /// Complete new lines (ending in a newline), or `None` if there is nothing new. If
    /// the file shrank (cleared or rotated), reading restarts from its beginning.
    pub fn read_new_lines(&mut self) -> Option<String> {
        let mut file = File::open(&self.path).ok()?;
        let length = file.metadata().ok()?.len();
        if length < self.offset {
            self.offset = 0;
            self.partial.clear();
        }
        if length == self.offset {
            return None;
        }
        file.seek(SeekFrom::Start(self.offset)).ok()?;
        let mut bytes = std::mem::take(&mut self.partial);
        let read = file.read_to_end(&mut bytes).ok()?;
        // `read_to_end` returns a `usize`; on 64-bit systems it always fits a `u64`.
        self.offset += u64::try_from(read).unwrap_or(0);
        complete_lines(&mut bytes, &mut self.partial)
    }
}

/// Find the level a log line was written at, e.g. `ERROR` in
/// `[2026-10-19 17:08:13.017] [5a41] ERROR guiscp::app: ...`, so viewers can colour it.
///
/// # Arguments
///
/// * `line` - One line of the log file
///
/// # Returns
///
/// The level and the byte range of its word in `line`, or `None` for lines without one
/// (such as the continuation of a multi-line message)
pub fn line_level(line: &str) -> Option<(LogLevel, Range<usize>)> {
    // Skip the bracketed time and session tag in front.
    let mut start = 0;
    loop {
        let rest = &line[start..];
        let trimmed = rest.trim_start();
        start += rest.len() - trimmed.len();
        if !trimmed.starts_with('[') {
            break;
        }
        start += trimmed.find(']')? + 1;
    }
    let rest = &line[start..];
    let word = &rest[..rest.find(char::is_whitespace).unwrap_or(rest.len())];
    let level = match word {
        "ERROR" => LogLevel::Error,
        "WARN" => LogLevel::Warn,
        "INFO" => LogLevel::Info,
        "DEBUG" => LogLevel::Debug,
        "TRACE" => LogLevel::Trace,
        _ => return None,
    };
    Some((level, start..start + word.len()))
}

/// Split off the complete lines of `bytes`, keeping any unfinished last line in
/// `partial` for next time.
///
/// # Arguments
///
/// * `bytes` - Newly read data (consumed)
/// * `partial` - Receives the trailing incomplete line, if any
///
/// # Returns
///
/// The complete lines as text, or `None` if there are none yet
fn complete_lines(bytes: &mut Vec<u8>, partial: &mut Vec<u8>) -> Option<String> {
    let end = bytes.iter().rposition(|&byte| byte == b'\n')? + 1;
    *partial = bytes.split_off(end);
    Some(String::from_utf8_lossy(bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_lines_holds_back_an_unfinished_line() {
        let mut bytes = b"one\ntwo\nthr".to_vec();
        let mut partial = Vec::new();

        let lines = complete_lines(&mut bytes, &mut partial);

        assert_eq!(lines.as_deref(), Some("one\ntwo\n"));
        assert_eq!(partial, b"thr");
    }

    #[test]
    fn line_level_finds_the_level_word() {
        let line = "[2026-10-19 17:08:13.017] [5a41] ERROR guiscp::app: it broke";
        let (level, range) = line_level(line).expect("level");
        assert_eq!(level, LogLevel::Error);
        assert_eq!(&line[range], "ERROR");
        let (level, range) = line_level("[t] [ab] WARN  x").expect("level");
        assert_eq!((level, range), (LogLevel::Warn, 9..13));
        assert_eq!(line_level("    continued message"), None);
        assert_eq!(line_level("[unclosed"), None);
    }

    #[test]
    fn complete_lines_waits_when_no_line_is_finished() {
        let mut bytes = b"no newline yet".to_vec();
        let mut partial = Vec::new();

        assert_eq!(complete_lines(&mut bytes, &mut partial), None);
    }

    #[test]
    fn read_new_lines_follows_growth_and_restarts_after_truncation() {
        // Arrange
        let temp = tempfile::tempdir().expect("create temp dir");
        let path = temp.path().join("app.log");
        std::fs::write(&path, "first\n").expect("write log");
        let mut reader = LogReader::new(path.clone());

        // Act + Assert: initial content, then nothing new, then appended lines.
        assert_eq!(reader.read_new_lines().as_deref(), Some("first\n"));
        assert_eq!(reader.read_new_lines(), None);
        std::fs::write(&path, "first\nsecond\n").expect("append");
        assert_eq!(reader.read_new_lines().as_deref(), Some("second\n"));

        // The file is cleared and rewritten: reading restarts from the top.
        std::fs::write(&path, "new\n").expect("truncate");
        assert_eq!(reader.read_new_lines().as_deref(), Some("new\n"));
    }

    #[test]
    fn new_starts_near_the_end_of_a_long_file() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let path = temp.path().join("big.log");
        std::fs::write(&path, "x".repeat(200_000) + "\nlast line\n").expect("write log");

        let mut reader = LogReader::new(path);
        let text = reader.read_new_lines().expect("tail");

        assert!(text.len() <= 64 * 1024);
        assert!(text.ends_with("last line\n"));
    }
}
