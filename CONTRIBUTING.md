# Contributing to Constellation

Bug reports, fixes and improvements are welcome. For a larger feature, open an issue first so we can agree on the behaviour and scope.

## Build from source

Use Rust's stable toolchain. Windows builds need the MSVC target and an activated MSVC/Windows SDK environment. A Python installation is needed for the packaging tools; the app itself has no Python runtime dependency.

From the repository root:

```cmd
cargo build --release --locked
python scripts/package.py
```

The executable is `target/release/constellation.exe`. Packaging creates a portable folder under `portable/Constellation` and a ZIP with its SHA-256 checksum under `dist`. The portable core also builds and tests on Linux; Linux desktop integration is not implemented.

If a portable copy is running, package into another directory:

```cmd
python scripts/package.py --staging-dir portable/Constellation-update
```

Close the app before replacing its executable, and keep the existing `data` folder. The packager includes only the executable, user guide, manifesto, license and dependency notices. Do not include personal configurations in a release.

## Checks

Format changes and run the checks that cover them:

```cmd
cargo fmt --all --check
cargo clippy --locked --all-targets -- -A warnings -D clippy::correctness
python scripts/check_release.py
cargo test --test core --test editing --test editor_layout --test feedback --test notes --test icon_assets --test background --test browser --test bundle --test audit_tools --locked
```

These integration targets cover the model, editing, layout, feedback and assets without driving the desktop. Windows still needs the activated build environment to compile them.

The complete suite (`cargo test --locked`) also contains native Windows checks that launch controlled child processes and create console resources. Run those in CI or a desktop session dedicated to testing. The interactive `tests/native_smoke.py` and `tests/large_editor_smoke.py` scripts move the pointer, interact with windows or change foreground state; use them only when that desktop is available for testing, with `--interactive-session`. They refuse an ordinary running resident/editor and address only fixture-owned windows. The idle measurement helper has the same gate. Do not play audio or take over someone else's working session as an incidental check.

CI runs on Windows and Linux, checks formatting, exercises native Windows regressions, and builds and validates the portable package. Passing automation does not establish subjective audio quality, gesture comfort or compatibility with every monitor/game setup. The [manual checklist](docs/MANUAL-CHECKS.md) covers hands-on review.

## Find your way around

| Location | Contents |
|---|---|
| `src/` | Shared configuration, geometry, navigation, storage and sound synthesis |
| `src/windows/` | Windows tray, input, rendering, editor, audio and launching |
| `tests/` | Integration checks and explicitly interactive desktop probes |
| `scripts/` | Icon, packaging and measurement tools |
| `examples/` | Core stress measurements and offline audio generation |
| [Behavior specification](docs/SPEC.md) | Expected interaction and acceptance criteria |
| [Audio implementation](docs/AUDIO-SYNTHESIS.md) | Synthesis choices and implementation references |

Keep useful implementation documentation and reproducible checks in the repository. Put exploratory notes, local evidence and personal working material under `.local/`; build output and runtime data are ignored by Git.

Describe what your change fixes and how you checked it. Avoid unrelated changes, preserve existing configurations, and keep input/rendering paths responsive.

After packaging, run `python scripts/check_release.py --archive dist/Constellation-VERSION-windows-x64.zip`. It checks the checksum, exact five-file contents, qualified executable and packaged documents. After publishing, pass `--published-json PATH` with the saved GitHub release API response to compare the public asset digest/size/tag against that same qualified archive. Missing digests fail verification. Existing published archives must not be rebuilt in place.

CI cancels superseded runs on the same ref. Branches run the full matrix. A tag can reuse a successful main run for the identical commit only when both Windows/Linux jobs passed and its portable artifact is still available. It waits up to four minutes for an in-progress main run; otherwise it runs the full matrix. Reuse verifies the source run again, rechecks the downloaded package and retains the same artifact without rebuilding it. Pull-request results and different commits cannot qualify a tag. Run `python -m unittest discover -s tests -p "test_ci_qualification.py"` for device-free qualification regressions.

For an explicit device-free large-tree timing probe, run `cargo test --release --test audit_tools measure_large_tree_preview -- --ignored --nocapture`; this does not open windows, synthesize input or play sound.
