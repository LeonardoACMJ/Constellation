# Constellation

A portable system-tray radial launcher. Stars launch programs, shortcuts, URLs, folders, and scripts; nested Constellations organize them.

The Windows native application is being implemented. The [product and behavior spec](docs/SPEC.md) defines its intended behavior and acceptance checks. Linux portability is a design constraint; a Linux desktop adapter is not implemented yet.

The [browser interaction prototype](prototype/README.md) is an experiment, not the native app. Its paging, edge fitting, and delay controls are superseded by the current spec.

## Development environment

Rust is installed on the development PC. Activate the existing portable MSVC/Windows SDK environment before Windows builds:

```cmd
call E:\_DevTools\env.cmd
cargo test
cargo build --release
```

Runtime configuration and other app-managed files belong beside the distributed executable. Build output and local runtime data are ignored by Git.
