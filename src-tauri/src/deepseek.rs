use crate::{
    models::{DeepSeekConnectionResult, DeepSeekModelInfo, DeepSeekSettings, DeepSeekStreamEvent},
    storage::{now_string, path_to_string, read_json, write_json_atomic},
};
use futures_util::StreamExt;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Runtime};
use windows::{
    core::PCWSTR,
    Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
        CRED_TYPE_GENERIC,
    },
};

const DEEPSEEK_API_BASE: &str = "https://api.deepseek.com";
const DEEPSEEK_CREDENTIAL_TARGET: &str = "Ganmaoyuan.DeepSeek.ApiKey";
const DEEPSEEK_SETTINGS_FILE: &str = "deepseek-settings.json";
pub const DEEPSEEK_STREAM_EVENT: &str = "deepseek-stream";

static ACTIVE_STREAMS: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();

fn active_streams() -> &'static Mutex<HashMap<String, Arc<AtomicBool>>> {
    ACTIVE_STREAMS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct DeepSeekSettingsFile {
    #[serde(default)]
    selected_model_id: String,
    #[serde(default)]
    last_tested_at: String,
}

#[derive(Debug, Deserialize)]
struct DeepSeekModelsResponse {
    data: Vec<DeepSeekModelItem>,
}

#[derive(Debug, Deserialize)]
struct DeepSeekModelItem {
    id: String,
    #[serde(default)]
    owned_by: String,
}

#[derive(Debug, Deserialize)]
struct DeepSeekChatResponse {
    choices: Vec<DeepSeekChatChoice>,
}

#[derive(Debug, Deserialize)]
struct DeepSeekChatChoice {
    message: DeepSeekChatMessage,
}

#[derive(Debug, Deserialize)]
struct DeepSeekChatMessage {
    #[serde(default)]
    content: String,
}

pub fn load_settings<R: Runtime>(app: &AppHandle<R>) -> Result<DeepSeekSettings, String> {
    let settings = read_settings_file(settings_path(app)?)?;
    Ok(DeepSeekSettings {
        has_api_key: load_api_key()?.is_some(),
        selected_model_id: settings.selected_model_id,
        last_tested_at: settings.last_tested_at,
    })
}

pub fn save_api_key<R: Runtime>(
    app: &AppHandle<R>,
    api_key: String,
    selected_model_id: String,
) -> Result<DeepSeekSettings, String> {
    let trimmed = api_key.trim();
    if trimmed.is_empty() {
        return Err("DeepSeek API Key 不能为空。".to_string());
    }
    write_credential(trimmed)?;
    let mut settings = read_settings_file(settings_path(app)?)?;
    if !selected_model_id.trim().is_empty() {
        settings.selected_model_id = selected_model_id.trim().to_string();
    }
    write_settings_file(settings_path(app)?, &settings)?;
    load_settings(app)
}

pub fn delete_api_key<R: Runtime>(app: &AppHandle<R>) -> Result<DeepSeekSettings, String> {
    delete_credential()?;
    let mut settings = read_settings_file(settings_path(app)?)?;
    settings.last_tested_at.clear();
    write_settings_file(settings_path(app)?, &settings)?;
    load_settings(app)
}

pub async fn list_models() -> Result<Vec<DeepSeekModelInfo>, String> {
    let api_key = load_api_key()?.ok_or_else(|| "尚未保存 DeepSeek API Key。".to_string())?;
    let response = reqwest_client()?
        .get(format!("{DEEPSEEK_API_BASE}/models"))
        .header(AUTHORIZATION, format!("Bearer {api_key}"))
        .send()
        .await
        .map_err(map_network_error)?;
    let status = response.status();
    if !status.is_success() {
        return Err(http_status_error(status, "获取 DeepSeek 模型列表失败"));
    }
    let payload: DeepSeekModelsResponse = response
        .json()
        .await
        .map_err(|err| format!("解析 DeepSeek 模型列表失败：{err}"))?;
    Ok(payload
        .data
        .into_iter()
        .map(|item| DeepSeekModelInfo {
            id: item.id,
            owned_by: item.owned_by,
        })
        .collect())
}

pub async fn test_connection<R: Runtime>(
    app: &AppHandle<R>,
    selected_model_id: String,
) -> Result<DeepSeekConnectionResult, String> {
    let models = list_models().await?;
    let mut settings = read_settings_file(settings_path(app)?)?;
    if !selected_model_id.trim().is_empty() {
        settings.selected_model_id = selected_model_id.trim().to_string();
    } else if settings.selected_model_id.is_empty() {
        settings.selected_model_id = models
            .first()
            .map(|item| item.id.clone())
            .unwrap_or_default();
    }
    settings.last_tested_at = now_string();
    write_settings_file(settings_path(app)?, &settings)?;
    Ok(DeepSeekConnectionResult {
        settings: load_settings(app)?,
        models,
    })
}

pub fn selected_model_id<R: Runtime>(app: &AppHandle<R>) -> Result<String, String> {
    Ok(read_settings_file(settings_path(app)?)?.selected_model_id)
}

pub fn model_supports_vision(model_id: &str) -> bool {
    let normalized = model_id.trim().to_ascii_lowercase();
    normalized == "deepseek-flash"
        || normalized.contains("v4-flash")
        || normalized.contains("flash-vision")
}

pub fn stop_stream(project_root: &str) {
    if let Ok(mut guard) = active_streams().lock() {
        if let Some(flag) = guard.remove(&stream_project_key(project_root)) {
            flag.store(true, Ordering::SeqCst);
        }
    }
}

pub fn has_active_stream(project_root: &str) -> bool {
    active_streams()
        .lock()
        .map(|guard| {
            guard
                .get(&stream_project_key(project_root))
                .is_some_and(|flag| !flag.load(Ordering::SeqCst))
        })
        .unwrap_or(false)
}

pub fn start_stream_guard(project_root: &str) -> Result<Arc<AtomicBool>, String> {
    let token = Arc::new(AtomicBool::new(false));
    let mut guard = active_streams()
        .lock()
        .map_err(|_| "DeepSeek 流状态锁已损坏，请重试。".to_string())?;
    if let Some(previous) = guard.insert(stream_project_key(project_root), token.clone()) {
        previous.store(true, Ordering::SeqCst);
    }
    Ok(token)
}

pub fn finish_stream_guard(project_root: &str) {
    if let Ok(mut guard) = active_streams().lock() {
        guard.remove(&stream_project_key(project_root));
    }
}

fn stream_project_key(project_root: &str) -> String {
    let path = PathBuf::from(project_root);
    let normalized = path.canonicalize().unwrap_or(path);
    let value = path_to_string(&normalized);
    #[cfg(windows)]
    {
        value.to_ascii_lowercase()
    }
    #[cfg(not(windows))]
    {
        value
    }
}

pub async fn stream_chat<R: Runtime, F>(
    app: AppHandle<R>,
    project_root: String,
    message_id: String,
    model_id: String,
    messages: serde_json::Value,
    cancel_flag: Arc<AtomicBool>,
    mut persist_progress: F,
) -> Result<String, String>
where
    F: FnMut(&str) -> Result<(), String>,
{
    let api_key = load_api_key()?.ok_or_else(|| "尚未保存 DeepSeek API Key。".to_string())?;
    let response = reqwest_client()?
        .post(format!("{DEEPSEEK_API_BASE}/chat/completions"))
        .header(AUTHORIZATION, format!("Bearer {api_key}"))
        .header(CONTENT_TYPE, "application/json")
        .json(&json!({
            "model": model_id,
            "stream": true,
            "messages": messages,
        }))
        .send()
        .await
        .map_err(map_network_error)?;

    let status = response.status();
    if !status.is_success() {
        return Err(http_status_error(status, "DeepSeek 请求失败"));
    }

    let mut full_text = String::new();
    let mut buffer = Vec::new();
    let mut last_persisted_at: Option<Instant> = None;
    let mut last_persisted_len = 0_usize;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        if cancel_flag.load(Ordering::SeqCst) {
            emit_stream_event(
                &app,
                DeepSeekStreamEvent {
                    project_root: project_root.clone(),
                    message_id: message_id.clone(),
                    status: "cancelled".to_string(),
                    delta: String::new(),
                    text: full_text.clone(),
                    error: "已停止生成。".to_string(),
                    model_id: model_id.clone(),
                },
            )?;
            return Err("已停止生成。".to_string());
        }

        let chunk = chunk.map_err(map_network_error)?;
        for frame in append_sse_chunk(&mut buffer, &chunk)? {
            for line in frame.lines() {
                let line = line.trim();
                if !line.starts_with("data:") {
                    continue;
                }
                let payload = line.trim_start_matches("data:").trim();
                if payload == "[DONE]" {
                    emit_stream_event(
                        &app,
                        DeepSeekStreamEvent {
                            project_root: project_root.clone(),
                            message_id: message_id.clone(),
                            status: "completed".to_string(),
                            delta: String::new(),
                            text: full_text.clone(),
                            error: String::new(),
                            model_id: model_id.clone(),
                        },
                    )?;
                    return Ok(full_text);
                }
                let value: serde_json::Value = serde_json::from_str(payload)
                    .map_err(|err| format!("解析 DeepSeek 流响应失败：{err}"))?;
                let delta = value["choices"][0]["delta"]["content"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                if delta.is_empty() {
                    continue;
                }
                full_text.push_str(&delta);
                let now = Instant::now();
                let should_persist = last_persisted_at.is_none()
                    || full_text.len().saturating_sub(last_persisted_len) >= 512
                    || last_persisted_at.is_some_and(|previous| {
                        now.duration_since(previous) >= Duration::from_millis(500)
                    });
                if should_persist {
                    persist_progress(&full_text)?;
                    last_persisted_at = Some(now);
                    last_persisted_len = full_text.len();
                }
                emit_stream_event(
                    &app,
                    DeepSeekStreamEvent {
                        project_root: project_root.clone(),
                        message_id: message_id.clone(),
                        status: "streaming".to_string(),
                        delta,
                        text: full_text.clone(),
                        error: String::new(),
                        model_id: model_id.clone(),
                    },
                )?;
            }
        }
    }

    Err("DeepSeek 流式响应意外结束。".to_string())
}

fn append_sse_chunk(buffer: &mut Vec<u8>, chunk: &[u8]) -> Result<Vec<String>, String> {
    buffer.extend_from_slice(chunk);
    let mut frames = Vec::new();

    while let Some(index) = buffer.windows(2).position(|window| window == b"\n\n") {
        let frame = buffer[..index].to_vec();
        buffer.drain(..index + 2);
        frames.push(
            String::from_utf8(frame)
                .map_err(|err| format!("解析 DeepSeek 流响应失败：SSE 帧不是有效 UTF-8：{err}"))?,
        );
    }

    Ok(frames)
}

pub async fn complete_chat(
    model_id: String,
    messages: serde_json::Value,
) -> Result<String, String> {
    let api_key = load_api_key()?.ok_or_else(|| "尚未保存 DeepSeek API Key。".to_string())?;
    let response = reqwest_client()?
        .post(format!("{DEEPSEEK_API_BASE}/chat/completions"))
        .header(AUTHORIZATION, format!("Bearer {api_key}"))
        .header(CONTENT_TYPE, "application/json")
        .json(&json!({
            "model": model_id,
            "stream": false,
            "messages": messages,
            "response_format": { "type": "json_object" },
        }))
        .send()
        .await
        .map_err(map_network_error)?;

    let status = response.status();
    if !status.is_success() {
        return Err(http_status_error(status, "DeepSeek 请求失败"));
    }
    let payload: DeepSeekChatResponse = response
        .json()
        .await
        .map_err(|err| format!("解析 DeepSeek 项目理解响应失败：{err}"))?;
    payload
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.message.content)
        .filter(|content| !content.trim().is_empty())
        .ok_or_else(|| "DeepSeek 项目理解响应为空。".to_string())
}

pub fn emit_stream_event<R: Runtime>(
    app: &AppHandle<R>,
    payload: DeepSeekStreamEvent,
) -> Result<(), String> {
    app.emit(DEEPSEEK_STREAM_EVENT, payload)
        .map_err(|err| format!("发送 DeepSeek 流事件失败：{err}"))
}

fn reqwest_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(12))
        .timeout(Duration::from_secs(180))
        .build()
        .map_err(|err| format!("创建 DeepSeek 客户端失败：{err}"))
}

fn map_network_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        return "DeepSeek 请求超时，请稍后重试。".to_string();
    }
    if error.is_connect() {
        return "无法连接 DeepSeek，请检查网络。".to_string();
    }
    "DeepSeek 请求失败，请检查网络后重试。".to_string()
}

fn http_status_error(status: reqwest::StatusCode, prefix: &str) -> String {
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return "DeepSeek 认证失败（401），请检查 API Key。".to_string();
    }
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return "DeepSeek 请求过于频繁（429），请稍后再试。".to_string();
    }
    format!("{prefix}（HTTP {}）。", status.as_u16())
}

fn settings_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    Ok(crate::storage::global_data_dir(app)?.join(DEEPSEEK_SETTINGS_FILE))
}

fn read_settings_file(path: PathBuf) -> Result<DeepSeekSettingsFile, String> {
    if !path.exists() {
        return Ok(DeepSeekSettingsFile::default());
    }
    read_json(&path)
}

fn write_settings_file(path: PathBuf, value: &DeepSeekSettingsFile) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "创建 DeepSeek 设置目录失败：{}：{err}",
                path_to_string(parent)
            )
        })?;
    }
    write_json_atomic(&path, value)
}

fn load_api_key() -> Result<Option<String>, String> {
    let target = wide_null(DEEPSEEK_CREDENTIAL_TARGET);
    let mut credential = std::ptr::null_mut();
    match unsafe {
        CredReadW(
            PCWSTR(target.as_ptr()),
            CRED_TYPE_GENERIC,
            Some(0),
            &mut credential,
        )
    } {
        Ok(()) => {
            let credential_ref = unsafe { &*credential };
            let bytes = unsafe {
                std::slice::from_raw_parts(
                    credential_ref.CredentialBlob,
                    credential_ref.CredentialBlobSize as usize,
                )
            };
            let text = String::from_utf8(bytes.to_vec())
                .map_err(|err| format!("读取 DeepSeek Key 失败：{err}"))?;
            unsafe { CredFree(credential.cast()) };
            Ok(Some(text))
        }
        Err(_) => Ok(None),
    }
}

fn write_credential(value: &str) -> Result<(), String> {
    let target = wide_null(DEEPSEEK_CREDENTIAL_TARGET);
    let mut blob = value.as_bytes().to_vec();
    let mut credential = CREDENTIALW::default();
    credential.Type = CRED_TYPE_GENERIC;
    credential.TargetName = windows::core::PWSTR(target.as_ptr() as *mut _);
    credential.CredentialBlobSize = blob.len() as u32;
    credential.CredentialBlob = blob.as_mut_ptr();
    credential.Persist = CRED_PERSIST_LOCAL_MACHINE;
    unsafe { CredWriteW(&credential, 0) }
        .map_err(|err| format!("写入 Windows Credential Manager 失败：{err}"))
}

fn delete_credential() -> Result<(), String> {
    let target = wide_null(DEEPSEEK_CREDENTIAL_TARGET);
    let _ = unsafe { CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, Some(0)) };
    Ok(())
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::{append_sse_chunk, http_status_error, model_supports_vision};

    #[test]
    fn identifies_deepseek_vision_models_without_promoting_text_models() {
        assert!(model_supports_vision("deepseek-flash"));
        assert!(model_supports_vision("deepseek-v4-flash-vision-exp"));
        assert!(!model_supports_vision("deepseek-chat"));
        assert!(!model_supports_vision("deepseek-reasoner"));
    }

    #[test]
    fn maps_rate_limit_status_to_friendly_message() {
        let message =
            http_status_error(reqwest::StatusCode::TOO_MANY_REQUESTS, "DeepSeek 请求失败");

        assert_eq!(message, "DeepSeek 请求过于频繁（429），请稍后再试。");
    }

    #[test]
    fn reduces_provider_failures_to_status_without_response_content() {
        let message = http_status_error(reqwest::StatusCode::BAD_REQUEST, "DeepSeek 请求失败");
        assert!(message.contains("HTTP 400"));
    }

    #[test]
    fn preserves_utf8_characters_split_across_sse_chunks() {
        let frame = "data: {\"choices\":[{\"delta\":{\"content\":\"证据保留期限\"}}]}\n\n";
        let split = frame.find("保留").unwrap() + 1;
        let mut buffer = Vec::new();

        assert!(append_sse_chunk(&mut buffer, &frame.as_bytes()[..split])
            .unwrap()
            .is_empty());
        let frames = append_sse_chunk(&mut buffer, &frame.as_bytes()[split..]).unwrap();

        assert_eq!(frames.len(), 1);
        assert!(frames[0].contains("证据保留期限"));
        assert!(!frames[0].contains('\u{fffd}'));
        assert!(buffer.is_empty());
    }
}
