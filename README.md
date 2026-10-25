# Constellation

A portable system-tray radial launcher. Stars launch programs, shortcuts, URLs, folders, and scripts; nested Constellations organize them.

**0.1.32 is the first public beta for Windows x64.** See the [release notes](docs/releases/0.1.32.md) for known limits and pending manual checks.

The first Windows native application is implemented. The [product and behavior spec](docs/SPEC.md) defines intended behavior and acceptance checks; [the user guide](docs/USER-GUIDE.md) explains this preview's controls and limits. The [verification record](docs/VERIFICATION.md) separates automated evidence from checks still needing hands-on review. Linux portability is a design constraint; a Linux desktop adapter is not implemented yet.

The [browser interaction prototype](prototype/README.md) is an experiment, not the native app. Its paging, edge fitting, and delay controls are superseded by the current spec.

## Install and first run

Constellation is portable: there is no installer, no AppData fallback and no PATH change. App-managed data stays in its own folder. Explicit configuration exports go to the destination you choose; **Enable Windows startup** and **Add to Start menu** optionally create user shortcuts and write no registry settings.

1. Download the ZIP from [Releases](https://github.com/LeonardoACMJ/Constellation/releases) and extract the whole folder somewhere writable.
2. Run `constellation.exe`. It lives in the notification area and shows no permanent window.
3. Press **Ctrl+Q**. The radial opens centred on the monitor under the cursor.
4. Open **Settings** in the sample menu, or right-click the tray icon, to edit your own items.

Configuration, icons and logs stay beside the executable, in `data`; explicit exports and the optional Windows shortcuts are the exceptions described above. Move the folder and it keeps working; its relative paths resolve from the executable, not from where it was launched. To update, close the app, replace `constellation.exe` and keep `data`.

The **Enable Windows startup / Disable Windows startup** and **Add to Start menu / Remove from Start menu** buttons are below Import/Export in Configure's left column; scroll down if needed. Each applies immediately, independently of Save and the configuration draft. The text switches to the reverse action after success. Opening the Start menu entry starts the tray app, or opens Configure if it is already running. After moving the portable folder, remove and add each enabled shortcut again to update its location.

## Using it

Hover a Constellation to open it, click a Star or press Space over it to launch. Escape or right click cancels; the centre returns to the root. The radial opens without taking focus, so the app you were using keeps it.

A **shortcut profile** is a binding with its own behaviour: Click opens and stays open, Hold selects on release. Each profile opens one **universe** — configurations can hold several independent trees — and may start at any Constellation inside it instead of at the root, which is a view of that tree rather than a copy of it. An item can also name the profiles that show it, so two bindings can share one tree and still differ by an item or two.

[The user guide](docs/USER-GUIDE.md) covers the editor, every target type, the configuration files and the compatibility notes that matter before downgrading.

Read [our small manifesto for portable software / manifesto por aplicações portáteis](docs/PORTABLE-MANIFESTO.md).

## License

GNU General Public License, version 3 or later — see [LICENSE](LICENSE). You may use, study, change and share it, including commercially; if you distribute a modified version, its source has to be available under the same terms. Forks stay open.

The dependencies are MIT OR Apache-2.0, both compatible with GPLv3.

## Development environment

Rust is installed on the development PC. Activate the existing portable MSVC/Windows SDK environment before Windows builds:

```cmd
call E:\_DevTools\env.cmd
cargo test
cargo build --release
python scripts/package.py
```

The unpacked app is `portable/Constellation/constellation.exe`; the distributable ZIP and checksum are in `dist`. Start the executable directly; no installer is needed.

The native application uses Rust, Serde, and direct Windows APIs for tray, input, GDI rendering, launching, and audio. The configuration panel runs in a separate process of the same executable, keeping its UI and preview work out of the resident process. The geometry/model/state tests also build on Linux.

For an explicitly authorized interactive desktop session only, `python tests/native_smoke.py` after building can be used to exercise input consumption, launch/cancellation, fullscreen pass-through, cursor restoration, and configuration saving in an isolated `.local` folder. It temporarily creates its own foreground test window and restores the cursor afterward. Do not run it while the user is using this PC; it is not a headless check. Pillow is optional for screenshots; no test opens a browser.

Runtime configuration and other app-managed files belong beside the distributed executable. Build output and local runtime data are ignored by Git.

The editor supports native file/folder pickers, dropped files, quoted argument fields, bounded undo/redo, and validated configuration import/export. Missing targets and layout overflow remain advisory.

For repeatable large-tree measurements, the device-free core example is `cargo run --release --example stress`. The interactive `python tests/large_editor_smoke.py` requires a separately authorized desktop session. The first measures the portable core; the second measures a real native full-preview paint. The [personalization study](docs/PERSONALIZATION-STUDY.md) separates implemented custom/program icons from future themes, editable colors, background artwork and application actions.

When the current portable copy is running, package an update separately with `python scripts/package.py --staging-dir portable/Constellation-update`. The archive version comes from Cargo metadata. Close the editor and tray app before replacing the installed executable, and preserve its `data` directory.
