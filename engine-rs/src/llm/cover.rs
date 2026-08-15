//! 生图基础设施（cover 基础设施，74 号）——short-fiction-runner.ts 的
//! `resolveCoverGenerationRequest` + `generateImageFromPrompt` 消费面。
//!
//! 三 API 形态：
//! - `images`：POST {base}/images/generations（b64_json 或 url → 下载，401/403
//!   时带 Bearer 重试）
//! - `responses`：POST {base}/responses + `image_generation` tool（result 即
//!   base64，png）
//! - `gemini`：POST {base}/models/{model}:generateContent?key=…（inlineData）
//!
//! 请求解析优先级：环境变量（INKOS_COVER_ENDPOINT / INKOS_COVER_BASE_URL）→
//! inkos.json 的 `llm.cover`（service → preset 表 kkaiapi/openai/google）→
//! Err（"cover endpoint is required…"，端点转 needsCoverConfig 提示）。

use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

use crate::llm::secrets::load_secrets;

// ── preset 表（cover-providers.ts） ──────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverApi {
    Responses,
    Images,
    Gemini,
}

pub struct CoverProviderPreset {
    pub service: &'static str,
    pub label: &'static str,
    pub base_url: &'static str,
    pub api: CoverApi,
    pub default_model: &'static str,
}

pub const COVER_PROVIDER_PRESETS: &[CoverProviderPreset] = &[
    CoverProviderPreset {
        service: "kkaiapi",
        label: "kkaiapi",
        base_url: "https://api.kkaiapi.com/v1",
        api: CoverApi::Images,
        default_model: "gpt-image-2",
    },
    CoverProviderPreset {
        service: "openai",
        label: "OpenAI Images",
        base_url: "https://api.openai.com/v1",
        api: CoverApi::Images,
        default_model: "gpt-image-2",
    },
    CoverProviderPreset {
        service: "google",
        label: "Google Gemini",
        base_url: "https://generativelanguage.googleapis.com/v1beta",
        api: CoverApi::Gemini,
        default_model: "gemini-3.1-flash-image-preview",
    },
];

pub fn resolve_cover_provider_preset(service: &str) -> Option<&'static CoverProviderPreset> {
    COVER_PROVIDER_PRESETS
        .iter()
        .find(|preset| preset.service == service)
}

/// `normalizeCoverBaseUrl`：http(s) + 无 userinfo/query/hash + 去尾斜杠。
pub fn normalize_cover_base_url(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (scheme, rest) = trimmed.split_once("://")?;
    if scheme != "http" && scheme != "https" {
        return None;
    }
    if rest.contains('@') || rest.contains('?') || rest.contains('#') {
        return None;
    }
    Some(trimmed.trim_end_matches('/').to_string())
}

pub fn cover_secret_key(service: &str) -> String {
    format!("cover:{service}")
}

// ── 请求结构（ShortFictionCoverRequest） ─────────────────────────

#[derive(Debug, Clone)]
pub struct CoverGenerationRequest {
    pub api: CoverApi,
    pub base_url: String,
    pub endpoint: Option<String>,
    pub model: String,
    pub api_key: String,
}

/// `resolveCoverGenerationRequest`：env → 项目配置 → preset。
pub async fn resolve_cover_generation_request(root: &Path) -> Result<CoverGenerationRequest, String> {
    let env_endpoint = std::env::var("INKOS_COVER_ENDPOINT").ok().filter(|v| !v.is_empty());
    let env_base = std::env::var("INKOS_COVER_BASE_URL").ok().filter(|v| !v.is_empty());
    if env_endpoint.is_some() || env_base.is_some() {
        let endpoint = match env_endpoint {
            Some(endpoint) => endpoint,
            None => format!(
                "{}/images/generations",
                env_base.as_deref().unwrap_or_default().trim_end_matches('/')
            ),
        };
        let base_url = env_base.unwrap_or_else(|| {
            endpoint
                .trim_end_matches('/')
                .trim_end_matches("/responses")
                .trim_end_matches('/')
                .trim_end_matches("/images/generations")
                .to_string()
        });
        let api = if endpoint.contains("/responses") {
            CoverApi::Responses
        } else {
            CoverApi::Images
        };
        let model = std::env::var("INKOS_COVER_MODEL")
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "gpt-image-2".to_string());
        let api_key = resolve_cover_api_key("INKOS_COVER_API_KEY")?;
        return Ok(CoverGenerationRequest {
            api,
            base_url,
            endpoint: Some(endpoint),
            model,
            api_key,
        });
    }

    let project_cover = read_project_cover_config(root).await;
    let Some(project_cover) = project_cover else {
        return Err(
            "cover endpoint is required. Configure cover generation in Studio or set INKOS_COVER_BASE_URL."
                .to_string(),
        );
    };
    let Some(preset) = resolve_cover_provider_preset(&project_cover.service) else {
        return Err(format!("Unsupported cover service: {}", project_cover.service));
    };
    let api_key = resolve_project_cover_api_key(root, &project_cover.service).await;
    if api_key.is_empty() {
        return Err(format!(
            "Cover API key is required. Configure a cover key for {}.",
            preset.label
        ));
    }
    Ok(CoverGenerationRequest {
        api: preset.api,
        base_url: project_cover
            .base_url
            .unwrap_or_else(|| preset.base_url.to_string()),
        endpoint: None,
        model: project_cover
            .model
            .or_else(|| {
                std::env::var("INKOS_COVER_MODEL")
                    .ok()
                    .filter(|v| !v.is_empty())
            })
            .unwrap_or_else(|| preset.default_model.to_string()),
        api_key,
    })
}

struct ProjectCoverConfig {
    service: String,
    model: Option<String>,
    base_url: Option<String>,
}

async fn read_project_cover_config(root: &Path) -> Option<ProjectCoverConfig> {
    let raw = tokio::fs::read_to_string(root.join("inkos.json")).await.ok()?;
    let parsed: Value = serde_json::from_str(&raw).ok()?;
    let cover = parsed.get("llm")?.get("cover")?;
    let service = cover.get("service")?.as_str()?.trim().to_string();
    if service.is_empty() {
        return None;
    }
    let model = cover
        .get("model")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map(String::from);
    let base_url = cover
        .get("baseUrl")
        .and_then(Value::as_str)
        .and_then(normalize_cover_base_url);
    Some(ProjectCoverConfig {
        service,
        model,
        base_url,
    })
}

async fn resolve_project_cover_api_key(root: &Path, service: &str) -> String {
    let secrets = load_secrets(root).unwrap_or_default();
    if let Some(secret) = secrets.services.get(&cover_secret_key(service)) {
        if !secret.api_key.is_empty() {
            return secret.api_key.clone();
        }
    }
    if let Some(secret) = secrets.services.get(service) {
        if !secret.api_key.is_empty() {
            return secret.api_key.clone();
        }
    }
    let env_key: String = service
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    std::env::var(format!("{env_key}_API_KEY")).unwrap_or_default()
}

fn resolve_cover_api_key(api_key_env: &str) -> Result<String, String> {
    std::env::var(api_key_env)
        .ok()
        .filter(|key| !key.is_empty())
        .ok_or_else(|| format!("Cover API key is required. Set {api_key_env} or pass coverApiKeyEnv."))
}

// ── 生成（generateImageFromPrompt 三 API） ───────────────────────

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .unwrap_or_default()
}

#[derive(Debug)]
pub struct GeneratedImage {
    pub bytes: Vec<u8>,
    pub extension: &'static str,
}

/// `generateImageFromPrompt`：按请求 api 分流三形态。
pub async fn generate_image_from_prompt(
    request: &CoverGenerationRequest,
    prompt: &str,
    size: &str,
) -> Result<GeneratedImage, String> {
    match request.api {
        CoverApi::Gemini => generate_gemini_cover(request, prompt).await,
        CoverApi::Images => generate_images_cover(request, prompt, size).await,
        CoverApi::Responses => generate_responses_cover(request, prompt, size).await,
    }
}

/// images API：`{base}/images/generations`（b64 优先；url 下载，401/403 带
/// Bearer 重试）。
async fn generate_images_cover(
    request: &CoverGenerationRequest,
    prompt: &str,
    size: &str,
) -> Result<GeneratedImage, String> {
    let endpoint = request.endpoint.clone().unwrap_or_else(|| {
        format!("{}/images/generations", request.base_url.trim_end_matches('/'))
    });
    let response = http_client()
        .post(&endpoint)
        .header("Content-Type", "application/json")
        .bearer_auth(&request.api_key)
        .json(&json!({ "model": request.model, "prompt": prompt, "n": 1, "size": size }))
        .send()
        .await
        .map_err(|e| format!("cover generation failed: {e}"))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        let preview: String = text.chars().take(500).collect();
        return Err(format!("cover generation failed: HTTP {status} {preview}"));
    }
    let payload: Value =
        serde_json::from_str(&text).map_err(|e| format!("cover generation returned non-JSON response: {e}"))?;
    let image = extract_images_generation_image(&payload);
    if let Some((base64, extension)) = image {
        return decode_base64_image(&base64, extension);
    }
    let url = payload
        .pointer("/data/0/url")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|u| !u.is_empty());
    if let Some(url) = url {
        return download_generated_cover_image(url, &request.api_key).await;
    }
    Err("cover generation response did not include image URL or base64 data.".to_string())
}

/// responses API：`{base}/responses` + `image_generation` tool。
async fn generate_responses_cover(
    request: &CoverGenerationRequest,
    prompt: &str,
    size: &str,
) -> Result<GeneratedImage, String> {
    let endpoint = request.endpoint.clone().unwrap_or_else(|| {
        format!("{}/responses", request.base_url.trim_end_matches('/'))
    });
    let response = http_client()
        .post(&endpoint)
        .header("Content-Type", "application/json")
        .bearer_auth(&request.api_key)
        .json(&json!({
            "model": request.model,
            "input": prompt,
            "tools": [{ "type": "image_generation", "size": size }],
        }))
        .send()
        .await
        .map_err(|e| format!("image generation failed: {e}"))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        let preview: String = text.chars().take(500).collect();
        return Err(format!("image generation failed: HTTP {status} {preview}"));
    }
    let payload: Value =
        serde_json::from_str(&text).map_err(|e| format!("image generation returned non-JSON response: {e}"))?;
    let image_base64 = extract_responses_image_base64(&payload);
    let Some(image_base64) = image_base64 else {
        return Err("image generation response did not include image_generation_call result.".to_string());
    };
    decode_base64_image(&image_base64, "png")
}

/// gemini API：`{base}/models/{model}:generateContent?key=…`（inlineData）。
async fn generate_gemini_cover(
    request: &CoverGenerationRequest,
    prompt: &str,
) -> Result<GeneratedImage, String> {
    let model_encoded = url_escape_component(&request.model);
    let key_encoded = url_escape_component(&request.api_key);
    let endpoint = format!(
        "{}/models/{model_encoded}:generateContent?key={key_encoded}",
        request.base_url.trim_end_matches('/')
    );
    let response = http_client()
        .post(&endpoint)
        .header("Content-Type", "application/json")
        .json(&json!({
            "contents": [{ "role": "user", "parts": [{ "text": prompt }] }],
            "generationConfig": { "responseModalities": ["IMAGE", "TEXT"] },
        }))
        .send()
        .await
        .map_err(|e| format!("cover generation failed: {e}"))?;
    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    if !status.is_success() {
        let preview: String = text.chars().take(500).collect();
        return Err(format!("cover generation failed: HTTP {status} {preview}"));
    }
    let payload: Value =
        serde_json::from_str(&text).map_err(|e| format!("cover generation returned non-JSON response: {e}"))?;
    let image = extract_gemini_image_base64(&payload);
    let Some((base64, extension)) = image else {
        return Err("cover generation response did not include Gemini inline image data.".to_string());
    };
    decode_base64_image(&base64, extension)
}

/// JS encodeURIComponent 的组件转义（供 URL path/query 段使用）。
fn url_escape_component(value: &str) -> String {
    crate::server::task_store::js_encode_uri_component(value)
}

fn decode_base64_image(base64: &str, extension: &str) -> Result<GeneratedImage, String> {
    use base64::Engine as _;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64.trim())
        .map_err(|e| format!("invalid base64 image payload: {e}"))?;
    let extension = if extension == "jpg" || extension == "jpeg" { "jpg" } else { "png" };
    Ok(GeneratedImage { bytes, extension })
}

/// `extractImagesGenerationImage`：data[].b64_json（png）或 url。
/// 返回 (base64, extension) 形态；url 分支由调用方走下载。
fn extract_images_generation_image(payload: &Value) -> Option<(String, &'static str)> {
    let data = payload.get("data")?.as_array()?;
    for item in data {
        if let Some(b64) = item.get("b64_json").and_then(Value::as_str).map(str::trim) {
            if !b64.is_empty() {
                return Some((b64.to_string(), "png"));
            }
        }
    }
    None
}

/// `extractResponsesImageBase64`：output[] 的 image_generation_call.result 或
/// content[] 的 result/image_base64。
pub fn extract_responses_image_base64(payload: &Value) -> Option<String> {
    let output = payload.get("output")?.as_array()?;
    for item in output {
        let record_type = item.get("type").and_then(Value::as_str);
        if record_type == Some("image_generation_call") {
            if let Some(result) = item.get("result").and_then(Value::as_str).map(str::trim) {
                if !result.is_empty() {
                    return Some(result.to_string());
                }
            }
        }
        if let Some(content) = item.get("content").and_then(Value::as_array) {
            for content_item in content {
                for field in ["result", "image_base64"] {
                    if let Some(value) =
                        content_item.get(field).and_then(Value::as_str).map(str::trim)
                    {
                        if !value.is_empty() {
                            return Some(value.to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

/// `extractGeminiImageBase64`：candidates[].content.parts[].inlineData。
pub fn extract_gemini_image_base64(payload: &Value) -> Option<(String, &'static str)> {
    let candidates = payload.get("candidates")?.as_array()?;
    for candidate in candidates {
        let Some(parts) = candidate.pointer("/content/parts").and_then(Value::as_array) else {
            continue;
        };
        for part in parts {
            let inline = part
                .get("inlineData")
                .or_else(|| part.get("inline_data"))
                .and_then(Value::as_object);
            let Some(inline) = inline else { continue };
            let Some(data) = inline.get("data").and_then(Value::as_str).map(str::trim) else {
                continue;
            };
            if data.is_empty() {
                continue;
            }
            let mime = inline
                .get("mimeType")
                .or_else(|| inline.get("mime_type"))
                .and_then(Value::as_str)
                .unwrap_or("image/png")
                .to_lowercase();
            let extension = if mime.contains("jpeg") || mime.contains("jpg") {
                "jpg"
            } else {
                "png"
            };
            return Some((data.to_string(), extension));
        }
    }
    None
}

/// url 下载（401/403 时带 Bearer 重试）。
async fn download_generated_cover_image(url: &str, api_key: &str) -> Result<GeneratedImage, String> {
    let client = http_client();
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("cover image download failed: {e}"))?;
    let response = if response.status().as_u16() == 401 || response.status().as_u16() == 403 {
        client
            .get(url)
            .bearer_auth(api_key)
            .send()
            .await
            .map_err(|e| format!("cover image download failed: {e}"))?
    } else {
        response
    };
    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let bytes = response.bytes().await.map_err(|e| format!("cover image download failed: {e}"))?;
    if !status.is_success() {
        return Err(format!("cover image download failed: HTTP {status}"));
    }
    let extension = if content_type.contains("jpeg") || content_type.contains("jpg") {
        "jpg"
    } else {
        "png"
    };
    Ok(GeneratedImage {
        bytes: bytes.to_vec(),
        extension,
    })
}

// ── 封面提示词与生成链（generateShortFictionCover 消费面，76 号） ──

pub struct CoverSalesPackage<'a> {
    pub title: &'a str,
    pub intro: &'a str,
    pub selling_points: &'a [String],
    pub cover_prompt: &'a str,
}

/// `normalizeSellingPoints`：字符串按 `;；\n` 拆分数组；数组逐项 trim。
pub fn normalize_selling_points(value: Option<&str>) -> Vec<String> {
    value
        .unwrap_or_default()
        .split([';', '；', '\n'])
        .map(str::trim)
        .filter(|point| !point.is_empty())
        .map(String::from)
        .collect()
}

/// `buildCoverImagePrompt`（generic=提示词文件 / short=生成图片——TS 两处模式
/// 分别调用，76 号逐字）。
pub fn build_cover_image_prompt(
    package: &CoverSalesPackage<'_>,
    mode: &str,
    language: Option<&str>,
) -> String {
    let is_en = language == Some("en");
    let base: Vec<String> = {
        let mut lines = Vec::new();
        if is_en {
            lines.push(format!("Title: {}", package.title));
            if !package.intro.trim().is_empty() {
                lines.push(format!("Synopsis: {}", package.intro.trim()));
            }
            if !package.selling_points.is_empty() {
                lines.push(format!("Selling points: {}", package.selling_points.join("; ")));
            }
            if !package.cover_prompt.trim().is_empty() {
                lines.push(format!("User visual notes: {}", package.cover_prompt.trim()));
            }
        } else {
            lines.push(format!("标题：{}", package.title));
            if !package.intro.trim().is_empty() {
                lines.push(format!("简介：{}", package.intro.trim()));
            }
            if !package.selling_points.is_empty() {
                lines.push(format!("卖点：{}", package.selling_points.join("；")));
            }
            if !package.cover_prompt.trim().is_empty() {
                lines.push(format!("用户视觉要求：{}", package.cover_prompt.trim()));
            }
        }
        lines
    };
    if mode == "generic" {
        let header = if is_en {
            "Generate a cover image from the title, synopsis, selling points, and visual notes the user provided."
        } else {
            "按用户给出的标题、简介、卖点和视觉要求生成封面图。"
        };
        let mut lines = vec![header.to_string()];
        lines.extend(base);
        return lines.join("\n");
    }
    // short 模式（生成图片用）。
    let (title_label, notes_label) = if is_en {
        ("Main title: ", "Packaging notes: ")
    } else {
        ("主标题：", "包装提示：")
    };
    let mut mapped: Vec<String> = base
        .into_iter()
        .map(|line| {
            if is_en {
                line.replacen("Title: ", title_label, 1)
                    .replacen("User visual notes: ", notes_label, 1)
            } else {
                line.replacen("标题：", title_label, 1)
                    .replacen("用户视觉要求：", notes_label, 1)
            }
        })
        .collect();
    let mut lines = Vec::new();
    if is_en {
        lines.push("Generate a mobile portrait book cover for an English short story, 3:4 vertical.".to_string());
        lines.append(&mut mapped);
        lines.push(String::new());
        lines.push("Cover direction: a platform short-fiction book cover, not a movie poster. The title lettering is the primary visual — reserve a large two-to-four-line type zone; character in close-up or half-body with a charged expression (cold smirk, shock, breakdown, menace, or payback); props few but large, telegraphing the conflict at a glance.".to_string());
        lines.push("High-contrast, high-saturation colors that read as a phone-list thumbnail. Avoid realistic corporate photography, landscape video thumbnails, magazine editorial looks, delicate thin lettering, and long runs of text.".to_string());
        lines.push("If the model's text rendering is unreliable, prioritize a clear title whitespace/type-block/layout zone instead of covering the canvas with garbled lettering.".to_string());
    } else {
        lines.push("为中文短篇小说生成手机端竖版书封，3:4竖图。".to_string());
        lines.append(&mut mapped);
        lines.push(String::new());
        lines.push("封面方向：平台短篇书封，不是电影海报。标题字要成为主视觉，预留两到四行大字排版区；人物近景或半身，表情有冷笑、震惊、崩溃、压迫或反杀感；道具少而大，一眼能看出冲突。".to_string());
        lines.push("颜色高对比、高饱和，适合手机列表缩略图。避免写实会议摄影、横版视频缩略图、杂志大片、小清新细字和长段文字。".to_string());
        lines.push("如果模型文字不稳定，优先生成明确标题留白/字块/排版空间，不要把大量乱码文字铺满画面。".to_string());
    }
    lines.join("\n")
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverGenerationOutcome {
    pub title: String,
    pub output_dir: String,
    pub cover_prompt_path: String,
    pub cover_image_path: String,
}

/// `generateShortFictionCover`：标题必填 → cover-prompt.md（generic 提示词）→
/// 生成图片（short 提示词，默认 1024x1360）→ cover.png/jpg。
pub async fn generate_short_fiction_cover(
    root: &Path,
    title: &str,
    intro: Option<&str>,
    selling_points: &[String],
    cover_prompt: Option<&str>,
    output_dir: Option<&str>,
) -> Result<CoverGenerationOutcome, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("title is required for cover generation.".to_string());
    }
    let segment: String = {
        // runner.ts 的 safeSegment（与 script runner 同款）。
        let cleaned: String = title
            .trim()
            .chars()
            .flat_map(char::to_lowercase)
            .map(|c| {
                if matches!(c, '\\' | '/' | ':' | '\0' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_whitespace() {
                    '-'
                } else {
                    c
                }
            })
            .collect();
        let trimmed = cleaned.trim_matches('-').to_string();
        let mut out = String::new();
        let mut units = 0usize;
        for ch in trimmed.chars() {
            let ch_units = ch.len_utf16();
            if units + ch_units > 80 {
                break;
            }
            out.push(ch);
            units += ch_units;
        }
        if out.is_empty() || out == "." || out == ".." {
            format!("short-{}", crate::interaction::session::utc_now_ms())
        } else {
            out
        }
    };
    let output_dir = output_dir
        .map(str::trim)
        .filter(|dir| !dir.is_empty())
        .map(|dir| dir.trim_matches('/').to_string())
        .unwrap_or_else(|| format!("covers/{segment}"));
    let package = CoverSalesPackage {
        title,
        intro: intro.unwrap_or_default(),
        selling_points,
        cover_prompt: cover_prompt.unwrap_or_default(),
    };
    let prompt_path = format!("{output_dir}/cover-prompt.md");
    let generic_prompt = build_cover_image_prompt(&package, "generic", None);
    let full_prompt_path = root.join(&prompt_path);
    if let Some(parent) = full_prompt_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| e.to_string())?;
    }
    let mut payload = generic_prompt;
    if !payload.ends_with('\n') {
        payload.push('\n');
    }
    tokio::fs::write(&full_prompt_path, payload).await.map_err(|e| e.to_string())?;

    let request = resolve_cover_generation_request(root).await?;
    let size = std::env::var("INKOS_COVER_SIZE")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "1024x1360".to_string());
    let short_prompt = build_cover_image_prompt(&package, "short", None);
    let image = generate_image_from_prompt(&request, &short_prompt, &size).await?;
    let image_name = if image.extension == "jpg" { "cover.jpg" } else { "cover.png" };
    let image_rel = format!("{output_dir}/{image_name}");
    let full_image_path = root.join(&image_rel);
    if let Some(parent) = full_image_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| e.to_string())?;
    }
    tokio::fs::write(&full_image_path, &image.bytes).await.map_err(|e| e.to_string())?;
    Ok(CoverGenerationOutcome {
        title: title.to_string(),
        output_dir,
        cover_prompt_path: prompt_path,
        cover_image_path: image_rel,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_and_secret_key() {
        let kkaiapi = resolve_cover_provider_preset("kkaiapi").unwrap();
        assert_eq!(kkaiapi.api, CoverApi::Images);
        assert_eq!(kkaiapi.default_model, "gpt-image-2");
        let google = resolve_cover_provider_preset("google").unwrap();
        assert_eq!(google.api, CoverApi::Gemini);
        assert!(resolve_cover_provider_preset("unknown").is_none());
        assert_eq!(cover_secret_key("kkaiapi"), "cover:kkaiapi");
    }

    #[test]
    fn normalize_base_url_rules() {
        assert_eq!(
            normalize_cover_base_url("https://api.example.com/v1/"),
            Some("https://api.example.com/v1".to_string())
        );
        assert_eq!(normalize_cover_base_url("http://localhost:9000"), Some("http://localhost:9000".to_string()));
        assert_eq!(normalize_cover_base_url("ftp://x"), None);
        assert_eq!(normalize_cover_base_url("https://u:p@x"), None);
        assert_eq!(normalize_cover_base_url("https://x?q=1"), None);
        assert_eq!(normalize_cover_base_url("  "), None);
    }

    #[test]
    fn responses_extraction_paths() {
        let direct = json!({ "output": [
            { "type": "image_generation_call", "result": "  abc123  " }
        ]});
        assert_eq!(extract_responses_image_base64(&direct).as_deref(), Some("abc123"));

        let nested = json!({ "output": [
            { "type": "message", "content": [ { "image_base64": "img64" } ] }
        ]});
        assert_eq!(extract_responses_image_base64(&nested).as_deref(), Some("img64"));

        assert!(extract_responses_image_base64(&json!({ "output": [] })).is_none());
        assert!(extract_responses_image_base64(&json!({})).is_none());
    }

    #[test]
    fn gemini_extraction_paths() {
        let payload = json!({ "candidates": [ { "content": { "parts": [
            { "text": "caption" },
            { "inlineData": { "data": "gg", "mimeType": "image/jpeg" } }
        ]}}]});
        assert_eq!(extract_gemini_image_base64(&payload), Some(("gg".to_string(), "jpg")));

        let snake_case = json!({ "candidates": [ { "content": { "parts": [
            { "inline_data": { "data": "pp", "mime_type": "image/png" } }
        ]}}]});
        assert_eq!(extract_gemini_image_base64(&snake_case), Some(("pp".to_string(), "png")));
        assert!(extract_gemini_image_base64(&json!({})).is_none());
    }

    #[test]
    fn images_extraction_prefers_b64() {
        let payload = json!({ "data": [
            { "b64_json": "  b64data  " },
            { "url": "https://cdn/x.png" }
        ]});
        assert_eq!(extract_images_generation_image(&payload), Some(("b64data".to_string(), "png")));
        assert!(extract_images_generation_image(&json!({ "data": [ { "url": "https://x" } ] })).is_none());
        assert!(extract_images_generation_image(&json!({})).is_none());
    }

    #[tokio::test]
    async fn resolve_fails_without_config_and_uses_project_config() {
        let dir = tempfile::tempdir().unwrap();
        // 无 env 无 inkos.json → 明确错误。
        let err = resolve_cover_generation_request(dir.path()).await.unwrap_err();
        assert!(err.contains("cover endpoint is required"), "{err}");

        // inkos.json llm.cover（无 key）→ key 错误。
        std::fs::write(
            dir.path().join("inkos.json"),
            r#"{ "llm": { "cover": { "service": "kkaiapi", "model": "gpt-image-2" } } }"#,
        )
        .unwrap();
        let err = resolve_cover_generation_request(dir.path()).await.unwrap_err();
        assert!(err.contains("Cover API key is required"), "{err}");

        // secrets 带 cover:{service} key → 完整请求。
        let secrets_dir = dir.path().join(".inkos");
        std::fs::create_dir_all(&secrets_dir).unwrap();
        std::fs::write(
            secrets_dir.join("secrets.json"),
            r#"{ "services": { "cover:kkaiapi": { "apiKey": "sk-cover" } } }"#,
        )
        .unwrap();
        let request = resolve_cover_generation_request(dir.path()).await.unwrap();
        assert_eq!(request.api, CoverApi::Images);
        assert_eq!(request.model, "gpt-image-2");
        assert_eq!(request.api_key, "sk-cover");
        assert_eq!(request.base_url, "https://api.kkaiapi.com/v1");
    }

    #[tokio::test]
    async fn generate_images_api_b64_and_url_download() {
        // b64_json 形态。
        let server = mock_server(json!({ "data": [ { "b64_json": "aGVsbG8=" } ] }), 200).await;
        let request = CoverGenerationRequest {
            api: CoverApi::Images,
            base_url: server.clone(),
            endpoint: None,
            model: "gpt-image-2".to_string(),
            api_key: "k".to_string(),
        };
        let image = generate_image_from_prompt(&request, "画一张封面", "1024x1024").await.unwrap();
        assert_eq!(image.bytes, b"hello");
        assert_eq!(image.extension, "png");

        // url 形态 → 下载（二进制 jpg；url 为绝对地址——TS Node fetch 同要求）。
        let download_app = axum::Router::new().route(
            "/downloaded.jpg",
            axum::routing::get(|| async {
                axum::response::IntoResponse::into_response((
                    [(axum::http::header::CONTENT_TYPE, "image/jpeg")],
                    b"\xff\xd8fakejpg".to_vec(),
                ))
            }),
        );
        let download_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let download_addr = download_listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(download_listener, download_app).await.unwrap(); });

        let url_payload = json!({ "data": [ { "url": format!("http://{download_addr}/downloaded.jpg") } ] }).to_string();
        let app = axum::Router::new().route(
            "/images/generations",
            axum::routing::post(move || async move {
                axum::response::IntoResponse::into_response((
                    [(axum::http::header::CONTENT_TYPE, "application/json")],
                    url_payload,
                ))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        let request = CoverGenerationRequest {
            api: CoverApi::Images,
            base_url: format!("http://{addr}"),
            endpoint: None,
            model: "m".to_string(),
            api_key: "k".to_string(),
        };
        let image = generate_image_from_prompt(&request, "封面", "512x512").await.unwrap();
        assert_eq!(image.bytes, b"\xff\xd8fakejpg");
        assert_eq!(image.extension, "jpg");
    }

    #[tokio::test]
    async fn generate_responses_and_gemini_apis() {
        // responses：output[].image_generation_call.result。
        let responses_body = json!({ "output": [ { "type": "image_generation_call", "result": "aGVsbG8=" } ] }).to_string();
        let app = axum::Router::new().route(
            "/responses",
            axum::routing::post(move || async move {
                (
                    [(axum::http::header::CONTENT_TYPE, "application/json")],
                    responses_body,
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        let request = CoverGenerationRequest {
            api: CoverApi::Responses,
            base_url: format!("http://{addr}"),
            endpoint: None,
            model: "m".to_string(),
            api_key: "k".to_string(),
        };
        let image = generate_image_from_prompt(&request, "prompt", "1024x1024").await.unwrap();
        assert_eq!(image.bytes, b"hello");

        // gemini：candidates[].content.parts[].inlineData（模型名进 URL path）。
        let gemini_body = json!({ "candidates": [ { "content": { "parts": [
            { "inlineData": { "data": "aGVsbG8=", "mimeType": "image/png" } }
        ]}}]})
        .to_string();
        let app = axum::Router::new().route(
            "/models/:model",
            axum::routing::post(move || async move {
                (
                    [(axum::http::header::CONTENT_TYPE, "application/json")],
                    gemini_body,
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        let request = CoverGenerationRequest {
            api: CoverApi::Gemini,
            base_url: format!("http://{addr}"),
            endpoint: None,
            model: "gemini-x".to_string(),
            api_key: "k".to_string(),
        };
        let image = generate_image_from_prompt(&request, "prompt", "1024x1024").await.unwrap();
        assert_eq!(image.bytes, b"hello");
    }

    #[tokio::test]
    async fn generation_failure_surfaces_http_status() {
        let server = mock_server(json!({ "error": "quota" }), 429).await;
        let request = CoverGenerationRequest {
            api: CoverApi::Images,
            base_url: server,
            endpoint: None,
            model: "m".to_string(),
            api_key: "k".to_string(),
        };
        let err = generate_image_from_prompt(&request, "p", "1x1").await.unwrap_err();
        assert!(err.contains("HTTP 429"), "{err}");
    }

    async fn mock_server(body: Value, status: u16) -> String {
        let body = body.to_string();
        let app = axum::Router::new().route(
            "/images/generations",
            axum::routing::post(move || async move {
                (
                    axum::http::StatusCode::from_u16(status).unwrap(),
                    [(axum::http::header::CONTENT_TYPE, "application/json")],
                    body,
                )
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
        format!("http://{addr}")
    }
}

#[cfg(test)]
mod cover_prompt_tests {
    use super::*;

    #[test]
    fn selling_points_normalization() {
        assert_eq!(normalize_selling_points(Some("a；b\nc; d")), vec!["a", "b", "c", "d"]);
        assert!(normalize_selling_points(None).is_empty());
        assert!(normalize_selling_points(Some("  ")).is_empty());
    }

    #[test]
    fn cover_prompt_generic_and_short_modes() {
        let package = CoverSalesPackage {
            title: "山雨",
            intro: "简介文本",
            selling_points: &["卖点一".to_string(), "卖点二".to_string()],
            cover_prompt: "水墨",
        };
        let generic = build_cover_image_prompt(&package, "generic", None);
        assert!(generic.starts_with("按用户给出的标题、简介、卖点和视觉要求生成封面图。"), "{generic}");
        assert!(generic.contains("标题：山雨"), "{generic}");
        assert!(generic.contains("卖点：卖点一；卖点二"), "{generic}");

        let short = build_cover_image_prompt(&package, "short", None);
        assert!(short.starts_with("为中文短篇小说生成手机端竖版书封，3:4竖图。"), "{short}");
        assert!(short.contains("主标题：山雨"), "{short}");
        assert!(short.contains("包装提示：水墨"), "{short}");
        assert!(short.contains("封面方向：平台短篇书封"), "{short}");
    }
}
