//! Notices when the window stops responding, and writes it to the log with what the
//! window was last doing, so a hang leaves a trace even when the app has to be forced
//! to close.
//!
//! The window's thread (GTK's main loop) stamps the time once a second. A separate
//! thread checks the stamp: when it is more than [`STALL`] old, the window is stuck. The
//! components note each message they handle (only its name, never what it carries),
//! so the log says whether the window was stuck inside one, and which.

use std::fmt::{self, Write};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use relm4::gtk::glib;

/// How long the window may go without answering before it counts as stuck.
const STALL: Duration = Duration::from_secs(10);

/// How often the window stamps the time, and the watchdog looks.
const TICK: Duration = Duration::from_secs(1);

/// While the window stays stuck, how often that is said again.
const REPEAT: Duration = Duration::from_secs(60);

/// A watchdog tick this much later than due means the computer was asleep, not the
/// window stuck: the window's stamp is that old too, and must not count.
const SLEPT: Duration = Duration::from_secs(5);

/// When the window last answered, in milliseconds since 1970.
static LAST_BEAT: AtomicU64 = AtomicU64::new(0);

/// What the window is doing: the message being handled, or the last one and that it
/// was finished.
static DOING: Mutex<Doing> = Mutex::new(Doing {
    message: String::new(),
    busy: false,
});

/// The message being handled (`busy`) or the last one handled.
struct Doing {
    message: String,
    busy: bool,
}

/// Milliseconds since 1970 (0 if the clock is before it).
fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| {
            u64::try_from(time.as_millis()).unwrap_or(u64::MAX)
        })
}

/// Start watching: the window stamps the time once a second, and a thread of its own
/// checks the stamps. Call once, on the window's thread, before the main loop runs.
pub fn start() {
    LAST_BEAT.store(now_millis(), Ordering::Relaxed);
    glib::timeout_add_local(TICK, || {
        LAST_BEAT.store(now_millis(), Ordering::Relaxed);
        glib::ControlFlow::Continue
    });
    let spawned = std::thread::Builder::new()
        .name("watchdog".to_owned())
        .spawn(watch);
    if let Err(error) = spawned {
        tracing::warn!("cannot start the watchdog: {error}");
    }
}

/// The watchdog thread: report when the window gets stuck, and when it answers again.
fn watch() {
    // Once reported stuck: the window's last stamp then, and when it was last reported.
    let mut stuck: Option<(u64, Instant)> = None;
    let mut last_tick = Instant::now();
    loop {
        std::thread::sleep(TICK);
        let late = last_tick.elapsed();
        last_tick = Instant::now();
        if late > SLEPT && stuck.is_none() {
            // Asleep (suspend, a paused virtual machine): the window's stamp is old for
            // that reason, so give it a fresh start.
            tracing::debug!("watchdog: woke after {} s asleep", late.as_secs());
            LAST_BEAT.store(now_millis(), Ordering::Relaxed);
            continue;
        }
        let beat = LAST_BEAT.load(Ordering::Relaxed);
        let silent = Duration::from_millis(now_millis().saturating_sub(beat));
        match stuck {
            None if silent >= STALL => {
                stuck = Some((beat, Instant::now()));
                tracing::warn!(
                    "the window has not responded for {} s; {}",
                    silent.as_secs(),
                    describe()
                );
            }
            Some((since, _)) if beat > since => {
                stuck = None;
                tracing::warn!(
                    "the window responds again, after {} s",
                    beat.saturating_sub(since) / 1000
                );
            }
            Some((since, reported)) if reported.elapsed() >= REPEAT => {
                // Still stuck: said again now and then, in case it moved on to
                // something else that is stuck too.
                stuck = Some((since, Instant::now()));
                tracing::warn!(
                    "the window has still not responded, for {} s; {}",
                    silent.as_secs(),
                    describe()
                );
            }
            _ => {}
        }
    }
}

/// What the window was doing, for the log.
fn describe() -> String {
    match DOING.lock() {
        Ok(doing) if doing.message.is_empty() => "no message handled yet".to_owned(),
        Ok(doing) if doing.busy => format!("stuck handling {}", doing.message),
        Ok(doing) => format!(
            "not inside a message (last handled: {}), so in GTK itself",
            doing.message
        ),
        Err(_) => "unknown".to_owned(),
    }
}

/// Note that a component starts handling `message`; the returned guard notes the end
/// when dropped. Only the message's name is kept, never its contents.
///
/// # Arguments
///
/// * `component` - Which component, e.g. "FilePane"
/// * `message` - The message
pub fn handling(component: &str, message: &impl fmt::Debug) -> Handling {
    let name = variant_name(message);
    if let Ok(mut doing) = DOING.lock() {
        doing.message.clear();
        doing.message.push_str(component);
        doing.message.push_str("::");
        doing.message.push_str(&name);
        doing.busy = true;
    }
    Handling
}

/// Notes the end of a message when dropped (see [`handling`]).
pub struct Handling;

impl Drop for Handling {
    fn drop(&mut self) {
        if let Ok(mut doing) = DOING.lock() {
            doing.busy = false;
        }
    }
}

/// The name of an enum variant from its `Debug` output, without what it carries:
/// formatting stops at the first `(`, `{` or space, so a message holding a big listing
/// costs no more than its name, and nothing it holds reaches the log.
fn variant_name(message: &impl fmt::Debug) -> String {
    /// Keeps what is written up to a delimiter, then refuses the rest.
    struct NameOnly(String);

    impl Write for NameOnly {
        fn write_str(&mut self, text: &str) -> fmt::Result {
            match text.find(['(', '{', ' ']) {
                Some(end) => {
                    self.0.push_str(&text[..end]);
                    Err(fmt::Error)
                }
                None => {
                    self.0.push_str(text);
                    Ok(())
                }
            }
        }
    }

    let mut name = NameOnly(String::new());
    // The error is ours: it stops the formatting once the name is complete.
    let _ = write!(name, "{message:?}");
    name.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    #[allow(dead_code)]
    enum Message {
        Plain,
        Tuple(String),
        Fields { secret: String },
    }

    #[test]
    fn only_the_name_of_a_message_is_kept() {
        assert_eq!(variant_name(&Message::Plain), "Plain");
        assert_eq!(variant_name(&Message::Tuple("a path".to_owned())), "Tuple");
        assert_eq!(
            variant_name(&Message::Fields {
                secret: "hunter2".to_owned()
            }),
            "Fields"
        );
    }

    #[test]
    fn the_log_says_which_message_is_stuck() {
        let guard = handling("FilePane", &Message::Tuple("x".to_owned()));
        assert_eq!(describe(), "stuck handling FilePane::Tuple");
        drop(guard);
        assert!(describe().contains("last handled: FilePane::Tuple"));
    }
}
