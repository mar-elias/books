mod commands;
mod db;
mod metadata;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .register_uri_scheme_protocol("cover", |ctx, request| {
            commands::cover_response(&ctx, request)
        })
        .setup(|app| {
            let library = db::Library::open_default().map_err(|err| err.to_string());
            app.manage(commands::AppState {
                library: std::sync::Mutex::new(library),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::books,
            commands::authors,
            commands::categories,
            commands::tags,
            commands::lists,
            commands::add_book,
            commands::update_book,
            commands::delete_book,
            commands::create_category,
            commands::create_tag,
            commands::create_list,
            commands::rename_list,
            commands::delete_list,
            commands::add_book_to_list,
            commands::remove_book_from_list,
            commands::open_book,
            commands::library_paths
        ])
        .run(tauri::generate_context!())
        .expect("error while running Books");
}
