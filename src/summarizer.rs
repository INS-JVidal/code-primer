use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::parser::{FileUnits, TranslationUnit};

const SYSTEM_PROMPT: &str = r#"You summarize source code files for a semantic search index.
Given a file path and its parsed symbols (functions, types, constants), write a 3-5 sentence summary of the file's purpose and key functionality.
- Explain what the file does, not how (purpose over mechanics)
- Preserve domain-specific keywords, type names, and API names
- Mention the most important functions/types by name
- Note key dependencies or patterns when relevant
Output ONLY the summary text, no formatting or markdown."#;

// ── Backend trait ──────────────────────────────────────────────────

enum Backend {
    /// Shell out to `claude -p` — uses the user's subscription auth.
    ClaudeCli { model: String },
    /// Direct Anthropic API calls via HTTP.
    Api(ApiBackend),
}

struct ApiBackend {
    client: reqwest::Client,
    model: String,
    api_key: String,
}

pub struct Summarizer {
    backend: Backend,
}

impl Summarizer {
    /// Create a new Summarizer. Prefers `claude` CLI (subscription auth).
    /// Falls back to direct API if ANTHROPIC_API_KEY is set and `--backend api`
    /// is requested.
    pub fn new(model: String, force_api: bool) -> Result<Self> {
        if force_api {
            return Self::new_api(model);
        }

        // Default: try claude CLI
        if which_claude().is_some() {
            return Ok(Self {
                backend: Backend::ClaudeCli { model },
            });
        }

        // Fallback: direct API
        Self::new_api(model)
    }

    fn new_api(model: String) -> Result<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .or_else(|_| std::env::var("ANTHROPIC_AUTH_TOKEN"));

        match api_key {
            Ok(key) => Ok(Self {
                backend: Backend::Api(ApiBackend {
                    client: reqwest::Client::new(),
                    model,
                    api_key: key,
                }),
            }),
            Err(_) => bail!(
                "No auth available.\n\
                 Install `claude` CLI (recommended, uses your subscription),\n\
                 or set ANTHROPIC_API_KEY for direct API access."
            ),
        }
    }

    pub fn backend_name(&self) -> String {
        match &self.backend {
            Backend::ClaudeCli { .. } => {
                let version = claude_version().unwrap_or_else(|| "unknown".into());
                format!("claude CLI v{version} (subscription auth)")
            }
            Backend::Api(_) => "direct API (ANTHROPIC_API_KEY)".into(),
        }
    }

    /// Send a minimal prompt to verify auth works before processing files.
    /// Fails fast with a clear error instead of failing per-file.
    pub async fn preflight_check(&self) -> Result<()> {
        match &self.backend {
            Backend::ClaudeCli { model } => {
                let child = tokio::process::Command::new("claude")
                    .arg("-p")
                    .arg("Reply with the single word OK")
                    .arg("--model")
                    .arg(model)
                    .arg("--output-format")
                    .arg("text")
                    .arg("--no-session-persistence")
                    .arg("--allowed-tools")
                    .arg("")
                    .arg("--max-budget-usd")
                    .arg("0.01")
                    .env_remove("CLAUDECODE")
                    .env_remove("CLAUDE_CODE_ENTRYPOINT")
                    .env_remove("ANTHROPIC_API_KEY")
                    .env_remove("ANTHROPIC_AUTH_TOKEN")
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .context("failed to spawn `claude` CLI — is it installed?")?;

                let output = match tokio::time::timeout(
                    std::time::Duration::from_secs(30),
                    child.wait_with_output(),
                )
                .await
                {
                    Ok(Ok(output)) => output,
                    Ok(Err(e)) => bail!("claude CLI preflight failed: {e}"),
                    Err(_) => bail!("Auth check timed out after 30s — is claude CLI responsive?"),
                };

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                    let detail = if stderr.is_empty() {
                        String::from_utf8_lossy(&output.stdout).trim().to_string()
                    } else {
                        stderr
                    };
                    bail!(
                        "Auth check failed — claude CLI cannot reach the API.\n  \
                         Error: {detail}\n  \
                         Make sure you are logged in: run `claude` interactively first."
                    );
                }
                Ok(())
            }
            Backend::Api(api) => {
                let body = ApiRequest {
                    model: &api.model,
                    max_tokens: 4,
                    system: "Reply with OK",
                    messages: vec![ApiMessage {
                        role: "user",
                        content: "ping",
                    }],
                };

                let resp = api
                    .client
                    .post("https://api.anthropic.com/v1/messages")
                    .header("anthropic-version", "2023-06-01")
                    .header("content-type", "application/json")
                    .header("x-api-key", &api.api_key)
                    .json(&body)
                    .send()
                    .await
                    .context("API preflight: cannot reach Anthropic API")?;

                if !resp.status().is_success() {
                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    let msg = serde_json::from_str::<ApiError>(&text)
                        .ok()
                        .and_then(|e| e.error)
                        .and_then(|e| e.message)
                        .unwrap_or(text);
                    bail!(
                        "Auth check failed — API returned {status}.\n  \
                         Error: {msg}\n  \
                         Check your ANTHROPIC_API_KEY and credit balance."
                    );
                }
                Ok(())
            }
        }
    }

    pub async fn summarize_file(&self, file_units: &FileUnits) -> Result<String> {
        let prompt = build_prompt(file_units);

        match &self.backend {
            Backend::ClaudeCli { model } => call_claude_cli(&prompt, model).await,
            Backend::Api(api) => call_api(api, &prompt).await,
        }
    }
}

// ── Claude CLI backend ─────────────────────────────────────────────

fn claude_version() -> Option<String> {
    let output = std::process::Command::new("claude")
        .arg("--version")
        .output()
        .ok()?;
    if output.status.success() {
        let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
        // Output is typically "claude <version>" or just "<version>"
        let version = raw.strip_prefix("claude ").unwrap_or(&raw);
        if !version.is_empty() {
            return Some(version.to_string());
        }
    }
    None
}

fn which_claude() -> Option<std::path::PathBuf> {
    // Check if `claude` is on PATH and executable.
    // Use `where` on Windows, `which` on Unix.
    let cmd = if cfg!(windows) { "where" } else { "which" };
    let output = std::process::Command::new(cmd)
        .arg("claude")
        .output()
        .ok()?;
    if output.status.success() {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return Some(std::path::PathBuf::from(path));
        }
    }
    None
}

async fn call_claude_cli(prompt: &str, model: &str) -> Result<String> {
    use tokio::io::AsyncWriteExt;

    let mut child = tokio::process::Command::new("claude")
        .arg("-p")
        .arg("--system-prompt")
        .arg(SYSTEM_PROMPT)
        .arg("--model")
        .arg(model)
        .arg("--output-format")
        .arg("text")
        .arg("--no-session-persistence")
        .arg("--allowed-tools")
        .arg("")
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE_ENTRYPOINT")
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("ANTHROPIC_AUTH_TOKEN")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("failed to spawn `claude` CLI — is it installed?")?;

    // Write prompt via stdin to avoid ARG_MAX limits on large files
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(prompt.as_bytes())
            .await
            .context("writing prompt to claude stdin")?;
        // stdin is dropped here, closing the pipe
    }

    // 60-second timeout per file
    let output = match tokio::time::timeout(
        std::time::Duration::from_secs(60),
        child.wait_with_output(),
    )
    .await
    {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => bail!("claude CLI failed: {e}"),
        Err(_) => {
            // Timeout — child is already dropped here which kills it
            bail!("claude CLI timed out after 60s");
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let detail = if stderr.is_empty() {
            // Sometimes errors go to stdout
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        } else {
            stderr
        };
        bail!("claude CLI error (exit {}): {}", output.status, detail);
    }

    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() {
        bail!("claude CLI returned empty response");
    }
    Ok(text)
}

// ── API backend ────────────────────────────────────────────────────

#[derive(Serialize)]
struct ApiRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    system: &'a str,
    messages: Vec<ApiMessage<'a>>,
}

#[derive(Serialize)]
struct ApiMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Deserialize)]
struct ApiResponse {
    content: Vec<ContentBlock>,
    #[allow(dead_code)]
    stop_reason: Option<String>,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    text: Option<String>,
}

#[derive(Deserialize)]
struct ApiError {
    error: Option<ApiErrorDetail>,
}

#[derive(Deserialize)]
struct ApiErrorDetail {
    message: Option<String>,
}

async fn call_api(api: &ApiBackend, prompt: &str) -> Result<String> {
    let req = api
        .client
        .post("https://api.anthropic.com/v1/messages")
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .header("x-api-key", &api.api_key);

    let body = ApiRequest {
        model: &api.model,
        max_tokens: 512,
        system: SYSTEM_PROMPT,
        messages: vec![ApiMessage {
            role: "user",
            content: prompt,
        }],
    };

    let resp = req
        .json(&body)
        .send()
        .await
        .context("sending request to Anthropic API")?;

    let status = resp.status();
    let resp_text = resp.text().await.context("reading Anthropic response")?;

    if !status.is_success() {
        let msg = serde_json::from_str::<ApiError>(&resp_text)
            .ok()
            .and_then(|e| e.error)
            .and_then(|e| e.message)
            .unwrap_or_else(|| resp_text.clone());
        bail!("Anthropic API error ({}): {}", status, msg);
    }

    let api_resp: ApiResponse =
        serde_json::from_str(&resp_text).context("parsing Anthropic response")?;

    let block = api_resp
        .content
        .first()
        .context("empty response from Anthropic")?;

    if block.block_type != "text" {
        bail!("unexpected content type: {}", block.block_type);
    }

    block
        .text
        .as_ref()
        .map(|t| t.trim().to_string())
        .context("no text in response")
}

// ── Shared helpers ─────────────────────────────────────────────────

fn build_prompt(file_units: &FileUnits) -> String {
    let mut parts = vec![format!("File: {}", file_units.path), String::new()];
    for unit in &file_units.units {
        parts.push(format!("- {}", unit_label(unit)));
        if !unit.signature.is_empty() {
            parts.push(format!("  {}", unit.signature));
        }
        if !unit.doc_comment.is_empty() {
            parts.push(format!("  {}", unit.doc_comment));
        }
    }
    parts.join("\n")
}

fn unit_label(unit: &TranslationUnit) -> String {
    if unit.kind == "method" && !unit.receiver.is_empty() {
        return format!("method `{}` on {}", unit.name, unit.receiver);
    }
    if unit.kind == "imports" {
        let preview: String = unit.source.chars().take(200).collect();
        return format!("imports: {preview}");
    }
    format!("{} `{}`", unit.kind, unit.name)
}

pub fn fallback_summary(file_units: &FileUnits) -> String {
    let mut kinds = std::collections::BTreeMap::new();
    for u in &file_units.units {
        *kinds.entry(u.kind).or_insert(0u32) += 1;
    }
    let parts: Vec<String> = kinds
        .iter()
        .map(|(k, v)| format!("{v} {k}(s)"))
        .collect();
    format!("Contains {}.", parts.join(", "))
}
