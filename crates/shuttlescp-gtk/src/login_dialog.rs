//! The Login dialog: saved sites on the left (with Edit and Delete), the session details
//! on the right (with Save and Cancel), and Import / Export / Login / Close at the bottom.
//!
//! The session fields stay disabled until the user clicks "New Site", or picks a saved
//! site and clicks Edit. Passwords are kept in the desktop keyring; the form shows
//! whether one is saved. SFTP and SCP sites can also log in with a private key file
//! and the desktop's SSH agent; a protected key's passphrase is asked for when needed. The work itself (keyring, login, export / import) is done by
//! `shuttlescp_core::session` and friends, run as background commands so the window never
//! freezes, e.g. while the desktop asks to unlock the keyring. This file only presents it.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, UNIX_EPOCH};

use relm4::RelmRemoveAllExt;
use relm4::gtk::prelude::*;
use relm4::prelude::*;
use secrecy::{ExposeSecret, SecretString};

use shuttlescp_core::format::format_modified;
use shuttlescp_core::path::SidePath;
use shuttlescp_core::pending::PendingJob;
use shuttlescp_core::remote::{ConnectError, HostKey, Remote};
use shuttlescp_core::secrets;
use shuttlescp_core::session::{self, ExportReport, ImportReport, KeyringAction};
use shuttlescp_core::site_export::ExportFile;
use shuttlescp_core::sites::{
    FtpEncryption, FtpOptions, Protocol, Site, SiteBook, SshAuth, new_site_id,
};
use shuttlescp_core::transfer::{self, JobKind, Side};

use crate::confirm_dialog::{self, Confirmation};
use crate::dialog;
use crate::key_passphrase_dialog::{self, KeyPassphrase};
use crate::passphrase_dialog::{self, PassphraseAnswer, PassphrasePurpose};
use crate::transfer_panel::PendingStore;

/// Label of the first entry in the site list, which starts a new site.
const NEW_SITE_LABEL: &str = "New Site";
/// Icon of the "New Site" entry.
const NEW_SITE_ICON: &str = "list-add-symbolic";
/// Icon of a saved site.
const SITE_ICON: &str = "network-server-symbolic";
/// Icons of the password field's show/hide toggle.
const SHOW_PASSWORD_ICON: &str = "view-reveal-symbolic";
const HIDE_PASSWORD_ICON: &str = "view-conceal-symbolic";
/// Icon next to the saved-password indicator.
const PASSWORD_ICON: &str = "dialog-password-symbolic";
/// File name suggested for exports.
const EXPORT_FILE_NAME: &str = "shuttlescp-sites.json";

/// Name of the Direct tab's page.
const DIRECT_TAB: &str = "direct";
/// Name of the Site to site tab's page.
const SITE_TO_SITE_TAB: &str = "site-to-site";
/// Name of the Unfinished tab's page.
const UNFINISHED_TAB: &str = "unfinished";

/// What the site list has picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    /// Nothing picked yet: the fields are empty and disabled.
    Nothing,
    /// "New Site" was clicked: the fields are empty and editable.
    NewSite,
    /// A saved site, by index into the site book.
    Saved(usize),
}

/// Messages handled by the Login dialog.
#[derive(Debug)]
pub enum LoginMsg {
    /// Open the dialog.
    Show,
    /// Close (hide) the dialog.
    Close,
    /// An entry in the site list was clicked.
    SiteClicked(Selection),
    /// An entry in the site list was double-clicked: open a saved site straight away.
    SiteDoubleClicked(Selection),
    /// Start editing the selected saved site.
    Edit,
    /// Ask to confirm deleting the selected saved site.
    Delete,
    /// The user confirmed: delete the selected saved site.
    DeleteConfirmed,
    /// Store the edited or new site.
    Save,
    /// Discard the edits in progress.
    Cancel,
    /// The protocol drop-down changed to this position.
    ProtocolSelected(u32),
    /// The FTP encryption drop-down changed to this position.
    EncryptionSelected(u32),
    /// The "Passive mode" box was ticked or cleared.
    PassiveToggled(bool),
    /// The port field changed.
    PortChanged(i32),
    /// The "Save password" box was ticked or cleared.
    SavePasswordToggled(bool),
    /// The "Browse..." button of the key file field was clicked.
    BrowseKeyFile,
    /// The user chose a private key file.
    KeyFileChosen(PathBuf),
    /// The "Use SSH agent" box was ticked or cleared.
    UseAgentToggled(bool),
    /// The user typed the passphrase of the site's key file: log in with it.
    KeyPassphraseGiven(KeyPassphrase),
    /// Text in a field changed (re-evaluates whether Save is allowed).
    FormEdited,
    /// The Export button was clicked.
    ExportClicked,
    /// The user chose where to export to.
    ExportTo(PathBuf),
    /// Write the export, protecting passwords with `passphrase` (`None` leaves them out).
    ExportConfirmed {
        /// File to write.
        path: PathBuf,
        /// Passphrase for the passwords, if they are included.
        passphrase: Option<SecretString>,
    },
    /// The Import button was clicked.
    ImportClicked,
    /// The user chose a file to import.
    ImportFrom(PathBuf),
    /// Import `file`, decrypting its passwords with `passphrase` if it has any.
    ImportConfirmed {
        /// The parsed export.
        file: ExportFile,
        /// Passphrase the export was made with.
        passphrase: Option<SecretString>,
    },
    /// A file chooser or passphrase prompt was cancelled.
    Abandoned,
    /// The "Show at startup" box was ticked or cleared.
    ShowAtStartupToggled(bool),
    /// Connect to the selected site (Direct), or to both picked sites (Site to site).
    Login,
    /// The Direct or Site to site tab was brought forward.
    TabShown(LoginTab),
    /// A site was picked in one of the site-to-site lists.
    PairSitePicked {
        /// 0 for the left pane's list, 1 for the right pane's.
        pane: usize,
        /// What was clicked.
        selection: Selection,
    },
    /// An unfinished transfer's box was ticked or cleared (by its record key).
    UnfinishedTicked {
        /// The record.
        key: String,
        /// Whether it is ticked now.
        ticked: bool,
    },
    /// An unfinished transfer's line was picked: show its details.
    UnfinishedPicked(Option<String>),
    /// Log in to the ticked transfers' site(s) and resume them there.
    ResumeUnfinished,
    /// Delete the ticked transfers' partial files (logging in for it) and forget them.
    DiscardUnfinished,
    /// The user trusts this new host key: remember it and connect.
    TrustHostKey(Box<HostKey>),
    /// The user trusts this server's TLS certificate: remember it and connect.
    TrustCertificate {
        /// Server address.
        host: String,
        /// Server port.
        port: u16,
        /// The certificate's fingerprint.
        fingerprint: String,
    },
}

/// What the Login dialog starts with.
#[derive(Debug)]
pub struct LoginInit {
    /// Path of the saved sites file.
    pub sites_path: PathBuf,
    /// Current "show the dialog at startup" preference.
    pub show_at_startup: bool,
    /// The unfinished transfers' records, shared with the transfer panels.
    pub pending: Rc<PendingStore>,
}

/// Transfers to resume in the session a login opens, picked on the Unfinished tab.
#[derive(Debug, Clone)]
pub struct Resume {
    /// Their records' keys.
    pub keys: Vec<String>,
    /// The folder to show in each pane (left is [`Side::Local`]): where the first of
    /// them was.
    pub folders: Vec<(Side, SidePath)>,
}

/// Notifications the Login dialog sends to the main window.
#[derive(Debug)]
pub enum LoginOutput {
    /// The user changed whether the dialog opens when the app starts.
    ShowAtStartupChanged(bool),
    /// Logged in: browse `remote`, the server of `site`.
    Connected {
        /// The site that was connected to.
        site: Site,
        /// The open SFTP session.
        remote: Remote,
        /// Unfinished transfers to resume in it.
        resume: Option<Resume>,
    },
    /// Logged in to two sites, for copying between them: `left` for the left pane and
    /// `right` for the right one.
    ConnectedPair {
        /// The left pane's site and its connection.
        left: (Site, Remote),
        /// The right pane's site and its connection.
        right: (Site, Remote),
        /// Unfinished transfers to resume in it.
        resume: Option<Resume>,
    },
}

/// The Login dialog's two tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginTab {
    /// This computer and one server: today's login.
    Direct,
    /// Two servers, for copying between them through this computer.
    SiteToSite,
    /// Transfers that did not finish, to resume or give up.
    Unfinished,
}

/// The lines of the details under a site-to-site list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PairDetail {
    Protocol,
    Address,
    User,
    Login,
}

/// The lines of the details under the Unfinished list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UnfinishedDetail {
    What,
    From,
    To,
    Saved,
}

/// A login from the Site to site or Unfinished tab, with saved passwords and keys only:
/// its sites are connected one after the other.
struct GroupLogin {
    /// One site, or site to site the left pane's then the right pane's.
    sites: Vec<Site>,
    /// The connections opened so far, in the same order.
    opened: Vec<Remote>,
    /// What to do once all are open.
    then: AfterLogin,
}

impl GroupLogin {
    /// The site being connected now.
    fn current(&self) -> &Site {
        &self.sites[self.opened.len().min(self.sites.len() - 1)]
    }

    /// Whether more sites follow the one being connected now.
    fn more_to_open(&self) -> bool {
        self.opened.len() + 1 < self.sites.len()
    }
}

/// What a [`GroupLogin`] does once its sites are open.
enum AfterLogin {
    /// Open a session on them.
    Open,
    /// Open a session on them and resume these transfers there.
    Resume(Resume),
    /// Delete these transfers' partial files, forget them, and disconnect.
    Discard(Vec<PendingJob>),
}

/// Results of background work, delivered back to the dialog.
#[derive(Debug)]
pub enum LoginCmd {
    /// A site's password was stored (`saved`) or removed in the keyring.
    PasswordChanged {
        /// The site.
        site_id: String,
        /// Whether the site now has a saved password.
        saved: bool,
        /// Error text if the keyring refused.
        result: Result<(), String>,
        /// The typed password, handed back when storing it failed: saving the site
        /// emptied the password field, and the password would otherwise be lost.
        unsaved: Option<SecretString>,
    },
    /// The keyring entry of a deleted site was removed.
    PasswordForgotten(Result<(), String>),
    /// A site's key passphrase was stored in the keyring (after it opened the key).
    PassphraseSaved {
        /// The site.
        site_id: String,
        /// Error text if the keyring refused.
        result: Result<(), String>,
    },
    /// An export finished, with a summary or an error.
    Exported(Result<ExportReport, String>),
    /// An import finished: the sites (with `password_saved` set where the keyring
    /// accepted the password) and how many passwords could not be stored.
    Imported(Result<ImportReport, String>),
    /// A login attempt finished.
    Connected(Result<(Site, Remote), ConnectError>),
    /// Unfinished transfers were given up: `keys` had their partial files deleted
    /// (`deleted` files in all); `failed` says which could not be, and why.
    Discarded {
        /// The records whose partial files are gone.
        keys: Vec<String>,
        /// How many partial files were deleted.
        deleted: usize,
        /// "title: reason" for each transfer that could not be cleaned up.
        failed: Vec<String>,
    },
}

/// State of the Login dialog.
pub struct LoginDialog {
    book: SiteBook,
    sites_path: PathBuf,
    selection: Selection,
    /// Whether the session fields can be changed.
    editing: bool,
    protocol: Protocol,
    port: u16,
    /// FTP settings (shown for FTP sites).
    ftp: FtpOptions,
    /// Whether Save stores the typed password in the keyring.
    save_password: bool,
    // Text buffers shared with the entries: the entries edit them directly, and the
    // dialog reads and fills them, so typing needs no message round trip.
    host: gtk::EntryBuffer,
    user: gtk::EntryBuffer,
    password: gtk::EntryBuffer,
    /// The private key file field (SFTP and SCP).
    key_file: gtk::EntryBuffer,
    /// Whether the site also tries the SSH agent's keys.
    use_agent: bool,
    /// A key passphrase typed for the login under way, and whether to save it once it
    /// has worked.
    pending_passphrase: Option<KeyPassphrase>,
    site_list: SiteList,
    /// Last result or progress note, shown at the bottom left.
    status: String,
    /// An export or import is running.
    busy: bool,
    /// The open file chooser; kept here so it stays alive until the user answers.
    chooser: Option<gtk::FileChooserNative>,
    /// Whether the dialog opens when the app starts (saved by the main window).
    show_at_startup: bool,
    /// The tab in front.
    tab: LoginTab,
    /// The site-to-site lists (left pane, right pane), and the site picked in each, by
    /// id: sites added or deleted on Direct move the others in the list.
    pair_lists: [SiteList; 2],
    pair_picks: [Option<String>; 2],
    /// A login from the Site to site or Unfinished tab under way.
    group_login: Option<GroupLogin>,
    /// The unfinished transfers' records (shared with the transfer panels), those the
    /// Unfinished tab lists, the ones ticked and the one whose details show (by key).
    pending: Rc<PendingStore>,
    unfinished: Vec<PendingJob>,
    unfinished_ticked: HashSet<String>,
    unfinished_picked: Option<String>,
    unfinished_list: gtk::ListBox,
    /// The tabs, to bring Direct forward each time the dialog opens.
    tabs: Option<gtk::Stack>,
}

impl LoginDialog {
    /// The site a login under way is for: the current one of a pair, or the picked one.
    fn login_site(&self) -> Option<&Site> {
        self.group_login
            .as_ref()
            .map_or_else(|| self.selected_site(), |pair| Some(pair.current()))
    }

    /// The two sites picked on the Site to site tab, if both are.
    fn pair_sites(&self) -> Option<[Site; 2]> {
        Some([self.pair_site(0)?.clone(), self.pair_site(1)?.clone()])
    }

    /// Where the site picked on a site-to-site list is in the site book, if it still is.
    fn pair_index(&self, pane: usize) -> Option<usize> {
        let id = self.pair_picks[pane].as_deref()?;
        self.book.sites.iter().position(|site| site.id == id)
    }

    /// The site picked on a site-to-site list, if it still exists.
    fn pair_site(&self, pane: usize) -> Option<&Site> {
        self.book.sites.get(self.pair_index(pane)?)
    }

    /// One line of the details under a site-to-site list, for the picked site: its
    /// protocol (and FTP encryption), address, user, and how it logs in. Blank until a
    /// site is picked.
    fn pair_detail(&self, pane: usize, detail: PairDetail) -> String {
        let Some(site) = self.pair_site(pane) else {
            return String::new();
        };
        match detail {
            PairDetail::Protocol if site.protocol == Protocol::Ftp => {
                format!("FTP \u{b7} {}", site.ftp.encryption.label())
            }
            PairDetail::Protocol => site.protocol.label().to_owned(),
            PairDetail::Address => format!("{}:{}", site.host, site.port),
            PairDetail::User => site.user.clone(),
            PairDetail::Login => {
                let key = site
                    .ssh
                    .key_file
                    .as_ref()
                    .filter(|_| site.protocol != Protocol::Ftp)
                    .map(|path| {
                        path.file_name().map_or_else(
                            || path.display().to_string(),
                            |name| name.to_string_lossy().into_owned(),
                        )
                    });
                let agent = site.protocol != Protocol::Ftp && site.ssh.use_agent;
                let mut ways = Vec::new();
                if let Some(key) = key {
                    ways.push(format!("key file {key}"));
                }
                if agent {
                    ways.push(String::from("SSH agent"));
                }
                if site.password_saved {
                    ways.push(String::from("saved password"));
                }
                if ways.is_empty() {
                    String::from("no saved password")
                } else {
                    ways.join(", then ")
                }
            }
        }
    }

    /// Reread the unfinished transfers (sessions may have finished or recorded some) and
    /// list them, keeping the ticks and the pick of those still there.
    fn refresh_unfinished(&mut self, sender: &ComponentSender<Self>) {
        self.unfinished = self.pending.unclaimed();
        let keys: HashSet<&String> = self.unfinished.iter().map(|pending| &pending.key).collect();
        self.unfinished_ticked.retain(|key| keys.contains(key));
        if self
            .unfinished_picked
            .as_ref()
            .is_some_and(|key| !keys.contains(key))
        {
            self.unfinished_picked = None;
        }
        // Row by row: relm4's `remove_all` takes the placeholder for a row and panics.
        while let Some(row) = self.unfinished_list.row_at_index(0) {
            self.unfinished_list.remove(&row);
        }
        for pending in &self.unfinished {
            let row = unfinished_row(
                pending,
                &self.unfinished_route(pending),
                self.unfinished_ticked.contains(&pending.key),
                sender,
            );
            self.unfinished_list.append(&row);
            if self.unfinished_picked.as_ref() == Some(&pending.key)
                && let Some(row) = self
                    .unfinished_list
                    .last_child()
                    .and_downcast::<gtk::ListBoxRow>()
            {
                self.unfinished_list.select_row(Some(&row));
            }
        }
        self.show_unfinished_count();
    }

    /// Title the Unfinished tab with how many there are: "Unfinished (2)".
    fn show_unfinished_count(&self) {
        let Some(page) = self
            .tabs
            .as_ref()
            .and_then(|tabs| Some(tabs.page(&tabs.child_by_name(UNFINISHED_TAB)?)))
        else {
            return;
        };
        page.set_title(&match self.unfinished.len() {
            0 => String::from("Unfinished"),
            count => format!("Unfinished ({count})"),
        });
    }

    /// A site's name, or a note that it was deleted.
    fn site_name(&self, site_id: &str) -> String {
        self.book
            .sites
            .iter()
            .find(|site| site.id == site_id)
            .map_or_else(|| String::from("(deleted site)"), Site::display_name)
    }

    /// Where an unfinished transfer runs: "site", or site to site "left ⇄ right".
    fn unfinished_route(&self, pending: &PendingJob) -> String {
        match &pending.left_site_id {
            Some(left) => format!(
                "{} \u{21c4} {}",
                self.site_name(left),
                self.site_name(&pending.site_id)
            ),
            None => self.site_name(&pending.site_id),
        }
    }

    /// The ticked transfers, oldest first.
    fn ticked_unfinished(&self) -> Vec<PendingJob> {
        self.unfinished
            .iter()
            .filter(|pending| self.unfinished_ticked.contains(&pending.key))
            .cloned()
            .collect()
    }

    /// The saved sites the ticked transfers need (site to site, the left pane's first),
    /// if they all need the same ones and those still exist.
    ///
    /// # Errors
    ///
    /// Why Resume and Discard cannot log in for them (empty when nothing is ticked)
    fn unfinished_sites(&self) -> Result<Vec<Site>, String> {
        let ticked = self.ticked_unfinished();
        let Some(first) = ticked.first() else {
            return Err(String::new());
        };
        if ticked.iter().any(|pending| {
            pending.site_id != first.site_id || pending.left_site_id != first.left_site_id
        }) {
            return Err(String::from(
                "Tick transfers of one site (or one pair of sites) at a time.",
            ));
        }
        first
            .left_site_id
            .iter()
            .chain([&first.site_id])
            .map(|id| {
                self.book
                    .sites
                    .iter()
                    .find(|site| &site.id == id)
                    .cloned()
                    .ok_or_else(|| {
                        String::from(
                            "Its site was deleted, so it cannot be resumed. Discard \
                             forgets it (delete any partial files by hand).",
                        )
                    })
            })
            .collect()
    }

    /// The details of the picked unfinished transfer: what it did, from where to where,
    /// and when it was saved. Blank until one is picked.
    fn unfinished_detail(&self, detail: UnfinishedDetail) -> String {
        let Some(pending) = self
            .unfinished_picked
            .as_ref()
            .and_then(|key| self.unfinished.iter().find(|pending| &pending.key == key))
        else {
            return String::new();
        };
        let job = &pending.job;
        // Which site each pane is: left is this computer or, site to site, a server.
        let pane_name = |pane: Side| match (pane, &pending.left_site_id) {
            (Side::Local, Some(left)) => self.site_name(left),
            (Side::Local, None) => String::from("this computer"),
            (Side::Remote, _) => self.site_name(&pending.site_id),
        };
        let folders = job.pane_folders();
        let folder = |index: usize| {
            folders
                .get(index)
                .map(|(pane, dir)| format!("{dir} ({})", pane_name(*pane)))
                .unwrap_or_default()
        };
        match detail {
            UnfinishedDetail::What => match job.kind {
                JobKind::Copy => format!("Copy {}", job.subject()),
                JobKind::Move => format!("Move {}", job.subject()),
                JobKind::Delete => format!("Delete {}", job.subject()),
            },
            UnfinishedDetail::From => folder(0),
            UnfinishedDetail::To => folder(1),
            UnfinishedDetail::Saved => pending.saved_at.map_or_else(
                || String::from("Unknown (saved by an older version)"),
                |seconds| format_modified(UNIX_EPOCH + Duration::from_secs(seconds)),
            ),
        }
    }

    /// Log in to the ticked transfers' site(s), then resume them there or delete their
    /// partial files. Ticked transfers whose site was deleted are just forgotten.
    fn act_on_unfinished(&mut self, resume: bool, sender: &ComponentSender<Self>) {
        if self.busy {
            return;
        }
        let ticked = self.ticked_unfinished();
        let sites = match self.unfinished_sites() {
            Ok(sites) => sites,
            Err(_) if !resume && !ticked.is_empty() && self.unfinished_orphaned(&ticked) => {
                let keys: Vec<String> = ticked.into_iter().map(|pending| pending.key).collect();
                tracing::info!("forgetting {} transfers of deleted sites", keys.len());
                self.pending.forget(&keys);
                self.status = String::from("Forgotten");
                self.refresh_unfinished(sender);
                return;
            }
            Err(reason) => {
                self.status = reason;
                return;
            }
        };
        let then = if resume {
            AfterLogin::Resume(Resume {
                keys: ticked.iter().map(|pending| pending.key.clone()).collect(),
                folders: ticked
                    .first()
                    .map(|pending| pending.job.pane_folders())
                    .unwrap_or_default(),
            })
        } else {
            AfterLogin::Discard(ticked)
        };
        tracing::info!(
            "logging in to {} to {} unfinished transfers",
            sites
                .iter()
                .map(Site::display_name)
                .collect::<Vec<_>>()
                .join(" and "),
            if resume { "resume" } else { "discard" }
        );
        self.pending_passphrase = None;
        self.group_login = Some(GroupLogin {
            sites,
            opened: Vec::new(),
            then,
        });
        self.start_login(sender, None);
    }

    /// Whether every one of these transfers lost a site it needs (it was deleted).
    fn unfinished_orphaned(&self, jobs: &[PendingJob]) -> bool {
        let exists = |id: &String| self.book.sites.iter().any(|site| &site.id == id);
        jobs.iter().all(|pending| {
            !exists(&pending.site_id) || pending.left_site_id.as_ref().is_some_and(|id| !exists(id))
        })
    }

    /// Delete the partial files of given-up transfers in the background, over the
    /// connections just opened for it; they close once it is done.
    ///
    /// # Arguments
    ///
    /// * `jobs` - The transfers
    /// * `left`, `right` - Site to site the left pane's server, and the right pane's (or
    ///   the only) server
    /// * `sender` - Receives [`LoginCmd::Discarded`]
    fn discard_partials(
        &mut self,
        jobs: Vec<PendingJob>,
        left: Option<Remote>,
        right: Remote,
        sender: &ComponentSender<Self>,
    ) {
        self.busy = true;
        self.status = String::from("Deleting partial files...");
        sender.oneshot_command(async move {
            let (mut keys, mut deleted, mut failed) = (Vec::new(), 0, Vec::new());
            for pending in jobs {
                let title = pending.job.title();
                let (from, to) = pending.job.connections(left.as_ref(), &right);
                match transfer::discard_partials(pending.job, from, to).await {
                    Ok(count) => {
                        tracing::info!("{title}: {count} partial files deleted");
                        deleted += count;
                        keys.push(pending.key);
                    }
                    Err(error) => {
                        tracing::warn!("{title}: partial files not deleted: {error}");
                        failed.push(format!("{title}: {error}"));
                    }
                }
            }
            LoginCmd::Discarded {
                keys,
                deleted,
                failed,
            }
        });
    }

    /// Refill both site-to-site lists from the site book, keeping their picks.
    fn fill_pair_lists(&self) {
        for (pane, list) in self.pair_lists.iter().enumerate() {
            list.fill(
                &self.book.sites,
                self.pair_index(pane)
                    .map_or(Selection::Nothing, Selection::Saved),
            );
        }
    }
    /// The saved site currently picked, if any.
    fn selected_site(&self) -> Option<&Site> {
        match self.selection {
            Selection::Saved(index) => self.book.sites.get(index),
            Selection::Nothing | Selection::NewSite => None,
        }
    }

    /// Whether the picked site has a password in the keyring.
    fn password_saved(&self) -> bool {
        self.selected_site().is_some_and(|site| site.password_saved)
    }

    /// Show the picked site's details (blank for nothing or "New Site"), always clearing
    /// the typed password. Editing starts only for "New Site".
    fn load_form(&mut self) {
        let site = self.selected_site().cloned();
        self.protocol = site
            .as_ref()
            .map_or(Protocol::default(), |site| site.protocol);
        self.ftp = site
            .as_ref()
            .map_or_else(FtpOptions::default, |site| site.ftp);
        self.port = site
            .as_ref()
            .map_or_else(|| self.default_port(), |site| site.port);
        self.host
            .set_text(site.as_ref().map_or("", |site| site.host.as_str()));
        self.user
            .set_text(site.as_ref().map_or("", |site| site.user.as_str()));
        self.password.set_text("");
        let ssh = site
            .as_ref()
            .map_or_else(SshAuth::default, |site| site.ssh.clone());
        self.key_file.set_text(
            ssh.key_file
                .as_deref()
                .map_or_else(String::new, |path| path.display().to_string()),
        );
        self.use_agent = ssh.use_agent;
        self.pending_passphrase = None;
        // New sites offer to save the password; saved ones keep their current choice.
        self.save_password = site.as_ref().is_none_or(|site| site.password_saved);
        self.editing = self.selection == Selection::NewSite;
    }

    /// The usual port for the protocol (and FTP encryption) in the form.
    fn default_port(&self) -> u16 {
        default_port(self.protocol, self.ftp.encryption)
    }

    /// Pick `selection`, refresh the list highlight and the fields.
    ///
    /// # Arguments
    ///
    /// * `selection` - What to pick
    fn select(&mut self, selection: Selection) {
        self.selection = selection;
        self.site_list.fill(&self.book.sites, selection);
        self.load_form();
    }

    /// Whether the form holds enough to save a site (a host name is required).
    fn can_save(&self) -> bool {
        !self.host.text().trim().is_empty()
    }

    /// Whether Login applies: on the Direct tab, a saved site is picked and nothing else
    /// is going on; site to site, both sites are picked and nothing is going on.
    fn can_login(&self) -> bool {
        match self.tab {
            LoginTab::Direct => self.saved_site_idle(),
            LoginTab::SiteToSite => !self.busy && self.pair_sites().is_some(),
            // Its own Resume and Discard buttons log in.
            LoginTab::Unfinished => false,
        }
    }

    /// Start connecting to the picked site in the background, with the typed password
    /// or else the one saved in the keyring.
    ///
    /// # Arguments
    ///
    /// * `sender` - Used to run the connection attempt
    /// * `approved_key` - A host key the user has just agreed to trust
    fn start_login(&mut self, sender: &ComponentSender<Self>, approved_key: Option<HostKey>) {
        if self.busy || (self.group_login.is_none() && !self.can_login()) {
            return;
        }
        let Some(site) = self.login_site().cloned() else {
            return;
        };
        // Site to site uses saved passwords and keys only (the password field belongs to
        // the Direct tab).
        let typed = (self.group_login.is_none() && !self.password.text().is_empty())
            .then(|| SecretString::from(self.password.text().as_str()));
        let passphrase = self
            .pending_passphrase
            .as_ref()
            .map(|pending| pending.passphrase.clone());
        self.busy = true;
        self.status = format!("Connecting to {}...", site.display_name());
        sender.oneshot_command(async move {
            let result = session::login(&site, typed, passphrase, approved_key).await;
            LoginCmd::Connected(result.map(|remote| (site, remote)))
        });
    }

    /// Whether a saved site is picked and not being edited (Edit / Delete apply).
    fn saved_site_idle(&self) -> bool {
        matches!(self.selection, Selection::Saved(_)) && !self.editing && !self.busy
    }

    /// Grey hint text for the password field, reflecting whether one is saved.
    fn password_placeholder(&self) -> &'static str {
        match (self.editing, self.password_saved()) {
            (true, true) => "Saved - type to replace",
            (true, false) => "",
            (false, true) => {
                "\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022} (saved)"
            }
            (false, false) => "Not saved",
        }
    }

    /// Text of the indicator under the form.
    fn password_status(&self) -> &'static str {
        if self.password_saved() {
            "Password saved in the keyring"
        } else {
            "No saved password"
        }
    }

    /// Store the form as a new or updated site, write all sites to disk, and update its
    /// keyring entry in the background.
    ///
    /// # Arguments
    ///
    /// * `sender` - Used to run the keyring update
    fn save(&mut self, sender: &ComponentSender<Self>) {
        if !self.editing || !self.can_save() {
            return;
        }
        let existing = self.selected_site().cloned();
        let key_file = key_file_path(&self.key_file.text());
        // A saved passphrase belongs to the key it opened: it goes when the key changes.
        let old_passphrase = existing
            .as_ref()
            .filter(|site| site.ssh.passphrase_saved && site.ssh.key_file != key_file)
            .map(|site| session::passphrase_entry(&site.id));
        let site = Site {
            id: existing
                .as_ref()
                .map_or_else(new_site_id, |site| site.id.clone()),
            protocol: self.protocol,
            host: self.host.text().trim().to_owned(),
            port: self.port,
            user: self.user.text().trim().to_owned(),
            password_saved: existing.as_ref().is_some_and(|site| site.password_saved),
            ftp: self.ftp,
            ssh: SshAuth {
                passphrase_saved: old_passphrase.is_none()
                    && existing
                        .as_ref()
                        .is_some_and(|site| site.ssh.passphrase_saved),
                key_file,
                use_agent: self.use_agent,
            },
        };
        if let Some(entry) = old_passphrase {
            sender.oneshot_command(async move {
                LoginCmd::PasswordForgotten(
                    secrets::delete_password(&entry)
                        .await
                        .map_err(|error| error.to_string()),
                )
            });
        }
        // Capture the typed password and the "Save password" choice now: `select` below
        // reloads the form for the saved site, which clears the field and resets the box.
        let typed = (!self.password.text().is_empty())
            .then(|| SecretString::from(self.password.text().as_str()));
        let keyring_action =
            KeyringAction::decide(self.save_password, typed.is_some(), site.password_saved);
        let name = site.display_name();
        let (site_id, label) = (site.id.clone(), session::keyring_label(&site));

        let index = match self.selection {
            Selection::Saved(index) if index < self.book.sites.len() => {
                self.book.sites[index] = site;
                index
            }
            Selection::Saved(_) | Selection::Nothing | Selection::NewSite => {
                self.book.sites.push(site);
                self.book.sites.len() - 1
            }
        };
        self.write_sites(&format!("saved site {name}"));
        self.select(Selection::Saved(index));

        match (keyring_action, typed) {
            (KeyringAction::Store, Some(password)) => {
                self.status = String::from("Saving password to the keyring...");
                sender.oneshot_command(async move {
                    let result = secrets::store_password(&site_id, &label, &password).await;
                    LoginCmd::PasswordChanged {
                        site_id,
                        saved: true,
                        unsaved: result.is_err().then_some(password),
                        result: result.map_err(|error| error.to_string()),
                    }
                });
            }
            (KeyringAction::Remove, _) => {
                sender.oneshot_command(async move {
                    let result = secrets::delete_password(&site_id).await;
                    LoginCmd::PasswordChanged {
                        site_id,
                        saved: false,
                        result: result.map_err(|error| error.to_string()),
                        unsaved: None,
                    }
                });
            }
            (KeyringAction::Store, None) | (KeyringAction::Keep, _) => {}
        }
    }

    /// Remove the picked saved site, its keyring entry, and write all sites to disk.
    ///
    /// # Arguments
    ///
    /// * `sender` - Used to remove the keyring entry
    fn delete(&mut self, sender: &ComponentSender<Self>) {
        let Selection::Saved(index) = self.selection else {
            return;
        };
        if index >= self.book.sites.len() || self.editing {
            return;
        }
        let site = self.book.sites.remove(index);
        self.write_sites(&format!("deleted site {}", site.display_name()));
        if site.ssh.passphrase_saved {
            let entry = session::passphrase_entry(&site.id);
            sender.oneshot_command(async move {
                LoginCmd::PasswordForgotten(
                    secrets::delete_password(&entry)
                        .await
                        .map_err(|error| error.to_string()),
                )
            });
        }
        if site.password_saved {
            sender.oneshot_command(async move {
                LoginCmd::PasswordForgotten(
                    secrets::delete_password(&site.id)
                        .await
                        .map_err(|error| error.to_string()),
                )
            });
        }
        self.status.clear();
        self.select(Selection::Nothing);
    }

    /// Persist the site book, logging `done` on success.
    ///
    /// # Arguments
    ///
    /// * `done` - What was changed, for the log
    fn write_sites(&mut self, done: &str) {
        match self.book.save(&self.sites_path) {
            Ok(()) => tracing::info!("{done}"),
            Err(error) => {
                tracing::error!("cannot save sites: {error}");
                self.status = format!("Cannot save sites: {error}");
            }
        }
    }

    /// Open a file chooser for the private key file, starting in `~/.ssh`.
    ///
    /// # Arguments
    ///
    /// * `root` - The dialog window
    /// * `sender` - Receives the chosen file
    fn choose_key_file(&mut self, root: &gtk::Window, sender: &ComponentSender<Self>) {
        let chooser = gtk::FileChooserNative::new(
            Some("Private Key File"),
            Some(root),
            gtk::FileChooserAction::Open,
            Some("_Choose"),
            Some("_Cancel"),
        );
        if let Some(ssh) = std::env::home_dir().map(|home| home.join(".ssh"))
            && ssh.is_dir()
        {
            let _ = chooser.set_current_folder(Some(&gtk::gio::File::for_path(ssh)));
        }
        let sender = sender.clone();
        chooser.connect_response(move |chooser, response| {
            let path = (response == gtk::ResponseType::Accept)
                .then(|| chooser.file().and_then(|file| file.path()))
                .flatten();
            sender.input(path.map_or(LoginMsg::Abandoned, LoginMsg::KeyFileChosen));
        });
        chooser.show();
        self.chooser = Some(chooser);
    }

    /// Open a native file chooser over the dialog.
    ///
    /// # Arguments
    ///
    /// * `root` - The dialog window
    /// * `exporting` - Save dialog for export, open dialog for import
    /// * `sender` - Receives the chosen file
    fn choose_file(&mut self, root: &gtk::Window, exporting: bool, sender: &ComponentSender<Self>) {
        let chooser = gtk::FileChooserNative::new(
            Some(if exporting {
                "Export Sites"
            } else {
                "Import Sites"
            }),
            Some(root),
            if exporting {
                gtk::FileChooserAction::Save
            } else {
                gtk::FileChooserAction::Open
            },
            Some(if exporting { "_Export" } else { "_Import" }),
            Some("_Cancel"),
        );
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("Site exports (*.json)"));
        filter.add_pattern("*.json");
        chooser.add_filter(&filter);
        if exporting {
            chooser.set_current_name(EXPORT_FILE_NAME);
        }
        let sender = sender.clone();
        chooser.connect_response(move |chooser, response| {
            let path = (response == gtk::ResponseType::Accept)
                .then(|| chooser.file().and_then(|file| file.path()))
                .flatten();
            sender.input(match path {
                Some(path) if exporting => LoginMsg::ExportTo(path),
                Some(path) => LoginMsg::ImportFrom(path),
                None => LoginMsg::Abandoned,
            });
        });
        chooser.show();
        self.chooser = Some(chooser);
    }
}

#[relm4::component(pub)]
impl Component for LoginDialog {
    type Init = LoginInit;
    type Input = LoginMsg;
    type Output = LoginOutput;
    type CommandOutput = LoginCmd;

    view! {
        gtk::Window {
            set_title: Some("Login"),
            set_modal: true,
            // Closing only hides the dialog, so it can be shown again from the menu.
            set_hide_on_close: true,
            set_default_size: (760, 500),

            gtk::Box {
                set_orientation: gtk::Orientation::Vertical,
                set_spacing: 12,
                set_margin_all: 12,

                // The two ways to connect, as tabs: Direct (this computer and a server)
                // and Site to site (two servers).
                #[name = "tab_switcher"]
                gtk::StackSwitcher {
                    add_css_class: "login-tabs",
                    set_halign: gtk::Align::Start,
                    #[watch]
                    set_sensitive: !model.busy && !model.editing,
                },
                // Each tab's page sits in one outlined area under the tab buttons, so the
                // buttons and what they show read as one group (see style.css).
                #[name = "tabs"]
                gtk::Stack {
                    set_vexpand: true,
                    add_css_class: "login-tabs-page",
                    set_transition_type: gtk::StackTransitionType::None,

                        add_child = &gtk::Box {
                            set_orientation: gtk::Orientation::Horizontal,
                            set_spacing: 14,
                            set_vexpand: true,

                            // Left: saved sites, with the buttons that act on the picked one.
                            gtk::Box {
                                set_orientation: gtk::Orientation::Vertical,
                                set_spacing: 4,

                                gtk::ScrolledWindow {
                                    set_width_request: 230,
                                    set_vexpand: true,
                                    set_hscrollbar_policy: gtk::PolicyType::Never,
                                    add_css_class: "frame",
                                    // Finish with Save or Cancel before picking another entry.
                                    #[watch]
                                    set_sensitive: !model.editing && !model.busy,

                                    #[local_ref]
                                    site_list_box -> gtk::ListBox {},
                                },
                                gtk::Box {
                                    set_orientation: gtk::Orientation::Horizontal,
                                    set_spacing: 4,
                                    set_homogeneous: true,

                                    gtk::Button {
                                        set_label: "Edit",
                                        #[watch]
                                        set_sensitive: model.saved_site_idle(),
                                        connect_clicked => LoginMsg::Edit,
                                    },
                                    gtk::Button {
                                        set_label: "Delete",
                                        add_css_class: "destructive-action",
                                        #[watch]
                                        set_sensitive: model.saved_site_idle(),
                                        connect_clicked => LoginMsg::Delete,
                                    },
                                },
                            },

                            // Right: the session details and their Save / Cancel buttons, grouped
                            // in a dashed outline (drawn by the "session-group" style). It fills
                            // the height of the site list column so the two sides line up, with
                            // Save / Cancel level with Edit / Delete.
                            gtk::Box {
                                set_orientation: gtk::Orientation::Vertical,
                                set_spacing: 8,
                                set_hexpand: true,
                                add_css_class: "session-group",

                                gtk::Label {
                                    set_label: "Session",
                                    set_xalign: 0.0,
                                    add_css_class: "heading",
                                },

                                // File protocol.
                                gtk::Box {
                                    set_orientation: gtk::Orientation::Vertical,
                                    set_spacing: 2,

                                    gtk::Label {
                                        set_label: "File protocol:",
                                        set_xalign: 0.0,
                                    },
                                    gtk::Box {
                                        set_orientation: gtk::Orientation::Horizontal,
                                        set_spacing: 8,

                                        gtk::DropDown::from_strings(&protocol_labels) {
                                            set_halign: gtk::Align::Start,
                                            // Read-only until "New Site" or Edit.
                                            #[watch]
                                            set_sensitive: model.editing,
                                            #[watch]
                                            set_selected: model.protocol.index(),
                                            connect_selected_notify[sender] => move |dropdown| {
                                                sender.input(LoginMsg::ProtocolSelected(dropdown.selected()));
                                            },
                                        },
                                        // FTP only: encryption and passive mode.
                                        gtk::DropDown::from_strings(&encryption_labels) {
                                            set_tooltip_text: Some("How the FTP connection is encrypted"),
                                            #[watch]
                                            set_visible: model.protocol == Protocol::Ftp,
                                            #[watch]
                                            set_sensitive: model.editing,
                                            #[watch]
                                            set_selected: model.ftp.encryption.index(),
                                            connect_selected_notify[sender] => move |dropdown| {
                                                sender.input(LoginMsg::EncryptionSelected(dropdown.selected()));
                                            },
                                        },
                                        gtk::CheckButton::with_label("Passive mode") {
                                            set_tooltip_text: Some(
                                                "The client opens the data connections (works through \
                                                 firewalls and NAT). Untick for active mode.",
                                            ),
                                            #[watch]
                                            set_visible: model.protocol == Protocol::Ftp,
                                            #[watch]
                                            set_sensitive: model.editing,
                                            #[watch]
                                            set_active: model.ftp.passive,
                                            connect_toggled[sender] => move |check| {
                                                sender.input(LoginMsg::PassiveToggled(check.is_active()));
                                            },
                                        },
                                    },
                                },

                                // Host name (wide) and port number (short).
                                gtk::Box {
                                    set_orientation: gtk::Orientation::Horizontal,
                                    set_spacing: 12,

                                    gtk::Box {
                                        set_orientation: gtk::Orientation::Vertical,
                                        set_spacing: 2,
                                        set_hexpand: true,

                                        gtk::Label {
                                            set_label: "Host name:",
                                            set_xalign: 0.0,
                                        },
                                        gtk::Entry::with_buffer(&model.host) {
                                            #[watch]
                                            set_sensitive: model.editing,
                                            connect_changed => LoginMsg::FormEdited,
                                        },
                                    },
                                    gtk::Box {
                                        set_orientation: gtk::Orientation::Vertical,
                                        set_spacing: 2,

                                        gtk::Label {
                                            set_label: "Port:",
                                            set_xalign: 0.0,
                                        },
                                        gtk::SpinButton::with_range(1.0, 65535.0, 1.0) {
                                            set_digits: 0,
                                            // Room for five digits ("65535") and no more.
                                            set_width_chars: 5,
                                            add_css_class: "port",
                                            #[watch]
                                            set_sensitive: model.editing,
                                            #[watch]
                                            set_value: f64::from(model.port),
                                            connect_value_changed[sender] => move |spin| {
                                                sender.input(LoginMsg::PortChanged(spin.value_as_int()));
                                            },
                                        },
                                    },
                                },

                                // User name and password, side by side at equal widths.
                                gtk::Box {
                                    set_orientation: gtk::Orientation::Horizontal,
                                    set_spacing: 12,
                                    set_homogeneous: true,

                                    gtk::Box {
                                        set_orientation: gtk::Orientation::Vertical,
                                        set_spacing: 2,

                                        gtk::Label {
                                            set_label: "User name:",
                                            set_xalign: 0.0,
                                        },
                                        gtk::Entry::with_buffer(&model.user) {
                                            #[watch]
                                            set_sensitive: model.editing,
                                        },
                                    },
                                    gtk::Box {
                                        set_orientation: gtk::Orientation::Vertical,
                                        set_spacing: 2,

                                        gtk::Label {
                                            set_label: "Password:",
                                            set_xalign: 0.0,
                                        },
                                        gtk::Entry::with_buffer(&model.password) {
                                            set_visibility: false,
                                            set_input_purpose: gtk::InputPurpose::Password,
                                            // Usable for a picked saved site too, to log in with a
                                            // password that is not saved.
                                            #[watch]
                                            set_sensitive: model.editing || model.selected_site().is_some(),
                                            #[watch]
                                            set_placeholder_text: Some(model.password_placeholder()),
                                            // Enter in the password field logs in.
                                            connect_activate => LoginMsg::Login,
                                            set_secondary_icon_name: Some(SHOW_PASSWORD_ICON),
                                            set_secondary_icon_tooltip_text: Some("Show password"),
                                            connect_icon_press => move |entry, _| {
                                                // Spelled out because `Widget` also has an `is_visible`.
                                                let show = !EntryExt::is_visible(entry);
                                                entry.set_visibility(show);
                                                entry.set_secondary_icon_name(Some(if show {
                                                    HIDE_PASSWORD_ICON
                                                } else {
                                                    SHOW_PASSWORD_ICON
                                                }));
                                            },
                                        },
                                        gtk::CheckButton {
                                            set_label: Some("Save password"),
                                            #[watch]
                                            set_sensitive: model.editing,
                                            set_tooltip_text: Some(
                                                "Keep the password in the desktop keyring (encrypted \
                                                 with your login password)"
                                            ),
                                            #[watch]
                                            set_active: model.save_password,
                                            connect_toggled[sender] => move |check| {
                                                sender.input(LoginMsg::SavePasswordToggled(check.is_active()));
                                            },
                                        },
                                    },
                                },

                                // SSH keys (SFTP and SCP): a private key file, tried first, and the
                                // keys of the desktop's SSH agent; the password comes after them.
                                gtk::Box {
                                    set_orientation: gtk::Orientation::Vertical,
                                    set_spacing: 2,
                                    #[watch]
                                    set_visible: model.protocol != Protocol::Ftp,

                                    gtk::Label {
                                        set_label: "Private key file:",
                                        set_xalign: 0.0,
                                    },
                                    gtk::Box {
                                        set_orientation: gtk::Orientation::Horizontal,
                                        set_spacing: 8,

                                        gtk::Entry::with_buffer(&model.key_file) {
                                            set_hexpand: true,
                                            set_placeholder_text: Some("None (OpenSSH or PuTTY .ppk key)"),
                                            #[watch]
                                            set_sensitive: model.editing,
                                            connect_changed => LoginMsg::FormEdited,
                                        },
                                        gtk::Button {
                                            set_label: "Browse...",
                                            set_tooltip_text: Some("Choose a private key file"),
                                            #[watch]
                                            set_sensitive: model.editing,
                                            connect_clicked => LoginMsg::BrowseKeyFile,
                                        },
                                        gtk::CheckButton {
                                            set_label: Some("Use SSH agent"),
                                            set_tooltip_text: Some(
                                                "Also try the keys held by the desktop's SSH agent \
                                                 (as ssh does)"
                                            ),
                                            #[watch]
                                            set_sensitive: model.editing,
                                            #[watch]
                                            set_active: model.use_agent,
                                            connect_toggled[sender] => move |check| {
                                                sender.input(LoginMsg::UseAgentToggled(check.is_active()));
                                            },
                                        },
                                    },
                                },

                                // Saved-password indicator (left; stays readable while the fields
                                // are disabled) and Save / Cancel (right), along the bottom edge.
                                gtk::Box {
                                    set_orientation: gtk::Orientation::Horizontal,
                                    set_spacing: 4,
                                    set_vexpand: true,
                                    set_valign: gtk::Align::End,

                                    gtk::Box {
                                        set_orientation: gtk::Orientation::Horizontal,
                                        set_spacing: 4,
                                        set_hexpand: true,
                                        #[watch]
                                        set_visible: model.selected_site().is_some(),

                                        gtk::Image {
                                            set_icon_name: Some(PASSWORD_ICON),
                                        },
                                        gtk::Label {
                                            set_xalign: 0.0,
                                            #[watch]
                                            set_label: model.password_status(),
                                        },
                                    },
                                    // Keeps the buttons right-aligned when the indicator is hidden.
                                    gtk::Box {
                                        set_hexpand: true,
                                        #[watch]
                                        set_visible: model.selected_site().is_none(),
                                    },

                                    gtk::Button {
                                        set_label: "Save",
                                        #[watch]
                                        set_sensitive: model.editing && model.can_save(),
                                        connect_clicked => LoginMsg::Save,
                                    },
                                    gtk::Button {
                                        set_label: "Cancel",
                                        #[watch]
                                        set_sensitive: model.editing,
                                        connect_clicked => LoginMsg::Cancel,
                                    },
                                },
                            },
                        } -> {
                            set_name: DIRECT_TAB,
                            set_title: "Direct",
                        },

                        // Site to site: a saved site for each pane; copies between them
                        // pass through this computer.
                        add_child = &gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,
                            set_spacing: 8,

                            gtk::Label {
                                set_label: "Copies between the two servers are transferred through \
                                            this computer. Pick a saved site for each connection, \
                                            then Login.",
                                set_xalign: 0.0,
                                set_wrap: true,
                                add_css_class: "dim-label",
                            },
                            gtk::Box {
                                set_orientation: gtk::Orientation::Horizontal,
                                set_spacing: 14,
                                set_homogeneous: true,
                                set_vexpand: true,

                            // The left pane's site: a list of saved sites, and a summary
                            // of the picked one.
                            gtk::Box {
                                set_orientation: gtk::Orientation::Vertical,
                                set_spacing: 4,
                                set_hexpand: true,

                                gtk::Label {
                                    set_label: "Connection 1",
                                    set_tooltip_text: Some("Shown in the left pane"),
                                    set_xalign: 0.0,
                                    add_css_class: "heading",
                                },
                                // A short list: room below for the picked site's details.
                                gtk::ScrolledWindow {
                                    set_height_request: 150,
                                    set_hscrollbar_policy: gtk::PolicyType::Never,
                                    add_css_class: "frame",
                                    #[watch]
                                    set_sensitive: !model.busy,

                                    #[local_ref]
                                    left_list_box -> gtk::ListBox {},
                                },
                                // The picked site's details.
                                gtk::Grid {
                                    set_column_spacing: 10,
                                    set_row_spacing: 3,
                                    set_vexpand: true,
                                    set_valign: gtk::Align::Start,
                                    add_css_class: "site-details",
                                    attach[0, 0, 1, 1] = &gtk::Label {
                                        set_label: "Protocol:",
                                        set_xalign: 0.0,
                                        add_css_class: "dim-label",
                                    },
                                    attach[1, 0, 1, 1] = &gtk::Label {
                                        set_xalign: 0.0,
                                        set_hexpand: true,
                                        set_ellipsize: gtk::pango::EllipsizeMode::End,
                                        #[watch]
                                        set_label: &model.pair_detail(0, PairDetail::Protocol),
                                        #[watch]
                                        set_tooltip_text: Some(&model.pair_detail(0, PairDetail::Protocol)),
                                    },
                                    attach[0, 1, 1, 1] = &gtk::Label {
                                        set_label: "Address:",
                                        set_xalign: 0.0,
                                        add_css_class: "dim-label",
                                    },
                                    attach[1, 1, 1, 1] = &gtk::Label {
                                        set_xalign: 0.0,
                                        set_hexpand: true,
                                        set_ellipsize: gtk::pango::EllipsizeMode::End,
                                        #[watch]
                                        set_label: &model.pair_detail(0, PairDetail::Address),
                                        #[watch]
                                        set_tooltip_text: Some(&model.pair_detail(0, PairDetail::Address)),
                                    },
                                    attach[0, 2, 1, 1] = &gtk::Label {
                                        set_label: "User:",
                                        set_xalign: 0.0,
                                        add_css_class: "dim-label",
                                    },
                                    attach[1, 2, 1, 1] = &gtk::Label {
                                        set_xalign: 0.0,
                                        set_hexpand: true,
                                        set_ellipsize: gtk::pango::EllipsizeMode::End,
                                        #[watch]
                                        set_label: &model.pair_detail(0, PairDetail::User),
                                        #[watch]
                                        set_tooltip_text: Some(&model.pair_detail(0, PairDetail::User)),
                                    },
                                    attach[0, 3, 1, 1] = &gtk::Label {
                                        set_label: "Login:",
                                        set_xalign: 0.0,
                                        add_css_class: "dim-label",
                                    },
                                    attach[1, 3, 1, 1] = &gtk::Label {
                                        set_xalign: 0.0,
                                        set_hexpand: true,
                                        set_ellipsize: gtk::pango::EllipsizeMode::End,
                                        #[watch]
                                        set_label: &model.pair_detail(0, PairDetail::Login),
                                        #[watch]
                                        set_tooltip_text: Some(&model.pair_detail(0, PairDetail::Login)),
                                    },
                                },
                            },

                            // The right pane's site: a list of saved sites, and a summary
                            // of the picked one.
                            gtk::Box {
                                set_orientation: gtk::Orientation::Vertical,
                                set_spacing: 4,
                                set_hexpand: true,

                                gtk::Label {
                                    set_label: "Connection 2",
                                    set_tooltip_text: Some("Shown in the right pane"),
                                    set_xalign: 0.0,
                                    add_css_class: "heading",
                                },
                                // A short list: room below for the picked site's details.
                                gtk::ScrolledWindow {
                                    set_height_request: 150,
                                    set_hscrollbar_policy: gtk::PolicyType::Never,
                                    add_css_class: "frame",
                                    #[watch]
                                    set_sensitive: !model.busy,

                                    #[local_ref]
                                    right_list_box -> gtk::ListBox {},
                                },
                                // The picked site's details.
                                gtk::Grid {
                                    set_column_spacing: 10,
                                    set_row_spacing: 3,
                                    set_vexpand: true,
                                    set_valign: gtk::Align::Start,
                                    add_css_class: "site-details",
                                    attach[0, 0, 1, 1] = &gtk::Label {
                                        set_label: "Protocol:",
                                        set_xalign: 0.0,
                                        add_css_class: "dim-label",
                                    },
                                    attach[1, 0, 1, 1] = &gtk::Label {
                                        set_xalign: 0.0,
                                        set_hexpand: true,
                                        set_ellipsize: gtk::pango::EllipsizeMode::End,
                                        #[watch]
                                        set_label: &model.pair_detail(1, PairDetail::Protocol),
                                        #[watch]
                                        set_tooltip_text: Some(&model.pair_detail(1, PairDetail::Protocol)),
                                    },
                                    attach[0, 1, 1, 1] = &gtk::Label {
                                        set_label: "Address:",
                                        set_xalign: 0.0,
                                        add_css_class: "dim-label",
                                    },
                                    attach[1, 1, 1, 1] = &gtk::Label {
                                        set_xalign: 0.0,
                                        set_hexpand: true,
                                        set_ellipsize: gtk::pango::EllipsizeMode::End,
                                        #[watch]
                                        set_label: &model.pair_detail(1, PairDetail::Address),
                                        #[watch]
                                        set_tooltip_text: Some(&model.pair_detail(1, PairDetail::Address)),
                                    },
                                    attach[0, 2, 1, 1] = &gtk::Label {
                                        set_label: "User:",
                                        set_xalign: 0.0,
                                        add_css_class: "dim-label",
                                    },
                                    attach[1, 2, 1, 1] = &gtk::Label {
                                        set_xalign: 0.0,
                                        set_hexpand: true,
                                        set_ellipsize: gtk::pango::EllipsizeMode::End,
                                        #[watch]
                                        set_label: &model.pair_detail(1, PairDetail::User),
                                        #[watch]
                                        set_tooltip_text: Some(&model.pair_detail(1, PairDetail::User)),
                                    },
                                    attach[0, 3, 1, 1] = &gtk::Label {
                                        set_label: "Login:",
                                        set_xalign: 0.0,
                                        add_css_class: "dim-label",
                                    },
                                    attach[1, 3, 1, 1] = &gtk::Label {
                                        set_xalign: 0.0,
                                        set_hexpand: true,
                                        set_ellipsize: gtk::pango::EllipsizeMode::End,
                                        #[watch]
                                        set_label: &model.pair_detail(1, PairDetail::Login),
                                        #[watch]
                                        set_tooltip_text: Some(&model.pair_detail(1, PairDetail::Login)),
                                    },
                                },
                            },
                            },
                        } -> {
                            set_name: SITE_TO_SITE_TAB,
                            set_title: "Site to site",
                        },

                        // Unfinished: transfers interrupted earlier, to resume (logging in
                        // to their site and opening it where they were) or give up.
                        add_child = &gtk::Box {
                            set_orientation: gtk::Orientation::Vertical,
                            set_spacing: 8,

                            gtk::Label {
                                set_label: "Transfers that did not finish. Tick the ones to resume: \
                                            ShuttleSCP logs in to their site with its saved password \
                                            or keys and opens it where they were. Discard deletes \
                                            their partial files instead.",
                                set_xalign: 0.0,
                                set_wrap: true,
                                add_css_class: "dim-label",
                            },
                            gtk::ScrolledWindow {
                                set_vexpand: true,
                                set_hscrollbar_policy: gtk::PolicyType::Never,
                                add_css_class: "frame",
                                #[watch]
                                set_sensitive: !model.busy,

                                #[local_ref]
                                unfinished_list_box -> gtk::ListBox {},
                            },
                            // The picked transfer's details.
                            gtk::Grid {
                                set_column_spacing: 10,
                                set_row_spacing: 3,
                                add_css_class: "site-details",
                                attach[0, 0, 1, 1] = &gtk::Label {
                                    set_label: "Transfer:",
                                    set_xalign: 0.0,
                                    add_css_class: "dim-label",
                                },
                                attach[1, 0, 1, 1] = &gtk::Label {
                                    set_xalign: 0.0,
                                    set_hexpand: true,
                                    set_ellipsize: gtk::pango::EllipsizeMode::Middle,
                                    #[watch]
                                    set_label: &model.unfinished_detail(UnfinishedDetail::What),
                                    #[watch]
                                    set_tooltip_text: Some(&model.unfinished_detail(UnfinishedDetail::What)),
                                },
                                attach[0, 1, 1, 1] = &gtk::Label {
                                    set_label: "From:",
                                    set_xalign: 0.0,
                                    add_css_class: "dim-label",
                                },
                                attach[1, 1, 1, 1] = &gtk::Label {
                                    set_xalign: 0.0,
                                    set_hexpand: true,
                                    set_ellipsize: gtk::pango::EllipsizeMode::Middle,
                                    #[watch]
                                    set_label: &model.unfinished_detail(UnfinishedDetail::From),
                                    #[watch]
                                    set_tooltip_text: Some(&model.unfinished_detail(UnfinishedDetail::From)),
                                },
                                attach[0, 2, 1, 1] = &gtk::Label {
                                    set_label: "To:",
                                    set_xalign: 0.0,
                                    add_css_class: "dim-label",
                                },
                                attach[1, 2, 1, 1] = &gtk::Label {
                                    set_xalign: 0.0,
                                    set_hexpand: true,
                                    set_ellipsize: gtk::pango::EllipsizeMode::Middle,
                                    #[watch]
                                    set_label: &model.unfinished_detail(UnfinishedDetail::To),
                                    #[watch]
                                    set_tooltip_text: Some(&model.unfinished_detail(UnfinishedDetail::To)),
                                },
                                attach[0, 3, 1, 1] = &gtk::Label {
                                    set_label: "Saved:",
                                    set_xalign: 0.0,
                                    add_css_class: "dim-label",
                                },
                                attach[1, 3, 1, 1] = &gtk::Label {
                                    set_xalign: 0.0,
                                    set_hexpand: true,
                                    #[watch]
                                    set_label: &model.unfinished_detail(UnfinishedDetail::Saved),
                                },
                            },
                            gtk::Box {
                                set_orientation: gtk::Orientation::Horizontal,
                                set_spacing: 6,
                                set_halign: gtk::Align::End,

                                gtk::Button {
                                    set_label: "Discard",
                                    add_css_class: "destructive-action",
                                    set_tooltip_text: Some("Log in, delete the ticked transfers' partial files, and forget them"),
                                    #[watch]
                                    set_sensitive: !model.busy && !model.unfinished_ticked.is_empty(),
                                    connect_clicked => LoginMsg::DiscardUnfinished,
                                },
                                gtk::Button {
                                    set_label: "Resume",
                                    add_css_class: "suggested-action",
                                    set_tooltip_text: Some("Log in and continue the ticked transfers where they stopped"),
                                    #[watch]
                                    set_sensitive: !model.busy && model.unfinished_sites().is_ok(),
                                    connect_clicked => LoginMsg::ResumeUnfinished,
                                },
                            },
                        } -> {
                            set_name: UNFINISHED_TAB,
                            // With the count once there are any (`show_unfinished_count`).
                            set_title: "Unfinished",
                        },
                },

                gtk::Box {
                    set_orientation: gtk::Orientation::Horizontal,
                    set_spacing: 6,

                    gtk::CheckButton {
                        set_label: Some("Show at startup"),
                        set_tooltip_text: Some("Open this dialog when ShuttleSCP starts"),
                        set_margin_end: 6,
                        #[watch]
                        set_active: model.show_at_startup,
                        connect_toggled[sender] => move |check| {
                            sender.input(LoginMsg::ShowAtStartupToggled(check.is_active()));
                        },
                    },
                    gtk::Button {
                        set_label: "Import...",
                        set_tooltip_text: Some("Add sites from a ShuttleSCP export file"),
                        #[watch]
                        set_sensitive: !model.editing && !model.busy,
                        connect_clicked => LoginMsg::ImportClicked,
                    },
                    gtk::Button {
                        set_label: "Export...",
                        set_tooltip_text: Some("Save all sites to a file for another computer"),
                        #[watch]
                        set_sensitive: !model.editing && !model.busy && !model.book.sites.is_empty(),
                        connect_clicked => LoginMsg::ExportClicked,
                    },
                    gtk::Label {
                        set_hexpand: true,
                        set_xalign: 0.0,
                        set_margin_start: 6,
                        set_ellipsize: gtk::pango::EllipsizeMode::End,
                        add_css_class: "dim-label",
                        #[watch]
                        set_label: &model.status,
                        #[watch]
                        set_tooltip_text: Some(&model.status),
                    },
                    gtk::Button {
                        set_label: "Login",
                        add_css_class: "suggested-action",
                        set_tooltip_text: Some("Connect to the selected site"),
                        #[watch]
                        set_sensitive: model.can_login(),
                        connect_clicked => LoginMsg::Login,
                    },
                    gtk::Button {
                        set_label: "Close",
                        connect_clicked => LoginMsg::Close,
                    },
                },
            },
        }
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let LoginInit {
            sites_path,
            show_at_startup,
            pending,
        } = init;
        let book = SiteBook::load(&sites_path).unwrap_or_else(|error| {
            tracing::warn!("{error}; starting with no saved sites");
            SiteBook::default()
        });
        tracing::debug!("loaded {} saved sites", book.sites.len());

        let click_sender = sender.clone();
        let double_click_sender = sender.clone();
        let site_list = SiteList::new(
            true,
            move |selection| click_sender.input(LoginMsg::SiteClicked(selection)),
            move |selection| double_click_sender.input(LoginMsg::SiteDoubleClicked(selection)),
        );
        let pair_list = |pane: usize| {
            let sender = sender.clone();
            SiteList::new(
                false,
                move |selection| sender.input(LoginMsg::PairSitePicked { pane, selection }),
                |_| {},
            )
        };
        let pair_lists = [pair_list(0), pair_list(1)];
        // Clicking a line shows its details; its box ticks it for Resume or Discard.
        let unfinished_list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .build();
        let nothing = gtk::Label::new(Some("No unfinished transfers"));
        nothing.add_css_class("dim-label");
        unfinished_list.set_placeholder(Some(&nothing));
        {
            let sender = sender.clone();
            unfinished_list.connect_row_selected(move |_, row| {
                sender.input(LoginMsg::UnfinishedPicked(
                    row.map(|row| row.widget_name().to_string()),
                ));
            });
        }

        let mut model = Self {
            book,
            sites_path,
            selection: Selection::Nothing,
            editing: false,
            protocol: Protocol::default(),
            port: Protocol::default().default_port(),
            ftp: FtpOptions::default(),
            save_password: true,
            host: gtk::EntryBuffer::default(),
            user: gtk::EntryBuffer::default(),
            password: gtk::EntryBuffer::default(),
            key_file: gtk::EntryBuffer::default(),
            use_agent: true,
            pending_passphrase: None,
            site_list,
            status: String::new(),
            busy: false,
            chooser: None,
            show_at_startup,
            tab: LoginTab::Direct,
            pair_lists,
            pair_picks: [None, None],
            group_login: None,
            pending,
            unfinished: Vec::new(),
            unfinished_ticked: HashSet::new(),
            unfinished_picked: None,
            unfinished_list,
            tabs: None,
        };
        model.select(Selection::Nothing);
        model.fill_pair_lists();
        model.refresh_unfinished(&sender);

        let protocol_labels: Vec<&str> = Protocol::ALL.iter().map(|p| p.label()).collect();
        let encryption_labels: Vec<&str> = FtpEncryption::ALL.iter().map(|e| e.label()).collect();
        let site_list_box = model.site_list.widget();
        let left_list_box = model.pair_lists[0].widget();
        let right_list_box = model.pair_lists[1].widget();
        let unfinished_list_box = &model.unfinished_list;
        let widgets = view_output!();
        widgets.tab_switcher.set_stack(Some(&widgets.tabs));
        model.tabs = Some(widgets.tabs.clone());
        model.show_unfinished_count();
        {
            let sender = sender.clone();
            widgets
                .tabs
                .connect_visible_child_name_notify(move |stack| {
                    sender.input(LoginMsg::TabShown(
                        match stack.visible_child_name().as_deref() {
                            Some(SITE_TO_SITE_TAB) => LoginTab::SiteToSite,
                            Some(UNFINISHED_TAB) => LoginTab::Unfinished,
                            _ => LoginTab::Direct,
                        },
                    ));
                });
        }
        // The same title bar and compact controls as the other dialogs.
        dialog::style_window(&root, "Login");
        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, sender: ComponentSender<Self>, root: &Self::Root) {
        match message {
            LoginMsg::Show => {
                // Reopen without half-finished edits: keep a picked saved site, drop the rest.
                let keep = match self.selection {
                    Selection::Saved(_) => self.selection,
                    Selection::Nothing | Selection::NewSite => Selection::Nothing,
                };
                self.select(keep);
                self.group_login = None;
                // Always open on Direct, the usual way in.
                if let Some(tabs) = &self.tabs {
                    tabs.set_visible_child_name(DIRECT_TAB);
                }
                self.tab = LoginTab::Direct;
                self.fill_pair_lists();
                self.refresh_unfinished(&sender);
                self.status.clear();
                root.present();
            }
            LoginMsg::Close => root.close(),
            LoginMsg::SiteDoubleClicked(selection @ Selection::Saved(_)) => {
                // The first click of the double-click has already picked the site; this
                // only makes sure, then logs in as the Login button would.
                if !self.editing && !self.busy {
                    if self.selection != selection {
                        self.select(selection);
                    }
                    self.start_login(&sender, None);
                }
            }
            // Double-clicking "New Site" needs nothing beyond its single click.
            LoginMsg::SiteDoubleClicked(Selection::NewSite | Selection::Nothing) => {}
            LoginMsg::SiteClicked(selection) => {
                if !self.editing && !self.busy {
                    self.select(selection);
                }
            }
            LoginMsg::Edit => {
                if self.saved_site_idle() {
                    self.editing = true;
                }
            }
            LoginMsg::Delete => {
                if !self.saved_site_idle() {
                    return;
                }
                let Some(site) = self.selected_site() else {
                    return;
                };
                let question = format!("Delete {}?", site.display_name());
                let detail = if site.password_saved {
                    "The site and its saved password (in the keyring) will be removed. \
                     This cannot be undone."
                } else {
                    "The site will be removed. This cannot be undone."
                };
                let wording = Confirmation {
                    title: "Delete Site",
                    question: &question,
                    detail,
                    confirm_label: "Delete",
                };
                let sender = sender.clone();
                confirm_dialog::ask(root.upcast_ref(), wording, move |confirmed| {
                    if confirmed {
                        sender.input(LoginMsg::DeleteConfirmed);
                    }
                });
            }
            LoginMsg::DeleteConfirmed => self.delete(&sender),
            LoginMsg::Save => self.save(&sender),
            LoginMsg::Cancel => {
                // A new site is abandoned entirely; a saved one goes back to its details.
                let back_to = match self.selection {
                    Selection::NewSite => Selection::Nothing,
                    Selection::Nothing | Selection::Saved(_) => self.selection,
                };
                self.select(back_to);
            }
            LoginMsg::ProtocolSelected(index) => {
                if let Some(protocol) = Protocol::from_index(index)
                    && protocol != self.protocol
                {
                    // Follow the protocol's usual port unless a custom one was entered.
                    if self.port == self.default_port() {
                        self.port = default_port(protocol, self.ftp.encryption);
                    }
                    self.protocol = protocol;
                }
            }
            LoginMsg::EncryptionSelected(index) => {
                if let Some(encryption) = FtpEncryption::from_index(index)
                    && encryption != self.ftp.encryption
                {
                    // Implicit TLS has its own usual port (990).
                    if self.port == self.default_port() {
                        self.port = encryption.default_port();
                    }
                    self.ftp.encryption = encryption;
                }
            }
            LoginMsg::PassiveToggled(passive) => self.ftp.passive = passive,
            LoginMsg::PortChanged(value) => {
                if let Ok(port) = u16::try_from(value) {
                    self.port = port;
                }
            }
            LoginMsg::SavePasswordToggled(active) => self.save_password = active,
            LoginMsg::BrowseKeyFile => self.choose_key_file(root.upcast_ref(), &sender),
            LoginMsg::KeyFileChosen(path) => {
                self.chooser = None;
                self.key_file.set_text(path.display().to_string());
            }
            LoginMsg::UseAgentToggled(active) => self.use_agent = active,
            LoginMsg::KeyPassphraseGiven(answer) => {
                self.pending_passphrase = Some(answer);
                self.start_login(&sender, None);
            }
            LoginMsg::Login => {
                self.group_login = None;
                self.pending_passphrase = None;
                if self.tab == LoginTab::SiteToSite
                    && let Some(sites) = self.pair_sites()
                {
                    tracing::info!(
                        "connecting site to site: {} and {}",
                        sites[0].display_name(),
                        sites[1].display_name()
                    );
                    self.group_login = Some(GroupLogin {
                        sites: sites.to_vec(),
                        opened: Vec::new(),
                        then: AfterLogin::Open,
                    });
                }
                self.start_login(&sender, None);
            }
            LoginMsg::TabShown(tab) => {
                if !self.busy {
                    self.tab = tab;
                    self.status.clear();
                    // Sites may have been added, changed or deleted on Direct, and
                    // sessions may have recorded or finished transfers.
                    match tab {
                        LoginTab::SiteToSite => self.fill_pair_lists(),
                        LoginTab::Unfinished => self.refresh_unfinished(&sender),
                        LoginTab::Direct => {}
                    }
                }
            }
            LoginMsg::PairSitePicked { pane, selection } => {
                if !self.busy
                    && let Selection::Saved(index) = selection
                    && let Some(site) = self.book.sites.get(index)
                {
                    self.pair_picks[pane] = Some(site.id.clone());
                    self.fill_pair_lists();
                }
            }
            LoginMsg::UnfinishedTicked { key, ticked } => {
                if ticked {
                    self.unfinished_ticked.insert(key);
                } else {
                    self.unfinished_ticked.remove(&key);
                }
                self.status.clear();
            }
            LoginMsg::UnfinishedPicked(key) => self.unfinished_picked = key,
            LoginMsg::ResumeUnfinished => self.act_on_unfinished(true, &sender),
            LoginMsg::DiscardUnfinished => self.act_on_unfinished(false, &sender),
            LoginMsg::TrustHostKey(key) => self.start_login(&sender, Some(*key)),
            LoginMsg::TrustCertificate {
                host,
                port,
                fingerprint,
            } => match shuttlescp_core::remote::trust_certificate(&host, port, &fingerprint) {
                Ok(()) => self.start_login(&sender, None),
                Err(error) => {
                    tracing::warn!("cannot remember the certificate: {error}");
                    self.status = error.to_string();
                }
            },
            LoginMsg::ShowAtStartupToggled(show) => {
                if show != self.show_at_startup {
                    self.show_at_startup = show;
                    // Sending fails only if the main window is gone; nothing to do then.
                    let _ = sender.output(LoginOutput::ShowAtStartupChanged(show));
                }
            }
            // Nothing to store: the update itself refreshes the Save button.
            LoginMsg::FormEdited => {}
            LoginMsg::ExportClicked => self.choose_file(root.upcast_ref(), true, &sender),
            LoginMsg::ImportClicked => self.choose_file(root.upcast_ref(), false, &sender),
            LoginMsg::Abandoned => self.chooser = None,
            LoginMsg::ExportTo(path) => {
                self.chooser = None;
                if self.book.sites.iter().any(|site| site.password_saved) {
                    let sender = sender.clone();
                    passphrase_dialog::ask(root, PassphrasePurpose::Export, move |answer| {
                        sender.input(match answer {
                            Some(PassphraseAnswer::Passphrase(passphrase)) => {
                                LoginMsg::ExportConfirmed {
                                    path: path.clone(),
                                    passphrase: Some(passphrase),
                                }
                            }
                            Some(PassphraseAnswer::WithoutPasswords) => LoginMsg::ExportConfirmed {
                                path: path.clone(),
                                passphrase: None,
                            },
                            None => LoginMsg::Abandoned,
                        });
                    });
                } else {
                    sender.input(LoginMsg::ExportConfirmed {
                        path,
                        passphrase: None,
                    });
                }
            }
            LoginMsg::ExportConfirmed { path, passphrase } => {
                self.busy = true;
                self.status = String::from("Exporting...");
                let sites = self.book.sites.clone();
                sender.oneshot_command(async move {
                    LoginCmd::Exported(session::export_to_file(sites, path, passphrase).await)
                });
            }
            LoginMsg::ImportFrom(path) => {
                self.chooser = None;
                match ExportFile::read(&path) {
                    Err(error) => self.status = format!("Import failed: {error}"),
                    Ok(file) if file.has_passwords() => {
                        let sender = sender.clone();
                        passphrase_dialog::ask(root, PassphrasePurpose::Import, move |answer| {
                            sender.input(match answer {
                                Some(PassphraseAnswer::Passphrase(passphrase)) => {
                                    LoginMsg::ImportConfirmed {
                                        file: file.clone(),
                                        passphrase: Some(passphrase),
                                    }
                                }
                                Some(PassphraseAnswer::WithoutPasswords) | None => {
                                    LoginMsg::Abandoned
                                }
                            });
                        });
                    }
                    Ok(file) => sender.input(LoginMsg::ImportConfirmed {
                        file,
                        passphrase: None,
                    }),
                }
            }
            LoginMsg::ImportConfirmed { file, passphrase } => {
                self.busy = true;
                self.status = String::from("Importing...");
                sender.oneshot_command(async move {
                    LoginCmd::Imported(session::import_from_file(file, passphrase).await)
                });
            }
        }
    }

    fn update_cmd(
        &mut self,
        message: Self::CommandOutput,
        sender: ComponentSender<Self>,
        root: &Self::Root,
    ) {
        match message {
            LoginCmd::Discarded {
                keys,
                deleted,
                failed,
            } => {
                self.busy = false;
                self.pending.forget(&keys);
                self.refresh_unfinished(&sender);
                self.status = match failed.first() {
                    Some(reason) => format!("Not discarded: {reason}"),
                    None => format!(
                        "Discarded {}: {deleted} partial {} deleted",
                        match keys.len() {
                            1 => String::from("1 transfer"),
                            count => format!("{count} transfers"),
                        },
                        if deleted == 1 { "file" } else { "files" }
                    ),
                };
            }
            LoginCmd::Connected(result) => {
                self.busy = false;
                match result {
                    Ok((site, remote)) => {
                        self.status.clear();
                        self.password.set_text("");
                        let more_to_open = self
                            .group_login
                            .as_ref()
                            .is_some_and(GroupLogin::more_to_open);
                        // The passphrase opened the key: save it if asked to.
                        if let Some(pending) = self.pending_passphrase.take()
                            && pending.save
                        {
                            let (site_id, entry, label) = (
                                site.id.clone(),
                                session::passphrase_entry(&site.id),
                                session::passphrase_label(&site),
                            );
                            sender.oneshot_command(async move {
                                let result =
                                    secrets::store_password(&entry, &label, &pending.passphrase)
                                        .await;
                                LoginCmd::PassphraseSaved {
                                    site_id,
                                    result: result.map_err(|error| error.to_string()),
                                }
                            });
                        }
                        if more_to_open {
                            // The left site is open: now the right one.
                            if let Some(group) = &mut self.group_login {
                                group.opened.push(remote);
                            }
                            self.start_login(&sender, None);
                            return;
                        }
                        let (left, then) = match self.group_login.take() {
                            Some(GroupLogin {
                                sites,
                                opened,
                                then,
                            }) => (sites.into_iter().zip(opened).next(), then),
                            None => (None, AfterLogin::Open),
                        };
                        let resume = match then {
                            AfterLogin::Open => None,
                            AfterLogin::Resume(resume) => Some(resume),
                            AfterLogin::Discard(jobs) => {
                                let left = left.map(|(_, remote)| remote);
                                self.discard_partials(jobs, left, remote, &sender);
                                return;
                            }
                        };
                        let output = match left {
                            Some(left) => LoginOutput::ConnectedPair {
                                left,
                                right: (site, remote),
                                resume,
                            },
                            None => LoginOutput::Connected {
                                site,
                                remote,
                                resume,
                            },
                        };
                        // Sending fails only if the main window is gone; nothing to do then.
                        let _ = sender.output(output);
                        root.close();
                    }
                    Err(ConnectError::UnknownHostKey { fingerprint, key }) => {
                        self.status = String::from("Waiting for you to check the server's key");
                        let host = self
                            .login_site()
                            .map_or_else(String::new, |s| s.host.clone());
                        let question = format!("Trust the server {host}?");
                        let detail = format!(
                            "This is the first connection to this server, so its identity \
                             cannot be checked yet. Its host key fingerprint is:\n\n\
                             {fingerprint}\n\nIf it matches the server's real key, trust it: \
                             it will be remembered in ~/.ssh/known_hosts."
                        );
                        let wording = Confirmation {
                            title: "Unknown Server",
                            question: &question,
                            detail: &detail,
                            confirm_label: "Trust and Connect",
                        };
                        let sender = sender.clone();
                        confirm_dialog::ask(root.upcast_ref(), wording, move |trusted| {
                            if trusted {
                                sender.input(LoginMsg::TrustHostKey(key.clone()));
                            }
                        });
                    }
                    Err(ConnectError::UntrustedCertificate {
                        host,
                        port,
                        fingerprint,
                        reason,
                    }) => {
                        self.status =
                            String::from("Waiting for you to check the server's certificate");
                        let question = format!("Trust the certificate of {host}?");
                        let detail = format!(
                            "The server's TLS certificate cannot be verified: {reason}. FTP \
                             servers often use a self-signed certificate. Its fingerprint \
                             is:\n\n{fingerprint}\n\nIf it matches the server's real \
                             certificate, trust it: it will be remembered for this server."
                        );
                        let wording = Confirmation {
                            title: "Untrusted Certificate",
                            question: &question,
                            detail: &detail,
                            confirm_label: "Trust and Connect",
                        };
                        let sender = sender.clone();
                        confirm_dialog::ask(root.upcast_ref(), wording, move |trusted| {
                            if trusted {
                                sender.input(LoginMsg::TrustCertificate {
                                    host: host.clone(),
                                    port,
                                    fingerprint: fingerprint.clone(),
                                });
                            }
                        });
                    }
                    Err(ConnectError::KeyPassphraseRequired { path, wrong }) => {
                        let save = self
                            .pending_passphrase
                            .take()
                            .is_some_and(|pending| pending.save);
                        self.status = String::from("Waiting for the key's passphrase");
                        let sender = sender.clone();
                        key_passphrase_dialog::ask(
                            root.upcast_ref(),
                            &path,
                            wrong,
                            move |answer| {
                                sender.input(match answer {
                                    Some(mut answer) => {
                                        // Keep the earlier "save" choice when asked again.
                                        answer.save |= save;
                                        LoginMsg::KeyPassphraseGiven(answer)
                                    }
                                    None => LoginMsg::Abandoned,
                                });
                            },
                        );
                    }
                    Err(error) => {
                        tracing::warn!("login failed: {error}");
                        self.pending_passphrase = None;
                        // Site to site, say which site, and drop the other's connection.
                        self.status = match self.group_login.take() {
                            Some(pair) if matches!(error, ConnectError::PasswordRequired) => {
                                format!(
                                    "{} needs a password: save it on the Direct tab, or use SSH keys",
                                    pair.current().display_name()
                                )
                            }
                            Some(pair) => format!("{}: {error}", pair.current().display_name()),
                            None => error.to_string(),
                        };
                    }
                }
            }
            LoginCmd::PasswordChanged {
                site_id,
                saved,
                result,
                unsaved,
            } => match result {
                Ok(()) => {
                    if let Some(site) = self.book.sites.iter_mut().find(|site| site.id == site_id) {
                        site.password_saved = saved;
                    }
                    self.write_sites(if saved {
                        "stored a site password in the keyring"
                    } else {
                        "removed a site password from the keyring"
                    });
                    // The list and form were drawn before the keyring answered: refresh
                    // the key icon, and the "Save password" box if this site is shown.
                    self.site_list.fill(&self.book.sites, self.selection);
                    if !self.editing && self.selected_site().is_some_and(|site| site.id == site_id)
                    {
                        self.save_password = saved;
                    }
                    self.status = String::from(if saved {
                        "Password saved in the keyring"
                    } else {
                        "Saved password removed"
                    });
                }
                Err(error) => {
                    tracing::error!("keyring update failed: {error}");
                    self.status = format!("Keyring: {error}");
                    // Put the password back, so Login still works with it.
                    if let Some(password) = unsaved
                        && !self.editing
                        && self.selected_site().is_some_and(|site| site.id == site_id)
                    {
                        self.password.set_text(password.expose_secret());
                        self.status = format!(
                            "Password not saved ({error}); it stays in the field for this login"
                        );
                    }
                }
            },
            LoginCmd::PassphraseSaved { site_id, result } => match result {
                Ok(()) => {
                    if let Some(site) = self.book.sites.iter_mut().find(|site| site.id == site_id) {
                        site.ssh.passphrase_saved = true;
                    }
                    self.write_sites("stored a key passphrase in the keyring");
                }
                Err(error) => tracing::error!("cannot save the key passphrase: {error}"),
            },
            LoginCmd::PasswordForgotten(result) => {
                if let Err(error) = result {
                    tracing::warn!("could not remove a deleted site's password: {error}");
                }
            }
            LoginCmd::Exported(result) => {
                self.busy = false;
                self.status = match result {
                    Ok(report) => {
                        let mut summary = format!(
                            "Exported {} sites ({} with passwords) to {}",
                            report.sites,
                            report.with_passwords,
                            report.path.display()
                        );
                        if report.unreadable > 0 {
                            summary.push_str(&format!(
                                "; {} passwords could not be read",
                                report.unreadable
                            ));
                        }
                        tracing::info!("{summary}");
                        summary
                    }
                    Err(error) => {
                        tracing::error!("export failed: {error}");
                        format!("Export failed: {error}")
                    }
                };
            }
            LoginCmd::Imported(result) => {
                self.busy = false;
                match result {
                    Ok(ImportReport {
                        sites,
                        failed_passwords,
                    }) => {
                        let with_passwords = sites.iter().filter(|s| s.password_saved).count();
                        let (added, replaced) = self.book.merge(sites);
                        let mut summary = format!(
                            "Imported {} sites ({added} new, {replaced} updated, \
                             {with_passwords} with passwords)",
                            added + replaced
                        );
                        if failed_passwords > 0 {
                            summary.push_str(&format!(
                                "; {failed_passwords} passwords could not be stored"
                            ));
                        }
                        self.write_sites(&summary);
                        self.select(Selection::Nothing);
                        self.status = summary;
                    }
                    Err(error) => {
                        tracing::error!("import failed: {error}");
                        self.status = format!("Import failed: {error}");
                    }
                }
            }
        }
    }
}

/// The site list: "New Site" followed by every saved site.
struct SiteList {
    list: gtk::ListBox,
    /// Whether row 0 is "New Site" (the Direct tab's list; not the site-to-site ones).
    with_new_site: bool,
}

impl SiteList {
    /// Build an empty list.
    ///
    /// # Arguments
    ///
    /// * `on_click` - Called with the clicked entry. Clicks are used rather than selection
    ///   changes so that clicking an already highlighted "New Site" still starts a site.
    /// * `on_double_click` - Called with the double-clicked entry (after its first click
    ///   has reached `on_click`)
    fn new(
        with_new_site: bool,
        on_click: impl Fn(Selection) + 'static,
        on_double_click: impl Fn(Selection) + 'static,
    ) -> Self {
        // GTK's own selection is off: it would also highlight whichever row merely has
        // keyboard focus (the first row when the dialog opens). The picked row is marked
        // with the "picked" style class instead.
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .activate_on_single_click(true)
            .css_classes(["site-list"])
            .build();
        list.connect_row_activated(move |_, row| {
            on_click(selection_for_row(row.index(), with_new_site));
        });

        // Watch for double-clicks without taking the clicks away from the list: the
        // capture phase sees each press before the list does, and the gesture never
        // claims it, so single-click picking keeps working.
        let double_click = gtk::GestureClick::new();
        double_click.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak_list = list.downgrade();
        double_click.connect_pressed(move |_, presses, _x, y| {
            if presses != 2 {
                return;
            }
            // `y` is a fractional pixel position; rows are found by whole pixels.
            let row = weak_list.upgrade().and_then(|list| list.row_at_y(y as i32));
            if let Some(row) = row {
                on_double_click(selection_for_row(row.index(), with_new_site));
            }
        });
        list.add_controller(double_click);
        Self {
            list,
            with_new_site,
        }
    }

    /// The list widget, for placing in a layout.
    fn widget(&self) -> &gtk::ListBox {
        &self.list
    }

    /// Show `sites` and highlight `selection` (nothing is highlighted for
    /// [`Selection::Nothing`]).
    ///
    /// # Arguments
    ///
    /// * `sites` - Saved sites, in list order
    /// * `selection` - Entry to highlight
    fn fill(&self, sites: &[Site], selection: Selection) {
        self.list.remove_all();
        if self.with_new_site {
            self.list
                .append(&site_row(NEW_SITE_ICON, NEW_SITE_LABEL, false));
        }
        for site in sites {
            self.list.append(&site_row(
                SITE_ICON,
                &site.display_name(),
                site.password_saved,
            ));
        }
        let first_site = usize::from(self.with_new_site);
        let row = match selection {
            Selection::Nothing => None,
            Selection::NewSite => self.with_new_site.then_some(0),
            Selection::Saved(index) => index.checked_add(first_site),
        }
        .and_then(|row| i32::try_from(row).ok())
        .and_then(|row| self.list.row_at_index(row));
        if let Some(row) = row {
            row.add_css_class("picked");
        }
    }
}

/// Map a list row position to what it picks: with a "New Site" row, row 0 is "New Site"
/// and row `n` is saved site `n - 1`; without one, row `n` is saved site `n`.
///
/// # Arguments
///
/// * `row` - Row position as GTK reports it (negative when the row is not in a list)
/// * `with_new_site` - Whether the list starts with "New Site"
fn selection_for_row(row: i32, with_new_site: bool) -> Selection {
    match (usize::try_from(row), with_new_site) {
        (Ok(0), true) => Selection::NewSite,
        (Ok(row), true) => Selection::Saved(row - 1),
        (Ok(row), false) => Selection::Saved(row),
        (Err(_), _) => Selection::Nothing,
    }
}

/// One site list entry: an icon, a name, and a key icon when a password is saved.
///
/// # Arguments
///
/// * `icon` - Theme icon name
/// * `name` - Text to show
/// * `has_password` - Whether to show the saved-password key icon
fn site_row(icon: &str, name: &str, has_password: bool) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    row.append(&gtk::Image::from_icon_name(icon));
    row.append(
        &gtk::Label::builder()
            .label(name)
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build(),
    );
    if has_password {
        let key = gtk::Image::from_icon_name(PASSWORD_ICON);
        key.set_tooltip_text(Some("Password saved"));
        key.add_css_class("dim-label");
        row.append(&key);
    }
    row
}

/// A line of the Unfinished list: a tick box, what the transfer does, and below it
/// where it runs and when it was saved. The row is named after the record's key.
///
/// # Arguments
///
/// * `pending` - The unfinished transfer
/// * `route` - Its site, or site to site "left ⇄ right"
/// * `ticked` - Whether its box starts ticked
/// * `sender` - Receives [`LoginMsg::UnfinishedTicked`]
fn unfinished_row(
    pending: &PendingJob,
    route: &str,
    ticked: bool,
    sender: &ComponentSender<LoginDialog>,
) -> gtk::ListBoxRow {
    let job = &pending.job;
    let title = match (&pending.left_site_id, job.kind) {
        (Some(_), JobKind::Move) => format!("Move {}", job.subject()),
        (Some(_), _) => format!("Copy {}", job.subject()),
        (None, _) => job.title(),
    };
    let when = pending.saved_at.map_or_else(String::new, |seconds| {
        format!(
            " \u{b7} {}",
            format_modified(UNIX_EPOCH + Duration::from_secs(seconds))
        )
    });
    let check = gtk::CheckButton::builder()
        .active(ticked)
        .valign(gtk::Align::Center)
        .tooltip_text("Resume or discard this transfer")
        .build();
    {
        let (sender, key) = (sender.clone(), pending.key.clone());
        check.connect_toggled(move |check| {
            sender.input(LoginMsg::UnfinishedTicked {
                key: key.clone(),
                ticked: check.is_active(),
            });
        });
    }
    let text = gtk::Box::new(gtk::Orientation::Vertical, 1);
    let subtitle = format!("{route}{when}");
    for (label, class) in [
        (title.as_str(), None),
        (subtitle.as_str(), Some("dim-label")),
    ] {
        let label = gtk::Label::builder()
            .label(label)
            .tooltip_text(label)
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::Middle)
            .build();
        if let Some(class) = class {
            label.add_css_class(class);
        }
        text.append(&label);
    }
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    line.append(&check);
    line.append(&gtk::Image::from_icon_name(match job.kind {
        JobKind::Delete => "user-trash-symbolic",
        _ if pending.left_site_id.is_some() => "computer-symbolic",
        _ if job.side == Side::Local => "go-up-symbolic",
        _ => "go-down-symbolic",
    }));
    line.append(&text);
    let row = gtk::ListBoxRow::builder().child(&line).build();
    row.set_widget_name(&pending.key);
    row
}

/// The usual port for a protocol and, for FTP, its encryption.
fn default_port(protocol: Protocol, encryption: FtpEncryption) -> u16 {
    match protocol {
        Protocol::Ftp => encryption.default_port(),
        Protocol::Sftp | Protocol::Scp => protocol.default_port(),
    }
}

/// The key file path typed in the form: `None` when empty; a leading `~/` means the
/// home folder, as in a shell.
fn key_file_path(text: &str) -> Option<PathBuf> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    match (text.strip_prefix("~/"), std::env::home_dir()) {
        (Some(rest), Some(home)) => Some(home.join(rest)),
        _ => Some(Path::new(text).to_path_buf()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_file_paths_expand_the_home_folder() {
        let home = std::env::home_dir().expect("home");

        assert_eq!(key_file_path("  "), None);
        assert_eq!(
            key_file_path("~/.ssh/id_ed25519"),
            Some(home.join(".ssh/id_ed25519"))
        );
        assert_eq!(
            key_file_path(" /keys/server.ppk "),
            Some(PathBuf::from("/keys/server.ppk"))
        );
    }

    #[test]
    fn selection_for_row_maps_rows_after_new_site() {
        assert_eq!(selection_for_row(0, true), Selection::NewSite);
        assert_eq!(selection_for_row(1, true), Selection::Saved(0));
        assert_eq!(selection_for_row(4, true), Selection::Saved(3));
        assert_eq!(selection_for_row(-1, true), Selection::Nothing);
    }

    #[test]
    fn selection_for_row_maps_rows_without_new_site() {
        assert_eq!(selection_for_row(0, false), Selection::Saved(0));
        assert_eq!(selection_for_row(4, false), Selection::Saved(4));
        assert_eq!(selection_for_row(-1, false), Selection::Nothing);
    }
}
