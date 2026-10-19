//! Turning values into text for display: sizes, dates, shortened titles.

use std::borrow::Cow;
use std::time::SystemTime;

use chrono::{DateTime, Local};

/// Marker inserted where text was shortened.
const ELLIPSIS: &str = "...";

/// Format a byte count with binary units, e.g. `1536` becomes `"1.5 KiB"`.
///
/// # Arguments
///
/// * `bytes` - Size in bytes
///
/// # Returns
///
/// Whole bytes below 1 KiB, otherwise one decimal place in the largest fitting unit
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    // `as f64` is an explicit numeric cast; Rust never converts number types implicitly.
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// Format a timestamp as `YYYY-MM-DD HH:MM` in the local time zone.
///
/// # Arguments
///
/// * `time` - Timestamp to format
///
/// # Returns
///
/// The formatted local time
pub fn format_modified(time: SystemTime) -> String {
    DateTime::<Local>::from(time)
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

/// Shorten `text` to at most `max_chars` characters by replacing its middle with "...".
///
/// # Arguments
///
/// * `text` - Text to shorten
/// * `max_chars` - Maximum length in characters (Unicode scalar values), including "..."
///
/// # Returns
///
/// `text` unchanged (borrowed, no allocation) when it fits, otherwise its start and end
/// joined by "..."
///
/// # Examples
///
/// ```
/// assert_eq!(guiscp_core::format::shorten_middle("abcdefghij", 7), "ab...ij");
/// ```
pub fn shorten_middle(text: &str, max_chars: usize) -> Cow<'_, str> {
    let length = text.chars().count();
    if length <= max_chars {
        return Cow::Borrowed(text);
    }
    let kept = max_chars.saturating_sub(ELLIPSIS.len());
    // The start gets the extra character when `kept` is odd.
    let head_chars = kept.div_ceil(2);
    let tail_chars = kept - head_chars;
    // Convert character counts to byte offsets: Rust strings are UTF-8, so slicing by a
    // character count directly could split a multi-byte character.
    let head_end = text
        .char_indices()
        .nth(head_chars)
        .map_or(text.len(), |(offset, _)| offset);
    let tail_start = text
        .char_indices()
        .nth(length - tail_chars)
        .map_or(text.len(), |(offset, _)| offset);
    Cow::Owned(format!(
        "{}{ELLIPSIS}{}",
        &text[..head_end],
        &text[tail_start..]
    ))
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;

    #[test]
    fn format_size_uses_bytes_below_one_kibibyte() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1023), "1023 B");
    }

    #[test]
    fn format_size_scales_to_largest_unit() {
        assert_eq!(format_size(1024), "1.0 KiB");
        assert_eq!(format_size(1536), "1.5 KiB");
        assert_eq!(format_size(5 * 1024 * 1024 * 1024), "5.0 GiB");
        assert_eq!(format_size(u64::MAX), "16.0 EiB");
    }

    #[test]
    fn format_modified_produces_date_and_minutes() {
        // Arrange: noon UTC keeps the date stable in every time zone within +-11h.
        let time =
            UNIX_EPOCH + Duration::from_secs(1_700_000_000 - 1_700_000_000 % 86_400 + 43_200);

        // Act
        let text = format_modified(time);

        // Assert
        assert!(text.starts_with("2023-11-14 "), "unexpected: {text}");
        assert_eq!(text.len(), "2023-11-14 12:00".len());
    }

    #[test]
    fn shorten_middle_keeps_short_text() {
        assert!(matches!(
            shorten_middle("host - home", 32),
            Cow::Borrowed("host - home")
        ));
        let exact = "a".repeat(32);
        assert_eq!(shorten_middle(&exact, 32), exact);
    }

    #[test]
    fn shorten_middle_cuts_long_text_to_the_limit() {
        // Arrange
        let title = "workstation.example.com - a-very-long-project-folder-name";

        // Act
        let short = shorten_middle(title, 32);

        // Assert
        assert_eq!(short.chars().count(), 32);
        assert_eq!(short, "workstation.exa...ct-folder-name");
    }

    #[test]
    fn shorten_middle_counts_characters_not_bytes() {
        let short = shorten_middle("ééééééééééééééé", 9);
        assert_eq!(short, "ééé...ééé");
    }
}
