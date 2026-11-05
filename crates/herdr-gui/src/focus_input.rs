use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

pub fn unique_focused<T>(items: &[T], focused: impl Fn(&T) -> bool) -> Option<&T> {
    let mut matches = items.iter().filter(|item| focused(item));
    let item = matches.next()?;
    matches.next().is_none().then_some(item)
}

#[derive(Debug, PartialEq, Eq)]
pub enum PendingInput {
    Key(String),
    Text(String),
    Paste(String),
}

const DIRECTION_WARNING: &str = "Directional focus could not be confirmed (layout unavailable or destination mismatch); input remains blocked. Click the desired pane to resume input.";

#[derive(Clone, Debug)]
pub enum RecoveryTarget {
    Pane(String),
    Tab(String),
    Workspace(String),
    Agent {
        pane: Option<String>,
        tab: Option<String>,
        workspace: Option<String>,
    },
    Direction {
        previous_pane: String,
        tab: String,
        workspace: Option<String>,
        expected_pane: Arc<Mutex<Option<String>>>,
    },
    DirectionUnavailable,
}

pub struct FocusSnapshot<'a> {
    pub pane: &'a str,
    pub tab: Option<&'a str>,
    pub workspace: Option<&'a str>,
}

impl RecoveryTarget {
    fn matches(&self, focus: &FocusSnapshot<'_>) -> bool {
        match self {
            Self::Pane(id) => focus.pane == id,
            Self::Tab(id) => focus.tab == Some(id),
            Self::Workspace(id) => focus.workspace == Some(id),
            Self::Agent {
                pane,
                tab,
                workspace,
            } => {
                (pane.is_some() || tab.is_some() || workspace.is_some())
                    && pane.as_deref().is_none_or(|id| focus.pane == id)
                    && tab.as_deref().is_none_or(|id| focus.tab == Some(id))
                    && workspace
                        .as_deref()
                        .is_none_or(|id| focus.workspace == Some(id))
            }
            Self::Direction {
                previous_pane,
                tab,
                workspace,
                expected_pane,
            } => {
                focus.pane != previous_pane
                    && focus.tab == Some(tab)
                    && workspace
                        .as_deref()
                        .is_none_or(|id| focus.workspace == Some(id))
                    && expected_pane
                        .lock()
                        .ok()
                        .and_then(|id| id.clone())
                        .as_deref()
                        == Some(focus.pane)
            }
            Self::DirectionUnavailable => false,
        }
    }
}

#[derive(Default)]
pub struct InputBuffer {
    inputs: VecDeque<PendingInput>,
    blocked: bool,
    recovery_target: Option<RecoveryTarget>,
}

impl InputBuffer {
    pub fn push(&mut self, input: PendingInput) {
        if !self.blocked {
            self.inputs.push_back(input);
        }
    }

    pub fn discard(&mut self) {
        self.inputs.clear();
        self.recovery_target = None;
    }

    pub fn reject_focus(&mut self) {
        self.discard();
        self.blocked = true;
    }

    pub fn is_blocked(&self) -> bool {
        self.blocked
    }

    pub fn start_recovery(&mut self, target: RecoveryTarget) {
        if self.blocked {
            self.recovery_target = Some(target);
        }
    }

    pub fn direction_layout_warning(&self) -> Option<&'static str> {
        match self.recovery_target.as_ref()? {
            RecoveryTarget::Direction { expected_pane, .. }
                if expected_pane.lock().ok().is_none_or(|pane| pane.is_none()) =>
            {
                Some(DIRECTION_WARNING)
            }
            RecoveryTarget::DirectionUnavailable => Some(DIRECTION_WARNING),
            _ => None,
        }
    }

    pub fn direction_warning(&self) -> Option<&'static str> {
        matches!(
            self.recovery_target,
            Some(RecoveryTarget::Direction { .. } | RecoveryTarget::DirectionUnavailable)
        )
        .then_some(DIRECTION_WARNING)
    }

    pub fn finish_recovery(&mut self, focus: FocusSnapshot<'_>) -> Option<&'static str> {
        let target = self.recovery_target.take()?;
        if target.matches(&focus) {
            self.blocked = false;
            None
        } else if matches!(
            target,
            RecoveryTarget::Direction { .. } | RecoveryTarget::DirectionUnavailable
        ) {
            Some(DIRECTION_WARNING)
        } else {
            None
        }
    }

    pub fn take_ready(
        &mut self,
        confirmed: bool,
        pane_id: &str,
        expected_target: &str,
        attached_target: Option<&str>,
        token: u64,
        current_token: u64,
    ) -> Option<(String, VecDeque<PendingInput>)> {
        if !confirmed || attached_target != Some(expected_target) || token != current_token {
            return None;
        }
        Some((pane_id.to_string(), std::mem::take(&mut self.inputs)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn focus(pane: &str) -> FocusSnapshot<'_> {
        FocusSnapshot {
            pane,
            tab: Some("tab"),
            workspace: Some("workspace"),
        }
    }

    #[test]
    fn unsupported_direction_layout_keeps_input_blocked_and_explains_recovery() {
        let mut input = InputBuffer::default();
        input.reject_focus();
        input.start_recovery(RecoveryTarget::Direction {
            previous_pane: "B".into(),
            tab: "tab".into(),
            workspace: Some("workspace".into()),
            expected_pane: Arc::new(Mutex::new(None)),
        });
        assert_eq!(input.direction_layout_warning(), Some(DIRECTION_WARNING));
        assert_eq!(input.finish_recovery(focus("C")), Some(DIRECTION_WARNING));
        assert!(input.is_blocked());
    }

    #[test]
    fn missing_origin_and_mismatched_destination_are_actionable() {
        let mut input = InputBuffer::default();
        input.reject_focus();
        input.start_recovery(RecoveryTarget::DirectionUnavailable);
        assert_eq!(input.direction_layout_warning(), Some(DIRECTION_WARNING));
        assert_eq!(input.finish_recovery(focus("C")), Some(DIRECTION_WARNING));
        input.start_recovery(RecoveryTarget::Direction {
            previous_pane: "B".into(),
            tab: "tab".into(),
            workspace: Some("workspace".into()),
            expected_pane: Arc::new(Mutex::new(Some("other".into()))),
        });
        assert_eq!(input.direction_layout_warning(), None);
        assert_eq!(input.finish_recovery(focus("C")), Some(DIRECTION_WARNING));
        assert!(input.is_blocked());
    }

    #[test]
    fn focus_requires_exactly_one_focused_pane() {
        let none = [("first", false), ("second", false)];
        assert_eq!(unique_focused(&none, |pane| pane.1), None);

        let one = [("first", false), ("second", true)];
        assert_eq!(unique_focused(&one, |pane| pane.1), Some(&("second", true)));

        let multiple = [("first", true), ("second", true)];
        assert_eq!(unique_focused(&multiple, |pane| pane.1), None);
        assert_eq!(unique_focused::<(&str, bool)>(&[], |pane| pane.1), None);
    }

    #[test]
    fn waits_for_confirmed_current_attachment_and_preserves_fifo() {
        let mut input = InputBuffer::default();
        input.push(PendingInput::Text("a".into()));
        input.push(PendingInput::Key("enter".into()));
        input.push(PendingInput::Paste("b".into()));
        assert_eq!(
            input.take_ready(false, "pane-new", "new", Some("old"), 1, 1),
            None
        );
        assert_eq!(
            input.take_ready(true, "pane-new", "new", Some("old"), 1, 1),
            None
        );
        assert_eq!(
            input.take_ready(true, "pane-new", "new", Some("new"), 1, 2),
            None
        );
        assert_eq!(
            input.take_ready(true, "pane-new", "new", Some("new"), 2, 2),
            Some((
                "pane-new".into(),
                VecDeque::from([
                    PendingInput::Text("a".into()),
                    PendingInput::Key("enter".into()),
                    PendingInput::Paste("b".into()),
                ])
            ))
        );
        assert_eq!(
            input.take_ready(true, "pane-new", "new", Some("new"), 2, 2),
            Some(("pane-new".into(), VecDeque::new()))
        );
    }

    #[test]
    fn discarded_input_is_never_replayed_after_failure() {
        let mut input = InputBuffer::default();
        input.push(PendingInput::Text("secret".into()));
        input.discard();
        assert_eq!(
            input.take_ready(true, "pane-new", "new", Some("new"), 2, 2),
            Some(("pane-new".into(), VecDeque::new()))
        );
    }

    #[test]
    fn rejected_second_focus_must_not_send_later_paste_to_first_target() {
        let mut input = InputBuffer::default();
        input.push(PendingInput::Text("before".into()));
        input.reject_focus();
        input.push(PendingInput::Paste("intended for C".into()));
        assert_eq!(
            input.take_ready(true, "B", "B-terminal", Some("B-terminal"), 1, 1),
            Some(("B".into(), VecDeque::new()))
        );
        let _ = input.finish_recovery(focus("B"));
        assert!(input.is_blocked());
        input.start_recovery(RecoveryTarget::Pane("C".into()));
        let _ = input.finish_recovery(focus("B"));
        assert!(input.is_blocked());
        input.start_recovery(RecoveryTarget::Pane("C".into()));
        let _ = input.finish_recovery(focus("C"));
        assert!(!input.is_blocked());
        input.push(PendingInput::Text("new input".into()));
        assert_eq!(
            input.take_ready(true, "C", "C-terminal", Some("C-terminal"), 2, 2),
            Some((
                "C".into(),
                VecDeque::from([PendingInput::Text("new input".into())])
            ))
        );
    }

    #[test]
    fn failed_or_superseded_recovery_stays_blocked() {
        let mut input = InputBuffer::default();
        input.reject_focus();
        input.start_recovery(RecoveryTarget::Pane("C".into()));
        input.discard();
        let _ = input.finish_recovery(focus("C"));
        assert!(input.is_blocked());
        input.start_recovery(RecoveryTarget::Pane("C".into()));
        input.reject_focus();
        let _ = input.finish_recovery(focus("C"));
        assert!(input.is_blocked());
        input.push(PendingInput::Paste("not sent".into()));
        assert_eq!(
            input.take_ready(true, "C", "C-terminal", Some("C-terminal"), 3, 3),
            Some(("C".into(), VecDeque::new()))
        );
    }

    #[test]
    fn explicit_focus_routes_match_only_their_confirmed_targets() {
        let targets = [
            RecoveryTarget::Pane("C".into()),
            RecoveryTarget::Tab("tab".into()),
            RecoveryTarget::Workspace("workspace".into()),
            RecoveryTarget::Agent {
                pane: Some("C".into()),
                tab: Some("tab".into()),
                workspace: Some("workspace".into()),
            },
            RecoveryTarget::Direction {
                previous_pane: "B".into(),
                tab: "tab".into(),
                workspace: Some("workspace".into()),
                expected_pane: Arc::new(Mutex::new(Some("C".into()))),
            },
        ];
        for target in targets {
            let mut input = InputBuffer::default();
            input.reject_focus();
            input.start_recovery(target);
            assert_eq!(input.finish_recovery(focus("C")), None);
            assert!(!input.is_blocked());
        }
    }

    #[test]
    fn wrong_missing_or_stale_focus_does_not_recover() {
        let cases = [
            RecoveryTarget::Pane("other".into()),
            RecoveryTarget::Tab("other".into()),
            RecoveryTarget::Workspace("other".into()),
            RecoveryTarget::Agent {
                pane: Some("C".into()),
                tab: Some("other".into()),
                workspace: None,
            },
            RecoveryTarget::Agent {
                pane: None,
                tab: None,
                workspace: None,
            },
            RecoveryTarget::Direction {
                previous_pane: "C".into(),
                tab: "tab".into(),
                workspace: Some("workspace".into()),
                expected_pane: Arc::new(Mutex::new(Some("C".into()))),
            },
            RecoveryTarget::Direction {
                previous_pane: "B".into(),
                tab: "other".into(),
                workspace: Some("workspace".into()),
                expected_pane: Arc::new(Mutex::new(Some("C".into()))),
            },
            RecoveryTarget::Direction {
                previous_pane: "B".into(),
                tab: "tab".into(),
                workspace: Some("workspace".into()),
                expected_pane: Arc::new(Mutex::new(Some("other".into()))),
            },
            RecoveryTarget::Direction {
                previous_pane: "B".into(),
                tab: "tab".into(),
                workspace: Some("workspace".into()),
                expected_pane: Arc::new(Mutex::new(None)),
            },
        ];
        for target in cases {
            let mut input = InputBuffer::default();
            input.reject_focus();
            input.start_recovery(target);
            let _ = input.finish_recovery(focus("C"));
            assert!(input.is_blocked());
        }
        let mut input = InputBuffer::default();
        input.reject_focus();
        input.start_recovery(RecoveryTarget::Tab("tab".into()));
        let _ = input.finish_recovery(FocusSnapshot {
            pane: "C",
            tab: None,
            workspace: Some("workspace"),
        });
        assert!(input.is_blocked());
    }
}
