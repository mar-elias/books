use std::rc::Rc;

use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

pub fn prompt(
    parent: &impl gtk4::prelude::IsA<gtk4::Widget>,
    title: &str,
    confirm: &str,
    initial: &str,
    on_ok: impl Fn(String) + 'static,
) {
    let dialog = adw::Dialog::new();
    dialog.set_title(title);
    dialog.set_content_width(360);

    let header = adw::HeaderBar::new();
    let cancel = gtk4::Button::with_label("Cancel");
    header.pack_start(&cancel);
    let save = gtk4::Button::with_label(confirm);
    save.add_css_class("suggested-action");
    header.pack_end(&save);

    let entry = gtk4::Entry::new();
    entry.set_text(initial);
    entry.set_hexpand(true);
    entry.set_activates_default(true);

    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    content.set_margin_top(12);
    content.set_margin_bottom(16);
    content.set_margin_start(16);
    content.set_margin_end(16);
    content.append(&entry);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&content));
    dialog.set_child(Some(&toolbar));

    let on_ok = Rc::new(on_ok);
    let submit = {
        let dialog = dialog.clone();
        let entry = entry.clone();
        let on_ok = on_ok.clone();
        move || {
            let text = entry.text().trim().to_string();
            if text.is_empty() {
                return;
            }
            dialog.close();
            on_ok(text);
        }
    };
    let submit = Rc::new(submit);

    let submit_click = submit.clone();
    save.connect_clicked(move |_| submit_click());
    let submit_activate = submit.clone();
    entry.connect_activate(move |_| submit_activate());

    let dialog_cancel = dialog.clone();
    cancel.connect_clicked(move |_| {
        dialog_cancel.close();
    });

    dialog.present(Some(parent));
    entry.grab_focus();
}

pub fn confirm(
    parent: &impl gtk4::prelude::IsA<gtk4::Widget>,
    heading: &str,
    body: &str,
    confirm_label: &str,
    on_confirm: impl Fn() + 'static,
) {
    let dialog = adw::AlertDialog::new(Some(heading), Some(body));
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("confirm", confirm_label);
    dialog.set_response_appearance("confirm", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    dialog.connect_response(None, move |_, response| {
        if response == "confirm" {
            on_confirm();
        }
    });
    dialog.present(Some(parent));
}
