# Perora

[日本語](README.md) | English

A screenshot tool for Windows: capture any part of every monitor, draw on it, then copy or save it.

The name is coined from Latin *per* (through) and *ora* (edges): through the edges between screens.

## Features

- **Captures across monitors.** The screen is held still on every monitor at once, and you choose what to keep: a window, a rectangle (across monitors if you like), a monitor, or all of them in one image. A window is captured on its own, without what lay in front of it.
- **An editor built in.** Pen, marker, shapes and arrows, text, numbered markers, blur and mosaic, crop. Anything drawn can be selected, moved, resized and restyled afterwards.
- **Copy or save as you like.** PNG, JPG or WebP, at full size or made to fit within a size, chosen once in the settings or for a single capture. Saving can write several formats at once.
- **Stays out of the way.** After a capture, a small preview in a corner of the screen (or the editor, or nothing but the clipboard: your choice). It does not take the keyboard from what you are doing.
- English and Japanese, light and dark, starts with Windows if you want it to.

## Download

Get the installer from [Releases](../../releases). Windows only, for now.

## Use

Press **Ctrl+Shift+H** (you can change it in the settings, or have Perora take **Win+Shift+S** too). Then:

| Key | Mode | |
|---|---|---|
| `1` | Smart | Click a window, or drag a rectangle |
| `2` | Window | Pick a window from a list |
| `3` | Rectangle | Drag a rectangle |
| `4` | Monitor | Click a monitor |
| `5` | All monitors | Everything, in one image |

`Esc` gives up. What you chose goes to the clipboard and shows in the preview; click the picture there to draw on it. Each window shows a short tour the first time it is opened; the **i** button shows it again.

Perora lives in the notification area. Its window (a click on the icon) has the settings.

### Uploading (optional)

Perora can also upload captures to [i.f2a.dev](https://i.f2a.dev) and put the link on the clipboard. This is off unless you sign in and switch it on; nothing leaves your machine otherwise.

## Building

Needs Rust (stable) and Node.js.

```bash
npm install
npm run tauri build      # the installer, under target/release/bundle
npm run tauri dev        # or run it while developing
cargo test --workspace
```

The texts are in [`lang/`](lang) (`en.json` is the reference). A `<code>.json` placed in `%APPDATA%\dev.f2a.perora\lang` adds a language, or changes texts, without building.

## License

[MIT](LICENSE)
