use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use gtk4::{gdk, gio, glib, GridView, Picture, Stack};

use crate::db::Book;
use crate::metadata::BookFormat;

use super::data;

#[derive(Clone)]
pub struct BookGrid {
    pub stack: Stack,
    pub store: gio::ListStore,
    pub empty: libadwaita::StatusPage,
}

mod imp {
    use std::cell::RefCell;

    use super::*;

    #[derive(Default)]
    pub struct BookObject {
        pub book: RefCell<Option<Book>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for BookObject {
        const NAME: &'static str = "BooksBook";
        type Type = super::BookObject;
        type ParentType = glib::Object;
    }

    impl ObjectImpl for BookObject {}
}

glib::wrapper! {
    pub struct BookObject(ObjectSubclass<imp::BookObject>);
}

impl BookObject {
    pub fn new(book: Book) -> Self {
        let object: Self = glib::Object::builder().build();
        object.imp().book.replace(Some(book));
        object
    }

    pub fn book(&self) -> Book {
        self.imp()
            .book
            .borrow()
            .clone()
            .expect("book object is set before use")
    }
}

pub fn build(
    on_open: impl Fn(i64) + 'static,
    on_menu: impl Fn(i64, f64, f64, gtk4::Widget) + 'static,
) -> BookGrid {
    let on_open = Rc::new(on_open);
    let on_menu = Rc::new(on_menu);
    let factory = gtk4::SignalListItemFactory::new();

    let open_for_setup = on_open.clone();
    let menu_for_setup = on_menu.clone();
    factory.connect_setup(move |_, item| {
        let item = item.downcast_ref::<gtk4::ListItem>().unwrap();
        let tile = cover_tile();
        attach_gestures(&tile, open_for_setup.clone(), menu_for_setup.clone());
        item.set_child(Some(&tile));
    });

    factory.connect_bind(|_, item| {
        let item = item.downcast_ref::<gtk4::ListItem>().unwrap();
        let Some(book_object) = item.item().and_then(|object| object.downcast::<BookObject>().ok()) else {
            return;
        };
        let Some(child) = item.child() else {
            return;
        };
        let book = book_object.book();
        data::set(&child, "book-id", book.id);
        bind_cover(&child, &book);
    });

    let store = gio::ListStore::new::<BookObject>();
    let selection = gtk4::NoSelection::new(Some(store.clone()));
    let view = GridView::builder()
        .model(&selection)
        .factory(&factory)
        .min_columns(2)
        .max_columns(12)
        .single_click_activate(false)
        .vexpand(true)
        .hexpand(true)
        .build();
    view.set_margin_top(8);
    view.set_margin_bottom(16);
    view.set_margin_start(12);
    view.set_margin_end(12);

    let scrolled = gtk4::ScrolledWindow::new();
    scrolled.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scrolled.set_child(Some(&view));
    scrolled.set_vexpand(true);
    scrolled.set_hexpand(true);

    let empty = libadwaita::StatusPage::new();
    empty.set_title("No books");
    empty.set_description(Some("Add a PDF or EPUB to start your library."));
    empty.set_vexpand(true);

    let stack = Stack::new();
    stack.add_named(&scrolled, Some("grid"));
    stack.add_named(&empty, Some("empty"));
    stack.set_vexpand(true);
    stack.set_hexpand(true);

    BookGrid { stack, store, empty }
}

fn cover_tile() -> gtk4::Overlay {
    let frame = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    frame.set_size_request(160, 240);
    frame.add_css_class("cover-frame");

    let picture = Picture::builder()
        .can_shrink(true)
        .content_fit(gtk4::ContentFit::Contain)
        .hexpand(true)
        .vexpand(true)
        .build();
    picture.set_widget_name("cover-picture");
    frame.append(&picture);

    let badge = gtk4::Label::new(None);
    badge.set_widget_name("cover-badge");
    badge.add_css_class("cover-badge");
    badge.set_halign(gtk4::Align::End);
    badge.set_valign(gtk4::Align::End);

    let overlay = gtk4::Overlay::new();
    overlay.set_child(Some(&frame));
    overlay.add_overlay(&badge);
    overlay.set_margin_top(8);
    overlay.set_margin_bottom(8);
    overlay.set_margin_start(8);
    overlay.set_margin_end(8);
    overlay
}

fn attach_gestures(
    tile: &gtk4::Overlay,
    on_open: Rc<dyn Fn(i64)>,
    on_menu: Rc<dyn Fn(i64, f64, f64, gtk4::Widget)>,
) {
    let drag = gtk4::DragSource::new();
    drag.set_actions(gdk::DragAction::COPY);
    drag.connect_prepare(|source, _, _| {
        let widget = source.widget()?;
        let id = data::get_copy::<i64>(&widget, "book-id")?;
        Some(gdk::ContentProvider::for_value(&id.to_value()))
    });
    tile.add_controller(drag);

    let click = gtk4::GestureClick::new();
    click.set_button(gdk::BUTTON_PRIMARY);
    click.connect_released(move |gesture, n_press, _, _| {
        if n_press != 2 {
            return;
        }
        let Some(widget) = gesture.widget() else {
            return;
        };
        let Some(id) = data::get_copy::<i64>(&widget, "book-id") else {
            return;
        };
        on_open(id);
    });
    tile.add_controller(click);

    let menu = gtk4::GestureClick::new();
    menu.set_button(gdk::BUTTON_SECONDARY);
    menu.connect_pressed(move |gesture, _, x, y| {
        gesture.set_state(gtk4::EventSequenceState::Claimed);
        let Some(widget) = gesture.widget() else {
            return;
        };
        let Some(id) = data::get_copy::<i64>(&widget, "book-id") else {
            return;
        };
        on_menu(id, x, y, widget);
    });
    tile.add_controller(menu);
}

fn bind_cover(tile: &gtk4::Widget, book: &Book) {
    if let Some(picture) = find_named(tile, "cover-picture").and_downcast::<Picture>() {
        if let Some(path) = &book.cover_path {
            picture.set_file(Some(&gio::File::for_path(path)));
        } else {
            picture.set_file(None::<&gio::File>);
        }
        picture.set_alternative_text(Some(&book.title));
    }
    if let Some(badge) = find_named(tile, "cover-badge").and_downcast::<gtk4::Label>() {
        let format = if book.format == "epub" {
            BookFormat::Epub
        } else {
            BookFormat::Pdf
        };
        badge.set_text(format.label());
    }
    let author = crate::db::author_label(&book.author);
    tile.set_tooltip_text(Some(&format!("{}\n{author}", book.title)));
}

fn find_named(root: &gtk4::Widget, name: &str) -> Option<gtk4::Widget> {
    let mut pending = vec![root.clone()];
    while let Some(widget) = pending.pop() {
        if widget.widget_name().as_str() == name {
            return Some(widget);
        }
        let mut child = widget.first_child();
        while let Some(next) = child {
            let sibling = next.next_sibling();
            pending.push(next);
            child = sibling;
        }
    }
    None
}
