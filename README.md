# Constellation

A portable radial launcher for Windows. Press **Ctrl+Q**, move through your menus, and open an app, folder, website or script.

**[Download the Windows beta](https://github.com/LeonardoACMJ/Constellation/releases/tag/v0.1.33)** · [User guide](docs/USER-GUIDE.md) · [Release notes](docs/releases/0.1.33.md)

## Get started

1. Download the ZIP and extract it somewhere you want to keep the app.
2. Run `constellation.exe`. Look for its star icon in the system tray.
3. Press **Ctrl+Q** to open the menu in the centre of your screen. Hover over a Constellation to explore it; click a Star to launch it. **Escape** closes the menu.
4. Open **Settings** in the sample menu, or right-click the tray icon and choose **Configure / Visualize…**, to make it yours.

Constellation currently supports **Windows x64**. This is a beta; see the [release notes](docs/releases/0.1.33.md) for known issues. The executable is unsigned, so Windows may show a warning when you download or run it.

## Build your own constellation

**Stars** open things. **Constellations** group them into menus, and can contain more Constellations.

- Add programs, Windows shortcuts, folders, websites and scripts.
- Choose your own icons, including icons taken from a different program.
- Give each shortcut its own menu and behaviour: press once to keep the menu open, or hold and release to choose an item.
- Keep several independent menus as **universes**, or let a shortcut open directly inside an existing Constellation.
- Adjust spacing with a preview, choose a sound or silence, and use a small white dot or your usual cursor.

The launcher stays in the tray when you close Settings. It leaves fullscreen applications alone. **Quit** in the tray menu closes it completely.

## A folder you control

There is no installer. Settings, icons and logs stay in the app's `data` folder. Close Constellation and copy the whole folder to back up your setup or move it to another drive. Programs you launch remain separate from Constellation.

You can optionally add a Start menu shortcut or enable automatic startup in Settings. Those buttons apply immediately, without **Save**; each has a matching remove/disable action. They create shortcuts in your Windows user folders. Exporting a configuration also writes to the location you choose.

To update, save and close Settings, quit the tray app, then replace the application files with the new release. **Keep your `data` folder.** If you move the app, remove and recreate any optional Windows shortcuts so they point to its new location.

We believe portability gives people more control over their software. Read [our small manifesto for portability — português / English](docs/PORTABLE-MANIFESTO.md).

## Help and contributions

The [user guide](docs/USER-GUIDE.md) covers configuration and shortcuts in detail. If something goes wrong, [report an issue](https://github.com/LeonardoACMJ/Constellation/issues) with your app version and the steps that led to it. Please leave personal paths and private configuration out of your report.

Want to work on Constellation? See [Contributing](CONTRIBUTING.md) for building, testing and the project structure.

## License

Constellation is free software under [GPL-3.0-or-later](LICENSE).
