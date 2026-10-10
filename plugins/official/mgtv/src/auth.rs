use super::*;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Start {
    connection_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Poll {
    connection_id: String,
    login_session: String,
}

fn params(connection: &str) -> Vec<(&str, &str)> {
    vec![
        ("_support", "10000000"),
        ("deviceid", connection),
        ("appVersion", "pcweb-9.0.8"),
        ("dname", ""),
        ("src", "mgtv"),
        ("invoker", "pcweb"),
        ("noBinding", "0"),
    ]
}
pub(super) fn start(request: Start) -> Result<Value, Error> {
    validate_connection(&request.connection_id)?;
    let data = get(
        &request.connection_id,
        &query(
            "https://nuc.api.mgtv.com/GetMultiPic",
            &params(&request.connection_id),
        )?,
        false,
    )?;
    let (rcode, url) = parse_generate(&data)?;
    let at = now()?;
    let expires = rfc3339(at + time::Duration::seconds(180))?;
    let session = format!("qr:{}:{}", at.unix_timestamp_nanos(), request.connection_id);
    storage_set(
        &request.connection_id,
        "auth/qr",
        &json!({"session":session,"rcode":rcode,"expiresAt":expires,"lastPoll":0}),
    )?;
    Ok(json!({"loginSession":session,"qrCodeUrl":url,"expiresAt":expires,"pollAfterSeconds":3}))
}
fn parse_generate(data: &Value) -> Result<(String, String), Error> {
    let rcode = string(data, "rcode")?;
    if rcode.len() > 256
        || !rcode
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_=+".contains(&c))
    {
        return Err(Error::response());
    }
    let url = safe_url(string(data, "url")?).ok_or_else(Error::response)?;
    if Url::parse(&url)
        .ok()
        .and_then(|v| v.host_str().map(str::to_owned))
        .as_deref()
        != Some("oauth.mgtv.com")
    {
        return Err(Error::response());
    }
    Ok((rcode.to_owned(), url))
}
pub(super) fn poll(request: Poll) -> Result<Value, Error> {
    validate_connection(&request.connection_id)?;
    let mut state = storage_get(&request.connection_id, "auth/qr")?.ok_or_else(Error::request)?;
    if state.get("session").and_then(Value::as_str) != Some(request.login_session.as_str()) {
        return Err(Error::request());
    }
    let expires = OffsetDateTime::parse(string(&state, "expiresAt")?, &Rfc3339)
        .map_err(|_| Error::response())?;
    let at = now()?;
    if at >= expires {
        return Ok(json!({"state":"expired","authenticated":false}));
    }
    if number(state.get("lastPoll"))
        .is_some_and(|previous| at.unix_timestamp() < previous as i64 + 2)
    {
        return Err(Error::new("rate-limited", "扫码轮询过于频繁"));
    }
    state["lastPoll"] = json!(at.unix_timestamp());
    storage_set(&request.connection_id, "auth/qr", &state)?;
    let rcode = string(&state, "rcode")?;
    let time = (at.unix_timestamp_nanos() / 1_000_000).to_string();
    let mut query_params = params(&request.connection_id);
    query_params.extend([("rcode", rcode), ("t", time.as_str())]);
    let payload = decode_http(&http(
        &request.connection_id,
        &query("https://nuc.api.mgtv.com/GetQrcodeResult", &query_params)?,
        false,
    )?)?;
    match poll_code(&payload)? {
        PollState::Pending => {
            Ok(json!({"state":"pending","authenticated":false,"pollAfterSeconds":3}))
        }
        PollState::Expired => Ok(json!({"state":"expired","authenticated":false})),
        PollState::Confirmed => {
            let data = payload.get("data").ok_or_else(Error::response)?;
            if data
                .get("needRelate")
                .is_some_and(|v| v == true || number(Some(v)) == Some(1))
            {
                return Err(Error::new(
                    "browser-verification-required",
                    "请先在芒果官方客户端完成账号手机号绑定后重新扫码",
                ));
            }
            let url = cookie_url(data)?;
            let response = host(
                1,
                &json!({"connectionId":request.connection_id,"method":"GET","url":url,
                "headers":{"Accept":"application/json","Referer":"https://www.mgtv.com/","User-Agent":USER_AGENT},
                "captureCredentialScope":SESSION_SCOPE,"timeoutMs":15000}),
            )?;
            let cookie_payload = decode_http(&response)?;
            provider_data(&cookie_payload)?;
            let capture = string(&response, "credentialCaptureRef")?;
            let committed = host(
                8,
                &json!({"connectionId":request.connection_id,"scope":SESSION_SCOPE,"captureRef":capture}),
            )?;
            let credential_version = committed_version(&committed)?;
            let account = account(&request.connection_id)?;
            storage_set(
                &request.connection_id,
                "auth/qr",
                &json!({"completed":true}),
            )?;
            Ok(
                json!({"state":"confirmed","authenticated":true,"credentialVersion":credential_version,"account":account}),
            )
        }
    }
}
fn committed_version(committed: &Value) -> Result<u64, Error> {
    if committed.get("credentialUpdated").and_then(Value::as_bool) != Some(true) {
        return Err(Error::new("internal", "芒果登录凭据保存失败"));
    }
    committed
        .get("credentialVersion")
        .and_then(Value::as_u64)
        .filter(|v| *v > 0)
        .ok_or_else(|| Error::new("internal", "芒果登录凭据版本无效"))
}
#[derive(Debug, PartialEq)]
enum PollState {
    Pending,
    Expired,
    Confirmed,
}
fn poll_code(payload: &Value) -> Result<PollState, Error> {
    match number(payload.get("code")) {
        Some(203) => Ok(PollState::Pending),
        Some(26) => Ok(PollState::Expired),
        Some(200) if payload.get("data").is_some_and(Value::is_object) => Ok(PollState::Confirmed),
        _ => Err(Error::new("upstream-unavailable", "芒果扫码状态暂不可用")),
    }
}
fn cookie_url(data: &Value) -> Result<String, Error> {
    // The provider's temporary ticket is used once in memory. Only Host captures
    // the desktop Set-Cookie; neither this payload nor ticket enters storage.
    let uid = string(data, "uuid")?;
    validate_connection(uid)?;
    let ticket = string(data, "ticket")?;
    let avatar = data
        .get("avatar")
        .and_then(Value::as_str)
        .and_then(|text| serde_json::from_str::<Value>(text).ok())
        .and_then(|v| v.get("l").and_then(Value::as_str).map(str::to_owned))
        .unwrap_or_default();
    let mut fields = vec![
        ("ticket", ticket),
        ("uid", uid),
        ("avatar", avatar.as_str()),
    ];
    let mapping = [
        ("nickname", "nickname"),
        ("phone", "mobile"),
        ("relate_mobile", "relate_mobile"),
        ("loginAccount", "loginaccount"),
        ("email", "email"),
        ("rtype", "rtype"),
        ("logintype", "logintype"),
        ("shortnickname", "nickname"),
        ("isband", "isband"),
        ("iscert", "iscert"),
        ("isValidated", "isValidated"),
    ];
    let strings = mapping
        .iter()
        .map(|(_, key)| match data.get(*key) {
            Some(Value::String(v)) => v.clone(),
            Some(Value::Number(v)) => v.to_string(),
            _ => String::new(),
        })
        .collect::<Vec<_>>();
    for ((name, _), value) in mapping.iter().zip(&strings) {
        fields.push((name, value));
    }
    query("https://i.mgtv.com/account/setCookie", &fields)
}
fn account(connection: &str) -> Result<Value, Error> {
    let data = get(
        connection,
        "https://nuc.api.mgtv.com/user/get_login_user?needVipIcon=1",
        true,
    )?;
    account_value(&data)
}
pub(super) fn account_id(connection: &str) -> Result<String, Error> {
    let account = account(connection)?;
    Ok(string(&account, "id")?.to_owned())
}
fn account_value(data: &Value) -> Result<Value, Error> {
    let id = data
        .get("uid")
        .or_else(|| data.get("uuid"))
        .and_then(|v| {
            v.as_str()
                .map(str::to_owned)
                .or_else(|| v.as_u64().map(|v| v.to_string()))
        })
        .ok_or_else(Error::response)?;
    validate_connection(&id)?;
    let name = string(data, "nickname")?;
    if name.len() > 240 {
        return Err(Error::response());
    }
    let membership =
        match number(data.pointer("/vipinfo/type")).or_else(|| number(data.get("isvip"))) {
            Some(0) => json!({"status":"inactive","label":"普通账号"}),
            Some(_) => json!({"status":"active","label":"芒果会员（具体视频权益以实时鉴权为准）"}),
            None => json!({"status":"unknown","label":"会员状态待确认"}),
        };
    let mut result = json!({"id":id,"name":name,"membership":membership});
    if let Some(avatar) = data
        .get("avatar")
        .and_then(Value::as_str)
        .and_then(safe_url)
    {
        result["avatarUrl"] = json!(avatar);
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observed_complete_qr_response_keeps_reviewed_login_target() {
        let payload: Value =
            serde_json::from_str(include_str!("../fixtures/auth-generate.json")).unwrap();
        let (rcode, target) = parse_generate(provider_data(&payload).unwrap()).unwrap();
        assert_eq!(rcode.len(), 32);
        let target = Url::parse(&target).unwrap();
        assert_eq!(target.scheme(), "https");
        assert_eq!(target.host_str(), Some("oauth.mgtv.com"));
        assert_eq!(target.path(), "/2.0/multicode_login");
        assert_eq!(target.query_pairs().count(), 7);
    }
    #[test]
    fn real_pending_and_expired_codes_do_not_authenticate() {
        assert_eq!(
            poll_code(&json!({"code":203,"msg":"未扫码登录","seqId":""})).unwrap(),
            PollState::Pending
        );
        assert_eq!(
            poll_code(&json!({"code":26,"msg":"二维码失效","seqId":""})).unwrap(),
            PollState::Expired
        );
        assert!(poll_code(&json!({"code":200})).is_err());
    }
    #[test]
    fn qr_target_requires_reviewed_official_domain() {
        assert!(parse_generate(&json!({"rcode":"fixture-code","url":"https://oauth.mgtv.com/2.0/multicode_login?uid=fixture"})).is_ok());
        assert!(parse_generate(
            &json!({"rcode":"fixture-code","url":"https://evil.example/login"})
        )
        .is_err());
    }
    #[test]
    fn synthetic_account_contract_never_includes_ticket_or_mobile() {
        let value=account_value(&json!({"uuid":"12345","nickname":"管理员","ticket":"secret","mobile":"123456789","vipinfo":{"type":"1"}})).unwrap();
        assert_eq!(value.pointer("/membership/status"), Some(&json!("active")));
        assert!(!value.to_string().contains("secret"));
        assert!(!value.to_string().contains("123456789"));
        assert_eq!(
            account_value(&json!({"uuid":"12345","nickname":"管理员"}))
                .unwrap()
                .pointer("/membership/status"),
            Some(&json!("unknown"))
        );
    }
    #[test]
    fn confirmed_summary_requires_successful_host_credential_version() {
        assert_eq!(
            committed_version(&json!({"credentialUpdated":true,"credentialVersion":7})).unwrap(),
            7
        );
        assert!(committed_version(&json!({"credentialUpdated":true})).is_err());
        assert!(
            committed_version(&json!({"credentialUpdated":false,"credentialVersion":7})).is_err()
        );
        assert!(
            committed_version(&json!({"credentialUpdated":true,"credentialVersion":0})).is_err()
        );
    }
}
