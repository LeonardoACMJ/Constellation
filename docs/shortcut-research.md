# Default shortcut — first research pass

Checked 2026-10-23. Scope: English-language default Windows keyboard documentation, Firefox, Edge, VS Code, Word, plus Blender for Ctrl+Q. This is a small documented-collision survey, not a measurement of how frequently all users press each shortcut. Custom bindings, extensions, keyboard layouts, and other utilities can add conflicts.

Reach envelope from the requested QWERTY boundary: Q W E R T / A S D F G H / Z X C V B N. Physical comfort still needs hands-on testing.

## 1. Windows first

These are contextual Windows/text-editing/File Explorer uses, not a claim that every combination is globally reserved. Source: [Microsoft's Windows keyboard reference](https://support.microsoft.com/en-gb/accessibility/windows/keyboard-shortcuts-in-windows).

| Ctrl + key | Documented Windows context / use |
|---|---|
| Q | No plain Ctrl+Q entry found in this reference |
| W | File Explorer: close tab/window |
| E | File Explorer: search |
| R | Refresh current window |
| T | Windows 11 File Explorer: new tab |
| A | Select all |
| S | No plain Ctrl+S entry found in this reference; see app use below |
| D | Delete selected item to Recycle Bin |
| F | Find / File Explorer search |
| G | No plain Ctrl+G entry found in this reference |
| H | Text editing: Replace |
| Z | Undo |
| X | Cut |
| C | Copy |
| V | Paste |
| B | Text editing: Bold |
| N | File Explorer: new window |

Absence from this reference means only “not listed,” not “available.” Windows+Ctrl+Q is Quick Assist; it is a different combination from Ctrl+Q.

## 2. Common apps next

Windows leaves Q, G, and S worth inspecting first in this sample. B and H are included because they may look attractive for a launcher despite existing text-editing conflicts.

| Binding | Browser example | VS Code | Word |
|---|---|---|---|
| Ctrl+Q | No plain Ctrl+Q entry found in the reviewed Edge list | Quick Open View | Remove paragraph formatting |
| Ctrl+G | Edge: next Find result | Go to line | Go To dialog |
| Ctrl+S | Firefox: save page | Save | Save document |
| Ctrl+B | Firefox: bookmarks sidebar | Toggle sidebar | Bold |
| Ctrl+H | Edge: history | Replace | Replace |

Sources: [Firefox](https://support.mozilla.org/en-US/kb/keyboard-shortcuts-perform-firefox-tasks-quickly), [Edge](https://support.microsoft.com/en-us/edge/keyboard-shortcuts-in-microsoft-edge), [VS Code](https://code.visualstudio.com/docs/reference/default-keybindings), [Word](https://support.microsoft.com/en-us/accessibility/word/keyboard-shortcuts-in-word?wt.mc_id=otc_word).

Ctrl+Q also means Quit in [Blender's default keymap](https://docs.blender.org/manual/id/dev/interface/keymap/blender_default.html). It therefore deserves extra caution as a default candidate even if it looks relatively sparse in the Windows/browser documentation.

The temporary prototype bindings are occupied too: Ctrl+Shift+F opens VS Code Search; Ctrl+Shift+G opens Source Control. Adding Shift does not automatically solve the problem. See the [VS Code reference](https://code.visualstudio.com/docs/reference/default-keybindings).

## 3. Conclusion for the current decision

Every plain Ctrl+letter in the stated reach envelope has at least one documented use across this sample. There is no basis here to label a candidate conflict-free or statistically “least used.” The chosen default and required input-consumption behavior are defined in [the spec](SPEC.md) under CFG-01 and BIND-03; this survey does not override those decisions.

The user considers the surveyed Ctrl+Q actions expendable in their workflow. Other users may choose a different configurable binding based on application use and ergonomic comfort.

Native registration failure must be reported, but registration success does not prove that a foreground app has no contextual use. [RegisterHotKey](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-registerhotkey) handles system-wide registration; this survey addresses an additional usability concern.
