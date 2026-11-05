# TODO

## Reliability and safety

- Preserve multiline paste and its ordering. Check Herdr's paste and bracketed-paste contract before changing `HerdrGui::paste`; the current implementation sends separate text and Enter requests and drops a trailing newline.
- Move blocking Herdr calls off the UI thread, bound socket waits, and allow reconnecting after bootstrap fails. `HerdrClient::call` currently waits for a response without a timeout.
- Distinguish an attachment failure from a terminal exit before closing a workspace. `poll_terminal` currently treats any stream disconnect as an exit; verify the server's close semantics before changing recovery.
- Reserve unmodified arrow keys for terminal input. Verify GPUI action dispatch and terminal input together; Right and Shift-Right currently also bind pane operations.
- Report agent-list failures and snapshot freshness instead of silently showing an empty list. Reconcile state after reconnect; use Herdr events when their subscription API is stable.

## Terminal and workspace capabilities

- Render simultaneous split panes if Herdr exposes their layout. Split commands exist, but `pane_grid` renders only the focused or first pane.
- Add terminal selection and copy, measured cell geometry, and scrollback search. Check wide characters, styles, and real terminal applications on macOS.
- Explore a needs-attention view of Herdr agents that navigates to the relevant pane. Keep status and process ownership in Herdr.
- Make Windows support a separate, tested port. PR #2 explores named pipes and an interim snapshot terminal, but the latter lacks scrollback and accurate cursor state, and its input paths need validation.

## Deferred by backend scope

- Add ACP only after Herdr event subscription is stable and there is a Herdr-owned ACP surface.
- Add a full settings editor only if Herdr exposes settings over the socket API; for now the app can reload Herdr config.
