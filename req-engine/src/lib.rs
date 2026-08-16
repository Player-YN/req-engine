//! Requirements Engine — verb-based requirement lifecycle (no free-form status updates).

pub mod db;
pub mod desktop;
pub mod domain;
pub mod http;
pub mod mcp;
pub mod paths;
pub mod services;

pub use domain::state::{
    Role, Status, Transition, TransitionError, apply_transition, can_transition,
};
pub use paths::{default_home, resolve_home};
