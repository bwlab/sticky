use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gettextrs::gettext;
use gtk::gdk;
use gtk::prelude::*;
use sticky_core::Style;

use crate::util::style_label;

const COLUMNS: i32 = 5;

/// A grid of colours to pick the style of a note from.
#[derive(Clone)]
pub struct StyleSelector {
    pub widget: gtk::Box,
    state: Rc<State>,
}

struct State {
    style: Cell<Style>,
    buttons: Vec<(Style, gtk::CheckButton)>,
    on_changed: RefCell<Vec<Rc<dyn Fn(Style)>>>,
}

thread_local! {
    static PROVIDER_LOADED: Cell<bool> = const { Cell::new(false) };
}

fn load_css_once() {
    if PROVIDER_LOADED.with(|p| p.replace(true)) {
        return;
    }
    let Some(display) = gdk::Display::default() else { return };
    let provider = gtk::CssProvider::new();
    provider.load_from_resource("/com/vixalien/sticky/style-selector.css");
    gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
}

impl StyleSelector {
    pub fn new(style: Style) -> StyleSelector {
        load_css_once();

        let widget = gtk::Box::builder().orientation(gtk::Orientation::Vertical).css_name("styleselector").hexpand(true).build();
        let grid = gtk::Grid::builder()
            .row_spacing(8)
            .column_spacing(8)
            .column_homogeneous(true)
            .hexpand(true)
            .build();
        widget.append(&grid);

        let mut buttons: Vec<(Style, gtk::CheckButton)> = vec![];
        for (i, s) in Style::ALL.iter().copied().enumerate() {
            let button = gtk::CheckButton::builder()
                .tooltip_text(gettext("Switch to %s style").replace("%s", &style_label(s)))
                .width_request(44)
                .height_request(44)
                .hexpand(true)
                .halign(gtk::Align::Center)
                .build();
            button.add_css_class("style-selector");
            button.add_css_class(&format!("style-{}", s.name()));
            if let Some((_, first)) = buttons.first() {
                button.set_group(Some(first));
            }
            button.set_active(s == style);
            grid.attach(&button, i as i32 % COLUMNS, i as i32 / COLUMNS, 1, 1);
            buttons.push((s, button));
        }

        let selector = StyleSelector {
            widget,
            state: Rc::new(State { style: Cell::new(style), buttons, on_changed: RefCell::new(vec![]) }),
        };

        for (s, button) in &selector.state.buttons {
            let weak = Rc::downgrade(&selector.state);
            let widget = selector.widget.clone();
            let s = *s;
            button.connect_toggled(move |b| {
                if b.is_active() {
                    if let Some(state) = weak.upgrade() {
                        StyleSelector { widget: widget.clone(), state }.set_style(s);
                    }
                }
            });
        }
        selector
    }

    /// The selector may be changed from outside, e.g. by its twin in the toolbar.
    pub fn set_style(&self, style: Style) {
        if self.state.style.get() == style {
            return;
        }
        self.state.style.set(style);
        for (s, button) in &self.state.buttons {
            button.set_active(*s == style);
        }
        let handlers: Vec<_> = self.state.on_changed.borrow().clone();
        for h in handlers {
            h(style);
        }
    }

    pub fn connect_style_changed(&self, f: impl Fn(Style) + 'static) {
        self.state.on_changed.borrow_mut().push(Rc::new(f));
    }
}
