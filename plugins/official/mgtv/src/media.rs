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
            ("vf", "h264,h265,av01"),
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
        return Err(
            Error::new("permission-denied", "当前地区无权播放该芒果视频")
                .because("region-restricted"),
        );
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
            if data.pointer("/user/login").and_then(Value::as_bool) == Some(false) {
                "not-authenticated"
            } else {
                "permission-denied"
            },
            "当前账号仅可试看或尚未获得完整播放权益",
        )
        .because("entitlement-required"));
    }
    if number(data.pointer("/info/drmFlag")).is_some_and(|v| v != 0)
        || ["licenseUrl", "drmFirm", "drmType"].iter().any(|key| {
            data.pointer(&format!("/drm/{key}"))
                .and_then(Value::as_str)
                .is_some_and(|v| !v.is_empty())
        })
    {
        return Err(
            Error::new("permission-denied", "该视频采用当前不支持的 DRM 保护")
                .because("drm-unsupported"),
        );
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
        return Err(
            Error::new("permission-denied", "芒果当前未为该视频开放下载入口")
                .because("download-unavailable"),
        );
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
            .reduce(|best, candidate| {
                // Keep the first available codec at equal resolution.
                if number(candidate.1.get("videoHeight")).unwrap_or(0)
                    > number(best.1.get("videoHeight")).unwrap_or(0)
                {
                    candidate
                } else {
                    best
                }
            }),
    }
    .ok_or_else(|| {
        Error::new("permission-denied", "所选芒果清晰度不可用").because("quality-unavailable")
    })?;
    if number(selected.1.get("needPay")) != Some(0)
        || !supported_stream(selected.1)
        || selected
            .1
            .get("url")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
    {
        return Err(
            Error::new("permission-denied", "当前账号无所选清晰度的完整权益")
                .because("quality-unavailable"),
        );
    }
    let full_duration = number(data.pointer("/info/duration")).ok_or_else(Error::response)?;
    let stream_duration = number(selected.1.get("ftime")).ok_or_else(Error::response)?;
    if full_duration.abs_diff(stream_duration) > 2 {
        return Err(Error::new("permission-denied", "芒果返回了不完整的视频流")
            .because("incomplete-stream"));
    }
    Ok((selected.0.clone(), selected.1, variants))
}
fn dispatch_url(connection: &str, data: &Value, row: &Value) -> Result<String, Error> {
    dispatch_url_with(data, row, |url| decode_http(&http(connection, url, false)?))
}
fn dispatch_url_with(
    data: &Value,
    row: &Value,
    mut fetch: impl FnMut(&str) -> Result<Value, Error>,
) -> Result<String, Error> {
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
        let response = match fetch(&url) {
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
        let candidate = string(&response, "info").and_then(|url| {
            safe_url(url).ok_or_else(|| {
                Error::new("permission-denied", "芒果当前 CDN 域名尚未获准")
                    .because("asset-domain-denied")
            })
        });
        match candidate {
            Ok(url) => return Ok(url),
            Err(error) => {
                // Keep the exact selected stream, but try its other official
                // dispatchers when this replica picks an unreviewed CDN.
                failure = error;
            }
        }
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
    let data = diagnosed("stream-list", streams(&request.connection_id, cid, vid))?;
    log_stream_observation(&data, cid, vid);
    if download {
        if request.variant_id.as_deref().is_none_or(str::is_empty) {
            return Err(Error::request());
        }
        let info = diagnosed(
            "download-info",
            catalog::info(&request.connection_id, cid, Some(vid)),
        )?;
        diagnosed(
            "download-entitlement",
            download_entitlement(&data, &info, cid, vid),
        )?;
    } else {
        diagnosed(
            "playback-entitlement",
            complete_entitlement(&data, cid, vid),
        )?;
    }
    let (selected, row, variants) =
        diagnosed("quality", select(&data, request.variant_id.as_deref()))?;
    let url = diagnosed("dispatch", dispatch_url(&request.connection_id, &data, row))?;
    let format = if row.get("fileFormat").and_then(Value::as_str) == Some("MP4") {
        "progressive"
    } else {
        "hls"
    };
    let (asset, expires) = diagnosed("asset-register", asset(&request.connection_id, &url))?;
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
fn diagnosed<T>(stage: &'static str, result: Result<T, Error>) -> Result<T, Error> {
    result.inspect_err(|error| {
        let _ = host(
            4,
            &json!({"level":"warn","operation":"media.playback",
            "message":"Mango media resolution rejected",
            "fields":{"stage":stage,"code":error.code,"reason":error.reason.unwrap_or("unknown")}}),
        );
    })
}
fn log_stream_observation(data: &Value, cid: &str, vid: &str) {
    // These allowlisted scalars describe only the selected entitlement. No
    // account ID, nickname, cookie, URL, ticket or complete body enters logs.
    let fields = stream_observation(data, cid, vid);
    let _ = host(
        4,
        &json!({"level":"debug","operation":"media.playback",
        "message":"Mango selected media entitlement observed","fields":fields}),
    );
}
fn stream_observation(data: &Value, cid: &str, vid: &str) -> Value {
    let mut fields = json!({"stage":"stream-entitlement","identity_matched":
        data.pointer("/info/collection_id").and_then(Value::as_str)==Some(cid) &&
        data.pointer("/info/video_id").and_then(Value::as_str)==Some(vid)});
    if let Some(login) = data.pointer("/user/login").and_then(Value::as_bool) {
        fields["login"] = json!(login);
    }
    if let Some(purview) = number(data.pointer("/user/purview")) {
        fields["purview"] = json!(purview);
    }
    fields
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
    fn synthetic_high_quality_data() -> Value {
        let mut data = fixture("stream-free.json")["data"].clone();
        // Only the envelope is an observed fixture. These stream rows, numeric
        // definitions and paths are synthetic; they are not a VIP observation
        // or an official definition-to-resolution mapping.
        data["user"]["login"] = json!(true);
        data["user"]["isvip"] = json!("1");
        data["stream"] = json!([{
            "definition":"3", "fileId":"1", "barName":"720P",
            "videoWidth":"1280", "videoHeight":"720", "ftime":"7560",
            "needPay":0, "url":"/SYNTHETIC-720", "fileFormat":"HLS-TS",
            "videoFormat":"H264"
        }]);
        data["stream_h265"] = json!([
            {
                "definition":"4", "fileId":"2", "barName":"1080P SDR",
                "videoWidth":"1920", "videoHeight":"1080", "ftime":"7560",
                "needPay":0, "url":"/SYNTHETIC-1080", "fileFormat":"HLS-FMP4",
                "videoFormat":"H265"
            },
            {
                "definition":"9", "fileId":"3", "barName":"4K",
                "videoWidth":"3840", "videoHeight":"2160", "ftime":"7560",
                "needPay":0, "url":"/SYNTHETIC-2160", "fileFormat":"HLS-FMP4",
                "videoFormat":"H265"
            }
        ]);
        data["stream_av01"] = json!([{
            "definition":"9", "fileId":"4", "barName":"4K",
            "videoWidth":"3840", "videoHeight":"2160", "ftime":"7560",
            "needPay":0, "url":"/SYNTHETIC-AV1", "fileFormat":"MP4",
            "videoFormat":"AV1"
        }]);
        data
    }
    #[test]
    fn stream_request_declares_supported_codec_groups() {
        let connection = "00000000-0000-4000-8000-000000000001";
        let url = Url::parse(&stream_url(connection, "687100", "22025165").unwrap()).unwrap();
        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str(), Some("pcweb.api.mgtv.com"));
        assert_eq!(url.path(), "/video/streamList");
        let query: std::collections::BTreeMap<_, _> = url.query_pairs().collect();
        assert_eq!(
            query.get("vf").map(|value| value.as_ref()),
            Some("h264,h265,av01")
        );
        assert_eq!(query.get("cid").map(|value| value.as_ref()), Some("687100"));
        assert_eq!(
            query.get("video_id").map(|value| value.as_ref()),
            Some("22025165")
        );
        assert_eq!(
            query.get("did").map(|value| value.as_ref()),
            Some(connection)
        );
        assert_eq!(
            query.get("definitionType").map(|value| value.as_ref()),
            Some("2")
        );
    }
    #[test]
    fn synthetic_high_quality_rows_remain_selectable_and_keep_exact_variants() {
        let full = synthetic_high_quality_data();
        let mut hd = full.clone();
        hd["stream_h265"] = json!([hd["stream_h265"][0].clone()]);
        hd["stream_av01"] = json!([]);
        for (data, expected_id, height) in [(hd, "h265:4:2", 1080), (full, "h265:9:3", 2160)] {
            complete_entitlement(&data, "687100", "22025165").unwrap();
            let (id, row, variants) = select(&data, None).unwrap();
            assert_eq!(id, expected_id);
            assert_eq!(number(row.get("videoHeight")), Some(height));
            let selected = variants.iter().find(|variant| variant["id"] == id).unwrap();
            assert_eq!(selected["available"], true);
            assert_eq!(selected["height"], height);
            assert_eq!(selected["videoCodec"], "H265");
            for (candidate, _) in stream_rows(&data).unwrap() {
                assert_eq!(select(&data, Some(&candidate)).unwrap().0, candidate);
            }
            assert!(!serde_json::to_string(&variants)
                .unwrap()
                .contains("SYNTHETIC"));
        }
    }
    #[test]
    fn default_quality_keeps_first_available_codec_on_equal_height() {
        let mut data = synthetic_high_quality_data();
        let mut h264 = data["stream_h265"][1].clone();
        h264["fileId"] = json!("5");
        h264["videoFormat"] = json!("H264");
        h264["fileFormat"] = json!("HLS-TS");
        data["stream"].as_array_mut().unwrap().push(h264);
        assert_eq!(select(&data, None).unwrap().0, "h264:9:5");
        for requested in ["h265:9:3", "av01:9:4"] {
            assert_eq!(select(&data, Some(requested)).unwrap().0, requested);
        }
        data["stream"][1]["needPay"] = json!(1);
        assert_eq!(select(&data, None).unwrap().0, "h265:9:3");
        data["stream_h265"][1]["url"] = json!("");
        assert_eq!(select(&data, None).unwrap().0, "av01:9:4");
    }
    #[test]
    fn high_quality_selection_requires_each_row_to_be_available() {
        for (key, value) in [
            ("url", Value::Null),
            ("url", json!("")),
            ("needPay", json!(1)),
            ("needPay", Value::Null),
            ("fileFormat", json!("DASH")),
            ("fileFormat", Value::Null),
        ] {
            let mut data = synthetic_high_quality_data();
            data["stream_h265"] = json!([data["stream_h265"][1].clone()]);
            data["stream_av01"] = json!([]);
            data["stream_h265"][0][key] = value;
            let (id, _, variants) = select(&data, None).unwrap();
            assert_eq!(id, "h264:3:1", "unavailable high quality: {key}");
            assert_eq!(variants[1]["available"], false);
            assert_eq!(
                select(&data, Some("h265:9:3")).unwrap_err().reason,
                Some("quality-unavailable")
            );
            data["stream"][0]["needPay"] = json!(1);
            assert_eq!(
                select(&data, None).unwrap_err().reason,
                Some("quality-unavailable")
            );
        }
    }
    #[test]
    fn synthetic_high_qualities_cannot_replace_full_entitlement_or_duration() {
        for (path, value, reason) in [
            ("/user/purview", json!(101), "entitlement-required"),
            ("/user/iplimit", json!(1), "region-restricted"),
            ("/info/drmFlag", json!(1), "drm-unsupported"),
            (
                "/drm/licenseUrl",
                json!("SYNTHETIC-license"),
                "drm-unsupported",
            ),
        ] {
            let mut data = synthetic_high_quality_data();
            *data.pointer_mut(path).unwrap() = value;
            assert_eq!(
                complete_entitlement(&data, "687100", "22025165")
                    .unwrap_err()
                    .reason,
                Some(reason)
            );
        }
        let mut preview = synthetic_high_quality_data();
        preview["info"]["authInfo"] = json!({"pay_info":{"preview_playing":{"components":[{}]}}});
        assert_eq!(
            complete_entitlement(&preview, "687100", "22025165")
                .unwrap_err()
                .reason,
            Some("entitlement-required")
        );
        let mut incomplete = synthetic_high_quality_data();
        incomplete["stream_h265"][1]["ftime"] = json!(300);
        assert_eq!(
            select(&incomplete, None).unwrap_err().reason,
            Some("incomplete-stream")
        );
        assert_eq!(
            select(&incomplete, Some("h265:9:3")).unwrap_err().reason,
            Some("incomplete-stream")
        );
        // A failed higher row is not silently replaced by another codec/quality.
        assert_eq!(select(&incomplete, Some("av01:9:4")).unwrap().0, "av01:9:4");
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
        let denied = complete_entitlement(&data, "778406", "24623021").unwrap_err();
        assert_eq!(denied.code, "not-authenticated");
        assert_eq!(denied.reason, Some("entitlement-required"));
        let mut disguised = data.clone();
        disguised["user"]["purview"] = json!("200");
        assert!(complete_entitlement(&disguised, "778406", "24623021").is_err());
        let mut logged_in = data;
        logged_in["user"]["login"] = json!(true);
        assert_eq!(
            complete_entitlement(&logged_in, "778406", "24623021")
                .unwrap_err()
                .code,
            "permission-denied"
        );
    }
    #[test]
    fn download_requires_exact_full_stream_and_official_enabled_entry() {
        let data = fixture("stream-free.json")["data"].clone();
        let info = fixture("info-free.json")["data"].clone();
        download_entitlement(&data, &info, "687100", "22025165").unwrap();
        let mut hidden = info.clone();
        hidden["config"]["download"]["downloadDisplay"] = json!(0);
        assert!(download_entitlement(&data, &hidden, "687100", "22025165").is_err());
        hidden["config"]["download"]
            .as_object_mut()
            .unwrap()
            .remove("downloadDisplay");
        assert_eq!(
            download_entitlement(&data, &hidden, "687100", "22025165")
                .unwrap_err()
                .reason,
            Some("download-unavailable")
        );
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
    #[test]
    fn official_free_stream_trial_configuration_is_not_active_preview() {
        let data = fixture("stream-free.json")["data"].clone();
        assert_eq!(number(data.pointer("/info/trialtime")), Some(300));
        // Current official full entitlement still includes a trial-time config.
        complete_entitlement(&data, "687100", "22025165").unwrap();
    }
    #[test]
    fn official_dispatch_host_projections_are_exactly_reviewed() {
        for observation in fixture("dispatch-observations.json").as_array().unwrap() {
            assert_eq!(observation["status"], "ok");
            assert_eq!(observation["scheme"], "https");
            assert_eq!(observation["port"], 443);
            assert_eq!(observation["format"], "m3u8");
            // URL is explicitly synthetic: only the hostname is real evidence.
            let url = format!(
                "https://{}/SYNTHETIC.m3u8",
                observation["hostname"].as_str().unwrap()
            );
            assert!(safe_url(&url).is_some());
        }
        assert!(safe_url("https://unreviewed.titan.mgtv.com/SYNTHETIC.m3u8").is_none());
        assert!(safe_url("https://pcvideotx.titan.mgtv.com.evil.invalid/SYNTHETIC.m3u8").is_none());
    }
    #[test]
    fn dispatch_retries_same_selected_stream_after_unknown_cdn() {
        let data = json!({"stream_domain":["https://web-disp.titan.mgtv.com", "https://web-disp1.titan.mgtv.com"]});
        let row = json!({"url":"/SYNTHETIC-dispatch"});
        let mut calls = Vec::new();
        let resolved = dispatch_url_with(&data, &row, |url| {
            calls.push(url.to_owned());
            Ok(json!({"status":"ok","info":if calls.len()==1 {
                "https://unreviewed.titan.mgtv.com/SYNTHETIC.m3u8"
            } else {"https://pcvideotx.titan.mgtv.com/SYNTHETIC.m3u8"}}))
        })
        .unwrap();
        assert_eq!(
            calls,
            vec![
                "https://web-disp.titan.mgtv.com/SYNTHETIC-dispatch",
                "https://web-disp1.titan.mgtv.com/SYNTHETIC-dispatch"
            ]
        );
        assert_eq!(resolved, "https://pcvideotx.titan.mgtv.com/SYNTHETIC.m3u8");
        let error = dispatch_url_with(&data, &row, |_| {
            Ok(json!({"status":"ok","info":"https://unreviewed.titan.mgtv.com/SYNTHETIC.m3u8"}))
        })
        .unwrap_err();
        assert_eq!(error.reason, Some("asset-domain-denied"));
    }
    #[test]
    fn entitlement_diagnostics_preserve_only_safe_scalars() {
        let mut data = fixture("stream-free.json")["data"].clone();
        data["user"]["ticket"] = json!("DO-NOT-LOG-TICKET");
        data["user"]["uuid"] = json!("DO-NOT-LOG-ACCOUNT");
        data["stream"][0]["url"] = json!("https://cdn.invalid/?secret=DO-NOT-LOG-URL");
        let fields = stream_observation(&data, "687100", "22025165");
        assert_eq!(
            fields,
            json!({"stage":"stream-entitlement","identity_matched":true,"login":false,"purview":200})
        );
        assert!(!fields.to_string().contains("DO-NOT-LOG"));
    }
}
