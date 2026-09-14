use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    mem, slice,
    sync::{Mutex, OnceLock},
};
use time::{
    format_description::well_known::Rfc3339, macros::format_description, Duration, OffsetDateTime,
    PrimitiveDateTime, UtcOffset,
};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

const HOST_HTTP: i32 = 1;
const HOST_NOW: i32 = 5;
const HOST_ASSET_REGISTER: i32 = 7;
const HOST_CREDENTIAL_COMMIT: i32 = 8;
const HOST_CONFIG_GET: i32 = 9;
const HOST_BUFFER_BYTES: usize = 4 * 1024 * 1024;
const SESSION_SCOPE: &str = "guolia.session";
const USER_AGENT: &str = "Mozilla/5.0 OhMyCine/0.1 GuoliaPlugin/0.1";

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "ohmycine")]
extern "C" {
    fn host_call(
        operation: i32,
        request_pointer: i32,
        request_length: i32,
        response_pointer: i32,
        response_capacity: i32,
    ) -> i32;
}
#[cfg(not(target_arch = "wasm32"))]
unsafe fn host_call(_: i32, _: i32, _: i32, _: i32, _: i32) -> i32 {
    -4
}

#[derive(Zeroize, ZeroizeOnDrop)]
struct PendingLogin {
    connection_id: String,
    username: String,
    password: String,
    expires_at: i64,
    has_session_cookie: bool,
}
static PENDING: OnceLock<Mutex<HashMap<String, PendingLogin>>> = OnceLock::new();
fn pending() -> &'static Mutex<HashMap<String, PendingLogin>> {
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

#[no_mangle]
pub extern "C" fn omc_api_version() -> i32 {
    1
}
#[no_mangle]
pub extern "C" fn omc_start() {}
#[no_mangle]
pub extern "C" fn omc_alloc(size: i32) -> i32 {
    if size < 0 {
        return 0;
    }
    let mut buffer = vec![0_u8; size as usize].into_boxed_slice();
    let pointer = buffer.as_mut_ptr();
    mem::forget(buffer);
    pointer as i32
}
#[no_mangle]
/// Releases a buffer previously returned by this plugin ABI.
///
/// # Safety
///
/// `pointer` and `length` must describe an allocation created by `omc_alloc`
/// or a response allocation returned from `omc_invoke`, exactly once.
pub unsafe extern "C" fn omc_free(pointer: i32, length: i32) {
    if pointer > 0 && length >= 0 {
        std::ptr::write_bytes(pointer as *mut u8, 0, length as usize);
        let raw = std::ptr::slice_from_raw_parts_mut(pointer as *mut u8, length as usize);
        drop(Box::from_raw(raw));
    }
}

#[no_mangle]
/// Dispatches one Runtime v1 JSON operation.
///
/// # Safety
///
/// The Host must provide a readable guest-memory range beginning at
/// `request_pointer` with exactly `request_length` bytes for this call.
pub unsafe extern "C" fn omc_invoke(
    operation: i32,
    request_pointer: i32,
    request_length: i32,
) -> i64 {
    if request_pointer <= 0 || request_length < 0 {
        return encode(plugin_error("invalid-request", "请求无效"));
    }
    let request = slice::from_raw_parts(request_pointer as *const u8, request_length as usize);
    let result = match operation {
        14 => parse::<ResourceSearchRequest>(request).and_then(resource_search),
        15 => parse::<ResourceResolveRequest>(request).and_then(resource_resolve),
        16 => parse::<ResourceHealthRequest>(request).and_then(resource_health),
        17 => parse::<ResourceLoginRequest>(request).and_then(resource_login),
        18 => parse::<ResourceCaptchaRequest>(request).and_then(resource_captcha),
        19 => parse::<ResourceCookieRequest>(request).and_then(resource_cookie),
        _ => Err(PluginError::new("invalid-request", "不支持的资源站操作")),
    };
    encode(match result {
        Ok(value) => value,
        Err(error) => plugin_error(error.code, error.message),
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResourceSearchRequest {
    connection_id: String,
    query: String,
    kind: Option<String>,
    year: Option<i32>,
    page: Option<i32>,
}
#[derive(Serialize)]
struct ResourceSearchItem {
    id: String,
    title: String,
    #[serde(rename = "sizeBytes")]
    size_bytes: i64,
    seeders: i32,
    #[serde(rename = "updatedAt", skip_serializing_if = "Option::is_none")]
    updated_at: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tags: Vec<String>,
}
#[derive(Serialize)]
struct ResourceSearchResponse {
    items: Vec<ResourceSearchItem>,
    page: i32,
    #[serde(rename = "hasNext")]
    has_next: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResourceResolveRequest {
    connection_id: String,
    resource_id: String,
}
#[derive(Serialize)]
struct ResourceResolveResponse {
    magnet: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResourceHealthRequest {
    connection_id: String,
}
#[derive(Serialize)]
struct ResourceHealthResponse {
    status: String,
    #[serde(rename = "accountName", skip_serializing_if = "Option::is_none")]
    account_name: Option<String>,
}
#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResourceLoginRequest {
    connection_id: String,
    username: String,
    password: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResourceCaptchaRequest {
    connection_id: String,
    challenge_id: String,
    points: Vec<Point>,
}
#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ResourceCookieRequest {
    connection_id: String,
    cookie: String,
}
#[derive(Deserialize)]
struct Point {
    x: i32,
    y: i32,
}
#[derive(Serialize)]
struct CaptchaChallenge {
    #[serde(rename = "challengeId")]
    challenge_id: String,
    #[serde(rename = "imageAssetRef")]
    image_asset_ref: String,
    width: i32,
    height: i32,
    prompt: String,
    #[serde(rename = "maxPoints")]
    max_points: i32,
}
#[derive(Serialize)]
struct LoginResponse {
    state: String,
    #[serde(rename = "accountName", skip_serializing_if = "Option::is_none")]
    account_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    challenge: Option<CaptchaChallenge>,
    #[serde(rename = "errorCode", skip_serializing_if = "Option::is_none")]
    error_code: Option<String>,
}

fn resource_search(request: ResourceSearchRequest) -> Result<Value, PluginError> {
    validate_connection(&request.connection_id)?;
    let _ = (&request.kind, &request.year);
    if request.query.trim().is_empty() || request.query.chars().count() > 256 {
        return Err(PluginError::new("invalid-request", "搜索关键词无效"));
    }
    let page = request.page.unwrap_or(1).clamp(1, 10000);
    let origin = entry_origin(&request.connection_id)?;
    let api_url = format!(
        "{origin}/res/search?q={}&type=4&ziyuan=&mode=1&page={page}",
        urlencoding::encode(request.query.trim())
    );
    let response = http_get(&request.connection_id, &api_url, true)?;
    if browser_verification_response(&response.body) {
        return Err(PluginError::new(
            "browser-verification-required",
            "资源站要求完成浏览器验证",
        ));
    }
    if auth_required_response(response.status, &response.body) {
        return Err(PluginError::new("not-authenticated", "资源站登录已失效"));
    }
    if response.status == 429 {
        return Err(PluginError::new("rate-limited", "资源站请求受到限流"));
    }
    if (200..300).contains(&response.status) {
        if let Some(mut result) = parse_json_search(&response.body, page) {
            normalize_search_times(&mut result)?;
            return Ok(json!(result));
        }
    } else if response.status != 404 && response.status != 405 {
        return Err(PluginError::new(
            "upstream-unavailable",
            "资源站入口暂时不可用",
        ));
    }

    // The current site serves the browser search as an HTML table. Keep the
    // historical JSON endpoint as a fast compatibility path, but fall back to
    // the real browser route when the endpoint is absent or returns HTML.
    let html_url = format!(
        "{origin}/search?q={}&type=4&mode=1&page={page}",
        urlencoding::encode(request.query.trim())
    );
    let response = http_get(&request.connection_id, &html_url, true)?;
    if browser_verification_response(&response.body) {
        return Err(PluginError::new(
            "browser-verification-required",
            "资源站要求完成浏览器验证",
        ));
    }
    if auth_required_response(response.status, &response.body) {
        return Err(PluginError::new("not-authenticated", "资源站登录已失效"));
    }
    if response.status == 429 {
        return Err(PluginError::new("rate-limited", "资源站请求受到限流"));
    }
    if !(200..300).contains(&response.status) {
        return Err(PluginError::new(
            "upstream-unavailable",
            "资源站入口暂时不可用",
        ));
    }
    let mut parsed = parse_html_search(&response.body, page)
        .ok_or_else(|| PluginError::new("invalid-response", "资源站搜索响应无效"))?;
    normalize_search_times(&mut parsed)?;
    Ok(json!(parsed))
}

fn normalize_search_times(response: &mut ResourceSearchResponse) -> Result<(), PluginError> {
    let needs_now = response.items.iter().any(|item| {
        item.updated_at
            .as_deref()
            .and_then(relative_age_seconds)
            .is_some()
    });
    let now = needs_now.then(host_now_unix).transpose()?;
    normalize_search_times_at(response, now);
    Ok(())
}

fn normalize_search_times_at(response: &mut ResourceSearchResponse, now_unix: Option<i64>) {
    for item in &mut response.items {
        let Some(raw) = item.updated_at.as_deref() else {
            continue;
        };
        item.updated_at = normalize_updated_at(raw, now_unix);
    }
}

fn normalize_updated_at(raw: &str, now_unix: Option<i64>) -> Option<String> {
    let raw = raw.trim();
    if relative_age_seconds(raw).is_some() {
        return normalize_relative_updated_at(raw, now_unix?);
    }
    if let Ok(value) = OffsetDateTime::parse(raw, &Rfc3339) {
        return value.format(&Rfc3339).ok();
    }
    let format = format_description!("[year]-[month]-[day] [hour]:[minute]:[second]");
    let local = PrimitiveDateTime::parse(raw, &format).ok()?;
    let china_standard_time = UtcOffset::from_hms(8, 0, 0).ok()?;
    local
        .assume_offset(china_standard_time)
        .to_offset(UtcOffset::UTC)
        .format(&Rfc3339)
        .ok()
}

fn normalize_relative_updated_at(raw: &str, now_unix: i64) -> Option<String> {
    let age_seconds = relative_age_seconds(raw)?;
    OffsetDateTime::from_unix_timestamp(now_unix)
        .ok()?
        .checked_sub(Duration::seconds(age_seconds))?
        .format(&Rfc3339)
        .ok()
}

fn relative_age_seconds(raw: &str) -> Option<i64> {
    let text = raw.trim();
    if text == "刚刚" {
        return Some(0);
    }
    if text == "昨天" {
        return Some(24 * 60 * 60);
    }
    if text == "前天" {
        return Some(2 * 24 * 60 * 60);
    }
    for (suffix, multiplier) in [
        ("秒前", 1_i64),
        ("分钟前", 60),
        ("小时前", 60 * 60),
        ("天前", 24 * 60 * 60),
        ("周前", 7 * 24 * 60 * 60),
        ("月前", 30 * 24 * 60 * 60),
        ("年前", 365 * 24 * 60 * 60),
    ] {
        let Some(number) = text.strip_suffix(suffix) else {
            continue;
        };
        let number = number.trim();
        if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let value = number.parse::<i64>().ok()?;
        if !(0..=1000).contains(&value) {
            return None;
        }
        return value.checked_mul(multiplier);
    }
    None
}

fn parse_json_search(body: &[u8], page: i32) -> Option<ResourceSearchResponse> {
    let payload: Value = serde_json::from_slice(body).ok()?;
    if browser_verification_required(&payload) || !search_payload_shape(&payload) {
        return None;
    }
    let rows = payload
        .get("inlist")
        .and_then(Value::as_array)
        .or_else(|| {
            payload
                .get("data")
                .and_then(|data| data.get("inlist"))
                .and_then(Value::as_array)
        })?;
    let mut items = Vec::new();
    for row in rows.iter().take(200) {
        let id = value_string(row, &["i", "id"]);
        let title = value_string(row, &["title", "name"]);
        if id.is_empty() || title.is_empty() {
            continue;
        }
        let updated = value_string(row, &["time", "updatedAt"]);
        items.push(ResourceSearchItem {
            id,
            title,
            size_bytes: parse_size(&value_string(row, &["size"])),
            seeders: value_i32(row, &["seeds", "seeders"]).max(0),
            updated_at: if updated.is_empty() {
                None
            } else {
                Some(updated)
            },
            tags: value_string(row, &["k", "type"])
                .split_whitespace()
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
                .take(8)
                .collect(),
        });
    }
    let has_next = payload
        .get("footer")
        .and_then(Value::as_object)
        .and_then(|footer| footer.get("next"))
        .and_then(Value::as_bool)
        .unwrap_or(items.len() >= 50);
    Some(ResourceSearchResponse {
        items,
        page,
        has_next,
    })
}

fn parse_html_search(body: &[u8], page: i32) -> Option<ResourceSearchResponse> {
    let html = std::str::from_utf8(body).ok()?;
    let document = Html::parse_document(html);
    let table_selector = Selector::parse("table").ok()?;
    let row_selector = Selector::parse("tr").ok()?;
    let cell_selector = Selector::parse("th, td").ok()?;
    let link_selector = Selector::parse("a[href]").ok()?;

    let mut recognized_table = false;
    let mut items = Vec::new();
    for table in document.select(&table_selector) {
        let mut columns = None;
        for row in table.select(&row_selector) {
            let cells = row.select(&cell_selector).collect::<Vec<_>>();
            if cells.is_empty() {
                continue;
            }
            if columns.is_none() {
                let headings = cells
                    .iter()
                    .map(|cell| normalized_text(cell.text()))
                    .collect::<Vec<_>>();
                columns = search_columns(&headings);
                if columns.is_some() {
                    recognized_table = true;
                    continue;
                }
            }
            let Some((title_index, size_index, seeders_index, updated_index)) = columns else {
                continue;
            };
            if items.len() >= 200
                || [title_index, size_index, seeders_index, updated_index]
                    .into_iter()
                    .any(|index| index >= cells.len())
            {
                continue;
            }
            let Some(link) = cells[title_index].select(&link_selector).find(|link| {
                link.value()
                    .attr("href")
                    .and_then(resource_id_from_href)
                    .is_some()
            }) else {
                continue;
            };
            let Some(id) = link.value().attr("href").and_then(resource_id_from_href) else {
                continue;
            };
            let title = normalized_text(link.text());
            if title.is_empty() {
                continue;
            }
            let updated = normalized_text(cells[updated_index].text());
            items.push(ResourceSearchItem {
                id,
                title,
                size_bytes: parse_size(&normalized_text(cells[size_index].text())),
                seeders: parse_non_negative_i32(&normalized_text(cells[seeders_index].text())),
                updated_at: (!updated.is_empty()).then_some(updated),
                tags: Vec::new(),
            });
        }
    }
    if !recognized_table {
        return None;
    }
    let has_next = document.select(&link_selector).any(|link| {
        link.value()
            .attr("href")
            .is_some_and(|href| link_targets_later_page(href, page))
    });
    Some(ResourceSearchResponse {
        items,
        page,
        has_next,
    })
}

fn search_columns(headings: &[String]) -> Option<(usize, usize, usize, usize)> {
    let find = |needles: &[&str]| {
        headings
            .iter()
            .position(|heading| needles.iter().any(|needle| heading.contains(needle)))
    };
    Some((
        find(&["名称", "标题", "资源"])?,
        find(&["大小", "size"])?,
        find(&["做种", "种子", "seed"])?,
        find(&["更新", "时间", "updated"])?,
    ))
}

fn normalized_text<'a>(parts: impl Iterator<Item = &'a str>) -> String {
    parts
        .flat_map(str::split_whitespace)
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_owned()
}

fn resource_id_from_href(raw: &str) -> Option<String> {
    let parsed = url::Url::parse(raw)
        .or_else(|_| url::Url::parse("https://plugin.invalid/").and_then(|base| base.join(raw)))
        .ok()?;
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return None;
    }
    let segments = parsed.path_segments()?.collect::<Vec<_>>();
    if segments.len() != 2 || segments[0] != "bt" {
        return None;
    }
    let id = segments[1].trim();
    if id.is_empty()
        || id.len() > 128
        || !id
            .bytes()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, b'-' | b'_'))
    {
        return None;
    }
    Some(id.to_owned())
}

fn parse_non_negative_i32(raw: &str) -> i32 {
    let digits = raw
        .chars()
        .filter(|character| character.is_ascii_digit())
        .take(10)
        .collect::<String>();
    digits.parse::<i32>().unwrap_or(0).max(0)
}

fn link_targets_later_page(raw: &str, current_page: i32) -> bool {
    let parsed = url::Url::parse(raw)
        .or_else(|_| url::Url::parse("https://plugin.invalid/").and_then(|base| base.join(raw)));
    let Ok(parsed) = parsed else { return false };
    parsed.path() == "/search"
        && parsed
            .query_pairs()
            .find(|(key, _)| key == "page")
            .and_then(|(_, value)| value.parse::<i32>().ok())
            .is_some_and(|page| page > current_page)
}

fn resource_resolve(request: ResourceResolveRequest) -> Result<Value, PluginError> {
    validate_connection(&request.connection_id)?;
    if request.resource_id.trim().is_empty() || request.resource_id.len() > 256 {
        return Err(PluginError::new("invalid-request", "资源站资源 ID 无效"));
    }
    let origin = entry_origin(&request.connection_id)?;
    let response = http_get(
        &request.connection_id,
        &format!(
            "{origin}/bt/{}",
            urlencoding::encode(request.resource_id.trim())
        ),
        true,
    )?;
    if auth_required_response(response.status, &response.body) {
        return Err(PluginError::new("not-authenticated", "资源站登录已失效"));
    }
    if response.status == 429 {
        return Err(PluginError::new("rate-limited", "资源站请求受到限流"));
    }
    if !(200..300).contains(&response.status) {
        return Err(PluginError::new(
            "upstream-unavailable",
            "资源站入口暂时不可用",
        ));
    }
    let html = String::from_utf8_lossy(&response.body);
    let magnet =
        extract_magnet(&html).ok_or_else(|| PluginError::new("not-found", "详情页没有磁力链接"))?;
    Ok(json!(ResourceResolveResponse { magnet }))
}
fn resource_health(request: ResourceHealthRequest) -> Result<Value, PluginError> {
    validate_connection(&request.connection_id)?;
    let origin = entry_origin(&request.connection_id)?;
    let response = http_get(
        &request.connection_id,
        &format!("{origin}/res/search?q=test&type=4&ziyuan=&mode=1&page=1"),
        true,
    )?;
    if response.status == 429 || browser_verification_response(&response.body) {
        return Ok(json!(ResourceHealthResponse {
            status: "rate_limited".to_owned(),
            account_name: None,
        }));
    }
    if auth_required_response(response.status, &response.body) {
        return Ok(json!(ResourceHealthResponse {
            status: "auth_required".to_owned(),
            account_name: None,
        }));
    }
    if (200..300).contains(&response.status) && parse_json_search(&response.body, 1).is_some() {
        return Ok(json!(ResourceHealthResponse {
            status: "healthy".to_owned(),
            account_name: None,
        }));
    }
    if !(200..300).contains(&response.status) && response.status != 404 && response.status != 405 {
        return Ok(json!(ResourceHealthResponse {
            status: "unavailable".to_owned(),
            account_name: None,
        }));
    }

    let response = http_get(
        &request.connection_id,
        &format!("{origin}/search?q=test&type=4&mode=1&page=1"),
        true,
    )?;
    if response.status == 429 || browser_verification_response(&response.body) {
        return Ok(json!(ResourceHealthResponse {
            status: "rate_limited".to_owned(),
            account_name: None,
        }));
    }
    if auth_required_response(response.status, &response.body) {
        return Ok(json!(ResourceHealthResponse {
            status: "auth_required".to_owned(),
            account_name: None,
        }));
    }
    let status = if (200..300).contains(&response.status)
        && parse_html_search(&response.body, 1).is_some()
    {
        "healthy"
    } else {
        "unavailable"
    };
    Ok(json!(ResourceHealthResponse {
        status: status.to_owned(),
        account_name: None
    }))
}

fn resource_login(request: ResourceLoginRequest) -> Result<Value, PluginError> {
    validate_connection(&request.connection_id)?;
    if request.username.chars().count() < 2
        || request.username.chars().count() > 128
        || request.password.chars().count() < 6
        || request.password.chars().count() > 256
    {
        return Err(PluginError::new("invalid-request", "账号或密码长度无效"));
    }
    let origin = entry_origin(&request.connection_id)?;
    let body = login_form(&request.username, &request.password, None);
    let response = http_post_with_capture(
        &request.connection_id,
        &format!("{origin}/user/login"),
        &body,
        false,
    )?;
    if response.status == 429 || browser_verification_response(&response.body) {
        return Err(PluginError::new("rate-limited", "资源站请求受到限流"));
    }
    if let Some(message) = json_login_success(&response.body) {
        if message {
            return commit_capture(
                &request.connection_id,
                response.capture_ref.as_deref(),
                &request.username,
            );
        }
    }
    if response.status == 401
        || response.status == 403
        || String::from_utf8_lossy(&response.body).contains("验证码")
    {
        let mut has_session_cookie = false;
        if let Some(reference) = response.capture_ref.as_deref() {
            commit_capture_session(&request.connection_id, reference)?;
            has_session_cookie = true;
        }
        let (captcha, has_session_cookie) =
            fetch_captcha(&request.connection_id, &origin, has_session_cookie)?;
        let now = host_now_unix()?;
        let mut pending = pending().lock().unwrap();
        pending.retain(|_, login| login.expires_at > now);
        pending.insert(
            captcha.challenge_id.clone(),
            PendingLogin {
                connection_id: request.connection_id.clone(),
                username: request.username.clone(),
                password: request.password.clone(),
                expires_at: now + 120,
                has_session_cookie,
            },
        );
        return Ok(json!(LoginResponse {
            state: "captcha_required".to_owned(),
            account_name: None,
            challenge: Some(captcha),
            error_code: None
        }));
    }
    Ok(json!(LoginResponse {
        state: "failed".to_owned(),
        account_name: None,
        challenge: None,
        error_code: Some("resource_auth_failed".to_owned())
    }))
}

fn resource_captcha(request: ResourceCaptchaRequest) -> Result<Value, PluginError> {
    if request.points.is_empty()
        || request.points.len() > 8
        || request.challenge_id.is_empty()
        || request
            .points
            .iter()
            .any(|point| point.x < 0 || point.x >= 320 || point.y < 0 || point.y >= 100)
    {
        return Err(PluginError::new("invalid-request", "验证码坐标无效"));
    }
    let now = host_now_unix()?;
    let pending_login = pending()
        .lock()
        .unwrap()
        .remove(&request.challenge_id)
        .ok_or_else(|| PluginError::new("captcha-expired", "验证码挑战已过期"))?;
    if pending_login.connection_id != request.connection_id || pending_login.expires_at <= now {
        return Err(PluginError::new("captcha-expired", "验证码挑战已过期"));
    }
    let code = request
        .points
        .iter()
        .map(|p| format!("{},{}", p.x.max(0), p.y.max(0)))
        .collect::<Vec<_>>()
        .join(";");
    let origin = entry_origin(&request.connection_id)?;
    let body = login_form(
        &pending_login.username,
        &pending_login.password,
        Some(&code),
    );
    let response = http_post_with_capture(
        &request.connection_id,
        &format!("{origin}/user/login"),
        &body,
        pending_login.has_session_cookie,
    )?;
    if response.status == 429 || browser_verification_response(&response.body) {
        return Err(PluginError::new("rate-limited", "资源站请求受到限流"));
    }
    if json_login_success(&response.body).unwrap_or(false) {
        if let Some(reference) = response.capture_ref.as_deref() {
            commit_capture_session(&request.connection_id, reference)?;
        } else if !pending_login.has_session_cookie {
            return Err(PluginError::new("auth-failed", "登录未返回 Cookie"));
        }
        return Ok(authenticated_response(Some(&pending_login.username)));
    }
    Err(PluginError::new("auth-failed", "验证码或账号密码不正确"))
}
fn resource_cookie(request: ResourceCookieRequest) -> Result<Value, PluginError> {
    validate_connection(&request.connection_id)?;
    if request.cookie.trim().is_empty() {
        return Err(PluginError::new("invalid-request", "Cookie 不能为空"));
    }
    Ok(json!(LoginResponse {
        state: "authenticated".to_owned(),
        account_name: None,
        challenge: None,
        error_code: None
    }))
}

fn login_form(username: &str, password: &str, code: Option<&str>) -> Zeroizing<String> {
    let mut body = Zeroizing::new(format!(
        "siteid=1&dosubmit=1&cookietime=10506240&username={}&password={}",
        urlencoding::encode(username),
        urlencoding::encode(password)
    ));
    if let Some(code) = code {
        body.push_str("&code=");
        body.push_str(&urlencoding::encode(code));
    }
    body
}

fn fetch_captcha(
    connection_id: &str,
    origin: &str,
    had_session_cookie: bool,
) -> Result<(CaptchaChallenge, bool), PluginError> {
    let response = http_get_with_capture(
        connection_id,
        &format!("{origin}/res/captcha/2"),
        had_session_cookie,
    )?;
    if response.status == 429 || browser_verification_response(&response.body) {
        return Err(PluginError::new("rate-limited", "资源站请求受到限流"));
    }
    if !(200..300).contains(&response.status) {
        return Err(PluginError::new("captcha-required", "验证码图片获取失败"));
    }
    let mut has_session_cookie = had_session_cookie;
    if let Some(reference) = response.capture_ref.as_deref() {
        commit_capture_session(connection_id, reference)?;
        has_session_cookie = true;
    }
    let content_type = captcha_content_type(&response.body)
        .ok_or_else(|| PluginError::new("captcha-required", "验证码图片无效"))?;
    let asset = host_json(
        HOST_ASSET_REGISTER,
        &json!({"connectionId":connection_id,"bodyBase64":BASE64.encode(&response.body),"contentType":content_type,"ttlSeconds":120}),
    )?;
    let reference = asset
        .get("ref")
        .and_then(Value::as_str)
        .ok_or_else(|| PluginError::new("invalid-response", "验证码图片注册失败"))?;
    // The Host-generated opaque asset reference is random, short-lived and
    // already exposed with the challenge, so it is also a collision-resistant
    // challenge identity without adding guest randomness.
    let challenge_id = reference.to_owned();
    Ok((
        CaptchaChallenge {
            challenge_id,
            image_asset_ref: reference.to_owned(),
            width: 320,
            height: 100,
            prompt: "请按顺序点击验证码字符".to_owned(),
            max_points: 8,
        },
        has_session_cookie,
    ))
}
fn commit_capture(
    connection_id: &str,
    capture_ref: Option<&str>,
    username: &str,
) -> Result<Value, PluginError> {
    let reference =
        capture_ref.ok_or_else(|| PluginError::new("auth-failed", "登录未返回 Cookie"))?;
    commit_capture_session(connection_id, reference)?;
    Ok(authenticated_response(Some(username)))
}

fn authenticated_response(username: Option<&str>) -> Value {
    json!(LoginResponse {
        state: "authenticated".to_owned(),
        account_name: username.map(str::to_owned),
        challenge: None,
        error_code: None
    })
}

fn commit_capture_session(connection_id: &str, capture_ref: &str) -> Result<(), PluginError> {
    host_json(
        HOST_CREDENTIAL_COMMIT,
        &json!({"connectionId":connection_id,"scope":SESSION_SCOPE,"captureRef":capture_ref}),
    )?;
    Ok(())
}

#[derive(Zeroize, ZeroizeOnDrop)]
struct HttpResponse {
    status: i32,
    body: Vec<u8>,
    capture_ref: Option<String>,
}
fn http_get(
    connection_id: &str,
    url: &str,
    authenticated: bool,
) -> Result<HttpResponse, PluginError> {
    http_get_inner(connection_id, url, authenticated, false)
}

fn http_get_with_capture(
    connection_id: &str,
    url: &str,
    authenticated: bool,
) -> Result<HttpResponse, PluginError> {
    http_get_inner(connection_id, url, authenticated, true)
}

fn http_get_inner(
    connection_id: &str,
    url: &str,
    authenticated: bool,
    capture: bool,
) -> Result<HttpResponse, PluginError> {
    let mut request = json!({"connectionId":connection_id,"method":"GET","url":url,"headers":{"Accept":"application/json, text/html","Referer":url,"User-Agent":USER_AGENT},"timeoutMs":12000});
    if authenticated {
        request["credentialRef"] = Value::String(SESSION_SCOPE.to_owned());
    }
    if capture {
        request["captureCredentialScope"] = Value::String(SESSION_SCOPE.to_owned());
    }
    parse_http(host_json(HOST_HTTP, &request)?)
}

fn http_post_with_capture(
    connection_id: &str,
    url: &str,
    body: &str,
    authenticated: bool,
) -> Result<HttpResponse, PluginError> {
    let mut request = json!({"connectionId":connection_id,"method":"POST","url":url,"headers":{"Accept":"application/json, text/html","Content-Type":"application/x-www-form-urlencoded","Referer":url,"User-Agent":USER_AGENT},"bodyBase64":BASE64.encode(body.as_bytes()),"timeoutMs":12000});
    if authenticated {
        request["credentialRef"] = Value::String(SESSION_SCOPE.to_owned());
    }
    request["captureCredentialScope"] = Value::String(SESSION_SCOPE.to_owned());
    let result = host_json(HOST_HTTP, &request);
    if let Value::String(encoded) = &mut request["bodyBase64"] {
        encoded.zeroize();
    }
    parse_http(result?)
}
fn parse_http(value: Value) -> Result<HttpResponse, PluginError> {
    let status = value.get("status").and_then(Value::as_i64).unwrap_or(0) as i32;
    let body = BASE64
        .decode(
            value
                .get("bodyBase64")
                .and_then(Value::as_str)
                .unwrap_or(""),
        )
        .map_err(|_| PluginError::new("invalid-response", "站点响应正文无效"))?;
    Ok(HttpResponse {
        status,
        body,
        capture_ref: value
            .get("credentialCaptureRef")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}
fn json_login_success(body: &[u8]) -> Option<bool> {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v.get("code").and_then(Value::as_i64).map(|code| code == 0))
        .or_else(|| {
            let text = String::from_utf8_lossy(body);
            if text.contains("登录成功") {
                Some(true)
            } else {
                None
            }
        })
}

fn browser_verification_required(payload: &Value) -> bool {
    payload
        .get("code")
        .and_then(Value::as_i64)
        .map(|code| code == 419)
        .unwrap_or(false)
        || payload
            .get("msg")
            .and_then(Value::as_str)
            .map(|message| message.contains("浏览器验证") || message.contains("安全验证"))
            .unwrap_or(false)
}

fn browser_verification_response(body: &[u8]) -> bool {
    if let Ok(payload) = serde_json::from_slice::<Value>(body) {
        return browser_verification_required(&payload);
    }
    let text = String::from_utf8_lossy(body);
    text.contains("浏览器安全验证")
        || text.contains("验证完成后自动继续")
        || text.contains("安全验证") && text.contains("浏览器")
}

fn auth_required_response(status: i32, body: &[u8]) -> bool {
    if status == 401 || status == 403 {
        return true;
    }
    if let Ok(payload) = serde_json::from_slice::<Value>(body) {
        let code = payload.get("code").and_then(Value::as_i64);
        let message = value_string(&payload, &["msg", "message"]);
        return matches!(code, Some(401 | 403))
            || message.contains("请登录")
            || message.contains("登录失效")
            || message.to_ascii_lowercase().contains("not authenticated");
    }
    let text = String::from_utf8_lossy(body).to_ascii_lowercase();
    text.contains("/user/login")
        && (text.contains("password") || text.contains("请登录") || text.contains("登录后"))
}

fn search_payload_shape(payload: &Value) -> bool {
    payload.get("inlist").is_some_and(Value::is_array)
        || payload
            .get("data")
            .and_then(|data| data.get("inlist"))
            .is_some_and(Value::is_array)
}

fn captcha_content_type(body: &[u8]) -> Option<&'static str> {
    if body.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if body.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if body.len() >= 12 && &body[..4] == b"RIFF" && &body[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

fn host_now_unix() -> Result<i64, PluginError> {
    let raw = host_json(HOST_NOW, &json!({}))?
        .get("now")
        .and_then(Value::as_str)
        .ok_or_else(|| PluginError::new("invalid-response", "宿主时间响应无效"))?
        .to_owned();
    OffsetDateTime::parse(&raw, &Rfc3339)
        .map(|value| value.unix_timestamp())
        .map_err(|_| PluginError::new("invalid-response", "宿主时间响应无效"))
}

fn validate_connection(connection_id: &str) -> Result<(), PluginError> {
    if connection_id.is_empty()
        || connection_id.len() > 128
        || connection_id.chars().any(char::is_control)
    {
        Err(PluginError::new("invalid-request", "连接无效"))
    } else {
        Ok(())
    }
}
fn entry_origin(connection_id: &str) -> Result<String, PluginError> {
    let config = host_json(HOST_CONFIG_GET, &json!({"connectionId":connection_id}))?;
    let raw = config
        .get("entryOrigin")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let parsed =
        url::Url::parse(raw).map_err(|_| PluginError::new("invalid-request", "镜像入口无效"))?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || parsed.port().is_some()
        || parsed.username() != ""
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.path() != "/"
    {
        return Err(PluginError::new("invalid-request", "镜像入口无效"));
    }
    Ok(format!("https://{}", parsed.host_str().unwrap()))
}
fn value_string(value: &Value, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|key| {
            value.get(*key).and_then(|item| {
                item.as_str()
                    .map(str::to_owned)
                    .or_else(|| item.as_i64().map(|number| number.to_string()))
                    .or_else(|| item.as_u64().map(|number| number.to_string()))
            })
        })
        .unwrap_or_default()
}
fn value_i32(value: &Value, keys: &[&str]) -> i32 {
    keys.iter()
        .find_map(|key| {
            value
                .get(*key)
                .and_then(|v| {
                    v.as_i64()
                        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                })
                .map(|v| v as i32)
        })
        .unwrap_or(0)
}
fn parse_size(raw: &str) -> i64 {
    let compact = raw.replace(',', "").trim().to_owned();
    let split_at = compact
        .char_indices()
        .find(|(_, character)| !character.is_ascii_digit() && *character != '.')
        .map(|(index, _)| index)
        .unwrap_or(compact.len());
    let (number, suffix) = compact.split_at(split_at);
    let value = number
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value >= 0.0)
        .unwrap_or(0.0);
    let multiplier = match suffix.trim().to_ascii_uppercase().as_str() {
        "K" | "KB" | "KIB" => 1024.0,
        "M" | "MB" | "MIB" => 1024.0 * 1024.0,
        "G" | "GB" | "GIB" => 1024.0 * 1024.0 * 1024.0,
        "T" | "TB" | "TIB" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        "" | "B" => 1.0,
        _ => return 0,
    };
    let bytes = value * multiplier;
    if bytes >= i64::MAX as f64 {
        i64::MAX
    } else {
        bytes as i64
    }
}
fn extract_magnet(html: &str) -> Option<String> {
    let marker = "magnet:?xt=urn:btih:";
    let lower = html.to_ascii_lowercase();
    let mut offset = 0;
    while let Some(relative) = lower[offset..].find(marker) {
        let start = offset + relative;
        let rest = &html[start..];
        let end = rest
            .find(['"', '\'', '<', ' ', '\r', '\n'])
            .unwrap_or(rest.len());
        let mut magnet = rest[..end].replace("&amp;", "&");
        let hash = magnet.get(marker.len()..)?.split('&').next()?.trim();
        if hash.len() == 40 && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            magnet.truncate(magnet.find('&').unwrap_or(magnet.len()));
            return Some(magnet);
        }
        offset = start.saturating_add(marker.len());
        if offset >= lower.len() {
            break;
        }
    }
    None
}

#[derive(Clone, Copy)]
struct PluginError {
    code: &'static str,
    message: &'static str,
}
impl PluginError {
    fn new(code: &'static str, message: &'static str) -> Self {
        Self { code, message }
    }
}
fn plugin_error(code: &str, message: &str) -> Value {
    json!({"pluginError":{"code":code,"message":message}})
}
fn parse<T: for<'de> Deserialize<'de>>(request: &[u8]) -> Result<T, PluginError> {
    serde_json::from_slice(request).map_err(|_| PluginError::new("invalid-request", "请求格式无效"))
}
fn encode(value: Value) -> i64 {
    let bytes = serde_json::to_vec(&value).unwrap_or_else(|_| {
        serde_json::to_vec(&json!({"pluginError":{"code":"internal","message":"internal error"}}))
            .unwrap_or_default()
    });
    let mut buffer = bytes.into_boxed_slice();
    let pointer = buffer.as_mut_ptr() as u32;
    let length = buffer.len() as u32;
    mem::forget(buffer);
    ((pointer as u64) << 32 | length as u64) as i64
}
fn host_json(operation: i32, request: &Value) -> Result<Value, PluginError> {
    let bytes = Zeroizing::new(
        serde_json::to_vec(request).map_err(|_| PluginError::new("internal", "请求编码失败"))?,
    );
    let mut response = Zeroizing::new(vec![0_u8; HOST_BUFFER_BYTES]);
    let length = unsafe {
        host_call(
            operation,
            bytes.as_ptr() as i32,
            bytes.len() as i32,
            response.as_mut_ptr() as i32,
            response.len() as i32,
        )
    };
    if length < 0 {
        return Err(PluginError::new(
            if length == -2 {
                "permission-denied"
            } else if operation == HOST_HTTP {
                "upstream-unavailable"
            } else {
                "internal"
            },
            "宿主能力调用失败",
        ));
    }
    response.truncate(length as usize);
    let envelope: Value = serde_json::from_slice(&response)
        .map_err(|_| PluginError::new("invalid-response", "宿主响应无效"))?;
    envelope
        .get("data")
        .cloned()
        .ok_or_else(|| PluginError::new("invalid-response", "宿主响应缺少数据"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_compact_site_sizes() {
        assert_eq!(parse_size("4.6GB"), 4_939_212_390);
        assert_eq!(parse_size("700 MB"), 734_003_200);
        assert_eq!(parse_size("1.5 GiB"), 1_610_612_736);
        assert_eq!(parse_size("unknown"), 0);
    }

    #[test]
    fn extracts_only_valid_magnet_and_ignores_bad_candidate() {
        let valid = "42B8A6E4F45924B949389B4C01B777686B40FB58";
        let html = format!(
            "<a href=\"magnet:?xt=urn:btih:too-short\">bad</a><a href=\"MAGNET:?XT=URN:BTIH:{valid}&dn=x\">good</a>"
        );
        assert_eq!(
            extract_magnet(&html),
            Some(format!("MAGNET:?XT=URN:BTIH:{valid}"))
        );
        assert!(extract_magnet("<a href=\"magnet:?xt=urn:btih:not-a-hash\">x</a>").is_none());
    }

    #[test]
    fn login_success_accepts_site_json_and_success_text() {
        assert_eq!(json_login_success(br#"{"code":0}"#), Some(true));
        assert_eq!(json_login_success(br#"{"code":1}"#), Some(false));
        assert_eq!(json_login_success("登录成功".as_bytes()), Some(true));
        assert_eq!(json_login_success(br#"{}"#), None);
    }

    #[test]
    fn distinguishes_login_html_from_a_healthy_search_payload() {
        assert!(auth_required_response(
            200,
            br#"<form action='/user/login'><input type='password'></form>"#
        ));
        let payload: Value = serde_json::from_slice(br#"{"inlist":[]}"#).unwrap();
        assert!(search_payload_shape(&payload));
        assert!(!auth_required_response(200, br#"{"inlist":[]}"#));
    }

    #[test]
    fn accepts_only_supported_captcha_image_formats() {
        assert_eq!(
            captcha_content_type(b"\x89PNG\r\n\x1a\nrest"),
            Some("image/png")
        );
        assert_eq!(
            captcha_content_type(b"\xff\xd8\xffrest"),
            Some("image/jpeg")
        );
        assert_eq!(
            captcha_content_type(b"RIFF0000WEBPrest"),
            Some("image/webp")
        );
        assert_eq!(captcha_content_type(b"<html>not an image</html>"), None);
    }

    #[test]
    fn reads_numeric_resource_identifiers_without_loss() {
        let row = json!({"i": 123456789_u64});
        assert_eq!(value_string(&row, &["i", "id"]), "123456789");
    }

    #[test]
    fn parses_observed_html_search_table_and_next_page() {
        let page = parse_html_search(include_bytes!("fixtures/search.html"), 1).unwrap();
        assert_eq!(page.items.len(), 5);
        assert_eq!(page.items[0].id, "987654");
        assert_eq!(page.items[0].title, "示例资源 S01E01 2160p");
        assert_eq!(page.items[0].size_bytes, 4_939_212_390);
        assert_eq!(page.items[0].seeders, 18);
        assert_eq!(page.items[0].updated_at.as_deref(), Some("4月前"));
        assert_eq!(page.items[1].id, "abc_123");
        assert_eq!(page.items[1].seeders, 1205);
        assert_eq!(page.items[2].updated_at.as_deref(), Some("12分钟前"));
        assert_eq!(page.items[3].updated_at.as_deref(), Some("昨天"));
        assert_eq!(page.items[4].updated_at.as_deref(), Some("前天"));
        assert!(page.has_next);
    }

    #[test]
    fn normalizes_observed_relative_update_times_at_the_plugin_boundary() {
        let now = OffsetDateTime::from_unix_timestamp(1_757_577_600).unwrap();
        for (input, expected) in [
            ("4月前", "2025-05-14T08:00:00Z"),
            ("12分钟前", "2025-09-11T07:48:00Z"),
            ("昨天", "2025-09-10T08:00:00Z"),
            ("前天", "2025-09-09T08:00:00Z"),
        ] {
            assert_eq!(
                normalize_relative_updated_at(input, now.unix_timestamp()).as_deref(),
                Some(expected),
                "input={input}"
            );
        }
        assert_eq!(relative_age_seconds("2026-09-11 08:09:10"), None);

        let mut page = parse_html_search(include_bytes!("fixtures/search.html"), 1).unwrap();
        normalize_search_times_at(&mut page, Some(now.unix_timestamp()));
        assert_eq!(
            page.items
                .iter()
                .map(|item| item.updated_at.as_deref())
                .collect::<Vec<_>>(),
            vec![
                Some("2025-05-14T08:00:00Z"),
                Some("2026-09-09T23:08:09Z"),
                Some("2025-09-11T07:48:00Z"),
                Some("2025-09-10T08:00:00Z"),
                Some("2025-09-09T08:00:00Z"),
            ]
        );
        assert_eq!(
            normalize_updated_at("2025-09-11T08:00:00+08:00", None).as_deref(),
            Some("2025-09-11T08:00:00+08:00")
        );
        assert_eq!(normalize_updated_at("not-a-time", None), None);
    }

    #[test]
    fn rejects_malformed_overflowing_or_unbounded_relative_update_times() {
        for input in [
            "",
            "分钟前",
            "+1分钟前",
            "-1分钟前",
            "1.5小时前",
            "十二分钟前",
            "1001年前",
            "999999999999999999999999999999年前",
            "1个月前",
        ] {
            assert_eq!(relative_age_seconds(input), None, "input={input}");
        }
        assert_eq!(relative_age_seconds("0秒前"), Some(0));
        assert_eq!(relative_age_seconds("1000年前"), Some(31_536_000_000));
    }

    #[test]
    fn html_search_requires_known_columns_and_safe_detail_identity() {
        assert!(parse_html_search(b"<table><tr><th>other</th></tr></table>", 1).is_none());
        assert_eq!(resource_id_from_href("/bt/123"), Some("123".to_owned()));
        assert!(resource_id_from_href("/bt/123?next=evil").is_none());
        assert!(resource_id_from_href("/bt/../admin").is_none());
        assert!(resource_id_from_href("/other/123").is_none());
    }

    #[test]
    fn detects_browser_security_verification_html() {
        assert!(browser_verification_response(
            "<title>浏览器安全验证</title><p>验证完成后自动继续</p>".as_bytes()
        ));
        assert!(!browser_verification_response(
            "<title>搜索</title><p>正常内容</p>".as_bytes()
        ));
    }

    #[test]
    fn login_form_matches_the_site_frontend_contract() {
        let without_captcha = login_form("name@example.com", "p&a=ss", None);
        assert_eq!(
            without_captcha.as_str(),
            "siteid=1&dosubmit=1&cookietime=10506240&username=name%40example.com&password=p%26a%3Dss"
        );
        let with_captcha = login_form("user", "secret", Some("10,20;30,40"));
        assert!(with_captcha.ends_with("&code=10%2C20%3B30%2C40"));
        assert!(!with_captcha.contains("captchainfo"));
    }
}
