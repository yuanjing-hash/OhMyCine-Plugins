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
fn category_nodes(channel: &str, fields: &[Value]) -> Result<Vec<Value>, Error> {
    let mut nodes = vec![
        json!({"id":format!("all-{channel}"),"title":"全部","kind":"feed","routeKey":format!("catalog:{channel}"),"refreshable":true}),
    ];
    // Official kind is the content taxonomy for every current channel. Other
    // filter dimensions must not become category directories or substitutes.
    let mut content_fields = fields
        .iter()
        .filter(|field| field.get("eName").and_then(Value::as_str) == Some("kind"));
    if let Some(field) = content_fields.next() {
        if content_fields.next().is_some() {
            return Err(Error::response());
        }
        let status = number(field.get("status")).ok_or_else(Error::response)?;
        if status == 1 {
            nodes.extend(tag_nodes(channel, "kind", field, false)?);
        }
    }
    if nodes.len() > 100 {
        return Err(Error::response());
    }
    Ok(nodes)
}
fn tag_nodes(
    channel: &str,
    field_name: &str,
    field: &Value,
    include_all: bool,
) -> Result<Vec<Value>, Error> {
    let tags = array(field, "items")?;
    if tags.len() > 100 {
        return Err(Error::response());
    }
    let mut nodes = Vec::new();
    let mut seen = HashSet::new();
    for tag in tags {
        let status = number(tag.get("status")).ok_or_else(Error::response)?;
        if status != 1 {
            continue;
        }
        let id = string(tag, "tagId")?;
        let title = string(tag, "tagName")?;
        let route = format!("catalog:{channel}:{field_name}:{id}");
        if id.len() > 124
            || !id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
            || title.len() > 256
            || route.len() > 256
            || !seen.insert(id)
        {
            return Err(Error::response());
        }
        // Keep the original unfiltered route as the sole visible All entry;
        // old filter tokens still include the provider's a1 tag normally.
        if include_all || id != "a1" {
            nodes.push(json!({"id":format!("tag-{id}"),"title":title,"kind":"feed","routeKey":route,"refreshable":true}));
        }
    }
    Ok(nodes)
}
fn navigation_parent(key: &str) -> Result<(&str, Option<&str>), Error> {
    let parts = key.split(':').collect::<Vec<_>>();
    if parts.first() != Some(&"category")
        || !(parts.len() == 2 || parts.len() == 4 && parts[2] == "filter")
    {
        return Err(Error::request());
    }
    let channel = numeric_id(parts[1])?;
    let field = if parts.len() == 4 {
        if parts[3].is_empty() || !parts[3].bytes().all(|c| c.is_ascii_alphanumeric()) {
            return Err(Error::request());
        }
        Some(parts[3])
    } else {
        None
    };
    Ok((channel, field))
}
fn filter_nodes(channel: &str, fields: &[Value], name: &str) -> Result<Vec<Value>, Error> {
    let field = fields
        .iter()
        .find(|field| field.get("eName").and_then(Value::as_str) == Some(name))
        .ok_or_else(Error::request)?;
    tag_nodes(channel, name, field, true)
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
            let (channel, filter) = navigation_parent(key)?;
            let data = config(&request.connection_id, channel)?;
            let fields = array(&data, "listItems")?;
            match filter {
                None => category_nodes(channel, fields)?,
                Some(name) => filter_nodes(channel, fields, name)?,
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
type FeedRoute<'a> = (&'a str, Option<(&'a str, &'a str)>);

fn feed_route(route_key: &str) -> Result<FeedRoute<'_>, Error> {
    let parts = route_key.split(':').collect::<Vec<_>>();
    if parts.first() != Some(&"catalog") || !(parts.len() == 2 || parts.len() == 4) {
        return Err(Error::request());
    }
    Ok((
        numeric_id(parts[1])?,
        if parts.len() == 4 {
            Some((parts[2], parts[3]))
        } else {
            None
        },
    ))
}
fn feed_url(
    channel: &str,
    filter: Option<(&str, &str)>,
    data: &Value,
    pn: u32,
) -> Result<String, Error> {
    let page_string = pn.to_string();
    let mut params = vec![
        ("platform", "pcweb"),
        ("channelId", channel),
        ("pn", page_string.as_str()),
        ("pc", "40"),
        ("hudong", "1"),
        ("_support", "10000000"),
    ];
    if let Some((name, id)) = filter {
        let field = array(data, "listItems")?
            .iter()
            .find(|f| f.get("eName").and_then(Value::as_str) == Some(name))
            .ok_or_else(Error::request)?;
        if !array(field, "items")?.iter().any(|t| {
            t.get("tagId").and_then(Value::as_str) == Some(id) && number(t.get("status")) == Some(1)
        }) {
            return Err(Error::request());
        }
        params.push((name, id));
    }
    query("https://pianku.api.mgtv.com/rider/list/pcweb/v3", &params)
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
    let (channel, filter) = feed_route(&request.route_key)?;
    let data = config(&request.connection_id, channel)?;
    let pn = page(request.cursor.as_deref())?;
    let result = get(
        &request.connection_id,
        &feed_url(channel, filter, &data, pn)?,
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
    let mut short_rows = Vec::new();
    let mut seen = HashSet::new();
    let channel = number(data.pointer("/info/fstlvlId"));
    let mut count = 1;
    // Page zero means "locate the currently selected video", not first page.
    // On a long variety series it can start at page two (or later).
    for index in 1..=64 {
        if index > count {
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
        let page_count = episode_page_count(&episodes, index)?;
        if index > 1 && count != page_count {
            return Err(Error::response());
        }
        count = page_count;
        for row in array(&episodes, "list")? {
            let item = segment(cid, row, rows.len() + 1, channel, false)?;
            if seen.insert(string(&item, "id")?.to_owned()) {
                rows.push(item);
            }
        }
        // The official endpoint repeats the same short list on every page.
        // Retain it once, after the complete regular list, rather than inserting
        // shorts between episode pages and changing chronological order.
        if index == 1 {
            short_rows = array(&episodes, "short")?.clone();
        }
        if rows.len() + short_rows.len() > 1000 {
            return Err(Error::new(
                "response-too-large",
                "芒果分集列表超过当前支持上限",
            ));
        }
    }
    for row in &short_rows {
        let item = segment(cid, row, rows.len() + 1, channel, true)?;
        if seen.insert(string(&item, "id")?.to_owned()) {
            rows.push(item);
        }
    }
    // A movie's list can be empty; the authoritative selected video remains
    // a stable segment after checking its exact clip/video association.
    if rows.is_empty() {
        rows.push(json!({"id":format!("video:{vid}"),"title":data.pointer("/info/videoName"),"index":1,"versions":[{"id":format!("mgtv:{cid}:{vid}"),"label":"芒果 TV","sourceLabel":"芒果 TV","delivery":"online","variants":[]}]}));
    }
    work_from_info(cid, &data, rows)
}
fn episode_page_count(data: &Value, page: u64) -> Result<u64, Error> {
    let count = number(data.get("total_page"))
        .ok_or_else(Error::response)?
        .max(1);
    if count > 64 {
        return Err(Error::new(
            "response-too-large",
            "芒果分集列表超过当前支持上限",
        ));
    }
    if page == 0
        || page > count
        || number(data.get("current_page")) != Some(page)
        // Official drama lists append bonus rows beyond the requested size.
        || array(data, "list")?.len() > 1000
        || array(data, "short")?.len() > 100
    {
        return Err(Error::response());
    }
    Ok(count)
}
fn segment_edition(row: &Value, short: bool) -> &'static str {
    if short {
        return "花絮片段";
    }
    let summary = row.get("t2").and_then(Value::as_str).unwrap_or_default();
    if summary.starts_with("超前彩蛋") {
        "超前彩蛋"
    } else if summary.to_ascii_lowercase().starts_with("plus版") {
        "Plus版"
    } else if summary.starts_with("超前聚会") {
        "超前聚会"
    } else if number(row.get("isIntact")) == Some(7) {
        "彩蛋"
    } else if number(row.get("isIntact")) == Some(1) {
        "正片"
    } else {
        "其他视频"
    }
}
fn segment(
    cid: &str,
    row: &Value,
    index: usize,
    channel: Option<u64>,
    short: bool,
) -> Result<Value, Error> {
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
    let edition = segment_edition(row, short);
    let mut segment = json!({"id":format!("video:{vid}"),"title":title,"index":index,"versions":[{"id":format!("mgtv:{cid}:{vid}"),"label":"芒果 TV","sourceLabel":"芒果 TV","edition":edition,"delivery":"online","variants":[]}]});
    // t1 is an ordering slot for variety programs: official t1=32 can be
    // 第8期（上）, while 31 is its bonus. Only regular drama/anime rows carry
    // actual episode numbers; short/bonus rows never inherit this counter.
    if matches!(channel, Some(2 | 10)) && edition == "正片" {
        if let Some(episode) = row
            .get("t1")
            .and_then(Value::as_str)
            .and_then(|v| v.parse::<u32>().ok())
            .filter(|v| *v > 0 && *v < 10000)
        {
            segment["episodeNumber"] = json!(episode);
        }
    }
    Ok(segment)
}
fn work_from_info(cid: &str, data: &Value, segments: Vec<Value>) -> Result<Value, Error> {
    let info = data.get("info").ok_or_else(Error::response)?;
    let default = info
        .get("videoId")
        .and_then(Value::as_str)
        .and_then(|id| numeric_id(id).ok())
        .map(|id| format!("video:{id}"))
        .filter(|id| {
            segments
                .iter()
                .any(|segment| segment.get("id").and_then(Value::as_str) == Some(id))
        });
    let mut work = json!({"id":format!("clip:{cid}"),"title":string(info,"title")?,"kind":kind(number(info.get("fstlvlId"))),"identity":{"scheme":"mgtv.clip","value":cid},"overview":info.pointer("/detail/story"),"posterUrl":info.get("clipImage2").and_then(Value::as_str).and_then(safe_url),"backdropUrl":info.get("clipImage").and_then(Value::as_str).and_then(safe_url),"publishedAt":info.pointer("/detail/releaseTime"),"segments":segments});
    if let Some(id) = default {
        work["defaultSegmentId"] = json!(id);
    }
    Ok(work)
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
    fn official_drama_children_flatten_kind_with_one_all_feed() {
        let data = fixture("channel.json")["data"].clone();
        let nodes = category_nodes("2", array(&data, "listItems").unwrap()).unwrap();
        assert_eq!(nodes.len(), 25);
        assert_eq!(nodes[0]["id"], "all-2");
        assert_eq!(nodes[0]["routeKey"], "catalog:2");
        assert_eq!(nodes[1]["routeKey"], "catalog:2:kind:3094");
        assert_eq!(nodes[1]["title"], "爱情");
        assert_eq!(
            nodes.iter().filter(|node| node["title"] == "全部").count(),
            1
        );
        assert!(nodes.iter().all(|node| node["kind"] == "feed"
            && node["refreshable"] == true
            && node.get("nodeKey").is_none()
            && node.get("hasChildren").is_none()));
        let kind = &data["listItems"][0];
        let expected = kind["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|tag| tag["tagId"] != "a1")
            .map(|tag| {
                (
                    tag["tagId"].as_str().unwrap(),
                    tag["tagName"].as_str().unwrap(),
                )
            })
            .collect::<Vec<_>>();
        let actual = nodes[1..]
            .iter()
            .map(|node| {
                (
                    node["id"].as_str().unwrap().strip_prefix("tag-").unwrap(),
                    node["title"].as_str().unwrap(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }
    #[test]
    fn all_seven_official_channels_use_kind_identity_and_order() {
        // Reduced synthetic configs use the first public kind tag observed for
        // each channel on 2026-10-12. They are not complete provider responses.
        let samples = [
            ("1", "8", "音乐"),
            ("2", "3094", "爱情"),
            ("3", "175", "爱情"),
            ("10", "184", "儿歌"),
            ("50", "84", "恋爱"),
            ("51", "98", "文化"),
            ("115", "134", "幼儿"),
        ];
        let channels = fixture("channels.json")["data"].clone();
        let mut channels = channels.as_array().unwrap().clone();
        channels.retain(|row| number(row.get("status")) == Some(1));
        channels.sort_by_key(|row| number(row.get("rank")).unwrap());
        assert_eq!(channels.len(), samples.len());
        for (row, (channel, tag_id, title)) in channels.iter().zip(samples) {
            assert_eq!(row["channelId"], channel);
            // The first field and its translated name are deliberately
            // unrelated to the content taxonomy. Only eName=kind selects it.
            let fields = vec![
                json!({"eName":"sort","typeName":"类型","status":1,"items":[{"tagId":"c1","tagName":"最新","status":1}]}),
                json!({"eName":"kind","typeName":"内容","status":1,"items":[
                    {"tagId":"a1","tagName":"全部","status":1},
                    {"tagId":tag_id,"tagName":title,"status":1},
                    {"tagId":"hidden","tagName":"不显示","status":0},
                    {"tagId":"last","tagName":"最后一项","status":1}
                ]}),
                json!({"eName":"year","status":1,"items":[{"tagId":"2026","tagName":"2026年","status":1}]}),
            ];
            let nodes = category_nodes(channel, &fields).unwrap();
            assert_eq!(nodes.len(), 3);
            assert_eq!(nodes[0]["id"], format!("all-{channel}"));
            assert_eq!(nodes[0]["routeKey"], format!("catalog:{channel}"));
            assert_eq!(nodes[1]["id"], format!("tag-{tag_id}"));
            assert_eq!(nodes[1]["title"], title);
            assert_eq!(
                nodes[1]["routeKey"],
                format!("catalog:{channel}:kind:{tag_id}")
            );
            assert_eq!(nodes[2]["id"], "tag-last");
            assert!(nodes.iter().all(|node| node["kind"] == "feed"));
        }
    }
    #[test]
    fn missing_disabled_or_empty_kind_keeps_only_unfiltered_all() {
        let other =
            json!({"eName":"sort","status":1,"items":[{"tagId":"c1","tagName":"最新","status":1}]});
        let variants = [
            vec![],
            vec![other.clone()],
            vec![other.clone(), json!({"eName":"kind","status":0})],
            vec![other.clone(), json!({"eName":"kind","status":1,"items":[]})],
            vec![
                other,
                json!({"eName":"kind","status":1,"items":[
                    {"tagId":"a1","tagName":"全部","status":1},
                    {"tagId":"hidden","tagName":"不显示","status":0}
                ]}),
            ],
        ];
        for fields in variants {
            let nodes = category_nodes("1", &fields).unwrap();
            assert_eq!(
                nodes,
                vec![
                    json!({"id":"all-1","title":"全部","kind":"feed","routeKey":"catalog:1","refreshable":true})
                ]
            );
        }
    }
    #[test]
    fn enabled_kind_rejects_malformed_tags_and_duplicate_identities() {
        for items in [Value::Null, json!({}), json!("tags"), json!([true])] {
            let fields = vec![json!({"eName":"kind","status":1,"items":items})];
            assert_eq!(
                category_nodes("1", &fields).unwrap_err().code,
                "invalid-response"
            );
        }
        assert!(category_nodes("1", &[json!({"eName":"kind","status":1})]).is_err());
        for status in [Value::Null, json!(true), json!("unknown")] {
            assert!(
                category_nodes("1", &[json!({"eName":"kind","status":status,"items":[]})]).is_err()
            );
            let field = json!({"eName":"kind","status":1,"items":[{"tagId":"8","tagName":"类型","status":status}]});
            assert!(category_nodes("1", &[field]).is_err());
        }
        for id in [
            "",
            "has:colon",
            "../path",
            "https://host",
            "有中文",
            &"x".repeat(125),
        ] {
            let fields = vec![
                json!({"eName":"kind","status":1,"items":[{"tagId":id,"tagName":"类型","status":1}]}),
            ];
            assert_eq!(
                category_nodes("1", &fields).unwrap_err().code,
                "invalid-response"
            );
        }
        for title in ["", "bad\nlabel", &"x".repeat(257)] {
            let fields = vec![
                json!({"eName":"kind","status":1,"items":[{"tagId":"8","tagName":title,"status":1}]}),
            ];
            assert_eq!(
                category_nodes("1", &fields).unwrap_err().code,
                "invalid-response"
            );
        }
        for id in ["8", "a1"] {
            let tag = json!({"tagId":id,"tagName":"类型","status":1});
            let fields = vec![json!({"eName":"kind","status":1,"items":[tag.clone(),tag]})];
            assert_eq!(
                category_nodes("1", &fields).unwrap_err().code,
                "invalid-response"
            );
        }
        let kind = json!({"eName":"kind","status":1,"items":[]});
        assert!(category_nodes("1", &[kind.clone(), kind]).is_err());
    }
    #[test]
    fn category_node_budget_counts_the_all_entry_without_truncation() {
        let tags = (1..=99)
            .map(|id| json!({"tagId":id.to_string(),"tagName":format!("类型{id}"),"status":1}))
            .collect::<Vec<_>>();
        let mut field = json!({"eName":"kind","status":1,"items":tags});
        let nodes = category_nodes("1", &[field.clone()]).unwrap();
        assert_eq!(nodes.len(), 100);
        assert_eq!(nodes[99]["routeKey"], "catalog:1:kind:99");
        field["items"]
            .as_array_mut()
            .unwrap()
            .push(json!({"tagId":"100","tagName":"类型100","status":1}));
        // 100 non-All tags plus our unfiltered All is an invalid 101 nodes.
        assert_eq!(
            category_nodes("1", &[field.clone()]).unwrap_err().code,
            "invalid-response"
        );
        // The official a1 sentinel can fill the raw 100-tag bound while the
        // projected directory still contains exactly 100 accepted nodes.
        field["items"].as_array_mut().unwrap()[99] =
            json!({"tagId":"a1","tagName":"全部","status":1});
        assert_eq!(category_nodes("1", &[field.clone()]).unwrap().len(), 100);
        field["items"]
            .as_array_mut()
            .unwrap()
            .push(json!({"tagId":"101","tagName":"类型101","status":0}));
        assert!(category_nodes("1", &[field]).is_err());
    }
    #[test]
    fn legacy_filter_tokens_and_stable_leaf_routes_remain_accepted() {
        let data = fixture("channel.json")["data"].clone();
        let fields = array(&data, "listItems").unwrap();
        assert_eq!(navigation_parent("category:2").unwrap(), ("2", None));
        let flat = category_nodes("2", fields).unwrap();
        for name in ["kind", "year", "area", "chargeInfo", "sort"] {
            let key = format!("category:2:filter:{name}");
            let (channel, filter) = navigation_parent(&key).unwrap();
            assert_eq!(filter, Some(name));
            let legacy = filter_nodes(channel, fields, filter.unwrap()).unwrap();
            assert!(!legacy.is_empty());
            assert!(legacy.iter().all(|node| node["kind"] == "feed"));
            if name == "kind" {
                assert_eq!(legacy[0]["routeKey"], "catalog:2:kind:a1");
                assert_eq!(legacy[1..], flat[1..]);
            }
            let route = legacy.last().unwrap()["routeKey"].as_str().unwrap();
            let (channel, filter) = feed_route(route).unwrap();
            let url = Url::parse(&feed_url(channel, filter, &data, 2).unwrap()).unwrap();
            assert!(url.query_pairs().any(|(key, _)| key == name));
            assert_eq!(
                artwork_route(Some(&format!("route:{route}"))).unwrap(),
                route
            );
            assert_eq!(
                artwork_route(Some(&format!("branch:{key}"))).unwrap(),
                "catalog:2"
            );
        }
        assert_eq!(
            filter_nodes("2", fields, "unknown").unwrap_err().code,
            "invalid-request"
        );
        for key in [
            "category:0",
            "category:2:kind",
            "category:2:filter",
            "category:2:filter:kind:extra",
            "category:2:filter:",
            "category:2:filter:bad/name",
            "channel:2",
        ] {
            assert_eq!(navigation_parent(key).unwrap_err().code, "invalid-request");
        }
    }
    #[test]
    fn flat_kind_feed_keeps_exact_filter_defaults_and_paging_query() {
        let data = fixture("channel.json")["data"].clone();
        let nodes = category_nodes("2", array(&data, "listItems").unwrap()).unwrap();
        let route = nodes[1]["routeKey"].as_str().unwrap();
        let (channel, filter) = feed_route(route).unwrap();
        for (cursor, expected_page) in [(None, "1"), (Some("page:2"), "2")] {
            let pn = page(cursor).unwrap();
            let url = Url::parse(&feed_url(channel, filter, &data, pn).unwrap()).unwrap();
            assert_eq!(url.scheme(), "https");
            assert_eq!(url.host_str(), Some("pianku.api.mgtv.com"));
            assert_eq!(url.path(), "/rider/list/pcweb/v3");
            let params = url.query_pairs().collect::<Vec<_>>();
            let expected = [
                ("platform", "pcweb"),
                ("channelId", "2"),
                ("pn", expected_page),
                ("pc", "40"),
                ("hudong", "1"),
                ("_support", "10000000"),
                ("kind", "3094"),
            ];
            assert_eq!(
                params
                    .iter()
                    .map(|(key, value)| (key.as_ref(), value.as_ref()))
                    .collect::<Vec<_>>(),
                expected
            );
        }
        let (channel, filter) = feed_route("catalog:2").unwrap();
        let all = Url::parse(&feed_url(channel, filter, &data, 1).unwrap()).unwrap();
        assert_eq!(all.query_pairs().count(), 6);
        assert!(!all.query_pairs().any(|(key, _)| key == "kind"));
        for route in [
            "catalog:0",
            "catalog:2:kind",
            "catalog:2:kind:3094:extra",
            "other:2",
        ] {
            assert_eq!(feed_route(route).unwrap_err().code, "invalid-request");
        }
        for filter in [Some(("kind", "unknown")), Some(("unknown", "3094"))] {
            assert_eq!(
                feed_url("2", filter, &data, 1).unwrap_err().code,
                "invalid-request"
            );
        }
        let mut disabled = data.clone();
        disabled["listItems"][0]["items"][1]["status"] = json!(0);
        assert_eq!(
            feed_url("2", Some(("kind", "3094")), &disabled, 1)
                .unwrap_err()
                .code,
            "invalid-request"
        );
    }
    #[test]
    fn real_catalog_and_episodes_preserve_provider_ids() {
        let list = fixture("catalog.json");
        let row = catalog_item(&list["data"]["hitDocs"][0]).unwrap();
        assert_eq!(row["work"]["id"], "clip:778406");
        let episodes = fixture("episodes.json");
        let rows = episodes["data"]["list"].as_array().unwrap();
        let a = segment("778406", &rows[0], 1, Some(2), false).unwrap();
        assert_eq!(a["id"], "video:24589329");
        assert_eq!(a["versions"][0]["id"], "mgtv:778406:24589329");
        let mut bad = rows[0].clone();
        bad["clip_id"] = json!("999");
        assert!(segment("778406", &bad, 1, Some(2), false).is_err());
    }
    #[test]
    fn official_variety_pages_preserve_order_titles_and_editions() {
        let first = fixture("episodes-variety-page1.json")["data"].clone();
        let second = fixture("episodes-variety-page2.json")["data"].clone();
        assert_eq!(episode_page_count(&first, 1).unwrap(), 2);
        assert_eq!(episode_page_count(&second, 2).unwrap(), 2);
        // Page-zero auto-location can return page two, which must not be
        // accepted as a requested first page.
        assert!(episode_page_count(&second, 1).is_err());
        let list = first["list"]
            .as_array()
            .unwrap()
            .iter()
            .chain(second["list"].as_array().unwrap());
        let segments = list
            .enumerate()
            .map(|(index, row)| segment("887305", row, index + 1, Some(1), false).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(segments.len(), 50);
        assert_eq!(segments[0]["id"], "video:24485448");
        assert_eq!(segments[30]["id"], "video:24618061");
        assert!(segments.iter().all(|s| s.get("episodeNumber").is_none()));
        assert_eq!(segments[30]["versions"][0]["edition"], "超前彩蛋");
        assert_eq!(segments[31]["versions"][0]["edition"], "正片");
        assert!(segments[31]["title"].as_str().unwrap().contains("第8期"));
        assert!(segments[31]["title"].as_str().unwrap().ends_with("（上）"));
        assert!(segments[32]["title"].as_str().unwrap().ends_with("（下）"));
        assert_eq!(segments[33]["versions"][0]["edition"], "Plus版");
        let short = segment("887305", &first["short"][0], 51, Some(1), true).unwrap();
        assert_eq!(short["versions"][0]["edition"], "花絮片段");
        assert!(short.get("episodeNumber").is_none());
    }
    #[test]
    fn actual_numbered_drama_does_not_number_bonus_or_short_clips() {
        let data = fixture("episodes.json")["data"].clone();
        assert!(data["list"].as_array().unwrap().len() > 30);
        assert_eq!(episode_page_count(&data, 1).unwrap(), 1);
        let main = &data["list"][0];
        let episode = segment("778406", main, 1, Some(2), false).unwrap();
        assert_eq!(episode["episodeNumber"], 1);
        let bonus = data["list"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| number(r.get("isIntact")) == Some(7))
            .unwrap();
        let bonus = segment("778406", bonus, 25, Some(2), false).unwrap();
        assert_eq!(bonus["versions"][0]["edition"], "彩蛋");
        assert!(bonus.get("episodeNumber").is_none());
        let mut short = data["short"][0].clone();
        short["t1"] = json!("32");
        short["isIntact"] = json!("1");
        assert!(segment("778406", &short, 42, Some(2), true)
            .unwrap()
            .get("episodeNumber")
            .is_none());
    }
    #[test]
    fn episode_page_shape_and_bounds_fail_closed() {
        let first = fixture("episodes-variety-page1.json")["data"].clone();
        for key in ["current_page", "total_page"] {
            let mut invalid = first.clone();
            invalid.as_object_mut().unwrap().remove(key);
            assert!(episode_page_count(&invalid, 1).is_err());
        }
        let mut invalid = first.clone();
        invalid["total_page"] = json!(65);
        assert_eq!(
            episode_page_count(&invalid, 1).unwrap_err().code,
            "response-too-large"
        );
        invalid = first.clone();
        invalid["list"] = json!(vec![first["list"][0].clone(); 1001]);
        assert!(episode_page_count(&invalid, 1).is_err());
    }
    #[test]
    fn official_default_video_does_not_reorder_segments_or_guess_missing_ids() {
        let info = fixture("info-variety.json")["data"].clone();
        let mut rows = Vec::new();
        for name in ["episodes-variety-page1.json", "episodes-variety-page2.json"] {
            let page = fixture(name);
            for row in page["data"]["list"].as_array().unwrap() {
                rows.push(segment("887305", row, rows.len() + 1, Some(1), false).unwrap());
            }
        }
        let work = work_from_info("887305", &info, rows.clone()).unwrap();
        assert_eq!(work["defaultSegmentId"], "video:24691407");
        assert_eq!(work["segments"][0]["id"], "video:24485448");
        rows.retain(|row| row["id"] != "video:24691407");
        assert!(work_from_info("887305", &info, rows)
            .unwrap()
            .get("defaultSegmentId")
            .is_none());
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
