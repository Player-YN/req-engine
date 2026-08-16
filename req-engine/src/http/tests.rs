//! HTTP integration tests (tower oneshot).

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::db::open_in_memory;
use crate::http::{AppState, app};
use crate::services::tokens::generate_bootstrap_tokens;

struct TestEnv {
    app: axum::Router,
    admin: String,
    planner: String,
    foreman: String,
}

fn setup() -> TestEnv {
    let conn = open_in_memory().unwrap();
    let tokens = generate_bootstrap_tokens(&conn).unwrap();
    let admin = tokens
        .iter()
        .find(|t| t.name == "admin")
        .unwrap()
        .plaintext
        .clone();
    let planner = tokens
        .iter()
        .find(|t| t.name == "planner")
        .unwrap()
        .plaintext
        .clone();
    let foreman = tokens
        .iter()
        .find(|t| t.name == "foreman")
        .unwrap()
        .plaintext
        .clone();
    let tmp = tempfile::tempdir().unwrap();
    let state = AppState::new(conn, tmp.path().to_path_buf());
    std::mem::forget(tmp);
    TestEnv {
        app: app(state),
        admin,
        planner,
        foreman,
    }
}

async fn json_req(
    app: axum::Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        builder = builder.header("Authorization", format!("Bearer {t}"));
    }
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let req = if let Some(b) = body {
        builder.body(Body::from(b.to_string())).unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    };

    let res = app.oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let val = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::String(
            String::from_utf8_lossy(&bytes).into_owned(),
        ))
    };
    (status, val)
}

#[tokio::test]
async fn health_no_auth() {
    let env = setup();
    let (status, body) = json_req(env.app, "GET", "/v1/health", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn planner_cannot_claim_returns_403() {
    let env = setup();

    // Create project + requirement as admin
    let (st, proj) = json_req(
        env.app.clone(),
        "POST",
        "/v1/projects",
        Some(&env.admin),
        Some(json!({"name": "P", "color": "#000", "blurb": ""})),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);
    let pid = proj["id"].as_str().unwrap();

    let (st, req) = json_req(
        env.app.clone(),
        "POST",
        &format!("/v1/projects/{pid}/requirements"),
        Some(&env.planner),
        Some(json!({"title": "T", "description": "d"})),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);
    let rid = req["id"].as_str().unwrap();

    let (st, body) = json_req(
        env.app,
        "POST",
        &format!("/v1/requirements/{rid}/claim"),
        Some(&env.planner),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "body={body}");
}

#[tokio::test]
async fn create_claim_submit_complete_pass() {
    let env = setup();

    let (st, proj) = json_req(
        env.app.clone(),
        "POST",
        "/v1/projects",
        Some(&env.admin),
        Some(json!({"name": "Flow", "color": "#111", "blurb": "x"})),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);
    let pid = proj["id"].as_str().unwrap().to_string();

    let (st, req) = json_req(
        env.app.clone(),
        "POST",
        &format!("/v1/projects/{pid}/requirements"),
        Some(&env.admin),
        Some(json!({
            "title": "Ship",
            "description": "do it",
            "priority": "high",
            "scope": ["a"],
            "acceptance_criteria": ["done"]
        })),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);
    assert_eq!(req["status"], "todo");
    let rid = req["id"].as_str().unwrap().to_string();

    let (st, claimed) = json_req(
        env.app.clone(),
        "POST",
        &format!("/v1/requirements/{rid}/claim"),
        Some(&env.foreman),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(claimed["status"], "in_progress");
    assert_eq!(claimed["claimed_by"], "foreman");

    let (st, _) = json_req(
        env.app.clone(),
        "POST",
        &format!("/v1/requirements/{rid}/progress"),
        Some(&env.foreman),
        Some(json!({"summary": "working"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let (st, review) = json_req(
        env.app.clone(),
        "POST",
        &format!("/v1/requirements/{rid}/submit-review"),
        Some(&env.foreman),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(review["status"], "review");

    let (st, done) = json_req(
        env.app.clone(),
        "POST",
        &format!("/v1/requirements/{rid}/complete-review"),
        Some(&env.admin),
        Some(json!({"pass": true, "reason": "lgtm"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(done["status"], "done");

    let (st, detail) = json_req(
        env.app,
        "GET",
        &format!("/v1/requirements/{rid}"),
        Some(&env.admin),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(detail["status"], "done");
    assert!(detail["events"].as_array().unwrap().len() >= 4);
}

#[tokio::test]
async fn create_project_with_local_path_roundtrip() {
    let env = setup();

    let path = r"C:\Users\demo\repos\my-app";
    let (st, proj) = json_req(
        env.app.clone(),
        "POST",
        "/v1/projects",
        Some(&env.admin),
        Some(json!({
            "name": "Bound Project",
            "color": "#6366f1",
            "blurb": "local folder",
            "local_path": path
        })),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "body={proj}");
    assert_eq!(proj["local_path"], path);
    assert_eq!(proj["name"], "Bound Project");
    let pid = proj["id"].as_str().unwrap().to_string();

    let (st, list) = json_req(
        env.app.clone(),
        "GET",
        "/v1/projects",
        Some(&env.admin),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let arr = list.as_array().expect("list is array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["local_path"], path);

    let (st, patched) = json_req(
        env.app,
        "PATCH",
        &format!("/v1/projects/{pid}"),
        Some(&env.admin),
        Some(json!({"local_path": r"D:\other\path", "blurb": "updated"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "body={patched}");
    assert_eq!(patched["local_path"], r"D:\other\path");
    assert_eq!(patched["blurb"], "updated");
    assert_eq!(patched["name"], "Bound Project");
}

#[tokio::test]
async fn list_projects_empty_without_seed() {
    let env = setup();
    let (st, list) = json_req(env.app, "GET", "/v1/projects", Some(&env.admin), None).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(list.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn double_claim_only_one_succeeds() {
    let env = setup();

    let (st, proj) = json_req(
        env.app.clone(),
        "POST",
        "/v1/projects",
        Some(&env.admin),
        Some(json!({"name": "Race", "color": "#222", "blurb": ""})),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);
    let pid = proj["id"].as_str().unwrap();

    let (st, req) = json_req(
        env.app.clone(),
        "POST",
        &format!("/v1/projects/{pid}/requirements"),
        Some(&env.admin),
        Some(json!({"title": "One winner"})),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);
    let rid = req["id"].as_str().unwrap();

    let (st1, body1) = json_req(
        env.app.clone(),
        "POST",
        &format!("/v1/requirements/{rid}/claim"),
        Some(&env.foreman),
        None,
    )
    .await;
    assert_eq!(st1, StatusCode::OK, "first claim: {body1}");
    assert_eq!(body1["claimed_by"], "foreman");

    // Second claim as admin should also fail (already in_progress / claimed).
    let (st2, body2) = json_req(
        env.app,
        "POST",
        &format!("/v1/requirements/{rid}/claim"),
        Some(&env.admin),
        None,
    )
    .await;
    assert!(
        st2 == StatusCode::CONFLICT || st2 == StatusCode::FORBIDDEN,
        "second claim should fail, got {st2} {body2}"
    );
    assert_ne!(st2, StatusCode::OK);
}

#[tokio::test]
async fn archived_project_rejects_writes_until_unarchive() {
    let env = setup();

    let (st, proj) = json_req(
        env.app.clone(),
        "POST",
        "/v1/projects",
        Some(&env.admin),
        Some(json!({"name": "FreezeMe", "id": "freeze-me"})),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "body={proj}");

    let (st, req) = json_req(
        env.app.clone(),
        "POST",
        "/v1/projects/freeze-me/requirements",
        Some(&env.admin),
        Some(json!({"title": "before-archive"})),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);
    let rid = req["id"].as_str().unwrap().to_string();

    let (st, _) = json_req(
        env.app.clone(),
        "POST",
        "/v1/projects/freeze-me/archive",
        Some(&env.admin),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);

    let (st, list) = json_req(env.app.clone(), "GET", "/v1/projects", Some(&env.admin), None).await;
    assert_eq!(st, StatusCode::OK);
    let ids: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p["id"].as_str())
        .collect();
    assert!(!ids.contains(&"freeze-me"));

    let (st, body) = json_req(
        env.app.clone(),
        "POST",
        "/v1/projects/freeze-me/requirements",
        Some(&env.planner),
        Some(json!({"title": "after-archive"})),
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "create after archive: {body}");
    assert_eq!(body["error"]["code"], "project_archived");

    let (st, body) = json_req(
        env.app.clone(),
        "POST",
        &format!("/v1/requirements/{rid}/claim"),
        Some(&env.foreman),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "claim after archive: {body}");
    assert_eq!(body["error"]["code"], "project_archived");

    let (st, body) = json_req(
        env.app.clone(),
        "GET",
        "/v1/projects/freeze-me/requirements",
        Some(&env.admin),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "reads stay allowed: {body}");
    assert_eq!(body.as_array().unwrap().len(), 1);

    let (st, thawed) = json_req(
        env.app.clone(),
        "POST",
        "/v1/projects/freeze-me/unarchive",
        Some(&env.admin),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "unarchive: {thawed}");
    assert!(thawed["archived_at"].is_null() || thawed.get("archived_at").is_none());

    let (st, after) = json_req(
        env.app.clone(),
        "POST",
        "/v1/projects/freeze-me/requirements",
        Some(&env.planner),
        Some(json!({"title": "after-unarchive"})),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "create after unarchive: {after}");
}

#[tokio::test]
async fn claim_blocked_by_unsatisfied_dependency() {
    let env = setup();
    let (st, proj) = json_req(
        env.app.clone(),
        "POST",
        "/v1/projects",
        Some(&env.admin),
        Some(json!({"name": "Deps", "id": "deps"})),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{proj}");

    let (st, parent) = json_req(
        env.app.clone(),
        "POST",
        "/v1/projects/deps/requirements",
        Some(&env.admin),
        Some(json!({"title": "parent"})),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);
    let parent_id = parent["id"].as_str().unwrap();

    let (st, child) = json_req(
        env.app.clone(),
        "POST",
        "/v1/projects/deps/requirements",
        Some(&env.admin),
        Some(json!({"title": "child", "dependencies": [parent_id]})),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED, "{child}");
    let child_id = child["id"].as_str().unwrap();

    let (st, body) = json_req(
        env.app.clone(),
        "POST",
        &format!("/v1/requirements/{child_id}/claim"),
        Some(&env.foreman),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["error"]["code"], "dependencies_not_met");
}

#[tokio::test]
async fn reject_review_requires_reason() {
    let env = setup();
    let (st, proj) = json_req(
        env.app.clone(),
        "POST",
        "/v1/projects",
        Some(&env.admin),
        Some(json!({"name": "Rev"})),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);
    let proj_id = proj["id"].as_str().unwrap();
    let (st, req) = json_req(
        env.app.clone(),
        "POST",
        &format!("/v1/projects/{proj_id}/requirements"),
        Some(&env.admin),
        Some(json!({"title": "needs-reason"})),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);
    let rid = req["id"].as_str().unwrap();
    assert_eq!(
        json_req(
            env.app.clone(),
            "POST",
            &format!("/v1/requirements/{rid}/claim"),
            Some(&env.foreman),
            None,
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        json_req(
            env.app.clone(),
            "POST",
            &format!("/v1/requirements/{rid}/submit-review"),
            Some(&env.foreman),
            None,
        )
        .await
        .0,
        StatusCode::OK
    );

    let (st, body) = json_req(
        env.app.clone(),
        "POST",
        &format!("/v1/requirements/{rid}/complete-review"),
        Some(&env.admin),
        Some(json!({"pass": false})),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"]["code"], "reject_reason_required");

    let (st, body) = json_req(
        env.app.clone(),
        "POST",
        &format!("/v1/requirements/{rid}/complete-review"),
        Some(&env.admin),
        Some(json!({"pass": false, "reason": "   "})),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "blank reason: {body}");

    let (st, body) = json_req(
        env.app,
        "POST",
        &format!("/v1/requirements/{rid}/complete-review"),
        Some(&env.admin),
        Some(json!({"pass": false, "reason": "missing tests"})),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{body}");
    assert_eq!(body["status"], "todo");
}

#[tokio::test]
async fn pair_codes_issue_and_rotate() {
    let env = setup();
    let (st, proj) = json_req(
        env.app.clone(),
        "POST",
        "/v1/projects",
        Some(&env.admin),
        Some(json!({"name": "Paired"})),
    )
    .await;
    assert_eq!(st, StatusCode::CREATED);
    let pid = proj["id"].as_str().unwrap();

    let (st, codes) = json_req(
        env.app.clone(),
        "GET",
        &format!("/v1/projects/{pid}/pair-codes"),
        Some(&env.admin),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{codes}");
    let disc = codes["discuss"]["code"].as_str().unwrap().to_string();
    let build = codes["build"]["code"].as_str().unwrap().to_string();
    assert!(disc.starts_with("disc_"));
    assert!(build.starts_with("build_"));
    let copy = codes["discuss"]["copy_text"].as_str().unwrap();
    assert!(copy.contains(&disc));
    assert!(copy.contains("--pair"));
    assert!(copy.contains("不要自己下场") || copy.contains("不要改业务代码"));
    assert!(copy.contains("通读代码工作区"));
    assert!(copy.contains("stdio"));
    assert!(copy.contains("--pair"));
    assert!(!copy.contains("claim"));
    assert!(!copy.contains("--home"));
    assert!(!copy.contains("你是谁"));
    assert!(!copy.contains("产品：需求引擎"));
    let copy_l = copy.to_ascii_lowercase();
    assert!(!copy_l.contains("grok"), "{copy}");
    assert!(!copy_l.contains("cursor"), "{copy}");
    assert!(!copy.contains("127.0.0.1"), "{copy}");
    assert_eq!(codes["discuss"]["seated"], false);
    assert_eq!(codes["build"]["seated"], false);

    let (st, denied) = json_req(
        env.app.clone(),
        "GET",
        &format!("/v1/projects/{pid}/pair-codes"),
        Some(&env.planner),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{denied}");

    let (st, rotated) = json_req(
        env.app,
        "POST",
        &format!("/v1/projects/{pid}/pair-codes/discuss/rotate"),
        Some(&env.admin),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{rotated}");
    let new_disc = rotated["discuss"]["code"].as_str().unwrap();
    assert_ne!(new_disc, disc);
    assert!(new_disc.starts_with("disc_"));
    assert_eq!(rotated["build"]["code"], build);
}

#[tokio::test]
async fn projects_list_shows_live_seat() {
    use crate::domain::models::AgentSeat;
    use crate::services::{create_project, touch_seat_presence_client, OccupantHint};

    let conn = open_in_memory().unwrap();
    let tokens = generate_bootstrap_tokens(&conn).unwrap();
    let admin = tokens
        .iter()
        .find(|t| t.name == "admin")
        .unwrap()
        .plaintext
        .clone();
    let p = create_project(&conn, "LiveSeat", "#111", "", "").unwrap();
    touch_seat_presence_client(
        &conn,
        &p.id,
        AgentSeat::Discuss,
        &OccupantHint {
            name: "cursor".into(),
            title: None,
            version: None,
        },
    )
    .unwrap();
    let pid = p.id.clone();
    let tmp = tempfile::tempdir().unwrap();
    let app = crate::http::app(AppState::new(conn, tmp.path().to_path_buf()));
    std::mem::forget(tmp);

    let (st, list) = json_req(app.clone(), "GET", "/v1/projects", Some(&admin), None).await;
    assert_eq!(st, StatusCode::OK, "{list}");
    let row = list
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == pid)
        .unwrap();
    assert_eq!(row["discuss_seated"], true);
    assert_eq!(row["build_seated"], false);
    assert_eq!(row["discuss_occupant"]["key"], "cursor");
    assert_eq!(row["discuss_occupant"]["label"], "Cursor");
    assert_eq!(row["discuss_occupant"]["known"], true);

    let (st, codes) = json_req(
        app,
        "GET",
        &format!("/v1/projects/{pid}/pair-codes"),
        Some(&admin),
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{codes}");
    assert_eq!(codes["discuss"]["seated"], true);
    assert_eq!(codes["build"]["seated"], false);
}
