# Contributing to Wallweave

Small, focused changes and reproducible reports are welcome. For larger features, open an issue first so the expected behavior and renderer boundaries are clear.

## Development

Install stable Rust and the Linux development packages listed in the [README](README.md#build-from-source), then run:

```sh
cargo fmt --check
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo check --locked --no-default-features
```

Use an isolated absolute directory for development state:

```sh
WALLWEAVE_HOME="$(mktemp -d)" cargo run --locked -- doctor
```

Tests use fake renderers and private supervisor sockets. Desktop playback testing is separate; record the compositor, renderer version, output type and what you actually observed. Do not infer rendering compatibility from a process successfully starting.

Do not commit personal configuration, Steam libraries, downloaded Workshop content, renderer runtimes or compiled binaries. The original `examples/web-pulse` fixture is available for basic project/property checks.

## Bug reports

Include the Wallweave version, distribution, desktop/compositor, renderer version, steps to reproduce and expected behavior. For an affected wallpaper, include its Workshop ID or link and whether window preview also fails. Diagnostic output from `wallweave doctor --json` and Activity logs helps; remove private paths before posting.

## Pull requests

Explain the concrete problem and resulting behavior, and include relevant validation. Add regression coverage for process management, library discovery or state changes. Visual changes should include an app screenshot at a supported size. Keep dependencies and scope limited to the change.
