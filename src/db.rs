use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
pub struct AuthorGroup {
    pub author: String,
    pub count: i64,
}

#[derive(Debug, Clone)]
pub struct BookList {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone)]
pub enum BookScope {
    All,
    Author(String),
    List(i64),
}

#[derive(Debug)]
pub enum DbError {
    Sqlite(rusqlite::Error),
    Io(std::io::Error),
    DuplicateFile,
    DuplicateList,
    NotFound,
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(err) => write!(f, "{err}"),
            Self::Io(err) => write!(f, "{err}"),
            Self::DuplicateFile => write!(f, "That file is already in the library"),
            Self::DuplicateList => write!(f, "A list with that name already exists"),
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
                draft.file_path,
                draft.format
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
                draft.file_path,
                draft.format,
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
                 file_path TEXT NOT NULL UNIQUE,
                 format TEXT NOT NULL CHECK (format IN ('pdf', 'epub')),
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
}

pub fn database_path() -> PathBuf {
    gtk4::glib::user_data_dir()
        .join("ebook-reader")
        .join("library.db")
}

pub fn covers_dir() -> PathBuf {
    gtk4::glib::user_cache_dir()
        .join("ebook-reader")
        .join("covers")
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
}

fn map_unique(err: rusqlite::Error, kind: UniqueKind) -> DbError {
    let message = err.to_string();
    if message.contains("UNIQUE") {
        match kind {
            UniqueKind::File => DbError::DuplicateFile,
            UniqueKind::List => DbError::DuplicateList,
        }
    } else {
        DbError::Sqlite(err)
    }
}

fn map_book_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Book> {
    Ok(Book {
        id: row.get(0)?,
        title: row.get(1)?,
        author: row.get(2)?,
        year: row.get(3)?,
        file_path: row.get(4)?,
        format: row.get(5)?,
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
    let (link_delete, insert_name, link_insert, orphan_delete) = match kind {
        NameKind::Category => (
            "DELETE FROM book_categories WHERE book_id = ?1",
            "INSERT INTO categories (name) VALUES (?1) ON CONFLICT(name) DO NOTHING",
            "INSERT INTO book_categories (book_id, category_id)
             SELECT ?1, id FROM categories WHERE name = ?2 COLLATE NOCASE",
            "DELETE FROM categories
             WHERE id NOT IN (SELECT category_id FROM book_categories)",
        ),
        NameKind::Tag => (
            "DELETE FROM book_tags WHERE book_id = ?1",
            "INSERT INTO tags (name) VALUES (?1) ON CONFLICT(name) DO NOTHING",
            "INSERT INTO book_tags (book_id, tag_id)
             SELECT ?1, id FROM tags WHERE name = ?2 COLLATE NOCASE",
            "DELETE FROM tags WHERE id NOT IN (SELECT tag_id FROM book_tags)",
        ),
    };
    conn.execute(link_delete, params![book_id])?;
    for name in names {
        conn.execute(insert_name, params![name])?;
        conn.execute(link_insert, params![book_id, name])?;
    }
    conn.execute(orphan_delete, [])?;
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
    fn split_names_trims_and_dedupes() {
        let names = split_names(" Fiction, history, fiction , ,Science ");
        assert_eq!(names, vec!["Fiction", "history", "Science"]);
    }
}
