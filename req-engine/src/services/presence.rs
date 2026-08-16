//! Live seat occupancy: MCP `--pair` process heartbeats into SQLite.
//! Desktop/UI treats a seat as occupied if last_seen is within TTL.

use chrono::{DateTime, Duration, Utc};
use rusqlite::OptionalExtension;
use rusqlite::Connection;

use crate::domain::models::AgentSeat;
use crate::services::client_host::{recognize, OccupantFace};

/// A missed-heartbeat window after which the UI shows the seat empty.
pub const SEAT_TTL: Duration = Duration::seconds(15);

#[derive(Debug, Clone, Default)]
pub struct OccupantHint {
    pub name: String,
    pub title: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SeatLive {
    pub seated: bool,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub pid: Option<i64>,
    pub client_name: Option<String>,
    pub client_title: Option<String>,
    pub face: Option<OccupantFace>,
}

#[derive(Debug, Clone, Default)]
pub struct ProjectPresence {
    pub discuss: SeatLive,
    pub build: SeatLive,
}

pub fn current_pid() -> i64 {
    std::process::id() as i64
}

pub fn touch_seat_presence(
    conn: &Connection,
    project_id: &str,
    seat: AgentSeat,
) -> Result<SeatLive, rusqlite::Error> {
    touch_seat_presence_at(conn, project_id, seat, Utc::now(), current_pid(), None)
}

pub fn touch_seat_presence_client(
    conn: &Connection,
    project_id: &str,
    seat: AgentSeat,
    hint: &OccupantHint,
) -> Result<SeatLive, rusqlite::Error> {
    touch_seat_presence_at(
        conn,
        project_id,
        seat,
        Utc::now(),
        current_pid(),
        Some(hint),
    )
}

pub fn touch_seat_presence_at(
    conn: &Connection,
    project_id: &str,
    seat: AgentSeat,
    now: DateTime<Utc>,
    pid: i64,
    hint: Option<&OccupantHint>,
) -> Result<SeatLive, rusqlite::Error> {
    let now_s = now.to_rfc3339();
    let seat_s = seat.as_str();
    let existing: Option<String> = conn
        .query_row(
            "SELECT started_at FROM seat_presence WHERE project_id = ?1 AND seat = ?2",
            rusqlite::params![project_id, seat_s],
            |r| r.get(0),
        )
        .optional()?;
    let started = existing.unwrap_or_else(|| now_s.clone());
    let name = hint.map(|h| h.name.as_str());
    let title = hint.and_then(|h| h.title.as_deref());
    let version = hint.and_then(|h| h.version.as_deref());
    conn.execute(
        "INSERT INTO seat_presence (project_id, seat, last_seen_at, started_at, pid, client_name, client_title, client_version)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(project_id, seat) DO UPDATE SET
           last_seen_at = excluded.last_seen_at,
           pid = excluded.pid,
           client_name = COALESCE(excluded.client_name, seat_presence.client_name),
           client_title = COALESCE(excluded.client_title, seat_presence.client_title),
           client_version = COALESCE(excluded.client_version, seat_presence.client_version)",
        rusqlite::params![project_id, seat_s, now_s, started, pid, name, title, version],
    )?;
    Ok(live_from(true, Some(now), Some(pid), name.map(str::to_string), title.map(str::to_string)))
}

/// Clear this process's row. A newer occupant (different pid) is left alone.
pub fn clear_seat_presence(
    conn: &Connection,
    project_id: &str,
    seat: AgentSeat,
) -> Result<(), rusqlite::Error> {
    clear_seat_presence_pid(conn, project_id, seat, current_pid())
}

pub fn clear_seat_presence_pid(
    conn: &Connection,
    project_id: &str,
    seat: AgentSeat,
    pid: i64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "DELETE FROM seat_presence WHERE project_id = ?1 AND seat = ?2 AND pid = ?3",
        rusqlite::params![project_id, seat.as_str(), pid],
    )?;
    Ok(())
}

pub fn project_presence(conn: &Connection, project_id: &str) -> Result<ProjectPresence, rusqlite::Error> {
    project_presence_at(conn, project_id, Utc::now())
}

pub fn project_presence_at(
    conn: &Connection,
    project_id: &str,
    now: DateTime<Utc>,
) -> Result<ProjectPresence, rusqlite::Error> {
    let mut out = ProjectPresence::default();
    let mut stmt = conn.prepare(
        "SELECT seat, last_seen_at, pid, client_name, client_title FROM seat_presence WHERE project_id = ?1",
    )?;
    let rows = stmt.query_map(rusqlite::params![project_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, Option<String>>(4)?,
        ))
    })?;
    for row in rows {
        let (seat_s, last_s, pid, client_name, client_title) = row?;
        let last = DateTime::parse_from_rfc3339(&last_s)
            .ok()
            .map(|d| d.with_timezone(&Utc));
        let seated = last
            .map(|t| now.signed_duration_since(t) <= SEAT_TTL)
            .unwrap_or(false);
        let live = live_from(seated, last, Some(pid), client_name, client_title);
        match AgentSeat::parse(&seat_s) {
            Some(AgentSeat::Discuss) => out.discuss = live,
            Some(AgentSeat::Build) => out.build = live,
            None => {}
        }
    }
    Ok(out)
}

fn live_from(
    seated: bool,
    last_seen_at: Option<DateTime<Utc>>,
    pid: Option<i64>,
    client_name: Option<String>,
    client_title: Option<String>,
) -> SeatLive {
    let face = client_name.as_deref().map(|n| recognize(n, client_title.as_deref()));
    SeatLive {
        seated,
        last_seen_at,
        pid,
        client_name,
        client_title,
        face,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;
    use crate::services::create_project;

    #[test]
    fn touch_then_seated_until_ttl() {
        let conn = open_in_memory().unwrap();
        let p = create_project(&conn, "P", "#111", "", "").unwrap();
        let t0 = Utc::now();
        touch_seat_presence_at(&conn, &p.id, AgentSeat::Discuss, t0, 42, None).unwrap();
        let live = project_presence_at(&conn, &p.id, t0).unwrap();
        assert!(live.discuss.seated);
        assert!(!live.build.seated);

        let stale = project_presence_at(&conn, &p.id, t0 + SEAT_TTL + Duration::seconds(1)).unwrap();
        assert!(!stale.discuss.seated);

        clear_seat_presence_pid(&conn, &p.id, AgentSeat::Discuss, 99).unwrap();
        let still = project_presence_at(&conn, &p.id, t0).unwrap();
        assert!(still.discuss.seated, "other pid must not clear occupant");

        clear_seat_presence_pid(&conn, &p.id, AgentSeat::Discuss, 42).unwrap();
        let gone = project_presence_at(&conn, &p.id, t0).unwrap();
        assert!(!gone.discuss.seated);
    }

    #[test]
    fn client_name_survives_heartbeat_without_hint() {
        let conn = open_in_memory().unwrap();
        let p = create_project(&conn, "P", "#111", "", "").unwrap();
        let t0 = Utc::now();
        let hint = OccupantHint {
            name: "cursor".into(),
            title: None,
            version: Some("1".into()),
        };
        touch_seat_presence_at(&conn, &p.id, AgentSeat::Discuss, t0, 1, Some(&hint)).unwrap();
        touch_seat_presence_at(&conn, &p.id, AgentSeat::Discuss, t0, 1, None).unwrap();
        let live = project_presence_at(&conn, &p.id, t0).unwrap();
        assert_eq!(live.discuss.client_name.as_deref(), Some("cursor"));
        assert_eq!(live.discuss.face.as_ref().map(|f| f.key.as_str()), Some("cursor"));
    }
}
