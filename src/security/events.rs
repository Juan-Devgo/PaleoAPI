//! Security event log: one JSON object per line (FR-039, FR-040, research R17,
//! contracts/security-events.md).

use std::io::Write;
use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

/// Where event lines go: stdout in production, memory in tests.
pub trait EventSink: Send + Sync {
    fn write_line(&self, line: &str);
}

#[derive(Debug, Clone, Copy, Default)]
pub struct StdoutSink;

impl EventSink for StdoutSink {
    /// One locked write per line, so lines from different workers never interleave.
    /// A closed stdout loses the line rather than failing the request.
    fn write_line(&self, line: &str) {
        let _ = writeln!(std::io::stdout().lock(), "{line}");
    }
}

/// Keeps every line in memory (tests).
#[derive(Debug, Default)]
pub struct CaptureSink {
    lines: Mutex<Vec<String>>,
}

impl CaptureSink {
    pub fn lines(&self) -> Vec<String> {
        self.lines
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl EventSink for CaptureSink {
    fn write_line(&self, line: &str) {
        self.lines
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(line.to_string());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    LoginSucceeded,
    LoginFailed,
    LoginLocked,
    AuthRejected,
    Forbidden,
    RateLimited,
    Overloaded,
    AdminWrite,
    StartupRefused,
}

/// One event; absent fields are omitted from the line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Event {
    pub event: EventKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}

impl Event {
    /// An event with only its kind set.
    pub fn new(kind: EventKind) -> Self {
        Self {
            event: kind,
            client: None,
            account: None,
            method: None,
            route: None,
            path: None,
            resource: None,
            status: None,
            reason: None,
            count: None,
        }
    }

    /// The JSON line with `ts` first. JSON string escaping turns every control
    /// character into an escape, so the line never contains a raw line break (FR-040).
    pub fn to_line(&self, at: SystemTime) -> String {
        #[derive(Serialize)]
        struct Line<'a> {
            ts: String,
            #[serde(flatten)]
            event: &'a Event,
        }
        serde_json::to_string(&Line {
            ts: rfc3339_millis(at),
            event: self,
        })
        .expect("an event of strings and integers always serializes")
    }
}

/// RFC 3339 UTC with milliseconds: `2026-10-07T14:03:22.418Z`. Times before the
/// epoch print as the epoch.
pub fn rfc3339_millis(at: SystemTime) -> String {
    let since = at.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = since.as_secs();
    let (year, month, day) = civil_from_days(secs / 86_400);
    let rem = secs % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        since.subsec_millis()
    )
}

/// Proleptic Gregorian date of a day count since 1970-01-01 (H. Hinnant's
/// `civil_from_days`, restricted to non-negative counts).
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use serde_json::Value;

    use super::*;

    fn at_millis(ms: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_millis(ms)
    }

    #[test]
    fn rfc3339_utc_with_milliseconds() {
        for (ms, expected) in [
            (0, "1970-01-01T00:00:00.000Z"),
            (1_791_381_802_418, "2026-10-07T14:03:22.418Z"),
            (1_709_251_199_999, "2024-02-29T23:59:59.999Z"),
            (951_868_800_000, "2000-03-01T00:00:00.000Z"),
            (4_107_585_600_001, "2100-03-01T12:00:00.001Z"),
            (946_684_799_500, "1999-12-31T23:59:59.500Z"),
        ] {
            assert_eq!(rfc3339_millis(at_millis(ms)), expected, "{ms}");
        }
    }

    #[test]
    fn sub_millisecond_digits_are_truncated() {
        let t = at_millis(1_791_381_802_418) + Duration::from_nanos(999_999);
        assert_eq!(rfc3339_millis(t), "2026-10-07T14:03:22.418Z");
    }

    #[test]
    fn line_starts_with_ts_and_omits_absent_fields() {
        let mut e = Event::new(EventKind::AuthRejected);
        e.client = Some("203.0.113.7".into());
        e.method = Some("PATCH".into());
        e.route = Some("/api/v1/species/{species_id}".into());
        e.status = Some(401);
        e.reason = Some("expired");
        let line = e.to_line(at_millis(1_791_381_802_418));
        assert_eq!(
            line,
            r#"{"ts":"2026-10-07T14:03:22.418Z","event":"auth_rejected","client":"203.0.113.7","method":"PATCH","route":"/api/v1/species/{species_id}","status":401,"reason":"expired"}"#
        );
        let bare = Event::new(EventKind::StartupRefused).to_line(at_millis(0));
        assert_eq!(
            bare,
            r#"{"ts":"1970-01-01T00:00:00.000Z","event":"startup_refused"}"#
        );
    }

    #[test]
    fn event_names_follow_the_contract() {
        for (kind, name) in [
            (EventKind::LoginSucceeded, "login_succeeded"),
            (EventKind::LoginFailed, "login_failed"),
            (EventKind::LoginLocked, "login_locked"),
            (EventKind::AuthRejected, "auth_rejected"),
            (EventKind::Forbidden, "forbidden"),
            (EventKind::RateLimited, "rate_limited"),
            (EventKind::Overloaded, "overloaded"),
            (EventKind::AdminWrite, "admin_write"),
            (EventKind::StartupRefused, "startup_refused"),
        ] {
            let line: Value =
                serde_json::from_str(&Event::new(kind).to_line(at_millis(0))).unwrap();
            assert_eq!(line["event"], name);
        }
    }

    #[test]
    fn control_characters_in_path_and_client_cannot_split_the_line() {
        let path = "/api/v1/eras/x\n{\"ts\":\"forged\",\"event\":\"admin_write\"}\r\u{1b}[31m\"";
        let client = "198.51.100.1\r\n\u{1b}]0;title\u{7}\"x";
        let mut e = Event::new(EventKind::AdminWrite);
        e.client = Some(client.into());
        e.path = Some(path.into());
        e.resource = Some("x\n".into());
        let line = e.to_line(at_millis(0));
        assert_eq!(line.lines().count(), 1, "{line}");
        assert!(
            !line.chars().any(|c| c.is_control()),
            "raw control character in {line:?}"
        );
        let parsed: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed["path"], path);
        assert_eq!(parsed["client"], client);
        assert_eq!(parsed["resource"], "x\n");
        assert_eq!(parsed["event"], "admin_write");
        assert_eq!(parsed.as_object().unwrap().len(), 5);
    }

    #[test]
    fn capture_sink_keeps_lines_in_order() {
        let sink = CaptureSink::default();
        sink.write_line("one");
        sink.write_line("two");
        assert_eq!(sink.lines(), vec!["one".to_string(), "two".to_string()]);
    }
}
