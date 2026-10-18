//! The Login dialog: saved sites on the left (with Edit and Delete), the session details
//! on the right (with Save and Cancel), and Import / Export / Login / Close at the bottom.
//!
//! The session fields stay disabled until the user clicks "New Site", or picks a saved
//! site and clicks Edit. Passwords are kept in the desktop keyring (see `secrets`); the
//! form shows whether one is saved. Keyring work runs as background commands so the
//! window never freezes, e.g. while the desktop asks to unlock the keyring.

use std::path::PathBuf;

use relm4::RelmRemoveAllExt;
use relm4::gtk::prelude::*;
use relm4::prelude::*;
use russh::keys::PublicKey;
use secrecy::SecretString;

use crate::confirm_dialog::{self, Confirmation};
use crate::passphrase_dialog::{self, PassphraseAnswer, PassphrasePurpose};
use crate::remote::{self, ConnectError, Remote};
use crate::secrets;
use crate::site_export::{self, ExportFile};
use crate::sites::{Protocol, Site, SiteBook, new_site_id};

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
const EXPORT_FILE_NAME: &str = "guiscp-sites.json";

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
    /// The port field changed.
    PortChanged(i32),
    /// The "Save password" box was ticked or cleared.
    SavePasswordToggled(bool),
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
    /// Connect to the selected site.
    Login,
    /// The user trusts this new host key: remember it and connect.
    TrustHostKey(Box<PublicKey>),
}

/// What the Login dialog starts with.
#[derive(Debug)]
pub struct LoginInit {
    /// Path of the saved sites file.
    pub sites_path: PathBuf,
    /// Current "show the dialog at startup" preference.
    pub show_at_startup: bool,
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
    },
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
    },
    /// The keyring entry of a deleted site was removed.
    PasswordForgotten(Result<(), String>),
    /// An export finished, with a summary or an error.
    Exported(Result<String, String>),
    /// An import finished: the sites (with `password_saved` set where the keyring
    /// accepted the password) and how many passwords could not be stored.
    Imported(Result<(Vec<Site>, usize), String>),
    /// A login attempt finished.
    Connected(Result<(Site, Remote), ConnectError>),
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
    /// Whether Save stores the typed password in the keyring.
    save_password: bool,
    // Text buffers shared with the entries: the entries edit them directly, and the
    // dialog reads and fills them, so typing needs no message round trip.
    host: gtk::EntryBuffer,
    user: gtk::EntryBuffer,
    password: gtk::EntryBuffer,
    site_list: SiteList,
    /// Last result or progress note, shown at the bottom left.
    status: String,
    /// An export or import is running.
    busy: bool,
    /// The open file chooser; kept here so it stays alive until the user answers.
    chooser: Option<gtk::FileChooserNative>,
    /// Whether the dialog opens when the app starts (saved by the main window).
    show_at_startup: bool,
}

impl LoginDialog {
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
        self.port = site
            .as_ref()
            .map_or(self.protocol.default_port(), |site| site.port);
        self.host
            .set_text(site.as_ref().map_or("", |site| site.host.as_str()));
        self.user
            .set_text(site.as_ref().map_or("", |site| site.user.as_str()));
        self.password.set_text("");
        // New sites offer to save the password; saved ones keep their current choice.
        self.save_password = site.as_ref().is_none_or(|site| site.password_saved);
        self.editing = self.selection == Selection::NewSite;
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

    /// Whether Login applies: a saved site is picked and nothing else is going on.
    fn can_login(&self) -> bool {
        self.saved_site_idle()
    }

    /// Start connecting to the picked site in the background, with the typed password
    /// or else the one saved in the keyring.
    ///
    /// # Arguments
    ///
    /// * `sender` - Used to run the connection attempt
    /// * `approved_key` - A host key the user has just agreed to trust
    fn start_login(&mut self, sender: &ComponentSender<Self>, approved_key: Option<PublicKey>) {
        if !self.can_login() {
            return;
        }
        let Some(site) = self.selected_site().cloned() else {
            return;
        };
        let typed = (!self.password.text().is_empty())
            .then(|| SecretString::from(self.password.text().as_str()));
        self.busy = true;
        self.status = format!("Connecting to {}...", site.display_name());
        sender.oneshot_command(async move {
            let password = match typed {
                Some(password) => Some(password),
                None if site.password_saved => match secrets::load_password(&site.id).await {
                    Ok(password) => password,
                    Err(error) => {
                        tracing::warn!("cannot read the saved password: {error}");
                        None
                    }
                },
                None => None,
            };
            let result = remote::connect(&site, password, approved_key).await;
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
        let site = Site {
            id: existing
                .as_ref()
                .map_or_else(new_site_id, |site| site.id.clone()),
            protocol: self.protocol,
            host: self.host.text().trim().to_owned(),
            port: self.port,
            user: self.user.text().trim().to_owned(),
            password_saved: existing.as_ref().is_some_and(|site| site.password_saved),
        };
        // Capture the typed password and the "Save password" choice now: `select` below
        // reloads the form for the saved site, which clears the field and resets the box.
        let typed = (!self.password.text().is_empty())
            .then(|| SecretString::from(self.password.text().as_str()));
        let keyring_action = self.save_password_choice(site.password_saved);
        let name = site.display_name();
        let (site_id, label) = (site.id.clone(), format!("GuiSCP: {name}"));

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
            (Some(true), Some(password)) => {
                self.status = String::from("Saving password to the keyring...");
                sender.oneshot_command(async move {
                    let result = secrets::store_password(&site_id, &label, &password).await;
                    LoginCmd::PasswordChanged {
                        site_id,
                        saved: true,
                        result: result.map_err(|error| error.to_string()),
                    }
                });
            }
            (Some(false), _) => {
                sender.oneshot_command(async move {
                    let result = secrets::delete_password(&site_id).await;
                    LoginCmd::PasswordChanged {
                        site_id,
                        saved: false,
                        result: result.map_err(|error| error.to_string()),
                    }
                });
            }
            (Some(true), None) | (None, _) => {}
        }
    }

    /// What Save should do with the keyring: `Some(true)` store the typed password,
    /// `Some(false)` remove the saved one, `None` leave it as it is.
    ///
    /// # Arguments
    ///
    /// * `was_saved` - Whether the site had a saved password before this save
    fn save_password_choice(&self, was_saved: bool) -> Option<bool> {
        match (self.save_password, was_saved) {
            (true, _) => Some(true),
            (false, true) => Some(false),
            (false, false) => None,
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
            set_default_size: (760, 420),
            // Sized down in style.css.
            add_css_class: "login-dialog",

            gtk::Box {
                set_orientation: gtk::Orientation::Vertical,
                set_spacing: 12,
                set_margin_all: 12,

                gtk::Box {
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
                    // in a dashed outline (drawn by the "session-group" style).
                    gtk::Box {
                        set_orientation: gtk::Orientation::Vertical,
                        set_spacing: 4,
                        set_hexpand: true,
                        set_valign: gtk::Align::Start,
                        add_css_class: "session-group",

                        gtk::Grid {
                            set_row_spacing: 4,
                            set_column_spacing: 14,

                            attach[0, 0, 2, 1] = &gtk::Label {
                                set_label: "Session",
                                set_xalign: 0.0,
                                add_css_class: "heading",
                            },
                            attach[0, 1, 2, 1] = &gtk::Label {
                                set_label: "File protocol:",
                                set_xalign: 0.0,
                            },
                            attach[0, 2, 1, 1] = &gtk::DropDown::from_strings(&protocol_labels) {
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

                            attach[0, 3, 1, 1] = &gtk::Label {
                                set_label: "Host name:",
                                set_xalign: 0.0,
                            },
                            attach[1, 3, 1, 1] = &gtk::Label {
                                set_label: "Port number:",
                                set_xalign: 0.0,
                            },
                            attach[0, 4, 1, 1] = &gtk::Entry::with_buffer(&model.host) {
                                set_hexpand: true,
                                #[watch]
                                set_sensitive: model.editing,
                                connect_changed => LoginMsg::FormEdited,
                            },
                            attach[1, 4, 1, 1] = &gtk::SpinButton::with_range(1.0, 65535.0, 1.0) {
                                set_digits: 0,
                                #[watch]
                                set_sensitive: model.editing,
                                #[watch]
                                set_value: f64::from(model.port),
                                connect_value_changed[sender] => move |spin| {
                                    sender.input(LoginMsg::PortChanged(spin.value_as_int()));
                                },
                            },

                            attach[0, 5, 1, 1] = &gtk::Label {
                                set_label: "User name:",
                                set_xalign: 0.0,
                            },
                            attach[1, 5, 1, 1] = &gtk::Label {
                                set_label: "Password:",
                                set_xalign: 0.0,
                            },
                            attach[0, 6, 1, 1] = &gtk::Entry::with_buffer(&model.user) {
                                set_hexpand: true,
                                #[watch]
                                set_sensitive: model.editing,
                            },
                            attach[1, 6, 1, 1] = &gtk::Entry::with_buffer(&model.password) {
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
                            attach[1, 7, 1, 1] = &gtk::CheckButton {
                                set_label: Some("Save password"),
                                #[watch]
                                set_sensitive: model.editing,
                                set_tooltip_text: Some(
                                    "Keep the password in the desktop keyring (encrypted with \
                                     your login password)"
                                ),
                                #[watch]
                                set_active: model.save_password,
                                connect_toggled[sender] => move |check| {
                                    sender.input(LoginMsg::SavePasswordToggled(check.is_active()));
                                },
                            },
                        },

                        // Saved-password indicator (left; stays readable while the fields
                        // are disabled) and Save / Cancel (right), right under the fields.
                        gtk::Box {
                            set_orientation: gtk::Orientation::Horizontal,
                            set_spacing: 4,
                            set_margin_top: 4,

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
                },

                gtk::Box {
                    set_orientation: gtk::Orientation::Horizontal,
                    set_spacing: 6,

                    gtk::CheckButton {
                        set_label: Some("Show at startup"),
                        set_tooltip_text: Some("Open this dialog when GuiSCP starts"),
                        set_margin_end: 6,
                        #[watch]
                        set_active: model.show_at_startup,
                        connect_toggled[sender] => move |check| {
                            sender.input(LoginMsg::ShowAtStartupToggled(check.is_active()));
                        },
                    },
                    gtk::Button {
                        set_label: "Import...",
                        set_tooltip_text: Some("Add sites from a GuiSCP export file"),
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
        } = init;
        let book = SiteBook::load(&sites_path).unwrap_or_else(|error| {
            tracing::warn!("{error}; starting with no saved sites");
            SiteBook::default()
        });
        tracing::debug!("loaded {} saved sites", book.sites.len());

        let click_sender = sender.clone();
        let site_list =
            SiteList::new(move |selection| click_sender.input(LoginMsg::SiteClicked(selection)));

        let mut model = Self {
            book,
            sites_path,
            selection: Selection::Nothing,
            editing: false,
            protocol: Protocol::default(),
            port: Protocol::default().default_port(),
            save_password: true,
            host: gtk::EntryBuffer::default(),
            user: gtk::EntryBuffer::default(),
            password: gtk::EntryBuffer::default(),
            site_list,
            status: String::new(),
            busy: false,
            chooser: None,
            show_at_startup,
        };
        model.select(Selection::Nothing);

        let protocol_labels: Vec<&str> = Protocol::ALL.iter().map(|p| p.label()).collect();
        let site_list_box = model.site_list.widget();
        let widgets = view_output!();
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
                self.status.clear();
                root.present();
            }
            LoginMsg::Close => root.close(),
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
                    if self.port == self.protocol.default_port() {
                        self.port = protocol.default_port();
                    }
                    self.protocol = protocol;
                }
            }
            LoginMsg::PortChanged(value) => {
                if let Ok(port) = u16::try_from(value) {
                    self.port = port;
                }
            }
            LoginMsg::SavePasswordToggled(active) => self.save_password = active,
            LoginMsg::Login => self.start_login(&sender, None),
            LoginMsg::TrustHostKey(key) => self.start_login(&sender, Some(*key)),
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
                sender.oneshot_command(export(sites, path, passphrase));
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
                sender.oneshot_command(import(file, passphrase));
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
            LoginCmd::Connected(result) => {
                self.busy = false;
                match result {
                    Ok((site, remote)) => {
                        self.status.clear();
                        self.password.set_text("");
                        // Sending fails only if the main window is gone; nothing to do then.
                        let _ = sender.output(LoginOutput::Connected { site, remote });
                        root.close();
                    }
                    Err(ConnectError::UnknownHostKey { fingerprint, key }) => {
                        self.status = String::from("Waiting for you to check the server's key");
                        let host = self
                            .selected_site()
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
                    Err(error) => {
                        tracing::warn!("login failed: {error}");
                        self.status = error.to_string();
                    }
                }
            }
            LoginCmd::PasswordChanged {
                site_id,
                saved,
                result,
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
                }
            },
            LoginCmd::PasswordForgotten(result) => {
                if let Err(error) = result {
                    tracing::warn!("could not remove a deleted site's password: {error}");
                }
            }
            LoginCmd::Exported(result) => {
                self.busy = false;
                self.status = match result {
                    Ok(summary) => {
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
                    Ok((sites, failed_passwords)) => {
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

/// Background export: read the passwords from the keyring (if they are included),
/// encrypt them and write the file.
///
/// # Arguments
///
/// * `sites` - Sites to export
/// * `path` - File to write
/// * `passphrase` - Passphrase protecting the passwords, or `None` to leave them out
async fn export(sites: Vec<Site>, path: PathBuf, passphrase: Option<SecretString>) -> LoginCmd {
    let mut passwords = Vec::new();
    let mut unreadable = 0;
    if passphrase.is_some() {
        for site in sites.iter().filter(|site| site.password_saved) {
            match secrets::load_password(&site.id).await {
                Ok(Some(password)) => passwords.push((site.id.clone(), password)),
                Ok(None) | Err(_) => unreadable += 1,
            }
        }
    }
    let included = passwords.len();
    // Key derivation is deliberately slow, so it runs on a worker thread, not the UI's.
    let written = relm4::tokio::task::spawn_blocking(move || {
        site_export::export_sites(
            &sites,
            &passwords,
            passphrase.as_ref(),
            site_export::DEFAULT_COST,
        )
        .and_then(|file| file.write(&path).map(|()| (file.sites.len(), path)))
    })
    .await;
    LoginCmd::Exported(match written {
        Ok(Ok((count, path))) => {
            let mut summary = format!(
                "Exported {count} sites ({included} with passwords) to {}",
                path.display()
            );
            if unreadable > 0 {
                summary.push_str(&format!("; {unreadable} passwords could not be read"));
            }
            Ok(summary)
        }
        Ok(Err(error)) => Err(error.to_string()),
        Err(error) => Err(error.to_string()),
    })
}

/// Background import: decrypt the passwords and store them in the keyring.
///
/// # Arguments
///
/// * `file` - The export to import
/// * `passphrase` - Passphrase the export was made with
async fn import(file: ExportFile, passphrase: Option<SecretString>) -> LoginCmd {
    let decrypted = relm4::tokio::task::spawn_blocking(move || {
        site_export::import_sites(&file, passphrase.as_ref())
    })
    .await;
    let entries = match decrypted {
        Ok(Ok(entries)) => entries,
        Ok(Err(error)) => return LoginCmd::Imported(Err(error.to_string())),
        Err(error) => return LoginCmd::Imported(Err(error.to_string())),
    };
    let mut sites = Vec::with_capacity(entries.len());
    let mut failed = 0;
    for (mut site, password) in entries {
        if let Some(password) = password {
            let label = format!("GuiSCP: {}", site.display_name());
            match secrets::store_password(&site.id, &label, &password).await {
                Ok(()) => site.password_saved = true,
                Err(error) => {
                    tracing::warn!("could not store an imported password: {error}");
                    failed += 1;
                }
            }
        }
        sites.push(site);
    }
    LoginCmd::Imported(Ok((sites, failed)))
}

/// The site list: "New Site" followed by every saved site.
struct SiteList {
    list: gtk::ListBox,
}

impl SiteList {
    /// Build an empty list.
    ///
    /// # Arguments
    ///
    /// * `on_click` - Called with the clicked entry. Clicks are used rather than selection
    ///   changes so that clicking an already highlighted "New Site" still starts a site.
    fn new(on_click: impl Fn(Selection) + 'static) -> Self {
        // GTK's own selection is off: it would also highlight whichever row merely has
        // keyboard focus (the first row when the dialog opens). The picked row is marked
        // with the "picked" style class instead.
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .activate_on_single_click(true)
            .css_classes(["site-list"])
            .build();
        list.connect_row_activated(move |_, row| on_click(selection_for_row(row.index())));
        Self { list }
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
        self.list
            .append(&site_row(NEW_SITE_ICON, NEW_SITE_LABEL, false));
        for site in sites {
            self.list.append(&site_row(
                SITE_ICON,
                &site.display_name(),
                site.password_saved,
            ));
        }
        let row = match selection {
            Selection::Nothing => None,
            Selection::NewSite => Some(0),
            Selection::Saved(index) => index.checked_add(1),
        }
        .and_then(|row| i32::try_from(row).ok())
        .and_then(|row| self.list.row_at_index(row));
        if let Some(row) = row {
            row.add_css_class("picked");
        }
    }
}

/// Map a list row position to what it picks: row 0 is "New Site", row `n` is saved site
/// `n - 1`.
///
/// # Arguments
///
/// * `row` - Row position as GTK reports it (negative when the row is not in a list)
fn selection_for_row(row: i32) -> Selection {
    match usize::try_from(row) {
        Ok(0) => Selection::NewSite,
        Ok(row) => Selection::Saved(row - 1),
        Err(_) => Selection::Nothing,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_for_row_maps_rows_after_new_site() {
        assert_eq!(selection_for_row(0), Selection::NewSite);
        assert_eq!(selection_for_row(1), Selection::Saved(0));
        assert_eq!(selection_for_row(4), Selection::Saved(3));
        assert_eq!(selection_for_row(-1), Selection::Nothing);
    }
}
