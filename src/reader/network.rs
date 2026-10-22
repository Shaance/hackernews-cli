//! OpenRouter transport. The endpoint is injected at construction in boundary tests.
use super::{
    config::Profile, sources, summary_prompts, Article, ArticleCapture, Discussion,
    DiscussionCapture, Failure,
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
            response_log_dir: std::env::var_os("XDG_STATE_HOME")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
                .map(|root| root.join("hn/openrouter")),
        }
    }
}
impl Network {
    pub async fn article(
        &self,
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
            .ok_or_else(|| Failure::new("Empty article summary. r Retry"))?;
        Ok(Article {
            text: sources::clean(summary),
        })
    }
    pub async fn discussion(
        &self,
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
        if profile == Profile::Fast {
            schema["properties"]["points"]["maxItems"] = json!(3);
        }
        let prompt = match profile {
            Profile::Fast => summary_prompts::FAST_DISCUSSION,
            Profile::Thorough => summary_prompts::THOROUGH_DISCUSSION,
        };
        let output = self
            .complete(
                key,
                profile,
                prompt,
                json!({"title":captured.title,"post":captured.post,"comments":comments}),
                schema,
            )
            .await?;
        let mut output: Discussion = serde_json::from_value(output)
            .map_err(|_| Failure::new("Discussion response could not be read. r Retry"))?;
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
                "Discussion citations could not be verified. r Retry",
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
        key: &str,
        profile: Profile,
        prompt: &str,
        input: Value,
        schema: Value,
    ) -> Result<Value, Failure> {
        let body = json!({"model":profile.model(),"messages":[{"role":"system","content":prompt},{"role":"user","content":input.to_string()}],
            "max_tokens":4096,"reasoning":{"effort":"low","exclude":true},
            "provider":{"require_parameters":true},"response_format":{"type":"json_schema","json_schema":{"name":"summary","strict":true,"schema":schema}}});
        let response = self
            .client
            .post(&self.endpoint)
            .bearer_auth(key)
            .json(&body)
            .send()
            .await
            .map_err(|_| Failure::new("Cannot reach OpenRouter. r Retry"))?;
        let status = response.status();
        let request_id = response
            .headers()
            .get("x-request-id")
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
                "model": profile.model(), "profile": profile.label(),
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
                    message: "OpenRouter rejected the key. M Edit key".into(),
                    auth: true,
                },
                402 => Failure::new("OpenRouter credits unavailable. Add credits, then r Retry"),
                429 => Failure::new("OpenRouter is rate limited. Try r again shortly"),
                _ => Failure::new(format!(
                    "OpenRouter returned HTTP {}. r Retry",
                    status.as_u16()
                )),
            });
        }
        let value =
            value.ok_or_else(|| Failure::new("OpenRouter response could not be read. r Retry"))?;
        // OpenRouter can commit HTTP 200 before an upstream provider fails.
        if value.get("error").is_some_and(|error| !error.is_null()) {
            return Err(Failure::new(
                "OpenRouter provider could not complete the summary. r Retry",
            ));
        }
        if value["choices"][0]["finish_reason"] == "length" {
            return Err(Failure::new("Summary was cut short. r Retry"));
        }
        let text = value["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| Failure::new("OpenRouter returned no summary. r Retry"))?;
        serde_json::from_str(text).map_err(|_| Failure::new("Summary format was invalid. r Retry"))
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
