use std::sync::atomic::{AtomicBool, Ordering};

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

const BILLING_MARKERS: &[&str] = &["credit balance", "insufficient_quota", "billing"];

pub struct Summarizer {
    client: reqwest::Client,
    model: String,
    auth: Auth,
    switched: AtomicBool,
}

struct Auth {
    token: Option<String>,
    api_key: Option<String>,
}

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

impl Summarizer {
    pub fn new(model: String) -> Result<Self> {
        let token = std::env::var("ANTHROPIC_AUTH_TOKEN").ok();
        let api_key = std::env::var("ANTHROPIC_API_KEY").ok();

        if token.is_none() && api_key.is_none() {
            bail!(
                "No Anthropic auth configured.\n\
                 Set ANTHROPIC_AUTH_TOKEN (subscription) or ANTHROPIC_API_KEY (credits)."
            );
        }

        Ok(Self {
            client: reqwest::Client::new(),
            model,
            auth: Auth { token, api_key },
            switched: AtomicBool::new(false),
        })
    }

    pub async fn summarize_file(&self, file_units: &FileUnits) -> Result<String> {
        let prompt = build_prompt(file_units);

        match self.call_api(&prompt).await {
            Ok(text) => Ok(text),
            Err(e) => {
                if is_billing_error(&e) && self.try_switch() {
                    eprintln!("  Switching to subscription auth (ANTHROPIC_AUTH_TOKEN)...");
                    self.call_api(&prompt).await
                } else {
                    Err(e)
                }
            }
        }
    }

    async fn call_api(&self, prompt: &str) -> Result<String> {
        let use_token = self.switched.load(Ordering::Relaxed)
            || (self.auth.token.is_some() && self.auth.api_key.is_none());

        let mut req = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json");

        if use_token {
            if let Some(token) = &self.auth.token {
                req = req.header("Authorization", format!("Bearer {token}"));
            }
        } else if let Some(key) = &self.auth.api_key {
            req = req.header("x-api-key", key);
        }

        let body = ApiRequest {
            model: &self.model,
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

    fn try_switch(&self) -> bool {
        if self.switched.load(Ordering::Relaxed) {
            return false;
        }
        if self.auth.token.is_some() {
            self.switched.store(true, Ordering::Relaxed);
            return true;
        }
        false
    }
}

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

fn is_billing_error(err: &anyhow::Error) -> bool {
    let msg = err.to_string().to_lowercase();
    BILLING_MARKERS.iter().any(|m| msg.contains(m))
}
