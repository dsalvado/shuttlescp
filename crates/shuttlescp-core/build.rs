//! Rebuild when the translations change: `static_loader!` (src/i18n.rs) builds the
//! `i18n` folder into the library, so a new language or a changed text must rebuild it.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=i18n");
}
