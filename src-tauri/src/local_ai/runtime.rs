//! A private, short-lived inference process. Dropping it kills the process and frees RAM.
use super::{models, AiState, Config, Progress};
use crate::error::{AppError, AppResult};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tauri::AppHandle;
use tidy_core::store::meeting_ai::validate_vector;
use tokio::process::{Child, Command};

/// What `chat` reports when the prompt will not fit the model's context.
/// Callers that can split or trim their input look for it.
pub const TOO_LONG: &str =
    "This request is too long for the local model. Use a shorter selection or narrower question.";
pub fn too_long(error: &AppError) -> bool {
    error.to_string().contains(TOO_LONG)
}

pub fn client() -> AppResult<reqwest::Client> {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| AppError::Other(e.to_string()))
}
pub fn executable() -> AppResult<PathBuf> {
    let exe = std::env::current_exe()?;
    let folder = exe
        .parent()
        .ok_or_else(|| AppError::Other("Application path is unavailable".into()))?;
    let packaged = folder.join("tidy-ai");
    if packaged.exists() {
        return Ok(packaged);
    }
    if cfg!(any(debug_assertions, test)) {
        let dev =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries/tidy-ai-aarch64-apple-darwin");
        if dev.exists() {
            return Ok(dev);
        }
    }
    Err(AppError::Other(
        "This build is missing the local AI runtime. Install a complete Tidy release.".into(),
    ))
}
pub struct Engine {
    child: Option<std::sync::Arc<std::sync::Mutex<Option<Child>>>>,
    base: String,
    key: String,
    model: String,
    ollama: bool,
    client: reqwest::Client,
}
impl Engine {
    pub async fn start(
        app: &AppHandle,
        config: &Config,
        embedding: bool,
        state: &AiState,
    ) -> AppResult<Self> {
        if !embedding && config.provider == "disabled" {
            return Err(AppError::Invalid(
                "Enable local AI in Settings first".into(),
            ));
        }
        let client = client()?;
        if !embedding && config.provider == "ollama" {
            // Explicit, local-only model selection; cloud model names are never accepted.
            if config.ollama_model.contains("cloud") {
                return Err(AppError::Invalid("Cloud models are not supported".into()));
            }
            let info: Value = client
                .post("http://127.0.0.1:11434/api/show")
                .json(&json!({"model":config.ollama_model}))
                .timeout(Duration::from_secs(5))
                .send()
                .await
                .map_err(|e| AppError::Other(format!("Ollama is unavailable: {e}")))?
                .error_for_status()
                .map_err(|e| AppError::Other(e.to_string()))?
                .json()
                .await
                .map_err(|e| AppError::Other(e.to_string()))?;
            if info.get("remote_host").is_some()
                || !info["capabilities"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|v| v == "completion"))
            {
                return Err(AppError::Invalid(
                    "The selected Ollama model is no longer a local chat model".into(),
                ));
            }
            return Ok(Self {
                child: None,
                base: "http://127.0.0.1:11434".into(),
                key: String::new(),
                model: config.ollama_model.clone(),
                ollama: true,
                client,
            });
        }
        let id = if embedding {
            "nomic-embed"
        } else {
            &config.model_id
        };
        Self::start_builtin(Some(app), models::path(app, id)?, id, state).await
    }

    pub(super) async fn start_builtin(
        app: Option<&AppHandle>,
        path: PathBuf,
        id: &str,
        state: &AiState,
    ) -> AppResult<Self> {
        let def = models::model(id)?;
        let embedding = def.purpose == "embedding";
        state.progress(Progress::new("Loading local model", Some(id)));
        models::ensure_verified(app, &path, def, state).await?;
        let mut last = None;
        for attempt in 0..2 {
            match Self::spawn_once(&path, id, embedding, state).await {
                Ok(engine) => return Ok(engine),
                Err((retryable, error)) => {
                    if !retryable || attempt == 1 {
                        return Err(error);
                    }
                    // A port can be taken between choosing it and binding it.
                    // Try once more on a fresh one before blaming memory.
                    log::warn!("Local AI stopped before it was ready, retrying: {error}");
                    last = Some(error);
                }
            }
        }
        Err(last.unwrap_or_else(|| AppError::Other("Local AI could not be started.".into())))
    }

    async fn spawn_once(
        path: &Path,
        id: &str,
        embedding: bool,
        state: &AiState,
    ) -> Result<Self, (bool, AppError)> {
        let fail = |e: AppError| (false, e);
        let client = client().map_err(fail)?;
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .map_err(|e| fail(AppError::from(e)))?;
        let port = listener
            .local_addr()
            .map_err(|e| fail(AppError::from(e)))?
            .port();
        drop(listener);
        let key = crate::db::new_id();
        let mut command = Command::new(executable().map_err(fail)?);
        command
            .args(["--model"])
            .arg(path)
            .args([
                "--host",
                "127.0.0.1",
                "--port",
                &port.to_string(),
                "--ctx-size",
                "8192",
                "--parallel",
                "1",
                "--threads",
                "4",
                "--batch-size",
                if embedding { "2048" } else { "512" },
                "--ubatch-size",
                if embedding { "2048" } else { "128" },
                "--n-gpu-layers",
                "99",
                "--no-webui",
                "--no-warmup",
                "--log-disable",
            ])
            .env("LLAMA_API_KEY", &key)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if embedding {
            command.args(["--embedding", "--pooling", "mean"]);
        } else {
            command.args([
                "--jinja",
                "--chat-template-kwargs",
                r#"{"enable_thinking":false}"#,
            ]);
        }
        let mut child = command
            .spawn()
            .map_err(|e| fail(AppError::Other(format!("Could not start local AI: {e}"))))?;
        // Drain the child's stderr so a full pipe cannot wedge it, and keep the
        // last few lines: they say what actually went wrong.
        let reported = Arc::new(Mutex::new(VecDeque::<String>::new()));
        if let Some(stderr) = child.stderr.take() {
            let reported = reported.clone();
            tokio::spawn(async move {
                use tokio::io::AsyncBufReadExt;
                let mut lines = tokio::io::BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if let Ok(mut reported) = reported.lock() {
                        reported.push_back(line);
                        while reported.len() > 12 {
                            reported.pop_front();
                        }
                    }
                }
            });
        }
        *state
            .process
            .lock()
            .map_err(|_| fail(AppError::Other("AI process state unavailable".into())))? =
            Some(child);
        let engine = Self {
            child: Some(state.process.clone()),
            base: format!("http://127.0.0.1:{port}"),
            key,
            model: id.into(),
            ollama: false,
            client,
        };
        for _ in 0..240 {
            state.check_cancel().map_err(fail)?;
            if let Some(shared) = &engine.child {
                let exited = {
                    let mut guard = shared.lock().map_err(|_| {
                        fail(AppError::Other("AI process state unavailable".into()))
                    })?;
                    let child = guard.as_mut().ok_or_else(|| {
                        fail(AppError::Other("Local AI was stopped".into()))
                    })?;
                    child
                        .try_wait()
                        .map_err(|e| fail(AppError::from(e)))?
                        .is_some()
                };
                if exited {
                    // Give the stderr reader a moment to catch the last lines.
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    let detail = reported
                        .lock()
                        .map(|lines| lines.iter().cloned().collect::<Vec<_>>().join(" "))
                        .unwrap_or_default();
                    let detail = detail.trim().to_string();
                    let message = if detail.is_empty() {
                        "Local AI could not load the model. Close memory-heavy apps and retry."
                            .to_string()
                    } else {
                        format!("Local AI could not load the model. Close memory-heavy apps and retry. The runtime reported: {detail}")
                    };
                    return Err((true, AppError::Other(message)));
                }
            }
            // /props is authenticated (unlike /health). Verify our private server before sending any text.
            if let Ok(response) = engine
                .client
                .get(format!("{}/props", engine.base))
                .bearer_auth(&engine.key)
                .timeout(Duration::from_secs(1))
                .send()
                .await
            {
                if response.status().is_success() {
                    return Ok(engine);
                }
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        Err((
            false,
            AppError::Other("Local AI took too long to start. Retry after closing other apps.".into()),
        ))
    }
    async fn post(&self, path: &str, body: Value, state: &AiState) -> AppResult<Value> {
        let request = self
            .client
            .post(format!("{}{path}", self.base))
            .bearer_auth(&self.key)
            .json(&body);
        tokio::select! {
            _=state.cancelled()=>Err(AppError::Other("AI processing cancelled".into())),
            result=async {
                let response=request.send().await.map_err(|e|AppError::Other(format!("Local AI connection: {e}")))?;
                if !response.status().is_success(){return Err(AppError::Other(format!("Local AI request failed ({}). Check the model and available memory.",response.status())));}
                response.json().await.map_err(|e|AppError::Other(format!("Local AI response: {e}")))
            }=>result
        }
    }
    pub async fn chat(
        &self,
        system: &str,
        text: &str,
        schema: Option<Value>,
        state: &AiState,
    ) -> AppResult<String> {
        let messages = json!([{"role":"system","content":system},{"role":"user","content":text}]);
        if self.ollama {
            let mut body = json!({"model":self.model,"messages":messages,"stream":false,"think":false,"keep_alive":0,"options":{"num_ctx":8192,"num_predict":2000,"temperature":0}});
            if let Some(schema) = schema {
                body["format"] = schema;
            }
            let value = self.post("/api/chat", body, state).await?;
            return value["message"]["content"]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .map(str::to_owned)
                .ok_or_else(|| AppError::Other("Local AI returned no answer".into()));
        }
        let tokens = self
            .post(
                "/tokenize",
                json!({"content":format!("{system}\n{text}"),"add_special":true}),
                state,
            )
            .await?;
        if tokens["tokens"].as_array().map_or(true, |t| t.len() > 5800) {
            return Err(AppError::Invalid(TOO_LONG.into()));
        }
        let mut body = json!({"model":self.model,"messages":messages,"stream":false,"max_tokens":2000,"temperature":0,"chat_template_kwargs":{"enable_thinking":false}});
        if let Some(schema) = schema {
            body["response_format"] = json!({"type":"json_schema","json_schema":{"name":"result","strict":true,"schema":schema}});
        }
        let value = self.post("/v1/chat/completions", body, state).await?;
        if value["choices"][0]["finish_reason"] == "length" {
            return Err(AppError::Other(
                "The model reached its answer limit. Try a more focused question.".into(),
            ));
        }
        value["choices"][0]["message"]["content"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .map(str::to_owned)
            .ok_or_else(|| AppError::Other("Local AI returned no answer".into()))
    }
    pub async fn embed(&self, text: &str, state: &AiState) -> AppResult<Vec<f32>> {
        let value = self
            .post(
                "/v1/embeddings",
                json!({"model":self.model,"input":text,"encoding_format":"float"}),
                state,
            )
            .await?;
        let vector: Vec<f32> = serde_json::from_value(value["data"][0]["embedding"].clone())?;
        validate_vector(&vector)?;
        Ok(vector)
    }
    /// Embed several texts in one request, in order.
    pub async fn embed_many(&self, texts: &[String], state: &AiState) -> AppResult<Vec<Vec<f32>>> {
        let value = self
            .post(
                "/v1/embeddings",
                json!({"model":self.model,"input":texts,"encoding_format":"float"}),
                state,
            )
            .await?;
        let mut items = value["data"]
            .as_array()
            .ok_or_else(|| AppError::Other("Local AI returned no search vectors".into()))?
            .iter()
            .map(|item| {
                let index = item["index"].as_u64().unwrap_or(u64::MAX) as usize;
                let vector: Vec<f32> = serde_json::from_value(item["embedding"].clone())?;
                validate_vector(&vector)?;
                Ok((index, vector))
            })
            .collect::<AppResult<Vec<_>>>()?;
        items.sort_by_key(|(index, _)| *index);
        if items.len() != texts.len() || items.iter().enumerate().any(|(i, (index, _))| i != *index) {
            return Err(AppError::Other("Local AI returned the wrong number of search vectors".into()));
        }
        Ok(items.into_iter().map(|(_, vector)| vector).collect())
    }
    pub async fn stop(self) -> AppResult<()> {
        let child = match &self.child {
            Some(shared) => shared
                .lock()
                .map_err(|_| AppError::Other("AI process state unavailable".into()))?
                .take(),
            None => None,
        };
        if let Some(mut child) = child {
            child.kill().await?;
            child.wait().await?;
        }
        Ok(())
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        if let Some(shared) = &self.child {
            if let Ok(mut guard) = shared.lock() {
                if let Some(mut child) = guard.take() {
                    if let Err(error) = child.start_kill() {
                        log::warn!("AI process cleanup: {error}");
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    #[ignore = "requires TIDY_AI_MODEL_DIR with the pinned model downloads"]
    async fn real_runtime_roundtrip_and_cancellation() {
        let folder =
            PathBuf::from(std::env::var("TIDY_AI_MODEL_DIR").expect("model fixture directory"));
        let state = AiState::default();
        let _operation = state.begin("integration test").unwrap();
        let engine =
            Engine::start_builtin(None, folder.join("nomic-embed.gguf"), "nomic-embed", &state)
                .await
                .unwrap();
        let vector = engine
            .embed("search_document: The launch budget is $500.", &state)
            .await
            .unwrap();
        assert_eq!(vector.len(), 768);
        engine.stop().await.unwrap();
        assert!(state.process.lock().unwrap().is_none());
        let engine = Engine::start_builtin(None, folder.join("qwen3-4b.gguf"), "qwen3-4b", &state)
            .await
            .unwrap();
        let schema = json!({"type":"object","properties":{"answer":{"type":"string"}},"required":["answer"],"additionalProperties":false});
        let raw = engine
            .chat(
                "Answer only from the transcript. Return JSON with an answer field.",
                "Transcript: The launch budget is $500. Question: What is the budget?",
                Some(schema),
                &state,
            )
            .await
            .unwrap();
        let answer: Value = serde_json::from_str(&raw).unwrap();
        assert!(answer["answer"].as_str().unwrap().contains("500"));
        assert!(engine
            .chat("Answer this", &"word ".repeat(7000), None, &state)
            .await
            .is_err());
        state
            .cancel
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(engine
            .chat("Summarise", "The budget is $500", None, &state)
            .await
            .is_err());
        drop(engine);
        assert!(state.process.lock().unwrap().is_none());
    }
}
