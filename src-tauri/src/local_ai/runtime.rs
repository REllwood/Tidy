//! A private, short-lived inference process. Dropping it kills the process and frees RAM.
use super::{models, AiState, Config, Progress};
use crate::error::{AppError, AppResult};
use appflower_core::store::meeting_ai::validate_vector;
use serde_json::{json, Value};
use std::{path::PathBuf, process::Stdio, time::Duration};
use tauri::AppHandle;
use tokio::process::{Child, Command};

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
    if cfg!(debug_assertions) {
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
        Self::start_builtin(models::path(app, id)?, id, state).await
    }

    async fn start_builtin(path: PathBuf, id: &str, state: &AiState) -> AppResult<Self> {
        let client = client()?;
        let def = models::model(id)?;
        let embedding = def.purpose == "embedding";
        state.progress(Progress::new("Loading local model", Some(id)));
        models::verify(&path, def, state).await?;
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        drop(listener);
        let key = crate::db::new_id();
        let mut command = Command::new(executable()?);
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
                "512",
                "--ubatch-size",
                "128",
                "--n-gpu-layers",
                "99",
                "--no-webui",
                "--no-warmup",
                "--log-disable",
            ])
            .env("LLAMA_API_KEY", &key)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
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
        let child = command
            .spawn()
            .map_err(|e| AppError::Other(format!("Could not start local AI: {e}")))?;
        *state
            .process
            .lock()
            .map_err(|_| AppError::Other("AI process state unavailable".into()))? = Some(child);
        let engine = Self {
            child: Some(state.process.clone()),
            base: format!("http://127.0.0.1:{port}"),
            key,
            model: id.into(),
            ollama: false,
            client,
        };
        for _ in 0..240 {
            state.check_cancel()?;
            if let Some(shared) = &engine.child {
                let mut guard = shared
                    .lock()
                    .map_err(|_| AppError::Other("AI process state unavailable".into()))?;
                let child = guard
                    .as_mut()
                    .ok_or_else(|| AppError::Other("Local AI was stopped".into()))?;
                if child.try_wait()?.is_some() {
                    return Err(AppError::Other(
                        "Local AI could not load the model. Close memory-heavy apps and retry."
                            .into(),
                    ));
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
        Err(AppError::Other(
            "Local AI took too long to start. Retry after closing other apps.".into(),
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
            let mut body = json!({"model":self.model,"messages":messages,"stream":false,"think":false,"keep_alive":0,"options":{"num_ctx":8192,"num_predict":1400,"temperature":0.1}});
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
        if tokens["tokens"].as_array().map_or(true, |t| t.len() > 6200) {
            return Err(AppError::Invalid("This request is too long for the local model. Use a shorter selection or narrower question.".into()));
        }
        let mut body = json!({"model":self.model,"messages":messages,"stream":false,"max_tokens":1400,"temperature":0.1,"chat_template_kwargs":{"enable_thinking":false}});
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
        let engine = Engine::start_builtin(folder.join("nomic-embed.gguf"), "nomic-embed", &state)
            .await
            .unwrap();
        let vector = engine
            .embed("search_document: The launch budget is $500.", &state)
            .await
            .unwrap();
        assert_eq!(vector.len(), 768);
        engine.stop().await.unwrap();
        assert!(state.process.lock().unwrap().is_none());
        let engine = Engine::start_builtin(folder.join("qwen3-4b.gguf"), "qwen3-4b", &state)
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
