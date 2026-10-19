//! GuiSCP's business logic, independent of any GUI toolkit.
//!
//! Front ends (today the relm4 / GTK one for Linux; later possibly native Windows or
//! macOS ones) build on these modules and add only presentation:
//!
//! * [`sites`] / [`config`] - saved connection sites and user preferences (JSON files)
//! * [`site_export`] - portable export / import with passphrase-encrypted passwords
//! * [`secrets`] - passwords in the system's password store
//! * [`remote`] - SSH host-key checks, login and SFTP listing
//! * [`session`] - the login, export and import workflows
//! * [`entry`] / [`local_fs`] / [`disks`] - files, local folders and disks
//! * [`logging`] / [`log_reader`] - the log file and following it live
//! * [`format`] - sizes, dates and shortened titles for display
//! * [`about`] - the app's name, description and build details

pub mod about;
pub mod config;
pub mod disks;
pub mod entry;
pub mod format;
pub mod local_fs;
pub mod log_reader;
pub mod logging;
pub mod remote;
pub mod secrets;
pub mod session;
pub mod site_export;
pub mod sites;
