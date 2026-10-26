use gtk4::glib;
use gtk4::prelude::*;

pub fn set<T: 'static>(widget: &impl IsA<glib::Object>, key: &str, value: T) {
    unsafe { widget.set_data(key, value) }
}

pub fn get_copy<T: 'static + Copy>(widget: &impl IsA<glib::Object>, key: &str) -> Option<T> {
    unsafe { widget.data::<T>(key).map(|ptr| *ptr.as_ref()) }
}

pub fn get_clone<T: 'static + Clone>(widget: &impl IsA<glib::Object>, key: &str) -> Option<T> {
    unsafe { widget.data::<T>(key).map(|ptr| ptr.as_ref().clone()) }
}
