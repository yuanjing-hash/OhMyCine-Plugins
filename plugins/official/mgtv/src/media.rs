use super::*;
use std::collections::HashSet;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Request {
    connection_id: String,
    item_id: String,
    segment_id: String,
    version_id: String,
    variant_id: Option<String>,
}
pub(super) fn identity<'a>(
    work: &'a str,
    segment: &'a str,
    version: &str,
) -> Result<(&'a str, &'a str), Error> {
    let cid = catalog::clip_id(work)?;
    let vid = numeric_id(segment.strip_prefix("video:").ok_or_else(Error::request)?)?;
    if version != format!("mgtv:{cid}:{vid}") {
        return Err(Error::request());
    }
    Ok((cid, vid))
}
fn stream_url(connection: &str, cid: &str, vid: &str) -> Result<String, Error> {
    query(
        "https://pcweb.api.mgtv.com/video/streamList",
        &[
            ("playType", "1"),
            ("auth_mode", "1"),
            ("definitionType", "2"),
            ("video_id", vid),
            ("cid", cid),
            ("type", "pch5"),
            ("did", connection),
            ("_support", "10000000"),
            ("allowedRC", "1"),
        ],
    )
}
fn streams(connection: &str, cid: &str, vid: &str) -> Result<Value, Error> {
    let url = stream_url(connection, cid, vid)?;
    let response = match http(connection, &url, true) {
        Ok(data) => data,
        Err(error) if error.code == "permission-denied" => http(connection, &url, false)?,
        Err(error) => return Err(error),
    };
    let payload = decode_http(&response)?;
    provider_data(&payload).cloned()
}
fn complete_entitlement(data: &Value, cid: &str, vid: &str) -> Result<(), Error> {
    if data.pointer("/info/collection_id").and_then(Value::as_str) != Some(cid)
        || data.pointer("/info/video_id").and_then(Value::as_str) != Some(vid)
    {
        return Err(Error::response());
    }
    let ip_limit = number(data.pointer("/user/iplimit"));
    if ip_limit.is_some_and(|v| v != 0) {
        return Err(Error::new(
            "permission-denied",
            "当前地区无权播放该芒果视频",
        ));
    }
    // The official desktop player defines preview from these components. A
    // non-empty stream URL or an account VIP flag alone can still be a trial.
    if number(data.pointer("/user/purview")) != Some(200)
        || ["preview_starting", "preview_playing", "preview_end"]
            .iter()
            .any(|field| {
                data.pointer(&format!("/info/authInfo/pay_info/{field}/components"))
                    .and_then(Value::as_array)
                    .is_some_and(|v| !v.is_empty())
            })
    {
        return Err(Error::new(
            "permission-denied",
            "当前账号仅可试看或尚未获得完整播放权益",
        ));
    }
    if number(data.pointer("/info/drmFlag")).is_some_and(|v| v != 0)
        || ["licenseUrl", "drmFirm", "drmType"].iter().any(|key| {
            data.pointer(&format!("/drm/{key}"))
                .and_then(Value::as_str)
                .is_some_and(|v| !v.is_empty())
        })
    {
        return Err(Error::new(
            "permission-denied",
            "该视频采用当前不支持的 DRM 保护",
        ));
    }
    if number(data.pointer("/info/duration"))
        .filter(|v| *v > 0)
        .is_none()
    {
        return Err(Error::response());
    }
    Ok(())
}
fn download_entitlement(data: &Value, info: &Value, cid: &str, vid: &str) -> Result<(), Error> {
    complete_entitlement(data, cid, vid)?;
    if info.pointer("/info/clipId").and_then(Value::as_str) != Some(cid)
        || info.pointer("/info/videoId").and_then(Value::as_str) != Some(vid)
    {
        return Err(Error::response());
    }
    if number(info.pointer("/config/download/downloadDisplay")) != Some(1) {
        return Err(Error::new(
            "permission-denied",
            "芒果当前未为该视频开放下载入口",
        ));
    }
    Ok(())
}
fn stream_rows(data: &Value) -> Result<Vec<(String, &Value)>, Error> {
    let mut rows = Vec::new();
    let mut seen = HashSet::new();
    for (codec, key) in [
        ("h264", "stream"),
        ("h265", "stream_h265"),
        ("av01", "stream_av01"),
    ] {
        if let Some(list) = data.get(key).and_then(Value::as_array) {
            for row in list {
                let definition = number(row.get("definition")).ok_or_else(Error::response)?;
                let file = number(row.get("fileId"))
                    .filter(|v| *v > 0)
                    .ok_or_else(Error::response)?;
                let id = format!("{codec}:{definition}:{file}");
                if !seen.insert(id.clone()) {
                    return Err(Error::response());
                }
                rows.push((id, row));
            }
        }
    }
    if rows.is_empty() || rows.len() > 32 {
        return Err(Error::response());
    }
    Ok(rows)
}
fn supported_stream(row: &Value) -> bool {
    matches!(
        row.get("fileFormat").and_then(Value::as_str),
        Some("HLS-TS" | "HLS-FMP4" | "MP4")
    )
}
fn variant(id: &str, row: &Value, entitled: bool) -> Result<Value, Error> {
    let available = entitled
        && number(row.get("needPay")) == Some(0)
        && row
            .get("url")
            .and_then(Value::as_str)
            .is_some_and(|v| !v.is_empty())
        && supported_stream(row);
    let label = string(row, "barName")?;
    let mut result = json!({"id":id,"label":label,"available":available,
        "width":number(row.get("videoWidth")),"height":number(row.get("videoHeight")),
        "bitrate":number(row.get("filebitrate")).and_then(|n|n.checked_mul(1000)),
        "videoCodec":row.get("videoFormat"),"audioCodec":row.get("audioFormat"),
        "frameRate":row.get("framerate").and_then(Value::as_str).and_then(|v|v.parse::<f64>().ok()),
        "container":if row.get("fileFormat").and_then(Value::as_str)==Some("MP4"){"mp4"}else{"hls"}});
    if !available {
        result["unavailableReason"] = json!(if !entitled {
            "无完整播放权益"
        } else if !supported_stream(row) {
            "当前不支持此视频格式"
        } else {
            "当前账号无此清晰度权益"
        });
    }
    Ok(result)
}
fn select<'a>(
    data: &'a Value,
    requested: Option<&str>,
) -> Result<(String, &'a Value, Vec<Value>), Error> {
    let rows = stream_rows(data)?;
    let variants = rows
        .iter()
        .map(|(id, row)| variant(id, row, true))
        .collect::<Result<Vec<_>, _>>()?;
    let selected = match requested {
        Some(id) => rows.iter().find(|(candidate, _)| candidate == id),
        None => rows
            .iter()
            .filter(|(_, row)| {
                number(row.get("needPay")) == Some(0)
                    && supported_stream(row)
                    && row
                        .get("url")
                        .and_then(Value::as_str)
                        .is_some_and(|v| !v.is_empty())
            })
            .max_by_key(|(_, row)| number(row.get("videoHeight")).unwrap_or(0)),
    }
    .ok_or_else(|| Error::new("permission-denied", "所选芒果清晰度不可用"))?;
    if number(selected.1.get("needPay")) != Some(0)
        || !supported_stream(selected.1)
        || selected
            .1
            .get("url")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
    {
        return Err(Error::new(
            "permission-denied",
            "当前账号无所选清晰度的完整权益",
        ));
    }
    let full_duration = number(data.pointer("/info/duration")).ok_or_else(Error::response)?;
    let stream_duration = number(selected.1.get("ftime")).ok_or_else(Error::response)?;
    if full_duration.abs_diff(stream_duration) > 2 {
        return Err(Error::new("permission-denied", "芒果返回了不完整的视频流"));
    }
    Ok((selected.0.clone(), selected.1, variants))
}
fn dispatch_url(connection: &str, data: &Value, row: &Value) -> Result<String, Error> {
    let path = string(row, "url")?;
    let domains = data
        .get("stream_domain")
        .and_then(Value::as_array)
        .ok_or_else(Error::response)?;
    if domains.is_empty() || domains.len() > 8 {
        return Err(Error::response());
    }
    let mut failure = Error::new("upstream-unavailable", "芒果播放地址解析失败");
    for domain in domains {
        let Some(domain) = domain.as_str().and_then(safe_url) else {
            continue;
        };
        let url = Url::parse(&domain)
            .and_then(|base| base.join(path))
            .ok()
            .and_then(|v| safe_url(v.as_str()))
            .ok_or_else(Error::response)?;
        let response = match http(connection, &url, false).and_then(|r| decode_http(&r)) {
            Ok(value) => value,
            Err(error) => {
                failure = error;
                continue;
            }
        };
        if response
            .get("status")
            .and_then(Value::as_str)
            .is_none_or(|v| !v.eq_ignore_ascii_case("ok"))
        {
            continue;
        }
        return safe_url(string(&response, "info")?)
            .ok_or_else(|| Error::new("permission-denied", "芒果当前 CDN 域名尚未获准"));
    }
    Err(failure)
}
struct Resolved {
    selected: String,
    variants: Vec<Value>,
    asset: String,
    expires: String,
    format: &'static str,
    subtitles: Vec<Value>,
}

fn subtitle_tracks(connection: &str, vid: &str, data: &Value) -> Result<Vec<Value>, Error> {
    let response = get(
        connection,
        &query(
            "https://mobile-stream.api.mgtv.com/v2/video/title",
            &[
                ("videoId", vid),
                ("_support", "10000000"),
                ("src", "mgtv"),
                ("platform", "5"),
            ],
        )?,
        false,
    )?;
    if response
        .get("partId")
        .and_then(Value::as_str)
        .is_some_and(|id| id != vid)
    {
        return Err(Error::response());
    }
    let rows = response
        .get("title")
        .and_then(Value::as_array)
        .ok_or_else(Error::response)?;
    if rows.len() > 24 {
        return Err(Error::response());
    }
    let mut tracks = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let raw_url = string(row, "url")?;
        let url = if raw_url.starts_with("http") {
            safe_url(raw_url).ok_or_else(Error::response)?
        } else {
            let domain = data
                .pointer("/stream_domain/0")
                .and_then(Value::as_str)
                .and_then(safe_url)
                .ok_or_else(Error::response)?;
            let url = Url::parse(&domain)
                .and_then(|base| base.join(raw_url))
                .map_err(|_| Error::response())?;
            let resolved = decode_http(&http(connection, url.as_str(), false)?)?;
            safe_url(string(&resolved, "info")?).ok_or_else(Error::response)?
        };
        let (reference, _) = asset(connection, &url)?;
        let label = row
            .get("name")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty() && v.len() <= 256)
            .unwrap_or("官方字幕");
        let mut track = json!({"id":format!("subtitle-{index}"),"label":label,"format":"srt","urlRef":reference});
        if let Some(language) = row
            .get("captionSimpleName")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty() && v.len() <= 32)
        {
            track["language"] = json!(language);
        }
        tracks.push(track);
    }
    Ok(tracks)
}
fn resolve(request: &Request, download: bool) -> Result<Resolved, Error> {
    validate_connection(&request.connection_id)?;
    let (cid, vid) = identity(&request.item_id, &request.segment_id, &request.version_id)?;
    let data = streams(&request.connection_id, cid, vid)?;
    if download {
        if request.variant_id.as_deref().is_none_or(str::is_empty) {
            return Err(Error::request());
        }
        let info = catalog::info(&request.connection_id, cid, Some(vid))?;
        download_entitlement(&data, &info, cid, vid)?;
    } else {
        complete_entitlement(&data, cid, vid)?;
    }
    let (selected, row, variants) = select(&data, request.variant_id.as_deref())?;
    let url = dispatch_url(&request.connection_id, &data, row)?;
    let format = if row.get("fileFormat").and_then(Value::as_str) == Some("MP4") {
        "progressive"
    } else {
        "hls"
    };
    let (asset, expires) = asset(&request.connection_id, &url)?;
    // Subtitle availability is optional; a provider subtitle outage must not
    // turn an otherwise complete entitled video into an unavailable video.
    let subtitles = subtitle_tracks(&request.connection_id, vid, &data).unwrap_or_default();
    Ok(Resolved {
        selected,
        variants,
        asset,
        expires,
        format,
        subtitles,
    })
}
pub(super) fn playback(request: Request) -> Result<Value, Error> {
    let resolved = resolve(&request, false)?;
    Ok(
        json!({"workId":request.item_id,"segmentId":request.segment_id,"versionId":request.version_id,
        "variantId":resolved.selected,"variants":resolved.variants,"assets":[{"kind":if resolved.format=="hls"{"hls"}else{"progressive"},"urlRef":resolved.asset}],"delivery":"server-gateway","expiresAt":resolved.expires,"subtitles":resolved.subtitles}),
    )
}
pub(super) fn offline(request: Request) -> Result<Value, Error> {
    let resolved = resolve(&request, true)?;
    let expires = OffsetDateTime::parse(&resolved.expires, &Rfc3339)
        .map_err(|_| Error::response())?
        .unix_timestamp();
    let (_, vid) = identity(&request.item_id, &request.segment_id, &request.version_id)?;
    let mut assets = vec![
        json!({"id":"video","kind":"video","urlRef":resolved.asset,"expectedContentType":if resolved.format=="hls"{"application/vnd.apple.mpegurl"}else{"video/mp4"}}),
    ];
    for track in resolved.subtitles {
        assets.push(json!({"id":track["id"],"kind":"subtitle","urlRef":track["urlRef"],"expectedContentType":"application/x-subrip"}));
    }
    Ok(
        json!({"version":1,"workId":request.item_id,"segmentId":request.segment_id,"versionId":request.version_id,
        "variantId":resolved.selected,"suggestedFileName":format!("MangoTV-{vid}.mp4"),"format":resolved.format,
        "expiresAt":expires,"assets":assets}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(name: &str) -> Value {
        serde_json::from_str(
            &std::fs::read_to_string(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("fixtures")
                    .join(name),
            )
            .unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn real_free_response_grants_only_actual_available_quality() {
        let data = fixture("stream-free.json")["data"].clone();
        complete_entitlement(&data, "687100", "22025165").unwrap();
        let (id, row, variants) = select(&data, None).unwrap();
        assert_eq!(number(row.get("definition")), Some(3));
        assert!(id.starts_with("h264:3:"));
        assert!(variants
            .iter()
            .any(|v| v["label"] == "1080P SDR" && v["available"] == false));
        let denied = stream_rows(&data)
            .unwrap()
            .iter()
            .find(|(_, row)| number(row.get("definition")) == Some(4))
            .unwrap()
            .0
            .clone();
        assert!(select(&data, Some(&denied)).is_err());
    }
    #[test]
    fn real_trial_urls_do_not_prove_full_entitlement() {
        let data = fixture("stream-preview.json")["data"].clone();
        assert!(data["stream"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| number(s.get("needPay")) == Some(0)));
        assert!(complete_entitlement(&data, "778406", "24623021").is_err());
        let mut disguised = data.clone();
        disguised["user"]["purview"] = json!("200");
        assert!(complete_entitlement(&disguised, "778406", "24623021").is_err());
    }
    #[test]
    fn download_requires_exact_full_stream_and_official_enabled_entry() {
        let data = fixture("stream-free.json")["data"].clone();
        let info = fixture("info-free.json")["data"].clone();
        download_entitlement(&data, &info, "687100", "22025165").unwrap();
        let mut hidden = info.clone();
        hidden["config"]["download"]["downloadDisplay"] = json!(0);
        assert!(download_entitlement(&data, &hidden, "687100", "22025165").is_err());
        let preview = fixture("stream-preview.json")["data"].clone();
        assert!(download_entitlement(&preview, &info, "687100", "22025165").is_err());
        let mut drm = data.clone();
        drm["drm"]["licenseUrl"] = json!("https://license.invalid");
        assert!(complete_entitlement(&drm, "687100", "22025165").is_err());
    }
    #[test]
    fn identity_format_and_duration_never_silently_fallback() {
        assert!(identity("clip:687100", "video:22025165", "mgtv:1:22025165").is_err());
        let mut data = fixture("stream-free.json")["data"].clone();
        assert!(select(&data, Some("h264:4:does-not-exist")).is_err());
        data["info"]["duration"] = json!("100000");
        assert!(select(&data, None).is_err());
    }
}
