mod app;
mod db;
mod metadata;
mod ui;

fn main() -> gtk4::glib::ExitCode {
    app::run()
}
