# Mar-Elias Books

A Linux bookshelf for local PDF and EPUB files. Covers are shown in a grid, lists accept dragged books, and a double-click opens the file in Okular.

The window is a Tauri shell. The shelf itself is HTML.

## Build

Install WebKitGTK 4.1, GTK 3, Poppler, and Cairo, then run the app:

```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev patchelf libssl-dev libpoppler-glib-dev libcairo2-dev libpango1.0-dev pkg-config build-essential
cargo install tauri-cli --version "^2"
cargo tauri dev
```

Rust 1.92 or newer is required. Okular is used to open books; if it is not installed, the file opens with `xdg-open`.

The catalog is stored in `$XDG_DATA_HOME/ebook-reader/library.db`. Cover thumbnails are stored in `$XDG_CACHE_HOME/ebook-reader/covers/`. Book files stay where they are.

# Storage:
The zoom step is saved in the web view’s localStorage file, not in the book catalog.

On this machine that file is:

/home/elias/.local/share/com.local.books/localstorage/http_tauri.localhost_0.localstorage

That directory is the app data folder for com.local.books ($XDG_DATA_HOME or ~/.local/share). WebKit creates the file the first time the desktop app uses localStorage. It is not there yet. Inside it, the row key is books-zoom, and the value is the step index 0–6 (2 is 100%).

The library database is a different file, ~/.local/share/ebook-reader/library.db.

