pub mod models;
pub mod runtime;
mod service;

use crate::{
    db::Db,
    error::{AppError, AppResult},
};
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use tauri::{AppHandle, Manager};
use tokio::sync::{Mutex as AsyncMutex, MutexGuard};

pub const CONFIG_KEY: &str = "local_ai.config.v1";
#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    pub provider: String,
    pub model_id: String,
    pub ollama_model: String,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            provider: "disabled".into(),
            model_id: "qwen3-4b".into(),
            ollama_model: String::new(),
        }
    }
}
/// Who asked for the work in progress. The interface only offers to cancel
/// what the person started themselves.
#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    User,
    Worker,
}
#[derive(Clone, Serialize)]
pub struct Progress {
    pub label: String,
    pub model_id: Option<String>,
    pub completed: u64,
    pub total: u64,
    pub origin: Origin,
}
impl Default for Progress {
    fn default() -> Self {
        Self {
            label: String::new(),
            model_id: None,
            completed: 0,
            total: 0,
            origin: Origin::User,
        }
    }
}
impl Progress {
    pub fn new(label: &str, id: Option<&str>) -> Self {
        Self {
            label: label.into(),
            model_id: id.map(str::to_owned),
            ..Self::default()
        }
    }
}
/// The message a cancelled operation reports. Work stopped this way is put back
/// on the queue rather than recorded as a failure.
pub const CANCELLED: &str = "AI processing cancelled";

#[derive(Default)]
pub struct AiState {
    gate: AsyncMutex<()>,
    process: std::sync::Arc<Mutex<Option<tokio::process::Child>>>,
    pub cancel: AtomicBool,
    pub recording: AtomicBool,
    /// A meeting is being recorded right now (not transcribed): questions the
    /// person asks may run, but background work waits.
    pub live: AtomicBool,
    current: Mutex<Option<Progress>>,
    /// True while the background worker owns the gate.
    worker_origin: AtomicBool,
    request_id: Mutex<Option<String>>,
    worker: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
}
fn busy() -> AppError {
    AppError::Other(
        "Another local AI operation is running. Wait for it to finish or cancel it in Settings."
            .into(),
    )
}
pub struct Operation<'a> {
    state: &'a AiState,
    _guard: MutexGuard<'a, ()>,
}
impl Drop for Operation<'_> {
    fn drop(&mut self) {
        if let Ok(mut id) = self.state.request_id.lock() {
            *id = None;
        }
        if let Ok(mut p) = self.state.current.lock() {
            *p = None;
        }
        self.state.worker_origin.store(false, Ordering::SeqCst);
    }
}
impl AiState {
    pub fn begin(&self, label: &str) -> AppResult<Operation<'_>> {
        let operation = self.claim(Origin::User)?;
        self.progress(Progress::new(label, None));
        Ok(operation)
    }
    /// Take the gate for the background worker without showing anything. The
    /// queue poll runs every few seconds and must not flicker in the interface;
    /// real work publishes its own progress once it starts.
    pub fn begin_worker(&self) -> AppResult<Operation<'_>> {
        self.claim(Origin::Worker)
    }
    /// Start something the person is waiting on. Background meeting
    /// processing steps aside for it and is queued again; anything else the
    /// person started has to finish first.
    pub async fn begin_now(&self, label: &str) -> AppResult<Operation<'_>> {
        let guard = match self.gate.try_lock() {
            Ok(guard) => guard,
            Err(_) if self.worker_origin.load(Ordering::SeqCst) => {
                self.progress(Progress::new("Pausing background meeting processing", None));
                self.cancel.store(true, Ordering::SeqCst);
                tokio::time::timeout(std::time::Duration::from_secs(30), self.gate.lock())
                    .await
                    .map_err(|_| busy())?
            }
            Err(_) => return Err(busy()),
        };
        let operation = self.claim_with(guard, Origin::User)?;
        self.progress(Progress::new(label, None));
        Ok(operation)
    }
    fn claim(&self, origin: Origin) -> AppResult<Operation<'_>> {
        let guard = self.gate.try_lock().map_err(|_| busy())?;
        self.claim_with(guard, origin)
    }
    fn claim_with<'a>(
        &'a self,
        guard: MutexGuard<'a, ()>,
        origin: Origin,
    ) -> AppResult<Operation<'a>> {
        self.cancel.store(false, Ordering::SeqCst);
        let live_request = origin == Origin::User && self.live.load(Ordering::SeqCst);
        if self.recording.load(Ordering::SeqCst) && !live_request {
            return Err(AppError::Other(
                "AI processing is paused while recording or transcribing a meeting.".into(),
            ));
        }
        self.worker_origin
            .store(origin == Origin::Worker, Ordering::SeqCst);
        Ok(Operation {
            state: self,
            _guard: guard,
        })
    }
    pub fn origin(&self) -> Origin {
        if self.worker_origin.load(Ordering::SeqCst) {
            Origin::Worker
        } else {
            Origin::User
        }
    }
    pub fn progress(&self, p: Progress) {
        let mut p = p;
        p.origin = self.origin();
        if let Ok(mut current) = self.current.lock() {
            *current = Some(p);
        }
    }
    pub fn check_cancel(&self) -> AppResult<()> {
        if self.cancel.load(Ordering::SeqCst) {
            Err(AppError::Other(CANCELLED.into()))
        } else {
            Ok(())
        }
    }
    pub async fn cancelled(&self) {
        while !self.cancel.load(Ordering::SeqCst) {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }
    pub fn reserve_recording(&self) -> AppResult<()> {
        if self.recording.swap(true, Ordering::SeqCst) {
            return Err(AppError::Other(
                "A meeting is already recording or processing".into(),
            ));
        }
        self.cancel.store(true, Ordering::SeqCst);
        Ok(())
    }
    pub async fn wait_until_idle(&self) {
        let _guard = self.gate.lock().await;
    }
    pub async fn pause_for_recording(&self) -> AppResult<()> {
        self.reserve_recording()?;
        self.wait_until_idle().await;
        Ok(())
    }
    pub fn cancel_request(&self, request_id: Option<&str>) {
        if let Some(id) = request_id {
            // Hold ownership stable until the cancellation flag is set.
            if let Ok(active) = self.request_id.lock() {
                if active.as_deref() == Some(id) {
                    self.cancel.store(true, Ordering::SeqCst);
                }
            }
        } else if self.origin() == Origin::User {
            // Cancel only what the person started: background work they never
            // asked about keeps going.
            self.cancel.store(true, Ordering::SeqCst);
        }
    }
    pub fn shutdown(&self) {
        if let Ok(mut process) = self.process.lock() {
            if let Some(mut child) = process.take() {
                if let Err(error) = child.start_kill() {
                    log::warn!("AI shutdown: {error}");
                }
            }
        }
        self.cancel.store(true, Ordering::SeqCst);
        if let Ok(mut worker) = self.worker.lock() {
            if let Some(task) = worker.take() {
                task.abort();
            }
        }
    }
}
pub async fn db<T: Send + 'static>(
    app: &AppHandle,
    f: impl FnOnce(&mut rusqlite::Connection) -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Db>();
        let mut conn = state.lock()?;
        f(&mut conn)
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))?
}
pub fn read_config(c: &rusqlite::Connection) -> AppResult<Config> {
    crate::db::get_setting(c, CONFIG_KEY)?
        .map(|s| serde_json::from_str(&s).map_err(AppError::from))
        .unwrap_or_else(|| Ok(Config::default()))
}
/// Put back work that was dropped because a meeting started recording. It was
/// never a failure, so it should not look like one once the meeting is over.
pub async fn requeue_cancelled(app: &AppHandle) -> AppResult<()> {
    db(app, |c| {
        c.execute(
            "UPDATE meeting_ai SET state='queued',error=NULL WHERE state='error' AND error LIKE ?1",
            [format!("%{CANCELLED}%")],
        )?;
        Ok(())
    })
    .await
}

pub fn start_worker(app: AppHandle) {
    let handle = app.clone();
    let worker = tauri::async_runtime::spawn(async move {
        if let Err(e) = db(&handle, move |c| {
            c.execute(
                "UPDATE meeting_ai SET state='queued' WHERE state IN ('summarising','indexing')",
                [],
            )?;
            tidy_core::store::meeting_ai::queue_outdated(c)?;
            // A note summary cut short by quitting starts again.
            c.execute("UPDATE note_ai SET state='queued' WHERE state='summarising'", [])?;
            Ok(())
        })
        .await
        {
            log::error!("AI queue recovery: {e}");
        }
        if let Err(e) = requeue_cancelled(&handle).await {
            log::warn!("AI queue could not be resumed: {e}");
        }
        loop {
            if let Err(e) = service::process_next(&handle).await {
                log::warn!("AI processing: {e}");
            }
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        }
    });
    if let Ok(mut slot) = app.state::<AiState>().worker.lock() {
        *slot = Some(worker);
    };
}

#[derive(Serialize)]
pub struct ModelStatus {
    #[serde(flatten)]
    model: models::Model,
    downloaded: bool,
}
#[derive(Serialize)]
pub struct Status {
    memory_bytes: Option<u64>,
    recommended_model_id: String,
    config: Config,
    models: Vec<ModelStatus>,
    progress: Option<Progress>,
    runtime_available: bool,
    ready: bool,
    search_ready: bool,
    disk_bytes: u64,
    recording_paused: bool,
}
#[tauri::command]
pub async fn local_ai_status(app: AppHandle) -> AppResult<Status> {
    let memory_bytes = machine_memory().await?;
    let recommended_model_id = recommended_model(memory_bytes).to_string();
    let config = db(&app, read_config_mut).await?;
    let a = app.clone();
    let catalogue = models::all()?;
    let (models, disk_bytes) = tauri::async_runtime::spawn_blocking(move || {
        let models = catalogue
            .iter()
            .map(|m| ModelStatus {
                model: m.clone(),
                downloaded: models::installed(&a, m),
            })
            .collect::<Vec<_>>();
        let size = models::dir(&a)
            .ok()
            .and_then(|d| std::fs::read_dir(d).ok())
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .filter_map(|e| e.metadata().ok())
                    .map(|m| m.len())
                    .sum()
            })
            .unwrap_or(0);
        (models, size)
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))?;
    let runtime_available = runtime::executable().is_ok();
    let ready = match config.provider.as_str() {
        "builtin" => {
            runtime_available
                && models
                    .iter()
                    .any(|m| m.model.id == config.model_id && m.downloaded)
        }
        "ollama" => !config.ollama_model.is_empty(),
        _ => false,
    };
    let search_ready = ready
        && runtime_available
        && models
            .iter()
            .any(|m| m.model.id == "nomic-embed" && m.downloaded);
    let progress = app
        .state::<AiState>()
        .current
        .lock()
        .map_err(|_| AppError::Other("AI status is unavailable".into()))?
        .clone();
    Ok(Status {
        memory_bytes,
        recommended_model_id,
        config,
        models,
        progress,
        runtime_available,
        ready,
        search_ready,
        disk_bytes,
        // During a live recording, questions can still be asked.
        recording_paused: {
            let ai = app.state::<AiState>();
            ai.recording.load(Ordering::SeqCst) && !ai.live.load(Ordering::SeqCst)
        },
    })
}
fn recommended_model(memory: Option<u64>) -> &'static str {
    match memory.map(|bytes| bytes / (1024 * 1024 * 1024)) {
        Some(0..=7) => "qwen3-small",
        Some(8..=11) => "qwen3-1.7b",
        Some(12..=23) | None => "qwen3-4b",
        Some(24..=31) => "qwen3-8b",
        Some(32..=63) => "qwen3-14b",
        Some(_) => "qwen3-32b",
    }
}
async fn machine_memory() -> AppResult<Option<u64>> {
    tauri::async_runtime::spawn_blocking(|| {
        static MEMORY: std::sync::OnceLock<Option<u64>> = std::sync::OnceLock::new();
        *MEMORY.get_or_init(|| {
            std::process::Command::new("/usr/sbin/sysctl")
                .args(["-n", "hw.memsize"])
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .and_then(|s| s.trim().parse().ok())
        })
    })
    .await
    .map_err(|e| AppError::Other(e.to_string()))
}
fn read_config_mut(c: &mut rusqlite::Connection) -> AppResult<Config> {
    read_config(c)
}
#[tauri::command]
pub async fn local_ai_configure(app: AppHandle, config: Config) -> AppResult<()> {
    let state = app.state::<AiState>();
    let _op = state.begin("Updating AI settings")?;
    models::configure(&app, config).await
}
#[tauri::command]
pub async fn local_ai_setup(app: AppHandle) -> AppResult<()> {
    let state = app.state::<AiState>();
    let _op = state.begin("Preparing local AI")?;
    runtime::executable()?;
    let selected = recommended_model(machine_memory().await?);
    models::download(&app, selected, &state).await?;
    models::download(&app, "nomic-embed", &state).await?;
    models::configure(
        &app,
        Config {
            provider: "builtin".into(),
            model_id: selected.into(),
            ..Config::default()
        },
    )
    .await
}
#[tauri::command]
pub async fn local_ai_download(app: AppHandle, id: String) -> AppResult<()> {
    let state = app.state::<AiState>();
    let _op = state.begin("Preparing download")?;
    models::download(&app, &id, &state).await
}
#[tauri::command]
pub async fn local_ai_remove(app: AppHandle, id: String, confirmed: bool) -> AppResult<()> {
    let state = app.state::<AiState>();
    let _op = state.begin("Removing model")?;
    models::remove(&app, &id, confirmed).await
}
#[tauri::command]
pub fn local_ai_cancel(state: tauri::State<AiState>, request_id: Option<String>) {
    state.cancel_request(request_id.as_deref());
}
#[tauri::command]
pub async fn meeting_ai_jobs(
    app: AppHandle,
) -> AppResult<Vec<tidy_core::store::meeting_ai::MeetingJob>> {
    db(&app, |c| tidy_core::store::meeting_ai::list(c)).await
}
#[tauri::command]
pub async fn meeting_ai_retry(app: AppHandle, page_id: String) -> AppResult<()> {
    db(&app,move|c|{
    // Retrying or regenerating always writes new notes, even for an unchanged transcript.
    let n=c.execute("UPDATE meeting_ai SET state='queued',error=NULL,regenerate=1 WHERE page_id=?1 AND state NOT IN ('summarising','indexing')",[page_id])?;
    if n==0{return Err(AppError::Invalid("Meeting is already processing or unavailable".into()));}Ok(())
}).await
}
#[tauri::command]
pub async fn ask_meetings(
    app: AppHandle,
    question: String,
    from: Option<i64>,
    until: Option<i64>,
    client_id: Option<String>,
) -> AppResult<service::Answer> {
    service::ask(&app, &question, from, until, client_id).await
}
/// The live transcript as text for the model: "[mm:ss] You: …", with the
/// call's side as Others. Without a call, everyone is in the room.
fn live_text(lines: &[crate::audio::live::LiveLine]) -> String {
    let call = lines.iter().any(|l| l.source == "system");
    let mut lines: Vec<&crate::audio::live::LiveLine> = lines.iter().collect();
    lines.sort_by_key(|l| l.start_ms);
    lines
        .iter()
        .map(|l| {
            let s = l.start_ms.max(0) / 1000;
            let who = match (l.source, call) {
                ("system", _) => "Others",
                (_, true) => "You",
                _ => "Room",
            };
            format!("[{:02}:{:02}] {who}: {}", s / 60, s % 60, l.text)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// "Recap so far" while a meeting is being recorded.
#[tauri::command]
pub async fn live_recap(app: AppHandle) -> AppResult<service::Recap> {
    let transcript = live_text(&crate::commands::recording::live_lines(&app));
    if transcript.trim().is_empty() {
        return Err(AppError::Invalid(
            "Nothing has been transcribed yet. Try again once people have spoken.".into(),
        ));
    }
    let state = app.state::<AiState>();
    let _op = state.begin_now("Writing a recap").await?;
    let config = db(&app, read_config_mut).await?;
    let engine = runtime::Engine::start(&app, &config, false, &state).await?;
    let result = service::recap(&engine, &transcript, &state).await;
    engine.stop().await?;
    result
}

pub async fn generate(
    app: &AppHandle,
    instruction: &str,
    context: Option<&str>,
    request_id: Option<String>,
    mode: Option<&str>,
) -> AppResult<String> {
    let context = context.unwrap_or("");
    if instruction.chars().count() > 4000 {
        return Err(AppError::Invalid("Use a shorter instruction".into()));
    }
    // Long pages are read in parts, so only a truly huge one is refused.
    if context.chars().count() > 400_000 {
        return Err(AppError::Invalid(
            "This page is too long for the assistant. Select part of it first.".into(),
        ));
    }
    let state = app.state::<AiState>();
    let _op = state.begin_now("Generating locally").await?;
    *state
        .request_id
        .lock()
        .map_err(|_| AppError::Other("AI request state is unavailable".into()))? = request_id;
    let config = db(app, read_config_mut).await?;
    let engine = runtime::Engine::start(app, &config, false, &state).await?;
    let result = service::assist(&engine, instruction, context, mode, &state).await;
    engine.stop().await?;
    result
}
pub async fn summarise(
    app: &AppHandle,
    transcript: &str,
) -> AppResult<tidy_core::llm::MeetingSummary> {
    let state = app.state::<AiState>();
    let _op = state.begin_now("Summarising locally").await?;
    let config = db(app, read_config_mut).await?;
    service::summarise(
        app,
        &config,
        transcript,
        tidy_core::store::note_templates::general(),
        &state,
    )
    .await
}

#[tauri::command]
pub async fn ai_end_recording(app: AppHandle) -> AppResult<()> {
    app.state::<AiState>()
        .recording
        .store(false, Ordering::SeqCst);
    requeue_cancelled(&app).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recommendations_leave_headroom_and_never_select_search_models() {
        for (gb, expected) in [(4, "qwen3-small"), (8, "qwen3-1.7b"), (12, "qwen3-4b"), (16, "qwen3-4b"), (24, "qwen3-8b"), (32, "qwen3-14b"), (48, "qwen3-14b"), (64, "qwen3-32b"), (128, "qwen3-32b")] {
            let id = recommended_model(Some(gb * 1024 * 1024 * 1024));
            assert_eq!(id, expected);
            let model = models::model(id).unwrap();
            assert_eq!(model.purpose, "chat");
            assert!(model.recommended_ram_gb <= gb);
        }
        assert_eq!(recommended_model(None), "qwen3-4b");
    }

    #[test]
    fn stale_assistant_cancellation_does_not_cancel_a_download() {
        let state = AiState::default();
        let op = state.begin("assistant").unwrap();
        *state.request_id.lock().unwrap() = Some("first".into());
        state.cancel_request(Some("different"));
        assert!(state.check_cancel().is_ok());
        state.cancel_request(Some("first"));
        assert!(state.check_cancel().is_err());
        drop(op);
        let _download = state.begin("download").unwrap();
        state.cancel_request(Some("first"));
        assert!(state.check_cancel().is_ok());
        state.cancel_request(None);
        assert!(state.check_cancel().is_err());
    }
    #[tokio::test]
    async fn serialises_operations_and_honours_cancellation() {
        let state = AiState::default();
        let op = state.begin("test").unwrap();
        assert!(state.begin("second").is_err());
        state.cancel.store(true, Ordering::SeqCst);
        assert!(state.check_cancel().is_err());
        drop(op);
        assert!(state.current.lock().unwrap().is_none());
        let op = state.begin("retry").unwrap();
        assert!(state.check_cancel().is_ok());
        drop(op);
        state.pause_for_recording().await.unwrap();
        assert!(state.begin("during recording").is_err());
        state.recording.store(false, Ordering::SeqCst);
        assert!(state.begin("after recording").is_ok());
    }

    #[tokio::test]
    async fn questions_can_be_asked_during_a_live_recording_but_not_background_work() {
        let state = AiState::default();
        state.pause_for_recording().await.unwrap();
        state.live.store(true, Ordering::SeqCst);
        assert!(state.begin_worker().is_err(), "background work waits for the call to end");
        assert!(state.begin("a question during the call").is_ok());
        // Transcribing afterwards is not live: everything waits again.
        state.live.store(false, Ordering::SeqCst);
        assert!(state.begin("during transcription").is_err());
    }

    #[test]
    fn the_live_transcript_reads_as_you_and_others_or_the_room() {
        use crate::audio::live::LiveLine;
        let line = |source, start_ms, text: &str| LiveLine { source, start_ms, end_ms: start_ms + 1000, text: text.into() };
        let call = vec![line("system", 5_000, "Shall we start?"), line("mic", 1_000, "Morning all.")];
        assert_eq!(super::live_text(&call), "[00:01] You: Morning all.\n[00:05] Others: Shall we start?");
        let room = vec![line("mic", 65_000, "Let's begin.")];
        assert_eq!(super::live_text(&room), "[01:05] Room: Let's begin.");
    }
}
