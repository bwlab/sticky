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

import Gdk from "gi://Gdk?version=4.0";
import GLib from "gi://GLib";
import GObject from "gi://GObject";
import Gtk from "gi://Gtk?version=4.0";
import Pango from "gi://Pango";
import Adw from "gi://Adw?version=1";

import { find } from "linkifyjs";

import {
  clipboard_has_image,
  load_texture,
  read_clipboard_textures,
  save_texture,
  textures_from_files,
} from "./images.js";
import { IMAGE_PLACEHOLDER, ITag, Note, NoteImage } from "./util.js";
import { Style } from "./util.js";

/**
 * Checklist and bullet items are stored as plain characters at the start of a
 * line. This keeps notes readable when copied elsewhere, and means the note
 * format doesn't change: a checklist survives saving, searching and the
 * read-only previews shown in the "All Notes" window for free.
 */
export const UNCHECKED = "☐";
export const CHECKED = "☑";
export const BULLET = "•";

const CHECK_PREFIXES = [`${UNCHECKED} `, `${CHECKED} `];
const BULLET_PREFIXES = [`${BULLET} `];
const LIST_PREFIXES = [...CHECK_PREFIXES, ...BULLET_PREFIXES];

// The images of a note are child anchors in its buffer, each one standing for
// a file. They are kept here rather than in fields because the buffer is filled
// while the view is still being constructed, before fields are initialized.
const image_anchors = new WeakMap<object, Map<Gtk.TextChildAnchor, string>>();
const anchors_of = (view: object) => {
  let anchors = image_anchors.get(view);
  if (!anchors) image_anchors.set(view, anchors = new Map());
  return anchors;
};

export type TextSize = "normal" | "small" | "large" | "larger";

class AbstractStickyNote extends Gtk.TextView {
  static {
    GObject.registerClass(
      {
        GTypeName: "AbstractStickyNote",
        Signals: {
          "tag-toggle": {
            param_types: [GObject.TYPE_STRING, GObject.TYPE_BOOLEAN],
          },
          "selection-changed": {},
          "link-selected": {
            param_types: [GObject.TYPE_STRING],
          },
          "link-unselected": {},
        },
      },
      this,
    );
  }

  bold_tag = Gtk.TextTag.new("bold");
  underline_tag = Gtk.TextTag.new("underline");
  italic_tag = Gtk.TextTag.new("italic");
  strikethrough_tag = Gtk.TextTag.new("strikethrough");
  monospace_tag = Gtk.TextTag.new("monospace");
  highlight_tag = Gtk.TextTag.new("highlight");
  header_tag = Gtk.TextTag.new("header");
  small_tag = Gtk.TextTag.new("small");
  large_tag = Gtk.TextTag.new("large");
  larger_tag = Gtk.TextTag.new("larger");
  link_tag = Gtk.TextTag.new("link");

  /**
   * Draws the list markers larger than the text they sit next to, so a
   * checkbox looks like something you can tick.
   *
   * It is deliberately nameless: the note format stores tags by name, and this
   * one is presentation the app re-applies on its own rather than something
   * worth saving.
   */
  marker_tag = Gtk.TextTag.new(null);

  actions = [
    ["bold", this.bold_tag],
    ["underline", this.underline_tag],
    ["italic", this.italic_tag],
    ["strikethrough", this.strikethrough_tag],
    ["monospace", this.monospace_tag],
    ["highlight", this.highlight_tag],
    ["header", this.header_tag],
  ] as [string, Gtk.TextTag][];

  size_tags = [
    ["small", this.small_tag],
    ["large", this.large_tag],
    ["larger", this.larger_tag],
  ] as [Exclude<TextSize, "normal">, Gtk.TextTag][];

  /**
   * Tags that all change the size of the text, so only one of them may be
   * applied to a range at a time.
   */
  private get scale_tags() {
    return [this.header_tag, ...this.size_tags.map(([, tag]) => tag)];
  }

  _note?: Note;

  get note() {
    return this._note;
  }

  set note(note: Note | undefined) {
    if (!note) return;

    this._note = note;
    this.clear_tags();
    this.buffer.text = note.content;
    this.restore_images(note);
    this.init_tags(note.tags);
    this.update_markers();

    this.update_link_tag_color();
    this.remove_listeners();
    this.init_listeners();
  }

  /** Puts the images of the note back where its content has placeholders. */
  restore_images(_note: Note) {}

  /**
   * The content of the note: the text, with a placeholder for each image, so
   * that the offsets of tags and images match the ones in the buffer.
   */
  get content_text() {
    return this.buffer.get_slice(
      this.buffer.get_start_iter(),
      this.buffer.get_end_iter(),
      true,
    );
  }

  listeners = new Set<number>();

  init_listeners() {
    if (!this.note) return;

    this.listeners.add(this.note.connect("notify::style", () => {
      this.update_link_tag_color();
    }));
  }

  remove_listeners() {}

  style_manager: Adw.StyleManager;

  constructor(
    note?: Note,
  ) {
    super();

    this.style_manager = Adw.StyleManager.get_default();

    this.register_tags();
    if (note) this.note = note;

    this.buffer.connect("mark-set", (_buffer, _loc, mark) => {
      if (!this.note) return;

      if (
        mark.name === "insert" || mark.name === "selection_bound" ||
        this.buffer.get_iter_at_mark(mark).equal(
          this.buffer.get_iter_at_mark(this.buffer.get_insert()),
        )
      ) {
        this.emit("selection-changed");
      }

      this.check_link_selected();
    });
  }

  last_link: number | false = false;
  last_link_end: number = 0;

  check_link_selected() {
    const has_link = this.has_tag(this.link_tag);

    if (has_link) {
      const start = this.buffer.get_iter_at_offset(has_link);

      if (!start.starts_tag(this.link_tag)) {
        start.backward_to_tag_toggle(this.link_tag);
      }

      const end = start.copy();
      end.forward_to_tag_toggle(this.link_tag);

      if (
        start.get_offset() !== this.last_link ||
        end.get_offset() !== this.last_link_end
      ) {
        const text = this.buffer.get_text(start, end, false)
          .replace(/\u200B/g, "");

        this.emit("link-selected", text);

        this.last_link = start.get_offset();
        this.last_link_end = end.get_offset();
      }
    } else if (has_link !== this.last_link) {
      this.emit("link-unselected");
      this.last_link = false;
    }
  }

  update_link_tag_color() {
    if (!this.note) return;
    let color;

    if (this.style_manager.dark) {
      const accent_color = this.get_style_context().lookup_color(
        "accent_fg_color",
      );
      if (accent_color[0]) {
        color = accent_color[1].to_string();
      } else {
        color = "#0000ff";
      }
    } else {
      const accent_color = this.get_style_context().lookup_color(
        `link_color_${Style[this.note.style]}`,
      );
      if (accent_color[0]) {
        color = accent_color[1].to_string();
      } else {
        color = "#0000ff";
      }
    }

    this.link_tag.foreground = color;
  }

  private register_tags() {
    this.bold_tag.weight = Pango.Weight.BOLD;
    this.underline_tag.underline = Pango.Underline.SINGLE;
    this.italic_tag.style = Pango.Style.ITALIC;
    this.strikethrough_tag.strikethrough = true;
    this.monospace_tag.family = "Monospace";

    // the highlighter also sets a foreground, as the notes it is used on can
    // be dark, and white on yellow is unreadable
    this.highlight_tag.background = "#f9f06b";
    this.highlight_tag.foreground = "#241f31";

    this.header_tag.scale = 1.5;
    this.header_tag.pixels_above_lines = 14;
    this.header_tag.pixels_below_lines = 4;

    // the same steps Pango uses for small/large/x-large
    this.small_tag.scale = 1 / 1.2;
    this.large_tag.scale = 1.2;
    this.larger_tag.scale = 1.2 * 1.2;

    this.link_tag.underline = Pango.Underline.SINGLE;

    this.marker_tag.scale = 1.4;
    this.buffer.tag_table.add(this.marker_tag);

    if (this.style_manager.system_supports_color_schemes) {
      this.style_manager.connect(
        "notify::dark",
        this.update_link_tag_color.bind(this),
      );
    }

    for (const [, tag] of this.actions) {
      this.buffer.tag_table.add(tag);
    }

    for (const [, tag] of this.size_tags) {
      this.buffer.tag_table.add(tag);
    }

    this.buffer.tag_table.add(this.link_tag);
  }

  init_tags(tags: Note["tags"]) {
    for (const tag of tags) {
      const start = this.buffer.get_iter_at_offset(tag.start);
      const end = this.buffer.get_iter_at_offset(tag.end);
      this.buffer.apply_tag_by_name(tag.name, start, end);
    }
  }

  has_tag(tag: Gtk.TextTag) {
    let [selection, start, end] = this.buffer.get_selection_bounds();
    if (!selection) {
      start = this.buffer.get_iter_at_mark(
        this.buffer.get_insert(),
      );
      end = start.copy();
      end.forward_cursor_position();
    }

    do {
      if (start.has_tag(tag) !== false) {
        return start.get_offset();
      }
      start.forward_char();
    } while (start.compare(end) < 0);

    return false;
  }

  /**
   * The range the next formatting operation applies to: the selection, or a
   * spot at the cursor the user can type into.
   */
  private get_format_range(): [Gtk.TextIter, Gtk.TextIter] {
    let [selection, start, end] = this.buffer.get_selection_bounds();

    if (!selection) {
      /**
       * If the user has not selected anything, we insert zero-width spaces
       * around the cursor and mark them as the start and end of the selection.
       */
      const get_cursor_position = () => this.buffer.cursor_position;

      this.buffer.insert_at_cursor("\u200B\u200B\u200B", 9);
      start = this.buffer.get_iter_at_offset(get_cursor_position() - 3);
      end = this.buffer.get_iter_at_offset(get_cursor_position() - 1);
      const selec = this.buffer.get_iter_at_offset(get_cursor_position());
      selec.backward_chars(2);
      this.buffer.place_cursor(selec);
    }

    return [start, end];
  }

  /** Tags that cannot be combined with `tag` on the same range. */
  private conflicting_tags(tag: Gtk.TextTag) {
    const scale_tags = this.scale_tags;

    if (!scale_tags.includes(tag)) return [];

    return scale_tags.filter((other) => other !== tag);
  }

  apply_tag(tag: Gtk.TextTag) {
    const [start, end] = this.get_format_range();

    const has_tag = this.has_tag(tag);

    if (has_tag !== false) {
      this.buffer.remove_tag(tag, start, end);
    } else {
      for (const other of this.conflicting_tags(tag)) {
        this.buffer.remove_tag(other, start, end);
        this.emit("tag-toggle", other.name, false);
      }

      this.buffer.apply_tag(tag, start, end);
    }

    this.emit("tag-toggle", tag.name, has_tag === false);
  }

  get_text_size(): TextSize {
    for (const [name, tag] of this.size_tags) {
      if (this.has_tag(tag) !== false) return name;
    }

    return "normal";
  }

  set_text_size(size: TextSize) {
    const [start, end] = this.get_format_range();

    for (const [name, tag] of this.size_tags) {
      this.buffer.remove_tag(tag, start, end);
      this.emit("tag-toggle", name, false);
    }

    // a header has a size of its own, so picking a size drops it
    this.buffer.remove_tag(this.header_tag, start, end);
    this.emit("tag-toggle", "header", false);

    if (size === "normal") return;

    const tag = this.size_tags.find(([name]) => name === size)?.[1];
    if (!tag) return;

    this.buffer.apply_tag(tag, start, end);
    this.emit("tag-toggle", size, true);
  }

  clear_tags() {
    const start = this.buffer.get_start_iter();
    const end = this.buffer.get_end_iter();

    for (const [name, tag] of [...this.actions, ...this.size_tags]) {
      this.buffer.remove_tag(tag, start, end);
      this.emit("tag-toggle", name, false);
    }
  }

  private get_line_text(line: number) {
    const [found, start] = this.buffer.get_iter_at_line(line);
    if (!found) return null;

    const end = start.copy();
    if (!end.ends_line()) end.forward_to_line_end();

    return this.buffer.get_text(start, end, false);
  }

  /** The list marker `line` starts with, if it is one of `prefixes`. */
  private get_line_prefix(line: number, prefixes: string[]) {
    const text = this.get_line_text(line);
    if (text === null) return null;

    return prefixes.find((prefix) => text.startsWith(prefix)) ?? null;
  }

  private get_selected_lines(): [number, number] {
    const [selection, start, end] = this.buffer.get_selection_bounds();

    if (!selection) {
      const cursor = this.buffer.get_iter_at_mark(this.buffer.get_insert());
      return [cursor.get_line(), cursor.get_line()];
    }

    return [start.get_line(), end.get_line()];
  }

  private remove_line_prefix(line: number, prefix: string) {
    const [found, start] = this.buffer.get_iter_at_line(line);
    if (!found) return;

    const end = start.copy();
    end.forward_chars(prefix.length);

    this.buffer.delete(start, end);
  }

  private add_line_prefix(line: number, prefix: string) {
    const [found, start] = this.buffer.get_iter_at_line(line);
    if (!found) return;

    this.buffer.insert(start, prefix, -1);
  }

  /**
   * Turn every selected line into a list of the given kind, or strip the
   * markers if all of them already are one.
   */
  private toggle_list(prefixes: string[]) {
    const [first, last] = this.get_selected_lines();

    let remove = true;
    for (let line = first; line <= last; line++) {
      if (!this.get_line_prefix(line, prefixes)) {
        remove = false;
        break;
      }
    }

    for (let line = first; line <= last; line++) {
      // a line is only ever one kind of list item, so swapping replaces
      const existing = this.get_line_prefix(line, LIST_PREFIXES);
      if (existing) this.remove_line_prefix(line, existing);

      if (!remove) this.add_line_prefix(line, prefixes[0]);
    }
  }

  toggle_checklist() {
    this.toggle_list(CHECK_PREFIXES);
  }

  toggle_bullets() {
    this.toggle_list(BULLET_PREFIXES);
  }

  /** Re-applies the marker styling to every list item in the note. */
  protected update_markers() {
    this.buffer.remove_tag(
      this.marker_tag,
      this.buffer.get_start_iter(),
      this.buffer.get_end_iter(),
    );

    const lines = this.buffer.get_line_count();

    for (let line = 0; line < lines; line++) {
      if (!this.get_line_prefix(line, LIST_PREFIXES)) continue;

      const [found, start] = this.buffer.get_iter_at_line(line);
      if (!found) continue;

      const end = start.copy();
      end.forward_char();

      this.buffer.apply_tag(this.marker_tag, start, end);
    }
  }

  /** Whether a click at this spot lands on a checkbox rather than on text. */
  protected is_checkbox_at(iter: Gtk.TextIter) {
    return iter.get_line_offset() <= 1 &&
      this.get_line_prefix(iter.get_line(), CHECK_PREFIXES) !== null;
  }

  /** Tick or untick every checkbox in the selection. */
  toggle_checkboxes() {
    const [first, last] = this.get_selected_lines();

    for (let line = first; line <= last; line++) {
      this.toggle_checkbox(line);
    }
  }

  /** Tick or untick the checkbox `line` starts with. */
  toggle_checkbox(line: number) {
    const prefix = this.get_line_prefix(line, CHECK_PREFIXES);
    if (!prefix) return false;

    const [found, start] = this.buffer.get_iter_at_line(line);
    if (!found) return false;

    const end = start.copy();
    end.forward_char();

    // swapping one character for another leaves every tag offset in the note
    // untouched
    this.buffer.delete(start, end);
    this.buffer.insert(start, prefix[0] === UNCHECKED ? CHECKED : UNCHECKED, -1);

    return true;
  }

  get_tags() {
    const tags: Note["tags"] = [];

    this.buffer.get_tag_table().foreach((tag) => {
      // nameless tags are styling the app applies itself, not part of the note
      if (!tag.name) return;

      const start = this.buffer.get_start_iter();

      do {
        if (!start.starts_tag(tag)) continue;

        const begin = start.copy();
        start.forward_to_tag_toggle(tag);

        tags.push({
          name: tag.name,
          start: begin.get_offset(),
          end: start.get_offset(),
        });
      } while (start.forward_to_tag_toggle(tag));
    });

    return tags;
  }
}

export class ReadonlyStickyNote extends AbstractStickyNote {
  static {
    GObject.registerClass(
      {
        GTypeName: "ReadonlyStickyNote",
      },
      this,
    );
  }

  constructor(note?: Note) {
    super(note);

    this.note = note;
    this.editable = false;
    this.cursor_visible = false;
    this.show_content();
  }

  set note(note: Note | undefined) {
    this._note = super.note = note;

    this.show_content();
  }

  get note() {
    return super.note;
  }

  clip_content(content: string) {
    const MAX_LINES = 5;
    const MAX_CHARS = 240;

    content = content.replace(/\s+$/, "");

    let cut = content.split("\n").slice(0, MAX_LINES).join("\n");

    if (cut.length > MAX_CHARS) {
      cut = cut.slice(0, MAX_CHARS);
    }

    return cut.length < content.length ? `${cut}…` : cut;
  }

  show_content() {
    if (!this.note) return;

    if (!this.note.content.replace(/\s/g, "")) {
      this.buffer.text = "";
      this.buffer.insert_markup(
        this.buffer.get_start_iter(),
        `<i>(${_("Empty Note")})</i>`,
        -1,
      );
    } else {
      this.buffer.text = this.clip_content(
        this.note!.content.split(IMAGE_PLACEHOLDER).join("▣"),
      );
      this.clear_tags();
      this.init_tags(this.note!.tags);
      this.update_markers();
    }
  }

  init_listeners() {
    super.init_listeners();

    if (!this.note) return;

    this.listeners.add(this.note.connect("notify::content", () => {
      this.show_content();
    }));

    this.listeners.add(this.note.connect("notify::tag_list", () => {
      this.clear_tags();
      this.init_tags(this.note!.tags);
      this.update_markers();
    }));
  }

  remove_listeners() {
    super.remove_listeners();

    if (!this.note) return;

    for (const listener of this.listeners) {
      this.note!.disconnect(listener);
    }

    this.listeners.clear();
  }
}

export class WriteableStickyNote extends AbstractStickyNote {
  static {
    GObject.registerClass(
      {
        GTypeName: "WriteableStickyNote",
      },
      this,
    );
  }

  updating = false;
  restoring = false;
  source: number | null = null;

  // the text views showing this buffer, with the widget each one has for an image
  image_views = new Map<Gtk.TextView, Map<Gtk.TextChildAnchor, Gtk.Widget>>();

  restore_images(note: Note) {
    const anchors = anchors_of(this);
    anchors.clear();
    if (!note.images.length) return;

    this.restoring = true;
    try {
      for (const { offset, file } of note.images) {
        const at = this.buffer.get_iter_at_offset(offset);
        // gunichar comes back as a string or a number depending on the binding
        const char = at.get_char() as unknown as string | number;
        if (char !== IMAGE_PLACEHOLDER && char !== IMAGE_PLACEHOLDER.codePointAt(0)) {
          continue;
        }

        const end = at.copy();
        end.forward_char();
        this.buffer.delete(at, end);

        const anchor = this.buffer.create_child_anchor(
          this.buffer.get_iter_at_offset(offset),
        );
        anchors.set(anchor, file);
      }
    } finally {
      this.restoring = false;
    }
  }

  /** The images the buffer holds right now, as they are saved in the note. */
  collect_images() {
    const anchors = anchors_of(this);
    const images: NoteImage[] = [];

    for (const anchor of [...anchors.keys()]) {
      if (anchor.get_deleted()) anchors.delete(anchor);
    }
    if (!anchors.size) return images;

    const at = this.buffer.get_start_iter();
    do {
      const file = anchors.get(at.get_child_anchor() as Gtk.TextChildAnchor);
      if (file) images.push({ offset: at.get_offset(), file });
    } while (at.forward_char());

    return images;
  }

  on_content_changed() {
    this.update_links();
    this.update_markers();
    this.sync_images();
    const content = this.content_text;
    if (content == this.note!.content) return;
    this.change("content", content);
  }

  sync_images() {
    const note = this.note;
    if (!note) return;

    const images = this.collect_images();
    if (JSON.stringify(images) !== JSON.stringify(note.images)) {
      note.images = images;
    }
  }

  /** Puts the image `file` in the note, at the cursor unless told where. */
  insert_image(file: string, at?: Gtk.TextIter) {
    const iter = at ?? this.buffer.get_iter_at_mark(this.buffer.get_insert());

    // the change is handled once the anchor is known, or its image would be
    // left out of the note until the next edit
    this.restoring = true;
    const anchor = this.buffer.create_child_anchor(iter);
    anchors_of(this).set(anchor, file);
    this.restoring = false;
    this.on_content_changed();

    for (const text_view of this.image_views.keys()) {
      this.add_picture(text_view, anchor, file);
    }
  }

  insert_textures(textures: Gdk.Texture[], at?: Gtk.TextIter) {
    for (const texture of textures) this.insert_image(save_texture(texture), at);
  }

  private add_picture(
    text_view: Gtk.TextView,
    anchor: Gtk.TextChildAnchor,
    file: string,
  ) {
    const texture = load_texture(file);
    const widget: Gtk.Widget = texture
      ? new Gtk.Picture({
        paintable: texture,
        can_shrink: true,
        content_fit: Gtk.ContentFit.SCALE_DOWN,
        halign: Gtk.Align.START,
      })
      : new Gtk.Label({ label: `(${_("Image not found")})` });

    widget.add_css_class("note-image");
    (widget as any).natural_size = texture
      ? [texture.get_width(), texture.get_height()]
      : null;

    text_view.add_child_at_anchor(widget, anchor);
    this.fit_picture(text_view, widget);
    this.image_views.get(text_view)!.set(anchor, widget);
  }

  /**
   * A text view gives an anchored widget its minimum size, which for a picture
   * that can shrink is nothing, so the size is set here: the picture's own,
   * or less when the note is narrower.
   */
  private fit_picture(text_view: Gtk.TextView, widget: Gtk.Widget) {
    const natural = (widget as any).natural_size as [number, number] | null;
    if (!natural) return;

    const [width, height] = natural;
    const room = text_view.get_width() - text_view.left_margin -
      text_view.right_margin;
    const fitted = room > 0 ? Math.max(32, Math.min(width, room)) : width;

    widget.width_request = fitted;
    widget.height_request = Math.max(1, Math.round(height * fitted / width));
  }

  /**
   * Shows the images in `text_view`, a view of this note's buffer, and lets
   * images be pasted or dropped into it.
   *
   * Like the checkbox gesture, this is done on the view the user works in,
   * because the buffer is shared but widgets belong to one text view.
   */
  attach_images(text_view: Gtk.TextView) {
    this.image_views.set(text_view, new Map());

    for (const [anchor, file] of anchors_of(this)) {
      if (!anchor.get_deleted()) this.add_picture(text_view, anchor, file);
    }

    // an image never grows wider than the note
    let last_width = -1;
    text_view.add_tick_callback(() => {
      const width = text_view.get_width();
      if (width === last_width) return GLib.SOURCE_CONTINUE;
      last_width = width;

      for (const widget of this.image_views.get(text_view)!.values()) {
        this.fit_picture(text_view, widget);
      }
      return GLib.SOURCE_CONTINUE;
    });

    text_view.connect("paste-clipboard", () => this.paste_images(text_view));

    const drop = Gtk.DropTarget.new(GObject.TYPE_NONE, Gdk.DragAction.COPY);
    drop.set_gtypes([Gdk.FileList.$gtype, Gdk.Texture.$gtype]);
    drop.connect("drop", (_drop, value, x, y) => {
      const textures = value instanceof Gdk.Texture
        ? [value]
        : textures_from_files(value?.get_files?.() ?? []);
      if (!textures.length) return false;

      const [bx, by] = text_view.window_to_buffer_coords(
        Gtk.TextWindowType.TEXT,
        x,
        y,
      );
      const [found, at] = text_view.get_iter_at_location(bx, by);
      this.insert_textures(textures, found ? at : undefined);
      return true;
    });
    text_view.add_controller(drop);
  }

  private paste_images(text_view: Gtk.TextView) {
    const clipboard = text_view.get_clipboard();
    const kind = clipboard_has_image(clipboard);
    if (!kind.texture && !kind.files) return;

    // whether it holds images is only known once it is read, so the default
    // paste is always replaced, and puts text back when there are no images
    text_view.stop_emission_by_name("paste-clipboard");

    read_clipboard_textures(clipboard).then((textures) => {
      if (textures.length) {
        this.insert_textures(textures);
      } else if (kind.files) {
        clipboard.read_text_async(null, (_clipboard, result) => {
          const text = clipboard.read_text_finish(result);
          if (text) this.buffer.insert_at_cursor(text, -1);
        });
      }
    }).catch(logError);
  }

  get note() {
    return super.note;
  }

  set note(note: Note | undefined) {
    this.updating = true;
    super.note = note;
  }

  change<T extends keyof Note>(key: T, value: Note[T]) {
    if (!this.note) return;
    this.note[key] = value;
    this.note.modified_date = new Date();
  }

  constructor(note?: Note) {
    super(note);

    this.buffer.connect_after("insert-text", (buffer, loc, text, length) => {
      this.on_text_inserted(buffer, loc, text, length);
    });

    this.buffer.connect("changed", () => {
      // images are being put back one by one: the content hasn't changed
      if (this.restoring) return;

      if (this.updating) {
        this.updating = false;
        return;
      }
      this.on_content_changed();
    });

    this.buffer.connect("mark-set", () => {
      if (!this.note) return;

      const tags = this.get_tags();
      if (compare_tags(tags, this.note.tags)) return;
      this.note.tags = tags;
    });

  }

  /**
   * Lets a checkbox be ticked by clicking the box itself.
   *
   * The window shows the note in a text view of its own that only shares this
   * one's buffer, so the view the clicks arrive on has to be passed in.
   */
  attach_checkbox_gesture(text_view: Gtk.TextView) {
    const gesture = new Gtk.GestureClick();

    gesture.connect("released", (_gesture, _n_press, x, y) => {
      // a click that ended a selection was a drag, not a tick
      if (this.buffer.get_has_selection()) return;

      const [buffer_x, buffer_y] = text_view.window_to_buffer_coords(
        Gtk.TextWindowType.WIDGET,
        x,
        y,
      );

      const [over_text, iter] = text_view.get_iter_at_location(
        buffer_x,
        buffer_y,
      );

      // only the box toggles, so the rest of the line stays editable
      if (!over_text || !this.is_checkbox_at(iter)) return;

      this.toggle_checkbox(iter.get_line());
    });

    text_view.add_controller(gesture);
    this.attach_checkbox_cursor(text_view);
  }

  /** Points the cursor at the boxes, so they read as something to click. */
  private attach_checkbox_cursor(text_view: Gtk.TextView) {
    const motion = new Gtk.EventControllerMotion();

    const update = (x: number, y: number) => {
      const [buffer_x, buffer_y] = text_view.window_to_buffer_coords(
        Gtk.TextWindowType.WIDGET,
        x,
        y,
      );

      const [over_text, iter] = text_view.get_iter_at_location(
        buffer_x,
        buffer_y,
      );

      text_view.set_cursor_from_name(
        over_text && this.is_checkbox_at(iter) ? "pointer" : "text",
      );
    };

    motion.connect("motion", (_motion, x, y) => update(x, y));
    motion.connect("leave", () => text_view.set_cursor_from_name("text"));

    text_view.add_controller(motion);
  }

  clear_links() {
    this.buffer.remove_tag(
      this.link_tag,
      this.buffer.get_start_iter(),
      this.buffer.get_end_iter(),
    );
    this.emit("tag-toggle", "link", false);
  }

  update_links() {
    const text = this.content_text;

    this.clear_links();

    find(text)
      .forEach((match) => {
        const start = this.buffer.get_iter_at_offset(match.start);
        const end = this.buffer.get_iter_at_offset(match.end);

        this.buffer.apply_tag(this.link_tag, start, end);
      });

    this.emit("tag-toggle", "link", true);
  }

  clear_tags() {
    super.clear_tags();
    // this.change("tags", []);
  }

  apply_tag(tag: Gtk.TextTag) {
    super.apply_tag(tag);
    const tags = this.get_tags();
    if (compare_tags(tags, this.note!.tags)) return;
    this.change("tags", tags);
  }

  on_text_inserted(
    buffer: Gtk.TextBuffer,
    loc: Gtk.TextIter,
    text: string,
    length: number,
  ) {
    if (text === "\n") {
      const start_iter = loc.copy();
      start_iter.backward_char();
      start_iter.set_line_offset(0);

      const end_iter = start_iter.copy();
      end_iter.forward_chars(2);
      const chars = buffer.get_text(start_iter, end_iter, false);

      const simple_regex_pattern = new RegExp(
        `^[-+*${BULLET}${UNCHECKED}${CHECKED}] $`,
      );
      if (simple_regex_pattern.test(chars)) {
        // a ticked item carries on as a fresh, unticked one
        const bullet = chars[0] === CHECKED ? UNCHECKED : chars[0];
        const line_end = loc.copy();
        line_end.backward_char();
        if (line_end.get_line_offset() === 2) {
          start_iter.set_line_offset(0);
          buffer.delete(start_iter, loc);
        } else {
          buffer.insert(loc, bullet + " ", -1);
        }
      } else {
        const search_limit = start_iter.copy();
        const search_end = start_iter.copy();
        search_limit.forward_chars(10);

        search_end.forward_find_char((ch) => ch === " ", search_limit);
        search_end.forward_char();
        const chars = buffer.get_text(start_iter, search_end, false);

        const ordered_regex_pattern = /^\d+\. $/;
        if (ordered_regex_pattern.test(chars)) {
          const current_order = parseInt(chars.slice(0, -2));
          const new_order = current_order + 1;
          const new_order_bullet = `${new_order}. `;
          const line_end = loc.copy();
          line_end.backward_char();

          if (
            line_end.get_line_offset() === current_order.toString().length + 2
          ) {
            start_iter.set_line_offset(0);
            buffer.delete(start_iter, loc);
          } else {
            buffer.insert(loc, new_order_bullet, -1);
          }
        }
      }
    }
  }
}

const compare_tags = (a: ITag[], b: ITag[]) => {
  if (a.length != b.length) return false;
  for (const tag of a) {
    if (
      !b.find((t) =>
        t.name == tag.name && t.start == tag.start && t.end == tag.end
      )
    ) {
      return false;
    }
  }
  return true;
};
