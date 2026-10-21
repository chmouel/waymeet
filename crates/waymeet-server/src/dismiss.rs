//! Locally dismissed meetings.
//!
//! A dismissed meeting is hidden from every output and skips notifications
//! until it ends. The calendar itself is never modified. Dismissals are
//! persisted as JSON so they survive daemon restarts.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use waymeet_core::MeetingView;

/// A single dismissed meeting instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DismissedMeeting {
    /// Provider name (e.g. "google:work").
    pub provider_name: String,
    /// Calendar identifier in that provider.
    pub calendar_id: String,
    /// Provider event identifier.
    pub event_id: String,
    /// Start of the dismissed instance; a rescheduled meeting is shown again.
    pub start: DateTime<Utc>,
    /// End of the meeting, after which the entry is discarded.
    pub end: DateTime<Utc>,
    /// Meeting title, for user-facing messages.
    pub title: String,
    /// When the meeting was dismissed.
    pub dismissed_at: DateTime<Utc>,
}

impl DismissedMeeting {
    fn from_meeting(meeting: &MeetingView, now: DateTime<Utc>) -> Self {
        Self {
            provider_name: meeting.provider_name.clone(),
            calendar_id: meeting.calendar_id.clone(),
            event_id: meeting.id.clone(),
            start: meeting.start_local.with_timezone(&Utc),
            end: meeting.end_local.with_timezone(&Utc),
            title: meeting.title.clone(),
            dismissed_at: now,
        }
    }

    /// Returns true if this entry refers to the given meeting instance.
    pub fn matches(
        &self,
        provider_name: &str,
        calendar_id: &str,
        event_id: &str,
        start: DateTime<Utc>,
    ) -> bool {
        self.event_id == event_id
            && self.start == start
            && self.calendar_id == calendar_id
            && self.provider_name == provider_name
    }

    fn matches_meeting(&self, meeting: &MeetingView) -> bool {
        self.matches(
            &meeting.provider_name,
            &meeting.calendar_id,
            &meeting.id,
            meeting.start_local.with_timezone(&Utc),
        )
    }
}

/// Dismissed meetings, ordered from oldest to most recent dismissal.
#[derive(Debug, Default)]
pub struct DismissStore {
    path: Option<PathBuf>,
    entries: Vec<DismissedMeeting>,
}

impl DismissStore {
    /// Creates a store that is never written to disk.
    pub fn in_memory() -> Self {
        Self::default()
    }

    /// Loads the store from `path`, starting empty if the file is missing or
    /// unreadable. Expired entries are dropped.
    pub fn load(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let entries = match fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_else(|e| {
                warn!(path = %path.display(), error = %e, "Ignoring unreadable dismissed meetings file");
                Vec::new()
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => {
                warn!(path = %path.display(), error = %e, "Failed to read dismissed meetings file");
                Vec::new()
            }
        };
        let mut store = Self {
            path: Some(path),
            entries,
        };
        if store.prune(Utc::now()) {
            store.save();
        }
        store
    }

    /// Returns the dismissed entries, oldest first.
    pub fn entries(&self) -> &[DismissedMeeting] {
        &self.entries
    }

    /// Returns true if the meeting has been dismissed.
    pub fn is_dismissed(&self, meeting: &MeetingView) -> bool {
        self.entries.iter().any(|e| e.matches_meeting(meeting))
    }

    /// Dismisses a meeting. Dismissing it again makes it the most recent entry.
    pub fn dismiss(&mut self, meeting: &MeetingView) {
        let now = Utc::now();
        self.prune(now);
        self.entries.retain(|e| !e.matches_meeting(meeting));
        self.entries
            .push(DismissedMeeting::from_meeting(meeting, now));
        self.save();
    }

    /// Restores the most recently dismissed meeting that has not yet ended.
    pub fn undo_last(&mut self) -> Option<DismissedMeeting> {
        self.prune(Utc::now());
        let restored = self.entries.pop();
        self.save();
        restored
    }

    /// Restores every dismissed meeting that has not yet ended.
    pub fn clear(&mut self) -> Vec<DismissedMeeting> {
        self.prune(Utc::now());
        let restored = std::mem::take(&mut self.entries);
        self.save();
        restored
    }

    /// Drops entries for meetings that have ended. Returns true if any were removed.
    fn prune(&mut self, now: DateTime<Utc>) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.end > now);
        self.entries.len() != before
    }

    fn save(&self) {
        let Some(path) = &self.path else {
            return;
        };
        if let Err(e) = write_atomic(path, &self.entries) {
            warn!(path = %path.display(), error = %e, "Failed to save dismissed meetings");
        } else {
            debug!(path = %path.display(), count = self.entries.len(), "Saved dismissed meetings");
        }
    }
}

/// Writes the entries to a temporary file with owner-only permissions and
/// renames it into place.
fn write_atomic(path: &Path, entries: &[DismissedMeeting]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_vec_pretty(entries)?;
    let tmp = path.with_extension("json.tmp");

    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp)?;
    file.write_all(&json)?;
    file.sync_all()?;
    drop(file);

    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Local};

    fn meeting(id: &str, minutes_from_now: i64) -> MeetingView {
        let start = Local::now() + Duration::minutes(minutes_from_now);
        MeetingView {
            id: id.to_string(),
            provider_name: "google:work".to_string(),
            title: format!("Meeting {id}"),
            start_local: start,
            end_local: start + Duration::minutes(30),
            is_all_day: false,
            is_ongoing: false,
            primary_link: None,
            secondary_links: vec![],
            calendar_url: None,
            calendar_id: "primary".to_string(),
            user_response_status: waymeet_core::ResponseStatus::Unknown,
            other_attendee_count: 0,
            location: None,
            description: None,
            attendees: vec![],
            reminder_minutes: None,
        }
    }

    #[test]
    fn dismiss_matches_only_the_same_instance() {
        let mut store = DismissStore::in_memory();
        let m = meeting("a", 10);
        store.dismiss(&m);
        assert!(store.is_dismissed(&m));

        let mut rescheduled = m.clone();
        rescheduled.start_local += Duration::minutes(15);
        assert!(!store.is_dismissed(&rescheduled));

        let mut other_calendar = m.clone();
        other_calendar.calendar_id = "team".to_string();
        assert!(!store.is_dismissed(&other_calendar));
    }

    #[test]
    fn undo_last_restores_most_recent_and_redismiss_reorders() {
        let mut store = DismissStore::in_memory();
        let a = meeting("a", 10);
        let b = meeting("b", 20);
        store.dismiss(&a);
        store.dismiss(&b);
        store.dismiss(&a);
        assert_eq!(store.entries().len(), 2);

        assert_eq!(store.undo_last().unwrap().event_id, "a");
        assert!(!store.is_dismissed(&a));
        assert!(store.is_dismissed(&b));
        assert_eq!(store.undo_last().unwrap().event_id, "b");
        assert!(store.undo_last().is_none());
    }

    #[test]
    fn clear_restores_all() {
        let mut store = DismissStore::in_memory();
        store.dismiss(&meeting("a", 10));
        store.dismiss(&meeting("b", 20));
        assert_eq!(store.clear().len(), 2);
        assert!(store.entries().is_empty());
    }

    #[test]
    fn ended_meetings_are_pruned() {
        let mut store = DismissStore::in_memory();
        store.dismiss(&meeting("past", -60));
        store.dismiss(&meeting("future", 10));
        assert!(store.undo_last().is_some());
        // The ended entry was pruned rather than restored.
        assert!(store.undo_last().is_none());
    }

    #[test]
    fn persists_across_loads_with_private_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("dismissed-events.json");
        let m = meeting("a", 10);

        let mut store = DismissStore::load(&path);
        store.dismiss(&m);
        store.dismiss(&meeting("past", -60));

        let reloaded = DismissStore::load(&path);
        assert!(reloaded.is_dismissed(&m));
        assert_eq!(reloaded.entries().len(), 1);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn corrupt_file_starts_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dismissed-events.json");
        fs::write(&path, "not json").unwrap();
        assert!(DismissStore::load(&path).entries().is_empty());
    }
}
