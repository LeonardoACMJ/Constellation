# Native preview verification

2026-10-23 · Windows x64 release build · existing Rust 1.96 and portable MSVC/Windows SDK activated from `E:\_DevTools`.

## Implemented

- Shared Rust configuration model, validation, radial geometry/hit testing, navigation state, and portable storage.
- Native Windows tray, global keyboard/mouse bindings, input consumption, centered overlay/cursor, restoration, and fullscreen suspension.
- Continuous subdivision with saved geometry; dots, parent/child lines, and generated pentatonic bell feedback.
- A separate native configuration process: item editing/reparenting/order, four spacing controls, monitor preview with full fan-out and advisory warnings, sound controls, and advanced JSON profile editing.
- Local save/backup/recovery and a portable ZIP packaging command. New configuration is signaled to the resident process; changes are deferred during an active radial gesture.

## Automated evidence

| Check | Evidence / scope |
|---|---|
| Rust core | 15 integration tests: subdivision, stable ancestor geometry, immediate hover, gaps, click/hold, profile isolation, validation, local saving, backup recovery, relocation, and failed-write behavior. |
| Windows desktop | `tests/native_smoke.py` creates an isolated portable copy and its own foreground probe window. It verifies Ctrl+Q consumption, ignored profiles, Space launching, Hold cancellation, modifier release, live reload, single-key and Mouse4 Hold, relative script launch, nested fan rendering, fullscreen input pass-through, cursor restoration, warning-only saving, and process exit. |
| Visual inspection | Program-owned screenshots of the radial, nested connection, full fan-out editor, and overflow warning were inspected. Screenshot backgrounds outside the radial are black in PrintWindow capture; the actual overlay uses color-key transparency. |
| Idle sample | One 60-second tray-only sample: **8.30 MiB working set**, **1.67 MiB private bytes**, **0.0000 additional CPU seconds** at the API's reporting resolution. Release x64, default sample tree, editor/radial closed; target processes excluded. This is one sample, not a worst-case guarantee. |

Local generated evidence is under `.local/native-smoke` and `.local/idle-check`, ignored by Git. `scripts/measure_idle.py` reproduces the lightweight idle sample. The Windows/Linux CI matrix tests core portability; Linux desktop integration is not implemented.

## Verify when back at the PC

- Gesture precision with the real tree, large/narrow fans, and rapid cross-branch movement.
- Cursor placement/restoration across the actual monitor arrangement, display scaling, disconnected displays, and focus changes.
- Fullscreen suspension with the user's actual games/apps. Detection is a native window/style heuristic; the automated check covers a controlled borderless fullscreen window.
- Bell timbre, loudness, mute, and rapid-note feel. Audio is bounded to one voice, so a new note replaces the previous tail. Desktop tests mute audio and do not establish perceived audio quality.
- Program, URL, folder, shortcut, and script targets using real user configurations. Automated end-to-end launch checks cover a controlled program and Python script; they do not open user browsers or folders.
- Direct portable-folder write behavior under additional policies/filesystems; existing checks cover isolated storage, malformed data, and failure with a non-directory storage path, not a complete system-wide file-write trace.

## Current limits

- Profiles are editable via the Advanced JSON window; there is no graphical binding recorder yet.
- Supported bindings are documented in the user guide. Modifier+mouse combinations and primary-button invocation are not included.
- The configuration window uses plain native controls. Very small/high-DPI desktops and keyboard accessibility need further UI review.
- Script runners must already be installed. Batch metacharacters are rejected to avoid unintended shell interpretation; a shortcut wrapper can handle advanced cases.
- No installer, startup registration, automatic updater, or Linux desktop adapter is included.
