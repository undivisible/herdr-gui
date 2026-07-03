# Herdr GUI

Minimal macOS GPUI client for Herdr.

Herdr owns sessions, spaces, tabs, panes, agents, persistence, and process control. This app renders a small Warp/Arc-style shell over the running Herdr server and embeds a libghostty-backed terminal for the active pane.

## Requirements

- macOS
- Rust
- `wax`
- `herdr`
- bundled or discoverable `libghostty-vt`

If `herdr` is missing, the app tries `wax install herdr`.

## Run

```sh
crepus dev
```

or:

```sh
cargo run
```

## Checks

```sh
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## Scope

- Herdr socket API only
- GPUI via `crepuscularity-gpui`
- libghostty VT rendering
- no browser panes
- no plugin marketplace
- no cloud account layer
- no telemetry
