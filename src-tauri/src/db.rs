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
    pub list_ids: Vec<i64>,
    pub color: String,
    pub notes: String,
    pub notes_path: String,
    pub added_at: Option<String>,
    pub modified_at: Option<String>,
    pub rating: Option<i32>,
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
    pub color: String,
    pub notes: String,
    pub notes_path: String,
    pub rating: Option<i32>,
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
    pub id: i64,
    pub name: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookList {
    pub id: i64,
    pub name: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShelfCounts {
    pub books: i64,
    pub authors: i64,
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
        let stamp = now_utc();
        let insert = tx.execute(
            "INSERT INTO books (title, author, year, file_path, format, color, notes, notes_path, added_at, modified_at, rating)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                draft.title,
                draft.author,
                draft.year,
                none_if_empty(&draft.file_path),
                none_if_empty(&draft.format),
                none_if_empty(&draft.color),
                none_if_empty(&draft.notes),
                none_if_empty(&draft.notes_path),
                &stamp,
                &stamp,
                draft.rating
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
             SET title = ?1, author = ?2, year = ?3, file_path = ?4, format = ?5, color = ?6, notes = ?7,
                 notes_path = ?8, modified_at = ?9, rating = ?10
             WHERE id = ?11",
            params![
                draft.title,
                draft.author,
                draft.year,
                none_if_empty(&draft.file_path),
                none_if_empty(&draft.format),
                none_if_empty(&draft.color),
                none_if_empty(&draft.notes),
                none_if_empty(&draft.notes_path),
                now_utc(),
                draft.rating,
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
            "SELECT id, title, author, year, file_path, format, cover_path, color, notes, added_at, modified_at, rating, notes_path
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
        book.list_ids = self.list_ids_for(book.id)?;
        Ok(Some(book))
    }

    pub fn books(&self, scope: &BookScope) -> Result<Vec<Book>, DbError> {
        let mut stmt;
        let rows = match scope {
            BookScope::All => {
                stmt = self.conn.prepare(
                    "SELECT id, title, author, year, file_path, format, cover_path, color, notes, added_at, modified_at, rating, notes_path
                     FROM books
                     ORDER BY title COLLATE NOCASE, id",
                )?;
                stmt.query_map([], map_book_row)?.collect::<Result<Vec<_>, _>>()?
            }
            BookScope::Author(author) => {
                stmt = self.conn.prepare(
                    "SELECT id, title, author, year, file_path, format, cover_path, color, notes, added_at, modified_at, rating, notes_path
                     FROM books
                     WHERE author = ?1
                     ORDER BY title COLLATE NOCASE, id",
                )?;
                stmt.query_map(params![author], map_book_row)?
                    .collect::<Result<Vec<_>, _>>()?
            }
            BookScope::Category(name) => {
                stmt = self.conn.prepare(
                    "SELECT b.id, b.title, b.author, b.year, b.file_path, b.format, b.cover_path, b.color, b.notes, b.added_at, b.modified_at, b.rating, b.notes_path
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
                    "SELECT b.id, b.title, b.author, b.year, b.file_path, b.format, b.cover_path, b.color, b.notes, b.added_at, b.modified_at, b.rating, b.notes_path
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
                    "SELECT b.id, b.title, b.author, b.year, b.file_path, b.format, b.cover_path, b.color, b.notes, b.added_at, b.modified_at, b.rating, b.notes_path
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
            book.list_ids = self.list_ids_for(book.id)?;
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

    pub fn create_category(&self, name: &str) -> Result<NameCount, DbError> {
        self.insert_named("categories", name, UniqueKind::Category)
    }

    pub fn create_tag(&self, name: &str) -> Result<NameCount, DbError> {
        self.insert_named("tags", name, UniqueKind::Tag)
    }

    pub fn rename_category(&self, id: i64, name: &str) -> Result<(), DbError> {
        self.rename_named("categories", id, name, UniqueKind::Category)
    }

    pub fn rename_tag(&self, id: i64, name: &str) -> Result<(), DbError> {
        self.rename_named("tags", id, name, UniqueKind::Tag)
    }

    pub fn delete_category(&self, id: i64) -> Result<(), DbError> {
        self.delete_named("categories", id)
    }

    pub fn delete_tag(&self, id: i64) -> Result<(), DbError> {
        self.delete_named("tags", id)
    }

    pub fn move_category(&self, id: i64, direction: i32) -> Result<(), DbError> {
        self.move_named("categories", id, direction)
    }

    pub fn move_tag(&self, id: i64, direction: i32) -> Result<(), DbError> {
        self.move_named("tags", id, direction)
    }

    pub fn move_list(&self, id: i64, direction: i32) -> Result<(), DbError> {
        self.move_named("lists", id, direction)
    }

    pub fn sort_category(&self, by_name: bool) -> Result<(), DbError> {
        self.sort_named("categories", by_name)
    }

    pub fn sort_tag(&self, by_name: bool) -> Result<(), DbError> {
        self.sort_named("tags", by_name)
    }

    pub fn sort_list(&self, by_name: bool) -> Result<(), DbError> {
        self.sort_named("lists", by_name)
    }

    fn insert_named(&self, table: &str, name: &str, kind: UniqueKind) -> Result<NameCount, DbError> {
        let sql = format!(
            "INSERT INTO {table} (name, position)
             VALUES (?1, COALESCE((SELECT MAX(position) FROM {table}), 0) + 1)"
        );
        match self.conn.execute(&sql, params![name]) {
            Ok(_) => Ok(NameCount {
                id: self.conn.last_insert_rowid(),
                name: name.to_string(),
                count: 0,
            }),
            Err(err) => Err(map_unique(err, kind)),
        }
    }

    fn rename_named(
        &self,
        table: &str,
        id: i64,
        name: &str,
        kind: UniqueKind,
    ) -> Result<(), DbError> {
        let result = self.conn.execute(
            &format!("UPDATE {table} SET name = ?1 WHERE id = ?2"),
            params![name, id],
        );
        match result {
            Ok(0) => Err(DbError::NotFound),
            Ok(_) => Ok(()),
            Err(err) => Err(map_unique(err, kind)),
        }
    }

    fn delete_named(&self, table: &str, id: i64) -> Result<(), DbError> {
        let removed = self
            .conn
            .execute(&format!("DELETE FROM {table} WHERE id = ?1"), params![id])?;
        if removed == 0 {
            Err(DbError::NotFound)
        } else {
            Ok(())
        }
    }

    fn move_named(&self, table: &str, id: i64, direction: i32) -> Result<(), DbError> {
        let mut ids = self.ordered_ids(table)?;
        let Some(index) = ids.iter().position(|item| *item == id) else {
            return Err(DbError::NotFound);
        };
        let target = index as isize + direction as isize;
        if target < 0 || target >= ids.len() as isize {
            return Ok(());
        }
        ids.swap(index, target as usize);
        self.write_positions(table, &ids)
    }

    fn sort_named(&self, table: &str, by_name: bool) -> Result<(), DbError> {
        let order = if by_name {
            "name COLLATE NOCASE, id"
        } else {
            "id"
        };
        let ids = {
            let mut stmt = self
                .conn
                .prepare(&format!("SELECT id FROM {table} ORDER BY {order}"))?;
            stmt.query_map([], |row| row.get(0))?
                .collect::<Result<Vec<i64>, _>>()?
        };
        self.write_positions(table, &ids)
    }

    fn write_positions(&self, table: &str, ids: &[i64]) -> Result<(), DbError> {
        let tx = self.conn.unchecked_transaction()?;
        for (position, item_id) in ids.iter().enumerate() {
            tx.execute(
                &format!("UPDATE {table} SET position = ?1 WHERE id = ?2"),
                params![position as i64, item_id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    fn ordered_ids(&self, table: &str) -> Result<Vec<i64>, DbError> {
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT id FROM {table} ORDER BY position, id"))?;
        let ids = stmt
            .query_map([], |row| row.get(0))?
            .collect::<Result<Vec<i64>, _>>()?;
        Ok(ids)
    }

    fn name_counts(&self, kind: NameKind) -> Result<Vec<NameCount>, DbError> {
        let sql = match kind {
            NameKind::Category => {
                "SELECT c.id, c.name, COUNT(bc.book_id)
                 FROM categories c
                 LEFT JOIN book_categories bc ON bc.category_id = c.id
                 GROUP BY c.id
                 ORDER BY c.position, c.id"
            }
            NameKind::Tag => {
                "SELECT t.id, t.name, COUNT(bt.book_id)
                 FROM tags t
                 LEFT JOIN book_tags bt ON bt.tag_id = t.id
                 GROUP BY t.id
                 ORDER BY t.position, t.id"
            }
        };
        let mut stmt = self.conn.prepare(sql)?;
        let names = stmt
            .query_map([], |row| {
                Ok(NameCount {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    count: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(names)
    }

    pub fn lists(&self) -> Result<Vec<BookList>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT l.id, l.name, COUNT(lb.book_id)
             FROM lists l
             LEFT JOIN list_books lb ON lb.list_id = l.id
             GROUP BY l.id
             ORDER BY l.position, l.id",
        )?;
        let lists = stmt
            .query_map([], |row| {
                Ok(BookList {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    count: row.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(lists)
    }

    pub fn shelf_counts(&self) -> Result<ShelfCounts, DbError> {
        let books = self
            .conn
            .query_row("SELECT COUNT(*) FROM books", [], |row| row.get(0))?;
        let authors = self.conn.query_row(
            "SELECT COUNT(DISTINCT author) FROM books",
            [],
            |row| row.get(0),
        )?;
        Ok(ShelfCounts { books, authors })
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
            count: 0,
        })
    }

    pub fn rename_list(&self, id: i64, name: &str) -> Result<(), DbError> {
        self.rename_named("lists", id, name, UniqueKind::List)
    }

    pub fn delete_list(&self, id: i64) -> Result<(), DbError> {
        self.delete_named("lists", id)
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
                 cover_path TEXT,
                 color TEXT,
                 notes TEXT,
                 notes_path TEXT,
                 added_at TEXT,
                 modified_at TEXT,
                 rating INTEGER
             );
             CREATE TABLE IF NOT EXISTS categories (
                 id INTEGER PRIMARY KEY,
                 name TEXT NOT NULL UNIQUE COLLATE NOCASE,
                 position INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS book_categories (
                 book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
                 category_id INTEGER NOT NULL REFERENCES categories(id) ON DELETE CASCADE,
                 PRIMARY KEY (book_id, category_id)
             );
             CREATE TABLE IF NOT EXISTS tags (
                 id INTEGER PRIMARY KEY,
                 name TEXT NOT NULL UNIQUE COLLATE NOCASE,
                 position INTEGER NOT NULL DEFAULT 0
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
        self.add_book_color()?;
        self.add_book_notes()?;
        self.add_book_notes_path()?;
        self.add_catalog_positions()?;
        self.add_book_dates()?;
        self.add_book_rating()?;
        Ok(())
    }

    fn add_book_rating(&self) -> Result<(), DbError> {
        if self.has_column("books", "rating")? {
            return Ok(());
        }
        self.conn
            .execute("ALTER TABLE books ADD COLUMN rating INTEGER", [])?;
        Ok(())
    }

    fn add_book_dates(&self) -> Result<(), DbError> {
        if !self.has_column("books", "added_at")? {
            self.conn
                .execute("ALTER TABLE books ADD COLUMN added_at TEXT", [])?;
        }
        if !self.has_column("books", "modified_at")? {
            self.conn
                .execute("ALTER TABLE books ADD COLUMN modified_at TEXT", [])?;
        }
        Ok(())
    }

    fn add_catalog_positions(&self) -> Result<(), DbError> {
        self.add_position_column("categories")?;
        self.add_position_column("tags")?;
        Ok(())
    }

    fn add_position_column(&self, table: &str) -> Result<(), DbError> {
        if self.has_column(table, "position")? {
            return Ok(());
        }
        self.conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN position INTEGER NOT NULL DEFAULT 0"),
            [],
        )?;
        let ids = {
            let mut stmt = self.conn.prepare(&format!(
                "SELECT id FROM {table} ORDER BY name COLLATE NOCASE, id"
            ))?;
            stmt.query_map([], |row| row.get(0))?
                .collect::<Result<Vec<i64>, _>>()?
        };
        let tx = self.conn.unchecked_transaction()?;
        for (position, id) in ids.iter().enumerate() {
            tx.execute(
                &format!("UPDATE {table} SET position = ?1 WHERE id = ?2"),
                params![position as i64, id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    fn has_column(&self, table: &str, column: &str) -> Result<bool, DbError> {
        let mut stmt = self
            .conn
            .prepare(&format!("PRAGMA table_info({table})"))?;
        let names = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(names.iter().any(|name| name == column))
    }

    fn add_book_notes_path(&self) -> Result<(), DbError> {
        if self.has_column("books", "notes_path")? {
            return Ok(());
        }
        self.conn
            .execute("ALTER TABLE books ADD COLUMN notes_path TEXT", [])?;
        Ok(())
    }

    fn add_book_notes(&self) -> Result<(), DbError> {
        let mut stmt = self.conn.prepare("PRAGMA table_info(books)")?;
        let columns = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?;
        if columns.iter().any(|name| name == "notes") {
            return Ok(());
        }
        self.conn.execute("ALTER TABLE books ADD COLUMN notes TEXT", [])?;
        Ok(())
    }

    fn add_book_color(&self) -> Result<(), DbError> {
        let mut stmt = self.conn.prepare("PRAGMA table_info(books)")?;
        let columns = stmt
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<Result<Vec<_>, _>>()?;
        if columns.iter().any(|name| name == "color") {
            return Ok(());
        }
        self.conn.execute("ALTER TABLE books ADD COLUMN color TEXT", [])?;
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

    fn list_ids_for(&self, book_id: i64) -> Result<Vec<i64>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT list_id FROM list_books WHERE book_id = ?1 ORDER BY position, list_id",
        )?;
        let ids = stmt
            .query_map(params![book_id], |row| row.get(0))?
            .collect::<Result<Vec<i64>, _>>()?;
        Ok(ids)
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

pub fn settings_path() -> PathBuf {
    xdg_dir(dirs::config_dir(), ".config")
        .join("ebook-reader")
        .join("settings.json")
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
        list_ids: Vec::new(),
        color: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
        notes: row.get::<_, Option<String>>(8)?.unwrap_or_default(),
        added_at: row.get(9)?,
        modified_at: row.get(10)?,
        rating: row.get(11)?,
        notes_path: row.get::<_, Option<String>>(12)?.unwrap_or_default(),
    })
}

fn now_utc() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0);
    let seconds = millis.div_euclid(1000);
    let fraction = millis.rem_euclid(1000);
    let stamp = format_utc(seconds);
    format!("{}.{fraction:03}Z", &stamp[..stamp.len() - 1])
}

fn format_utc(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let time = seconds.rem_euclid(86_400) as u32;
    let hour = time / 3600;
    let minute = (time % 3600) / 60;
    let second = time % 60;
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let year = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let month_index = (5 * doy + 2) / 153;
    let day = doy - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 { month_index + 3 } else { month_index - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    (year as i32, month as u32, day as u32)
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
            "INSERT INTO categories (name, position)
             VALUES (?1, COALESCE((SELECT MAX(position) FROM categories), 0) + 1)
             ON CONFLICT(name) DO NOTHING",
            "INSERT INTO book_categories (book_id, category_id)
             SELECT ?1, id FROM categories WHERE name = ?2 COLLATE NOCASE",
        ),
        NameKind::Tag => (
            "DELETE FROM book_tags WHERE book_id = ?1",
            "INSERT INTO tags (name, position)
             VALUES (?1, COALESCE((SELECT MAX(position) FROM tags), 0) + 1)
             ON CONFLICT(name) DO NOTHING",
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
            color: String::new(),
            notes: String::new(),
            notes_path: String::new(),
            rating: None,
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
        assert_eq!(list.count, 0);
        assert!(library.add_book_to_list(list.id, first.id).unwrap());
        assert!(!library.add_book_to_list(list.id, first.id).unwrap());
        assert_eq!(library.lists().unwrap()[0].count, 1);
        let counts = library.shelf_counts().unwrap();
        assert_eq!(counts.books, 3);
        assert_eq!(counts.authors, 2);
        let members = library.books(&BookScope::List(list.id)).unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].id, first.id);
        assert_eq!(members[0].list_ids, vec![list.id]);

        library.remove_book_from_list(list.id, first.id).unwrap();
        assert_eq!(library.lists().unwrap()[0].count, 0);
        assert!(library.book(first.id).unwrap().unwrap().list_ids.is_empty());
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
    fn notes_path_is_stored_and_can_be_cleared() {
        let (library, dir) = temp_library();
        let mut item = pathless("Commented");
        item.notes_path = "/notes/dune.txt".to_string();
        let saved = library.add_book(&item).unwrap();
        assert_eq!(saved.notes_path, "/notes/dune.txt");
        assert_eq!(
            library.book(saved.id).unwrap().unwrap().notes_path,
            "/notes/dune.txt"
        );
        item.notes_path.clear();
        let updated = library.update_book(saved.id, &item).unwrap();
        assert_eq!(updated.notes_path, "");
        let _ = fs::remove_dir_all(dir);
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
        assert!(books[0].added_at.is_none());
        assert!(books[0].modified_at.is_none());
        library.add_book(&pathless("Notes")).unwrap();
        library.add_book(&pathless("More")).unwrap();
        assert_eq!(library.books(&BookScope::All).unwrap().len(), 3);
        assert_eq!(library.books(&BookScope::All).unwrap()[0].color, "");
        assert_eq!(library.books(&BookScope::All).unwrap()[0].notes, "");
        assert_eq!(library.books(&BookScope::All).unwrap()[0].notes_path, "");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn book_notes_are_saved_and_cleared() {
        let (library, dir) = temp_library();
        let mut item = draft("Dune", "Frank Herbert", "/books/dune.epub");
        item.notes = "Read the appendix.".to_string();
        let book = library.add_book(&item).unwrap();
        assert_eq!(book.notes, "Read the appendix.");
        assert_eq!(
            library.books(&BookScope::All).unwrap()[0].notes,
            "Read the appendix."
        );
        item.notes.clear();
        let updated = library.update_book(book.id, &item).unwrap();
        assert_eq!(updated.notes, "");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn book_dates_are_set_on_add_and_modified_on_save() {
        let (library, dir) = temp_library();
        let book = library
            .add_book(&draft("Dune", "Frank Herbert", "/books/dune.epub"))
            .unwrap();
        let added = book.added_at.clone().expect("date added");
        let modified = book.modified_at.clone().expect("date modified");
        assert_eq!(added, modified);
        assert!(added.contains('T') && added.ends_with('Z'));
        std::thread::sleep(std::time::Duration::from_millis(5));
        let saved = library
            .update_book(book.id, &draft("Dune", "Frank Herbert", "/books/dune.epub"))
            .unwrap();
        assert_eq!(saved.added_at.as_deref(), Some(added.as_str()));
        let next_modified = saved.modified_at.expect("date modified");
        assert!(next_modified > added);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn book_rating_is_saved_and_cleared() {
        let (library, dir) = temp_library();
        let mut item = draft("Dune", "Frank Herbert", "/books/dune.epub");
        item.rating = Some(4);
        let book = library.add_book(&item).unwrap();
        assert_eq!(book.rating, Some(4));
        item.rating = Some(0);
        let zero = library.update_book(book.id, &item).unwrap();
        assert_eq!(zero.rating, Some(0));
        assert_eq!(zero.added_at, book.added_at);
        item.rating = None;
        let cleared = library.update_book(book.id, &item).unwrap();
        assert_eq!(cleared.rating, None);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn utc_stamp_formats_known_instants() {
        assert_eq!(format_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_utc(1_577_923_200), "2020-01-01T00:00:00Z");
        assert_eq!(format_utc(1_577_923_200 + 3661), "2020-01-01T01:01:01Z");
    }

    #[test]
    fn catalogs_without_notes_gain_the_column() {
        let dir = std::env::temp_dir().join(format!(
            "ebook-reader-notes-{}-{}",
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
                     file_path TEXT UNIQUE,
                     format TEXT CHECK (format IS NULL OR format IN ('pdf', 'epub')),
                     cover_path TEXT,
                     color TEXT
                 );
                 INSERT INTO books (title, author, color)
                 VALUES ('Dune', 'Frank Herbert', '#fa517e');",
            )
            .unwrap();
        }
        let library = Library::open(&db_path, &dir.join("covers")).unwrap();
        let books = library.books(&BookScope::All).unwrap();
        assert_eq!(books[0].title, "Dune");
        assert_eq!(books[0].color, "#fa517e");
        assert_eq!(books[0].notes, "");
        let mut item = draft("Dune", "Frank Herbert", "/books/dune.epub");
        item.color = "#fa517e".to_string();
        item.notes = "Keep the map.".to_string();
        let updated = library.update_book(books[0].id, &item).unwrap();
        assert_eq!(updated.notes, "Keep the map.");
        assert_eq!(updated.color, "#fa517e");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn book_color_is_saved_and_cleared() {
        let (library, dir) = temp_library();
        let mut item = draft("Dune", "Frank Herbert", "/books/dune.epub");
        item.color = "#fa517e".to_string();
        let book = library.add_book(&item).unwrap();
        assert_eq!(book.color, "#fa517e");
        assert_eq!(library.books(&BookScope::All).unwrap()[0].color, "#fa517e");
        item.color.clear();
        let updated = library.update_book(book.id, &item).unwrap();
        assert_eq!(updated.color, "");
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
    fn catalog_order_rename_and_delete() {
        let dir = std::env::temp_dir().join(format!(
            "ebook-reader-order-{}-{}",
            std::process::id(),
            unique_suffix()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("library.db");
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE categories (
                     id INTEGER PRIMARY KEY,
                     name TEXT NOT NULL UNIQUE COLLATE NOCASE
                 );
                 INSERT INTO categories (name) VALUES ('Zebra');
                 INSERT INTO categories (name) VALUES ('Alpha');
                 INSERT INTO categories (name) VALUES ('Middle');
                 CREATE TABLE tags (
                     id INTEGER PRIMARY KEY,
                     name TEXT NOT NULL UNIQUE COLLATE NOCASE
                 );
                 INSERT INTO tags (name) VALUES ('zeta');
                 INSERT INTO tags (name) VALUES ('alpha');",
            )
            .unwrap();
        }
        let library = Library::open(&db_path, &dir.join("covers")).unwrap();
        let names = |items: Vec<NameCount>| items.into_iter().map(|item| item.name).collect::<Vec<_>>();
        assert_eq!(
            names(library.categories().unwrap()),
            vec!["Alpha", "Middle", "Zebra"]
        );
        assert_eq!(names(library.tags().unwrap()), vec!["alpha", "zeta"]);

        let middle = library
            .categories()
            .unwrap()
            .into_iter()
            .find(|item| item.name == "Middle")
            .unwrap();
        library.move_category(middle.id, -1).unwrap();
        assert_eq!(
            names(library.categories().unwrap()),
            vec!["Middle", "Alpha", "Zebra"]
        );
        let first = library.categories().unwrap()[0].id;
        library.move_category(first, -1).unwrap();
        assert_eq!(
            names(library.categories().unwrap()),
            vec!["Middle", "Alpha", "Zebra"]
        );

        let gamma = library.create_category("Gamma").unwrap();
        assert_eq!(gamma.count, 0);
        assert_eq!(
            names(library.categories().unwrap()),
            vec!["Middle", "Alpha", "Zebra", "Gamma"]
        );
        let alpha = library
            .categories()
            .unwrap()
            .into_iter()
            .find(|item| item.name == "Alpha")
            .unwrap();
        library.rename_category(alpha.id, "History").unwrap();
        assert!(matches!(
            library.rename_category(alpha.id, "middle"),
            Err(DbError::DuplicateCategory)
        ));

        let mut item = draft("Dune", "Frank Herbert", "/books/dune.epub");
        item.categories = vec!["History".to_string(), "Gamma".to_string()];
        item.tags = vec!["later".to_string()];
        let book = library.add_book(&item).unwrap();
        assert_eq!(
            names(library.categories().unwrap()),
            vec!["Middle", "History", "Zebra", "Gamma"]
        );
        assert_eq!(
            names(library.tags().unwrap()),
            vec!["alpha", "zeta", "later"]
        );
        library.delete_category(gamma.id).unwrap();
        let stored = library.book(book.id).unwrap().unwrap();
        assert_eq!(stored.categories, vec!["History".to_string()]);
        assert!(matches!(
            library.delete_category(gamma.id),
            Err(DbError::NotFound)
        ));

        let first_list = library.create_list("First").unwrap();
        library.create_list("Second").unwrap();
        let third = library.create_list("Third").unwrap();
        library.move_list(third.id, -1).unwrap();
        library.move_list(first_list.id, -1).unwrap();
        let lists = library.lists().unwrap();
        assert_eq!(
            lists.iter().map(|list| list.name.as_str()).collect::<Vec<_>>(),
            vec!["First", "Third", "Second"]
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn catalog_sorts_by_name_or_date_added() {
        let (library, dir) = temp_library();
        library.create_category("Zebra").unwrap();
        library.create_category("Alpha").unwrap();
        library.create_category("Middle").unwrap();
        library.sort_category(true).unwrap();
        let names = |items: Vec<NameCount>| {
            items.into_iter().map(|item| item.name).collect::<Vec<_>>()
        };
        assert_eq!(
            names(library.categories().unwrap()),
            vec!["Alpha", "Middle", "Zebra"]
        );
        library.sort_category(false).unwrap();
        assert_eq!(
            names(library.categories().unwrap()),
            vec!["Zebra", "Alpha", "Middle"]
        );

        library.create_list("Weekend").unwrap();
        library.create_list("Later").unwrap();
        library.sort_list(true).unwrap();
        assert_eq!(
            library
                .lists()
                .unwrap()
                .into_iter()
                .map(|list| list.name)
                .collect::<Vec<_>>(),
            vec!["Later".to_string(), "Weekend".to_string()]
        );
        library.sort_list(false).unwrap();
        assert_eq!(
            library
                .lists()
                .unwrap()
                .into_iter()
                .map(|list| list.name)
                .collect::<Vec<_>>(),
            vec!["Weekend".to_string(), "Later".to_string()]
        );
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
        assert!(settings_path().ends_with("ebook-reader/settings.json"));
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
            list_ids: Vec::new(),
            color: String::new(),
            notes: String::new(),
            notes_path: "/notes/dune.txt".to_string(),
            added_at: Some("2026-11-16T12:00:00.000Z".to_string()),
            modified_at: Some("2026-11-16T12:04:00.000Z".to_string()),
            rating: Some(4),
        };
        let json = serde_json::to_value(&book).unwrap();
        assert_eq!(json["filePath"], "/books/dune.epub");
        assert_eq!(json["addedAt"], "2026-11-16T12:00:00.000Z");
        assert_eq!(json["modifiedAt"], "2026-11-16T12:04:00.000Z");
        assert_eq!(json["rating"], 4);
        assert_eq!(json["notesPath"], "/notes/dune.txt");
        assert!(json.get("file_path").is_none());
    }

    #[test]
    fn split_names_trims_and_dedupes() {
        let names = split_names(" Fiction, history, fiction , ,Science ");
        assert_eq!(names, vec!["Fiction", "history", "Science"]);
    }
}
