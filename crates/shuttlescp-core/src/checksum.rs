//! SHA-256 checksums of files, for checking that a copy matches its source.

use std::fmt;
use std::io;
use std::path::PathBuf;

use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt};

/// A file's SHA-256 checksum. Shown as `sha256sum` prints it: 64 lowercase hex digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checksum([u8; 32]);

impl Checksum {
    /// Read a checksum written as 64 hex digits (either case), as `sha256sum` and FTP
    /// servers answer.
    ///
    /// # Returns
    ///
    /// The checksum, or `None` if `text` is anything else
    pub fn from_hex(text: &str) -> Option<Self> {
        let text = text.as_bytes();
        if text.len() != 64 {
            return None;
        }
        let mut bytes = [0; 32];
        let (pairs, _) = text.as_chunks::<2>();
        for (byte, &[high, low]) in bytes.iter_mut().zip(pairs) {
            let digit = |c: u8| char::from(c).to_digit(16);
            // Each digit is below 16, so the pair fits a byte.
            *byte = u8::try_from(digit(high)? * 16 + digit(low)?).ok()?;
        }
        Some(Self(bytes))
    }
}

impl fmt::Display for Checksum {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// The checksum in a server's answer: its first word that is one. Answers differ:
/// `sha256sum` prints "<checksum>  <name>" (with a `\` before it when the name holds a
/// `\` or a line break), FTP's HASH "213 SHA-256 0-100 <checksum> <name>".
pub(crate) fn first_checksum_in(answer: &str) -> Option<Checksum> {
    answer
        .split_whitespace()
        .find_map(|word| Checksum::from_hex(word.strip_prefix('\\').unwrap_or(word)))
}

/// The checksum of a file on this computer. Reading and hashing run on a thread of
/// their own: a large file keeps a processor core busy for a while.
///
/// # Errors
///
/// Returns the error of opening or reading the file
pub async fn local_sha256(path: PathBuf) -> io::Result<Checksum> {
    tokio::task::spawn_blocking(move || {
        let mut file = std::fs::File::open(&path)?;
        let mut hasher = Sha256::new();
        io::copy(&mut file, &mut hasher)?;
        Ok(Checksum(hasher.finalize().into()))
    })
    .await
    .map_err(io::Error::other)?
}

/// The checksum of everything `reader` gives, such as a file read from a server.
///
/// # Errors
///
/// Returns the error of reading
pub(crate) async fn sha256_of(mut reader: impl AsyncRead + Unpin) -> io::Result<Checksum> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 256 * 1024];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            return Ok(Checksum(hasher.finalize().into()));
        }
        hasher.update(&buffer[..read]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SHA-256 of nothing at all, from the standard (FIPS 180-4) examples.
    const EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    /// SHA-256 of "abc", from the same examples.
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[tokio::test]
    async fn local_files_hash_to_the_standard_values() {
        // Arrange
        let dir = tempfile::tempdir().expect("temp dir");
        let (empty, abc) = (dir.path().join("empty"), dir.path().join("abc"));
        std::fs::write(&empty, "").expect("write");
        std::fs::write(&abc, "abc").expect("write");

        // Act
        let (empty, abc) = (local_sha256(empty).await, local_sha256(abc).await);

        // Assert
        assert_eq!(empty.expect("hash").to_string(), EMPTY);
        assert_eq!(abc.expect("hash").to_string(), ABC);
    }

    #[tokio::test]
    async fn a_stream_hashes_like_the_same_bytes_in_a_file() {
        // More than one buffer's worth, so the pieces must join up.
        let bytes: Vec<u8> = (0..700_000_u32).map(|i| (i % 251) as u8).collect();
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("f"), &bytes).expect("write");
        assert_eq!(
            sha256_of(bytes.as_slice()).await.expect("hash"),
            local_sha256(dir.path().join("f")).await.expect("hash")
        );
        assert_eq!(sha256_of(&b"abc"[..]).await.expect("hash").to_string(), ABC);
    }

    #[tokio::test]
    async fn a_missing_file_is_an_error() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert!(local_sha256(dir.path().join("missing")).await.is_err());
    }

    #[test]
    fn hex_reads_back_in_either_case() {
        let checksum = Checksum::from_hex(ABC).expect("valid");
        assert_eq!(checksum.to_string(), ABC);
        assert_eq!(Checksum::from_hex(&ABC.to_uppercase()), Some(checksum));
    }

    #[test]
    fn the_checksum_is_found_in_each_kind_of_answer() {
        let abc = Checksum::from_hex(ABC);
        for answer in [
            format!("{ABC}  notes.txt\n"),
            format!("{ABC} *notes.txt\n"),
            format!("\\{ABC}  odd\\nname\n"),
            format!("{ABC}\n"),
            format!("213 SHA-256 0-3 {ABC} notes.txt"),
            format!("250 {ABC}"),
        ] {
            assert_eq!(first_checksum_in(&answer), abc, "in {answer:?}");
        }
        assert_eq!(first_checksum_in("sha256sum: missing: No such file"), None);
        assert_eq!(first_checksum_in(""), None);
    }

    #[test]
    fn hex_of_the_wrong_length_or_with_other_characters_is_refused() {
        assert_eq!(Checksum::from_hex(""), None);
        assert_eq!(Checksum::from_hex(&ABC[..63]), None);
        assert_eq!(Checksum::from_hex(&format!("{ABC}0")), None);
        assert_eq!(Checksum::from_hex(&ABC.replace('b', "g")), None);
        assert_eq!(Checksum::from_hex(&ABC.replace("ba", "é")), None);
    }
}
