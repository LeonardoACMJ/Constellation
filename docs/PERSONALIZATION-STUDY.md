# Personalization and application actions

2026-10-23 · Feasibility study; proposed direction, not shipped settings.

**Finding:** colors and local icons fit the current native design. Arbitrary image transparency needs a rendering change. Application actions are feasible for programs exposing a documented command line or API; support will be program-specific.

## Appearance

| Capability | Proposed approach | Cost / boundary |
|---|---|---|
| Themes | A small named palette: Star/Constellation fills, highlight, border, text, dots, lines, hub. Built-in presets plus editable colors. | Very small configuration data; no background processing. Use a shared color model so Linux can consume it later. |
| Item colors | Optional accent on a Star or Constellation, inheriting the theme when unset. | Keep hierarchy/order and hit regions unchanged. Preview changes in configuration. |
| Custom icons | Choose a local PNG/ICO; copy a normalized thumbnail into `data/assets/` and reference it with a relative path. | Load visible icons into a bounded cache; missing assets fall back to the existing Star/Constellation symbols. |
| Program icons | Extract an executable's icon in the editor and save a local thumbnail. Resolve `.lnk` icon metadata there too. | Discovery/extraction should not happen during a rapid gesture or on every redraw. Windows provides [ExtractIconExW](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-extracticonexw); WIC provides native [PNG, ICO and JPEG decoders](https://learn.microsoft.com/en-us/windows/win32/wic/-wic-about-windows-imaging-codec). |
| Item/background images | Optional static images, cropped in the editor, with a separate readable label layer. Start with item icons before large backgrounds. | More pixels and blending work. Decode and resize once; do not keep original full-resolution photos in resident memory. |

The current overlay uses black as a transparency key. Windows makes every pixel of that exact color transparent, so arbitrary black artwork would acquire holes. That mechanism also cannot represent a PNG's varying per-pixel alpha. This follows [SetLayeredWindowAttributes](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setlayeredwindowattributes).

**Proposed rendering step:** prepare a premultiplied BGRA surface and present it with [UpdateLayeredWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-updatelayeredwindow), or evaluate Direct2D if image composition justifies it. Recreate/reset the layering style when leaving the existing color-key mechanism; Windows documents that a prior `SetLayeredWindowAttributes` call otherwise prevents `UpdateLayeredWindow` from working. Keep drawing event-driven and retain the existing geometry/hit testing.

Memory arithmetic, **not measured implementation results**: one 64×64 RGBA icon is 16 KiB; 100 such decoded icons are about 1.56 MiB. One 1920×1080 four-byte surface is about 7.91 MiB, and one 3840×2160 surface is about 31.64 MiB. Additional buffers and decoders add overhead. A bounded icon cache and a buffer only for the active monitor should keep this compatible with the lightweight goal; measure before choosing defaults.

Proposed rollout: palette controls → per-item accents → custom/program icons → optional static backgrounds. Animated backgrounds, executable theme scripts, CSS engines and downloaded theme dependencies are outside this initial direction.

## Actions inside compatible programs — later

| Integration route | Example | What is already possible / still needed |
|---|---|---|
| Documented launch arguments | VS Code opens a file at a line with `--goto`, compares files with `--diff`, or opens a new window with `--new-window`. | Existing program/script Stars can express these commands. A later preset can provide named fields instead of handwritten arguments. See [VS Code CLI](https://code.visualstudio.com/docs/configure/command-line). |
| Local application API | OBS can expose requests for scenes, recording and other controls through obs-websocket. | A later adapter must connect, authenticate when configured, check supported requests, execute one operation with a timeout, and report its result. No integration was enabled or connected during this study. See the [official protocol](https://github.com/obsproject/obs-websocket/blob/master/docs/generated/protocol.md). |
| Desktop action metadata | Linux `.desktop` files can declare named additional actions. | Candidate source of explicit actions during the Linux port; parse only declared/supported action groups. See [Desktop Entry actions](https://specifications.freedesktop.org/desktop-entry/latest/extra-actions.html). |
| Simulated keystrokes / UI automation | Sending a menu shortcut to an already-running app. | Fragile because focus, localization and app state matter. Defer until a specific requested app lacks a supported API. Do not present this as universal action support. |

**Proposed action model:** a Star refers to a named adapter, operation and structured parameters. Start with command presets, then one concrete API adapter. Keep any connection/helper short-lived or idle-blocking; do not poll every supported program in the tray process. Reuse existing exactly-once dispatch and error reporting.

Authenticated adapters need a deliberate credential/export design before implementation: portable storage must remain local to the application folder, while sharing a theme or configuration must not accidentally share secrets. This study does not choose a secret-storage policy or expand URL support beyond the current HTTP/HTTPS targets.

## Review before implementation

Choose preferred palettes, whether accents apply per item or per Constellation subtree, and which program's action should be the first real adapter. These are future customization choices; they do not block the current launcher/editor work.

Custom cursor follow-up: a small native dot cursor was tried during the feedback pass, but desktop checks did not establish consistent behavior over transparent areas. It is not shipped. Keep the normal pointer until an optional replacement works consistently without changing global cursor settings or impairing selection/focus.
