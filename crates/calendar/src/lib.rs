//! # Calendar
//!
//! Events and contacts per user. [`Calendar`] holds the tables, [`router`] serves:
//!
//! * `GET /calendar` page of upcoming events (html, gemtext or text)
//! * `GET /calendar/export.ics` iCalendar feed of the caller's events
//! * `/calendar/events/{list,create,read,update,delete}` and `/calendar/contacts/...` json crud
//!   (per-user, see [`user::owned_crud`])

use database::{Database, DbError, Record, Table, Uuid, Value};
use element::{Page, heading, table, text};
use router::{Request, Response, Router};
use user::{Owned, Users, owned_crud};
use utils::{unix_now, unix_to_ical};

/// Personal contact information.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct Contact {
    pub id: Uuid,
    pub owner: Uuid,
    pub name: String,
    #[record(default)]
    pub email: String,
    #[record(default)]
    pub phone: String,
    #[record(default)]
    pub address: String,
    pub notes: Vec<String>,
    #[record(default)]
    pub created_at: u64,
    #[record(default)]
    pub updated_at: u64,
}

/// A calendar event. `people` are free-form names, emails, or contact ids.
#[derive(Clone, Debug, PartialEq, Record)]
pub struct Event {
    pub id: Uuid,
    pub owner: Uuid,
    pub title: String,
    #[record(default)]
    pub description: String,
    /// Unix seconds (UTC).
    pub start_at: u64,
    pub end_at: u64,
    #[record(default)]
    pub location: String,
    pub people: Vec<String>,
    #[record(default)]
    pub created_at: u64,
    #[record(default)]
    pub updated_at: u64,
}

impl Owned for Contact {
    fn owner(&self) -> Uuid {
        self.owner
    }
}
impl Owned for Event {
    fn owner(&self) -> Uuid {
        self.owner
    }
}

#[derive(Clone)]
pub struct Calendar {
    pub events: Table<Event>,
    pub contacts: Table<Contact>,
}

impl Calendar {
    pub fn new(db: Database) -> Result<Calendar, DbError> {
        Ok(Calendar { events: Table::new(db.clone())?, contacts: Table::new(db)? })
    }

    /// Add an event programmatically. Validates the time range.
    pub fn add_event(&self, owner: Uuid, title: &str, start_at: u64, end_at: u64) -> Result<Event, String> {
        if title.trim().is_empty() {
            return Err("title is required".into());
        }
        if end_at < start_at {
            return Err("an event cannot end before it starts".into());
        }
        let now = unix_now();
        let e = Event { id: Uuid::new_v4(), owner, title: title.into(), description: String::new(), start_at, end_at, location: String::new(), people: vec![], created_at: now, updated_at: now };
        self.events.insert(&e).map_err(|e| e.to_string())?;
        Ok(e)
    }

    /// Events of `owner` overlapping `[from, to)`, soonest first.
    pub fn between(&self, owner: &Uuid, from: u64, to: u64) -> Vec<Event> {
        let mut v = self.events.find(|e| e.owner == *owner && e.end_at >= from && e.start_at < to).unwrap_or_default();
        v.sort_by(|a, b| a.start_at.cmp(&b.start_at).then(a.id.cmp(&b.id)));
        v
    }
}

fn ical_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace(';', "\\;").replace(',', "\\,").replace('\n', "\\n").replace('\r', "")
}

/// RFC 5545 feed (lines folded at 75 octets).
pub fn to_ical(events: &[Event]) -> String {
    fn fold(line: &str, out: &mut String) {
        let mut count = 0;
        for c in line.chars() {
            let l = c.len_utf8();
            if count + l > 75 {
                out.push_str("\r\n ");
                count = 1;
            }
            out.push(c);
            count += l;
        }
        out.push_str("\r\n");
    }
    let mut out = String::new();
    out.push_str("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//tunic//calendar//EN\r\n");
    for e in events {
        out.push_str("BEGIN:VEVENT\r\n");
        fold(&format!("UID:{}@tunic", e.id), &mut out);
        fold(&format!("DTSTAMP:{}", unix_to_ical(e.updated_at.max(e.created_at))), &mut out);
        fold(&format!("DTSTART:{}", unix_to_ical(e.start_at)), &mut out);
        fold(&format!("DTEND:{}", unix_to_ical(e.end_at)), &mut out);
        fold(&format!("SUMMARY:{}", ical_escape(&e.title)), &mut out);
        if !e.description.is_empty() {
            fold(&format!("DESCRIPTION:{}", ical_escape(&e.description)), &mut out);
        }
        if !e.location.is_empty() {
            fold(&format!("LOCATION:{}", ical_escape(&e.location)), &mut out);
        }
        out.push_str("END:VEVENT\r\n");
    }
    out.push_str("END:VCALENDAR\r\n");
    out
}

pub fn router(cal: Calendar, users: Users) -> Router {
    let (c, u) = (cal.clone(), users.clone());
    let page = move |req: &Request| {
        let Some(me) = u.from_request(req) else { return Response::unauthorized() };
        let now = unix_now();
        let upcoming = c.between(&me.id, now, now + 30 * 86400);
        let rows: Vec<Vec<String>> = upcoming
            .iter()
            .map(|e| vec![utils::unix_to_ical(e.start_at), e.title.clone(), e.location.clone(), e.people.join(", ")])
            .collect();
        let p = Page::new("Calendar").add(heading(1, "Calendar")).add(text(format!("{} event(s) in the next 30 days", rows.len()))).add(table(["Start (UTC)", "Title", "Location", "People"], rows));
        Response::page(req, &p)
    };

    let (c, u) = (cal.clone(), users.clone());
    let ics = move |req: &Request| {
        let Some(me) = u.from_request(req) else { return Response::unauthorized() };
        let mut all = c.events.find(|e| e.owner == me.id).unwrap_or_default();
        all.sort_by_key(|e| e.start_at);
        Response::bytes("text/calendar; charset=utf-8", to_ical(&all).into_bytes()).header("Content-Disposition", "attachment; filename=\"calendar.ics\"")
    };

    let (c, u) = (cal.clone(), users.clone());
    let range = move |req: &Request| {
        let Some(me) = u.from_request(req) else { return Response::unauthorized() };
        let from = req.query_param("from").and_then(|s| s.parse().ok()).unwrap_or(0u64);
        let to = req.query_param("to").and_then(|s| s.parse().ok()).unwrap_or(u64::MAX);
        let list: Vec<Value> = c.between(&me.id, from, to).iter().map(|e| e.public_value()).collect();
        Response::json(&utils::json_obj! {"events" => list})
    };

    Router::new()
        .get("/calendar", page)
        .get("/calendar/export.ics", ics)
        .get("/calendar/range", range)
        .nest("/calendar/events", owned_crud(users.clone(), cal.events))
        .nest("/calendar/contacts", owned_crud(users, cal.contacts))
        .tagged("public")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (Calendar, Users, Router, String, Uuid) {
        let db = Database::memory();
        let users = Users::with_config(db.scope("user").unwrap(), user::UsersConfig { password_iterations: 50, ..Default::default() }).unwrap();
        let me = users.create_user("alice", "alice password", false).unwrap();
        let cookie = format!("session_id={}", users.login("alice", "alice password").unwrap().1.id);
        let cal = Calendar::new(db.scope("calendar").unwrap()).unwrap();
        let r = router(cal.clone(), users.clone());
        (cal, users, r, cookie, me.id)
    }

    #[test]
    fn events_validate_and_query() {
        let (cal, _, _, _, me) = setup();
        assert!(cal.add_event(me, " ", 1, 2).is_err());
        assert!(cal.add_event(me, "x", 5, 2).is_err());
        cal.add_event(me, "late", 5000, 6000).unwrap();
        cal.add_event(me, "early", 1000, 2000).unwrap();
        cal.add_event(Uuid::new_v4(), "someone else", 1000, 2000).unwrap();
        let t: Vec<_> = cal.between(&me, 0, 10_000).into_iter().map(|e| e.title).collect();
        assert_eq!(t, vec!["early", "late"]);
        assert_eq!(cal.between(&me, 2500, 4000).len(), 0);
        assert_eq!(cal.between(&me, 1500, 1600).len(), 1, "overlap counts");
    }

    #[test]
    fn crud_ics_and_page_over_http() {
        let (_, _, r, cookie, _) = setup();
        let post = |p: &str, b: &str| r.handle(Request::post(p, b).with_header("cookie", &cookie));
        let created = post("/calendar/events/create", r#"{"title":"Dentist, 2nd; visit","start_at":1700000000,"end_at":1700003600,"people":["Bob"],"location":"Main St"}"#);
        assert_eq!(created.status, 201, "{}", created.body_str());
        assert_eq!(post("/calendar/events/create", r#"{"start_at":1}"#).status, 400);
        let ics = r.handle(Request::get("/calendar/export.ics").with_header("cookie", &cookie));
        let body = ics.body_str();
        assert!(body.starts_with("BEGIN:VCALENDAR\r\n") && body.ends_with("END:VCALENDAR\r\n"));
        assert!(body.contains("DTSTART:20231114T221320Z"), "{body}");
        assert!(body.contains("SUMMARY:Dentist\\, 2nd\\; visit"), "{body}");
        assert!(body.contains("LOCATION:Main St"));
        let range = r.handle(Request::get("/calendar/range?from=1700000000&to=1700001000").with_header("cookie", &cookie)).json_body().unwrap();
        assert_eq!(range.get("events").unwrap().as_array().unwrap().len(), 1);
        assert_eq!(r.handle(Request::get("/calendar/export.ics")).status, 401);
        assert_eq!(post("/calendar/contacts/create", r#"{"name":"Bob","email":"bob@example.com","notes":["met at conf"]}"#).status, 201);
        assert!(r.handle(Request::get("/calendar").with_header("cookie", &cookie)).body_str().contains("Calendar"));
    }

    #[test]
    fn long_ical_lines_are_folded() {
        let e = Event { id: Uuid::new_v4(), owner: Uuid::new_v4(), title: "x".repeat(200), description: String::new(), start_at: 0, end_at: 1, location: String::new(), people: vec![], created_at: 0, updated_at: 0 };
        for line in to_ical(&[e]).split("\r\n") {
            assert!(line.len() <= 76, "{}", line.len());
        }
    }
}
