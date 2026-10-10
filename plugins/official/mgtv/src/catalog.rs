use super::*;
use std::collections::HashSet;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Navigation {
    connection_id: String,
    parent_node_key: Option<String>,
    depth: Option<u8>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Feed {
    connection_id: String,
    route_key: String,
    cursor: Option<String>,
    refresh_session: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Search {
    connection_id: String,
    query: String,
    cursor: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Detail {
    pub(super) connection_id: String,
    pub(super) item_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Artwork {
    connection_id: String,
    scope_key: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Metadata {
    connection_id: String,
    item_id: String,
    segment_id: String,
    version_id: String,
}

fn array<'a>(data: &'a Value, key: &str) -> Result<&'a Vec<Value>, Error> {
    data.get(key)
        .and_then(Value::as_array)
        .ok_or_else(Error::response)
}
fn channels(connection: &str) -> Result<Vec<Value>, Error> {
    let data = get(
        connection,
        "https://pianku.api.mgtv.com/rider/config/platformChannels/v1?platform=pcweb",
        false,
    )?;
    let mut rows = data.as_array().cloned().ok_or_else(Error::response)?;
    rows.retain(|row| number(row.get("status")) == Some(1));
    if rows.is_empty() || rows.len() > 90 {
        return Err(Error::response());
    }
    rows.sort_by_key(|row| number(row.get("rank")).unwrap_or(u64::MAX));
    for row in &rows {
        numeric_id(string(row, "channelId")?)?;
        string(row, "channelName")?;
    }
    Ok(rows)
}
fn config(connection: &str, channel: &str) -> Result<Value, Error> {
    numeric_id(channel)?;
    let data = get(
        connection,
        &query(
            "https://pianku.api.mgtv.com/rider/config/channel/v1",
            &[("platform", "pcweb"), ("channelId", channel)],
        )?,
        false,
    )?;
    if string(&data, "channelId")? != channel || array(&data, "listItems")?.len() > 20 {
        return Err(Error::response());
    }
    Ok(data)
}
pub(super) fn navigation(request: Navigation) -> Result<Value, Error> {
    validate_connection(&request.connection_id)?;
    if request.depth.unwrap_or(0) > 3 {
        return Err(Error::request());
    }
    let nodes = match request.parent_node_key.as_deref() {
        None => {
            let mut nodes = Vec::new();
            for row in channels(&request.connection_id)? {
                let id = string(&row, "channelId")?;
                nodes.push(json!({"id":format!("channel-{id}"),"title":string(&row,"channelName")?,"kind":"branch","nodeKey":format!("category:{id}"),"hasChildren":true}));
            }
            nodes.push(json!({"id":"search","title":"搜索","kind":"search","routeKey":"search"}));
            nodes
        }
        Some(key) => {
            let parts = key.split(':').collect::<Vec<_>>();
            if parts.first() != Some(&"category")
                || !(parts.len() == 2 || parts.len() == 4 && parts[2] == "filter")
            {
                return Err(Error::request());
            }
            let channel = parts[1];
            let data = config(&request.connection_id, channel)?;
            let fields = array(&data, "listItems")?;
            if parts.len() == 2 {
                let mut nodes = vec![
                    json!({"id":format!("all-{channel}"),"title":"全部","kind":"feed","routeKey":format!("catalog:{channel}"),"refreshable":true}),
                ];
                for field in fields.iter().filter(|f| number(f.get("status")) == Some(1)) {
                    let name = string(field, "eName")?;
                    if !name.bytes().all(|c| c.is_ascii_alphanumeric()) {
                        return Err(Error::response());
                    }
                    nodes.push(json!({"id":format!("filter-{name}"),"title":string(field,"typeName")?,"kind":"branch","nodeKey":format!("category:{channel}:filter:{name}"),"hasChildren":true}));
                }
                nodes
            } else {
                let field = fields
                    .iter()
                    .find(|field| field.get("eName").and_then(Value::as_str) == Some(parts[3]))
                    .ok_or_else(Error::request)?;
                let tags = array(field, "items")?;
                if tags.len() > 100 {
                    return Err(Error::response());
                }
                tags.iter().filter(|tag|number(tag.get("status"))==Some(1)).map(|tag| {
                    let id=string(tag,"tagId")?;
                    if !id.bytes().all(|c|c.is_ascii_alphanumeric() || c==b'-') { return Err(Error::response()); }
                    Ok(json!({"id":format!("tag-{id}"),"title":string(tag,"tagName")?,"kind":"feed","routeKey":format!("catalog:{channel}:{}:{id}",parts[3]),"refreshable":true}))
                }).collect::<Result<Vec<_>,Error>>()?
            }
        }
    };
    Ok(json!({"version":2,"mode":"hierarchical","nodes":nodes}))
}
fn page(cursor: Option<&str>) -> Result<u32, Error> {
    match cursor {
        None => Ok(1),
        Some(value) => value
            .strip_prefix("page:")
            .and_then(|v| v.parse().ok())
            .filter(|p| *p > 0 && *p < 10000)
            .ok_or_else(Error::request),
    }
}
pub(super) fn feed(request: Feed) -> Result<Value, Error> {
    validate_connection(&request.connection_id)?;
    if request
        .refresh_session
        .as_ref()
        .is_some_and(|v| v.len() > 512)
    {
        return Err(Error::request());
    }
    let parts = request.route_key.split(':').collect::<Vec<_>>();
    if parts.first() != Some(&"catalog") || !(parts.len() == 2 || parts.len() == 4) {
        return Err(Error::request());
    }
    let data = config(&request.connection_id, parts[1])?;
    let pn = page(request.cursor.as_deref())?;
    let page_string = pn.to_string();
    let mut params = vec![
        ("platform", "pcweb"),
        ("channelId", parts[1]),
        ("pn", page_string.as_str()),
        ("pc", "40"),
        ("hudong", "1"),
        ("_support", "10000000"),
    ];
    if parts.len() == 4 {
        let field = array(&data, "listItems")?
            .iter()
            .find(|f| f.get("eName").and_then(Value::as_str) == Some(parts[2]))
            .ok_or_else(Error::request)?;
        if !array(field, "items")?.iter().any(|t| {
            t.get("tagId").and_then(Value::as_str) == Some(parts[3])
                && number(t.get("status")) == Some(1)
        }) {
            return Err(Error::request());
        }
        params.push((parts[2], parts[3]));
    }
    let result = get(
        &request.connection_id,
        &query("https://pianku.api.mgtv.com/rider/list/pcweb/v3", &params)?,
        false,
    )?;
    let rows = array(&result, "hitDocs")?;
    if rows.len() > 40 {
        return Err(Error::response());
    }
    let items = rows
        .iter()
        .map(catalog_item)
        .collect::<Result<Vec<_>, _>>()?;
    let more = result
        .get("hasMore")
        .and_then(Value::as_bool)
        .ok_or_else(Error::response)?;
    if more && items.is_empty() {
        return Err(Error::response());
    }
    Ok(
        json!([{"id":request.route_key,"title":string(&data,"channelName")?,"layout":"poster-grid","items":items,"cursor":if more{Some(format!("page:{}",pn+1))}else{None},"refreshable":true}]),
    )
}
fn catalog_item(row: &Value) -> Result<Value, Error> {
    let cid = numeric_id(string(row, "clipId")?)?;
    Ok(
        json!({"work":{"id":format!("clip:{cid}"),"title":string(row,"title")?,"kind":kind(number(row.get("fstlvlId"))),"identity":{"scheme":"mgtv.clip","value":cid},"overview":row.get("story"),"posterUrl":row.get("img").and_then(Value::as_str).and_then(safe_url),"backdropUrl":row.get("imgUrlH").and_then(Value::as_str).and_then(safe_url),"publishedAt":row.get("year")}}),
    )
}
fn kind(channel: Option<u64>) -> &'static str {
    if channel == Some(3) {
        "movie"
    } else {
        "series"
    }
}
pub(super) fn search(request: Search) -> Result<Value, Error> {
    validate_connection(&request.connection_id)?;
    if request.query.trim().is_empty()
        || request.query.len() > 240
        || request.query.chars().any(char::is_control)
    {
        return Err(Error::request());
    }
    let pn = page(request.cursor.as_deref())?;
    let page = pn.to_string();
    // The official desktop query includes the current account's non-secret uid
    // and sends its Cookie. Host still owns all session material.
    let account_id = match auth::account_id(&request.connection_id) {
        Ok(id) => Some(id),
        Err(error) if error.code == "permission-denied" => None,
        Err(error) => return Err(error),
    };
    let data = get(
        &request.connection_id,
        &query(
            "https://mobileso.bz.mgtv.com/pc/search/v1",
            &[
                ("q", &request.query),
                ("pn", &page),
                ("pc", "20"),
                ("corr", "1"),
                ("_support", "10000000"),
                ("uid", account_id.as_deref().unwrap_or("")),
            ],
        )?,
        account_id.is_some(),
    )?;
    if number(data.get("forbidden")).is_some_and(|value| value != 0) {
        return Err(Error::new("permission-denied", "芒果搜索暂受访问限制"));
    }
    let contents = array(&data, "contents")?;
    if contents.len() > 50 {
        return Err(Error::response());
    }
    // Search templates are heterogeneous. Map only cards with an official
    // /b/<clip>/<video> URL; never manufacture a work from a displayed title.
    let mut items = Vec::new();
    let mut seen = HashSet::new();
    for content in contents {
        if let Some(work) = search_work(content)? {
            if seen.insert(string(&work, "id")?.to_owned()) {
                items.push(json!({"work":work}));
            }
        }
    }
    let more = data
        .get("hasMore")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| number(data.get("totalHits")).unwrap_or(0) > pn as u64 * 20);
    if !contents.is_empty() && items.is_empty() {
        return Err(Error::new("invalid-response", "芒果搜索模板暂不支持"));
    }
    Ok(
        json!([{"id":"search","title":"搜索结果","layout":"poster-grid","items":items,"cursor":if more{Some(format!("page:{}",pn+1))}else{None}}]),
    )
}
fn search_work(content: &Value) -> Result<Option<Value>, Error> {
    let Some(row) = content.get("data") else {
        return Ok(None);
    };
    let candidates = row.get("url").and_then(Value::as_str).into_iter().chain(
        row.get("sourceList")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|source| source.get("url").and_then(Value::as_str)),
    );
    for raw_url in candidates {
        let Ok(url) = Url::parse("https://www.mgtv.com/").and_then(|base| base.join(raw_url))
        else {
            continue;
        };
        if url.host_str() != Some("www.mgtv.com")
            || !url.username().is_empty()
            || url.password().is_some()
            || !matches!(url.scheme(), "http" | "https")
        {
            continue;
        }
        let path = url.path().split('/').collect::<Vec<_>>();
        if path.len() < 3 || path[1] != "b" {
            continue;
        }
        let cid = path[2].trim_end_matches(".html");
        if numeric_id(cid).is_err() {
            continue;
        }
        let title = row
            .get("title")
            .or_else(|| row.get("name"))
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty() && v.len() <= 512 && !v.chars().any(char::is_control))
            .ok_or_else(Error::response)?;
        let kind = match content.get("type").and_then(Value::as_str) {
            Some("movie") => "movie",
            Some("video") => "video",
            _ => "series",
        };
        return Ok(Some(
            json!({"id":format!("clip:{cid}"),"title":title,"kind":kind,
            "identity":{"scheme":"mgtv.clip","value":cid},"overview":row.get("story"),
            "posterUrl":row.get("img").or_else(||row.get("pic")).and_then(Value::as_str).and_then(safe_url)}),
        ));
    }
    Ok(None)
}
pub(super) fn clip_id(item: &str) -> Result<&str, Error> {
    numeric_id(item.strip_prefix("clip:").ok_or_else(Error::request)?)
}
pub(super) fn info(connection: &str, cid: &str, vid: Option<&str>) -> Result<Value, Error> {
    let mut params = vec![("cid", numeric_id(cid)?)];
    if let Some(vid) = vid {
        params.push(("vid", numeric_id(vid)?));
    }
    let data = get(
        connection,
        &query("https://pcweb.api.mgtv.com/video/info", &params)?,
        false,
    )?;
    if data.pointer("/info/clipId").and_then(Value::as_str) != Some(cid) {
        return Err(Error::new(
            "invalid-response",
            "芒果视频详情身份与所选节目不一致",
        ));
    }
    Ok(data)
}
pub(super) fn detail(request: Detail) -> Result<Value, Error> {
    let cid = clip_id(&request.item_id)?;
    let data = info(&request.connection_id, cid, None)?;
    let vid = numeric_id(
        string(data.get("info").ok_or_else(Error::response)?, "videoId")
            .map_err(|_| Error::new("invalid-response", "芒果视频详情缺少视频身份"))?,
    )?;
    let mut rows = Vec::new();
    let mut seen = HashSet::new();
    let mut count = 1;
    for index in 0..64 {
        if index >= count {
            break;
        }
        let page = index.to_string();
        let episodes = get(
            &request.connection_id,
            &query(
                "https://pcweb.api.mgtv.com/episode/list",
                &[
                    ("video_id", vid),
                    ("page", &page),
                    ("size", "30"),
                    ("platform", "4"),
                    ("src", "mgtv"),
                    ("_support", "10000000"),
                ],
            )?,
            false,
        )?;
        count = number(episodes.get("total_page")).unwrap_or(1).max(1);
        if count > 64 {
            return Err(Error::new(
                "response-too-large",
                "芒果分集列表超过当前支持上限",
            ));
        }
        for key in ["list", "short"] {
            if let Some(list) = episodes.get(key).and_then(Value::as_array) {
                for row in list {
                    let segment = segment(cid, row, rows.len() + 1)?;
                    let id = string(&segment, "id")?.to_owned();
                    if seen.insert(id) {
                        rows.push(segment);
                    }
                }
            }
        }
        if rows.len() > 1000 {
            return Err(Error::new(
                "response-too-large",
                "芒果分集列表超过当前支持上限",
            ));
        }
    }
    // A movie's list can be empty; the authoritative selected video remains
    // a stable segment after checking its exact clip/video association.
    if rows.is_empty() {
        rows.push(json!({"id":format!("video:{vid}"),"title":data.pointer("/info/videoName"),"index":1,"versions":[{"id":format!("mgtv:{cid}:{vid}"),"label":"芒果 TV","sourceLabel":"芒果 TV","delivery":"online","variants":[]}]}));
    }
    work_from_info(cid, &data, rows)
}
fn segment(cid: &str, row: &Value, index: usize) -> Result<Value, Error> {
    let vid = numeric_id(string(row, "video_id")?)?;
    if row.get("clip_id").and_then(Value::as_str) != Some(cid) {
        return Err(Error::response());
    }
    let title = row
        .get("t3")
        .or_else(|| row.get("t1"))
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty() && v.len() <= 512)
        .ok_or_else(Error::response)?;
    let mut segment = json!({"id":format!("video:{vid}"),"title":title,"index":index,"versions":[{"id":format!("mgtv:{cid}:{vid}"),"label":"芒果 TV","sourceLabel":"芒果 TV","edition":if number(row.get("isIntact"))==Some(1){"正片"}else{"其他视频"},"delivery":"online","variants":[]}]});
    if let Some(episode) = row
        .get("t1")
        .and_then(Value::as_str)
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|v| *v > 0 && *v < 10000)
    {
        segment["episodeNumber"] = json!(episode);
    }
    Ok(segment)
}
fn work_from_info(cid: &str, data: &Value, segments: Vec<Value>) -> Result<Value, Error> {
    let info = data.get("info").ok_or_else(Error::response)?;
    Ok(
        json!({"id":format!("clip:{cid}"),"title":string(info,"title")?,"kind":kind(number(info.get("fstlvlId"))),"identity":{"scheme":"mgtv.clip","value":cid},"overview":info.pointer("/detail/story"),"posterUrl":info.get("clipImage2").and_then(Value::as_str).and_then(safe_url),"backdropUrl":info.get("clipImage").and_then(Value::as_str).and_then(safe_url),"publishedAt":info.pointer("/detail/releaseTime"),"segments":segments}),
    )
}
pub(super) fn artwork(request: Artwork) -> Result<Value, Error> {
    let route = artwork_route(request.scope_key.as_deref())?;
    let sections = feed(Feed {
        connection_id: request.connection_id.clone(),
        route_key: route,
        cursor: None,
        refresh_session: None,
    })?;
    let mut candidates = Vec::new();
    for item in sections
        .pointer("/0/items")
        .and_then(Value::as_array)
        .ok_or_else(Error::response)?
    {
        let work = &item["work"];
        let Some(url) = work.get("posterUrl").and_then(Value::as_str) else {
            continue;
        };
        if let Ok((reference, _)) = asset(&request.connection_id, url) {
            candidates.push(json!({"id":work["id"],"assetRef":reference}));
        }
        if candidates.len() == 9 {
            break;
        }
    }
    Ok(json!(candidates))
}
fn artwork_route(scope: Option<&str>) -> Result<String, Error> {
    match scope {
        None => Ok("catalog:1".to_owned()),
        Some(key) if key.starts_with("route:catalog:") => Ok(key[6..].to_owned()),
        Some(key) if key.starts_with("branch:category:") => {
            let parts = key[7..].split(':').collect::<Vec<_>>();
            if !(parts.len() == 2 || parts.len() == 4 && parts[2] == "filter") {
                return Err(Error::request());
            }
            Ok(format!("catalog:{}", numeric_id(parts[1])?))
        }
        _ => Err(Error::request()),
    }
}
pub(super) fn metadata(request: Metadata) -> Result<Value, Error> {
    let identity = media::identity(&request.item_id, &request.segment_id, &request.version_id)?;
    let data = info(&request.connection_id, identity.0, Some(identity.1))?;
    let info = data.get("info").ok_or_else(Error::response)?;
    Ok(
        json!({"version":1,"workId":request.item_id,"segmentId":request.segment_id,"kind":if number(info.get("fstlvlId"))==Some(3){"movie"}else{"episode"},"title":string(info,"videoName")?,"overview":info.pointer("/detail/story"),"publishedAt":info.pointer("/detail/releaseTime").and_then(Value::as_str).and_then(publication),"uniqueIds":{"mgtv.clip":identity.0,"mgtv.video":identity.1}}),
    )
}
fn publication(value: &str) -> Option<String> {
    if OffsetDateTime::parse(value, &Rfc3339).is_ok() {
        return Some(value.to_owned());
    }
    let description = time::format_description::parse_borrowed::<2>("[year]-[month]-[day]").ok()?;
    let date = time::Date::parse(value, &description).ok()?;
    let offset = time::UtcOffset::from_hms(8, 0, 0).ok()?;
    rfc3339(date.with_hms(0, 0, 0).ok()?.assume_offset(offset)).ok()
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
    fn real_catalog_and_episodes_preserve_provider_ids() {
        let list = fixture("catalog.json");
        let row = catalog_item(&list["data"]["hitDocs"][0]).unwrap();
        assert_eq!(row["work"]["id"], "clip:778406");
        let episodes = fixture("episodes.json");
        let rows = episodes["data"]["list"].as_array().unwrap();
        let a = segment("778406", &rows[0], 1).unwrap();
        assert_eq!(a["id"], "video:24589329");
        assert_eq!(a["versions"][0]["id"], "mgtv:778406:24589329");
        let mut bad = rows[0].clone();
        bad["clip_id"] = json!("999");
        assert!(segment("778406", &bad, 1).is_err());
    }
    #[test]
    fn cursor_and_identity_are_bounded() {
        assert_eq!(page(Some("page:2")).unwrap(), 2);
        assert!(page(Some("http://upstream")).is_err());
        assert!(clip_id("clip:../secret").is_err());
        assert_eq!(
            publication("2026-08-31").as_deref(),
            Some("2026-08-31T00:00:00+08:00")
        );
        assert!(publication("2026").is_none());
        assert_eq!(
            artwork_route(Some("branch:category:3:filter:year")).unwrap(),
            "catalog:3"
        );
        assert!(artwork_route(Some("branch:unknown:3")).is_err());
    }
    #[test]
    fn primary_script_search_templates_accept_only_provider_works() {
        // Synthetic cards follow the official Smovie/Sserial sourceList and
        // Svideo direct-url rendering code. They are not live success fixtures.
        let work = search_work(&json!({"type":"movie","data":{"name":"电影","sourceList":[{"url":"https://other.invalid/b/999"},{"url":"https://www.mgtv.com/b/687100/22025165.html"}]}})).unwrap().unwrap();
        assert_eq!(work["id"], "clip:687100");
        assert_eq!(work["kind"], "movie");
        assert!(search_work(
            &json!({"type":"movie","data":{"name":"电影","url":"https://evil.invalid/b/687100"}})
        )
        .unwrap()
        .is_none());
    }
}
