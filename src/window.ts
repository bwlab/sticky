/* MIT License
 *
 * Copyright (c) 2023 Angelo Verlain, Chris Davis
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 *
 * SPDX-License-Identifier: MIT
 */

import GObject from "gi://GObject";
import Gtk from "gi://Gtk?version=4.0";
import Adw from "gi://Adw";
import Gio from "gi://Gio";
import GLib from "gi://GLib";

import { StyleSelector } from "./styleselector.js";
import { confirm_delete, NO_POSITION, Note, Style } from "./util.js";
import { TextSize, WriteableStickyNote } from "./view.js";
import { find } from "linkifyjs";
import { Application } from "./application.js";

export class Window extends Adw.ApplicationWindow {
  declare _container: Gtk.Box;
  declare _text: Gtk.TextView;
  declare _menu_button: Gtk.MenuButton;
  declare _title_label: Gtk.EditableLabel;

  declare _bold_button: Gtk.ToggleButton;
  declare _underline_button: Gtk.ToggleButton;
  declare _italic_button: Gtk.ToggleButton;
  declare _strikethrough_button: Gtk.ToggleButton;
  declare _format_button: Gtk.MenuButton;
  declare _action_button: Gtk.ToggleButton;
  declare _action_revealer: Gtk.Revealer;

  // buffer = new Gtk.TextBuffer();

  view: WriteableStickyNote;

  selector: StyleSelector;

  note: Note;
  deleted = false;
  cursor_scroll_source: number | null = null;
  last_revealer = false;

  static {
    GObject.registerClass(
      {
        Template: "resource:///com/vixalien/sticky/ui/window.ui",
        GTypeName: "StickyNoteWindow",
        InternalChildren: [
          "container",
          "text",
          "title_label",
          "bold_button",
          "underline_button",
          "italic_button",
          "strikethrough_button",
          "format_button",
          "menu_button",
          "action_revealer",
          "action_button",
        ],
        Signals: {
          deleted: {
            param_types: [GObject.TYPE_STRING],
          },
        },
      },
      this,
    );
  }

  get_style() {
    return this.selector.style;
  }

  constructor(
    { note, ...params }: Partial<Adw.ApplicationWindow.ConstructorProps> & {
      note: Note;
    },
  ) {
    super(params);

    this.note = note;

    this.default_width = note.width;
    this.default_height = note.height;

    this.note.bind_property_full(
      "display-title",
      this,
      "title",
      GObject.BindingFlags.SYNC_CREATE,
      (__, title) => {
        if (!title) {
          return [true, _("Sticky Note")];
        }
        return [true, title];
      },
      null,
    );

    // the label shows the same title as the window, so that a note that
    // hasn't been given one still has something to click on to give it one
    this.bind_property(
      "title",
      this._title_label,
      "text",
      GObject.BindingFlags.SYNC_CREATE,
    );

    this._title_label.connect("notify::editing", () => {
      if (this._title_label.editing) return;
      this.rename(this._title_label.text);
    });

    this.connect("close-request", () => {
      this.cancel_cursor_scroll();

      if (this.deleted) return;

      // a note that was given a title is worth keeping even while it is empty
      if (
        this.view.note && this.view.note.content.trim().length === 0 &&
        !this.note.title
      ) {
        (this.application as Application).delete_note(this.note.uuid);
        return;
      }

      const width = this.get_allocated_width();
      const height = this.get_allocated_height();

      if (width !== note.width) {
        this.note.width = width;
      }
      if (height !== note.height) {
        this.note.height = height;
      }
    });
    this.connect("unrealize", this.cancel_cursor_scroll.bind(this));

    this.set_style(note.style, true);

    this.view = new WriteableStickyNote(note);
    this.view.connect("selection-changed", this.check_tags.bind(this));
    this.view.connect(
      "tag-toggle",
      (_view: WriteableStickyNote, tag: string, active: boolean) => {
        const button =
          this[`_${tag}_button` as keyof typeof this] as Gtk.ToggleButton;
        if (!button) return;
        button.active = active;
      },
    );
    this.view.connect("link-selected", (_, text) => {
      this.update_link(true, text);
    });
    this.view.connect("link-unselected", () => {
      this.update_link(false, "");
    });

    this._text.buffer = this.view.buffer;
    this.view.buffer.connect(
      "notify::cursor-position",
      this.queue_cursor_scroll.bind(this),
    );
    this.view.buffer.connect("changed", this.queue_cursor_scroll.bind(this));

    this.view.attach_checkbox_gesture(this._text);

    this.add_actions();
    this.apply_font_size();

    this.selector = new StyleSelector({ style: note.style });
    this.selector.connect("style-changed", (_selector, style) => {
      this.set_style(style);
      this.note.style = style;
    });

    const popover = this._menu_button.get_popover() as Gtk.PopoverMenu;
    popover.add_child(this.selector, "notestyleswitcher");
  }

  queue_cursor_scroll() {
    if (!this.get_realized() || this.cursor_scroll_source !== null) return;

    this.cursor_scroll_source = GLib.idle_add(
      GLib.PRIORITY_DEFAULT_IDLE,
      () => {
        this.cursor_scroll_source = null;
        this.scroll_cursor_into_view();
        return GLib.SOURCE_REMOVE;
      },
    );
  }

  cancel_cursor_scroll() {
    if (this.cursor_scroll_source === null) return;

    GLib.source_remove(this.cursor_scroll_source);
    this.cursor_scroll_source = null;
  }

  scroll_cursor_into_view() {
    const cursor = this.view.buffer.get_iter_at_mark(
      this.view.buffer.get_insert(),
    );
    const cursor_rect = this._text.get_iter_location(cursor);
    const visible_rect = this._text.get_visible_rect();
    const visible_top = visible_rect.y + this._text.top_margin;
    const visible_bottom = visible_rect.y + visible_rect.height -
      this._text.bottom_margin;

    let offset = 0;
    if (cursor_rect.y < visible_top) {
      offset = cursor_rect.y - visible_top;
    } else if (cursor_rect.y + cursor_rect.height > visible_bottom) {
      offset = cursor_rect.y + cursor_rect.height - visible_bottom;
    }

    if (offset === 0) return;
    this._text.vadjustment.value += offset;
  }

  /**
   * The title shown in the header bar and the window manager, which is the
   * note's own title when it has one, and one derived from its content
   * otherwise.
   */
  get display_title() {
    return this.note.display_title || _("Sticky Note");
  }

  /**
   * Gives the note the title left in the header bar, or takes its title away
   * when the label was emptied, letting it follow the content again.
   *
   * The label shows the derived title of an untitled note, so text that was
   * left as it was found means the note keeps having no title of its own.
   */
  rename(text: string) {
    const title = text.trim();

    if (title === this.display_title) return;

    this.note.title = title;
    this.note.modified_date = new Date();
  }


  update_link(selected: boolean, text: string) {
    if (selected) {
      const link = find(text)[0];
      if (link) {
        this._action_button.action_target = GLib.Variant.new_string(link.href);
      } else {
        selected = false;
      }
    }

    if (this.last_revealer === selected) return;

    this.last_revealer = selected;
    this._action_revealer.reveal_child = selected;

    // add timeout for the transition to finish
    GLib.timeout_add(
      GLib.PRIORITY_DEFAULT,
      this._action_revealer.transition_duration,
      () => {
        const target = Adw.PropertyAnimationTarget.new(
          this._text,
          "bottom_margin",
        );

        const animation = Adw.TimedAnimation.new(
          this._text,
          this._text.bottom_margin,
          60 +
            (selected ? this._action_revealer.get_allocated_height() : 0),
          this._action_revealer.transition_duration,
          target,
        );

        animation.play();

        return GLib.SOURCE_REMOVE;
      },
    );
  }

  /**
   * Formats that are only reachable from the formatting menu. They keep their
   * state so the menu item can show whether they are on, in place of the
   * toolbar button the other formats have.
   */
  static menu_formats = ["monospace", "highlight", "header"];

  check_tags() {
    for (const [name, tag] of this.view.actions) {
      const active = this.view.has_tag(tag) !== false;

      const button = this[`_${name}_button` as keyof typeof this] as
        | Gtk.ToggleButton
        | undefined;

      if (button && button.active !== active) {
        button.active = active;
      }

      const action = this.lookup_action(name) as Gio.SimpleAction | null;

      if (action?.state_type && action.get_state()?.get_boolean() !== active) {
        action.set_state(GLib.Variant.new_boolean(active));
      }
    }

    const size = this.view.get_text_size();
    const size_action = this.lookup_action("text-size") as
      | Gio.SimpleAction
      | null;

    // selection-changed can fire while the buffer is assigned, before add_actions()
    if (size_action && size_action.get_state()?.deepUnpack<string>() !== size) {
      size_action.set_state(GLib.Variant.new_string(size));
    }
  }

  position_variant() {
    const has = this.note.x !== NO_POSITION && this.note.y !== NO_POSITION;
    return GLib.Variant.new_tuple([
      GLib.Variant.new_boolean(has),
      GLib.Variant.new_int32(has ? this.note.x : 0),
      GLib.Variant.new_int32(has ? this.note.y : 0),
    ]);
  }

  add_actions() {
    const delete_ = Gio.SimpleAction.new("delete", null);
    delete_.connect("activate", () => this.delete());
    this.add_action(delete_);

    // the two actions below are read by the GNOME Shell extension, which keeps
    // the window above the others (Wayland clients can't do that themselves)
    const keep_above = Gio.SimpleAction.new_stateful(
      "keep-above",
      null,
      GLib.Variant.new_boolean(this.note.keep_above),
    );
    keep_above.connect("change-state", (_, value) => {
      if (!value) return;

      this.note.keep_above = value.get_boolean();
      keep_above.set_state(value);
    });
    this.add_action(keep_above);

    // (has position, x, y): the extension restores it when the window opens and
    // reports it again whenever the window is moved
    const position = Gio.SimpleAction.new_stateful(
      "position",
      GLib.VariantType.new("(bii)"),
      this.position_variant(),
    );
    position.connect("change-state", (_, value) => {
      if (!value) return;

      const [has, x, y] = value.deepUnpack<[boolean, number, number]>();
      if (!has) return;

      this.note.freeze_notify();
      this.note.x = x;
      this.note.y = y;
      this.note.thaw_notify();
      position.set_state(value);
    });
    this.add_action(position);

    this.add_action(
      Gio.SimpleAction.new_stateful(
        "note-uuid",
        null,
        GLib.Variant.new_string(this.note.uuid),
      ),
    );

    const set_font_size = Gio.SimpleAction.new_stateful(
      "set-font-size",
      GLib.VariantType.new("i"),
      GLib.Variant.new_int32(this.note.font_size),
    );
    set_font_size.connect("change-state", (_, value) => {
      if (!value) return;

      const size = (value as GLib.Variant).get_int32();
      this.note.font_size = size;
      this.note.modified_date = new Date();
      this.apply_font_size();
      set_font_size.set_state(GLib.Variant.new_int32(size));
    });
    this.add_action(set_font_size);

    const rename = Gio.SimpleAction.new("rename", null);
    rename.connect("activate", () => this._title_label.start_editing());
    this.add_action(rename);

    for (const [name, tag] of this.view.actions) {
      const action = Window.menu_formats.includes(name)
        ? new Gio.SimpleAction({ name, state: GLib.Variant.new_boolean(false) })
        : Gio.SimpleAction.new(name, null);

      action.connect("activate", () => {
        this.view.apply_tag(tag);
        this.check_tags();
      });
      this.add_action(action);
    }

    const text_size = new Gio.SimpleAction({
      name: "text-size",
      parameter_type: GLib.VariantType.new("s"),
      state: GLib.Variant.new_string("normal"),
    });
    text_size.connect("activate", (_action, parameter) => {
      if (!parameter) return;

      this.view.set_text_size(parameter.deepUnpack<TextSize>());
      this.check_tags();
    });
    this.add_action(text_size);

    const checklist = Gio.SimpleAction.new("checklist", null);
    checklist.connect("activate", () => this.view.toggle_checklist());
    this.add_action(checklist);

    const bullets = Gio.SimpleAction.new("bullets", null);
    bullets.connect("activate", () => this.view.toggle_bullets());
    this.add_action(bullets);

    const toggle_checkbox = Gio.SimpleAction.new("toggle-checkbox", null);
    toggle_checkbox.connect("activate", () => this.view.toggle_checkboxes());
    this.add_action(toggle_checkbox);
  }

  apply_font_size() {
    const class_prefix = "font-size-";
    for (const css of this._text.get_css_classes()) {
      if (css.startsWith(class_prefix)) {
        this._text.remove_css_class(css);
      }
    }
    this._text.add_css_class(`${class_prefix}${this.note.font_size}`);
  }

  set_style(style: Style, is_init = false) {
    for (const s of this._container.get_css_classes()) {
      if (s.startsWith("style-") && s !== `style-specifity`) {
        this._container.remove_css_class(s);
      }
    }

    this._container.add_css_class(`style-${Style[style]}`);

    if (!is_init) this.note.modified_date = new Date();
  }

  delete() {
    confirm_delete(this, () => {
      this.deleted = true;
      this.emit("deleted", this.note.uuid);
      console.log("emitted deleted");
    });
  }
}
