//! MCP JSON-RPC 계층 — 전송(hyper)과 앱 상태를 모르는 순수 함수 모음.
//!
//! 두 프로토콜 리비전을 함께 지원한다. modern(`2026-07-28`) 클라이언트는
//! `server/discover`로 서버 세대를 프로브한 뒤 요청마다 `_meta` 봉투에 버전을 싣고,
//! legacy(`2025-11-25`) 클라이언트는 `initialize` 핸드셰이크로 시작한다. 프로브에
//! 답하지 못하면 클라이언트가 legacy로 폴백하므로 두 경로가 모두 열려 있어야 한다.
//!
//! **결과 계약의 출처**: 이 모듈이 지키는 결과 형태(필수 `resultType`, `tools/list`의
//! `ttlMs`·`cacheScope`)는 공개 스펙 문서가 아니라 설치된 클라이언트 바이너리에서 읽은
//! 검증기·스키마로 확정했다 — 그 응답을 받는 구현이 무엇을 요구하는지가 유일한 판정 기준이기
//! 때문이다. **읽은 판본은 `@anthropic-ai/claude-code` 2.1.278**이다. 출처가 그 판본에 묶여
//! 있으므로, 클라이언트를 올린 뒤에는 등록된 세션에서
//! `tools/list`와 실패 호출을 한 번씩 실제로 관측한다(§검증 B23) — 단위 테스트는 우리가 적은
//! 계약만 지키고 그 계약이 여전히 맞는지는 보지 않는다.

use serde::Deserialize;
use serde_json::{Value, json};

/// 최신 지원 리비전. `server/discover`를 요구하며 요청마다 `_meta` 봉투를 쓴다.
pub const PROTOCOL_MODERN: &str = "2026-07-28";
/// `initialize` 핸드셰이크 기반의 이전 리비전.
pub const PROTOCOL_LEGACY: &str = "2025-11-25";
/// 협상 대상 버전. 최신이 앞에 온다 (미지원 버전 에러의 `data.supported` 순서와 공유).
pub const SUPPORTED_PROTOCOL_VERSIONS: [&str; 2] = [PROTOCOL_MODERN, PROTOCOL_LEGACY];

/// MCP 클라이언트에 노출되는 서버 이름.
pub const SERVER_NAME: &str = "run-config-manager";

/// modern 리비전이 요청 `params._meta`에 버전을 싣는 키.
const META_PROTOCOL_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
/// `server/discover` 응답이 서버 신원을 싣는 `_meta` 키.
const META_SERVER_INFO: &str = "io.modelcontextprotocol/serverInfo";

// JSON-RPC 2.0 표준 에러 코드.
pub const PARSE_ERROR: i32 = -32700;
pub const INVALID_REQUEST: i32 = -32600;
pub const METHOD_NOT_FOUND: i32 = -32601;
pub const INVALID_PARAMS: i32 = -32602;
pub const INTERNAL_ERROR: i32 = -32603;
/// MCP 고유 코드: 클라이언트가 요구한 리비전을 서버가 지원하지 않음.
pub const UNSUPPORTED_PROTOCOL_VERSION: i32 = -32022;

/// JSON-RPC 에러 본문.
#[derive(Debug, Clone, PartialEq)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    pub data: Option<Value>,
}

impl JsonRpcError {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    pub fn with_data(code: i32, message: impl Into<String>, data: Value) -> Self {
        Self {
            code,
            message: message.into(),
            data: Some(data),
        }
    }
}

/// 파싱된 JSON-RPC 요청. `id`가 없으면 notification(응답 금지)이다.
#[derive(Debug, Clone)]
pub struct Request {
    pub id: Option<Value>,
    pub method: String,
    pub params: Value,
}

impl Request {
    /// notification 여부. `id`가 없거나 `null`이면 응답을 보내지 않는다.
    pub fn is_notification(&self) -> bool {
        matches!(self.id, None | Some(Value::Null))
    }

    /// `params.arguments` — `tools/call`의 인자 객체. 없으면 빈 객체.
    pub fn arguments(&self) -> Value {
        self.params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}))
    }

    /// `params.name` — `tools/call`의 대상 툴 이름.
    pub fn tool_name(&self) -> Option<&str> {
        self.params.get("name").and_then(Value::as_str)
    }
}

/// 요청 봉투만 검증하는 최소 스키마. `method` 외의 필드는 관용적으로 받는다.
#[derive(Deserialize)]
struct RawRequest {
    #[serde(default)]
    jsonrpc: String,
    #[serde(default)]
    id: Value,
    #[serde(default)]
    method: String,
    #[serde(default)]
    params: Value,
}

/// 요청 본문을 파싱한다. 배치 요청(최상위 배열)은 지원하지 않는다 — MCP 최신
/// 리비전에서 제거되었고, 우리 툴은 모두 단발 호출이다.
pub fn parse_request(body: &[u8]) -> Result<Request, JsonRpcError> {
    let raw: RawRequest = serde_json::from_slice(body)
        .map_err(|e| JsonRpcError::new(PARSE_ERROR, format!("Parse error: {e}")))?;

    if raw.jsonrpc != "2.0" {
        return Err(JsonRpcError::new(
            INVALID_REQUEST,
            "Invalid Request: jsonrpc must be \"2.0\"",
        ));
    }
    if raw.method.is_empty() {
        return Err(JsonRpcError::new(
            INVALID_REQUEST,
            "Invalid Request: missing method",
        ));
    }

    Ok(Request {
        id: match raw.id {
            Value::Null => None,
            id => Some(id),
        },
        method: raw.method,
        params: raw.params,
    })
}

/// 버전 협상 결과.
#[derive(Debug, Clone, PartialEq)]
pub enum VersionCheck {
    /// 협상 성공 — 이 요청에 적용되는 리비전.
    Ok(&'static str),
    /// `MCP-Protocol-Version` 헤더와 본문 `_meta` 버전이 불일치 (HTTP 400).
    Mismatch { header: String, body: String },
    /// 지원하지 않는 리비전 요구 (JSON-RPC `-32022`).
    Unsupported(String),
}

/// 요청에 적용할 리비전을 판정한다.
///
/// 헤더와 본문 `_meta`가 모두 있으면 값이 같아야 한다(스펙 요구). 어느 쪽도 버전을
/// 싣지 않으면 legacy 클라이언트로 간주한다 — modern 리비전은 `_meta` 봉투를
/// 필수로 요구하므로, 봉투가 없다는 것 자체가 legacy의 증거다.
pub fn check_protocol_version(header: Option<&str>, params: &Value) -> VersionCheck {
    let body = params
        .get("_meta")
        .and_then(|meta| meta.get(META_PROTOCOL_VERSION))
        .and_then(Value::as_str);

    let requested = match (header, body) {
        (Some(h), Some(b)) if h != b => {
            return VersionCheck::Mismatch {
                header: h.to_string(),
                body: b.to_string(),
            };
        }
        (Some(v), _) | (None, Some(v)) => v,
        (None, None) => return VersionCheck::Ok(PROTOCOL_LEGACY),
    };

    match supported_version(requested) {
        Some(v) => VersionCheck::Ok(v),
        None => VersionCheck::Unsupported(requested.to_string()),
    }
}

/// 요청 버전이 지원 집합에 있으면 그 정적 문자열을 돌려준다.
fn supported_version(requested: &str) -> Option<&'static str> {
    SUPPORTED_PROTOCOL_VERSIONS
        .into_iter()
        .find(|v| *v == requested)
}

/// 미지원 리비전 에러. 스펙이 요구하는 `data.supported`를 함께 싣는다.
pub fn unsupported_version_error(requested: &str) -> JsonRpcError {
    JsonRpcError::with_data(
        UNSUPPORTED_PROTOCOL_VERSION,
        "Unsupported protocol version",
        json!({
            "supported": SUPPORTED_PROTOCOL_VERSIONS,
            "requested": requested,
        }),
    )
}

/// `server/discover` 결과 — modern 클라이언트가 서버 세대를 판정하는 프로브 응답.
///
/// 여기 실리는 것(지원 리비전 · capability · 서버 버전 · 설명)은 빌드마다 고정이라 캐시해도
/// 되지만, 앱이 다른 버전으로 재시작한 직후를 짧게 잡기 위해 `ttlMs`를 1분으로 둔다.
/// 값이 호출자와 무관하게 바이너리에만 달려 있으므로 `cacheScope`는 `public`이다.
pub fn discover_result(server_version: &str) -> Value {
    json!({
        "supportedVersions": SUPPORTED_PROTOCOL_VERSIONS,
        "capabilities": { "tools": { "listChanged": false } },
        "_meta": {
            META_SERVER_INFO: { "name": SERVER_NAME, "version": server_version },
        },
        "instructions": INSTRUCTIONS,
        "ttlMs": 60_000,
        "cacheScope": "public",
    })
}

/// legacy `initialize` 결과.
///
/// 클라이언트가 요구한 버전이 지원 집합에 있으면 그대로 되돌려주고, 없으면 우리가
/// 지원하는 legacy 버전을 제시한다(스펙: 서버는 자신이 지원하는 버전으로 답한다).
/// `initialize` 자체가 legacy 경로이므로 modern 버전을 제시하지 않는다.
pub fn initialize_result(requested: Option<&str>, server_version: &str) -> Value {
    let negotiated = requested
        .and_then(supported_version)
        .unwrap_or(PROTOCOL_LEGACY);

    json!({
        "protocolVersion": negotiated,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": SERVER_NAME, "version": server_version },
        "instructions": INSTRUCTIONS,
    })
}

/// 두 핸드셰이크가 공유하는 서버 설명. 에이전트가 이 서버를 언제 쓸지 판단하는 근거다.
const INSTRUCTIONS: &str = "Run/Debug configurations and their live terminal sessions in the \
    RunConfigManager desktop app. Use it to inspect saved run configurations and to read the output \
    of running or finished sessions. The exposed tool set depends on the permission tier the user \
    picked in the app's settings, so treat a missing tool as \"not permitted\" rather than \
    \"not supported\".";

/// 성공 응답 봉투. 모든 결과에 `resultType: "complete"`를 싣는다.
///
/// 리비전 `2026-07-28` 클라이언트는 메서드와 무관하게 결과마다 이 필드를 요구하고 없으면
/// 응답을 버린다. `2025-11-25` 클라이언트는 결과 스키마 검증을 아예 하지 않고 이 필드가 있으면
/// 지운 뒤 통과시킨다. 그래서 무조건 싣는 것이 두 리비전 모두에 맞고 협상된 버전을 이 계층까지
/// 끌고 올 필요도 없다. 메서드별 결과 빌더가 아니라 봉투가 챙기는 이유는 모든 성공 응답이
/// 여기를 지나는 것이다 — 새 메서드가 빠뜨릴 수 없다.
pub fn success(id: Option<Value>, mut result: Value) -> Value {
    // modern 클라이언트는 `resultType`을 보기 **전에** 결과가 객체인지 보고 아니면 버린다.
    // JSON-RPC가 허용하는 비객체 결과를 이 프로토콜에서는 쓸 수 없으므로, 아래 `if let`이
    // 조용히 지나가는 모양을 테스트에서 세운다. 릴리스 빌드에서는 컴파일되지 않으므로 이 보증의
    // 범위는 디버그 빌드와 테스트 스위트다 — 실제 클라이언트 경로는 §검증 B23이 본다.
    // 값을 문구에 싣지 않는다 — 결과 payload에는 노출 설정에 따라 환경변수 값이 들어올 수 있고
    // 이 단정은 디버그 빌드에서 그 문구를 stderr로 내보낸다.
    debug_assert!(result.is_object(), "MCP 결과는 JSON 객체여야 한다");
    if let Some(object) = result.as_object_mut() {
        object.insert("resultType".to_owned(), json!("complete"));
    }
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// 에러 응답 봉투. `data`가 없으면 키를 넣지 않는다.
pub fn failure(id: Option<Value>, error: &JsonRpcError) -> Value {
    let mut body = json!({ "code": error.code, "message": error.message });
    if let Some(data) = &error.data {
        body["data"] = data.clone();
    }
    json!({ "jsonrpc": "2.0", "id": id, "error": body })
}

/// `tools/list` 결과.
///
/// `ttlMs`(정수 ≥ 0)와 `cacheScope`는 이 메서드를 묶는 **두 리비전의 결과 스키마 모두**가 필수로
/// 요구하며, `server/discover`와 달리 기본값 폴백이 없어 빠지면 결과가 검증에서 거절된다.
/// 그래서 리비전에 따라 갈라 싣지 않는다.
///
/// `ttlMs: 0`인 이유: 노출되는 목록은 사용자가 settings에서 바꾸는 권한 단계를 따르는데
/// 우리 capability는 `listChanged: false`여서 변경을 알릴 채널이 없다. 클라이언트는 쓸 때
/// `expiresAt = now + ttlMs`를 기록하고 읽을 때 `expiresAt > now`인 항목만 서브하므로, 0은 그
/// 항목이 한 번도 서브되지 않는다는 뜻이다. 목록이 호출자가 아니라 앱 설정 하나에만 달려 있으므로
/// `cacheScope`는 `public`이다.
pub fn tools_list_result(tools: Vec<Value>) -> Value {
    json!({ "tools": tools, "ttlMs": 0, "cacheScope": "public" })
}

/// `tools/call` 성공 결과. 구조화 payload를 pretty JSON 텍스트 블록으로 싣는다.
///
/// `structuredContent`를 쓰지 않는 이유: 스펙이 툴마다 `outputSchema` 선언을 요구해
/// 스키마와 실제 payload가 갈라질 여지가 생긴다. 텍스트 블록은 전 리비전·전 클라이언트가
/// 동일하게 읽는다.
pub fn tool_result(payload: &Value) -> Value {
    let text = serde_json::to_string_pretty(payload)
        .unwrap_or_else(|e| format!("{{\"error\":\"serialization failed: {e}\"}}"));
    json!({ "content": [{ "type": "text", "text": text }] })
}

/// `tools/call` 실행 실패 결과. 프로토콜 위반이 아닌 "툴이 일을 못 했다"는 상황이므로
/// JSON-RPC 에러가 아니라 `isError` 결과로 돌려준다 (스펙 구분).
pub fn tool_error(message: &str) -> Value {
    json!({
        "content": [{ "type": "text", "text": message }],
        "isError": true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(json: &str) -> Vec<u8> {
        json.as_bytes().to_vec()
    }

    #[test]
    fn parse_request_reads_method_and_params() {
        let req = parse_request(&body(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_sessions"}}"#,
        ))
        .expect("valid request");

        assert_eq!(req.method, "tools/call");
        assert_eq!(req.tool_name(), Some("list_sessions"));
        assert!(!req.is_notification());
    }

    #[test]
    fn parse_request_rejects_malformed_json() {
        let err = parse_request(&body("{not json")).expect_err("must fail");
        assert_eq!(err.code, PARSE_ERROR);
    }

    #[test]
    fn parse_request_rejects_wrong_jsonrpc_version() {
        let err = parse_request(&body(r#"{"jsonrpc":"1.0","id":1,"method":"ping"}"#))
            .expect_err("must fail");
        assert_eq!(err.code, INVALID_REQUEST);
    }

    #[test]
    fn parse_request_rejects_missing_method() {
        let err = parse_request(&body(r#"{"jsonrpc":"2.0","id":1}"#)).expect_err("must fail");
        assert_eq!(err.code, INVALID_REQUEST);
    }

    #[test]
    fn parse_request_treats_absent_and_null_id_as_notification() {
        let absent = parse_request(&body(
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        ))
        .expect("valid");
        let null = parse_request(&body(
            r#"{"jsonrpc":"2.0","id":null,"method":"notifications/initialized"}"#,
        ))
        .expect("valid");

        assert!(absent.is_notification());
        assert!(null.is_notification());
    }

    #[test]
    fn arguments_defaults_to_empty_object() {
        let req = parse_request(&body(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"x"}}"#,
        ))
        .expect("valid");
        assert_eq!(req.arguments(), json!({}));
    }

    #[test]
    fn version_check_without_any_version_falls_back_to_legacy() {
        assert_eq!(
            check_protocol_version(None, &json!({})),
            VersionCheck::Ok(PROTOCOL_LEGACY)
        );
    }

    #[test]
    fn version_check_accepts_modern_meta_envelope() {
        let params = json!({ "_meta": { META_PROTOCOL_VERSION: PROTOCOL_MODERN } });
        assert_eq!(
            check_protocol_version(Some(PROTOCOL_MODERN), &params),
            VersionCheck::Ok(PROTOCOL_MODERN)
        );
    }

    #[test]
    fn version_check_accepts_header_only() {
        assert_eq!(
            check_protocol_version(Some(PROTOCOL_LEGACY), &json!({})),
            VersionCheck::Ok(PROTOCOL_LEGACY)
        );
    }

    #[test]
    fn version_check_flags_header_body_mismatch() {
        let params = json!({ "_meta": { META_PROTOCOL_VERSION: PROTOCOL_LEGACY } });
        assert_eq!(
            check_protocol_version(Some(PROTOCOL_MODERN), &params),
            VersionCheck::Mismatch {
                header: PROTOCOL_MODERN.to_string(),
                body: PROTOCOL_LEGACY.to_string(),
            }
        );
    }

    #[test]
    fn version_check_rejects_unknown_version() {
        assert_eq!(
            check_protocol_version(Some("1900-01-01"), &json!({})),
            VersionCheck::Unsupported("1900-01-01".to_string())
        );
    }

    #[test]
    fn unsupported_version_error_lists_supported_versions() {
        let err = unsupported_version_error("1900-01-01");
        assert_eq!(err.code, UNSUPPORTED_PROTOCOL_VERSION);
        let data = err.data.expect("data present");
        assert_eq!(data["requested"], "1900-01-01");
        assert_eq!(data["supported"][0], PROTOCOL_MODERN);
        assert_eq!(data["supported"][1], PROTOCOL_LEGACY);
    }

    #[test]
    fn initialize_echoes_supported_request_version() {
        let result = initialize_result(Some(PROTOCOL_MODERN), "1.2.3");
        assert_eq!(result["protocolVersion"], PROTOCOL_MODERN);
        assert_eq!(result["serverInfo"]["version"], "1.2.3");
        assert_eq!(result["serverInfo"]["name"], SERVER_NAME);
    }

    #[test]
    fn initialize_offers_legacy_version_for_unknown_request() {
        // 스펙: 서버는 자신이 지원하는 버전을 제시한다. initialize는 legacy 경로이므로
        // modern을 제시하지 않는다.
        let result = initialize_result(Some("2024-11-05"), "1.2.3");
        assert_eq!(result["protocolVersion"], PROTOCOL_LEGACY);
    }

    #[test]
    fn initialize_without_requested_version_offers_legacy() {
        let result = initialize_result(None, "1.2.3");
        assert_eq!(result["protocolVersion"], PROTOCOL_LEGACY);
    }

    #[test]
    fn discover_advertises_both_revisions_and_tools() {
        let result = discover_result("1.2.3");
        assert_eq!(result["supportedVersions"][0], PROTOCOL_MODERN);
        assert_eq!(result["supportedVersions"][1], PROTOCOL_LEGACY);
        assert!(result["capabilities"]["tools"].is_object());
        assert_eq!(result["_meta"][META_SERVER_INFO]["version"], "1.2.3");
    }

    /// modern 리비전(`2026-07-28`) 클라이언트는 결과마다 `resultType`을 요구하므로
    /// 봉투가 그것을 싣는다 — 메서드별 결과 빌더가 각자 챙기는 형태로는 다음 메서드가 빠뜨린다.
    #[test]
    fn a_success_envelope_declares_the_complete_result_type() {
        let envelope = success(Some(json!(1)), json!({ "tools": [] }));

        assert_eq!(envelope["result"]["resultType"], "complete");
        assert_eq!(envelope["result"]["tools"], json!([]));
    }

    #[test]
    fn failure_omits_data_when_absent() {
        let body = failure(Some(json!(7)), &JsonRpcError::new(METHOD_NOT_FOUND, "nope"));
        assert_eq!(body["id"], 7);
        assert_eq!(body["error"]["code"], METHOD_NOT_FOUND);
        assert!(body["error"].get("data").is_none());
    }

    #[test]
    fn failure_includes_data_when_present() {
        let err = unsupported_version_error("x");
        let body = failure(Some(json!(1)), &err);
        assert!(body["error"]["data"]["supported"].is_array());
    }

    /// 비객체 결과는 `resultType`을 실을 자리가 없고 클라이언트도 그것을 먼저 거절한다. 봉투가
    /// 조용히 지나가는 대신 여기서 세우는 것이 새 메서드에 대한 유일한 구조적 가드다.
    ///
    /// `debug_assertions`가 꺼진 프로파일에서는 그 단정이 컴파일되지 않아 `should_panic`이
    /// 거짓으로 깨진다 — 지금 CI는 debug로만 테스트하지만 프로파일 하나가 그 전제를 바꾼다.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "JSON 객체여야 한다")]
    fn a_non_object_result_is_a_bug_not_a_silent_pass() {
        success(Some(json!(1)), json!("done"));
    }

    #[test]
    fn the_tool_list_result_carries_the_cache_fields() {
        let result = tools_list_result(vec![json!({ "name": "list_sessions" })]);

        assert_eq!(result["tools"][0]["name"], "list_sessions");
        assert_eq!(result["ttlMs"], 0);
        assert_eq!(result["cacheScope"], "public");
    }

    #[test]
    fn tool_result_carries_payload_as_json_text() {
        let result = tool_result(&json!({ "sessions": [] }));
        let text = result["content"][0]["text"].as_str().expect("text block");
        let parsed: Value = serde_json::from_str(text).expect("payload is valid json");
        assert!(parsed["sessions"].is_array());
        assert!(result.get("isError").is_none());
    }

    #[test]
    fn tool_error_marks_is_error() {
        let result = tool_error("session not found");
        assert_eq!(result["isError"], true);
        assert_eq!(result["content"][0]["text"], "session not found");
    }
}
