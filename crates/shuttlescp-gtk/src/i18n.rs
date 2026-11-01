//! The interface's texts in the user's language.
//!
//! The texts live in Fluent files (https://projectfluent.org), one folder per language
//! under `i18n/` (`i18n/en-US/shuttlescp.ftl`, ...), built into the program when it is
//! compiled: there is nothing to install beside it. English is the original and the
//! fallback for any text a translation lacks.
//!
//! In code, a text is fetched by its message name with [`tr!`]: `tr!("dialog-close")`,
//! or with values for its variables, `tr!("files-copied", count = 3)`.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::OnceLock;

pub use fluent_templates::fluent_bundle::FluentValue;
use fluent_templates::{LanguageIdentifier, Loader, static_loader};

static_loader! {
    static LOCALES = {
        locales: "./i18n",
        fallback_language: "en-US",
        // Fluent wraps each value in invisible direction marks, which only right-to-
        // left languages need; none is offered, and the marks would end up inside file
        // names and tooltips. Turn them back on with such a translation.
        customise: |bundle| bundle.set_use_isolating(false),
    };
}

/// The original language, and the fallback.
const FALLBACK: &str = "en-US";

/// The language the interface is shown in, set once at startup.
static LANGUAGE: OnceLock<LanguageIdentifier> = OnceLock::new();

/// Choose the interface language, once, before any window is built. A language with
/// no translation falls back to English.
///
/// # Arguments
///
/// * `tag` - A language tag, such as "fr-FR" (from the settings)
pub fn init(tag: &str) {
    let language = tag
        .parse::<LanguageIdentifier>()
        .ok()
        .filter(|language| LOCALES.locales().any(|available| available == language))
        .unwrap_or_else(|| {
            tracing::warn!("no translation for {tag}; using English");
            fallback()
        });
    tracing::debug!("interface language: {language}");
    // Set once: a second call (there is none) would change nothing.
    let _ = LANGUAGE.set(language);
}

/// A text in the interface language, by message name; its name if there is no such
/// message (in no language), so a mistake shows rather than crashing.
///
/// # Arguments
///
/// * `id` - The message name, e.g. "dialog-close"
pub fn text(id: &str) -> String {
    LOCALES.try_lookup(current(), id).unwrap_or_else(|| {
        tracing::warn!("no text for {id}");
        id.to_owned()
    })
}

/// [`text`], filling in the message's variables.
///
/// # Arguments
///
/// * `id` - The message name
/// * `args` - Each variable's name and value
pub fn text_with(id: &str, args: &[(&'static str, FluentValue<'static>)]) -> String {
    let args: HashMap<Cow<'static, str>, FluentValue> = args
        .iter()
        .map(|(name, value)| (Cow::Borrowed(*name), value.clone()))
        .collect();
    LOCALES
        .try_lookup_with_args(current(), id, &args)
        .unwrap_or_else(|| {
            tracing::warn!("no text for {id}");
            id.to_owned()
        })
}

/// The languages the interface can be shown in: each one's tag and its own name
/// (`language-name` in its file), English first, then by name.
pub fn languages() -> Vec<(String, String)> {
    let mut languages: Vec<(String, String)> = LOCALES
        .locales()
        .map(|language| {
            let name = LOCALES
                .try_lookup(language, "language-name")
                .unwrap_or_else(|| language.to_string());
            (language.to_string(), name)
        })
        .collect();
    languages.sort_by_key(|(tag, name)| (tag != FALLBACK, name.to_lowercase()));
    languages
}

/// The interface language: English until [`init`] chose one.
fn current() -> &'static LanguageIdentifier {
    LANGUAGE.get_or_init(fallback)
}

/// English.
fn fallback() -> LanguageIdentifier {
    // The tag is written above, so it always parses.
    FALLBACK.parse().unwrap_or_default()
}

/// A text in the interface language: `tr!("dialog-close")`, or with values for the
/// message's variables, `tr!("files-copied", count = 3, folder = name)`.
macro_rules! tr {
    ($id:literal) => {
        $crate::i18n::text($id)
    };
    ($id:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::text_with(
            $id,
            &[$((stringify!($name), $crate::i18n::FluentValue::from($value))),+],
        )
    };
}
pub(crate) use tr;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_texts_load() {
        assert_eq!(tr!("dialog-close"), "Close");
        assert_eq!(tr!("preferences-title"), "Preferences");
    }

    #[test]
    fn terms_are_filled_in() {
        // `{ -app-name }` inside the message becomes the product's name.
        assert!(tr!("preferences-restart").contains("ShuttleSCP"));
    }

    #[test]
    fn variables_are_filled_in() {
        assert_eq!(
            tr!("status-transfers-limited", running = 2, limit = 4),
            "Transfers: 2 of 4"
        );
    }

    #[test]
    fn blank_lines_in_a_message_are_kept() {
        let detail = tr!("login-trust-key-detail", fingerprint = "SHA256:abc");
        assert!(
            detail.contains("fingerprint is:\n\nSHA256:abc\n\nIf it matches"),
            "{detail:?}"
        );
    }

    #[test]
    fn plurals_can_nest() {
        assert_eq!(
            tr!("login-discarded", count = 2, deleted = 1),
            "Discarded 2 transfers: 1 partial file deleted"
        );
        assert_eq!(
            tr!("login-discarded", count = 1, deleted = 3),
            "Discarded 1 transfer: 3 partial files deleted"
        );
    }

    #[test]
    fn a_missing_text_shows_its_name() {
        assert_eq!(text("no-such-message"), "no-such-message");
    }

    /// Message names in a Fluent file: lines starting with a name and " =" (not
    /// comments, terms `-name`, or the continuation of a message).
    fn messages_in(ftl: &str) -> std::collections::BTreeSet<String> {
        ftl.lines()
            .filter_map(|line| line.split_once(" ="))
            .map(|(name, _)| name)
            .filter(|name| {
                name.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
                    && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            })
            .map(str::to_owned)
            .collect()
    }

    /// Message names the code asks for: every `tr!("name"` in the source files.
    fn messages_used() -> std::collections::BTreeSet<String> {
        fn scan(dir: &std::path::Path, used: &mut std::collections::BTreeSet<String>) {
            for entry in std::fs::read_dir(dir)
                .expect("read source folder")
                .flatten()
            {
                let path = entry.path();
                if path.is_dir() {
                    scan(&path, used);
                } else if path.extension().is_some_and(|ext| ext == "rs") {
                    let source = std::fs::read_to_string(&path).expect("read source file");
                    // The macro's own definition and documentation name examples.
                    if path.ends_with("i18n.rs") {
                        continue;
                    }
                    for (at, _) in source.match_indices("tr!(") {
                        // `tr!` itself, not the end of a longer name (`include_str!`).
                        let whole_word = source[..at]
                            .chars()
                            .next_back()
                            .is_none_or(|c| !c.is_alphanumeric() && c != '_');
                        // The name may be on the next line, where cargo fmt puts it.
                        let rest = source[at + "tr!(".len()..].trim_start();
                        if let Some((name, _)) = rest
                            .strip_prefix('"')
                            .and_then(|rest| rest.split_once('"'))
                            .filter(|_| whole_word)
                        {
                            used.insert(name.to_owned());
                        }
                    }
                }
            }
        }
        let mut used = std::collections::BTreeSet::new();
        scan(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut used,
        );
        used
    }

    #[test]
    fn every_text_the_code_uses_exists_and_every_text_is_used() {
        let english = messages_in(include_str!("../i18n/en-US/shuttlescp.ftl"));
        let used = messages_used();

        let missing: Vec<_> = used.difference(&english).collect();
        assert!(
            missing.is_empty(),
            "used in code, missing in en-US: {missing:?}"
        );
        // `language-name` is read by `languages`, not through `tr!`.
        let unused: Vec<_> = english
            .difference(&used)
            .filter(|name| *name != "language-name")
            .collect();
        assert!(unused.is_empty(), "in en-US, used nowhere: {unused:?}");
    }

    #[test]
    fn english_is_offered_first() {
        assert_eq!(
            languages().first(),
            Some(&(String::from("en-US"), String::from("English")))
        );
    }
}
