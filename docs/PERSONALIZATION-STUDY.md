# Personalization and application actions

Reviewed 2026-10-25 against 0.1.30 · Implemented icons and future personalization proposals.

**Current status (0.1.30):** Automatic, Symbol, Custom image and From Software item-icon modes are implemented. The antialiased renderer supports transparent images. Colors/theme controls remain future work.

**Earlier finding:** colors and local icons fit the native design; arbitrary image transparency required a rendering change. Application actions are feasible for programs exposing a documented command line or API; support will be program-specific.

## Appearance

| Capability | Proposed approach | Cost / boundary |
|---|---|---|
| Themes | A small named palette: Star/Constellation fills, highlight, border, text, dots, lines, hub. Built-in presets plus editable colors. | Very small configuration data; no background processing. Use a shared color model so Linux can consume it later. |
| Item colors | Optional accent on a Star or Constellation, inheriting the theme when unset. | Keep hierarchy/order and hit regions unchanged. Preview changes in configuration. |
| Custom icons | Implemented: PNG/JPEG/BMP/ICO imports copied into `data/icons`, referenced relatively. | Normalize to 64 × 64 in a bounded asynchronous cache; unavailable assets use the existing symbols. |
| Program icons | Implemented: Windows Shell extraction in a background worker, including `.lnk` and file-type associations. | Cache results; no extraction in input/paint handlers. Windows provides [ExtractIconExW](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-extracticonexw); WIC provides native [PNG, ICO and JPEG decoders](https://learn.microsoft.com/en-us/windows/win32/wic/-wic-about-windows-imaging-codec). |
| Item/background images | Optional static images, cropped in the editor, with a separate readable label layer. Start with item icons before large backgrounds. | More pixels and blending work. Decode and resize once; do not keep original full-resolution photos in resident memory. |

**Rendering already implemented:** the radial uses antialiased GDI+ paths on a premultiplied BGRA surface with per-pixel alpha. The previous black color-key limitation is historical; transparent custom icons do not require a new renderer. Icon extraction/decoding runs on a bounded background cache. See [verification](VERIFICATION.md) for implementation evidence and its limits.

Memory arithmetic, **not measured implementation results**: one 64×64 RGBA icon is 16 KiB; 100 such decoded icons are about 1.56 MiB. One 1920×1080 four-byte surface is about 7.91 MiB, and one 3840×2160 surface is about 31.64 MiB. Additional buffers and decoders add overhead. A bounded icon cache and a buffer only for the active monitor should keep this compatible with the lightweight goal; measure before choosing defaults.

Remaining proposed rollout: palette controls → per-item accents → optional static backgrounds. Custom/program icons are already available. Animated backgrounds, executable theme scripts, CSS engines and downloaded theme dependencies are outside this initial direction.

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

The optional white-dot cursor is implemented per shortcut profile, with the system cursor available as an alternative. It does not change Windows cursor-theme settings. Actual perceived behavior remains a manual check.
