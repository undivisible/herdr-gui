# TODO

## Reliability and safety

- Keep workspace lifecycle in Herdr: in Herdr 0.9.5, `pane.close` closes the workspace when its last pane closes. Do not follow it with a GUI-side `workspace.close`: focus may already have moved to another workspace, so a second close could remove the wrong one. See [Herdr's pane close handler](https://github.com/herdrdev/herdr/blob/ee5bfb5041bb3e4a49d591c72308aae3c81ecdde/src/app/api/panes.rs#L1959-L2000); verify behavior when supporting other server versions.
- Verify background Herdr commands and explicit reconnect after bootstrap failure with a real server, including a stalled or timed-out request. End-to-end controller sequencing remains unverified: exercise a failed command, skipped queued commands, failed and successful explicit refresh, and recovery without replay. The Linux orb cannot run the bundled macOS Ghostty archive; passing worker tests or macOS compilation alone does not verify this flow.
- Distinguish an attachment failure from a terminal exit before offering automatic recovery. A disconnected attachment no longer closes a workspace; verify the server's exit semantics before restoring automatic handling.
- Reserve unmodified arrow keys for terminal input. Verify GPUI action dispatch and terminal input together; Right and Shift-Right currently also bind pane operations.
- Show snapshot freshness and reconcile state after reconnect. Agent-list failures are indicated separately from an empty list; use Herdr events when their subscription API is stable.

## Terminal and workspace capabilities

- Render simultaneous split panes using Herdr's `pane.layout` where supported. Split commands exist, but `pane_grid` renders only the focused or first pane; live splits need a terminal session and geometry for each pane.
- Model each visible split as a pane-keyed session with its own attachment, frame, geometry, resize and lifetime token; keep input tied to confirmed focus. With the current attachment mechanism, each visible pane adds a `herdr terminal attach` process, PTY, reader thread and Ghostty render state, but not another shell. The implementation is roughly 1–2 engineering days, not a measured estimate; memory and CPU costs remain unmeasured. Check server support and concurrent attachment behavior before implementing fallback.
- Add terminal selection and copy, measured cell geometry, and scrollback search. Check the bundled libghostty selection API and handle wide characters, combining characters, wraps, and scrollback on macOS.
- Draft selection is unreliable during sustained output: it cancels with a warning when the painted terminal revision changes rather than risking selection of different text. Ghostty's tracked references follow grid locations, but cannot reconstruct the location of a cell from a stale painted frame. Preserving drag selection through output requires safe painted-frame-to-content mapping and validation of discarded or overwritten locations; measure the cost before retaining per-cell tracked references.
- The bundled universal libghostty archive matches the [upstream artifact for revision `842badca5f1fc8e8bca1521bf7c619d26561aaa5`](https://tip.files.ghostty.org/842badca5f1fc8e8bca1521bf7c619d26561aaa5/ghostty-vt.xcframework.zip) byte for byte: 17,564,536 bytes, SHA-256 `f67ccc5b3375ec531d933490e9d823dc2dbe7a51b1c2b463c1ee5d0cbd508f69`. Use [headers from that revision](https://github.com/ghostty-org/ghostty/tree/842badca5f1fc8e8bca1521bf7c619d26561aaa5/include/ghostty/vt) for selection bindings. Ghostty 1.4.0 changes the terminal constructor ABI; do not replace the archive without auditing the Rust FFI and testing on macOS.
- Explore a needs-attention view of Herdr agents that navigates to the relevant pane. Keep status and process ownership in Herdr.
- Make Windows support a separate, tested port. PR #2 explores named pipes and an interim snapshot terminal, but the latter lacks scrollback and accurate cursor state, and its input paths need validation.

## macOS interaction checks

- Interactive checks remain unverified without a Mac. Exercise multiline and bracketed paste, terminal shortcuts, split panes, reconnect and attachment failure against a running Herdr server. Check paste against older Herdr releases as well as the current release.
- Confirm startup clears a legacy request log on a real Mac. Files that are not owned by the user, are symlinks, or have multiple hard links are deliberately not cleared; inspect cleanup errors on stderr.

## Deferred by backend scope

- Add ACP only after Herdr event subscription is stable and there is a Herdr-owned ACP surface.
- Add a full settings editor only if Herdr exposes settings over the socket API; for now the app can reload Herdr config.
