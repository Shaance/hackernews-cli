//! Summary provider HTTP boundary. Endpoints are injected in boundary tests.
use super::{
    config::{Profile, Provider},
    sources, summary_prompts, Article, ArticleCapture, Discussion, DiscussionCapture, Failure,
};
use hn_lib::HackerNewsCliServiceImpl;
use reqwest::Client;
use serde_json::{json, Value};
use std::{
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone)]
pub struct Network {
    pub client: Client,
    pub service: HackerNewsCliServiceImpl,
    pub endpoint: String,
    pub anthropic_endpoint: String,
    pub response_log_dir: Option<PathBuf>,
}
impl Default for Network {
    fn default() -> Self {
        Self {
            service: HackerNewsCliServiceImpl::new(),
            client: Client::builder()
                .timeout(Duration::from_secs(90))
                .user_agent("hn-cli/0.1")
                .build()
                .expect("HTTP client"),
            endpoint: "https://openrouter.ai/api/v1/chat/completions".into(),
            anthropic_endpoint: "https://api.anthropic.com/v1/messages".into(),
            response_log_dir: std::env::var_os("XDG_STATE_HOME")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
                .map(|root| root.join("hn/openrouter")),
        }
    }
}
impl Network {
    pub(super) async fn article(
        &self,
        provider: Provider,
        key: &str,
        profile: Profile,
        captured: &ArticleCapture,
    ) -> Result<Article, Failure> {
        if captured.text.trim().is_empty() {
            return Ok(Article {
                text: "No article text to summarize.".into(),
            });
        }
        let schema = json!({"type":"object","properties":{"summary":{"type":"string"}},"required":["summary"],"additionalProperties":false});
        let output = self
            .complete(
                provider,
                key,
                profile,
                summary_prompts::ARTICLE,
                json!({"title": captured.title, "article": captured.text}),
                schema,
            )
            .await?;
        let summary = output["summary"]
            .as_str()
            .filter(|v| !v.trim().is_empty())
            .ok_or_else(|| Failure::new("Empty article summary. M → Enter Retry"))?;
        Ok(Article {
            text: sources::clean(summary),
        })
    }
    pub(super) async fn discussion(
        &self,
        provider: Provider,
        key: &str,
        profile: Profile,
        captured: &DiscussionCapture,
    ) -> Result<Discussion, Failure> {
        let readable: Vec<_> = captured
            .sources
            .iter()
            .filter(|s| !s.excerpt.is_empty())
            .collect();
        if readable.is_empty() {
            return Ok(Discussion {
                overview: "No readable comments to summarize yet.".into(),
                points: vec![],
            });
        }
        let comments: Vec<_> = readable
            .iter()
            .map(|s| json!({"id":s.id,"parent":s.parent,"author":s.author,"text":s.excerpt}))
            .collect();
        let mut schema = json!({"type":"object","properties":{"overview":{"type":"string"},"points":{"type":"array","minItems":1,"items":{"type":"object","properties":{"text":{"type":"string"},"sources":{"type":"array","minItems":1,"items":{"type":"integer","enum":readable.iter().map(|source| source.id).collect::<Vec<_>>()}}},"required":["text","sources"],"additionalProperties":false}}},"required":["overview","points"],"additionalProperties":false});
        // Native Anthropic JSON outputs do not support maxItems; the Fast prompt still asks for 1–3.
        if profile == Profile::Fast && provider == Provider::OpenRouter {
            schema["properties"]["points"]["maxItems"] = json!(3);
        }
        let prompt = match profile {
            Profile::Fast => summary_prompts::FAST_DISCUSSION,
            Profile::Thorough => summary_prompts::THOROUGH_DISCUSSION,
        };
        let output = self
            .complete(
                provider,
                key,
                profile,
                prompt,
                json!({"title":captured.title,"post":captured.post,"comments":comments}),
                schema,
            )
            .await?;
        let mut output: Discussion = serde_json::from_value(output)
            .map_err(|_| Failure::new("Discussion response could not be read. M → Enter Retry"))?;
        if output.points.is_empty()
            || output.points.iter().any(|p| {
                p.text.trim().is_empty()
                    || p.sources.is_empty()
                    || p.sources
                        .iter()
                        .any(|id| !readable.iter().any(|s| s.id == *id))
            })
        {
            return Err(Failure::new(
                "Discussion citations could not be verified. M → Enter Retry",
            ));
        }
        output.overview = sources::clean(&output.overview);
        for point in &mut output.points {
            point.text = sources::clean(&point.text);
        }
        Ok(output)
    }
    async fn complete(
        &self,
        provider: Provider,
        key: &str,
        profile: Profile,
        prompt: &str,
        input: Value,
        schema: Value,
    ) -> Result<Value, Failure> {
        let label = provider.label();
        let body = match provider {
            Provider::OpenRouter => {
                json!({"model":profile.model(provider),"messages":[{"role":"system","content":prompt},{"role":"user","content":input.to_string()}],
                "max_tokens":4096,"reasoning":{"effort":"low","exclude":true},
                "provider":{"require_parameters":true},"response_format":{"type":"json_schema","json_schema":{"name":"summary","strict":true,"schema":schema}}})
            }
            Provider::Anthropic => json!({"model":profile.model(provider),"system":prompt,
                "messages":[{"role":"user","content":input.to_string()}],"max_tokens":4096,
                "thinking":{"type":"adaptive","display":"omitted"},
                "output_config":{"effort":"low","format":{"type":"json_schema","schema":schema}}}),
        };
        let request = match provider {
            Provider::OpenRouter => self.client.post(&self.endpoint),
            Provider::Anthropic => self
                .client
                .post(&self.anthropic_endpoint)
                .header("anthropic-version", "2026-10-21"),
        };
        let response = request
            .bearer_auth(key)
            .json(&body)
            .send()
            .await
            .map_err(|_| Failure::new(format!("Cannot reach {label}. M → Enter Retry")))?;
        let status = response.status();
        let request_id = response
            .headers()
            .get(match provider {
                Provider::OpenRouter => "x-request-id",
                Provider::Anthropic => "request-id",
            })
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let raw = response.bytes().await;
        let value: Option<Value> = raw
            .as_ref()
            .ok()
            .and_then(|bytes| serde_json::from_slice(bytes).ok());
        if let (Some(directory), Ok(raw)) = (&self.response_log_dir, &raw) {
            let record = json!({
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "provider": label, "model": profile.model(provider), "profile": profile.label(),
                "section": if input.get("comments").is_some() { "discussion" } else { "article" },
                "title": input.get("title").and_then(Value::as_str), "http_status": status.as_u16(),
                "request_id": request_id, "generation_id": value.as_ref().and_then(|v| v.get("id")).and_then(Value::as_str),
                "raw_response": String::from_utf8_lossy(raw),
            });
            // Diagnostics must never turn a successful summary into a failure.
            let _ = write_response_log(directory, record, key);
        }
        if !status.is_success() {
            return Err(match status.as_u16() {
                401 | 403 => Failure {
                    message: format!("{label} rejected the key. M Edit key"),
                    auth: true,
                },
                402 => Failure::new(format!(
                    "{label} credits unavailable. Add credits, then M → Enter Retry"
                )),
                429 => Failure::new(format!(
                    "{label} is rate limited. Wait briefly, then M → Enter Retry"
                )),
                _ => Failure::new(format!(
                    "{label} returned HTTP {}. M → Enter Retry",
                    status.as_u16()
                )),
            });
        }
        let value = value.ok_or_else(|| {
            Failure::new(format!(
                "{label} response could not be read. M → Enter Retry"
            ))
        })?;
        // OpenRouter can commit HTTP 200 before an upstream provider fails.
        if value.get("error").is_some_and(|error| !error.is_null()) {
            return Err(Failure::new(format!(
                "{label} provider could not complete the summary. M → Enter Retry"
            )));
        }
        let text = match provider {
            Provider::OpenRouter => {
                if value["choices"][0]["finish_reason"] == "length" {
                    return Err(Failure::new("Summary was cut short. M → Enter Retry"));
                }
                value["choices"][0]["message"]["content"]
                    .as_str()
                    .map(str::to_owned)
            }
            Provider::Anthropic => {
                if value["stop_reason"] == "max_tokens" {
                    return Err(Failure::new("Summary was cut short. M → Enter Retry"));
                }
                value["content"].as_array().map(|blocks| {
                    blocks
                        .iter()
                        .filter(|block| block["type"] == "text")
                        .filter_map(|block| block["text"].as_str())
                        .collect::<String>()
                })
            }
        }
        .filter(|text| !text.is_empty())
        .ok_or_else(|| Failure::new(format!("{label} returned no summary. M → Enter Retry")))?;
        serde_json::from_str(&text)
            .map_err(|_| Failure::new("Summary format was invalid. M → Enter Retry"))
    }
}

fn write_response_log(directory: &Path, mut record: Value, key: &str) -> std::io::Result<()> {
    // Redact at the string-value boundary, including an escaped key in a raw JSON body.
    if !key.is_empty() {
        let escaped = serde_json::to_string(key)?;
        for value in record
            .as_object_mut()
            .expect("diagnostic record is an object")
            .values_mut()
        {
            if let Some(text) = value.as_str() {
                *value = Value::String(
                    text.replace(&escaped[1..escaped.len() - 1], "[REDACTED]")
                        .replace(key, "[REDACTED]"),
                );
            }
        }
    }
    let mut directories = std::fs::DirBuilder::new();
    directories.recursive(true);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
        directories.mode(0o700);
        options.mode(0o600);
    }
    directories.create(directory)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = directory.join(format!("{stamp}-{}.json", std::process::id()));
    let mut file = options.open(path)?;
    file.write_all(&serde_json::to_vec_pretty(&record)?)
}

#[cfg(test)]
#[path = "network_tests.rs"]
pub(super) mod tests;

#[cfg(test)]
#[path = "anthropic_tests.rs"]
mod anthropic_tests;
