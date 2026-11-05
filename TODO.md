# TODO

## Reliability and safety

- Verify background Herdr commands and explicit reconnect after bootstrap failure with a real server, including a stalled or timed-out request.
- Distinguish an attachment failure from a terminal exit before offering automatic recovery. A disconnected attachment no longer closes a workspace; verify the server's exit semantics before restoring automatic handling.
- Reserve unmodified arrow keys for terminal input. Verify GPUI action dispatch and terminal input together; Right and Shift-Right currently also bind pane operations.
- Show snapshot freshness and reconcile state after reconnect. Agent-list failures are indicated separately from an empty list; use Herdr events when their subscription API is stable.

## Terminal and workspace capabilities

- Render simultaneous split panes using Herdr's `pane.layout` where supported. Split commands exist, but `pane_grid` renders only the focused or first pane; live splits need a terminal session and geometry for each pane.
- Add terminal selection and copy, measured cell geometry, and scrollback search. Check the bundled libghostty selection API and handle wide characters, combining characters, wraps, and scrollback on macOS.
- Record the bundled libghostty archive's source revision and matching headers before calling its selection API. The archive exports selection symbols, but this repository does not identify their ABI.
- Explore a needs-attention view of Herdr agents that navigates to the relevant pane. Keep status and process ownership in Herdr.
- Make Windows support a separate, tested port. PR #2 explores named pipes and an interim snapshot terminal, but the latter lacks scrollback and accurate cursor state, and its input paths need validation.

## macOS interaction checks

- Interactive checks remain unverified without a Mac. Exercise multiline and bracketed paste, terminal shortcuts, split panes, reconnect and attachment failure against a running Herdr server. Check paste against older Herdr releases as well as the current release.
- Confirm startup clears a legacy request log on a real Mac. Files that are not owned by the user, are symlinks, or have multiple hard links are deliberately not cleared; inspect cleanup errors on stderr.

## Deferred by backend scope

- Add ACP only after Herdr event subscription is stable and there is a Herdr-owned ACP surface.
- Add a full settings editor only if Herdr exposes settings over the socket API; for now the app can reload Herdr config.
