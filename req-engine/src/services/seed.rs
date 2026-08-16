//! Idempotent-ish demo seed data aligned with the UI mock projects.

use rusqlite::Connection;
use thiserror::Error;

use crate::domain::models::CreateRequirementInput;
use crate::services::projects::{create_project_with_id, get_project, ProjectError};
use crate::services::requirements::{create_requirement, CreateRequirementError};

#[derive(Debug, Error)]
pub enum SeedError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error(transparent)]
    Project(#[from] ProjectError),

    #[error(transparent)]
    Requirement(#[from] CreateRequirementError),
}

struct SeedProject {
    id: &'static str,
    name: &'static str,
    color: &'static str,
    blurb: &'static str,
    requirements: &'static [SeedReq],
}

struct SeedReq {
    title: &'static str,
    description: &'static str,
    priority: &'static str,
    scope: &'static str,
    non_scope: &'static str,
    acceptance: &'static str,
}

const SEED: &[SeedProject] = &[
    SeedProject {
        id: "demo-shop",
        name: "Demo Shop",
        color: "#f59e0b",
        blurb: "Sample e-commerce demo for lifecycle walkthrough",
        requirements: &[
            SeedReq {
                title: "Cart checkout flow",
                description: "User can add items and complete checkout",
                priority: "high",
                scope: r#"["cart","checkout","payment mock"]"#,
                non_scope: r#"["real payment gateway"]"#,
                acceptance: r#"["cart totals correct","order confirmation page"]"#,
            },
            SeedReq {
                title: "Product catalog page",
                description: "List products with filters",
                priority: "medium",
                scope: r#"["list","filter by category"]"#,
                non_scope: r#"["recommendations"]"#,
                acceptance: r#"["shows 12 products","filter updates list"]"#,
            },
        ],
    },
    SeedProject {
        id: "trace-sight",
        name: "Trace Sight",
        color: "#06b6d4",
        blurb: "Observability / tracing UI mock",
        requirements: &[
            SeedReq {
                title: "Span timeline view",
                description: "Visualize request spans on a timeline",
                priority: "high",
                scope: r#"["timeline","hover details"]"#,
                non_scope: r#"["live tail"]"#,
                acceptance: r#"["renders sample trace","hover shows span attrs"]"#,
            },
            SeedReq {
                title: "Service map",
                description: "Graph of service dependencies from traces",
                priority: "medium",
                scope: r#"["nodes","edges","latency badge"]"#,
                non_scope: r#"["edit topology"]"#,
                acceptance: r#"["loads mock graph","click node filters list"]"#,
            },
        ],
    },
    SeedProject {
        id: "req-engine",
        name: "Req Engine",
        color: "#6366f1",
        blurb: "This product — verb-based requirements lifecycle",
        requirements: &[
            SeedReq {
                title: "HTTP serve + auth",
                description: "Axum server with bearer token roles",
                priority: "high",
                scope: r#"["/v1 routes","CORS localhost","token auth"]"#,
                non_scope: r#"["OAuth","MCP full"]"#,
                acceptance: r#"["health ok","claim verb works","planner cannot claim"]"#,
            },
            SeedReq {
                title: "Seed demo projects",
                description: "Four UI-aligned projects with sample requirements",
                priority: "medium",
                scope: r#"["demo-shop","trace-sight","req-engine","mobile-h5"]"#,
                non_scope: r#"["production data migration"]"#,
                acceptance: r#"["seed is idempotent-ish","list projects returns 4"]"#,
            },
        ],
    },
    SeedProject {
        id: "mobile-h5",
        name: "Mobile H5",
        color: "#ec4899",
        blurb: "Mobile web (H5) client shell",
        requirements: &[
            SeedReq {
                title: "Bottom navigation shell",
                description: "Tab bar for Home / Tasks / Me",
                priority: "high",
                scope: r#"["tabs","routing"]"#,
                non_scope: r#"["native apps"]"#,
                acceptance: r#"["three tabs","active state visible"]"#,
            },
            SeedReq {
                title: "Task card list",
                description: "Show requirement cards with status chips",
                priority: "medium",
                scope: r#"["card","status chip","priority"]"#,
                non_scope: r#"["offline sync"]"#,
                acceptance: r#"["loads from API","empty state"]"#,
            },
        ],
    },
];

/// Seed four demo projects + sample requirements. Safe to re-run: skips existing project ids.
pub fn seed_demo_data(conn: &Connection) -> Result<SeedReport, SeedError> {
    let mut report = SeedReport::default();

    for p in SEED {
        if get_project(conn, p.id)?.is_some() {
            report.projects_skipped += 1;
            continue;
        }

        create_project_with_id(conn, p.id, p.name, p.color, p.blurb, "")?;
        report.projects_created += 1;

        for r in p.requirements {
            create_requirement(
                conn,
                CreateRequirementInput {
                    project_id: p.id.to_string(),
                    title: r.title.to_string(),
                    description: r.description.to_string(),
                    priority: r.priority.to_string(),
                    scope_json: r.scope.to_string(),
                    non_scope_json: r.non_scope.to_string(),
                    acceptance_json: r.acceptance.to_string(),
                    dependencies_json: "[]".to_string(),
                    created_by: "seed".to_string(),
                },
            )?;
            report.requirements_created += 1;
        }
    }

    Ok(report)
}

#[derive(Debug, Default, Clone)]
pub struct SeedReport {
    pub projects_created: u32,
    pub projects_skipped: u32,
    pub requirements_created: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::services::projects::list_projects;

    #[test]
    fn seed_creates_four_projects_and_is_idempotent() {
        let conn = open_in_memory().unwrap();
        let r1 = seed_demo_data(&conn).unwrap();
        assert_eq!(r1.projects_created, 4);
        assert!(r1.requirements_created >= 8);

        let r2 = seed_demo_data(&conn).unwrap();
        assert_eq!(r2.projects_created, 0);
        assert_eq!(r2.projects_skipped, 4);

        let projects = list_projects(&conn).unwrap();
        assert_eq!(projects.len(), 4);
        let ids: Vec<_> = projects.iter().map(|p| p.id.as_str()).collect();
        assert!(ids.contains(&"demo-shop"));
        assert!(ids.contains(&"trace-sight"));
        assert!(ids.contains(&"req-engine"));
        assert!(ids.contains(&"mobile-h5"));
    }
}
