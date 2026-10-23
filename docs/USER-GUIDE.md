# Constellation — portable preview

Extract this whole folder to a writable location, then run `constellation.exe`.
The app stays in the notification area (system tray); it does not show a permanent main window. Only one resident instance can run in a Windows session.

## Open and launch

- Press **Ctrl+Q**. The cursor and radial move to the usable center of the monitor containing the cursor.
- Hover a Constellation to reveal its children immediately. Click a Star or press Space over it to launch.
- Escape, right click, or click outside the menu to cancel. Click the center to return to root; Backspace closes the deepest branch.
- The cursor returns to its saved position when the radial closes, unless restoration is disabled for that profile.
- Another shortcut profile is ignored while one is active. Pressing the active Click binding again closes the radial.
- Shortcuts are suspended while a fullscreen app is foreground. The app does not intentionally overlay fullscreen windows or move their cursor.

The sample tree includes Calculator, Notepad, the portable folder, and a GitHub URL. Customize it before relying on fixed gestures.

## Configure

Right-click the tray icon and choose **Configure / Visualize…**, or double-click it. You can also run:

```cmd
constellation.exe --configure
```

- Select an item in the left list. Edit its name, kind, target, arguments, script runner, working directory, or parent, then **Apply item**.
- Add Stars/Constellations, move them using the Parent field, reorder with Up/Down, or delete them. Converting a nonempty Constellation to a Star requires moving/removing its children first.
- Tune Radius, Distance (a multiplier of radius), Spread (degrees), and Gap. The full fan-out preview uses the selected monitor's work-area outline. The overview zoom is labeled.
- Overflow and narrow-target warnings are advisory. They do not block saving or force a fit. Fans keep subdividing; no paging is added.
- Set Chimes and Volume (0–1). The current lightweight bell uses random pentatonic notes and one voice, replacing the previous tail during fast navigation.
- **Save** writes configuration. New settings apply to the next invocation; an already-open gesture is not rearranged.

**Profiles / Advanced JSON…** edits the complete configuration, including shortcut profiles. Example profile:

```json
{
  "name": "Mouse hold",
  "binding": "Mouse4",
  "mode": "hold",
  "hover": true,
  "restore_cursor": true
}
```

Bindings support A–Z, 0–9, Space, F1–F24 except Windows-reserved F12, and Ctrl/Shift/Alt combinations. Mouse3 is middle; Mouse4/5 are side buttons. Left/right click are reserved for menu interaction. Modifier+mouse combinations are not supported in this preview. A mouse's software can map a button to a keyboard combination instead.

Click mode stays open after release. Hold mode selects the hovered Star when the bound key/button or any required modifier is released; releasing over empty space/Constellations cancels. `hover: false` changes Constellations to left-click opening. `restore_cursor: false` leaves the cursor where selection ended.

## Target types

- **Program:** executable path/name, JSON arguments array, optional working directory.
- **Windows shortcut:** `.lnk` file.
- **URL:** an HTTP/HTTPS URL opened through its Windows handler.
- **Filesystem folder:** folder opened through its Windows handler.
- **Script:** script file, optional runner, JSON arguments array, optional working directory. Defaults: `.ps1` → `powershell.exe -NoProfile -File`; `.py` → `python.exe`; `.bat`/`.cmd` → `cmd.exe /d /s /c`. Runners must already exist. PowerShell execution policy is respected; the app does not change it. Batch paths/arguments with shell metacharacters are rejected; use a shortcut wrapper or another runner.

Relative file paths resolve from this folder, not the directory from which the app was invoked. A plain program/runner name resolves through PATH. External installed applications remain external; copying Constellation does not copy those programs.

## Files and exit

`data/config.json` and `data/config.backup.json` live beside the executable. Errors, when present, are recorded in the same `data` directory. No installer, AppData fallback, startup registration, or global PATH modification is used.

Stop the resident app using **Quit** in its tray menu. Closing the configuration window leaves the tray app running. Stop before moving the folder or replacing the executable; preserve `data` when updating.

## Preview limits

This is the first native build. The configuration UI is functional and deliberately plain; profile editing currently uses JSON. Linux desktop integration is not implemented. Automated desktop probes do not replace checking your own displays, input devices, fullscreen applications, visual preferences, and chime volume.
