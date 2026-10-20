//! The common frame of every dialog, so they all look alike: a compact title bar,
//! the same margins and spacing, a row of buttons along the bottom right, and Escape
//! to close. Sizes and colours are in style.css, under the `app-dialog` class.

use relm4::gtk;
use relm4::gtk::prelude::*;

/// CSS class of every dialog window (see style.css).
pub const DIALOG_CLASS: &str = "app-dialog";
/// Space between the parts of a dialog, in pixels.
pub const SPACING: i32 = 6;
/// Space between the window edge and the contents, in pixels.
const MARGIN: i32 = 12;

/// A dialog window under construction: fill `content` and `buttons`, then present it.
pub struct Dialog {
    /// The window.
    pub window: gtk::Window,
    /// Where the dialog's own widgets go, top to bottom.
    pub content: gtk::Box,
    /// The buttons along the bottom right; the main action goes last.
    pub buttons: gtk::Box,
}

impl Dialog {
    /// Start a modal dialog over `parent`. Escape closes it.
    ///
    /// # Arguments
    ///
    /// * `parent` - Window the dialog stays above
    /// * `title` - Text of its title bar
    pub fn new(parent: &gtk::Window, title: &str) -> Self {
        let window = gtk::Window::builder()
            .title(title)
            .transient_for(parent)
            .modal(true)
            .resizable(false)
            .build();
        style_window(&window, title);

        let content = gtk::Box::new(gtk::Orientation::Vertical, SPACING);
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, SPACING);
        buttons.set_halign(gtk::Align::End);
        buttons.set_margin_top(SPACING);
        let frame = gtk::Box::new(gtk::Orientation::Vertical, SPACING);
        for margin in [
            gtk::Widget::set_margin_top,
            gtk::Widget::set_margin_bottom,
            gtk::Widget::set_margin_start,
            gtk::Widget::set_margin_end,
        ] {
            margin(frame.upcast_ref(), MARGIN);
        }
        frame.append(&content);
        frame.append(&buttons);
        window.set_child(Some(&frame));
        close_on_escape(&window);
        Self {
            window,
            content,
            buttons,
        }
    }

    /// Add a button to the button row (left to right) that runs `on_click`.
    ///
    /// # Arguments
    ///
    /// * `label` - Its text
    /// * `kind` - How it is drawn
    /// * `on_click` - What it does
    pub fn add_button(
        &self,
        label: &str,
        kind: ButtonKind,
        on_click: impl Fn() + 'static,
    ) -> gtk::Button {
        let button = gtk::Button::with_label(label);
        match kind {
            ButtonKind::Normal => {}
            ButtonKind::Suggested => button.add_css_class("suggested-action"),
            ButtonKind::Destructive => button.add_css_class("destructive-action"),
        }
        button.connect_clicked(move |_| on_click());
        self.buttons.append(&button);
        button
    }

    /// Add a button that just closes the dialog.
    pub fn add_close_button(&self, label: &str) -> gtk::Button {
        let window = self.window.clone();
        self.add_button(label, ButtonKind::Normal, move || window.close())
    }
}

/// How a dialog button is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    /// An ordinary button (Cancel, Skip, ...).
    Normal,
    /// The main, safe action (Create, Export, Login).
    Suggested,
    /// An action that deletes or replaces something.
    Destructive,
}

/// Give a window the dialog look: the `app-dialog` class and a compact title bar.
/// Also used by the Login dialog, which builds its own contents.
///
/// # Arguments
///
/// * `window` - The dialog window
/// * `title` - Text of its title bar
pub fn style_window(window: &gtk::Window, title: &str) {
    // Added rather than set with the builder: setting `css_classes` would replace the
    // window's built-in classes (such as "background"), leaving its text unstyled.
    window.add_css_class(DIALOG_CLASS);
    window.set_titlebar(Some(&title_bar(Some(title), ":close")));
}

/// A compact title bar (see "Title bars" in style.css).
///
/// # Arguments
///
/// * `title` - Its text; `None` shows the window's own title
/// * `layout` - Which window buttons it has, e.g. ":close" or
///   ":minimize,maximize,close" (GTK's decoration layout)
pub fn title_bar(title: Option<&str>, layout: &str) -> gtk::HeaderBar {
    let bar = gtk::HeaderBar::new();
    bar.add_css_class("compact-titlebar");
    bar.set_decoration_layout(Some(layout));
    if let Some(title) = title {
        let label = gtk::Label::builder()
            .label(title)
            .css_classes(["title"])
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        bar.set_title_widget(Some(&label));
    }
    bar
}

/// A wrapped, left-aligned paragraph of text.
///
/// # Arguments
///
/// * `text` - The text
/// * `classes` - Extra CSS classes, e.g. `["heading"]` for the dialog's question
pub fn text(text: &str, classes: &[&str]) -> gtk::Label {
    let label = gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .wrap(true)
        .max_width_chars(52)
        .build();
    for class in classes {
        label.add_css_class(class);
    }
    label
}

/// Make Escape close `window`, as in standard dialogs.
fn close_on_escape(window: &gtk::Window) {
    let keys = gtk::EventControllerKey::new();
    let target = window.clone();
    keys.connect_key_pressed(move |_, key, _, _| {
        if key == gtk::gdk::Key::Escape {
            target.close();
            gtk::glib::Propagation::Stop
        } else {
            gtk::glib::Propagation::Proceed
        }
    });
    window.add_controller(keys);
}
