use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use base64::Engine;
use serde::{Deserialize, Serialize};
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::db::{self, Book, BookList, BookScope, Draft, Library};
use crate::metadata::{self, BookFormat};

pub struct AppState {
    pub library: Mutex<Result<Library, String>>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ScopeQuery {
    All,
    Author { author: String },
    List { id: i64 },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookForm {
    pub title: String,
    pub author: String,
    pub year: String,
    pub file_path: String,
    pub categories: String,
    pub tags: String,
    pub cover_png: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookCard {
    #[serde(flatten)]
    book: Book,
    author_label: String,
    format_label: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorRow {
    author: String,
    label: String,
    count: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inspected {
    title: String,
    author: String,
    year: Option<i32>,
    format: String,
    cover_png: String,
}

fn with_library<T>(
    state: &AppState,
    f: impl FnOnce(&Library) -> Result<T, String>,
) -> Result<T, String> {
    let guard = state.library.lock().map_err(|err| err.to_string())?;
    match &*guard {
        Ok(library) => f(library),
        Err(err) => Err(err.clone()),
    }
}

fn encode_png(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn decode_cover(value: &str) -> Result<Vec<u8>, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(Vec::new());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|_| "Could not read that cover".to_string())?;
    if !bytes.is_empty() && !bytes.starts_with(b"\x89PNG") {
        return Err("Could not read that cover".to_string());
    }
    Ok(bytes)
}

fn parse_year(value: &str) -> Result<Option<i32>, String> {
    let value = value.trim();
    if value.is_empty() {
        Ok(None)
    } else {
        value
            .parse::<i32>()
            .map(Some)
            .map_err(|_| "Year should be a number.".to_string())
    }
}

pub fn form_to_draft(form: BookForm) -> Result<Draft, String> {
    let path = PathBuf::from(form.file_path.trim());
    if form.file_path.trim().is_empty() {
        return Err("Choose a PDF or EPUB file.".to_string());
    }
    let Some(format) = BookFormat::from_path(&path) else {
        return Err("Choose a PDF or EPUB file.".to_string());
    };
    let year = parse_year(&form.year)?;
    let title = {
        let text = form.title.trim();
        if text.is_empty() {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("Untitled")
                .to_string()
        } else {
            text.to_string()
        }
    };
    let author = form.author.trim().to_string();
    let mut cover = decode_cover(&form.cover_png)?;
    if cover.is_empty() {
        cover = metadata::title_card_png(&title, &author).map_err(|err| err.to_string())?;
    }
    Ok(Draft {
        title,
        author,
        year,
        file_path: path.to_string_lossy().into_owned(),
        format: format.as_str().to_string(),
        categories: db::split_names(&form.categories),
        tags: db::split_names(&form.tags),
        cover_png: cover,
    })
}

#[tauri::command]
pub fn books(state: State<AppState>, scope: ScopeQuery) -> Result<Vec<BookCard>, String> {
    let scope = match scope {
        ScopeQuery::All => BookScope::All,
        ScopeQuery::Author { author } => BookScope::Author(author),
        ScopeQuery::List { id } => BookScope::List(id),
    };
    with_library(&state, |library| {
        library
            .books(&scope)
            .map_err(|err| err.to_string())
            .map(|books| books.into_iter().map(book_card).collect())
    })
}

#[tauri::command]
pub fn authors(state: State<AppState>) -> Result<Vec<AuthorRow>, String> {
    with_library(&state, |library| {
        library.authors().map_err(|err| err.to_string()).map(|groups| {
            groups
                .into_iter()
                .map(|group| AuthorRow {
                    label: db::author_label(&group.author),
                    author: group.author,
                    count: group.count,
                })
                .collect()
        })
    })
}

fn book_card(book: Book) -> BookCard {
    let format_label = match book.format.as_str() {
        "epub" => BookFormat::Epub,
        _ => BookFormat::Pdf,
    }
    .label()
    .to_string();
    let author_label = db::author_label(&book.author);
    BookCard {
        book,
        author_label,
        format_label,
    }
}

#[tauri::command]
pub fn lists(state: State<AppState>) -> Result<Vec<BookList>, String> {
    with_library(&state, |library| {
        library.lists().map_err(|err| err.to_string())
    })
}

#[tauri::command]
pub fn inspect_file(path: String) -> Result<Inspected, String> {
    let path = PathBuf::from(path.trim());
    if !path.is_file() {
        return Err("Choose a PDF or EPUB file.".to_string());
    }
    let extracted = metadata::extract(&path).map_err(|err| err.to_string())?;
    Ok(Inspected {
        title: extracted.title,
        author: extracted.author,
        year: extracted.year,
        format: extracted.format.as_str().to_string(),
        cover_png: encode_png(&extracted.cover_png),
    })
}

#[tauri::command]
pub fn cover_from_image(path: String) -> Result<String, String> {
    let path = PathBuf::from(path.trim());
    let bytes = std::fs::read(&path).map_err(|err| err.to_string())?;
    let png = metadata::png_from_bytes(&bytes).map_err(|err| err.to_string())?;
    Ok(encode_png(&png))
}

#[tauri::command]
pub fn add_book(state: State<AppState>, draft: BookForm) -> Result<Book, String> {
    let draft = form_to_draft(draft)?;
    with_library(&state, |library| {
        library.add_book(&draft).map_err(|err| err.to_string())
    })
}

#[tauri::command]
pub fn update_book(state: State<AppState>, id: i64, draft: BookForm) -> Result<Book, String> {
    let draft = form_to_draft(draft)?;
    with_library(&state, |library| {
        library.update_book(id, &draft).map_err(|err| err.to_string())
    })
}

#[tauri::command]
pub fn delete_book(state: State<AppState>, id: i64) -> Result<(), String> {
    with_library(&state, |library| {
        library.delete_book(id).map_err(|err| err.to_string())
    })
}

#[tauri::command]
pub fn create_list(state: State<AppState>, name: String) -> Result<BookList, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Enter a list name.".to_string());
    }
    let name = name.to_string();
    with_library(&state, |library| {
        library.create_list(&name).map_err(|err| err.to_string())
    })
}

#[tauri::command]
pub fn rename_list(state: State<AppState>, id: i64, name: String) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Enter a list name.".to_string());
    }
    let name = name.to_string();
    with_library(&state, |library| {
        library.rename_list(id, &name).map_err(|err| err.to_string())
    })
}

#[tauri::command]
pub fn delete_list(state: State<AppState>, id: i64) -> Result<(), String> {
    with_library(&state, |library| {
        library.delete_list(id).map_err(|err| err.to_string())
    })
}

#[tauri::command]
pub fn add_book_to_list(state: State<AppState>, list_id: i64, book_id: i64) -> Result<bool, String> {
    with_library(&state, |library| {
        library
            .add_book_to_list(list_id, book_id)
            .map_err(|err| err.to_string())
    })
}

#[tauri::command]
pub fn remove_book_from_list(state: State<AppState>, list_id: i64, book_id: i64) -> Result<(), String> {
    with_library(&state, |library| {
        library
            .remove_book_from_list(list_id, book_id)
            .map_err(|err| err.to_string())
    })
}

#[tauri::command]
pub fn open_book(state: State<AppState>, id: i64) -> Result<(), String> {
    let path = with_library(&state, |library| {
        library
            .book(id)
            .map_err(|err| err.to_string())?
            .map(|book| book.file_path)
            .ok_or_else(|| "That book is no longer in the library".to_string())
    })?;
    let file = PathBuf::from(&path);
    if !file.exists() {
        return Err("That file is missing".to_string());
    }
    spawn_reader(&file)
}

fn spawn_reader(path: &Path) -> Result<(), String> {
    match spawn_detached("okular", path) {
        Ok(()) => Ok(()),
        Err(err) => spawn_detached("xdg-open", path).map_err(|fallback| {
            format!("Could not open Okular ({err}). {fallback}")
        }),
    }
}

fn spawn_detached(program: &str, path: &Path) -> std::io::Result<()> {
    let mut child = Command::new(program)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[tauri::command]
pub fn pick_book(app: tauri::AppHandle) -> Result<Option<String>, String> {
    pick_file(
        &app,
        "Choose a book",
        "PDF and EPUB",
        &["pdf", "epub"],
    )
}

#[tauri::command]
pub fn pick_cover(app: tauri::AppHandle) -> Result<Option<String>, String> {
    pick_file(&app, "Choose a cover", "Images", &["png", "jpg", "jpeg", "gif"])
}

fn pick_file(
    app: &tauri::AppHandle,
    title: &str,
    filter_name: &str,
    extensions: &[&str],
) -> Result<Option<String>, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut dialog = app
        .dialog()
        .file()
        .add_filter(filter_name, extensions)
        .set_title(title);
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&window);
    }
    dialog.pick_file(move |file| {
        let _ = tx.send(file);
    });
    match rx.recv().map_err(|err| err.to_string())? {
        Some(path) => path
            .into_path()
            .map(|file| Some(file.to_string_lossy().into_owned()))
            .map_err(|err| err.to_string()),
        None => Ok(None),
    }
}

pub fn cover_response(
    ctx: &tauri::UriSchemeContext<'_, tauri::Wry>,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Cow<'static, [u8]>> {
    let missing = || {
        tauri::http::Response::builder()
            .status(404)
            .body(Cow::Owned(Vec::new()))
            .unwrap()
    };
    let Some(id) = book_id_from_path(request.uri().path()) else {
        return missing();
    };
    let Some(state) = ctx.app_handle().try_state::<AppState>() else {
        return missing();
    };
    let path = {
        let guard = match state.library.lock() {
            Ok(guard) => guard,
            Err(_) => return missing(),
        };
        let Ok(library) = &*guard else {
            return missing();
        };
        let Ok(Some(book)) = library.book(id) else {
            return missing();
        };
        let Some(cover) = book.cover_path else {
            return missing();
        };
        let cover = PathBuf::from(cover);
        if !cover.starts_with(library.cover_directory()) {
            return missing();
        }
        cover
    };
    match std::fs::read(&path) {
        Ok(bytes) => tauri::http::Response::builder()
            .status(200)
            .header(tauri::http::header::CONTENT_TYPE, "image/png")
            .header(tauri::http::header::CACHE_CONTROL, "no-store")
            .header(tauri::http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
            .body(Cow::Owned(bytes))
            .unwrap(),
        Err(_) => missing(),
    }
}

fn book_id_from_path(path: &str) -> Option<i64> {
    let segment = path.rsplit('/').find(|part| !part.is_empty())?;
    let digits = segment.strip_suffix(".png").unwrap_or(segment);
    let id = digits.parse::<i64>().ok()?;
    (id > 0).then_some(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(title: &str, year: &str, path: &str, cover: &str) -> BookForm {
        BookForm {
            title: title.to_string(),
            author: "Ada".to_string(),
            year: year.to_string(),
            file_path: path.to_string(),
            categories: " Fiction, fiction ,History".to_string(),
            tags: "unread".to_string(),
            cover_png: cover.to_string(),
        }
    }

    #[test]
    fn empty_title_uses_file_stem() {
        let cover = encode_png(b"\x89PNG\r\n\x1a\n");
        let draft = form_to_draft(form("  ", "", "/books/kindred.epub", &cover)).unwrap();
        assert_eq!(draft.title, "kindred");
        assert_eq!(draft.author, "Ada");
        assert_eq!(draft.year, None);
        assert_eq!(draft.format, "epub");
        assert_eq!(draft.categories, vec!["Fiction", "History"]);
        assert_eq!(draft.tags, vec!["unread"]);
    }

    #[test]
    fn year_must_be_numeric() {
        let cover = encode_png(b"\x89PNG\r\n\x1a\n");
        let error = form_to_draft(form("Kindred", "nope", "/books/kindred.epub", &cover)).unwrap_err();
        assert_eq!(error, "Year should be a number.");
    }

    #[test]
    fn rejects_unsupported_files_and_bad_covers() {
        let missing = form_to_draft(form("Notes", "", "/books/notes.txt", "")).unwrap_err();
        assert_eq!(missing, "Choose a PDF or EPUB file.");
        let cover = form_to_draft(form("Kindred", "", "/books/kindred.epub", "aaaa")).unwrap_err();
        assert_eq!(cover, "Could not read that cover");
    }

    #[test]
    fn scope_json_uses_kind_tag() {
        let scope: ScopeQuery = serde_json::from_str(r#"{"kind":"list","id":3}"#).unwrap();
        assert!(matches!(scope, ScopeQuery::List { id: 3 }));
        let author: ScopeQuery =
            serde_json::from_str(r#"{"kind":"author","author":"Octavia Butler"}"#).unwrap();
        assert!(matches!(author, ScopeQuery::Author { author } if author == "Octavia Butler"));
    }

    #[test]
    fn cover_urls_use_the_book_id() {
        assert_eq!(book_id_from_path("/12"), Some(12));
        assert_eq!(book_id_from_path("/12.png"), Some(12));
        assert_eq!(book_id_from_path("/"), None);
        assert_eq!(book_id_from_path("/0"), None);
    }
}
