# Native preview verification

2026-10-23 · Windows x64 release build · existing Rust 1.96 and portable MSVC/Windows SDK activated from `E:\_DevTools`.

## Current revision and manual review

The current build adds overlapping chimes, three cursor-end choices, optional extended-direction targeting, fresh-frame opening, and native editor styling/scroll buffering with a cached preview. Custom-cursor work is deferred after inconsistent desktop observations.

Desktop automation was stopped at the user's request while they use the PC. Earlier passes and samples below describe their recorded revision/conditions; they do not certify the new editor's visual behavior. The new interaction checks reached extended targeting, center-on-close, opt-out, and fresh-root reopening before the run was stopped. The final styled editor has been compiled but requires [manual review](MANUAL-CHECKS.md). No further local desktop automation should run without arranging an idle period with the user.

## Implemented

- Shared Rust configuration model, validation, radial geometry/hit testing, navigation state, and portable storage.
- Native Windows tray, global keyboard/mouse bindings, input consumption, centered overlay/cursor, restoration, and fullscreen suspension.
- Continuous subdivision with saved geometry; dots, parent/child lines, and generated pentatonic bell feedback.
- A separate native configuration process: item editing/reparenting/order, four numeric/slider spacing controls, monitor preview with full fan-out and advisory overflow/narrow/overlap warnings, sound controls, graphical shortcut profiles with keyboard/mouse recording, native pickers/drop, quoted argument fields, bounded undo/redo, validated import/export, and advanced JSON.
- Stable profile IDs with legacy migration; repaired saves preserve valid backups. Local save/backup/recovery and a portable ZIP packaging command. New configuration is signaled to the resident process; changes are deferred during an active radial gesture.
- Lock/disconnect/suspend cancellation and input resynchronization on resume; late selection checks; a dedicated blocking input thread to keep hook delivery independent of disk I/O, launch and rendering work; taskbar icon recreation; explicit empty-Constellation feedback.

## Automated evidence

| Check | Evidence / scope |
|---|---|
| Rust core | 23 portable integration tests (17 interaction/storage + six editor/stress): argument round trips, undo/redo, target inference, import/export failure preservation, deepest supported JSON loading/recovery, 10,000 selectable Stars, subdivision, stable ancestor geometry, immediate hover, gaps, click/hold, profile isolation, validation, local saving, backup recovery/repair, relocation, stable legacy profile IDs, advisory overlap warnings, and failed-write behavior. |
| Windows launch targets | Five native tests cover program arguments/cwd, batch and PowerShell scripts, a real generated `.lnk` launching a controlled target, missing targets, and URL/folder dispatch. URL/folder handlers are replaced with a recording callback; no browser or Explorer window is opened. |
| Windows desktop | 36 checks in `tests/native_smoke.py` create an isolated portable copy and its own foreground probe window. It verifies Ctrl+Q consumption, ignored profiles, Space launching, Hold cancellation, modifier release, live reload, single-key and Mouse4 Hold, relative script launch, nested fan rendering, fullscreen input pass-through, cursor restoration, warning-only saving, and process exit. Profile checks cover recording an existing Ctrl+Q without invoking the resident, duplicate validation, keyboard/mouse capture, Escape cancellation, per-profile options, live F9 Hold reload, and cancelling removals. Extended checks cover empty-folder navigation, simulated lock/sleep/display/capture interruptions, focus loss before Hold release, rapid repeated gestures, taskbar recreation, sliders, relocated launch from a foreign working directory, alternate-storage sentinels, an NTFS write-denied folder with ACL restoration, and control reachability in a 900×650 configuration window. |
| New editor workflows | Native target/runner/folder pickers, quoted/empty arguments across item switches, Undo/Redo after Save, invalid-edit retention, missing-target advice, a real HDROP batch, GUI import/export including malformed JSON, and cancelled Reload. The independent foreground probe runs its own message thread to stay responsive during driver-side dialog/file waits. |
| Large trees | Release core sample: 10,000 Stars layout **1.25 ms**, warnings **5.44 ms**, 1,000 motion events **222 ms total**. Full native preview paint (layout + GDI + warnings): 10,000 sectors **509 ms**, 4,680 sectors **330 ms**, 64-level tree **12.4 ms**. Separate samples, no input-to-display or real launch latency claim. One machine/sample; not a worst-case guarantee. Removed repeated tree searches and unnecessary same-fan overlap comparisons; fixed JSON loading at the supported 64-level node limit. |
| Visual inspection | Program-owned screenshots of the radial, nested connection, full fan-out editor, graphical profiles, empty-folder feedback, a scrolled small window, the enhanced item editor, and overflow warning were inspected. Screenshot backgrounds outside the radial are black in PrintWindow capture; the actual overlay uses color-key transparency. |
| Idle sample | Previous revision: one 60-second tray-only sample of the static-CRT build: **9.39 MiB working set**, **1.88 MiB private bytes**, **0.0156 additional CPU seconds** at the API's reporting resolution. Release x64, default sample tree, editor/radial closed; target processes excluded. This is one sample, not a worst-case guarantee. |
| Portable executable | PE imports inspected after enabling static CRT: Windows system libraries only; no VCRUNTIME140.dll / Visual C++ redistributable dependency. Packaging explicitly excludes runtime configuration and checks for dynamic MSVC runtime imports. |

Local generated evidence is under `.local/native-smoke`, `.local/idle-check`, `.local/stress`, and `.local/editor-stress`, ignored by Git. `scripts/measure_idle.py` reproduces the lightweight idle sample. The Windows/Linux CI matrix tests core portability; Linux desktop integration is not implemented.

## Verify when back at the PC

- Gesture precision with the real tree, large/narrow fans, and rapid cross-branch movement.
- Cursor placement/restoration across the actual monitor arrangement, display scaling, and disconnected displays. Lock/suspend/display/capture tests deliver synthetic notifications to the isolated app; they do not physically lock, sleep, disconnect, or reconfigure this PC.
- Fullscreen suspension with the user's actual games/apps. Detection is a native window/style heuristic; the automated check covers a controlled borderless fullscreen window.
- Bell timbre, loudness, mute, and rapid-note feel. The current mixer allows up to 32 overlapping voices and drops new requests at saturation without stealing playing voices. Desktop tests mute audio and do not establish perceived audio quality.
- Program, URL, folder, shortcut, and script targets using real user configurations. Automated launches cover controlled programs, Python/batch/PowerShell scripts, and a generated Windows shortcut; URL/folder routing is checked without opening user handlers.
- Direct portable-folder write behavior under additional policies/filesystems. Checks cover relocation, foreign working directory, redirected alternate-storage sentinels, malformed data, and a real NTFS write denial. Sentinel checks detect writes to the tested environment paths; they are not a system-wide filesystem trace.

## Current limits

- Supported bindings are documented in the user guide. Modifier+mouse combinations and primary-button invocation are not included.
- Extremely dense full previews still incur native drawing cost (about half a second for 10,000 sectors in this sample); warnings do not change the saved geometry or remove targets.
- Themes, custom icons/images and compatible-program actions are researched in [the personalization study](PERSONALIZATION-STUDY.md), not implemented controls.
- The configuration window uses styled native controls. Updated scrolling, cached preview rendering, small-window reachability, mixed-DPI behavior, contrast and keyboard focus require manual review in this revision.
- Script runners must already be installed. Batch metacharacters are rejected to avoid unintended shell interpretation; a shortcut wrapper can handle advanced cases.
- No installer, startup registration, automatic updater, or Linux desktop adapter is included.
