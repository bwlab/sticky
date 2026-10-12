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

// Adapted from https://github.com/sonnyp/troll/blob/63392a57392fb8ed944e859269a3751f649f64ec/src/widgets/ThemeSelector.js

import GObject from "gi://GObject";
import Gtk from "gi://Gtk?version=4.0";
import Gdk from "gi://Gdk?version=4.0";

import { Style, StyleNames, styles } from "./util.js";

const { vprintf } = imports.format;

let provider: Gtk.CssProvider | null = null;

class StyleButton extends Gtk.CheckButton {
  style: Style;

  constructor(style: Style, group?: Gtk.CheckButton) {
    const style_name = Style[style];
    const display_name = StyleNames.get(style)!;

    super({
      // focus_on_click: false,
      tooltip_text: vprintf(_("Switch to %s style"), [display_name]),
      width_request: 44,
      height_request: 44,
      hexpand: true,
      halign: Gtk.Align.CENTER,
    });

    if (group) {
      this.set_group(group);
    }

    this.style = style;

    this.set_css_classes([
      ...this.css_classes,
      `style-selector`,
      `style-${style_name}`,
    ]);
  }

  static {
    GObject.registerClass({}, this);
  }
}

const COLUMNS = 5;

export class StyleSelector extends Gtk.Box {
  grid = new Gtk.Grid({
    row_spacing: 8,
    column_spacing: 8,
    column_homogeneous: true,
    hexpand: true,
  });

  private _style: Style;

  get style() {
    return this._style;
  }

  set style(style: Style) {
    if (this._style === style) {
      return;
    }

    this._style = style;
    this.notify("style");
    this.emit("style-changed", style);
  }

  constructor(params: { style?: Style } = {}) {
    super();

    this.append(this.grid);

    this._style = this.style = params.style ?? Style.yellow;
    this.build_styles();
    this.hexpand = true;

    if (!provider) {
      provider = new Gtk.CssProvider();
      provider.load_from_resource(
        "/com/vixalien/sticky/style-selector.css",
      );
      Gtk.StyleContext.add_provider_for_display(
        Gdk.Display.get_default()!,
        provider,
        Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION,
      );
    }
  }

  build_styles() {
    let group: Gtk.CheckButton | undefined = undefined;

    styles.forEach((style, i) => {
      const button = new StyleButton(style, group);
      button.active = style === this.style;
      button.connect("toggled", () => {
        if (button.active) {
          this.style = style;
        }
      });
      if (!group) {
        group = button;
      }

      this.grid.attach(button, i % COLUMNS, Math.floor(i / COLUMNS), 1, 1);
    });
  }

  static {
    GObject.registerClass({
      CssName: "styleselector",
      Properties: {
        style: GObject.ParamSpec.string(
          "style",
          "Style",
          "The current style",
          GObject.ParamFlags.READWRITE,
          "window",
        ),
      },
      Signals: {
        "style-changed": {
          param_types: [GObject.TYPE_UINT],
        },
      },
    }, this);
  }
}
