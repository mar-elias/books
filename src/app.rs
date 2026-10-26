use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};

pub fn run() -> glib::ExitCode {
    let app = libadwaita::Application::builder()
        .application_id("com.local.Books")
        .flags(gio::ApplicationFlags::empty())
        .build();

    app.connect_activate(|app| {
        load_css();
        crate::ui::build(app);
    });

    app.run()
}

fn load_css() {
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(include_str!("style.css"));
    if let Some(display) = gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}
