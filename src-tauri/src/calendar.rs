//! The Mac's calendar, read through EventKit once the person turns it on and
//! allows it. A recording takes its meeting's title, invitees and end time
//! from the event happening now, and earlier meetings in the same series give
//! the "Last time" brief.

use serde::Serialize;
use tauri::AppHandle;
use tidy_core::store::recordings::MeetingEvent;

use crate::error::AppResult;

const ENABLED: &str = "calendar.enabled";
/// An event starting this soon counts as the one a recording is for.
const SOON_MS: i64 = 10 * 60_000;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    Granted,
    Denied,
    NotDetermined,
}

#[derive(Debug, Serialize)]
pub struct CalendarStatus {
    pub enabled: bool,
    pub access: Access,
}

/// The event a recording starting now belongs to: one under way or starting
/// within ten minutes. Events with other people beat personal blocks, then
/// the one closest to its start wins, so a long afternoon block does not beat
/// the call happening now.
pub fn current(events: &[MeetingEvent], now: i64) -> Option<MeetingEvent> {
    events
        .iter()
        .filter(|e| e.start_ms - SOON_MS <= now && now < e.end_ms)
        .min_by_key(|e| (e.attendees.is_empty(), (e.start_ms - now).abs()))
        .cloned()
}

#[cfg(target_os = "macos")]
mod eventkit {
    use std::sync::{mpsc, Mutex};
    use std::time::Duration;

    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_event_kit::{EKAuthorizationStatus, EKEntityType, EKEvent, EKEventStatus, EKEventStore};
    use objc2_foundation::{NSDate, NSError};
    use tidy_core::store::recordings::MeetingEvent;

    use super::Access;

    pub fn access() -> Access {
        // SAFETY: a class method with no side effects.
        let status = unsafe { EKEventStore::authorizationStatusForEntityType(EKEntityType::Event) };
        match status {
            EKAuthorizationStatus::FullAccess => Access::Granted,
            EKAuthorizationStatus::NotDetermined => Access::NotDetermined,
            _ => Access::Denied,
        }
    }

    /// Ask macOS for calendar access, waiting for the person to answer.
    pub fn request() -> Access {
        // SAFETY: plain object construction.
        let store = unsafe { EKEventStore::new() };
        let (tx, rx) = mpsc::channel::<bool>();
        let tx = Mutex::new(Some(tx));
        let done = RcBlock::new(move |granted: Bool, _error: *mut NSError| {
            if let Some(tx) = tx.lock().ok().and_then(|mut t| t.take()) {
                let _ = tx.send(granted.as_bool());
            }
        });
        // SAFETY: the block outlives the call; EventKit copies it.
        unsafe { store.requestFullAccessToEventsWithCompletion(RcBlock::as_ptr(&done)) };
        match rx.recv_timeout(Duration::from_secs(300)) {
            Ok(true) => Access::Granted,
            Ok(false) => Access::Denied,
            Err(_) => access(),
        }
    }

    fn convert(event: &EKEvent) -> Option<MeetingEvent> {
        // SAFETY: property reads on an event EventKit returned.
        unsafe {
            if event.isAllDay() || event.status() == EKEventStatus::Canceled {
                return None;
            }
            let ms = |date: &NSDate| (date.timeIntervalSince1970() * 1000.0) as i64;
            let attendees = event
                .attendees()
                .map(|people| {
                    people
                        .iter()
                        .filter(|p| !p.isCurrentUser())
                        .filter_map(|p| {
                            p.name().map(|n| n.to_string()).filter(|n| !n.trim().is_empty()).or_else(|| {
                                p.URL()
                                    .absoluteString()
                                    .map(|u| u.to_string().trim_start_matches("mailto:").to_string())
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            let series = event
                .hasRecurrenceRules()
                .then(|| event.calendarItemExternalIdentifier().map(|s| s.to_string()))
                .flatten();
            Some(MeetingEvent {
                title: event.title().to_string(),
                start_ms: ms(&event.startDate()),
                end_ms: ms(&event.endDate()),
                attendees,
                series,
            })
        }
    }

    /// Timed events overlapping the window, earliest first.
    pub fn events(from_ms: i64, to_ms: i64) -> Vec<MeetingEvent> {
        let start = NSDate::dateWithTimeIntervalSince1970(from_ms as f64 / 1000.0);
        let end = NSDate::dateWithTimeIntervalSince1970(to_ms as f64 / 1000.0);
        // SAFETY: EventKit calls with valid dates; nil means every calendar.
        let found = unsafe {
            let store = EKEventStore::new();
            let predicate = store.predicateForEventsWithStartDate_endDate_calendars(&start, &end, None);
            store.eventsMatchingPredicate(&predicate)
        };
        let mut events: Vec<MeetingEvent> = found.iter().filter_map(|e| convert(&e)).collect();
        events.sort_by_key(|e| e.start_ms);
        events
    }
}

#[cfg(not(target_os = "macos"))]
mod eventkit {
    use super::Access;
    use tidy_core::store::recordings::MeetingEvent;
    pub fn access() -> Access {
        Access::Denied
    }
    pub fn request() -> Access {
        Access::Denied
    }
    pub fn events(_from: i64, _to: i64) -> Vec<MeetingEvent> {
        Vec::new()
    }
}

fn enabled(app: &AppHandle) -> bool {
    use tauri::Manager;
    let db = app.state::<crate::db::Db>();
    let Ok(conn) = db.lock() else {
        return false;
    };
    crate::db::get_setting(&conn, ENABLED).ok().flatten().as_deref() == Some("true")
}

/// Events today and this evening, when the calendar is on and allowed.
fn upcoming(app: &AppHandle) -> Vec<MeetingEvent> {
    if !enabled(app) || eventkit::access() != Access::Granted {
        return Vec::new();
    }
    let now = crate::db::now_ms();
    eventkit::events(now - 4 * 3_600_000, now + 12 * 3_600_000)
}

/// The event a recording starting now is for, if the calendar is on.
pub fn event_now(app: &AppHandle) -> Option<MeetingEvent> {
    current(&upcoming(app), crate::db::now_ms())
}

#[tauri::command]
pub async fn calendar_status(app: AppHandle) -> CalendarStatus {
    CalendarStatus { enabled: enabled(&app), access: eventkit::access() }
}

/// Turn the calendar on, asking macOS for access the first time.
#[tauri::command]
pub async fn calendar_connect(app: AppHandle) -> AppResult<CalendarStatus> {
    let access = tauri::async_runtime::spawn_blocking(|| match eventkit::access() {
        Access::NotDetermined => eventkit::request(),
        other => other,
    })
    .await
    .map_err(|e| crate::error::AppError::Other(e.to_string()))?;
    let on = access == Access::Granted;
    crate::local_ai::db(&app, move |c| crate::db::set_setting(c, ENABLED, if on { "true" } else { "false" })).await?;
    Ok(CalendarStatus { enabled: on, access })
}

#[tauri::command]
pub async fn calendar_disconnect(app: AppHandle) -> AppResult<()> {
    crate::local_ai::db(&app, |c| crate::db::set_setting(c, ENABLED, "false")).await
}

/// Meetings from earlier today until this evening, earliest first.
#[tauri::command]
pub async fn calendar_events(app: AppHandle) -> AppResult<Vec<MeetingEvent>> {
    tauri::async_runtime::spawn_blocking(move || upcoming(&app))
        .await
        .map_err(|e| crate::error::AppError::Other(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(title: &str, start: i64, end: i64, people: usize) -> MeetingEvent {
        MeetingEvent {
            title: title.into(),
            start_ms: start,
            end_ms: end,
            attendees: (0..people).map(|i| format!("Person {i}")).collect(),
            series: None,
        }
    }

    #[test]
    fn the_meeting_happening_now_is_picked() {
        let minute = 60_000;
        let events = vec![
            event("Focus block", 0, 240 * minute, 0),
            event("Acme weekly", 60 * minute, 90 * minute, 3),
            event("Later", 200 * minute, 230 * minute, 2),
        ];
        assert_eq!(current(&events, 70 * minute).unwrap().title, "Acme weekly");
        // Five minutes early still counts.
        assert_eq!(current(&events, 55 * minute).unwrap().title, "Acme weekly");
        // A personal block is used only when nothing with people is on.
        assert_eq!(current(&events, 120 * minute).unwrap().title, "Focus block");
        assert!(current(&events, 300 * minute).is_none());
    }
}
