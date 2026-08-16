//! Pure verb-based state machine. No free-form `set_status` / `update_status`.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Lifecycle status of a requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Todo,
    InProgress,
    Review,
    Done,
    Cancelled,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Todo => "todo",
            Status::InProgress => "in_progress",
            Status::Review => "review",
            Status::Done => "done",
            Status::Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "todo" => Some(Status::Todo),
            "in_progress" => Some(Status::InProgress),
            "review" => Some(Status::Review),
            "done" => Some(Status::Done),
            "cancelled" => Some(Status::Cancelled),
            _ => None,
        }
    }
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Actor role (token-bound later; modeled here for transition rules).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Admin,
    Planner,
    Foreman,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Planner => "planner",
            Role::Foreman => "foreman",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "admin" => Some(Role::Admin),
            "planner" => Some(Role::Planner),
            "foreman" => Some(Role::Foreman),
            _ => None,
        }
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Verb-based transitions only. There is no free-form status update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transition {
    /// todo → in_progress; sets claimed_by to actor
    ClaimTask,
    /// any non-terminal (except cancelled) — status unchanged; progress note only
    ReportProgress,
    /// in_progress → review; must be claimant or admin
    SubmitForReview,
    /// review → done (pass) or todo (fail)
    CompleteReview { pass: bool },
    /// in_progress → todo; claimant or admin; clears claim
    ReleaseTask,
    /// → cancelled (role-scoped; soft cancel only)
    Cancel,
}

/// Context required to evaluate / apply a transition (pure; no I/O).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionContext {
    pub current: Status,
    pub role: Role,
    /// Actor identity (token name / user id).
    pub actor: String,
    /// Current claimant, if any.
    pub claimed_by: Option<String>,
}

/// Outcome of a successful transition application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionResult {
    pub new_status: Status,
    /// If `Some(None)`, clear claimed_by. If `Some(Some(x))`, set claimed_by. If None, leave as-is.
    pub claimed_by_update: Option<Option<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TransitionError {
    #[error("transition `{verb}` is not allowed from status `{from}`")]
    IllegalFromStatus { verb: &'static str, from: Status },

    #[error("role `{role}` cannot perform `{verb}`")]
    ForbiddenRole { role: Role, verb: &'static str },

    #[error("actor `{actor}` is not the claimant (claimed_by={claimed_by:?})")]
    NotClaimant {
        actor: String,
        claimed_by: Option<String>,
    },

    #[error("requirement is already claimed by `{claimed_by}`")]
    AlreadyClaimed { claimed_by: String },

    #[error("requirement is not claimable in status `{status}`")]
    NotClaimable { status: Status },

    #[error("terminal status `{status}` rejects further transitions")]
    Terminal { status: Status },
}

fn verb_name(t: &Transition) -> &'static str {
    match t {
        Transition::ClaimTask => "claim_task",
        Transition::ReportProgress => "report_progress",
        Transition::SubmitForReview => "submit_for_review",
        Transition::CompleteReview { pass: true } => "complete_review_pass",
        Transition::CompleteReview { pass: false } => "complete_review_fail",
        Transition::ReleaseTask => "release_task",
        Transition::Cancel => "cancel",
    }
}

/// Check whether a transition is allowed (pure).
pub fn can_transition(ctx: &TransitionContext, transition: &Transition) -> Result<(), TransitionError> {
    // Done is fully terminal. Cancelled is terminal for all MVP verbs.
    if matches!(ctx.current, Status::Done | Status::Cancelled) {
        return Err(TransitionError::Terminal {
            status: ctx.current,
        });
    }

    match transition {
        Transition::ClaimTask => {
            // Only from todo, unclaimed.
            if ctx.current != Status::Todo {
                return Err(TransitionError::NotClaimable {
                    status: ctx.current,
                });
            }
            if let Some(ref owner) = ctx.claimed_by {
                return Err(TransitionError::AlreadyClaimed {
                    claimed_by: owner.clone(),
                });
            }
            // Foreman, planner, admin may claim (HTTP may restrict further).
            if !matches!(ctx.role, Role::Foreman | Role::Planner | Role::Admin) {
                return Err(TransitionError::ForbiddenRole {
                    role: ctx.role,
                    verb: verb_name(transition),
                });
            }
            Ok(())
        }

        Transition::ReportProgress => {
            // Allowed while in_progress (or review) by claimant / admin.
            if !matches!(ctx.current, Status::InProgress | Status::Review) {
                return Err(TransitionError::IllegalFromStatus {
                    verb: verb_name(transition),
                    from: ctx.current,
                });
            }
            ensure_claimant_or_admin(ctx, transition)?;
            Ok(())
        }

        Transition::SubmitForReview => {
            if ctx.current != Status::InProgress {
                return Err(TransitionError::IllegalFromStatus {
                    verb: verb_name(transition),
                    from: ctx.current,
                });
            }
            ensure_claimant_or_admin(ctx, transition)?;
            // Foreman (claimant) primarily; planner/admin if they claimed or admin force.
            if !matches!(ctx.role, Role::Foreman | Role::Planner | Role::Admin) {
                return Err(TransitionError::ForbiddenRole {
                    role: ctx.role,
                    verb: verb_name(transition),
                });
            }
            Ok(())
        }

        Transition::CompleteReview { .. } => {
            if ctx.current != Status::Review {
                return Err(TransitionError::IllegalFromStatus {
                    verb: verb_name(transition),
                    from: ctx.current,
                });
            }
            // Planner or admin complete review (HTTP MVP may restrict to admin only).
            if !matches!(ctx.role, Role::Planner | Role::Admin) {
                return Err(TransitionError::ForbiddenRole {
                    role: ctx.role,
                    verb: verb_name(transition),
                });
            }
            Ok(())
        }

        Transition::ReleaseTask => {
            if ctx.current != Status::InProgress {
                return Err(TransitionError::IllegalFromStatus {
                    verb: verb_name(transition),
                    from: ctx.current,
                });
            }
            // Claimant or admin may release.
            ensure_claimant_or_admin(ctx, transition)?;
            Ok(())
        }

        Transition::Cancel => {
            match ctx.role {
                Role::Planner => {
                    // Planner: todo only in MVP.
                    if ctx.current != Status::Todo {
                        return Err(TransitionError::IllegalFromStatus {
                            verb: verb_name(transition),
                            from: ctx.current,
                        });
                    }
                    Ok(())
                }
                Role::Admin => {
                    // Admin: any non-terminal (todo / in_progress / review).
                    if matches!(ctx.current, Status::Done | Status::Cancelled) {
                        return Err(TransitionError::Terminal {
                            status: ctx.current,
                        });
                    }
                    Ok(())
                }
                Role::Foreman => Err(TransitionError::ForbiddenRole {
                    role: ctx.role,
                    verb: verb_name(transition),
                }),
            }
        }
    }
}

fn ensure_claimant(
    ctx: &TransitionContext,
    transition: &Transition,
) -> Result<(), TransitionError> {
    match &ctx.claimed_by {
        Some(owner) if owner == &ctx.actor => Ok(()),
        other => {
            let _ = transition;
            Err(TransitionError::NotClaimant {
                actor: ctx.actor.clone(),
                claimed_by: other.clone(),
            })
        }
    }
}

fn ensure_claimant_or_admin(
    ctx: &TransitionContext,
    transition: &Transition,
) -> Result<(), TransitionError> {
    if ctx.role == Role::Admin {
        return Ok(());
    }
    ensure_claimant(ctx, transition)
}

/// Apply a transition: validate then compute next status / claim updates (pure).
pub fn apply_transition(
    ctx: &TransitionContext,
    transition: &Transition,
) -> Result<TransitionResult, TransitionError> {
    can_transition(ctx, transition)?;

    let result = match transition {
        Transition::ClaimTask => TransitionResult {
            new_status: Status::InProgress,
            claimed_by_update: Some(Some(ctx.actor.clone())),
        },
        Transition::ReportProgress => TransitionResult {
            new_status: ctx.current,
            claimed_by_update: None,
        },
        Transition::SubmitForReview => TransitionResult {
            new_status: Status::Review,
            claimed_by_update: None,
        },
        Transition::CompleteReview { pass: true } => TransitionResult {
            new_status: Status::Done,
            claimed_by_update: None,
        },
        Transition::CompleteReview { pass: false } => TransitionResult {
            new_status: Status::Todo,
            claimed_by_update: Some(None), // clear claim on fail-back to todo
        },
        Transition::ReleaseTask => TransitionResult {
            new_status: Status::Todo,
            claimed_by_update: Some(None),
        },
        Transition::Cancel => TransitionResult {
            new_status: Status::Cancelled,
            claimed_by_update: Some(None),
        },
    };

    Ok(result)
}

/// Create always yields `todo` (no free-form status).
pub fn status_on_create() -> Status {
    Status::Todo
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(
        status: Status,
        role: Role,
        actor: &str,
        claimed_by: Option<&str>,
    ) -> TransitionContext {
        TransitionContext {
            current: status,
            role,
            actor: actor.to_string(),
            claimed_by: claimed_by.map(str::to_string),
        }
    }

    #[test]
    fn create_is_always_todo() {
        assert_eq!(status_on_create(), Status::Todo);
    }

    #[test]
    fn claim_from_todo_ok() {
        let c = ctx(Status::Todo, Role::Foreman, "alice", None);
        let r = apply_transition(&c, &Transition::ClaimTask).unwrap();
        assert_eq!(r.new_status, Status::InProgress);
        assert_eq!(r.claimed_by_update, Some(Some("alice".into())));
    }

    #[test]
    fn claim_already_claimed_fails() {
        let c = ctx(Status::Todo, Role::Foreman, "bob", Some("alice"));
        let err = apply_transition(&c, &Transition::ClaimTask).unwrap_err();
        assert!(matches!(err, TransitionError::AlreadyClaimed { .. }));
    }

    #[test]
    fn claim_from_in_progress_fails() {
        let c = ctx(Status::InProgress, Role::Foreman, "alice", Some("alice"));
        let err = apply_transition(&c, &Transition::ClaimTask).unwrap_err();
        assert!(matches!(err, TransitionError::NotClaimable { .. }));
    }

    #[test]
    fn submit_for_review_requires_claimant() {
        let c = ctx(Status::InProgress, Role::Foreman, "bob", Some("alice"));
        let err = apply_transition(&c, &Transition::SubmitForReview).unwrap_err();
        assert!(matches!(err, TransitionError::NotClaimant { .. }));
    }

    #[test]
    fn submit_for_review_admin_ok_even_if_not_claimant() {
        let c = ctx(Status::InProgress, Role::Admin, "admin", Some("alice"));
        let r = apply_transition(&c, &Transition::SubmitForReview).unwrap();
        assert_eq!(r.new_status, Status::Review);
    }

    #[test]
    fn submit_for_review_ok() {
        let c = ctx(Status::InProgress, Role::Foreman, "alice", Some("alice"));
        let r = apply_transition(&c, &Transition::SubmitForReview).unwrap();
        assert_eq!(r.new_status, Status::Review);
    }

    #[test]
    fn complete_review_pass_done() {
        let c = ctx(Status::Review, Role::Planner, "planner1", Some("alice"));
        let r = apply_transition(&c, &Transition::CompleteReview { pass: true }).unwrap();
        assert_eq!(r.new_status, Status::Done);
    }

    #[test]
    fn complete_review_fail_back_to_todo() {
        let c = ctx(Status::Review, Role::Planner, "planner1", Some("alice"));
        let r = apply_transition(&c, &Transition::CompleteReview { pass: false }).unwrap();
        assert_eq!(r.new_status, Status::Todo);
        assert_eq!(r.claimed_by_update, Some(None));
    }

    #[test]
    fn foreman_cannot_complete_review() {
        let c = ctx(Status::Review, Role::Foreman, "alice", Some("alice"));
        let err =
            apply_transition(&c, &Transition::CompleteReview { pass: true }).unwrap_err();
        assert!(matches!(err, TransitionError::ForbiddenRole { .. }));
    }

    #[test]
    fn release_task_claimant_or_admin() {
        let c = ctx(Status::InProgress, Role::Foreman, "alice", Some("alice"));
        let r = apply_transition(&c, &Transition::ReleaseTask).unwrap();
        assert_eq!(r.new_status, Status::Todo);
        assert_eq!(r.claimed_by_update, Some(None));

        let c2 = ctx(Status::InProgress, Role::Foreman, "bob", Some("alice"));
        assert!(apply_transition(&c2, &Transition::ReleaseTask).is_err());

        let c3 = ctx(Status::InProgress, Role::Admin, "admin", Some("alice"));
        let r3 = apply_transition(&c3, &Transition::ReleaseTask).unwrap();
        assert_eq!(r3.new_status, Status::Todo);
        assert_eq!(r3.claimed_by_update, Some(None));
    }

    #[test]
    fn planner_cancel_todo_only() {
        let c = ctx(Status::Todo, Role::Planner, "p1", None);
        assert_eq!(
            apply_transition(&c, &Transition::Cancel)
                .unwrap()
                .new_status,
            Status::Cancelled
        );

        let c2 = ctx(Status::InProgress, Role::Planner, "p1", Some("alice"));
        assert!(matches!(
            apply_transition(&c2, &Transition::Cancel).unwrap_err(),
            TransitionError::IllegalFromStatus { .. }
        ));
    }

    #[test]
    fn admin_can_cancel_in_progress() {
        let c = ctx(Status::InProgress, Role::Admin, "admin", Some("alice"));
        let r = apply_transition(&c, &Transition::Cancel).unwrap();
        assert_eq!(r.new_status, Status::Cancelled);
    }

    #[test]
    fn foreman_cannot_cancel() {
        let c = ctx(Status::Todo, Role::Foreman, "alice", None);
        assert!(matches!(
            apply_transition(&c, &Transition::Cancel).unwrap_err(),
            TransitionError::ForbiddenRole { .. }
        ));
    }

    #[test]
    fn done_is_terminal() {
        let c = ctx(Status::Done, Role::Admin, "admin", Some("alice"));
        assert!(matches!(
            apply_transition(&c, &Transition::Cancel).unwrap_err(),
            TransitionError::Terminal { .. }
        ));
        assert!(apply_transition(&c, &Transition::ClaimTask).is_err());
    }

    #[test]
    fn cancelled_is_terminal() {
        let c = ctx(Status::Cancelled, Role::Admin, "admin", None);
        assert!(matches!(
            apply_transition(&c, &Transition::ClaimTask).unwrap_err(),
            TransitionError::Terminal { .. }
        ));
    }

    #[test]
    fn report_progress_no_status_change() {
        let c = ctx(Status::InProgress, Role::Foreman, "alice", Some("alice"));
        let r = apply_transition(&c, &Transition::ReportProgress).unwrap();
        assert_eq!(r.new_status, Status::InProgress);
        assert_eq!(r.claimed_by_update, None);
    }

    #[test]
    fn illegal_submit_from_todo() {
        let c = ctx(Status::Todo, Role::Foreman, "alice", None);
        assert!(matches!(
            apply_transition(&c, &Transition::SubmitForReview).unwrap_err(),
            TransitionError::IllegalFromStatus { .. }
        ));
    }

    #[test]
    fn no_free_form_status_api_exists() {
        // Compile-time / API surface: only Transition verbs; Status is set via apply_transition.
        // This test documents that callers must go through verbs.
        let allowed = [
            Transition::ClaimTask,
            Transition::ReportProgress,
            Transition::SubmitForReview,
            Transition::CompleteReview { pass: true },
            Transition::CompleteReview { pass: false },
            Transition::ReleaseTask,
            Transition::Cancel,
        ];
        assert_eq!(allowed.len(), 7);
    }
}
