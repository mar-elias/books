mod book_dialog;
mod data;
mod grid;
mod prompt;
mod sidebar;
mod window;

use gtk4::prelude::*;
use libadwaita::Application;

pub fn build(app: &Application) {
    if let Err(err) = window::build(app) {
        eprintln!("Could not open the library: {err}");
        app.quit();
    }
}
