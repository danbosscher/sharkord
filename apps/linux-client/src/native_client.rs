use anyhow::{Context, Result, anyhow};
use futures_util::StreamExt;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct NativeChannel {
    pub id: u64,
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NativeUser {
    pub id: u64,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NativeReplyPreview {
    pub id: u64,
    #[serde(rename = "userId")]
    pub user_id: u64,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NativeFile {
    pub id: u64,
    pub name: String,
    #[serde(rename = "originalName")]
    pub original_name: String,
    pub extension: String,
    pub size: u64,
    #[serde(rename = "_accessToken")]
    pub access_token: Option<String>,
    #[serde(rename = "_accessTokenExpiresAt")]
    pub access_token_expires_at: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NativeTempFile {
    pub id: String,
    #[serde(rename = "originalName")]
    pub original_name: String,
    pub size: u64,
    pub extension: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NativeBootstrap {
    #[serde(rename = "serverName")]
    pub server_name: String,
    #[serde(rename = "ownUserId")]
    pub own_user_id: u64,
    pub channels: Vec<NativeChannel>,
    pub users: Vec<NativeUser>,
    #[serde(rename = "readStates", default)]
    pub read_states: BTreeMap<u64, u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NativeMessage {
    pub id: u64,
    #[serde(rename = "userId")]
    pub user_id: u64,
    pub content: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "parentMessageId")]
    pub parent_message_id: Option<u64>,
    #[serde(rename = "replyToMessageId")]
    pub reply_to_message_id: Option<u64>,
    #[serde(rename = "replyCount")]
    pub reply_count: Option<u64>,
    #[serde(rename = "replyTo")]
    pub reply_to: Option<NativeReplyPreview>,
    pub files: Vec<NativeFile>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NativeMessagesResponse {
    pub messages: Vec<NativeMessage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NativeSearchMessage {
    pub id: u64,
    #[serde(rename = "channelId")]
    pub channel_id: u64,
    #[serde(rename = "channelName")]
    pub channel_name: String,
    #[serde(rename = "channelIsDm")]
    pub channel_is_dm: bool,
    #[serde(rename = "plainContent")]
    pub plain_content: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "parentMessageId")]
    pub parent_message_id: Option<u64>,
    #[serde(rename = "replyToMessageId")]
    pub reply_to_message_id: Option<u64>,
    #[serde(rename = "replyCount")]
    pub reply_count: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NativeSearchFile {
    pub file: NativeFile,
    #[serde(rename = "channelId")]
    pub channel_id: u64,
    #[serde(rename = "channelName")]
    pub channel_name: String,
    #[serde(rename = "messageId")]
    pub message_id: u64,
    #[serde(rename = "messageCreatedAt")]
    pub message_created_at: i64,
    #[serde(rename = "messageContent")]
    pub message_content: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NativeSearchResults {
    pub messages: Vec<NativeSearchMessage>,
    pub files: Vec<NativeSearchFile>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SendMessageResponse {
    #[serde(rename = "messageId")]
    pub message_id: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NativeEventEnvelope {
    #[serde(rename = "type")]
    pub event_type: String,
    pub payload: Value,
}

#[derive(Debug, Deserialize)]
struct LoginResponse {
    token: String,
}

#[derive(Debug, Clone)]
pub struct NativeClient {
    base_url: String,
    http: reqwest::Client,
}

impl NativeClient {
    pub fn new(base_url: impl Into<String>) -> Result<Self> {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        let http = reqwest::Client::builder()
            .user_agent("sharkord-linux-client/0.1")
            .build()
            .context("failed to build HTTP client")?;

        Ok(Self { base_url, http })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn login(&self, username: &str, password: &str) -> Result<String> {
        let response = self
            .http
            .post(format!("{}/login", self.base_url))
            .header(CONTENT_TYPE, "application/json")
            .json(&json!({
                "identity": username,
                "password": password
            }))
            .send()
            .await
            .context("failed to call /login")?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("login failed: {body}"));
        }

        let parsed: LoginResponse = response
            .json()
            .await
            .context("failed to decode login response")?;

        Ok(parsed.token)
    }

    pub async fn bootstrap(
        &self,
        token: &str,
        server_password: Option<&str>,
    ) -> Result<NativeBootstrap> {
        let mut body = serde_json::Map::new();

        if let Some(password) = server_password.filter(|password| !password.is_empty()) {
            body.insert("password".to_string(), Value::String(password.to_string()));
        }

        let response = self
            .http
            .post(format!("{}/native/bootstrap", self.base_url))
            .header(AUTHORIZATION, bearer(token))
            .json(&Value::Object(body))
            .send()
            .await
            .context("failed to call /native/bootstrap")?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("bootstrap failed: {body}"));
        }

        response
            .json()
            .await
            .context("failed to decode bootstrap response")
    }

    pub async fn fetch_messages(
        &self,
        token: &str,
        channel_id: u64,
    ) -> Result<NativeMessagesResponse> {
        self.fetch_messages_with_target(token, channel_id, None)
            .await
    }

    pub async fn fetch_messages_with_target(
        &self,
        token: &str,
        channel_id: u64,
        target_message_id: Option<u64>,
    ) -> Result<NativeMessagesResponse> {
        let response = self
            .http
            .post(format!("{}/native/messages/list", self.base_url))
            .header(AUTHORIZATION, bearer(token))
            .json(&json!({
                "channelId": channel_id,
                "targetMessageId": target_message_id,
                "limit": 20
            }))
            .send()
            .await
            .context("failed to call /native/messages/list")?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("message fetch failed: {body}"));
        }

        response
            .json()
            .await
            .context("failed to decode message list response")
    }

    pub async fn get_message(&self, token: &str, message_id: u64) -> Result<NativeMessage> {
        let response = self
            .http
            .post(format!("{}/native/messages/get", self.base_url))
            .header(AUTHORIZATION, bearer(token))
            .json(&json!({
                "messageId": message_id
            }))
            .send()
            .await
            .context("failed to call /native/messages/get")?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("message fetch failed: {body}"));
        }

        response
            .json()
            .await
            .context("failed to decode message response")
    }

    pub async fn fetch_thread_messages(
        &self,
        token: &str,
        parent_message_id: u64,
    ) -> Result<NativeMessagesResponse> {
        let response = self
            .http
            .post(format!("{}/native/messages/thread", self.base_url))
            .header(AUTHORIZATION, bearer(token))
            .json(&json!({
                "parentMessageId": parent_message_id,
                "limit": 50
            }))
            .send()
            .await
            .context("failed to call /native/messages/thread")?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("thread fetch failed: {body}"));
        }

        response
            .json()
            .await
            .context("failed to decode thread response")
    }

    pub async fn send_message(
        &self,
        token: &str,
        channel_id: u64,
        content: &str,
        files: &[String],
        parent_message_id: Option<u64>,
        reply_to_message_id: Option<u64>,
    ) -> Result<SendMessageResponse> {
        let response = self
            .http
            .post(format!("{}/native/messages/send", self.base_url))
            .header(AUTHORIZATION, bearer(token))
            .json(&json!({
                "channelId": channel_id,
                "content": content,
                "files": files,
                "parentMessageId": parent_message_id,
                "replyToMessageId": reply_to_message_id
            }))
            .send()
            .await
            .context("failed to call /native/messages/send")?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("message send failed: {body}"));
        }

        response
            .json()
            .await
            .context("failed to decode send response")
    }

    pub async fn signal_typing(
        &self,
        token: &str,
        channel_id: u64,
        parent_message_id: Option<u64>,
    ) -> Result<()> {
        let response = self
            .http
            .post(format!("{}/native/messages/signal-typing", self.base_url))
            .header(AUTHORIZATION, bearer(token))
            .json(&json!({
                "channelId": channel_id,
                "parentMessageId": parent_message_id
            }))
            .send()
            .await
            .context("failed to call /native/messages/signal-typing")?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("signal typing failed: {body}"));
        }

        Ok(())
    }

    pub async fn upload_temp_file(&self, token: &str, file_path: &Path) -> Result<NativeTempFile> {
        let file_name = file_path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| anyhow!("invalid file path"))?
            .to_string();

        let bytes = std::fs::read(file_path).context("failed to read file bytes")?;
        let content_length = bytes.len();

        let response = self
            .http
            .post(format!("{}/upload", self.base_url))
            .header(CONTENT_TYPE, "application/octet-stream")
            .header("x-file-name", file_name)
            .header("x-token", token)
            .header("content-length", content_length.to_string())
            .body(bytes)
            .send()
            .await
            .context("failed to call /upload")?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("file upload failed: {body}"));
        }

        response
            .json()
            .await
            .context("failed to decode temp file response")
    }

    pub async fn delete_temp_file(&self, token: &str, file_id: &str) -> Result<()> {
        let response = self
            .http
            .post(format!("{}/native/files/delete-temporary", self.base_url))
            .header(AUTHORIZATION, bearer(token))
            .json(&json!({
                "fileId": file_id
            }))
            .send()
            .await
            .context("failed to call /native/files/delete-temporary")?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("temporary file delete failed: {body}"));
        }

        Ok(())
    }

    pub async fn search_messages(&self, token: &str, query: &str) -> Result<NativeSearchResults> {
        let response = self
            .http
            .post(format!("{}/native/messages/search", self.base_url))
            .header(AUTHORIZATION, bearer(token))
            .json(&json!({
                "query": query
            }))
            .send()
            .await
            .context("failed to call /native/messages/search")?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("message search failed: {body}"));
        }

        response
            .json()
            .await
            .context("failed to decode message search response")
    }

    pub async fn edit_message(&self, token: &str, message_id: u64, content: &str) -> Result<()> {
        let response = self
            .http
            .post(format!("{}/native/messages/edit", self.base_url))
            .header(AUTHORIZATION, bearer(token))
            .json(&json!({
                "messageId": message_id,
                "content": content
            }))
            .send()
            .await
            .context("failed to call /native/messages/edit")?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("message edit failed: {body}"));
        }

        Ok(())
    }

    pub async fn delete_message(&self, token: &str, message_id: u64) -> Result<()> {
        let response = self
            .http
            .post(format!("{}/native/messages/delete", self.base_url))
            .header(AUTHORIZATION, bearer(token))
            .json(&json!({
                "messageId": message_id
            }))
            .send()
            .await
            .context("failed to call /native/messages/delete")?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("message delete failed: {body}"));
        }

        Ok(())
    }

    pub async fn stream_events<F>(&self, token: &str, mut on_event: F) -> Result<()>
    where
        F: FnMut(NativeEventEnvelope) -> Result<()>,
    {
        let response = self
            .http
            .get(format!("{}/native/events", self.base_url))
            .header(AUTHORIZATION, bearer(token))
            .send()
            .await
            .context("failed to call /native/events")?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("event stream failed: {body}"));
        }

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("failed to read event stream chunk")?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(index) = buffer.find("\n\n") {
                let frame = buffer[..index].to_string();
                buffer.drain(..index + 2);

                if let Some(event) = parse_event_frame(&frame)? {
                    on_event(event)?;
                }
            }
        }

        Ok(())
    }
}

pub fn first_text_channel_id(bootstrap: &NativeBootstrap) -> Option<u64> {
    bootstrap
        .channels
        .iter()
        .find(|channel| channel.kind == "TEXT")
        .map(|channel| channel.id)
}

pub fn text_channels(bootstrap: &NativeBootstrap) -> Vec<NativeChannel> {
    bootstrap
        .channels
        .iter()
        .filter(|channel| channel.kind == "TEXT")
        .cloned()
        .collect()
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

fn parse_event_frame(frame: &str) -> Result<Option<NativeEventEnvelope>> {
    let normalized = frame.replace("\r\n", "\n");
    let mut data_lines = Vec::new();

    for line in normalized.lines() {
        if line.starts_with(':') || line.is_empty() {
            continue;
        }

        if let Some(data) = line.strip_prefix("data:") {
            data_lines.push(data.trim_start());
        }
    }

    if data_lines.is_empty() {
        return Ok(None);
    }

    let data = data_lines.join("\n");
    let event = serde_json::from_str(&data).context("failed to parse SSE event")?;

    Ok(Some(event))
}
