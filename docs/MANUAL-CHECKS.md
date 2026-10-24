# Constellation 0.1.4 — manual checks

Desktop automation is paused while the user uses the PC. These checks exercise the current changes without an agent controlling input.

If Quit fails in an older build, save and close Configure, then end the resident `constellation.exe` in Task Manager before updating.

When updating, close Configure and choose **Quit** in the current tray app. Copy the files from the ZIP's `Constellation` directory into your existing app directory, keeping `data/`. Do not start a separate empty staging folder if you want to keep your existing settings. The configuration title should show **0.1.4**.

Start `constellation.exe`; right-click its tray icon → **Configure / Visualize…**. Settings persist with **Save**. Profile changes need **Done**, then **Save** in the main editor.

| Check | Steps | Expected |
|---|---|---|
| Tray Quit | Close Configure, right-click the tray icon → Quit. Check Task Manager, then start the same executable again and try Ctrl+Q. Repeat Quit after using the radial. | Resident exits, tray icon disappears, shortcut stops intercepting input; restarting succeeds without an already-running warning. |
| Editor scrolling | Make the editor short enough to scroll. Wheel up/down slowly, then quickly; drag the vertical/horizontal scrollbars. | No whole-form flash, overlapping/duplicated fields or preview-header jumps; the painted panel outlines move with their contents. Resize small → maximize → restore and repeat. |
| Maximize/restore responsiveness | Repeatedly maximize → restore, including after scrolling. Then click a field, type, scroll and close Configure. | The restored window responds immediately; no persistent stall. |
| Maximized layout | Maximize the editor after scrolling a small window. Restore it, then maximize again. | Unneeded scrollbars disappear, scroll offsets reset to zero, and the tree, item column and preview use the available width/height. |
| Editor appearance | Inspect the main editor, Shortcut profiles and Advanced JSON. Tab between controls and open dropdowns. | Related navy/teal/lavender styling, readable text, visible focus, usable fields/buttons and no clipped bottom actions. Some system-owned dialogs/chrome retain Windows styling. |
| Slider drawing | Drag each spacing slider rapidly; use arrows and Tab too. | No white flashing; dark background, readable thumb and visible keyboard focus. |
| Preview updates | Change each spacing slider, then type a value. Switch the monitor preview, resize the window and scroll. | Layout/warnings update when values change; scrolling alone reuses the same preview. Save remains available with advisory overflow warnings. |
| Chime overlap | Enable Chimes, set a comfortable low volume, Save, then sweep across several Stars quickly. Stop moving; test mute afterward. | Single notes and overlapping tails sound clean, without the reported crunch/crackle, clipping or obvious volume pumping. A new note does not cut off the preceding note; staying on one item does not repeatedly chime. |
| Cursor end | In Shortcut profiles, try each **Cursor on close** choice: Restore original, Return to menu center, Leave at selection. Test launch and Escape; optionally repeat with Hold. | Original position, invocation monitor's usable center, or selection position respectively. Cursor does not recenter during branch navigation. |
| Reopening | Hover Everyday → Calculator, launch it, then Ctrl+Q again. Repeat after closing an expanded branch with Escape. | The newly opened menu starts at the root with no flash of the previous expanded branch. |
| Outside targeting | Enable **Select by direction beyond the outer edge** for a profile. Move beyond a Star's outer edge in its direction; click or press Space. Repeat in a nested fan and, if configured, with Hold release. | The matching visible item stays highlighted and launches once. Painted sectors take precedence; gaps and the root hub stay neutral. |
| Exact targeting option | Disable that option, Save, then repeat outside the circle. | Outside positions stop targeting Stars; the drawn outer edge is the selection boundary. |
| Closing configuration | Close the configuration panel. | The editor process exits and releases its fonts, drawing buffers and preview cache. The separate tray process remains available. |

To create a branch: select **Your universe** → **+ Constellation**, name it → **Apply item**. With it selected, add a **+ Star**, choose its target kind and Browse to a program; Apply. Select the Constellation again to add a nested Constellation. **Parent constellation** moves existing items. **Save** before testing Ctrl+Q.

Custom cursors are deferred; this build deliberately uses the normal Windows cursor. Subjective audio quality and actual display flicker still require these manual checks.
