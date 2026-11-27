use crate::scene::Shape;
use serde::Deserialize;
use serde_json::json;
use std::{net::IpAddr, time::Duration};
use url::{Host, Url};

const MAX_PARTS: usize = 24;
const MAX_RESPONSE_BYTES: u64 = 64 * 1024;
const SYSTEM_PROMPT: &str = r#"You design simple editable game characters from geometric primitives.
Return one JSON object with a parts array and no other text. Use this exact format:
{"parts":[{"name":"Torso","shape":"box","position":[0,1.3,0],"rotation":[0,0,0],"size":[0.8,0.7,0.55],"color":[0.2,0.55,0.9]}]}
Create at most 24 parts. Build a recognizable silhouette from the player's description.
Allowed shapes: box, sphere, cylinder. All array fields have exactly three numeric components.
Coordinates: X left/right, Y up, Z front/back (positive Z is front). The ground is Y=0.
Aim for a total height around 2.3 units. Position is the center of each part. Size is full local width, height, and depth in units.
Rotation is XYZ in degrees. Cylinders are vertical by default. Use zero rotation where possible.
All colors are sRGB values between 0 and 1. Prefer legible, distinct colors and connected body parts."#;

#[derive(Debug, Deserialize)]
pub struct Draft {
    pub parts: Vec<DraftPart>,
}

#[derive(Debug, Deserialize)]
pub struct DraftPart {
    pub name: String,
    #[serde(deserialize_with = "deserialize_shape")]
    pub shape: Shape,
    pub position: [f32; 3],
    #[serde(default)]
    pub rotation: [f32; 3],
    pub size: [f32; 3],
    pub color: [f32; 3],
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct Message {
    content: Option<String>,
}

const ANTHROPIC_HOST: &str = "api.anthropic.com";
const ANTHROPIC_VERSION: &str = "2026-10-21";
const CLAUDE_MAX_TOKENS: u32 = 16_000;
const CLAUDE_MAX_RESPONSE_BYTES: u64 = 1024 * 1024;
pub(crate) const CLAUDE_DEFAULT_MODEL: &str = "claude-opus-5-5";

#[derive(Deserialize)]
struct ClaudeResponse {
    content: Vec<ClaudeBlock>,
    stop_reason: Option<String>,
}

#[derive(Deserialize)]
struct ClaudeBlock {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Deserialize)]
struct ClaudeErrorBody {
    error: ClaudeErrorDetail,
}

#[derive(Deserialize)]
struct ClaudeErrorDetail {
    message: String,
}

fn http_agent(timeout_secs: u64) -> ureq::Agent {
    ureq::config::Config::builder()
        .proxy(None)
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_global(Some(Duration::from_secs(timeout_secs)))
        .build()
        .into()
}

/// Returns the Messages API URL when the endpoint points at the Anthropic API.
fn anthropic_url(endpoint: &str) -> Option<Url> {
    let mut url = Url::parse(endpoint.trim()).ok()?;
    match url.host() {
        Some(Host::Domain(domain)) if domain.eq_ignore_ascii_case(ANTHROPIC_HOST) => {}
        _ => return None,
    }
    url.set_scheme("https").ok()?;
    url.set_path("/v1/messages");
    url.set_query(None);
    url.set_fragment(None);
    Some(url)
}

/// Models that accept `effort` and server-side `fallbacks: "default"`.
fn claude_current_model(model: &str) -> bool {
    matches!(
        model,
        "claude-opus-5-5" | "claude-opus-5" | "claude-sonnet-5-5" | "claude-fable-5-2" | "claude-fable-5-1"
    )
}

fn complete_claude(url: &Url, model: &str, key: &str, system_prompt: &str, prompt: &str) -> Result<String, String> {
    let key = if key.is_empty() {
        std::env::var("ANTHROPIC_API_KEY").unwrap_or_default()
    } else {
        key.to_owned()
    };
    let key = key.trim();
    if key.is_empty() {
        return Err(
            "Укажи API-ключ Claude (console.anthropic.com) или задай переменную ANTHROPIC_API_KEY.".into(),
        );
    }
    let mut body = json!({
        "model": model,
        "max_tokens": CLAUDE_MAX_TOKENS,
        "system": system_prompt,
        "messages": [{"role": "user", "content": prompt}],
    });
    let current = claude_current_model(model);
    if current {
        body["output_config"] = json!({"effort": "medium"});
        body["fallbacks"] = json!("default");
    }
    let mut request = http_agent(300)
        .post(url.as_str())
        .header("x-api-key", key)
        .header("anthropic-version", ANTHROPIC_VERSION);
    if current {
        request = request.header("anthropic-beta", "server-side-fallback-2026-07-01");
    }
    let mut response = request.send_json(body).map_err(|error| match error {
        ureq::Error::Timeout(_) => "Claude не ответил за отведённое время.".to_string(),
        _ => "Не удалось связаться с api.anthropic.com. Проверь интернет.".to_string(),
    })?;
    let status = response.status();
    if !status.is_success() {
        let detail = response
            .body_mut()
            .with_config()
            .limit(MAX_RESPONSE_BYTES)
            .read_json::<ClaudeErrorBody>()
            .map(|body| body.error.message)
            .unwrap_or_default();
        let hint = match status.as_u16() {
            401 => "проверь API-ключ",
            403 => "у ключа нет доступа к этой модели",
            404 => "проверь имя модели",
            429 => "превышен лимит запросов, повтори позже",
            529 => "серверы Claude перегружены, повтори позже",
            _ => "проверь запрос и настройки",
        };
        return Err(format!("Claude ответил HTTP {}: {hint}. {detail}", status.as_u16()).trim().to_owned());
    }
    let response: ClaudeResponse = response
        .body_mut()
        .with_config()
        .limit(CLAUDE_MAX_RESPONSE_BYTES)
        .read_json()
        .map_err(|_| "Ответ Claude слишком большой или в неожиданном формате.".to_string())?;
    claude_text(response)
}

fn claude_text(response: ClaudeResponse) -> Result<String, String> {
    match response.stop_reason.as_deref() {
        Some("max_tokens") => {
            return Err("Ответ Claude оборвался по лимиту токенов. Сократи описание.".into());
        }
        Some("refusal") => return Err("Claude отказался выполнять этот запрос.".into()),
        _ => {}
    }
    let text: String = response
        .content
        .into_iter()
        .filter(|block| block.kind == "text")
        .filter_map(|block| block.text)
        .collect();
    if text.trim().is_empty() {
        return Err("Claude не вернул текст ответа.".into());
    }
    Ok(text)
}

/// This is a blocking request. Invoke it in a worker thread outside the editor update loop.
pub fn generate(endpoint: &str, model: &str, key: &str, prompt: &str) -> Result<Draft, String> {
    parse_draft(&complete(endpoint, model, key, SYSTEM_PROMPT, prompt)?)
}

pub(crate) fn complete(
    endpoint: &str,
    model: &str,
    key: &str,
    system_prompt: &str,
    prompt: &str,
) -> Result<String, String> {
    let model = model.trim();
    let key = key.trim();
    let prompt = prompt.trim();
    if model.is_empty() || model.len() > 256 {
        return Err("Укажи имя модели из настроек сервера (до 256 байт).".into());
    }
    if prompt.is_empty() || prompt.len() > 65_536 {
        return Err("Запрос должен содержать описание до 64 КБ.".into());
    }
    if let Some(url) = anthropic_url(endpoint) {
        return complete_claude(&url, model, key, system_prompt, prompt);
    }
    let endpoint = completion_url(endpoint)?;

    let mut request = http_agent(150).post(endpoint.as_str());
    if !key.is_empty() {
        request = request.header("authorization", format!("Bearer {key}"));
    }
    let mut response = request
        .send_json(json!({
            "model": model,
            "messages": [
                {"role":"system", "content":system_prompt},
                {"role":"user", "content":prompt},
            ],
            "stream": false,
        }))
        .map_err(|error| match error {
            ureq::Error::Timeout(_) => "Сервер не ответил за отведённое время.".into(),
            _ => "Не удалось связаться с сервером. Проверь адрес, ключ и запущена ли модель."
                .to_string(),
        })?;
    let status = response.status();
    if !status.is_success() {
        let description = match status.as_u16() {
            401 | 403 => "проверь ключ доступа",
            404 => "проверь адрес API и имя модели",
            429 => "проверь лимит запросов или повтори позже",
            _ if status.is_redirection() => "укажи конечный адрес API",
            _ => "проверь настройки сервера и доступность модели",
        };
        return Err(format!(
            "Сервер ответил HTTP {}. {description}.",
            status.as_u16()
        ));
    }
    let response: ChatResponse = response
        .body_mut()
        .with_config()
        .limit(MAX_RESPONSE_BYTES)
        .read_json()
        .map_err(|_| {
            "Ответ сервера слишком большой или не соответствует формату Chat Completions."
                .to_string()
        })?;
    let choice = response
        .choices
        .into_iter()
        .next()
        .ok_or("Сервер вернул пустой список ответов.")?;
    if choice.finish_reason.as_deref() == Some("length") {
        return Err(
            "Ответ модели оборвался. Сократи описание или увеличь лимит ответа на сервере.".into(),
        );
    }
    choice
        .message
        .content
        .ok_or_else(|| "Модель не вернула текст ответа.".into())
}

fn completion_url(endpoint: &str) -> Result<Url, String> {
    let mut endpoint = Url::parse(endpoint.trim())
        .map_err(|_| "Укажи полный адрес API, например http://127.0.0.1:8080/v1.".to_string())?;
    let loopback = match endpoint.host() {
        Some(Host::Ipv4(ip)) => IpAddr::V4(ip).is_loopback(),
        Some(Host::Ipv6(ip)) => IpAddr::V6(ip).is_loopback(),
        Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        None => return Err("В адресе API отсутствует имя сервера.".into()),
    };
    match endpoint.scheme() {
        "https" => {}
        "http" if loopback => {}
        _ => {
            return Err(
                "Для удалённого сервера нужен HTTPS. HTTP доступен для локального сервера.".into(),
            );
        }
    }
    if !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
    {
        return Err(
            "Укажи адрес API без логина, пароля и параметров URL. Ключ вводится отдельно.".into(),
        );
    }

    let path = endpoint.path().trim_end_matches('/');
    let path = if path.ends_with("/chat/completions") {
        path.to_owned()
    } else if path.is_empty() {
        "/v1/chat/completions".into()
    } else {
        format!("{path}/chat/completions")
    };
    endpoint.set_path(&path);
    Ok(endpoint)
}

pub(crate) fn response_json(content: &str) -> &str {
    let mut content = content.trim();
    if let Some(thoughts) = content.strip_prefix("<think>")
        && let Some((_, answer)) = thoughts.split_once("</think>")
    {
        content = answer.trim();
    }
    if content.starts_with("```")
        && let Some((_, body)) = content.split_once('\n')
        && let Some(body) = body.trim_end().strip_suffix("```")
    {
        content = body.trim();
    }
    content
}

fn parse_draft(content: &str) -> Result<Draft, String> {
    let mut draft: Draft = serde_json::from_str(response_json(content)).map_err(|_| {
        "Модель не вернула корректный JSON персонажа. Попробуй описать его проще.".to_string()
    })?;
    if !(1..=MAX_PARTS).contains(&draft.parts.len()) {
        return Err(format!(
            "У персонажа должно быть от 1 до {MAX_PARTS} деталей."
        ));
    }
    for (index, part) in draft.parts.iter_mut().enumerate() {
        let name = part.name.trim();
        if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
            return Err(format!("У детали №{} недопустимое название.", index + 1));
        }
        if !part.position.iter().all(|v| (-100.0..=100.0).contains(v))
            || !part.rotation.iter().all(|v| (-3600.0..=3600.0).contains(v))
            || !part.size.iter().all(|v| (0.01..=100.0).contains(v))
            || !part.color.iter().all(|v| (0.0..=1.0).contains(v))
        {
            return Err(format!(
                "У детали №{} недопустимый размер, положение, угол или цвет.",
                index + 1
            ));
        }
        part.name = name.to_owned();
    }
    Ok(draft)
}

fn deserialize_shape<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Shape, D::Error> {
    let name = String::deserialize(deserializer)?;
    match name.trim().to_ascii_lowercase().as_str() {
        "box" | "cube" | "cuboid" => Ok(Shape::Box),
        "sphere" => Ok(Shape::Sphere),
        "cylinder" => Ok(Shape::Cylinder),
        _ => Err(serde::de::Error::custom("unknown primitive shape")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;

    const EXAMPLE: &str = r#"{"parts":[{"name":" Torso ","shape":"box","position":[0,1,0],"size":[1,1,1],"color":[0.1,0.2,0.3]}]}"#;

    #[test]
    fn routes_anthropic_endpoint_to_messages_api() {
        let url = anthropic_url("https://api.anthropic.com/v1").unwrap();
        assert_eq!(url.as_str(), "https://api.anthropic.com/v1/messages");
        assert!(anthropic_url("http://127.0.0.1:8080/v1").is_none());
        assert!(anthropic_url("https://api.anthropic.com.evil.test/v1").is_none());
    }

    #[test]
    fn extracts_claude_text_and_rejects_cutoffs() {
        let ok: ClaudeResponse = serde_json::from_str(
            r#"{"content":[{"type":"thinking","thinking":"","signature":"x"},{"type":"text","text":"{\"parts\":[]}"}],"stop_reason":"end_turn"}"#,
        )
        .unwrap();
        assert_eq!(claude_text(ok).unwrap(), "{\"parts\":[]}");
        let cut: ClaudeResponse =
            serde_json::from_str(r#"{"content":[{"type":"text","text":"{"}],"stop_reason":"max_tokens"}"#).unwrap();
        assert!(claude_text(cut).is_err());
        let refused: ClaudeResponse =
            serde_json::from_str(r#"{"content":[],"stop_reason":"refusal"}"#).unwrap();
        assert!(claude_text(refused).is_err());
    }

    #[test]
    fn normalizes_base_url_and_accepts_local_http() {
        let url = completion_url("http://127.0.0.1:8080/v1/").unwrap();
        assert_eq!(url.as_str(), "http://127.0.0.1:8080/v1/chat/completions");
        assert_eq!(
            completion_url("http://[::1]:8080").unwrap().path(),
            "/v1/chat/completions"
        );
        let remote = "https://example.com/api/v1/chat/completions";
        assert_eq!(completion_url(remote).unwrap().as_str(), remote);
        assert!(completion_url("http://example.com/v1").is_err());
        assert!(completion_url("https://example.com/v1?key=secret").is_err());
    }

    #[test]
    fn validates_model_parts() {
        let draft = parse_draft(&format!("<think>plan</think>\n```json\n{EXAMPLE}\n```")).unwrap();
        assert_eq!(draft.parts.len(), 1);
        assert_eq!(draft.parts[0].name, "Torso");
        assert_eq!(draft.parts[0].rotation, [0.0; 3]);
        assert!(parse_draft(&EXAMPLE.replace("[1,1,1]", "[-1,1,1]")).is_err());
        assert!(parse_draft(r#"{"parts":[]}"#).is_err());
    }

    #[test]
    fn sends_compatible_chat_request_and_accepts_robot() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut connection, _) = listener.accept().unwrap();
            connection
                .set_read_timeout(Some(Duration::from_secs(4)))
                .unwrap();
            let mut request = BufReader::new(&mut connection);
            let mut line = String::new();
            request.read_line(&mut line).unwrap();
            assert_eq!(line.trim(), "POST /v1/chat/completions HTTP/1.1");

            let mut content_length = None;
            let mut authenticated = false;
            loop {
                line.clear();
                if request.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
                let (name, value) = line.trim_end().split_once(':').unwrap();
                if name.eq_ignore_ascii_case("content-length") {
                    content_length = Some(value.trim().parse::<usize>().unwrap());
                }
                if name.eq_ignore_ascii_case("authorization") {
                    authenticated = value.trim() == "Bearer mock-key";
                }
            }
            assert!(authenticated);
            let mut bytes = vec![0; content_length.unwrap()];
            request.read_exact(&mut bytes).unwrap();
            let payload: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(payload["model"], "mock-model");
            assert_eq!(payload["stream"], false);
            assert_eq!(payload["messages"][1]["content"], "small blue robot");

            let reply = json!({"choices":[{"message":{"content":EXAMPLE},"finish_reason":"stop"}]})
                .to_string();
            write!(
                connection,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{reply}",
                reply.len()
            )
            .unwrap();
        });
        let draft = generate(&endpoint, "mock-model", "mock-key", "small blue robot").unwrap();
        server.join().unwrap();
        assert_eq!(draft.parts[0].name, "Torso");
        assert_eq!(draft.parts[0].shape, Shape::Box);
    }
}
