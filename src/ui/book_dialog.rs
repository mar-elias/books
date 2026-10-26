use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};
use libadwaita as adw;
use libadwaita::prelude::*;

use crate::db::{self, Book, Draft};
use crate::metadata::{self, BookFormat};

pub fn present(
    parent: &adw::ApplicationWindow,
    existing: Option<Book>,
    on_save: impl Fn(Draft) + 'static,
) {
    let dialog = adw::Dialog::new();
    dialog.set_title(if existing.is_some() {
        "Edit book"
    } else {
        "Add book"
    });
    dialog.set_content_width(460);
    dialog.set_content_height(640);

    let header = adw::HeaderBar::new();
    let cancel = gtk4::Button::with_label("Cancel");
    header.pack_start(&cancel);
    let save = gtk4::Button::with_label("Save");
    save.add_css_class("suggested-action");
    save.set_sensitive(existing.is_some());
    header.pack_end(&save);

    let file_button = gtk4::Button::with_label("Choose file");
    let file_label = gtk4::Label::new(Some("No file chosen"));
    file_label.set_xalign(0.0);
    file_label.set_wrap(true);
    file_label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    file_label.add_css_class("dim-label");

    let title = labeled_entry("Title");
    let author = labeled_entry("Author");
    let year = labeled_entry("Year");
    let categories = labeled_entry("Categories");
    categories.set_placeholder_text(Some("Fiction, History"));
    let tags = labeled_entry("Tags");
    tags.set_placeholder_text(Some("unread, favorite"));

    let preview = gtk4::Picture::builder()
        .content_fit(gtk4::ContentFit::Contain)
        .can_shrink(true)
        .width_request(120)
        .height_request(180)
        .build();
    preview.add_css_class("cover-frame");
    let cover_button = gtk4::Button::with_label("Choose cover");
    let status = gtk4::Label::new(None);
    status.set_xalign(0.0);
    status.set_wrap(true);
    status.add_css_class("dim-label");

    let form = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
    form.set_margin_top(12);
    form.set_margin_bottom(16);
    form.set_margin_start(16);
    form.set_margin_end(16);
    form.append(&file_button);
    form.append(&file_label);
    form.append(&field_label("Title"));
    form.append(&title);
    form.append(&field_label("Author"));
    form.append(&author);
    form.append(&field_label("Year"));
    form.append(&year);
    form.append(&field_label("Categories"));
    form.append(&categories);
    form.append(&field_label("Tags"));
    form.append(&tags);
    form.append(&preview);
    form.append(&cover_button);
    form.append(&status);

    let scroller = gtk4::ScrolledWindow::new();
    scroller.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroller.set_child(Some(&form));
    scroller.set_vexpand(true);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&scroller));
    dialog.set_child(Some(&toolbar));

    let state = Rc::new(FormState {
        path: RefCell::new(None),
        format: RefCell::new(None),
        cover: RefCell::new(Vec::new()),
        generation: Cell::new(0),
    });

    if let Some(book) = existing {
        title.set_text(&book.title);
        author.set_text(&book.author);
        if let Some(value) = book.year {
            year.set_text(&value.to_string());
        }
        categories.set_text(&book.categories.join(", "));
        tags.set_text(&book.tags.join(", "));
        file_label.set_text(&book.file_path);
        *state.path.borrow_mut() = Some(PathBuf::from(&book.file_path));
        *state.format.borrow_mut() = match book.format.as_str() {
            "epub" => Some(BookFormat::Epub),
            _ => Some(BookFormat::Pdf),
        };
        if let Some(path) = &book.cover_path {
            if let Ok(bytes) = std::fs::read(path) {
                show_png(&preview, &bytes);
                *state.cover.borrow_mut() = bytes;
            }
        }
    }

    let dialog_cancel = dialog.clone();
    cancel.connect_clicked(move |_| {
        dialog_cancel.close();
    });

    let parent_window = parent.clone();
    let state_for_file = state.clone();
    let file_label_for_file = file_label.clone();
    let title_for_file = title.clone();
    let author_for_file = author.clone();
    let year_for_file = year.clone();
    let preview_for_file = preview.clone();
    let status_for_file = status.clone();
    let save_for_file = save.clone();
    file_button.connect_clicked(move |_| {
        let filters = gio::ListStore::new::<gtk4::FileFilter>();
        let books = gtk4::FileFilter::new();
        books.set_name(Some("PDF and EPUB"));
        books.add_suffix("pdf");
        books.add_suffix("epub");
        books.add_mime_type("application/pdf");
        books.add_mime_type("application/epub+zip");
        filters.append(&books);

        let chooser = gtk4::FileDialog::new();
        chooser.set_title("Choose a book");
        chooser.set_filters(Some(&filters));
        chooser.set_default_filter(Some(&books));

        let state = state_for_file.clone();
        let file_label = file_label_for_file.clone();
        let title = title_for_file.clone();
        let author = author_for_file.clone();
        let year = year_for_file.clone();
        let preview = preview_for_file.clone();
        let status = status_for_file.clone();
        let save = save_for_file.clone();
        chooser.open(
            Some(&parent_window),
            None::<&gio::Cancellable>,
            move |result| {
                let Ok(file) = result else {
                    return;
                };
                let Some(path) = file.path() else {
                    return;
                };
                file_label.set_text(path.to_string_lossy().as_ref());
                *state.path.borrow_mut() = Some(path.clone());
                *state.format.borrow_mut() = BookFormat::from_path(&path);
                state.generation.set(state.generation.get().wrapping_add(1));
                let generation = state.generation.get();
                save.set_sensitive(false);
                status.set_text("Reading the file…");

                let state = state.clone();
                let title = title.clone();
                let author = author.clone();
                let year = year.clone();
                let preview = preview.clone();
                let status = status.clone();
                let save = save.clone();
                glib::spawn_future_local(async move {
                    let extracted = gio::spawn_blocking(move || metadata::extract(&path)).await;
                    if state.generation.get() != generation {
                        return;
                    }
                    match extracted {
                        Ok(Ok(book)) => {
                            title.set_text(&book.title);
                            author.set_text(&book.author);
                            match book.year {
                                Some(value) => year.set_text(&value.to_string()),
                                None => year.set_text(""),
                            }
                            *state.format.borrow_mut() = Some(book.format);
                            *state.cover.borrow_mut() = book.cover_png.clone();
                            show_png(&preview, &book.cover_png);
                            status.set_text("");
                        }
                        Ok(Err(err)) => status.set_text(&err.to_string()),
                        Err(_) => status.set_text("Could not read that file"),
                    }
                    save.set_sensitive(state.path.borrow().is_some());
                });
            },
        );
    });

    let parent_cover = parent.clone();
    let state_cover = state.clone();
    let preview_cover = preview.clone();
    let status_cover = status.clone();
    cover_button.connect_clicked(move |_| {
        let filters = gio::ListStore::new::<gtk4::FileFilter>();
        let images = gtk4::FileFilter::new();
        images.set_name(Some("Images"));
        images.add_mime_type("image/png");
        images.add_mime_type("image/jpeg");
        images.add_mime_type("image/gif");
        images.add_pixbuf_formats();
        filters.append(&images);

        let chooser = gtk4::FileDialog::new();
        chooser.set_title("Choose a cover");
        chooser.set_filters(Some(&filters));
        let state = state_cover.clone();
        let preview = preview_cover.clone();
        let status = status_cover.clone();
        chooser.open(
            Some(&parent_cover),
            None::<&gio::Cancellable>,
            move |result| {
                let Ok(file) = result else {
                    return;
                };
                let Some(path) = file.path() else {
                    return;
                };
                match std::fs::read(&path).and_then(|bytes| {
                    metadata::png_from_bytes(&bytes)
                        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err.to_string()))
                }) {
                    Ok(png) => {
                        show_png(&preview, &png);
                        *state.cover.borrow_mut() = png;
                        status.set_text("");
                    }
                    Err(err) => status.set_text(&err.to_string()),
                }
            },
        );
    });

    let dialog_save = dialog.clone();
    let state_save = state.clone();
    let title_save = title.clone();
    let author_save = author.clone();
    let year_save = year.clone();
    let categories_save = categories.clone();
    let tags_save = tags.clone();
    let status_save = status.clone();
    save.connect_clicked(move |_| {
        let Some(path) = state_save.path.borrow().clone() else {
            status_save.set_text("Choose a PDF or EPUB file.");
            return;
        };
        let Some(format) = *state_save.format.borrow() else {
            status_save.set_text("Choose a PDF or EPUB file.");
            return;
        };
        let year = match parse_year(&year_save.text()) {
            Ok(value) => value,
            Err(()) => {
                status_save.set_text("Year should be a number.");
                return;
            }
        };
        let title_text = {
            let text = title_save.text().trim().to_string();
            if text.is_empty() {
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("Untitled")
                    .to_string()
            } else {
                text
            }
        };
        let author_text = author_save.text().trim().to_string();
        let mut cover = state_save.cover.borrow().clone();
        if cover.is_empty() {
            match metadata::title_card_png(&title_text, &author_text) {
                Ok(png) => cover = png,
                Err(err) => {
                    status_save.set_text(&err.to_string());
                    return;
                }
            }
        }
        let draft = Draft {
            title: title_text,
            author: author_text,
            year,
            file_path: path.to_string_lossy().into_owned(),
            format: format.as_str().to_string(),
            categories: db::split_names(&categories_save.text()),
            tags: db::split_names(&tags_save.text()),
            cover_png: cover,
        };
        dialog_save.close();
        on_save(draft);
    });

    dialog.present(Some(parent));
}

struct FormState {
    path: RefCell<Option<PathBuf>>,
    format: RefCell<Option<BookFormat>>,
    cover: RefCell<Vec<u8>>,
    generation: Cell<u64>,
}

fn labeled_entry(placeholder: &str) -> gtk4::Entry {
    let entry = gtk4::Entry::new();
    entry.set_placeholder_text(Some(placeholder));
    entry.set_hexpand(true);
    entry
}

fn field_label(text: &str) -> gtk4::Label {
    let label = gtk4::Label::new(Some(text));
    label.set_xalign(0.0);
    label.add_css_class("heading");
    label
}

fn show_png(picture: &gtk4::Picture, png: &[u8]) {
    match gdk::Texture::from_bytes(&glib::Bytes::from(png)) {
        Ok(texture) => picture.set_paintable(Some(&texture)),
        Err(_) => picture.set_paintable(None::<&gdk::Texture>),
    }
}

fn parse_year(value: &str) -> Result<Option<i32>, ()> {
    let value = value.trim();
    if value.is_empty() {
        Ok(None)
    } else {
        value.parse::<i32>().map(Some).map_err(|_| ())
    }
}
