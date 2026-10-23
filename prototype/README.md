# Constellation interaction prototype

Local, buildless browser experiment. No dependencies, fonts, analytics, or network assets. This is a behavior prototype, not a native launcher or a specification.

From this directory, serve with an existing Python installation:

```cmd
python -m http.server 8765 --bind 127.0.0.1
```

Open http://127.0.0.1:8765. Stop the server with Ctrl+C.

Default temporary shortcuts: **Ctrl+Shift+F** toggles; **Ctrl+Shift+G** opens while held and launches the hovered star when any required key is released. These are experiment bindings, not a claim of system-wide availability. The page must have focus. Click a binding and type a replacement, or press a mouse button over the playground. Browser/OS-reserved combinations may be intercepted before the page receives them. Mouse buttons 4/5 may also be reserved by browser navigation.

- Each shortcut has its own activation and folder-opening behavior.
- All shortcuts and the Open radial button open at the canvas center, regardless of pointer position. Browsers cannot reposition the system cursor, so this prototype does not center or restore it. The intended native interaction saves the cursor position, centers it on the invoking monitor while the radial is open, and restores it on launch or dismissal.
- Stars launch with left click or Space while hovered. Launches are simulated.
- Hover delay is adjustable down to zero. Moving over ancestor sectors changes branches; gaps preserve the open path. Backspace moves up, the center hub returns to root, Escape/right click/outside-sector click dismisses. Focus loss cancels.
- Child fans translate outward by 52% of the base radius and overlap the parent. Their arc spans about 148 degrees. The whole stack translates, then scales only if needed, to fit the playground. This is an initial geometry to evaluate, not a settled design. Long labels are truncated with full labels on hover.
- Root sectors subdivide up to 12 visible targets. Child fans subdivide up to 9. Above that, Previous/More sectors page through all items instead of producing tiny targets.
- Add stars and nested constellations in the left panel; the density button adds 12 root stars. The demo tree and shortcut settings persist in this browser's local storage. Reset restores the sample data.

Checks: `node --check app.js`, `node --check geometry.mjs`, and `node check.mjs`. The last checks geometry and app events using a minimal DOM; it does **not** verify rendered appearance or actual browser key delivery.

Questions for hands-on evaluation: Does the overlapping stack feel natural? Can you reliably retrace a path? Do edge adjustments disrupt muscle memory? Should dense menus grow, page, or form additional constellations? The browser's resource use says nothing about the eventual native launcher's memory footprint.
