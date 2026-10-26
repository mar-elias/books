# Books

A Linux bookshelf for local PDF and EPUB files. Covers are shown in a grid, lists accept dragged books, and a double-click opens the file in Okular.

## Build

Install GTK 4, libadwaita, and Poppler, then run the app:

```bash
sudo apt install libgtk-4-dev libadwaita-1-dev libpoppler-glib-dev pkg-config build-essential
cargo run
```

Rust 1.92 or newer is required. Okular is used to open books; if it is not installed, the file opens in the default application.

The catalog is stored in `$XDG_DATA_HOME/ebook-reader/library.db`. Cover thumbnails are stored in `$XDG_CACHE_HOME/ebook-reader/covers/`. Book files stay where they are.
