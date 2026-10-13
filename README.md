<div align="center">
<img src="/data/icons/hicolor/scalable/apps/it.bwlab.StickyNotes.svg" height="64">

# Sticky Notes+

[![Please do not theme this app](https://stopthemingmy.app/badge.svg)](https://stopthemingmy.app) 

Sticky Notes+ is a note taking application for the GNOME desktop, with images,
twenty colors, checklists and a tray icon. It is written in GJS and uses GTK4.

It is a fork of [Sticky Notes](https://github.com/vixalien/sticky) by Angelo
Verlain and Christopher Davis, and keeps its MIT license.

![All notes](data/resources/screenshots/notes.png)
![Note](data/resources/screenshots/note.png)

</div>

## Installation

### Flatpak

Download `it.bwlab.StickyNotes.flatpak` from the
[releases](https://github.com/bwlab/sticky/releases) and install it:

```sh
flatpak install --user it.bwlab.StickyNotes.flatpak
```

The app id is `it.bwlab.StickyNotes`, so it runs next to the original Sticky
Notes and keeps its own notes. On the first start with no notes it offers to
import the notes of the original app: pick its notes folder
(`~/.var/app/com.vixalien.sticky/data/com.vixalien.sticky/notes`) in the file
chooser. The original notes are only read, never changed. You can do it later
from the menu, *Import from Original Sticky Notes…*. Notes exported from this
app can be brought back with *Import Notes…*.

### GNOME Shell extension (optional)

Wayland apps cannot stay above other windows or choose where they open, so
those two features come from a small extension in [`extension/`](extension).
Copy it to `~/.local/share/gnome-shell/extensions/sticky-notes-extras@bwlab.it`,
enable it and log out and in again.

The tray icon needs a host for status notifier items, such as the
*AppIndicator and KStatusNotifierItem Support* extension on GNOME.

### From source

To install from source, you need `nodejs` and `yarn` installed. Then, you need
to clone the repository by being careful to also pull in the submodules:

```sh
git clone https://github.com/bwlab/sticky.git --recurse-submodules
```

Open the project in GNOME Builder and click "Run Project".

> Note: Other non-Flatpak environments are no longer supported. This is because
Sticky Notes uses an upcoming version of libadwaita.
