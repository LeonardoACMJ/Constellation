# Constellation

A portable system-tray radial launcher. Stars launch programs, shortcuts, URLs, folders, and scripts; nested Constellations organize them.

The first Windows native application is implemented. The [product and behavior spec](docs/SPEC.md) defines intended behavior and acceptance checks; [the user guide](docs/USER-GUIDE.md) explains this preview's controls and limits. The [verification record](docs/VERIFICATION.md) separates automated evidence from checks still needing hands-on review. Linux portability is a design constraint; a Linux desktop adapter is not implemented yet.

The [browser interaction prototype](prototype/README.md) is an experiment, not the native app. Its paging, edge fitting, and delay controls are superseded by the current spec.

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

On a Windows desktop, run `python tests/native_smoke.py` after building to exercise input consumption, launch/cancellation, fullscreen pass-through, cursor restoration, and configuration saving in an isolated `.local` folder. It temporarily creates its own foreground test window and restores the cursor afterward. Pillow is optional for screenshots; no test opens a browser.

Runtime configuration and other app-managed files belong beside the distributed executable. Build output and local runtime data are ignored by Git.

The editor supports native file/folder pickers, dropped files, quoted argument fields, bounded undo/redo, and validated configuration import/export. Missing targets and layout overflow remain advisory.

For repeatable large-tree measurements, run `cargo run --release --example stress` and `python tests/large_editor_smoke.py`. The first measures the portable core; the second measures a real native full-preview paint. The [personalization study](docs/PERSONALIZATION-STUDY.md) examines themes, custom colors/icons, and future application actions; these are proposals, not implemented settings.

When the current portable copy is running, package an update separately with `python scripts/package.py --staging-dir portable/Constellation-update`. The archive version comes from Cargo metadata. Close the editor and tray app before replacing the installed executable, and preserve its `data` directory.
