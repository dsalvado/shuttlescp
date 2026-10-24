//! ShuttleSCP's business logic, independent of any GUI toolkit.
//!
//! Front ends (today the relm4 / GTK one for Linux; later possibly native Windows or
//! macOS ones) build on these modules and add only presentation:
//!
//! * [`sites`] / [`config`] - saved connection sites and user preferences (JSON files)
//! * [`site_export`] - portable export / import with passphrase-encrypted passwords
//! * [`secrets`] - passwords in the system's password store
//! * [`remote`] - connecting over SFTP (SSH host-key checks) or FTP / FTPS (TLS certificate
//!   checks), browsing and file operations
//! * [`transfer`] - copying, moving and deleting files and folders, with progress
//! * [`pending`] - unfinished transfers, kept so they can be resumed
//! * [`listing_cache`] - server folder listings saved between sessions
//! * [`session`] - the login, export and import workflows
//! * [`entry`] / [`local_fs`] / [`disks`] - files, local folders and disks
//! * [`path`] - paths on this computer and on the server (which always use `/`)
//! * [`logging`] / [`log_reader`] - the log file and following it live
//! * [`format`] - sizes, dates and shortened titles for display
//! * [`about`] - the app's name, description and build details

pub mod about;
pub mod config;
pub mod disks;
pub mod entry;
pub mod format;
pub mod listing_cache;
pub mod local_fs;
pub mod log_reader;
pub mod logging;
pub mod path;
pub mod pending;
pub mod remote;
pub mod secrets;
pub mod session;
pub mod site_export;
pub mod sites;
pub mod transfer;
pub mod update;
