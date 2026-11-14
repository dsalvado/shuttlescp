//! Asking the questions an SSH server puts while logging in (two-factor authentication:
//! the code from an authenticator app, ...), as the server words them.

use std::cell::RefCell;
use std::rc::Rc;

use relm4::gtk;
use relm4::gtk::glib;
use relm4::gtk::prelude::*;
use secrecy::SecretString;

use shuttlescp_core::remote::{Challenge, Prompter};
use shuttlescp_core::tr;

use crate::dialog::{self, ButtonKind, Dialog};

/// The prompter logins use: it shows the server's questions over the window in front
/// and waits for the answers. It may be called from any thread (logins run in the
/// background), so the dialog is opened on the main thread.
pub fn prompter() -> Prompter {
    std::sync::Arc::new(|challenge: Challenge| {
        let (send, receive) = relm4::tokio::sync::oneshot::channel();
        glib::MainContext::default().invoke(move || {
            let Some(parent) = front_window() else {
                tracing::warn!("no window to ask the server's questions over");
                let _ = send.send(None);
                return;
            };
            ask(&parent, &challenge, move |answers| {
                // Fails only if the login has given up waiting.
                let _ = send.send(answers);
            });
        });
        Box::pin(async move { receive.await.ok().flatten() })
    })
}

/// The window in front: the Login dialog while logging in, otherwise the main window.
fn front_window() -> Option<gtk::Window> {
    let application = relm4::main_application();
    application
        .active_window()
        .or_else(|| application.windows().into_iter().next())
}

/// Ask `challenge`'s questions over `parent`. `on_answer` is called exactly once, with
/// the answers in order, or `None` if the user cancelled.
///
/// # Arguments
///
/// * `parent` - Window the questions belong to
/// * `challenge` - The server's questions
/// * `on_answer` - Receives the answers
pub fn ask(
    parent: &gtk::Window,
    challenge: &Challenge,
    on_answer: impl FnOnce(Option<Vec<SecretString>>) + 'static,
) {
    let dialog = Dialog::new(parent, &tr!("challenge-title"));
    dialog.content.append(&dialog::text(
        &tr!("challenge-question", server = challenge.server.clone()),
        &["heading"],
    ));
    // The server's own words, if it gave any; otherwise a hint at the usual question.
    let explanation = [challenge.name.trim(), challenge.instructions.trim()]
        .into_iter()
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let explanation = if explanation.is_empty() {
        tr!("challenge-explain")
    } else {
        explanation
    };
    dialog.content.append(&dialog::text(&explanation, &[]));

    // One entry per question, under the question as the server words it. Answers that
    // must stay secret are hidden as they are typed.
    let entries: Vec<gtk::Entry> = challenge
        .prompts
        .iter()
        .map(|(prompt, echo)| {
            let label = gtk::Label::new(Some(prompt.trim()));
            label.set_xalign(0.0);
            label.set_wrap(true);
            let entry = gtk::Entry::builder()
                .visibility(*echo)
                .activates_default(true)
                .width_request(320)
                .build();
            label.set_mnemonic_widget(Some(&entry));
            dialog.content.append(&label);
            dialog.content.append(&entry);
            entry
        })
        .collect();

    // Set by Log In, handed over when the window closes, so `on_answer` runs exactly
    // once however the dialog is dismissed.
    let answers: Rc<RefCell<Option<Vec<SecretString>>>> = Rc::default();
    dialog.add_close_button(&tr!("dialog-cancel"));
    let log_in = {
        let (answers, entries, window) =
            (Rc::clone(&answers), entries.clone(), dialog.window.clone());
        dialog.add_button(&tr!("challenge-log-in"), ButtonKind::Suggested, move || {
            *answers.borrow_mut() = Some(
                entries
                    .iter()
                    .map(|entry| SecretString::from(entry.text().as_str()))
                    .collect(),
            );
            window.close();
        })
    };
    let window = dialog.window;
    window.set_default_widget(Some(&log_in));
    let on_answer = RefCell::new(Some(on_answer));
    window.connect_close_request(move |_| {
        if let Some(on_answer) = on_answer.borrow_mut().take() {
            on_answer(answers.borrow_mut().take());
        }
        glib::Propagation::Proceed
    });
    window.present();
    if let Some(first) = entries.first() {
        first.grab_focus();
    }
}
