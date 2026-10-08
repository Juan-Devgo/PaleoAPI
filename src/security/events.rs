//! Security event log: one JSON object per line (FR-039, FR-040, research R17,
//! contracts/security-events.md).

use std::sync::Mutex;
use std::time::SystemTime;

use serde::Serialize;

/// Where event lines go: stdout in production, memory in tests.
pub trait EventSink: Send + Sync {
    fn write_line(&self, line: &str);
}

#[derive(Debug, Clone, Copy, Default)]
pub struct StdoutSink;

impl EventSink for StdoutSink {
    fn write_line(&self, _line: &str) {
        todo!()
    }
}

/// Keeps every line in memory (tests).
#[derive(Debug, Default)]
pub struct CaptureSink {
    lines: Mutex<Vec<String>>,
}

impl CaptureSink {
    pub fn lines(&self) -> Vec<String> {
        let _ = &self.lines;
        todo!()
    }
}

impl EventSink for CaptureSink {
    fn write_line(&self, _line: &str) {
        todo!()
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
    pub fn new(_kind: EventKind) -> Self {
        todo!()
    }

    /// The JSON line with `ts` first; never contains a raw line break.
    pub fn to_line(&self, _at: SystemTime) -> String {
        todo!()
    }
}

/// RFC 3339 UTC with milliseconds: `2026-10-07T14:03:22.418Z`.
pub fn rfc3339_millis(_at: SystemTime) -> String {
    todo!()
}
