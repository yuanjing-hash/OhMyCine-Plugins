use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{mem, slice};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use url::Url;

mod auth;
mod catalog;
mod media;

const SESSION_SCOPE: &str = "mgtv.session";
const HOST_BUFFER_BYTES: usize = 4 * 1024 * 1024;
const USER_AGENT: &str = "Mozilla/5.0 OhMyCine MangoTVPlugin/0.1";

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
/// # Safety
/// The pointer and length must identify a live allocation from `omc_alloc`.
pub unsafe extern "C" fn omc_free(pointer: i32, length: i32) {
    if pointer > 0 && length >= 0 {
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
            pointer as *mut u8,
            length as usize,
        )));
    }
}
#[no_mangle]
/// # Safety
/// The input range must be readable guest memory for the duration of this call.
pub unsafe extern "C" fn omc_invoke(operation: i32, pointer: i32, length: i32) -> i64 {
    let result = if pointer <= 0 || length < 0 || length as usize > 128 * 1024 {
        Err(Error::new("invalid-request", "插件请求无效"))
    } else {
        dispatch(
            operation,
            slice::from_raw_parts(pointer as *const u8, length as usize),
        )
    };
    let response = match result {
        Ok(value) => value,
        Err(error) => error.envelope(),
    };
    let bytes = serde_json::to_vec(&response).unwrap_or_default();
    let length = bytes.len();
    let buffer = bytes.into_boxed_slice();
    let pointer = buffer.as_ptr();
    mem::forget(buffer);
    ((pointer as i64) << 32) | length as i64
}
fn dispatch(operation: i32, request: &[u8]) -> Result<Value, Error> {
    match operation {
        1 => catalog::navigation(parse(request)?),
        2 => catalog::feed(parse(request)?),
        3 => catalog::search(parse(request)?),
        4 => catalog::detail(parse(request)?),
        5 => media::playback(parse(request)?),
        10 => auth::start(parse(request)?),
        11 => auth::poll(parse(request)?),
        12 => catalog::metadata(parse(request)?),
        13 => catalog::artwork(parse(request)?),
        20 => media::offline(parse(request)?),
        _ => Err(Error::new("invalid-request", "此只读媒体库不支持该操作")),
    }
}
#[derive(Debug)]
struct Error {
    code: &'static str,
    message: &'static str,
    reason: Option<&'static str>,
}
impl Error {
    fn new(code: &'static str, message: &'static str) -> Self {
        Self {
            code,
            message,
            reason: None,
        }
    }
    fn because(mut self, reason: &'static str) -> Self {
        self.reason = Some(reason);
        self
    }
    fn envelope(self) -> Value {
        let mut value = json!({"pluginError":{"code":self.code,"message":self.message}});
        if let Some(reason) = self.reason {
            value["pluginError"]["reason"] = json!(reason);
        }
        value
    }
    fn response() -> Self {
        Self::new("invalid-response", "芒果接口响应无效")
    }
    fn request() -> Self {
        Self::new("invalid-request", "媒体身份或栏目无效")
    }
}
fn parse<T: for<'de> Deserialize<'de>>(request: &[u8]) -> Result<T, Error> {
    serde_json::from_slice(request).map_err(|_| Error::request())
}
fn host(operation: i32, request: &Value) -> Result<Value, Error> {
    let bytes = serde_json::to_vec(request).map_err(|_| Error::response())?;
    let mut buffer = vec![0_u8; HOST_BUFFER_BYTES];
    let size = unsafe {
        host_call(
            operation,
            bytes.as_ptr() as i32,
            bytes.len() as i32,
            buffer.as_mut_ptr() as i32,
            buffer.len() as i32,
        )
    };
    if size < 0 || size as usize > buffer.len() {
        let error = Error::new(
            if size == -2 {
                "permission-denied"
            } else {
                "internal"
            },
            if operation == 1 && size == -2 {
                "网络访问被拒绝，请检查插件域名授权及 DNS 是否解析到私网或代理 fake-IP"
            } else {
                "宿主能力调用失败"
            },
        );
        return Err(if operation == 1 && size == -2 {
            error.because("network-access-denied")
        } else {
            error
        });
    }
    buffer.truncate(size as usize);
    serde_json::from_slice::<Value>(&buffer)
        .map_err(|_| Error::response())?
        .get("data")
        .cloned()
        .ok_or_else(Error::response)
}
fn now() -> Result<OffsetDateTime, Error> {
    let response = host(5, &json!({}))?;
    OffsetDateTime::parse(string(&response, "now")?, &Rfc3339).map_err(|_| Error::response())
}
fn rfc3339(value: OffsetDateTime) -> Result<String, Error> {
    value.format(&Rfc3339).map_err(|_| Error::response())
}
fn http(connection: &str, url: &str, authenticated: bool) -> Result<Value, Error> {
    validate_connection(connection)?;
    let mut request = json!({"connectionId":connection,"method":"GET","url":url,
        "headers":{"Accept":"application/json","Referer":"https://www.mgtv.com/","User-Agent":USER_AGENT},"timeoutMs":15000});
    if authenticated {
        request["credentialRef"] = json!(SESSION_SCOPE);
    }
    host(1, &request)
}
fn decode_http(response: &Value) -> Result<Value, Error> {
    if !(200..300).contains(&response.get("status").and_then(Value::as_u64).unwrap_or(0)) {
        return Err(Error::new("upstream-unavailable", "芒果接口请求失败"));
    }
    // HTTP payloads are bounded by the Host ABI, but are routinely larger
    // than an individual display/identity string (4 KiB).
    let encoded = response
        .get("bodyBase64")
        .and_then(Value::as_str)
        .filter(|value| value.len() <= HOST_BUFFER_BYTES)
        .ok_or_else(Error::response)?;
    let body = BASE64.decode(encoded).map_err(|_| Error::response())?;
    serde_json::from_slice(&body).map_err(|_| Error::response())
}

fn get(connection: &str, url: &str, authenticated: bool) -> Result<Value, Error> {
    let payload = decode_http(&http(connection, url, authenticated)?)?;
    provider_data(&payload).cloned()
}
fn provider_data(payload: &Value) -> Result<&Value, Error> {
    match number(payload.get("code")) {
        Some(200) => payload.get("data").ok_or_else(Error::response),
        Some(101) => Err(Error::new(
            "not-authenticated",
            "芒果登录已失效，请重新扫码",
        )),
        _ => Err(Error::new("upstream-unavailable", "芒果拒绝了当前请求")),
    }
}
fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, Error> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= 4096 && !s.chars().any(char::is_control))
        .ok_or_else(Error::response)
}
fn number(value: Option<&Value>) -> Option<u64> {
    value.and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
}
fn numeric_id(value: &str) -> Result<&str, Error> {
    if value.is_empty()
        || value.len() > 20
        || !value.bytes().all(|c| c.is_ascii_digit())
        || value.starts_with('0')
    {
        Err(Error::request())
    } else {
        Ok(value)
    }
}
fn validate_connection(value: &str) -> Result<(), Error> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-')
    {
        Err(Error::request())
    } else {
        Ok(())
    }
}
fn query(base: &str, params: &[(&str, &str)]) -> Result<String, Error> {
    let mut url = Url::parse(base).map_err(|_| Error::request())?;
    url.query_pairs_mut().extend_pairs(params.iter().copied());
    Ok(url.into())
}
fn safe_url(value: &str) -> Option<String> {
    let normalized = if value.starts_with("//") {
        format!("https:{value}")
    } else if let Some(v) = value.strip_prefix("http://") {
        format!("https://{v}")
    } else {
        value.to_owned()
    };
    let url = Url::parse(&normalized).ok()?;
    if normalized.len() > 4096
        || normalized.chars().any(char::is_control)
        || url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some_and(|p| p != 443)
        || url.fragment().is_some()
    {
        return None;
    }
    let manifest: Value = serde_json::from_str(include_str!("../plugin.template.json")).ok()?;
    if !manifest
        .pointer("/permissions/0/domains")?
        .as_array()?
        .iter()
        .any(|domain| domain.as_str() == url.host_str())
    {
        return None;
    }
    Some(url.into())
}
fn asset(connection: &str, url: &str) -> Result<(String, String), Error> {
    let url = safe_url(url).ok_or_else(|| {
        Error::new("permission-denied", "芒果资源域名尚未获准").because("asset-domain-denied")
    })?;
    let data = host(
        7,
        &json!({"connectionId":connection,"url":url,"headers":{"Referer":"https://www.mgtv.com/","User-Agent":USER_AGENT},"ttlSeconds":300}),
    )?;
    Ok((
        string(&data, "ref")?.to_owned(),
        string(&data, "expiresAt")?.to_owned(),
    ))
}
fn storage_get(connection: &str, key: &str) -> Result<Option<Value>, Error> {
    let response = host(2, &json!({"connectionId":connection,"key":key}))?;
    if response.get("found").and_then(Value::as_bool) != Some(true) {
        return Ok(None);
    }
    let bytes = BASE64
        .decode(string(&response, "valueBase64")?)
        .map_err(|_| Error::response())?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| Error::response())
}
fn storage_set(connection: &str, key: &str, value: &Value) -> Result<(), Error> {
    let bytes = serde_json::to_vec(value).map_err(|_| Error::response())?;
    host(
        3,
        &json!({"connectionId":connection,"key":key,"valueBase64":BASE64.encode(bytes)}),
    )?;
    Ok(())
}

#[cfg(test)]
mod http_payload_tests {
    use super::*;

    #[test]
    fn safe_error_reason_is_optional_and_never_contains_transport_values() {
        let legacy = Error::new("invalid-response", "固定诊断").envelope();
        assert!(legacy["pluginError"].get("reason").is_none());
        let error = Error::new("permission-denied", "固定诊断")
            .because("asset-domain-denied")
            .envelope();
        assert_eq!(error["pluginError"]["reason"], "asset-domain-denied");
        assert_eq!(error["pluginError"].as_object().unwrap().len(), 3);
    }

    #[test]
    fn real_detail_and_episode_payloads_exceed_display_string_limit() {
        for fixture in [
            include_str!("../fixtures/info.json"),
            include_str!("../fixtures/episodes.json"),
        ] {
            let encoded = BASE64.encode(fixture.as_bytes());
            assert!(encoded.len() > 4096);
            let response = json!({"status":200,"bodyBase64":encoded});
            let payload = decode_http(&response).unwrap();
            assert_eq!(payload["code"], 200);
            assert!(provider_data(&payload).is_ok());
        }
    }

    #[test]
    fn http_payload_decode_rejects_invalid_or_oversized_transport() {
        assert!(decode_http(&json!({"status":200,"bodyBase64":"not-base64"})).is_err());
        assert!(
            decode_http(&json!({"status":200,"bodyBase64":"a".repeat(HOST_BUFFER_BYTES+1)}))
                .is_err()
        );
    }
}
