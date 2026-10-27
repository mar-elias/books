use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Book {
    pub id: i64,
    pub title: String,
    pub author: String,
    pub year: Option<i32>,
    pub file_path: String,
    pub format: String,
    pub cover_path: Option<String>,
    pub categories: Vec<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    pub title: String,
    pub author: String,
    pub year: Option<i32>,
    pub file_path: String,
    pub format: String,
    pub categories: Vec<String>,
    pub tags: Vec<String>,
    pub cover_png: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorGroup {
    pub author: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NameCount {
    pub name: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookList {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone)]
pub enum BookScope {
    All,
    Author(String),
    Category(String),
    Tag(String),
    List(i64),
}

#[derive(Debug)]
pub enum DbError {
    Sqlite(rusqlite::Error),
    Io(std::io::Error),
    DuplicateFile,
    DuplicateList,
    DuplicateCategory,
    DuplicateTag,
    NotFound,
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(err) => write!(f, "{err}"),
            Self::Io(err) => write!(f, "{err}"),
            Self::DuplicateFile => write!(f, "That file is already in the library"),
            Self::DuplicateList => write!(f, "A list with that name already exists"),
            Self::DuplicateCategory => write!(f, "A category with that name already exists"),
            Self::DuplicateTag => write!(f, "A tag with that name already exists"),
            Self::NotFound => write!(f, "That book is no longer in the library"),
        }
    }
}

impl std::error::Error for DbError {}

impl From<rusqlite::Error> for DbError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sqlite(value)
    }
}

impl From<std::io::Error> for DbError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

pub struct Library {
    conn: Connection,
    covers_dir: PathBuf,
}

impl Library {
    pub fn open(db_path: &Path, covers_dir: &Path) -> Result<Self, DbError> {
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::create_dir_all(covers_dir)?;
        let conn = Connection::open(db_path)?;
        let library = Self {
            conn,
            covers_dir: covers_dir.to_path_buf(),
        };
        library.migrate()?;
        Ok(library)
    }

    pub fn open_default() -> Result<Self, DbError> {
        Self::open(&database_path(), &covers_dir())
    }

    pub fn add_book(&self, draft: &Draft) -> Result<Book, DbError> {
        let tx = self.conn.unchecked_transaction()?;
        let insert = tx.execute(
            "INSERT INTO books (title, author, year, file_path, format)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                draft.title,
                draft.author,
                draft.year,
                none_if_empty(&draft.file_path),
                none_if_empty(&draft.format)
            ],
        );
        if let Err(err) = insert {
            return Err(map_unique(err, UniqueKind::File));
        }
        let id = tx.last_insert_rowid();
        replace_names(&tx, id, &draft.categories, NameKind::Category)?;
        replace_names(&tx, id, &draft.tags, NameKind::Tag)?;
        let cover_path = self.write_cover(id, &draft.cover_png)?;
        tx.execute(
            "UPDATE books SET cover_path = ?1 WHERE id = ?2",
            params![cover_path.to_string_lossy(), id],
        )?;
        tx.commit()?;
        self.book(id)?.ok_or(DbError::NotFound)
    }

    pub fn update_book(&self, id: i64, draft: &Draft) -> Result<Book, DbError> {
        let tx = self.conn.unchecked_transaction()?;
        let updated = tx.execute(
            "UPDATE books
             SET title = ?1, author = ?2, year = ?3, file_path = ?4, format = ?5
             WHERE id = ?6",
            params![
                draft.title,
                draft.author,
                draft.year,
                none_if_empty(&draft.file_path),
                none_if_empty(&draft.format),
                id
            ],
        );
        let updated = match updated {
            Ok(count) => count,
            Err(err) => return Err(map_unique(err, UniqueKind::File)),
        };
        if updated == 0 {
            return Err(DbError::NotFound);
        }
        replace_names(&tx, id, &draft.categories, NameKind::Category)?;
        replace_names(&tx, id, &draft.tags, NameKind::Tag)?;
        let cover_path = self.write_cover(id, &draft.cover_png)?;
        tx.execute(
            "UPDATE books SET cover_path = ?1 WHERE id = ?2",
            params![cover_path.to_string_lossy(), id],
        )?;
        tx.commit()?;
        self.book(id)?.ok_or(DbError::NotFound)
    }

    pub fn delete_book(&self, id: i64) -> Result<(), DbError> {
        let cover = self.book(id)?.and_then(|book| book.cover_path);
        let removed = self
            .conn
            .execute("DELETE FROM books WHERE id = ?1", params![id])?;
        if removed == 0 {
            return Err(DbError::NotFound);
        }
        if let Some(path) = cover {
            let path = PathBuf::from(path);
            if path.starts_with(&self.covers_dir) {
                let _ = fs::remove_file(path);
            }
        }
        Ok(())
    }

    pub fn book(&self, id: i64) -> Result<Option<Book>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, author, year, file_path, format, cover_path
             FROM books WHERE id = ?1",
        )?;
        let row = stmt
            .query_row(params![id], map_book_row)
            .optional()?;
        let Some(mut book) = row else {
            return Ok(None);
        };
        book.categories = self.names_for(book.id, NameKind::Category)?;
        book.tags = self.names_for(book.id, NameKind::Tag)?;
        Ok(Some(book))
    }

    pub fn books(&self, scope: &BookScope) -> Result<Vec<Book>, DbError> {
        let mut stmt;
        let rows = match scope {
            BookScope::All => {
                stmt = self.conn.prepare(
                    "SELECT id, title, author, year, file_path, format, cover_path
                     FROM books
                     ORDER BY title COLLATE NOCASE, id",
                )?;
                stmt.query_map([], map_book_row)?.collect::<Result<Vec<_>, _>>()?
            }
            BookScope::Author(author) => {
                stmt = self.conn.prepare(
                    "SELECT id, title, author, year, file_path, format, cover_path
                     FROM books
                     WHERE author = ?1
                     ORDER BY title COLLATE NOCASE, id",
                )?;
                stmt.query_map(params![author], map_book_row)?
                    .collect::<Result<Vec<_>, _>>()?
            }
            BookScope::Category(name) => {
                stmt = self.conn.prepare(
                    "SELECT b.id, b.title, b.author, b.year, b.file_path, b.format, b.cover_path
                     FROM books b
                     JOIN book_categories bc ON bc.book_id = b.id
                     JOIN categories c ON c.id = bc.category_id
                     WHERE c.name = ?1 COLLATE NOCASE
                     ORDER BY b.title COLLATE NOCASE, b.id",
                )?;
                stmt.query_map(params![name], map_book_row)?
                    .collect::<Result<Vec<_>, _>>()?
            }
            BookScope::Tag(name) => {
                stmt = self.conn.prepare(
                    "SELECT b.id, b.title, b.author, b.year, b.file_path, b.format, b.cover_path
                     FROM books b
                     JOIN book_tags bt ON bt.book_id = b.id
                     JOIN tags t ON t.id = bt.tag_id
                     WHERE t.name = ?1 COLLATE NOCASE
                     ORDER BY b.title COLLATE NOCASE, b.id",
                )?;
                stmt.query_map(params![name], map_book_row)?
                    .collect::<Result<Vec<_>, _>>()?
            }
            BookScope::List(list_id) => {
                stmt = self.conn.prepare(
                    "SELECT b.id, b.title, b.author, b.year, b.file_path, b.format, b.cover_path
                     FROM books b
                     JOIN list_books lb ON lb.book_id = b.id
                     WHERE lb.list_id = ?1
                     ORDER BY lb.position, b.title COLLATE NOCASE, b.id",
                )?;
                stmt.query_map(params![list_id], map_book_row)?
                    .collect::<Result<Vec<_>, _>>()?
            }
        };
        let mut books = rows;
        for book in &mut books {
            book.categories = self.names_for(book.id, NameKind::Category)?;
            book.tags = self.names_for(book.id, NameKind::Tag)?;
        }
        Ok(books)
    }

    pub fn authors(&self) -> Result<Vec<AuthorGroup>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT author, COUNT(*)
             FROM books
             GROUP BY author
             ORDER BY author COLLATE NOCASE",
        )?;
        let groups = stmt
            .query_map([], |row| {
                Ok(AuthorGroup {
                    author: row.get(0)?,
                    count: row.get(1)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(groups)
    }

    pub fn categories(&self) -> Result<Vec<NameCount>, DbError> {
        self.name_counts(NameKind::Category)
    }

    pub fn tags(&self) -> Result<Vec<NameCount>, DbError> {
        self.name_counts(NameKind::Tag)
    }

    pub fn create_category(&self, name: &str) -> Result<(), DbError> {
        self.insert_catalog_name(
            "INSERT INTO categories (name) VALUES (?1)",
            name,
            UniqueKind::Category,
        )
    }

    pub fn create_tag(&self, name: &str) -> Result<(), DbError> {
        self.insert_catalog_name("INSERT INTO tags (name) VALUES (?1)", name, UniqueKind::Tag)
    }

    fn insert_catalog_name(&self, sql: &str, name: &str, kind: UniqueKind) -> Result<(), DbError> {
        match self.conn.execute(sql, params![name]) {
            Ok(_) => Ok(()),
            Err(err) => Err(map_unique(err, kind)),
        }
    }

    fn name_counts(&self, kind: NameKind) -> Result<Vec<NameCount>, DbError> {
        let sql = match kind {
            NameKind::Category => {
                "SELECT c.name, COUNT(bc.book_id)
                 FROM categories c
                 LEFT JOIN book_categories bc ON bc.category_id = c.id
                 GROUP BY c.id
                 ORDER BY c.name COLLATE NOCASE"
            }
            NameKind::Tag => {
                "SELECT t.name, COUNT(bt.book_id)
                 FROM tags t
                 LEFT JOIN book_tags bt ON bt.tag_id = t.id
                 GROUP BY t.id
                 ORDER BY t.name COLLATE NOCASE"
            }
        };
        let mut stmt = self.conn.prepare(sql)?;
        let names = stmt
            .query_map([], |row| {
                Ok(NameCount {
                    name: row.get(0)?,
                    count: row.get(1)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(names)
    }

    pub fn lists(&self) -> Result<Vec<BookList>, DbError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name FROM lists ORDER BY position, id")?;
        let lists = stmt
            .query_map([], |row| {
                Ok(BookList {
                    id: row.get(0)?,
                    name: row.get(1)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(lists)
    }

    pub fn create_list(&self, name: &str) -> Result<BookList, DbError> {
        let insert = self.conn.execute(
            "INSERT INTO lists (name, position)
             VALUES (?1, COALESCE((SELECT MAX(position) FROM lists), 0) + 1)",
            params![name],
        );
        if let Err(err) = insert {
            return Err(map_unique(err, UniqueKind::List));
        }
        let id = self.conn.last_insert_rowid();
        Ok(BookList {
            id,
            name: name.to_string(),
        })
    }

    pub fn rename_list(&self, id: i64, name: &str) -> Result<(), DbError> {
        let result = self.conn.execute(
            "UPDATE lists SET name = ?1 WHERE id = ?2",
            params![name, id],
        );
        match result {
            Ok(0) => Err(DbError::NotFound),
            Ok(_) => Ok(()),
            Err(err) => Err(map_unique(err, UniqueKind::List)),
        }
    }

    pub fn delete_list(&self, id: i64) -> Result<(), DbError> {
        let removed = self
            .conn
            .execute("DELETE FROM lists WHERE id = ?1", params![id])?;
        if removed == 0 {
            Err(DbError::NotFound)
        } else {
            Ok(())
        }
    }

    pub fn add_book_to_list(&self, list_id: i64, book_id: i64) -> Result<bool, DbError> {
        let changed = self.conn.execute(
            "INSERT INTO list_books (list_id, book_id, position)
             VALUES (
                 ?1,
                 ?2,
                 COALESCE((SELECT MAX(position) FROM list_books WHERE list_id = ?1), 0) + 1
             )
             ON CONFLICT(list_id, book_id) DO NOTHING",
            params![list_id, book_id],
        )?;
        Ok(changed == 1)
    }

    pub fn remove_book_from_list(&self, list_id: i64, book_id: i64) -> Result<(), DbError> {
        self.conn.execute(
            "DELETE FROM list_books WHERE list_id = ?1 AND book_id = ?2",
            params![list_id, book_id],
        )?;
        Ok(())
    }

    fn migrate(&self) -> Result<(), DbError> {
        self.conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS books (
                 id INTEGER PRIMARY KEY,
                 title TEXT NOT NULL,
                 author TEXT NOT NULL DEFAULT '',
                 year INTEGER,
                 file_path TEXT UNIQUE,
                 format TEXT CHECK (format IS NULL OR format IN ('pdf', 'epub')),
                 cover_path TEXT
             );
             CREATE TABLE IF NOT EXISTS categories (
                 id INTEGER PRIMARY KEY,
                 name TEXT NOT NULL UNIQUE COLLATE NOCASE
             );
             CREATE TABLE IF NOT EXISTS book_categories (
                 book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
                 category_id INTEGER NOT NULL REFERENCES categories(id) ON DELETE CASCADE,
                 PRIMARY KEY (book_id, category_id)
             );
             CREATE TABLE IF NOT EXISTS tags (
                 id INTEGER PRIMARY KEY,
                 name TEXT NOT NULL UNIQUE COLLATE NOCASE
             );
             CREATE TABLE IF NOT EXISTS book_tags (
                 book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
                 tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
                 PRIMARY KEY (book_id, tag_id)
             );
             CREATE TABLE IF NOT EXISTS lists (
                 id INTEGER PRIMARY KEY,
                 name TEXT NOT NULL UNIQUE COLLATE NOCASE,
                 position INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS list_books (
                 list_id INTEGER NOT NULL REFERENCES lists(id) ON DELETE CASCADE,
                 book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
                 position INTEGER NOT NULL,
                 PRIMARY KEY (list_id, book_id)
             );
             CREATE INDEX IF NOT EXISTS books_author ON books(author);
             CREATE INDEX IF NOT EXISTS books_title ON books(title);",
        )?;
        self.allow_books_without_files()?;
        Ok(())
    }

    fn allow_books_without_files(&self) -> Result<(), DbError> {
        let sql: Option<String> = self
            .conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'books'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let Some(sql) = sql else {
            return Ok(());
        };
        if !sql.contains("file_path TEXT NOT NULL") {
            return Ok(());
        }
        self.conn.execute("PRAGMA foreign_keys = OFF", [])?;
        self.conn.execute_batch(
            "BEGIN;
             CREATE TABLE books_next (
                 id INTEGER PRIMARY KEY,
                 title TEXT NOT NULL,
                 author TEXT NOT NULL DEFAULT '',
                 year INTEGER,
                 file_path TEXT UNIQUE,
                 format TEXT CHECK (format IS NULL OR format IN ('pdf', 'epub')),
                 cover_path TEXT
             );
             INSERT INTO books_next (id, title, author, year, file_path, format, cover_path)
             SELECT id, title, author, year, file_path, format, cover_path FROM books;
             DROP TABLE books;
             ALTER TABLE books_next RENAME TO books;
             CREATE INDEX IF NOT EXISTS books_author ON books(author);
             CREATE INDEX IF NOT EXISTS books_title ON books(title);
             COMMIT;",
        )?;
        self.conn.execute("PRAGMA foreign_keys = ON", [])?;
        Ok(())
    }

    fn names_for(&self, book_id: i64, kind: NameKind) -> Result<Vec<String>, DbError> {
        let sql = match kind {
            NameKind::Category => {
                "SELECT c.name
                 FROM categories c
                 JOIN book_categories bc ON bc.category_id = c.id
                 WHERE bc.book_id = ?1
                 ORDER BY c.name COLLATE NOCASE"
            }
            NameKind::Tag => {
                "SELECT t.name
                 FROM tags t
                 JOIN book_tags bt ON bt.tag_id = t.id
                 WHERE bt.book_id = ?1
                 ORDER BY t.name COLLATE NOCASE"
            }
        };
        let mut stmt = self.conn.prepare(sql)?;
        let names = stmt
            .query_map(params![book_id], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        Ok(names)
    }

    fn write_cover(&self, id: i64, png: &[u8]) -> Result<PathBuf, DbError> {
        fs::create_dir_all(&self.covers_dir)?;
        let path = self.covers_dir.join(format!("{id}.png"));
        if png.is_empty() {
            let _ = fs::remove_file(&path);
            return Ok(path);
        }
        fs::write(&path, png)?;
        Ok(path)
    }

    pub fn cover_directory(&self) -> &Path {
        &self.covers_dir
    }
}

pub fn database_path() -> PathBuf {
    xdg_dir(dirs::data_dir(), ".local/share")
        .join("ebook-reader")
        .join("library.db")
}

pub fn covers_dir() -> PathBuf {
    xdg_dir(dirs::cache_dir(), ".cache")
        .join("ebook-reader")
        .join("covers")
}

fn xdg_dir(primary: Option<PathBuf>, fallback: &str) -> PathBuf {
    primary.unwrap_or_else(|| {
        dirs::home_dir()
            .map(|home| home.join(fallback))
            .unwrap_or_else(|| PathBuf::from("."))
    })
}

pub fn author_label(author: &str) -> String {
    let trimmed = author.trim();
    if trimmed.is_empty() {
        "Unknown".to_string()
    } else {
        trimmed.to_string()
    }
}

pub fn split_names(value: &str) -> Vec<String> {
    let mut names = Vec::new();
    for part in value.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        if names
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(trimmed))
        {
            continue;
        }
        names.push(trimmed.to_string());
    }
    names
}

#[derive(Clone, Copy)]
enum NameKind {
    Category,
    Tag,
}

enum UniqueKind {
    File,
    List,
    Category,
    Tag,
}

fn map_unique(err: rusqlite::Error, kind: UniqueKind) -> DbError {
    let message = err.to_string();
    if message.contains("UNIQUE") {
        match kind {
            UniqueKind::File => DbError::DuplicateFile,
            UniqueKind::List => DbError::DuplicateList,
            UniqueKind::Category => DbError::DuplicateCategory,
            UniqueKind::Tag => DbError::DuplicateTag,
        }
    } else {
        DbError::Sqlite(err)
    }
}

fn none_if_empty(value: &str) -> Option<&str> {
    if value.is_empty() { None } else { Some(value) }
}

fn map_book_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Book> {
    Ok(Book {
        id: row.get(0)?,
        title: row.get(1)?,
        author: row.get(2)?,
        year: row.get(3)?,
        file_path: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
        format: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
        cover_path: row.get(6)?,
        categories: Vec::new(),
        tags: Vec::new(),
    })
}

fn replace_names(
    conn: &Connection,
    book_id: i64,
    names: &[String],
    kind: NameKind,
) -> Result<(), DbError> {
    let (link_delete, insert_name, link_insert) = match kind {
        NameKind::Category => (
            "DELETE FROM book_categories WHERE book_id = ?1",
            "INSERT INTO categories (name) VALUES (?1) ON CONFLICT(name) DO NOTHING",
            "INSERT INTO book_categories (book_id, category_id)
             SELECT ?1, id FROM categories WHERE name = ?2 COLLATE NOCASE",
        ),
        NameKind::Tag => (
            "DELETE FROM book_tags WHERE book_id = ?1",
            "INSERT INTO tags (name) VALUES (?1) ON CONFLICT(name) DO NOTHING",
            "INSERT INTO book_tags (book_id, tag_id)
             SELECT ?1, id FROM tags WHERE name = ?2 COLLATE NOCASE",
        ),
    };
    conn.execute(link_delete, params![book_id])?;
    for name in names {
        conn.execute(insert_name, params![name])?;
        conn.execute(link_insert, params![book_id, name])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_library() -> (Library, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "ebook-reader-test-{}-{}",
            std::process::id(),
            unique_suffix()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let library = Library::open(&dir.join("library.db"), &dir.join("covers")).unwrap();
        (library, dir)
    }

    fn unique_suffix() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64
    }

    fn pathless(title: &str) -> Draft {
        let mut item = draft(title, "", "");
        item.format.clear();
        item
    }

    fn draft(title: &str, author: &str, path: &str) -> Draft {
        Draft {
            title: title.to_string(),
            author: author.to_string(),
            year: Some(2020),
            file_path: path.to_string(),
            format: "epub".to_string(),
            categories: vec!["Fiction".to_string()],
            tags: vec!["reread".to_string()],
            cover_png: b"\x89PNG\r\n\x1a\nfake".to_vec(),
        }
    }

    #[test]
    fn lists_authors_and_duplicate_file() {
        let (library, dir) = temp_library();
        let first = library
            .add_book(&draft("Dune", "Frank Herbert", "/books/dune.epub"))
            .unwrap();
        library
            .add_book(&draft("Messiah", "Frank Herbert", "/books/messiah.epub"))
            .unwrap();
        library
            .add_book(&draft("Kindred", "Octavia Butler", "/books/kindred.epub"))
            .unwrap();

        let duplicate = library.add_book(&draft("Again", "Someone", "/books/dune.epub"));
        assert!(matches!(duplicate, Err(DbError::DuplicateFile)));

        let authors = library.authors().unwrap();
        assert_eq!(authors.len(), 2);
        assert_eq!(authors[0].author, "Frank Herbert");
        assert_eq!(authors[0].count, 2);

        let categories = library.categories().unwrap();
        assert_eq!(categories.len(), 1);
        assert_eq!(categories[0].name, "Fiction");
        assert_eq!(categories[0].count, 3);
        let fiction = library
            .books(&BookScope::Category("fiction".to_string()))
            .unwrap();
        assert_eq!(fiction.len(), 3);
        let tags = library.tags().unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].name, "reread");
        assert_eq!(
            library
                .books(&BookScope::Tag("Reread".to_string()))
                .unwrap()
                .len(),
            3
        );

        let herbert = library
            .books(&BookScope::Author("Frank Herbert".to_string()))
            .unwrap();
        assert_eq!(herbert.len(), 2);
        assert_eq!(herbert[0].categories, vec!["Fiction".to_string()]);
        assert_eq!(herbert[0].tags, vec!["reread".to_string()]);
        assert!(herbert[0].cover_path.as_ref().unwrap().ends_with(".png"));

        let list = library.create_list("Weekend").unwrap();
        assert!(library.add_book_to_list(list.id, first.id).unwrap());
        assert!(!library.add_book_to_list(list.id, first.id).unwrap());
        let members = library.books(&BookScope::List(list.id)).unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].id, first.id);

        library.remove_book_from_list(list.id, first.id).unwrap();
        assert!(library.books(&BookScope::List(list.id)).unwrap().is_empty());

        library.rename_list(list.id, "Later").unwrap();
        assert_eq!(library.lists().unwrap()[0].name, "Later");
        let duplicate_list = library.create_list("Later");
        assert!(matches!(duplicate_list, Err(DbError::DuplicateList)));

        library.delete_book(first.id).unwrap();
        assert!(library.book(first.id).unwrap().is_none());
        assert!(!dir.join("covers").join(format!("{}.png", first.id)).exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn books_without_files_are_allowed() {
        let (library, dir) = temp_library();
        let first = library.add_book(&pathless("Notes")).unwrap();
        let second = library.add_book(&pathless("More notes")).unwrap();
        assert_eq!(first.file_path, "");
        assert_eq!(first.format, "");
        assert_eq!(second.file_path, "");
        let books = library.books(&BookScope::All).unwrap();
        assert_eq!(books.len(), 2);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn old_catalogs_keep_books_when_paths_become_optional() {
        let dir = std::env::temp_dir().join(format!(
            "ebook-reader-migrate-{}-{}",
            std::process::id(),
            unique_suffix()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("library.db");
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE books (
                     id INTEGER PRIMARY KEY,
                     title TEXT NOT NULL,
                     author TEXT NOT NULL DEFAULT '',
                     year INTEGER,
                     file_path TEXT NOT NULL UNIQUE,
                     format TEXT NOT NULL CHECK (format IN ('pdf', 'epub')),
                     cover_path TEXT
                 );
                 INSERT INTO books (title, author, year, file_path, format)
                 VALUES ('Dune', 'Frank Herbert', 1965, '/books/dune.epub', 'epub');",
            )
            .unwrap();
        }
        let library = Library::open(&db_path, &dir.join("covers")).unwrap();
        let books = library.books(&BookScope::All).unwrap();
        assert_eq!(books.len(), 1);
        assert_eq!(books[0].title, "Dune");
        assert_eq!(books[0].file_path, "/books/dune.epub");
        library.add_book(&pathless("Notes")).unwrap();
        library.add_book(&pathless("More")).unwrap();
        assert_eq!(library.books(&BookScope::All).unwrap().len(), 3);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn categories_and_tags_can_exist_without_books() {
        let (library, dir) = temp_library();
        library.create_category("History").unwrap();
        library.create_tag("favorite").unwrap();
        assert!(matches!(
            library.create_category("history"),
            Err(DbError::DuplicateCategory)
        ));
        assert!(matches!(
            library.create_tag("Favorite"),
            Err(DbError::DuplicateTag)
        ));
        assert_eq!(library.categories().unwrap()[0].count, 0);
        assert_eq!(library.tags().unwrap()[0].count, 0);

        let mut item = draft("Dune", "Frank Herbert", "/books/dune.epub");
        item.categories = vec!["History".to_string()];
        item.tags = vec!["favorite".to_string()];
        let book = library.add_book(&item).unwrap();
        assert_eq!(library.categories().unwrap()[0].count, 1);

        item.categories.clear();
        item.tags.clear();
        library.update_book(book.id, &item).unwrap();
        assert_eq!(library.categories().unwrap()[0].name, "History");
        assert_eq!(library.categories().unwrap()[0].count, 0);
        assert_eq!(library.tags().unwrap()[0].count, 0);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn unknown_author_label() {
        assert_eq!(author_label("  "), "Unknown");
        assert_eq!(author_label("Ada"), "Ada");
    }

    #[test]
    fn default_paths_follow_xdg() {
        assert!(database_path().ends_with("ebook-reader/library.db"));
        assert!(covers_dir().ends_with("ebook-reader/covers"));
    }

    #[test]
    fn book_json_uses_camel_case() {
        let book = Book {
            id: 1,
            title: "Dune".to_string(),
            author: "Frank Herbert".to_string(),
            year: Some(1965),
            file_path: "/books/dune.epub".to_string(),
            format: "epub".to_string(),
            cover_path: None,
            categories: vec!["Fiction".to_string()],
            tags: Vec::new(),
        };
        let json = serde_json::to_value(&book).unwrap();
        assert_eq!(json["filePath"], "/books/dune.epub");
        assert!(json.get("file_path").is_none());
    }

    #[test]
    fn split_names_trims_and_dedupes() {
        let names = split_names(" Fiction, history, fiction , ,Science ");
        assert_eq!(names, vec!["Fiction", "history", "Science"]);
    }
}
