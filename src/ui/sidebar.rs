use gtk4::prelude::*;
use gtk4::{ListBox, ListBoxRow, ScrolledWindow};

use super::data;

pub struct Sidebar {
    pub root: ScrolledWindow,
    pub library: ListBox,
    pub lists: ListBox,
    pub all_row: ListBoxRow,
    pub authors_row: ListBoxRow,
    pub new_list: gtk4::Button,
}

pub fn build() -> Sidebar {
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 0);

    content.append(&section_label("Library"));
    let library = ListBox::new();
    library.add_css_class("navigation-sidebar");
    library.set_selection_mode(gtk4::SelectionMode::Single);
    let all_row = nav_row("All books", "all");
    let authors_row = nav_row("Authors", "authors");
    library.append(&all_row);
    library.append(&authors_row);
    content.append(&library);

    content.append(&section_label("Lists"));
    let lists = ListBox::new();
    lists.add_css_class("navigation-sidebar");
    lists.set_selection_mode(gtk4::SelectionMode::Single);
    content.append(&lists);

    let new_list = gtk4::Button::with_label("New list");
    new_list.add_css_class("flat");
    new_list.set_halign(gtk4::Align::Start);
    new_list.set_margin_top(6);
    new_list.set_margin_start(6);
    new_list.set_margin_bottom(12);
    content.append(&new_list);

    let root = ScrolledWindow::new();
    root.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    root.set_child(Some(&content));
    root.set_vexpand(true);
    root.set_width_request(220);

    Sidebar {
        root,
        library,
        lists,
        all_row,
        authors_row,
        new_list,
    }
}

fn section_label(text: &str) -> gtk4::Label {
    let label = gtk4::Label::new(Some(text));
    label.set_xalign(0.0);
    label.add_css_class("dim-label");
    label.add_css_class("sidebar-section");
    label
}

fn nav_row(text: &str, kind: &'static str) -> ListBoxRow {
    let row = ListBoxRow::new();
    let label = gtk4::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_margin_top(10);
    label.set_margin_bottom(10);
    label.set_margin_start(12);
    label.set_margin_end(12);
    row.set_child(Some(&label));
    data::set(&row, "nav", kind);
    row
}
