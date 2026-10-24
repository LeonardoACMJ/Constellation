# Constellation — portable preview

Extract this whole folder to a writable location, then run `constellation.exe`.
The app stays in the notification area (system tray); it does not show a permanent main window. Only one resident instance can run in a Windows session.

The app/tray icon is a mint four-point star. Individual Stars use one star; Constellations use three smaller lavender stars, including the root. The configuration list uses one hollow star for Constellations.

## Open and launch

- Press **Ctrl+Q**. The cursor and radial move to the usable center of the monitor containing the cursor.
- Hover a Constellation to reveal its children immediately. Click a Star or press Space over it to launch.
- Escape, right click, or click an untargeted gap to cancel. Click the center to return to root; Backspace closes the deepest branch. With directional targeting enabled, moving beyond the circle still targets the item in that direction; clicking there selects it.
- Each shortcut profile chooses where the cursor ends: its original position, the menu center, or the selection position.
- Another shortcut profile is ignored while one is active. Pressing the active Click binding again closes the radial.
- Shortcuts are suspended while a fullscreen app is foreground. The app does not intentionally overlay fullscreen windows or move their cursor.
- Locking/disconnecting the session or suspending the PC cancels the current gesture. Availability resumes after unlock/reconnect/resume. Late releases cannot launch from the cancelled gesture.
- The radial opens without activating its window: the current app keeps focus while the launcher consumes navigation/selection inputs. Switching foreground applications cancels the radial. The tray icon is restored after Explorer recreates the taskbar.

The sample tree includes Calculator, Notepad, the portable folder, and a GitHub URL. Customize it before relying on fixed gestures.

## Configure

Every item, including root and nested Constellations, has a **Note** field below Name. Use it for short prerequisites or consequences (for example, “needs ssh-agent running” or “deletes the source”). The live counter caps typing/paste at 140 Unicode characters; pasted line breaks become spaces. **Apply item → Save** persists it. Notes participate in undo/redo and configuration import/export; clearing a note removes its JSON field.

When any note is configured, a fixed central panel shows the hovered item's name and note immediately; over the neutral centre/gaps it shows the root. Long notes wrap inside that panel. Empty-note items keep the same panel footprint. It can cover part of the radial visually but does not change targeting, add delays or affect launching. Notes do not replace missing-target advice in the editor. The full fan-out preview retains its geometry overview.

Choose **Icon: From Software → Browse…** to use an EXE/shortcut icon independently of the launch target. For example, keep a Hydrus instance script as the target and select the Hydrus executable as its icon source. **Apply item → Save** persists both independently. Software is referenced in place, not copied; reselect its source if you move that installation.

Each item has an **Icon** selector: **Automatic** uses its Windows target/file-association icon; **Star symbol** keeps the existing symbol. Click **Image…** to import a PNG, JPEG, BMP or ICO (up to 8 MiB; decoded images up to 4096 × 4096). This selects **Custom image** and copies the file into `data/icons`; **Apply item → Save** persists the choice. Constellations support custom images too. Automatic Constellations retain their three-star radial symbol; the list keeps its hollow star.

Icons appear asynchronously in the radial and full preview; unresolved or unreadable icons retain the symbol. Scripts use the associated file-type icon. URLs use Windows' URL-file association; website favicons are not downloaded. Move the whole app folder, including `data/icons`, to preserve custom images. Configuration JSON export references images but does not bundle them; copy `data/icons` alongside an imported configuration if needed. Imported files are retained when drafts are cancelled or items deleted, so undo and backups do not lose their images.

Right-click the tray icon and choose **Configure / Visualize…**, or double-click it. You can also run:

```cmd
constellation.exe --configure
```

- The native panel shares the radial’s navy/teal/lavender palette. Its fonts, preview cache and drawing buffers exist only in the configuration process. The panel has scrollbars for smaller windows; the mouse wheel scrolls vertically. Its controls can be reached with Tab.
- Select an item in the left list. Edit its name, kind, target, arguments, script runner, working directory, or parent. **Apply item** keeps the changes in the draft; switching items also applies valid edits automatically. Invalid fields stay visible and prevent switching until corrected or undone.
- Use **Browse…** for target files, script runners, and filesystem folders. Drop one or several files/folders into the panel to add Stars under the selected Constellation (or beside the selected Star). Types and names are inferred. Paths within the portable folder are stored relatively.
- Arguments use normal double-quoted values, for example `--profile "Work profile" ""`. The last value is an empty argument. Backslashes before quotes follow Windows-style escaping. These become separate argument strings; no shell expansion or environment substitution is performed by the editor.
- Missing-target advice does not prevent saving a path that will exist later.
- Add Stars/Constellations, move them using the Parent field, reorder with Up/Down, or delete them. Converting a nonempty Constellation to a Star requires moving/removing its children first.
- Tune Radius, Distance (a multiplier of radius), Spread (degrees), and Gap using sliders or direct numeric entry. Slider ranges are conveniences; numeric values can exceed them. The full fan-out preview uses the selected monitor's work-area outline. The overview zoom is labeled.
- Overflow, narrow-target, and full-preview overlap warnings are advisory. Intentional parent/descendant stacking is excluded from overlap warnings; mutually exclusive sibling branches can overlap in the full preview. They do not block saving or force a fit. Fans keep subdividing; no paging is added.
- Set Chimes and Volume (0–1). Random pentatonic notes overlap and finish their tails. The mixer keeps up to 32 voices; saturation drops new notes rather than cutting off playing notes. Audio waits without polling when silent.
- **Undo / Redo** restore draft changes, including deleted branches, item definitions and imported trees. History is kept only in this editor session and bounded to 64 snapshots and approximately 16 MiB of serialized data per stack (one large snapshot can exceed that budget). Undo after Save still requires Save to update the launcher. Undo discards invalid field text so it cannot trap you in a broken form.
- **Import…** validates a JSON configuration before replacing the draft. Undo returns to the previous draft. **Export…** writes a JSON snapshot to the file you choose; it does not copy target programs or other assets. Relative paths in imports still resolve from this executable folder, not the import file's folder. Exports are explicitly chosen document outputs; background application storage stays in the portable folder.
- **Save** writes configuration. New settings apply to the next invocation; an already-open gesture is not rearranged.

**Shortcut profiles…** opens the graphical profile editor:

- Add or remove profiles. Each profile has a name, shortcut, Click/Hold activation, hover-to-open setting, cursor-end choice and optional targeting beyond the outer edge.
- Type a binding or choose **Record shortcut** and press it. For a middle/side mouse button, click inside the recorder window. Escape cancels recording. The resident launcher suspends shortcuts while this window has focus.
- **Apply profile** keeps the current fields in the dialog draft. Switching profiles also applies valid fields; invalid or duplicate bindings stay visible with an inline message.
- **Done** transfers the draft to the main editor; then **Save** writes it and updates the resident launcher. **Cancel** or closing the profile window discards that dialog's changes.

**Advanced JSON…** remains available for editing the complete configuration directly.

Bindings support A–Z, 0–9, Space, F1–F24 except Windows-reserved F12, and Ctrl/Shift/Alt combinations. Mouse3 is middle; Mouse4/5 are side buttons. Left/right click are reserved for menu interaction. Modifier+mouse combinations are not supported in this preview. A mouse's software can map a button to a keyboard combination instead.

Click mode stays open after release. Hold mode selects the hovered Star when the bound key/button or any required modifier is released; releasing over empty space/Constellations cancels. `hover: false` changes Constellations to left-click opening. `cursor_end` accepts `restore`, `center`, or `leave`. Old `restore_cursor` settings still load: true maps to restore, false to leave. Saving writes the new field. `directional` enables targeting beyond the outer edge and is initially true. `dot_cursor` enables a small white dot with a dark outline while the radial is open over its monitor work area; it defaults to true, including for older profiles without the field. Uncheck **Use white-dot cursor while radial is open** to keep the system cursor. The selection point is the dot center. Closing the radial restores the previous cursor image without changing the Windows cursor theme.

## Create a test branch

1. Select **Your universe** and click **+ Constellation**. Name it, then **Apply item**.
2. With that Constellation selected, click **+ Star**, name it, choose **Program**, and **Browse…** to an executable. Apply it.
3. Select the Constellation again and add another Constellation to test nesting. Add a Star inside that one. The **Parent constellation** field moves existing items between branches.
4. Click **Save**, close the editor, and invoke Ctrl+Q. Hover through your new branch.

## Target types

- **Program:** executable path/name, arguments, optional working directory.
- **Windows shortcut / file:** `.lnk` or another file opened by its Windows handler.
- **URL:** an HTTP/HTTPS URL opened through its Windows handler.
- **Filesystem folder:** folder opened through its Windows handler.
- **Script:** script file, optional runner, arguments, optional working directory. Defaults: `.ps1` → `powershell.exe -NoProfile -File`; `.py` → `python.exe`; `.bat`/`.cmd` → `cmd.exe /d /s /c`. Runners must already exist. PowerShell execution policy is respected; the app does not change it. Batch paths/arguments with shell metacharacters are rejected; use a shortcut wrapper or another runner.

Relative file paths resolve from this folder, not the directory from which the app was invoked. A plain program/runner name resolves through PATH. External installed applications remain external; copying Constellation does not copy those programs.

## Files and exit

**Note compatibility (0.1.19):** the configuration format version stays unchanged and old files load normally. Older app builds reject configurations containing a `note` field because Node rejects unknown fields. A backup only helps if it predates notes; later saves can copy notes into the backup too. Before downgrading, keep a compatible configuration copy or remove `note` from every node, including Constellations and root. Notes live only in config.json; no scripts-folder README/index is generated.

`data/config.json` and `data/config.backup.json` live beside the executable. Profiles carry stable numeric IDs; older configurations without IDs are upgraded when loaded and persist those IDs on Save. Saving a repaired configuration preserves an existing valid backup instead of backing up malformed JSON. Errors, when present, are recorded in the same `data` directory. No installer, AppData fallback, startup registration, or global PATH modification is used.

Stop the resident app using **Quit** in its tray menu. Closing the configuration window leaves the tray app running. Stop before moving the folder or replacing the executable; preserve `data` when updating.

## Preview limits

This is the first native build. The configuration UI is functional and deliberately plain. Linux desktop integration is not implemented. Automated desktop probes do not replace checking your own displays, input devices, fullscreen applications, visual preferences, and chime volume.

Customization feasibility and the proposed later app-action adapters are described in [the personalization study](PERSONALIZATION-STUDY.md). Themes and action adapters remain future work; item icons are available in Configure.

Use [the feedback-build manual checklist](MANUAL-CHECKS.md) to review scrolling, styling, overlapping chimes, cursor-end options, fresh opening and outside targeting. Local desktop automation is on hold while the user uses the PC.
