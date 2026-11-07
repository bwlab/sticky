/* MIT License
 *
 * Copyright (c) 2026 Sticky Notes contributors
 *
 * SPDX-License-Identifier: MIT
 */

//! GTK4 has no tray icon support, so the icon is provided over D-Bus using the
//! StatusNotifierItem specification, and its menu using the com.canonical.dbusmenu
//! specification. Only the `gio` D-Bus API is used.
//!
//! Everything here runs on the GLib main thread.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};
use std::time::Duration;

use gettextrs::gettext;
use gtk::gio::{self, prelude::*, DBusConnection, DBusMethodInvocation, DBusNodeInfo};
use gtk::glib::{self, variant::ObjectPath, Variant};

const WATCHER_BUS_NAME: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_OBJECT_PATH: &str = "/StatusNotifierWatcher";
const ITEM_OBJECT_PATH: &str = "/StatusNotifierItem";
const MENU_OBJECT_PATH: &str = "/com/vixalien/sticky/TrayMenu";
const ITEM_INTERFACE: &str = "org.kde.StatusNotifierItem";
const MENU_INTERFACE: &str = "com.canonical.dbusmenu";

const ITEM_XML: &str = r#"
<node>
  <interface name="org.kde.StatusNotifierItem">
    <property name="Category" type="s" access="read"/>
    <property name="Id" type="s" access="read"/>
    <property name="Title" type="s" access="read"/>
    <property name="Status" type="s" access="read"/>
    <property name="WindowId" type="i" access="read"/>
    <property name="IconName" type="s" access="read"/>
    <property name="OverlayIconName" type="s" access="read"/>
    <property name="AttentionIconName" type="s" access="read"/>
    <property name="AttentionMovieName" type="s" access="read"/>
    <property name="ItemIsMenu" type="b" access="read"/>
    <property name="Menu" type="o" access="read"/>
    <method name="Activate">
      <arg name="x" type="i" direction="in"/>
      <arg name="y" type="i" direction="in"/>
    </method>
    <method name="SecondaryActivate">
      <arg name="x" type="i" direction="in"/>
      <arg name="y" type="i" direction="in"/>
    </method>
    <method name="ContextMenu">
      <arg name="x" type="i" direction="in"/>
      <arg name="y" type="i" direction="in"/>
    </method>
    <method name="Scroll">
      <arg name="delta" type="i" direction="in"/>
      <arg name="orientation" type="s" direction="in"/>
    </method>
    <signal name="NewIcon"/>
    <signal name="NewTitle"/>
    <signal name="NewStatus">
      <arg name="status" type="s"/>
    </signal>
  </interface>
</node>
"#;

const MENU_XML: &str = r#"
<node>
  <interface name="com.canonical.dbusmenu">
    <property name="Version" type="u" access="read"/>
    <property name="TextDirection" type="s" access="read"/>
    <property name="Status" type="s" access="read"/>
    <property name="IconThemePath" type="as" access="read"/>
    <method name="GetLayout">
      <arg name="parentId" type="i" direction="in"/>
      <arg name="recursionDepth" type="i" direction="in"/>
      <arg name="propertyNames" type="as" direction="in"/>
      <arg name="revision" type="u" direction="out"/>
      <arg name="layout" type="(ia{sv}av)" direction="out"/>
    </method>
    <method name="GetGroupProperties">
      <arg name="ids" type="ai" direction="in"/>
      <arg name="propertyNames" type="as" direction="in"/>
      <arg name="properties" type="a(ia{sv})" direction="out"/>
    </method>
    <method name="GetProperty">
      <arg name="id" type="i" direction="in"/>
      <arg name="name" type="s" direction="in"/>
      <arg name="value" type="v" direction="out"/>
    </method>
    <method name="Event">
      <arg name="id" type="i" direction="in"/>
      <arg name="eventId" type="s" direction="in"/>
      <arg name="data" type="v" direction="in"/>
      <arg name="timestamp" type="u" direction="in"/>
    </method>
    <method name="EventGroup">
      <arg name="events" type="a(isvu)" direction="in"/>
      <arg name="idErrors" type="ai" direction="out"/>
    </method>
    <method name="AboutToShow">
      <arg name="id" type="i" direction="in"/>
      <arg name="needUpdate" type="b" direction="out"/>
    </method>
    <method name="AboutToShowGroup">
      <arg name="ids" type="ai" direction="in"/>
      <arg name="updatesNeeded" type="ai" direction="out"/>
      <arg name="idErrors" type="ai" direction="out"/>
    </method>
    <signal name="ItemsPropertiesUpdated">
      <arg name="updatedProps" type="a(ia{sv})"/>
      <arg name="removedProps" type="a(ias)"/>
    </signal>
    <signal name="LayoutUpdated">
      <arg name="revision" type="u"/>
      <arg name="parent" type="i"/>
    </signal>
  </interface>
</node>
"#;

/// The "header" color of each note style (index = style number - 1), used to
/// draw the menu swatches.
const STYLE_COLORS: [&str; 20] = [
    "#ffee92", // yellow
    "#ffc5e1", // pink
    "#d1ebcb", // green
    "#e2d0f5", // purple
    "#cce7ff", // blue
    "#dddad8", // gray
    "#3f3d3c", // charcoal
    "#deddda", // window
    "#ffcf9a", // orange
    "#ffd2c0", // peach
    "#ffc0ba", // coral
    "#ffadad", // red
    "#ffa3d1", // rose
    "#d4cdff", // lavender
    "#b4c2ff", // indigo
    "#aee8f2", // cyan
    "#9ee8dc", // aqua
    "#bdeccb", // mint
    "#e3f299", // lime
    "#e6d5b3", // sand
];

const SWATCH_SIZE: i32 = 16;
const SWATCH_BORDER: i32 = 1;

// hosts cache menu items by id, so ids must be stable across rebuilds
const SEPARATOR_ID: i32 = 1;
const OPEN_ID: i32 = 2;
const NEW_NOTE_ID: i32 = 3;
const NEW_NOTE_SEPARATOR_ID: i32 = 4;
const FIRST_NOTE_ID: i32 = 5;

pub struct TrayNote {
    pub uuid: String,
    /// displayed title, may be empty
    pub title: String,
    /// 1..=20, numbering of `sticky_core::Style`
    pub style: u8,
}

pub struct Callbacks {
    pub activate: Box<dyn Fn()>,
    pub new_note: Box<dyn Fn()>,
    pub show_note: Box<dyn Fn(&str)>,
    pub open_app: Box<dyn Fn()>,
    pub registered_changed: Box<dyn Fn(bool)>,
}

type Props = HashMap<String, Variant>;

#[derive(Clone, Debug, PartialEq)]
enum Action {
    None,
    NewNote,
    ShowNote(String),
    OpenApp,
}

#[derive(Clone)]
struct MenuItem {
    id: i32,
    properties: Props,
    action: Action,
}

/// In dbusmenu labels, a single underscore marks the mnemonic character.
fn escape_label(label: &str) -> String {
    label.replace('_', "__")
}

fn sv(s: &str) -> Variant {
    s.to_variant()
}

fn separator(id: i32) -> MenuItem {
    MenuItem {
        id,
        properties: Props::from([("type".into(), sv("separator"))]),
        action: Action::None,
    }
}

/// Stable note ids. Hosts may not support moving items, so a reorder gets
/// fresh ids.
struct NoteIds {
    order: Vec<String>,
    ids: HashMap<String, i32>,
    next: i32,
}

impl NoteIds {
    fn new() -> Self {
        NoteIds { order: vec![], ids: HashMap::new(), next: FIRST_NOTE_ID }
    }

    /// Returns the id of each uuid, in the same order.
    fn assign(&mut self, uuids: &[&str]) -> Vec<i32> {
        let existing: HashSet<&str> = uuids.iter().copied().collect();
        self.ids.retain(|uuid, _| existing.contains(uuid.as_str()));

        let known: HashSet<&str> = self.order.iter().map(String::as_str).collect();
        let kept: Vec<&str> =
            self.order.iter().map(String::as_str).filter(|u| existing.contains(u)).collect();
        let kept_now: Vec<&str> = uuids.iter().copied().filter(|u| known.contains(u)).collect();
        if kept != kept_now {
            self.ids.clear();
        }
        self.order = uuids.iter().map(|u| u.to_string()).collect();

        uuids
            .iter()
            .map(|uuid| {
                *self.ids.entry(uuid.to_string()).or_insert_with(|| {
                    let id = self.next;
                    self.next += 1;
                    id
                })
            })
            .collect()
    }
}

/// Builds the menu. `swatch` gives the icon-data of a style.
fn build_items(
    note_ids: &mut NoteIds,
    notes: &[TrayNote],
    swatch: &mut dyn FnMut(u8) -> Variant,
) -> Vec<MenuItem> {
    // creating a note is the most common thing to do from here, so it comes first
    let mut items = vec![
        MenuItem {
            id: NEW_NOTE_ID,
            properties: Props::from([
                ("label".into(), sv(&escape_label(&gettext("New Note")))),
                ("icon-name".into(), sv("list-add-symbolic")),
            ]),
            action: Action::NewNote,
        },
        separator(NEW_NOTE_SEPARATOR_ID),
    ];

    let uuids: Vec<&str> = notes.iter().map(|n| n.uuid.as_str()).collect();
    let ids = note_ids.assign(&uuids);

    for (note, id) in notes.iter().zip(ids) {
        let title = note.title.trim();
        let label = if title.is_empty() {
            format!("({})", gettext("Empty Note"))
        } else {
            escape_label(title)
        };
        items.push(MenuItem {
            id,
            properties: Props::from([
                ("label".into(), sv(&label)),
                ("icon-data".into(), swatch(note.style)),
            ]),
            action: Action::ShowNote(note.uuid.clone()),
        });
    }

    items.push(separator(SEPARATOR_ID));
    items.push(MenuItem {
        id: OPEN_ID,
        properties: Props::from([
            ("label".into(), sv(&escape_label(&gettext("Open Sticky Notes")))),
            ("icon-name".into(), sv("edit-find-symbolic")),
        ]),
        action: Action::OpenApp,
    });
    items
}

type Updated = Vec<(i32, Props)>;
type Removed = Vec<(i32, Vec<String>)>;

/// Property differences between two menus. New items are not listed: they are
/// picked up through LayoutUpdated.
fn diff_items(previous: &HashMap<i32, Props>, items: &[MenuItem]) -> (Updated, Removed) {
    let mut updated = vec![];
    let mut removed = vec![];

    for item in items {
        let Some(old) = previous.get(&item.id) else { continue };

        let changed: Props = item
            .properties
            .iter()
            .filter(|(name, value)| old.get(*name) != Some(*value))
            .map(|(n, v)| (n.clone(), v.clone()))
            .collect();
        if !changed.is_empty() {
            updated.push((item.id, changed));
        }

        let mut dropped: Vec<String> =
            old.keys().filter(|n| !item.properties.contains_key(*n)).cloned().collect();
        if !dropped.is_empty() {
            dropped.sort();
            removed.push((item.id, dropped));
        }
    }
    (updated, removed)
}

fn root_properties() -> Props {
    Props::from([("children-display".into(), sv("submenu"))])
}

fn item_variant(id: i32, properties: &Props, children: Vec<Variant>) -> Variant {
    (id, properties, children).to_variant()
}

fn layout_variant(items: &[MenuItem], parent_id: i32, recursion_depth: i32) -> Variant {
    if parent_id != 0 {
        return match items.iter().find(|i| i.id == parent_id) {
            Some(item) => item_variant(item.id, &item.properties, vec![]),
            None => item_variant(parent_id, &Props::new(), vec![]),
        };
    }

    let children = if recursion_depth == 0 {
        vec![]
    } else {
        items.iter().map(|i| item_variant(i.id, &i.properties, vec![]).to_variant()).collect()
    };
    item_variant(0, &root_properties(), children)
}

fn group_properties(items: &[MenuItem], ids: &[i32]) -> Variant {
    let mut rows: Vec<(i32, Props)> = items
        .iter()
        .filter(|i| ids.is_empty() || ids.contains(&i.id))
        .map(|i| (i.id, i.properties.clone()))
        .collect();
    if ids.is_empty() || ids.contains(&0) {
        rows.insert(0, (0, root_properties()));
    }
    (rows,).to_variant()
}

fn parse_hex(hex: &str) -> u32 {
    u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0xffee92)
}

/// Border color: each channel darkened to 60%.
fn border_color(rgb: u32) -> u32 {
    let darken = |c: u32| (c as f64 * 0.6).round() as u32;
    (darken((rgb >> 16) & 0xff) << 16) | (darken((rgb >> 8) & 0xff) << 8) | darken(rgb & 0xff)
}

fn style_rgb(style: u8) -> u32 {
    let index = (style as usize).checked_sub(1).filter(|i| *i < STYLE_COLORS.len()).unwrap_or(0);
    parse_hex(STYLE_COLORS[index])
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend(kind);
    out.extend(data);
    let crc = crc32(&out[start..]);
    out.extend(crc.to_be_bytes());
}

/// A 16x16 RGBA PNG (uncompressed), written by hand so it needs no image
/// loader: a 1px border (the color darkened) around the style color.
fn make_swatch_png(style: u8) -> Vec<u8> {
    let rgb = style_rgb(style);
    let channels = |c: u32| [(c >> 16) as u8, (c >> 8) as u8, c as u8, 0xff];
    let (fill, border) = (channels(rgb), channels(border_color(rgb)));

    let size = SWATCH_SIZE as usize;
    let edge = SWATCH_BORDER as usize;
    let mut raw = Vec::with_capacity(size * (1 + 4 * size));
    for y in 0..size {
        raw.push(0); // filter: none
        for x in 0..size {
            let on_border = x < edge || y < edge || x >= size - edge || y >= size - edge;
            raw.extend(if on_border { border } else { fill });
        }
    }

    // zlib stream made of a single stored deflate block
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    let len = raw.len() as u16;
    let mut z = vec![0x78, 0x01, 0x01];
    z.extend(len.to_le_bytes());
    z.extend((!len).to_le_bytes());
    z.extend(&raw);
    z.extend(((b << 16) | a).to_be_bytes());

    let mut ihdr = Vec::new();
    ihdr.extend((size as u32).to_be_bytes());
    ihdr.extend((size as u32).to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]); // 8 bit, RGBA

    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png_chunk(&mut png, b"IHDR", &ihdr);
    png_chunk(&mut png, b"IDAT", &z);
    png_chunk(&mut png, b"IEND", &[]);
    png
}

struct Inner {
    app_id: String,
    callbacks: Callbacks,
    connection: RefCell<Option<DBusConnection>>,
    registrations: RefCell<Vec<gio::RegistrationId>>,
    registered: Cell<bool>,
    items: RefCell<Vec<MenuItem>>,
    notes: RefCell<Vec<TrayNote>>,
    note_ids: RefCell<NoteIds>,
    revision: Cell<u32>,
    rebuild_source: RefCell<Option<glib::SourceId>>,
    swatches: RefCell<HashMap<u8, Variant>>,
}

#[derive(Clone)]
pub struct Tray {
    inner: Rc<Inner>,
}

impl Tray {
    pub fn new(app_id: &str, callbacks: Callbacks) -> Tray {
        let inner = Rc::new(Inner {
            app_id: app_id.to_string(),
            callbacks,
            connection: RefCell::new(None),
            registrations: RefCell::new(vec![]),
            registered: Cell::new(false),
            items: RefCell::new(vec![]),
            notes: RefCell::new(vec![]),
            note_ids: RefCell::new(NoteIds::new()),
            revision: Cell::new(0),
            rebuild_source: RefCell::new(None),
            swatches: RefCell::new(HashMap::new()),
        });
        inner.rebuild_menu();
        Tray { inner }
    }

    /// Exports the icon and its menu on the session bus and watches for the
    /// status notifier watcher.
    pub fn start(&self) {
        if let Err(error) = self.inner.export() {
            glib::g_warning!("sticky", "Failed to export tray icon on D-Bus: {error}");
            return;
        }

        let weak = Rc::downgrade(&self.inner);
        let weak_vanished = weak.clone();
        // the watch lives as long as the application, so its id is not kept
        gio::bus_watch_name(
            gio::BusType::Session,
            WATCHER_BUS_NAME,
            gio::BusNameWatcherFlags::NONE,
            move |_, _, _| {
                if let Some(inner) = weak.upgrade() {
                    inner.register();
                }
            },
            move |_, _| {
                if let Some(inner) = weak_vanished.upgrade() {
                    inner.set_registered(false);
                }
            },
        );
    }

    /// Notes sorted from the most recent; the menu is rebuilt after 300 ms.
    pub fn set_notes(&self, notes: Vec<TrayNote>) {
        *self.inner.notes.borrow_mut() = notes;
        self.inner.invalidate_menu();
    }
}

impl Inner {
    fn export(self: &Rc<Self>) -> Result<(), glib::Error> {
        let connection = gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)?;
        let item_info = DBusNodeInfo::for_xml(ITEM_XML)?;
        let menu_info = DBusNodeInfo::for_xml(MENU_XML)?;
        let item_iface = item_info.lookup_interface(ITEM_INTERFACE).expect("item interface");
        let menu_iface = menu_info.lookup_interface(MENU_INTERFACE).expect("menu interface");

        let (w1, w2, w3, w4) = (Rc::downgrade(self), Rc::downgrade(self), Rc::downgrade(self), Rc::downgrade(self));
        let item_id = connection
            .register_object(ITEM_OBJECT_PATH, &item_iface)
            .method_call(move |_, _, _, _, method, _, invocation| {
                if let Some(inner) = w1.upgrade() {
                    inner.item_method(method, invocation);
                }
            })
            .property(move |_, _, _, _, name| match w2.upgrade() {
                Some(inner) => inner.item_property(name),
                None => "".to_variant(),
            })
            .build()?;
        let menu_id = connection
            .register_object(MENU_OBJECT_PATH, &menu_iface)
            .method_call(move |_, _, _, _, method, params, invocation| {
                if let Some(inner) = w3.upgrade() {
                    inner.menu_method(method, &params, invocation);
                }
            })
            .property(move |_, _, _, _, name| match w4.upgrade() {
                Some(_) => menu_property(name),
                None => "".to_variant(),
            })
            .build()?;

        *self.registrations.borrow_mut() = vec![item_id, menu_id];
        *self.connection.borrow_mut() = Some(connection);
        Ok(())
    }

    fn register(self: &Rc<Self>) {
        let Some(connection) = self.connection.borrow().clone() else { return };
        let weak = Rc::downgrade(self);
        connection.call(
            Some(WATCHER_BUS_NAME),
            WATCHER_OBJECT_PATH,
            WATCHER_BUS_NAME,
            "RegisterStatusNotifierItem",
            // registering the object path (instead of a bus name) works with all
            // modern hosts and doesn't require owning a second name on the bus
            Some(&(ITEM_OBJECT_PATH,).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            -1,
            gio::Cancellable::NONE,
            move |result| {
                let Some(inner) = weak.upgrade() else { return };
                match result {
                    Ok(_) => inner.set_registered(true),
                    Err(error) => {
                        glib::g_warning!(
                            "sticky",
                            "Failed to register with the status notifier watcher: {error}"
                        );
                        inner.set_registered(false);
                    }
                }
            },
        );
    }

    fn set_registered(&self, registered: bool) {
        if self.registered.replace(registered) == registered {
            return;
        }
        (self.callbacks.registered_changed)(registered);
    }

    fn item_property(&self, name: &str) -> Variant {
        match name {
            "Category" => "ApplicationStatus".to_variant(),
            "Id" | "IconName" => self.app_id.to_variant(),
            "Title" => gettext("Sticky Notes").to_variant(),
            "Status" => "Active".to_variant(),
            "WindowId" => 0i32.to_variant(),
            "ItemIsMenu" => false.to_variant(),
            "Menu" => ObjectPath::try_from(MENU_OBJECT_PATH).expect("valid path").to_variant(),
            // OverlayIconName, AttentionIconName, AttentionMovieName
            _ => "".to_variant(),
        }
    }

    fn item_method(&self, method: &str, invocation: DBusMethodInvocation) {
        match method {
            "Activate" | "SecondaryActivate" => (self.callbacks.activate)(),
            // ContextMenu: the host displays the Menu itself
            _ => {}
        }
        invocation.return_value(None);
    }

    fn run_action(&self, action: &Action) {
        match action {
            Action::None => {}
            Action::NewNote => (self.callbacks.new_note)(),
            Action::ShowNote(uuid) => (self.callbacks.show_note)(uuid),
            Action::OpenApp => (self.callbacks.open_app)(),
        }
    }

    /// The items borrow must be released before running callbacks, which may
    /// call `set_notes`.
    fn action_of(&self, id: i32) -> Option<Action> {
        self.items.borrow().iter().find(|i| i.id == id).map(|i| i.action.clone())
    }

    fn menu_method(&self, method: &str, params: &Variant, invocation: DBusMethodInvocation) {
        let reply: Option<Variant> = match method {
            "GetLayout" => params.get::<(i32, i32, Vec<String>)>().map(|(parent, depth, _)| {
                let layout = layout_variant(&self.items.borrow(), parent, depth);
                Variant::tuple_from_iter([self.revision.get().to_variant(), layout])
            }),
            "GetGroupProperties" => params
                .get::<(Vec<i32>, Vec<String>)>()
                .map(|(ids, _)| group_properties(&self.items.borrow(), &ids)),
            "GetProperty" => params.get::<(i32, String)>().map(|(id, name)| {
                let value = if id == 0 {
                    root_properties().remove(&name)
                } else {
                    self.items.borrow().iter().find(|i| i.id == id).and_then(|i| i.properties.get(&name).cloned())
                };
                (value.unwrap_or_else(|| "".to_variant()),).to_variant()
            }),
            "Event" => params.get::<(i32, String, Variant, u32)>().map(|(id, event, _, _)| {
                if event == "clicked" {
                    if let Some(action) = self.action_of(id) {
                        self.run_action(&action);
                    }
                }
                ().to_variant()
            }),
            "EventGroup" => params.get::<(Vec<(i32, String, Variant, u32)>,)>().map(|(events,)| {
                let mut id_errors: Vec<i32> = vec![];
                for (id, event, _, _) in events {
                    let action = self.action_of(id);
                    if action.is_none() && id != 0 {
                        id_errors.push(id);
                        continue;
                    }
                    if event == "clicked" {
                        if let Some(action) = action {
                            self.run_action(&action);
                        }
                    }
                }
                (id_errors,).to_variant()
            }),
            "AboutToShow" => Some((true,).to_variant()),
            "AboutToShowGroup" => params
                .get::<(Vec<i32>,)>()
                .map(|(ids,)| (ids, Vec::<i32>::new()).to_variant()),
            _ => None,
        };

        match reply {
            Some(v) => invocation.return_value(Some(&v)),
            None => invocation.return_error(
                gio::DBusError::InvalidArgs,
                &format!("Invalid call to {method}"),
            ),
        }
    }

    fn swatch(&self, style: u8) -> Variant {
        self.swatches
            .borrow_mut()
            .entry(style)
            .or_insert_with(|| make_swatch_png(style).to_variant())
            .clone()
    }

    fn rebuild_menu(&self) {
        let items = {
            let notes = self.notes.borrow();
            let mut swatch = |style| self.swatch(style);
            build_items(&mut self.note_ids.borrow_mut(), &notes, &mut swatch)
        };
        *self.items.borrow_mut() = items;
        self.revision.set(self.revision.get() + 1);
    }

    fn invalidate_menu(self: &Rc<Self>) {
        if self.rebuild_source.borrow().is_some() {
            return;
        }
        let weak: Weak<Inner> = Rc::downgrade(self);
        let source = glib::timeout_add_local_once(Duration::from_millis(300), move || {
            let Some(inner) = weak.upgrade() else { return };
            *inner.rebuild_source.borrow_mut() = None;

            let previous: HashMap<i32, Props> =
                inner.items.borrow().iter().map(|i| (i.id, i.properties.clone())).collect();
            inner.rebuild_menu();
            inner.emit_menu_updates(&previous);
        });
        *self.rebuild_source.borrow_mut() = Some(source);
    }

    fn emit_menu_updates(&self, previous: &HashMap<i32, Props>) {
        let Some(connection) = self.connection.borrow().clone() else { return };
        let (updated, removed) = diff_items(previous, &self.items.borrow());

        let emit = |name: &str, params: Variant| {
            if let Err(error) =
                connection.emit_signal(None, MENU_OBJECT_PATH, MENU_INTERFACE, name, Some(&params))
            {
                glib::g_warning!("sticky", "Failed to emit {name}: {error}");
            }
        };
        if !updated.is_empty() || !removed.is_empty() {
            emit("ItemsPropertiesUpdated", (updated, removed).to_variant());
        }
        emit("LayoutUpdated", (self.revision.get(), 0i32).to_variant());
    }
}

fn menu_property(name: &str) -> Variant {
    match name {
        "Version" => 3u32.to_variant(),
        "TextDirection" => "ltr".to_variant(),
        "Status" => "normal".to_variant(),
        "IconThemePath" => Vec::<String>::new().to_variant(),
        _ => "".to_variant(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(uuid: &str, title: &str, style: u8) -> TrayNote {
        TrayNote { uuid: uuid.into(), title: title.into(), style }
    }

    fn build(ids: &mut NoteIds, notes: &[TrayNote]) -> Vec<MenuItem> {
        build_items(ids, notes, &mut |s| vec![s].to_variant())
    }

    fn id_of(items: &[MenuItem], action: &Action) -> i32 {
        items.iter().find(|i| &i.action == action).unwrap().id
    }

    #[test]
    fn escapes_underscores() {
        assert_eq!(escape_label("a_b__c"), "a__b____c");
        assert_eq!(escape_label("plain"), "plain");
    }

    #[test]
    fn layout_and_fixed_ids() {
        let items = build(&mut NoteIds::new(), &[note("a", "A_1", 1), note("b", "  ", 2)]);
        let ids: Vec<i32> = items.iter().map(|i| i.id).collect();
        assert_eq!(ids, vec![NEW_NOTE_ID, NEW_NOTE_SEPARATOR_ID, 5, 6, SEPARATOR_ID, OPEN_ID]);
        assert_eq!(items[2].properties["label"].str(), Some("A__1"));
        assert_eq!(items[3].properties["label"].str(), Some("(Empty Note)"));
        assert_eq!(items[5].action, Action::OpenApp);
    }

    #[test]
    fn ids_stable_until_reorder() {
        let mut ids = NoteIds::new();
        let a = id_of(&build(&mut ids, &[note("a", "", 1), note("b", "", 1)]), &Action::ShowNote("a".into()));
        // same order, new note at the top, one removed: kept ids stay
        let items = build(&mut ids, &[note("c", "", 1), note("a", "", 1)]);
        assert_eq!(id_of(&items, &Action::ShowNote("a".into())), a);
        // reorder: fresh ids
        let items = build(&mut ids, &[note("a", "", 1), note("c", "", 1)]);
        assert_ne!(id_of(&items, &Action::ShowNote("a".into())), a);
    }

    #[test]
    fn diff_reports_changes_only() {
        let mut ids = NoteIds::new();
        let before = build(&mut ids, &[note("a", "x", 1)]);
        let prev: HashMap<i32, Props> = before.iter().map(|i| (i.id, i.properties.clone())).collect();

        let same = build(&mut ids, &[note("a", "x", 1)]);
        assert_eq!(diff_items(&prev, &same), (vec![], vec![]));

        let after = build(&mut ids, &[note("a", "y", 1), note("b", "new", 1)]);
        let (updated, removed) = diff_items(&prev, &after);
        assert_eq!(updated.len(), 1);
        assert_eq!(updated[0].0, 5);
        assert_eq!(updated[0].1.keys().collect::<Vec<_>>(), vec!["label"]);
        assert!(removed.is_empty()); // new item "b" is not listed

        let mut dropped = prev.clone();
        dropped.get_mut(&5).unwrap().insert("gone".into(), sv("x"));
        assert_eq!(diff_items(&dropped, &same).1, vec![(5, vec!["gone".to_string()])]);
    }

    #[test]
    fn layout_variants_have_dbusmenu_types() {
        let items = build(&mut NoteIds::new(), &[note("a", "x", 1)]);
        let full = layout_variant(&items, 0, -1);
        assert_eq!(full.type_().as_str(), "(ia{sv}av)");
        assert_eq!(full.child_value(2).n_children(), items.len());
        assert_eq!(layout_variant(&items, 0, 0).child_value(2).n_children(), 0);
        assert_eq!(layout_variant(&items, 99, 1).type_().as_str(), "(ia{sv}av)");
        assert_eq!(group_properties(&items, &[]).type_().as_str(), "(a(ia{sv}))");
        assert_eq!(group_properties(&items, &[]).child_value(0).n_children(), items.len() + 1);
        assert_eq!(group_properties(&items, &[3]).child_value(0).n_children(), 1);
    }

    #[test]
    fn swatch_is_png_and_colors() {
        let png = make_swatch_png(1);
        assert_eq!(&png[1..4], b"PNG");
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        // decodable by the real loader when it works in this environment
        if let Ok(pb) = gtk::gdk_pixbuf::Pixbuf::from_stream(
            &gio::MemoryInputStream::from_bytes(&glib::Bytes::from(&png)),
            gio::Cancellable::NONE,
        ) {
            assert_eq!((pb.width(), pb.height()), (16, 16));
            let px = pb.pixel_bytes().unwrap();
            assert_eq!(&px[..3], &[0x99, 0x8f, 0x58]); // border: #ffee92 * 0.6
            assert_eq!(&px[(16 * 8 + 8) * 4..][..3], &[0xff, 0xee, 0x92]);
        }
        assert_eq!(border_color(0xffffff), 0x999999);
        assert_eq!(style_rgb(7), 0x3f3d3c);
        assert_eq!(style_rgb(0), style_rgb(1)); // out of range falls back to yellow
        assert_eq!(style_rgb(21), style_rgb(1));
    }

    /// Needs a private bus: `dbus-run-session -- cargo test -- --ignored`.
    #[test]
    #[ignore = "needs a private session bus"]
    fn exports_on_a_private_bus() {
        let Ok(address) = std::env::var("DBUS_SESSION_BUS_ADDRESS") else { return };
        let ctx = glib::MainContext::default();
        let pump_until = |done: &dyn Fn() -> bool| {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while !done() && std::time::Instant::now() < deadline {
                ctx.iteration(false);
            }
            assert!(done(), "timeout");
        };

        // a fake watcher owned by a second connection
        let client = DBusConnection::for_address_sync(
            &address,
            gio::DBusConnectionFlags::AUTHENTICATION_CLIENT
                | gio::DBusConnectionFlags::MESSAGE_BUS_CONNECTION,
            None,
            gio::Cancellable::NONE,
        )
        .unwrap();
        let watcher_xml = DBusNodeInfo::for_xml(
            r#"<node><interface name="org.kde.StatusNotifierWatcher">
               <method name="RegisterStatusNotifierItem"><arg type="s" direction="in"/></method>
               </interface></node>"#,
        )
        .unwrap();
        let registered_path = Rc::new(RefCell::new(String::new()));
        let rp = registered_path.clone();
        let _w = client
            .register_object(WATCHER_OBJECT_PATH, &watcher_xml.lookup_interface(WATCHER_BUS_NAME).unwrap())
            .method_call(move |_, _, _, _, _, params, inv| {
                *rp.borrow_mut() = params.child_value(0).str().unwrap().to_string();
                inv.return_value(None);
            })
            .build()
            .unwrap();

        let log = Rc::new(RefCell::new(Vec::<String>::new()));
        let l = |name: &'static str, log: &Rc<RefCell<Vec<String>>>| {
            let log = log.clone();
            move || log.borrow_mut().push(name.to_string())
        };
        let (l1, l2, l3) = (l("activate", &log), l("new", &log), l("open", &log));
        let log2 = log.clone();
        let log3 = log.clone();
        let tray = Tray::new(
            "test.app",
            Callbacks {
                activate: Box::new(l1),
                new_note: Box::new(l2),
                show_note: Box::new(move |u| log2.borrow_mut().push(format!("show {u}"))),
                open_app: Box::new(l3),
                registered_changed: Box::new(move |r| log3.borrow_mut().push(format!("registered {r}"))),
            },
        );
        tray.set_notes(vec![note("u1", "Hello", 3)]);
        tray.start();

        let _own = gio::bus_own_name_on_connection(
            &client,
            WATCHER_BUS_NAME,
            gio::BusNameOwnerFlags::NONE,
            |_, _| {},
            |_, _| {},
        );
        pump_until(&|| log.borrow().iter().any(|e| e == "registered true"));
        assert_eq!(*registered_path.borrow(), ITEM_OBJECT_PATH);

        let dest = tray.inner.connection.borrow().as_ref().unwrap().unique_name().unwrap().to_string();
        let call = |path: &str, iface: &str, method: &str, args: Option<Variant>| -> Variant {
            let fut = client.call_future(
                Some(&dest), path, iface, method, args.as_ref(), None, gio::DBusCallFlags::NONE, 5000,
            );
            ctx.block_on(fut).unwrap()
        };

        // properties and layout, as a real host would read them
        let id = call(ITEM_OBJECT_PATH, "org.freedesktop.DBus.Properties", "Get",
            Some(("org.kde.StatusNotifierItem", "Id").to_variant()));
        assert_eq!(id.child_value(0).as_variant().unwrap().str(), Some("test.app"));
        let menu = call(ITEM_OBJECT_PATH, "org.freedesktop.DBus.Properties", "Get",
            Some(("org.kde.StatusNotifierItem", "Menu").to_variant()));
        assert_eq!(menu.child_value(0).as_variant().unwrap().str(), Some(MENU_OBJECT_PATH));

        // the first rebuild happens after the debounce
        pump_until(&|| tray.inner.items.borrow().len() == 5);
        let layout = call(MENU_OBJECT_PATH, MENU_INTERFACE, "GetLayout",
            Some((0i32, -1i32, Vec::<String>::new()).to_variant()));
        assert_eq!(layout.type_().as_str(), "(u(ia{sv}av))");
        assert_eq!(layout.child_value(1).child_value(2).n_children(), 5);

        // clicks reach the callbacks
        call(MENU_OBJECT_PATH, MENU_INTERFACE, "Event",
            Some((NEW_NOTE_ID, "clicked", 0u8.to_variant(), 0u32).to_variant()));
        call(MENU_OBJECT_PATH, MENU_INTERFACE, "Event",
            Some((5i32, "clicked", 0u8.to_variant(), 0u32).to_variant()));
        let errors = call(MENU_OBJECT_PATH, MENU_INTERFACE, "EventGroup",
            Some((vec![(OPEN_ID, "clicked".to_string(), 0u8.to_variant(), 0u32), (999, "clicked".to_string(), 0u8.to_variant(), 0u32)],).to_variant()));
        assert_eq!(errors.get::<(Vec<i32>,)>().unwrap().0, vec![999]);
        call(ITEM_OBJECT_PATH, ITEM_INTERFACE, "Activate", Some((0i32, 0i32).to_variant()));
        assert_eq!(&log.borrow()[1..], ["new", "show u1", "open", "activate"]);
    }
}
