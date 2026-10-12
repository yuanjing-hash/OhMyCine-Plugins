use super::*;

// Fabricated values, with only the container/element types taken from the
// finite authenticated response observation. No real resource text is retained.
fn column_fixture() -> Value {
    serde_json::from_slice(include_bytes!("fixtures/search-columns.json")).unwrap()
}

#[test]
fn serializes_observed_inlist_columns_and_uses_page_pages() {
    let wire =
        search_wire(parse_json_search(include_bytes!("fixtures/search-columns.json"), 1).unwrap());
    assert_eq!(wire["items"].as_array().unwrap().len(), 2);
    assert_eq!(wire["items"][0]["id"], "98765");
    assert_eq!(wire["items"][0]["title"], "示例资源 S01E01 2160p");
    assert_eq!(wire["items"][0]["sizeBytes"], 4_939_212_390_i64);
    assert_eq!(wire["items"][0]["seeders"], 18);
    assert_eq!(wire["items"][0]["updatedAt"], "2025-05-14T08:00:00Z");
    assert!(wire["items"][0].get("tags").is_none());
    assert_eq!(wire["page"], 1);
    assert_eq!(wire["hasNext"], true);
}

#[test]
fn rejects_misaligned_missing_and_wrongly_typed_columns_before_pairing() {
    for key in ["i", "title", "k", "size", "seeds", "time"] {
        let mut payload = column_fixture();
        payload["inlist"][key].as_array_mut().unwrap().pop();
        assert!(
            parse_json_search(&serde_json::to_vec(&payload).unwrap(), 1).is_none(),
            "short {key}"
        );
        let mut payload = column_fixture();
        payload["inlist"].as_object_mut().unwrap().remove(key);
        assert!(
            parse_json_search(&serde_json::to_vec(&payload).unwrap(), 1).is_none(),
            "missing {key}"
        );
        let mut payload = column_fixture();
        payload["inlist"][key] = json!({});
        assert!(
            parse_json_search(&serde_json::to_vec(&payload).unwrap(), 1).is_none(),
            "object {key}"
        );
        let mut payload = column_fixture();
        payload["inlist"][key][1] = json!(false);
        assert!(
            parse_json_search(&serde_json::to_vec(&payload).unwrap(), 1).is_none(),
            "element {key}"
        );
    }
    for (key, value) in [
        ("i", json!(98765)),
        ("seeds", json!("18")),
        ("size", json!(1024)),
        ("time", json!(1_757_577_600)),
    ] {
        let mut payload = column_fixture();
        payload["inlist"][key][1] = value;
        assert!(parse_json_search(&serde_json::to_vec(&payload).unwrap(), 1).is_none());
    }
}

#[test]
fn columns_empty_is_distinct_from_unknown_or_all_invalid() {
    let mut empty = column_fixture();
    for key in ["i", "title", "k", "size", "seeds", "time"] {
        empty["inlist"][key] = json!([]);
    }
    empty["page"]["pages"] = json!(0);
    let wire = search_wire(parse_json_search(&serde_json::to_vec(&empty).unwrap(), 1).unwrap());
    assert_eq!(wire, json!({"items":[], "page":1, "hasNext":false}));
    for input in [
        json!({"inlist":{},"page":{"pages":0}}),
        json!({"page":{"pages":0}}),
        json!({"inlist":{"fixture":[]}}),
    ] {
        assert!(parse_json_search(&serde_json::to_vec(&input).unwrap(), 1).is_none());
    }
    let mut invalid = column_fixture();
    invalid["inlist"]["i"] = json!(["bad/id", "a".repeat(129)]);
    assert!(parse_json_search(&serde_json::to_vec(&invalid).unwrap(), 1).is_none());
    let mut conflicting = column_fixture();
    conflicting["data"] = json!({"inlist":[]});
    assert!(parse_json_search(&serde_json::to_vec(&conflicting).unwrap(), 1).is_none());
    let direct_columns =
        json!({"i":[],"title":[],"k":[],"size":[],"seeds":[],"time":[],"page":{"pages":0}});
    assert!(parse_json_search(&serde_json::to_vec(&direct_columns).unwrap(), 1).is_none());
}

#[test]
fn column_pagination_uses_total_pages_even_for_short_pages() {
    let payload = column_fixture();
    let encoded = serde_json::to_vec(&payload).unwrap();
    assert_eq!(
        search_wire(parse_json_search(&encoded, 2).unwrap())["hasNext"],
        true
    );
    assert_eq!(
        search_wire(parse_json_search(&encoded, 17).unwrap())["hasNext"],
        false
    );
    assert!(parse_json_search(&encoded, 18).is_none());
    for pages in [json!(-1), json!(1.5), json!("17"), json!(10_001), json!(0)] {
        let mut payload = column_fixture();
        payload["page"]["pages"] = pages;
        assert!(parse_json_search(&serde_json::to_vec(&payload).unwrap(), 1).is_none());
    }
    let mut missing = column_fixture();
    missing.as_object_mut().unwrap().remove("page");
    assert!(parse_json_search(&serde_json::to_vec(&missing).unwrap(), 1).is_none());
    let mut conflicting = column_fixture();
    conflicting["footer"] = json!({"next":false});
    assert!(parse_json_search(&serde_json::to_vec(&conflicting).unwrap(), 1).is_none());
}

#[test]
fn column_output_normalizes_display_fields_and_preserves_valid_neighbors() {
    let mut payload = column_fixture();
    payload["inlist"]["title"][0] = json!(format!("{}\nTitle", "中".repeat(171)));
    payload["inlist"]["k"][0] = json!(format!("{}\0fixture", "类".repeat(22)));
    payload["inlist"]["seeds"][1] = json!(u64::MAX);
    let wire = search_wire(parse_json_search(&serde_json::to_vec(&payload).unwrap(), 1).unwrap());
    assert_eq!(wire["items"].as_array().unwrap().len(), 1);
    assert_eq!(wire["items"][0]["id"], "98765");
    assert_eq!(wire["items"][0]["title"], "中".repeat(170));
    assert_eq!(wire["items"][0]["tags"][0], "类".repeat(21));
    assert_eq!(wire["hasNext"], true);
    let mut invalid = column_fixture();
    invalid["inlist"]["seeds"] = json!([1_000_001, -1]);
    assert!(parse_json_search(&serde_json::to_vec(&invalid).unwrap(), 1).is_none());
}

#[test]
fn column_count_bound_is_exact_and_never_zips_a_truncated_subset() {
    for count in [200, 201] {
        let mut payload = column_fixture();
        for key in ["i", "title", "k", "size", "seeds", "time"] {
            let value = payload["inlist"][key][0].clone();
            payload["inlist"][key] = Value::Array(vec![value; count]);
        }
        let result = parse_json_search(&serde_json::to_vec(&payload).unwrap(), 1);
        if count == 200 {
            assert_eq!(
                search_wire(result.unwrap())["items"]
                    .as_array()
                    .unwrap()
                    .len(),
                200
            );
        } else {
            assert!(result.is_none());
        }
    }
}

#[test]
fn synthetic_same_contract_bootstrap_matches_column_json() {
    let payload = column_fixture();
    let body = format!("<script>_obj.header={{\"n\":1,\"u\":{{\"u\":\"fixture\"}}}};_obj.search={payload};_obj.footer={{\"fixture\":true}};renderFixture();</script>");
    let expected =
        search_wire(parse_json_search(&serde_json::to_vec(&payload).unwrap(), 1).unwrap());
    assert_eq!(
        search_wire(parse_html_search(body.as_bytes(), 1).unwrap()),
        expected
    );
}

fn search_wire(mut response: ResourceSearchResponse) -> Value {
    normalize_search_times_at(&mut response, Some(1_757_577_600));
    let bytes = serde_json::to_vec(&response).unwrap();
    let wire: Value = serde_json::from_slice(&bytes).unwrap();
    assert_search_contract(&wire);
    wire
}

fn assert_search_contract(wire: &Value) {
    let object = wire.as_object().unwrap();
    assert_eq!(object.len(), 3);
    assert!(wire["page"].as_i64().is_some_and(|page| page > 0));
    assert!(wire["hasNext"].is_boolean());
    let items = wire["items"].as_array().unwrap();
    assert!(items.len() <= 200);
    for item in items {
        assert!(item.as_object().unwrap().keys().all(|key| matches!(
            key.as_str(),
            "id" | "title" | "sizeBytes" | "seeders" | "updatedAt" | "tags"
        )));
        for (field, limit) in [("id", 256), ("title", 512)] {
            let value = item[field].as_str().unwrap();
            assert!(!value.trim().is_empty(), "empty {field}");
            assert!(value.len() <= limit, "oversized {field}");
            assert!(!value.contains(['\0', '\r', '\n']), "unsafe {field}");
        }
        assert!((0..=8_i64 << 40).contains(&item["sizeBytes"].as_i64().unwrap()));
        assert!((0..=1_000_000).contains(&item["seeders"].as_i64().unwrap()));
        if let Some(tags) = item.get("tags") {
            let tags = tags.as_array().unwrap();
            assert!(tags.len() <= 16);
            for tag in tags {
                let tag = tag.as_str().unwrap();
                assert!(!tag.trim().is_empty());
                assert!(tag.len() <= 64);
                assert!(!tag.contains(['\0', '\r', '\n']));
            }
        }
    }
}

#[test]
fn serialized_legacy_results_obey_utf8_display_limits() {
    let payload = json!({"inlist":[{
        "i":"fixture-id", "title":format!("{}\nfixture\0 title", "中".repeat(171)),
        "size":"4.6 GB", "seeds":18, "k":"类".repeat(22), "time":"4月前"
    }], "footer":{"next":false}});
    let response = parse_json_search(&serde_json::to_vec(&payload).unwrap(), 1).unwrap();
    let wire = search_wire(response);
    assert_eq!(wire["items"][0]["id"], "fixture-id");
    assert_eq!(wire["items"][0]["updatedAt"], "2025-05-14T08:00:00Z");
}

#[test]
fn legacy_numeric_overflow_never_wraps_or_poison_valid_neighbors() {
    let payload = json!({"inlist":[
        {"i":"bad-seeds", "title":"Bad", "size":"1 GB", "seeds":4_294_967_314_u64},
        {"i":"bad-size", "title":"Bad", "size":"8.1 TB", "seeds":18},
        {"i":"a".repeat(257), "title":"Bad", "size":"1 GB", "seeds":18},
        {"i":"valid", "title":"Valid", "size":"1 GB", "seeds":18}
    ], "footer":{"next":false}});
    let response = parse_json_search(&serde_json::to_vec(&payload).unwrap(), 1).unwrap();
    let wire = search_wire(response);
    assert_eq!(wire["items"].as_array().unwrap().len(), 1);
    assert_eq!(wire["items"][0]["id"], "valid");
}

#[test]
fn reads_only_leading_json_search_assignments_without_running_javascript() {
    let body = br#"<div id="app"></div><script>_obj.header={"n":1,"u":{"u":"fixture"}}; _obj.search={"inlist":[{"i":"fixture-id","title":"Fixture", "size":"1 GB", "seeds":3}],"footer":{"next":false}};_obj.footer={"version":"fixture"};throw new Error('must not execute');</script>"#;
    let wire = search_wire(parse_html_search(body, 1).unwrap());
    assert_eq!(wire["items"][0]["id"], "fixture-id");
    assert_eq!(wire["hasNext"], false);
    let with_unrelated_mention = [
        body.as_slice(),
        br#"<script>const fixture='_obj.search is data';</script>"#,
    ]
    .concat();
    assert!(parse_html_search(&with_unrelated_mention, 1).is_some());
}

#[test]
fn rejects_unknown_inert_malformed_and_conflicting_search_bootstrap() {
    let payload = r#"{"inlist":[],"footer":{"next":false}}"#;
    for source in [
        format!("<script>const example='_obj.search={payload};';</script>"),
        format!("<script>// _obj.search={payload};</script>"),
        format!("<script>if (false) {{ _obj.search={payload}; }}</script>"),
        format!("<script src='/fixture.js'>_obj.search={payload};</script>"),
        format!("<script type='application/json'>_obj.search={payload};</script>"),
        format!("<template><script>_obj.search={payload};</script></template>"),
        format!("<noscript><script>_obj.search={payload};</script></noscript>"),
        format!("<script>_obj.search={payload} + calculate();</script>"),
        format!("<script>_obj.search={payload};_obj.search={payload};</script>"),
        format!("<script>_obj.search={payload};</script><script>_obj.search={payload};</script>"),
        "<script>_obj.search={broken};</script>".to_owned(),
        "<script>_obj.header={\"n\":1};renderUnknown();</script><div id='app'></div>".to_owned(),
    ] {
        assert!(
            parse_html_search(source.as_bytes(), 1).is_none(),
            "source={source}"
        );
    }
}

#[test]
fn recognized_empty_rows_succeed_but_nonempty_invalid_rows_do_not() {
    let empty =
        search_wire(parse_json_search(br#"{"inlist":[],"footer":{"next":false}}"#, 2).unwrap());
    assert_eq!(empty, json!({"items":[],"page":2,"hasNext":false}));
    for row in [
        json!({"i":"", "title":"Fixture"}),
        json!({"i":"bad/id", "title":"Fixture"}),
        json!({"i":"fixture", "title":" \n\0 "}),
        json!({"i":"fixture", "title":"Fixture", "seeds":1_000_001}),
    ] {
        let payload = json!({"inlist":[row]});
        assert!(parse_json_search(&serde_json::to_vec(&payload).unwrap(), 1).is_none());
    }
}

#[test]
fn display_byte_boundaries_keep_whole_utf8_characters_and_exact_identity() {
    for (title, expected) in [
        ("中".repeat(170), "中".repeat(170)),
        ("中".repeat(171), "中".repeat(170)),
        ("😀".repeat(128), "😀".repeat(128)),
        ("😀".repeat(129), "😀".repeat(128)),
        (
            "  例子\n标题\0\r额外  ".to_owned(),
            "例子 标题 额外".to_owned(),
        ),
    ] {
        let payload = json!({"inlist":[{"i":u64::MAX, "title":title,"k":"类".repeat(22)}]});
        let wire =
            search_wire(parse_json_search(&serde_json::to_vec(&payload).unwrap(), 1).unwrap());
        assert_eq!(wire["items"][0]["id"], u64::MAX.to_string());
        assert_eq!(wire["items"][0]["title"], expected);
        assert_eq!(wire["items"][0]["tags"][0], "类".repeat(21));
    }
}

#[test]
fn checked_numeric_boundaries_and_all_invalid_pages() {
    let payload =
        json!({"inlist":[{"i":"boundary", "title":"Fixture", "size":"8 TB", "seeds":1_000_000}]});
    let wire = search_wire(parse_json_search(&serde_json::to_vec(&payload).unwrap(), 1).unwrap());
    assert_eq!(wire["items"][0]["sizeBytes"], 8_i64 << 40);
    assert_eq!(wire["items"][0]["seeders"], 1_000_000);
    for seeds in [
        json!(-1),
        json!(1_000_001),
        json!(i64::MAX),
        json!(u64::MAX),
        json!(1.5),
        json!("+1"),
    ] {
        let payload = json!({"inlist":[{"i":"fixture", "title":"Fixture", "seeds":seeds}]});
        assert!(parse_json_search(&serde_json::to_vec(&payload).unwrap(), 1).is_none());
    }
    for size in [
        json!(-1),
        json!(u64::MAX),
        json!("-1 GB"),
        json!("8.1 TB"),
        json!("1e999 TB"),
        json!("NaN GB"),
        json!({}),
    ] {
        let payload = json!({"inlist":[{"i":"fixture", "title":"Fixture", "size":size}]});
        assert!(parse_json_search(&serde_json::to_vec(&payload).unwrap(), 1).is_none());
    }
}

#[test]
fn legacy_row_wrapper_and_table_fixture_serialize_equivalent_items() {
    let rows = json!([
        {"i":"987654", "title":"示例资源 S01E01 2160p", "size":"4.6 GB", "seeds":18, "time":"4月前"},
        {"i":"abc_123", "title":"另一个资源", "size":"700 MB", "seeds":"1,205", "time":"2026-09-10 07:08:09"},
        {"i":"relative_12_min", "title":"分钟相对时间资源", "size":"1.5 GB", "seeds":5, "time":"12分钟前"},
        {"i":"yesterday", "title":"昨天相对时间资源", "size":"2 GB", "seeds":6, "time":"昨天"},
        {"i":"day_before_yesterday", "title":"前天相对时间资源", "size":"3 GB", "seeds":7, "time":"前天"}
    ]);
    let expected =
        search_wire(parse_html_search(include_bytes!("fixtures/search.html"), 1).unwrap());
    for payload in [
        json!({"inlist":rows,"footer":{"next":true}}),
        json!({"data":{"inlist":rows},"footer":{"next":true}}),
    ] {
        let wire =
            search_wire(parse_json_search(&serde_json::to_vec(&payload).unwrap(), 1).unwrap());
        assert_eq!(wire, expected);
    }
}
