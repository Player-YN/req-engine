//! Application services — lifecycle verbs, projects, tokens, seed.

pub mod client_host;
pub mod onboarding;
pub mod pair_codes;
pub mod presence;
pub mod projects;
pub mod requirements;
pub mod seed;
pub mod tokens;

pub use projects::{
    ProjectError, ack_agent_seat, archive_project, create_project, create_project_with_id,
    ensure_project_writable, get_project, list_projects, list_projects_filtered, normalize_local_path,
    unarchive_project,
    update_project,
};
pub use requirements::{
    ClaimError, CreateRequirementError, UpdateRequirementError, UpdateRequirementInput, VerbError,
    cancel_requirement, claim_task, complete_review, create_requirement, get_requirement,
    list_events_for_requirement, list_ready_tasks, list_requirements_for_project,
    list_requirements_for_project_filtered, release_task, report_progress, submit_for_review,
    update_requirement,
};
pub use seed::{SeedError, SeedReport, seed_demo_data};
pub use tokens::{GeneratedToken, generate_bootstrap_tokens, hash_token, lookup_token};
pub use pair_codes::{
    PairBinding, PairError, SeatPlaintexts, ensure_all_project_pair_codes,
    ensure_project_pair_codes, lookup_pair_code, read_plaintext_codes, rotate_pair_code,
};
pub use onboarding::{OnboardingCtx, onboarding_prompt};
pub use presence::{
    OccupantHint, ProjectPresence, SeatLive, clear_seat_presence, project_presence,
    touch_seat_presence, touch_seat_presence_client, SEAT_TTL,
};
pub use client_host::{recognize, OccupantFace};
