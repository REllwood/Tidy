//! Watches around a recording: offers to record when a call starts, and stops
//! a recording that has clearly finished, when the call ends, after minutes of
//! silence, or a while after the calendar event ends. Every automatic stop is
//! shown first with a 15-second countdown and a "Keep recording" button, in a
//! small window that floats above the meeting app without taking focus.
//!
//! The decisions are a pure state machine (`Monitor::step`) fed once a second
//! by a background thread, so they can be tested without a real call.

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

/// How long a call app must hold the microphone before Tidy offers to record.
pub const OFFER_AFTER_MS: i64 = 3_000;
/// How long the call app must have let go of the microphone before the call
/// counts as over.
pub const CALL_GONE_MS: i64 = 20_000;
/// How long a recording can be silent before Tidy offers to stop it.
pub const QUIET_MS: i64 = 180_000;
/// How long after a calendar event's end a recording may run on.
pub const EVENT_GRACE_MS: i64 = 300_000;
/// How long the countdown runs before an automatic stop.
pub const COUNTDOWN_MS: i64 = 15_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StopKind {
    CallEnded,
    Silence,
    EventEnded,
}

#[derive(Debug, Clone, PartialEq)]
struct PendingStop {
    kind: StopKind,
    reason: String,
    since_ms: i64,
    deadline_ms: i64,
}

/// What the floating window shows.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Hud {
    #[default]
    None,
    Offer { app: String },
    Countdown { reason: String, deadline_ms: i64 },
}

/// The calendar event a recording belongs to, when it is known.
#[derive(Debug, Clone, PartialEq)]
pub struct EventEnd {
    pub title: String,
    pub end_ms: i64,
}

/// One look at the world, taken every second.
#[derive(Debug, Clone, Default)]
pub struct Observation {
    pub now_ms: i64,
    /// Call apps holding the microphone, most likely first.
    pub call_apps: Vec<String>,
    pub recording: bool,
    /// When the recording last heard someone, on either side.
    pub last_voice_ms: i64,
    pub offers: bool,
    pub auto_stop: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Hud(Hud),
    Stop { reason: String },
}

#[derive(Debug, Default)]
pub struct Monitor {
    call_since: Option<(String, i64)>,
    offered: Option<String>,
    dismissed: bool,
    recording: bool,
    call_app: Option<String>,
    call_gone_since: Option<i64>,
    quiet_from: i64,
    kept: HashSet<StopKind>,
    pending: Option<PendingStop>,
    event: Option<EventEnd>,
}

impl Monitor {
    pub fn hud(&self) -> Hud {
        if let Some(p) = &self.pending {
            return Hud::Countdown { reason: p.reason.clone(), deadline_ms: p.deadline_ms };
        }
        match (&self.offered, self.recording, self.dismissed) {
            (Some(app), false, false) => Hud::Offer { app: app.clone() },
            _ => Hud::None,
        }
    }

    /// The calendar event the current recording belongs to.
    pub fn set_event(&mut self, event: Option<EventEnd>) {
        self.event = event;
    }

    fn propose(&mut self, kind: StopKind, reason: String, now: i64) {
        if self.pending.is_none() && !self.kept.contains(&kind) {
            self.pending = Some(PendingStop { kind, reason, since_ms: now, deadline_ms: now + COUNTDOWN_MS });
        }
    }

    pub fn step(&mut self, o: &Observation) -> Vec<Action> {
        let before = self.hud();
        let mut actions = Vec::new();
        let now = o.now_ms;

        if o.recording && !self.recording {
            self.recording = true;
            self.call_app = o.call_apps.first().cloned().or_else(|| self.offered.clone());
            self.call_gone_since = None;
            self.quiet_from = now;
            self.kept.clear();
            self.pending = None;
        } else if !o.recording && self.recording {
            self.recording = false;
            self.call_app = None;
            self.call_gone_since = None;
            self.pending = None;
            self.kept.clear();
            self.event = None;
            // Stopped by hand during a call: don't offer that call again.
            self.offered = o.call_apps.first().cloned();
            self.dismissed = self.offered.is_some();
        }

        if !self.recording {
            match o.call_apps.first() {
                Some(app) => {
                    let since = match &self.call_since {
                        Some((current, at)) if current == app => *at,
                        _ => {
                            self.call_since = Some((app.clone(), now));
                            now
                        }
                    };
                    if o.offers && self.offered.is_none() && now - since >= OFFER_AFTER_MS {
                        self.offered = Some(app.clone());
                        self.dismissed = false;
                    }
                }
                None => {
                    self.call_since = None;
                    self.offered = None;
                    self.dismissed = false;
                }
            }
        } else if o.auto_stop {
            if self.call_app.is_none() {
                self.call_app = o.call_apps.first().cloned();
            }
            if let Some(app) = self.call_app.clone() {
                if o.call_apps.is_empty() {
                    let gone = *self.call_gone_since.get_or_insert(now);
                    if now - gone >= CALL_GONE_MS {
                        self.propose(StopKind::CallEnded, format!("Stopped automatically when {app} ended the call"), now);
                    }
                } else {
                    self.call_gone_since = None;
                    if self.pending.as_ref().is_some_and(|p| p.kind == StopKind::CallEnded) {
                        self.pending = None; // the call came back
                    }
                }
            }
            let quiet_base = self.quiet_from.max(o.last_voice_ms);
            if now - quiet_base >= QUIET_MS {
                self.propose(StopKind::Silence, "Stopped automatically after 3 minutes of silence".into(), now);
            } else if self
                .pending
                .as_ref()
                .is_some_and(|p| p.kind == StopKind::Silence && o.last_voice_ms > p.since_ms)
            {
                self.pending = None; // someone spoke
            }
            if let Some(event) = self.event.clone() {
                if now >= event.end_ms + EVENT_GRACE_MS {
                    self.propose(
                        StopKind::EventEnded,
                        format!("Stopped automatically 5 minutes after {} ended", event.title),
                        now,
                    );
                }
            }
            if let Some(p) = self.pending.clone() {
                if now >= p.deadline_ms {
                    self.pending = None;
                    self.kept.insert(p.kind);
                    actions.push(Action::Stop { reason: p.reason });
                }
            }
        } else {
            self.pending = None;
        }

        let after = self.hud();
        if after != before {
            actions.push(Action::Hud(after));
        }
        actions
    }

    /// "Keep recording": cancel the countdown. A call that ended is not
    /// raised again; silence is, if the room stays quiet for another while.
    pub fn keep(&mut self, now_ms: i64) -> Vec<Action> {
        let before = self.hud();
        if let Some(p) = self.pending.take() {
            if p.kind == StopKind::Silence {
                self.quiet_from = now_ms;
            } else {
                self.kept.insert(p.kind);
            }
        }
        let after = self.hud();
        if after != before { vec![Action::Hud(after)] } else { Vec::new() }
    }

    /// "Not now" on an offer to record.
    pub fn dismiss(&mut self) -> Vec<Action> {
        let before = self.hud();
        self.dismissed = true;
        let after = self.hud();
        if after != before { vec![Action::Hud(after)] } else { Vec::new() }
    }
}

/// The monitor shared by the background thread and the window's commands.
#[derive(Default)]
pub struct MonitorState(pub Mutex<Monitor>);

impl MonitorState {
    fn lock(&self) -> std::sync::MutexGuard<'_, Monitor> {
        self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

const OFFERS: &str = "recording.offer_calls";
const AUTO_STOP: &str = "recording.auto_stop";
const HUD: &str = "meeting-hud";

/// Whether Tidy offers to record calls and stops finished recordings. Both
/// are on unless the person turned them off.
fn preferences(app: &AppHandle) -> (bool, bool) {
    let db = app.state::<crate::db::Db>();
    let Ok(conn) = db.lock() else {
        return (true, true);
    };
    let on = |key| crate::db::get_setting(&conn, key).ok().flatten().as_deref() != Some("false");
    (on(OFFERS), on(AUTO_STOP))
}

fn now_ms() -> i64 {
    crate::db::now_ms()
}

/// Show the floating window for an offer or countdown, or hide it.
fn show(app: &AppHandle, hud: &Hud) {
    let _ = app.emit("meeting-hud", hud);
    let existing = app.get_webview_window(HUD);
    if *hud == Hud::None {
        if let Some(window) = existing {
            let _ = window.hide();
        }
        return;
    }
    let window = match existing {
        Some(window) => window,
        None => match build(app) {
            Ok(window) => window,
            Err(error) => {
                log::warn!("The meeting window could not be opened: {error}");
                return;
            }
        },
    };
    let _ = window.show();
}

fn build(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    const WIDTH: f64 = 360.0;
    const HEIGHT: f64 = 128.0;
    let mut builder = tauri::WebviewWindowBuilder::new(app, HUD, tauri::WebviewUrl::App("index.html?view=hud".into()))
        .title("Tidy")
        .inner_size(WIDTH, HEIGHT)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .skip_taskbar(true)
        .focused(false);
    #[cfg(target_os = "macos")]
    {
        builder = builder.title_bar_style(tauri::TitleBarStyle::Overlay).hidden_title(true);
    }
    if let Ok(Some(monitor)) = app.primary_monitor() {
        let scale = monitor.scale_factor();
        let size = monitor.size().to_logical::<f64>(scale);
        builder = builder.position(size.width - WIDTH - 24.0, 48.0);
    }
    builder.build()
}

fn apply(app: &AppHandle, actions: Vec<Action>) {
    for action in actions {
        match action {
            Action::Hud(hud) => show(app, &hud),
            Action::Stop { reason } => {
                let _ = app.emit("auto-stop", reason);
            }
        }
    }
}

/// Start watching. Runs for the life of the app.
pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        let mut tick: u64 = 0;
        let mut call_apps: Vec<String> = Vec::new();
        let mut prefs = (true, true);
        loop {
            std::thread::sleep(Duration::from_secs(1));
            if tick % 2 == 0 {
                call_apps = crate::audio::calls::mic_apps().into_iter().map(String::from).collect();
            }
            if tick % 10 == 0 {
                prefs = preferences(&app);
            }
            tick += 1;
            let observation = Observation {
                now_ms: now_ms(),
                call_apps: call_apps.clone(),
                recording: crate::commands::recording::recording_in_flight(&app),
                last_voice_ms: crate::audio::recorder::last_voice_ms(),
                offers: prefs.0,
                auto_stop: prefs.1,
            };
            let actions = app.state::<MonitorState>().lock().step(&observation);
            apply(&app, actions);
        }
    });
}

/// What the floating window should show right now, for when it first loads.
#[tauri::command]
pub fn meeting_hud_state(state: tauri::State<'_, MonitorState>) -> Hud {
    state.lock().hud()
}

/// A button in the floating window: "record", "dismiss" or "keep".
#[tauri::command]
pub fn meeting_hud_respond(app: AppHandle, state: tauri::State<'_, MonitorState>, action: String) -> crate::error::AppResult<()> {
    let actions = match action.as_str() {
        "record" => {
            let app_name = match state.lock().hud() {
                Hud::Offer { app } => app,
                _ => String::new(),
            };
            // The main window owns the recorder, so it starts the recording.
            let _ = app.emit_to("main", "hud-record", app_name);
            state.lock().dismiss()
        }
        "dismiss" => state.lock().dismiss(),
        "keep" => state.lock().keep(now_ms()),
        _ => return Err(crate::error::AppError::Invalid("Unknown meeting window action.".into())),
    };
    apply(&app, actions);
    Ok(())
}

#[tauri::command]
pub async fn get_meeting_automation(app: AppHandle) -> crate::error::AppResult<(bool, bool)> {
    Ok(preferences(&app))
}

#[tauri::command]
pub async fn set_meeting_automation(app: AppHandle, offers: bool, auto_stop: bool) -> crate::error::AppResult<()> {
    crate::local_ai::db(&app, move |c| {
        crate::db::set_setting(c, OFFERS, if offers { "true" } else { "false" })?;
        crate::db::set_setting(c, AUTO_STOP, if auto_stop { "true" } else { "false" })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look(now: i64, apps: &[&str], recording: bool, last_voice: i64) -> Observation {
        Observation {
            now_ms: now,
            call_apps: apps.iter().map(|a| a.to_string()).collect(),
            recording,
            last_voice_ms: last_voice,
            offers: true,
            auto_stop: true,
        }
    }

    fn offers(actions: &[Action]) -> Vec<String> {
        actions
            .iter()
            .filter_map(|a| match a {
                Action::Hud(Hud::Offer { app }) => Some(app.clone()),
                _ => None,
            })
            .collect()
    }

    fn stops(actions: &[Action]) -> Vec<String> {
        actions
            .iter()
            .filter_map(|a| match a {
                Action::Stop { reason } => Some(reason.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_call_is_offered_after_three_seconds_and_withdrawn_when_it_ends() {
        let mut m = Monitor::default();
        assert!(m.step(&look(0, &["Zoom"], false, 0)).is_empty());
        assert!(m.step(&look(2_000, &["Zoom"], false, 0)).is_empty());
        assert_eq!(offers(&m.step(&look(3_000, &["Zoom"], false, 0))), vec!["Zoom"]);
        // Offered once, not every second.
        assert!(m.step(&look(4_000, &["Zoom"], false, 0)).is_empty());
        assert_eq!(m.step(&look(5_000, &[], false, 0)), vec![Action::Hud(Hud::None)]);
    }

    #[test]
    fn not_now_hides_the_offer_for_the_rest_of_that_call() {
        let mut m = Monitor::default();
        m.step(&look(0, &["Teams"], false, 0));
        m.step(&look(3_000, &["Teams"], false, 0));
        assert_eq!(m.dismiss(), vec![Action::Hud(Hud::None)]);
        assert!(m.step(&look(60_000, &["Teams"], false, 0)).is_empty());
        // A later call is offered again.
        m.step(&look(61_000, &[], false, 0));
        m.step(&look(62_000, &["Teams"], false, 0));
        assert_eq!(offers(&m.step(&look(65_000, &["Teams"], false, 0))), vec!["Teams"]);
    }

    #[test]
    fn offers_can_be_turned_off() {
        let mut m = Monitor::default();
        let mut o = look(0, &["Zoom"], false, 0);
        o.offers = false;
        m.step(&o);
        o.now_ms = 10_000;
        assert!(m.step(&o).is_empty());
    }

    #[test]
    fn the_end_of_the_call_counts_down_then_stops() {
        let mut m = Monitor::default();
        m.step(&look(0, &["Zoom"], true, 0));
        m.step(&look(60_000, &["Zoom"], true, 60_000));
        // Zoom lets go of the microphone; a brief gap is not the end.
        assert!(m.step(&look(61_000, &[], true, 61_000)).is_empty());
        assert!(m.step(&look(70_000, &[], true, 70_000)).is_empty());
        let shown = m.step(&look(81_000, &[], true, 81_000));
        assert_eq!(
            shown,
            vec![Action::Hud(Hud::Countdown {
                reason: "Stopped automatically when Zoom ended the call".into(),
                deadline_ms: 96_000
            })]
        );
        assert!(stops(&m.step(&look(90_000, &[], true, 90_000))).is_empty());
        let done = m.step(&look(96_000, &[], true, 96_000));
        assert_eq!(stops(&done), vec!["Stopped automatically when Zoom ended the call"]);
        assert!(done.contains(&Action::Hud(Hud::None)));
    }

    #[test]
    fn keep_recording_cancels_and_a_returning_call_cancels_too() {
        let mut m = Monitor::default();
        m.step(&look(0, &["Zoom"], true, 0));
        m.step(&look(1_000, &[], true, 1_000));
        m.step(&look(21_000, &[], true, 21_000));
        assert!(matches!(m.hud(), Hud::Countdown { .. }));
        assert_eq!(m.keep(25_000), vec![Action::Hud(Hud::None)]);
        // The call that ended is not raised again.
        assert!(stops(&m.step(&look(200_000, &[], true, 199_000))).is_empty());

        let mut m = Monitor::default();
        m.step(&look(0, &["Zoom"], true, 0));
        m.step(&look(1_000, &[], true, 1_000));
        m.step(&look(21_000, &[], true, 21_000));
        assert_eq!(m.step(&look(22_000, &["Zoom"], true, 22_000)), vec![Action::Hud(Hud::None)]);
    }

    #[test]
    fn a_silent_room_counts_down_unless_someone_speaks() {
        let mut m = Monitor::default();
        m.step(&look(0, &[], true, 0));
        assert!(m.step(&look(179_000, &[], true, 0)).is_empty());
        assert!(matches!(m.step(&look(180_000, &[], true, 0)).as_slice(), [Action::Hud(Hud::Countdown { .. })]));
        // Someone speaks during the countdown.
        assert_eq!(m.step(&look(185_000, &[], true, 184_000)), vec![Action::Hud(Hud::None)]);
        // Quiet again for the full time, then left to run out.
        m.step(&look(364_000, &[], true, 184_000));
        assert_eq!(
            stops(&m.step(&look(379_000, &[], true, 184_000))),
            vec!["Stopped automatically after 3 minutes of silence"]
        );
    }

    #[test]
    fn keeping_a_quiet_recording_waits_another_three_minutes() {
        let mut m = Monitor::default();
        m.step(&look(0, &[], true, 0));
        m.step(&look(180_000, &[], true, 0));
        m.keep(181_000);
        assert!(m.step(&look(300_000, &[], true, 0)).is_empty());
        assert!(matches!(m.step(&look(361_000, &[], true, 0)).as_slice(), [Action::Hud(Hud::Countdown { .. })]));
    }

    #[test]
    fn a_recording_stops_five_minutes_after_its_event() {
        let mut m = Monitor::default();
        m.step(&look(0, &[], true, 0));
        m.set_event(Some(EventEnd { title: "Weekly sync".into(), end_ms: 100_000 }));
        assert!(m.step(&look(399_000, &[], true, 399_000)).is_empty());
        assert!(matches!(m.step(&look(400_000, &[], true, 400_000)).as_slice(), [Action::Hud(Hud::Countdown { .. })]));
        assert_eq!(
            stops(&m.step(&look(415_000, &[], true, 415_000))),
            vec!["Stopped automatically 5 minutes after Weekly sync ended"]
        );
    }

    #[test]
    fn stopping_by_hand_during_a_call_does_not_offer_it_again() {
        let mut m = Monitor::default();
        m.step(&look(0, &["Zoom"], true, 0));
        assert!(offers(&m.step(&look(10_000, &["Zoom"], false, 0))).is_empty());
        assert!(offers(&m.step(&look(20_000, &["Zoom"], false, 0))).is_empty());
        assert_eq!(m.hud(), Hud::None);
    }

    #[test]
    fn auto_stop_can_be_turned_off() {
        let mut m = Monitor::default();
        let mut o = look(0, &["Zoom"], true, 0);
        o.auto_stop = false;
        m.step(&o);
        o.call_apps.clear();
        o.now_ms = 1_000_000;
        assert!(m.step(&o).is_empty());
    }
}
