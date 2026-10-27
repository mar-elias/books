use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

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
    pub cover_path: String,
    pub categories: String,
    pub tags: String,
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

pub fn form_to_draft(form: BookForm, keep_cover: Option<Vec<u8>>) -> Result<Draft, String> {
    let year = parse_year(&form.year)?;
    let file_text = form.file_path.trim();
    let (file_path, format) = if file_text.is_empty() {
        (String::new(), String::new())
    } else {
        let path = PathBuf::from(file_text);
        let Some(format) = BookFormat::from_path(&path) else {
            return Err("Book path should be a PDF or EPUB file.".to_string());
        };
        if !path.is_file() {
            return Err("That book file is missing.".to_string());
        }
        (
            path.to_string_lossy().into_owned(),
            format.as_str().to_string(),
        )
    };
    let title = {
        let text = form.title.trim();
        if !text.is_empty() {
            text.to_string()
        } else if let Some(stem) = Path::new(&file_path)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.is_empty())
        {
            stem.to_string()
        } else {
            "Untitled".to_string()
        }
    };
    let author = form.author.trim().to_string();
    let cover = cover_bytes(&form.cover_path, &file_path, &title, &author, keep_cover)?;
    Ok(Draft {
        title,
        author,
        year,
        file_path,
        format,
        categories: db::split_names(&form.categories),
        tags: db::split_names(&form.tags),
        cover_png: cover,
    })
}

fn cover_bytes(
    cover_path: &str,
    file_path: &str,
    title: &str,
    author: &str,
    keep_cover: Option<Vec<u8>>,
) -> Result<Vec<u8>, String> {
    let cover_path = cover_path.trim();
    if !cover_path.is_empty() {
        let bytes =
            std::fs::read(cover_path).map_err(|_| "Could not read that cover.".to_string())?;
        return metadata::png_from_bytes(&bytes)
            .map_err(|_| "Could not read that cover.".to_string());
    }
    if let Some(bytes) = keep_cover.filter(|bytes| bytes.starts_with(b"\x89PNG")) {
        return Ok(bytes);
    }
    if !file_path.is_empty() {
        return metadata::extract(Path::new(file_path))
            .map(|extracted| extracted.cover_png)
            .map_err(|_| "Could not read a cover from that book.".to_string());
    }
    metadata::title_card_png(title, author).map_err(|err| err.to_string())
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
        "epub" => BookFormat::Epub.label().to_string(),
        "pdf" => BookFormat::Pdf.label().to_string(),
        _ => String::new(),
    };
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
pub fn add_book(state: State<AppState>, draft: BookForm) -> Result<Book, String> {
    let draft = form_to_draft(draft, None)?;
    with_library(&state, |library| {
        library.add_book(&draft).map_err(|err| err.to_string())
    })
}

#[tauri::command]
pub fn update_book(state: State<AppState>, id: i64, draft: BookForm) -> Result<Book, String> {
    let existing = with_library(&state, |library| stored_cover(library, id))?;
    let draft = form_to_draft(draft, existing)?;
    with_library(&state, |library| {
        library.update_book(id, &draft).map_err(|err| err.to_string())
    })
}

fn stored_cover(library: &Library, id: i64) -> Result<Option<Vec<u8>>, String> {
    let book = library
        .book(id)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "That book is no longer in the library".to_string())?;
    let Some(path) = book.cover_path else {
        return Ok(None);
    };
    match std::fs::read(path) {
        Ok(bytes) if bytes.starts_with(b"\x89PNG") => Ok(Some(bytes)),
        _ => Ok(None),
    }
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
    if path.trim().is_empty() {
        return Err("That book has no file.".to_string());
    }
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
            cover_path: cover.to_string(),
            categories: " Fiction, fiction ,History".to_string(),
            tags: "unread".to_string(),
        }
    }

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ebook-reader-form-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_png() -> Vec<u8> {
        metadata::title_card_png("Cover", "Ada").unwrap()
    }

    #[test]
    fn empty_title_uses_file_stem() {
        let dir = temp_dir();
        let book = dir.join("kindred.epub");
        let cover = dir.join("cover.png");
        std::fs::write(&book, b"epub").unwrap();
        std::fs::write(&cover, sample_png()).unwrap();
        let draft = form_to_draft(
            form("  ", "", &book.to_string_lossy(), &cover.to_string_lossy()),
            None,
        )
        .unwrap();
        assert_eq!(draft.title, "kindred");
        assert_eq!(draft.author, "Ada");
        assert_eq!(draft.year, None);
        assert_eq!(draft.format, "epub");
        assert_eq!(draft.categories, vec!["Fiction", "History"]);
        assert_eq!(draft.tags, vec!["unread"]);
        assert!(draft.cover_png.starts_with(b"\x89PNG"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn year_must_be_numeric() {
        let error =
            form_to_draft(form("Kindred", "nope", "/books/kindred.epub", ""), None).unwrap_err();
        assert_eq!(error, "Year should be a number.");
    }

    #[test]
    fn rejects_unsupported_files_and_bad_covers() {
        let missing = form_to_draft(form("Notes", "", "/books/notes.txt", ""), None).unwrap_err();
        assert_eq!(missing, "Book path should be a PDF or EPUB file.");
        let absent =
            form_to_draft(form("Kindred", "", "/books/kindred.epub", ""), None).unwrap_err();
        assert_eq!(absent, "That book file is missing.");
        let dir = temp_dir();
        let book = dir.join("kindred.epub");
        std::fs::write(&book, b"epub").unwrap();
        let cover = form_to_draft(
            form(
                "Kindred",
                "",
                &book.to_string_lossy(),
                &dir.join("missing.png").to_string_lossy(),
            ),
            None,
        )
        .unwrap_err();
        assert_eq!(cover, "Could not read that cover.");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn entry_without_paths_uses_a_title_card() {
        let draft = form_to_draft(form("Notes", "", "", ""), None).unwrap();
        assert_eq!(draft.title, "Notes");
        assert_eq!(draft.file_path, "");
        assert_eq!(draft.format, "");
        assert!(draft.cover_png.starts_with(b"\x89PNG"));
        let untitled = form_to_draft(form("  ", "", "  ", "  "), None).unwrap();
        assert_eq!(untitled.title, "Untitled");
        assert_eq!(untitled.file_path, "");
    }

    #[test]
    fn missing_cover_path_uses_the_first_page() {
        let dir = temp_dir();
        let book = dir.join("hello.pdf");
        std::fs::write(&book, minimal_pdf()).unwrap();
        let draft = form_to_draft(form("", "", &book.to_string_lossy(), ""), None).unwrap();
        assert_eq!(draft.title, "hello");
        assert_eq!(draft.format, "pdf");
        assert!(draft.cover_png.starts_with(b"\x89PNG"));
        let kept = form_to_draft(
            form("Hello", "", &book.to_string_lossy(), ""),
            Some(sample_png()),
        )
        .unwrap();
        assert_eq!(kept.cover_png, sample_png());
        let _ = std::fs::remove_dir_all(dir);
    }

    fn minimal_pdf() -> Vec<u8> {
        let objects = [
            b"1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n".to_vec(),
            b"2 0 obj<</Type/Pages/Count 1/Kids[3 0 R]>>endobj\n".to_vec(),
            b"3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 300 400]/Contents 4 0 R/Resources<</Font<</F1 5 0 R>>>>>>endobj\n".to_vec(),
            b"4 0 obj<</Length 44>>stream\nBT /F1 24 Tf 40 200 Td (Hello) Tj ET\nendstream\nendobj\n".to_vec(),
            b"5 0 obj<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>endobj\n".to_vec(),
        ];
        let mut body = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for object in &objects {
            offsets.push(body.len());
            body.extend_from_slice(object);
        }
        let xref_at = body.len();
        let mut xref = format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
        for offset in offsets {
            xref.push_str(&format!("{offset:010} 00000 n \n"));
        }
        xref.push_str(&format!(
            "trailer<</Size {}/Root 1 0 R>>\nstartxref\n{xref_at}\n%%EOF\n",
            objects.len() + 1
        ));
        body.extend(xref.as_bytes());
        body
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
