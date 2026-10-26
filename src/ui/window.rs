use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};
use libadwaita as adw;

use crate::db::{self, AuthorGroup, Book, BookScope, DbError, Library};

use super::{book_dialog, data, grid, prompt, sidebar};

#[derive(Clone)]
enum View {
    All,
    Authors,
    Author(String),
    List { id: i64, name: String },
}

struct Shell {
    db: Library,
    view: View,
    search: String,
}

#[derive(Clone)]
struct Ui {
    window: adw::ApplicationWindow,
    toast: adw::ToastOverlay,
    title: adw::WindowTitle,
    back: gtk4::Button,
    stack: gtk4::Stack,
    grid: grid::BookGrid,
    author_list: gtk4::ListBox,
    library: gtk4::ListBox,
    lists: gtk4::ListBox,
    all_row: gtk4::ListBoxRow,
    authors_row: gtk4::ListBoxRow,
    suppress: Rc<Cell<bool>>,
}

pub fn build(app: &adw::Application) -> Result<(), DbError> {
    let state = Rc::new(std::cell::RefCell::new(Shell {
        db: Library::open_default()?,
        view: View::All,
        search: String::new(),
    }));

    let sidebar = sidebar::build();
    let ui_slot: Rc<std::cell::RefCell<Option<Ui>>> = Rc::new(std::cell::RefCell::new(None));

    let state_for_open = state.clone();
    let slot_for_open = ui_slot.clone();
    let state_for_menu = state.clone();
    let slot_for_menu = ui_slot.clone();
    let books = grid::build(
        move |id| open_id(&state_for_open, &slot_for_open, id),
        move |id, x, y, widget| show_book_menu(&state_for_menu, &slot_for_menu, id, x, y, &widget),
    );

    let author_list = gtk4::ListBox::new();
    author_list.add_css_class("navigation-sidebar");
    author_list.set_selection_mode(gtk4::SelectionMode::None);
    let author_scroll = gtk4::ScrolledWindow::new();
    author_scroll.set_child(Some(&author_list));
    author_scroll.set_vexpand(true);

    let content_stack = gtk4::Stack::new();
    content_stack.add_named(&books.stack, Some("books"));
    content_stack.add_named(&author_scroll, Some("authors"));
    content_stack.set_vexpand(true);
    content_stack.set_hexpand(true);

    let title = adw::WindowTitle::new("All books", "");
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&title));
    let back = gtk4::Button::from_icon_name("go-previous-symbolic");
    back.set_tooltip_text(Some("Back to authors"));
    back.set_visible(false);
    header.pack_start(&back);

    let add = gtk4::Button::with_label("Add book");
    add.add_css_class("suggested-action");
    header.pack_end(&add);

    let search = gtk4::SearchEntry::new();
    search.set_placeholder_text(Some("Search"));
    search.set_width_chars(22);
    header.pack_end(&search);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&content_stack));

    let split = adw::NavigationSplitView::new();
    split.set_min_sidebar_width(220.0);
    split.set_max_sidebar_width(280.0);
    split.set_sidebar_width_fraction(0.22);
    let sidebar_page = adw::NavigationPage::new(&sidebar.root, "Library");
    let content_page = adw::NavigationPage::new(&toolbar, "Books");
    split.set_sidebar(Some(&sidebar_page));
    split.set_content(Some(&content_page));

    let toast = adw::ToastOverlay::new();
    toast.set_child(Some(&split));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Books")
        .default_width(1100)
        .default_height(720)
        .content(&toast)
        .build();

    let ui = Ui {
        window: window.clone(),
        toast: toast.clone(),
        title: title.clone(),
        back: back.clone(),
        stack: content_stack,
        grid: books,
        author_list: author_list.clone(),
        library: sidebar.library.clone(),
        lists: sidebar.lists.clone(),
        all_row: sidebar.all_row.clone(),
        authors_row: sidebar.authors_row.clone(),
        suppress: Rc::new(Cell::new(false)),
    };
    *ui_slot.borrow_mut() = Some(ui.clone());

    let state_library = state.clone();
    let ui_library = ui.clone();
    sidebar.library.connect_row_selected(move |_, row| {
        if ui_library.suppress.get() {
            return;
        }
        let Some(row) = row else {
            return;
        };
        let Some(kind) = data::get_copy::<&'static str>(row, "nav") else {
            return;
        };
        state_library.borrow_mut().view = if kind == "authors" {
            View::Authors
        } else {
            View::All
        };
        refresh(&state_library, &ui_library);
    });

    let state_lists = state.clone();
    let ui_lists = ui.clone();
    sidebar.lists.connect_row_selected(move |_, row| {
        if ui_lists.suppress.get() {
            return;
        }
        let Some(row) = row else {
            return;
        };
        let Some(id) = data::get_copy::<i64>(row, "list-id") else {
            return;
        };
        let name = data::get_clone::<String>(row, "list-name").unwrap_or_default();
        state_lists.borrow_mut().view = View::List { id, name };
        refresh(&state_lists, &ui_lists);
    });

    let state_authors = state.clone();
    let ui_authors = ui.clone();
    author_list.connect_row_activated(move |_, row| {
        let Some(author) = data::get_clone::<String>(row, "author-key") else {
            return;
        };
        state_authors.borrow_mut().view = View::Author(author);
        refresh(&state_authors, &ui_authors);
    });

    let state_back = state.clone();
    let ui_back = ui.clone();
    back.connect_clicked(move |_| {
        state_back.borrow_mut().view = View::Authors;
        refresh(&state_back, &ui_back);
    });

    let state_search = state.clone();
    let ui_search = ui.clone();
    search.connect_search_changed(move |entry| {
        state_search.borrow_mut().search = entry.text().to_string();
        refresh(&state_search, &ui_search);
    });

    let state_add = state.clone();
    let ui_add = ui.clone();
    add.connect_clicked(move |_| {
        let state = state_add.clone();
        let ui = ui_add.clone();
        let window = ui.window.clone();
        book_dialog::present(&window, None, move |draft| {
            match state.borrow().db.add_book(&draft) {
                Ok(_) => refresh(&state, &ui),
                Err(err) => show_toast(&ui, &err.to_string()),
            }
        });
    });

    let state_new = state.clone();
    let ui_new = ui.clone();
    let window_new = window.clone();
    sidebar.new_list.connect_clicked(move |_| {
        let state = state_new.clone();
        let ui = ui_new.clone();
        prompt::prompt(&window_new, "New list", "Create", "", move |name| {
            match state.borrow().db.create_list(&name) {
                Ok(list) => {
                    state.borrow_mut().view = View::List {
                        id: list.id,
                        name: list.name,
                    };
                    refresh(&state, &ui);
                }
                Err(err) => show_toast(&ui, &err.to_string()),
            }
        });
    });

    refresh(&state, &ui);
    window.present();
    Ok(())
}

fn refresh(state: &Rc<std::cell::RefCell<Shell>>, ui: &Ui) {
    ui.suppress.set(true);

    let (view, search, books, authors, lists) = {
        let shell = state.borrow();
        let scope = match &shell.view {
            View::All | View::Authors => BookScope::All,
            View::Author(author) => BookScope::Author(author.clone()),
            View::List { id, .. } => BookScope::List(*id),
        };
        let books = shell.db.books(&scope).unwrap_or_default();
        let authors = shell.db.authors().unwrap_or_default();
        let lists = shell.db.lists().unwrap_or_default();
        (shell.view.clone(), shell.search.clone(), books, authors, lists)
    };

    rebuild_lists(state, ui, &lists, &view);
    match &view {
        View::All => {
            ui.library.select_row(Some(&ui.all_row));
            ui.lists.unselect_all();
        }
        View::Authors | View::Author(_) => {
            ui.library.select_row(Some(&ui.authors_row));
            ui.lists.unselect_all();
        }
        View::List { id, .. } => {
            ui.library.unselect_all();
            select_list_row(&ui.lists, *id);
        }
    }
    ui.suppress.set(false);

    let heading = match &view {
        View::All => "All books".to_string(),
        View::Authors => "Authors".to_string(),
        View::Author(author) => db::author_label(author),
        View::List { name, .. } => name.clone(),
    };
    ui.title.set_title(&heading);
    ui.back.set_visible(matches!(view, View::Author(_)));

    if matches!(view, View::Authors) {
        fill_authors(&ui.author_list, &authors, &search);
        ui.stack.set_visible_child_name("authors");
        return;
    }

    let filtered: Vec<Book> = books
        .into_iter()
        .filter(|book| book_matches(book, &search))
        .collect();
    ui.grid.store.remove_all();
    for book in &filtered {
        ui.grid.store.append(&grid::BookObject::new(book.clone()));
    }
    if filtered.is_empty() {
        if search.trim().is_empty() {
            ui.grid.empty.set_title("No books");
            ui.grid
                .empty
                .set_description(Some("Add a PDF or EPUB to start your library."));
        } else {
            ui.grid.empty.set_title("No matches");
            ui.grid
                .empty
                .set_description(Some("Try a different title or author."));
        }
        ui.grid.stack.set_visible_child_name("empty");
    } else {
        ui.grid.stack.set_visible_child_name("grid");
    }
    ui.stack.set_visible_child_name("books");
}

fn rebuild_lists(state: &Rc<std::cell::RefCell<Shell>>, ui: &Ui, lists: &[db::BookList], _view: &View) {
    while let Some(child) = ui.lists.first_child() {
        ui.lists.remove(&child);
    }
    for list in lists {
        let row = gtk4::ListBoxRow::new();
        data::set(&row, "list-id", list.id);
        data::set(&row, "list-name", list.name.clone());

        let label = gtk4::Label::new(Some(&list.name));
        label.set_xalign(0.0);
        label.set_hexpand(true);
        label.set_margin_top(8);
        label.set_margin_bottom(8);
        label.set_margin_start(12);

        let menu = gtk4::MenuButton::new();
        menu.set_icon_name("view-more-symbolic");
        menu.add_css_class("flat");
        menu.set_tooltip_text(Some("List actions"));
        let popover = gtk4::Popover::new();
        let actions = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        let rename = gtk4::Button::with_label("Rename");
        rename.add_css_class("flat");
        let delete = gtk4::Button::with_label("Delete");
        delete.add_css_class("flat");
        delete.add_css_class("destructive-action");
        actions.append(&rename);
        actions.append(&delete);
        popover.set_child(Some(&actions));
        menu.set_popover(Some(&popover));
        let popover_rename = popover.clone();
        let popover_delete = popover.clone();

        let row_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        row_box.append(&label);
        row_box.append(&menu);
        row.set_child(Some(&row_box));

        let drop = gtk4::DropTarget::new(i64::static_type(), gdk::DragAction::COPY);
        let list_id = list.id;
        let list_name = list.name.clone();
        let state_drop = state.clone();
        let ui_drop = ui.clone();
        drop.connect_drop(move |_, value, _, _| {
            let Ok(book_id) = value.get::<i64>() else {
                return false;
            };
            let message = match state_drop.borrow().db.add_book_to_list(list_id, book_id) {
                Ok(true) => format!("Added to {list_name}"),
                Ok(false) => format!("Already in {list_name}"),
                Err(err) => err.to_string(),
            };
            let state = state_drop.clone();
            let ui = ui_drop.clone();
            glib::idle_add_local_once(move || {
                show_toast(&ui, &message);
                refresh(&state, &ui);
            });
            true
        });
        row.add_controller(drop);

        let state_rename = state.clone();
        let ui_rename = ui.clone();
        let list_id = list.id;
        let current_name = list.name.clone();
        rename.connect_clicked(move |_| {
            popover_rename.popdown();
            let state = state_rename.clone();
            let ui = ui_rename.clone();
            let window = ui.window.clone();
            prompt::prompt(&window, "Rename list", "Rename", &current_name, move |name| {
                match state.borrow().db.rename_list(list_id, &name) {
                    Ok(()) => {
                        if let View::List { id, name: shown } = &mut state.borrow_mut().view {
                            if *id == list_id {
                                *shown = name;
                            }
                        }
                        refresh(&state, &ui);
                    }
                    Err(err) => show_toast(&ui, &err.to_string()),
                }
            });
        });

        let state_delete = state.clone();
        let ui_delete = ui.clone();
        let list_id = list.id;
        let list_name = list.name.clone();
        delete.connect_clicked(move |_| {
            popover_delete.popdown();
            let state = state_delete.clone();
            let ui = ui_delete.clone();
            let window = ui.window.clone();
            let name = list_name.clone();
            prompt::confirm(
                &window,
                "Delete list?",
                &format!("{name} will be removed. The books stay in the library."),
                "Delete",
                move || {
                    let removed_current = matches!(
                        state.borrow().view,
                        View::List { id, .. } if id == list_id
                    );
                    match state.borrow().db.delete_list(list_id) {
                        Ok(()) => {
                            if removed_current {
                                state.borrow_mut().view = View::All;
                            }
                            refresh(&state, &ui);
                        }
                        Err(err) => show_toast(&ui, &err.to_string()),
                    }
                },
            );
        });

        ui.lists.append(&row);
    }
}

fn select_list_row(lists: &gtk4::ListBox, id: i64) {
    let mut child = lists.first_child();
    while let Some(widget) = child {
        let next = widget.next_sibling();
        if widget
            .downcast_ref::<gtk4::ListBoxRow>()
            .and_then(|row| data::get_copy::<i64>(row, "list-id"))
            == Some(id)
        {
            if let Some(row) = widget.downcast_ref::<gtk4::ListBoxRow>() {
                lists.select_row(Some(row));
            }
            return;
        }
        child = next;
    }
}

fn fill_authors(list: &gtk4::ListBox, authors: &[AuthorGroup], search: &str) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    let query = search.trim().to_lowercase();
    for group in authors {
        let label = db::author_label(&group.author);
        if !query.is_empty() && !label.to_lowercase().contains(&query) {
            continue;
        }
        let row = gtk4::ListBoxRow::new();
        data::set(&row, "author-key", group.author.clone());
        let name = gtk4::Label::new(Some(&label));
        name.set_xalign(0.0);
        name.set_hexpand(true);
        name.set_margin_start(16);
        name.set_margin_top(8);
        name.set_margin_bottom(8);
        let count = gtk4::Label::new(Some(&group.count.to_string()));
        count.add_css_class("dim-label");
        count.add_css_class("author-count");
        let box_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 12);
        box_row.append(&name);
        box_row.append(&count);
        row.set_child(Some(&box_row));
        list.append(&row);
    }
}

fn show_book_menu(
    state: &Rc<std::cell::RefCell<Shell>>,
    slot: &Rc<std::cell::RefCell<Option<Ui>>>,
    id: i64,
    x: f64,
    y: f64,
    widget: &gtk4::Widget,
) {
    let in_list = matches!(state.borrow().view, View::List { .. });
    let popover = gtk4::Popover::new();
    popover.set_parent(widget);
    popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    let actions = gtk4::Box::new(gtk4::Orientation::Vertical, 0);

    let open = menu_button("Open");
    let edit = menu_button("Edit");
    let remove_from_list = menu_button("Remove from list");
    remove_from_list.set_visible(in_list);
    let remove = menu_button("Remove from library");
    actions.append(&open);
    actions.append(&edit);
    actions.append(&remove_from_list);
    actions.append(&remove);
    popover.set_child(Some(&actions));
    popover.connect_closed(|popover| {
        popover.unparent();
    });

    let pop = popover.clone();
    let state_open = state.clone();
    let slot_open = slot.clone();
    open.connect_clicked(move |_| {
        pop.popdown();
        open_id(&state_open, &slot_open, id);
    });

    let pop = popover.clone();
    let state_edit = state.clone();
    let slot_edit = slot.clone();
    edit.connect_clicked(move |_| {
        pop.popdown();
        let Some(ui) = slot_edit.borrow().clone() else {
            return;
        };
        let Some(book) = state_edit.borrow().db.book(id).ok().flatten() else {
            show_toast(&ui, "That book is no longer in the library");
            return;
        };
        let state = state_edit.clone();
        let ui_for_save = ui.clone();
        book_dialog::present(&ui.window, Some(book), move |draft| {
            match state.borrow().db.update_book(id, &draft) {
                Ok(_) => refresh(&state, &ui_for_save),
                Err(err) => show_toast(&ui_for_save, &err.to_string()),
            }
        });
    });

    let pop = popover.clone();
    let state_unlink = state.clone();
    let slot_unlink = slot.clone();
    remove_from_list.connect_clicked(move |_| {
        pop.popdown();
        let Some(ui) = slot_unlink.borrow().clone() else {
            return;
        };
        let View::List { id: list_id, .. } = state_unlink.borrow().view.clone() else {
            return;
        };
        if let Err(err) = state_unlink.borrow().db.remove_book_from_list(list_id, id) {
            show_toast(&ui, &err.to_string());
            return;
        }
        refresh(&state_unlink, &ui);
    });

    let pop = popover.clone();
    let state_remove = state.clone();
    let slot_remove = slot.clone();
    remove.connect_clicked(move |_| {
        pop.popdown();
        let Some(ui) = slot_remove.borrow().clone() else {
            return;
        };
        let title = state_remove
            .borrow()
            .db
            .book(id)
            .ok()
            .flatten()
            .map(|book| book.title)
            .unwrap_or_else(|| "This book".to_string());
        let state = state_remove.clone();
        let window = ui.window.clone();
        let slot_for_confirm = slot_remove.clone();
        prompt::confirm(
            &window,
            "Remove book?",
            &format!("{title} will leave the library. The file on disk stays where it is."),
            "Remove",
            move || {
                let Some(ui) = slot_for_confirm.borrow().clone() else {
                    return;
                };
                match state.borrow().db.delete_book(id) {
                    Ok(()) => refresh(&state, &ui),
                    Err(err) => show_toast(&ui, &err.to_string()),
                }
            },
        );
    });

    popover.popup();
}

fn menu_button(label: &str) -> gtk4::Button {
    let button = gtk4::Button::with_label(label);
    button.add_css_class("flat");
    button
}

fn open_id(state: &Rc<std::cell::RefCell<Shell>>, slot: &Rc<std::cell::RefCell<Option<Ui>>>, id: i64) {
    let Some(ui) = slot.borrow().clone() else {
        return;
    };
    let path = state.borrow().db.book(id).ok().flatten().map(|book| book.file_path);
    let Some(path) = path else {
        show_toast(&ui, "That book is no longer in the library");
        return;
    };
    let file = Path::new(&path);
    if !file.exists() {
        show_toast(&ui, "That file is missing");
        return;
    }
    if let Err(err) = launch_okular(file) {
        show_toast(&ui, &err);
    }
}

fn launch_okular(path: &Path) -> Result<(), String> {
    match std::process::Command::new("okular").arg(path).spawn() {
        Ok(_) => Ok(()),
        Err(err) => {
            let file = gio::File::for_path(path);
            gio::AppInfo::launch_default_for_uri(file.uri().as_str(), None::<&gio::AppLaunchContext>)
                .map(|_| ())
                .map_err(|fallback| format!("Could not open Okular ({err}). {fallback}"))
        }
    }
}

fn book_matches(book: &Book, search: &str) -> bool {
    let query = search.trim().to_lowercase();
    if query.is_empty() {
        return true;
    }
    book.title.to_lowercase().contains(&query)
        || db::author_label(&book.author).to_lowercase().contains(&query)
}

fn show_toast(ui: &Ui, message: &str) {
    let toast = adw::Toast::new(message);
    toast.set_timeout(4);
    ui.toast.add_toast(toast);
}
