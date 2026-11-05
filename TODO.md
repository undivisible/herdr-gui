# TODO

## Reliability and safety

- Move blocking Herdr calls off the UI thread and allow reconnecting after bootstrap fails. Socket reads and writes have timeouts, but calls still block their caller.
- Distinguish an attachment failure from a terminal exit before offering automatic recovery. A disconnected attachment no longer closes a workspace; verify the server's exit semantics before restoring automatic handling.
- Reserve unmodified arrow keys for terminal input. Verify GPUI action dispatch and terminal input together; Right and Shift-Right currently also bind pane operations.
- Report agent-list failures and snapshot freshness instead of silently showing an empty list. Reconcile state after reconnect; use Herdr events when their subscription API is stable.

## Terminal and workspace capabilities

- Render simultaneous split panes if Herdr exposes their layout. Split commands exist, but `pane_grid` renders only the focused or first pane.
- Add terminal selection and copy, measured cell geometry, and scrollback search. Check wide characters, styles, and real terminal applications on macOS.
- Explore a needs-attention view of Herdr agents that navigates to the relevant pane. Keep status and process ownership in Herdr.
- Make Windows support a separate, tested port. PR #2 explores named pipes and an interim snapshot terminal, but the latter lacks scrollback and accurate cursor state, and its input paths need validation.

## macOS interaction checks

- Interactive checks remain unverified without a Mac. Exercise multiline and bracketed paste, terminal shortcuts, split panes, reconnect and attachment failure against a running Herdr server. Check paste against older Herdr releases as well as the current release.
- Confirm startup clears a legacy request log on a real Mac. Files that are not owned by the user, are symlinks, or have multiple hard links are deliberately not cleared; inspect cleanup errors on stderr.

## Deferred by backend scope

- Add ACP only after Herdr event subscription is stable and there is a Herdr-owned ACP surface.
- Add a full settings editor only if Herdr exposes settings over the socket API; for now the app can reload Herdr config.
