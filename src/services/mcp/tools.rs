//! 툴 카탈로그와 인자 파싱 — 앱 상태를 모르는 순수 계층.
//!
//! 권한 단계(`McpPermission`)가 노출 여부를 결정한다. 허용되지 않은 툴은 `tools/list`에
//! 아예 나타나지 않으며, `tools/call`에서도 독립적으로 다시 검사한다 — 목록을 캐시한
//! 클라이언트가 낡은 이름으로 호출할 수 있기 때문이다.

use super::protocol::{INVALID_PARAMS, JsonRpcError, METHOD_NOT_FOUND};
use crate::models::compile_search_regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

/// `read_session_output`이 인자 없이 호출됐을 때 돌려주는 꼬리 라인 수.
/// 세션 버퍼는 기본 5만 라인까지 자라므로, 전체를 되돌리면 호출자의 컨텍스트를 잡아먹는다.
pub const DEFAULT_TAIL_LINES: usize = 200;
/// 한 번의 출력 조회로 되돌릴 수 있는 최대 라인 수 (요청이 더 크면 이 값으로 클램프).
pub const MAX_LINES_PER_CALL: usize = 5_000;
/// 한 번의 검색으로 되돌릴 수 있는 최대 매치 수.
pub const MAX_SEARCH_MATCHES: usize = 500;
/// 정규식 컴파일 에러에서 앞부분과 실패 사유에 각각 남길 최대 문자 수. `regex-syntax`는 잘못된
/// 패턴 전체를 되풀이한 뒤 offset만큼 공백을 채운 캐럿 라인을 붙이므로, 에러가 패턴 끝에 있으면
/// 메시지가 패턴 길이의 2배가 된다(실측: 200,001자 패턴 → `regex::Error` 400,052자, 여기에
/// 우리 프리픽스 41자). body 상한 1 MiB를 통과한 패턴 하나로 호출자 컨텍스트를 2 MB 채울 수 있다.
const MAX_REGEX_ERROR_CHARS: usize = 400;

/// MCP 클라이언트에 허용할 권한 단계. 사용자가 앱 설정에서 고르며 기본은 읽기 전용이다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum McpPermission {
    /// 구성·세션·출력 조회만 허용.
    #[default]
    ReadOnly,
    /// 조회 + 실행/중지/stdin 전송.
    Execute,
    /// 조회 + 실행 + 구성 생성/수정/삭제.
    Edit,
}

impl McpPermission {
    /// 설정 UI가 순서대로 노출하는 전체 단계.
    pub const ALL: [Self; 3] = [Self::ReadOnly, Self::Execute, Self::Edit];

    /// 포함 관계 비교용 서열. 상위 단계는 하위 단계의 모든 툴을 포함한다.
    fn level(self) -> u8 {
        match self {
            Self::ReadOnly => 0,
            Self::Execute => 1,
            Self::Edit => 2,
        }
    }

    /// 이 단계가 `required` 단계의 툴을 허용하는지.
    pub fn allows(self, required: Self) -> bool {
        self.level() >= required.level()
    }

    /// 설정 UI에 표시할 이름.
    pub fn label(self) -> &'static str {
        match self {
            Self::ReadOnly => "Read only",
            Self::Execute => "Execute",
            Self::Edit => "Edit",
        }
    }
}

/// 카탈로그의 툴 한 항목.
pub struct ToolSpec {
    pub name: &'static str,
    title: &'static str,
    description: &'static str,
    /// 이 툴을 노출하기 위해 필요한 최소 권한 단계.
    pub tier: McpPermission,
    /// 입력 스키마 생성자. `tools/list` 호출은 드물어 매번 만들어도 무해하다.
    schema: fn() -> Value,
}

impl ToolSpec {
    /// `tools/list` 항목 형태로 직렬화.
    fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "title": self.title,
            "description": self.description,
            "inputSchema": (self.schema)(),
        })
    }
}

/// 전체 툴 카탈로그. 노출은 `tools_for`가 권한으로 걸러낸다.
static TOOLS: &[ToolSpec] = &[
    ToolSpec {
        name: "list_configurations",
        title: "List run configurations",
        description: "List every saved run/debug configuration with its id, type and working \
                      directory. Environment variable names are included; values are masked unless \
                      the user enabled value exposure in the app settings.",
        tier: McpPermission::ReadOnly,
        schema: || json!({ "type": "object", "properties": {}, "additionalProperties": false }),
    },
    ToolSpec {
        name: "get_configuration",
        title: "Get a run configuration",
        description: "Full detail of one configuration, including the type-specific fields \
                      (command, script, node/kotlin settings, compound members).",
        tier: McpPermission::ReadOnly,
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "configuration": {
                        "type": "string",
                        "description": "Configuration id (UUID) or its exact name.",
                    },
                },
                "required": ["configuration"],
                "additionalProperties": false,
            })
        },
    },
    ToolSpec {
        name: "list_sessions",
        title: "List execution sessions",
        description: "List execution sessions with their status (running, succeeded, failed with \
                      exit code, stopped), start time, duration and buffered output line count.",
        tier: McpPermission::ReadOnly,
        schema: || json!({ "type": "object", "properties": {}, "additionalProperties": false }),
    },
    ToolSpec {
        name: "read_session_output",
        title: "Read session output",
        description: "Read a session's terminal output as plain text with ANSI styling removed. \
                      Returns the last 200 lines by default. Use since_line_id with the \
                      last_line_id from a previous call to stream only what was appended since.",
        tier: McpPermission::ReadOnly,
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Session id (UUID)." },
                    "tail_lines": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": MAX_LINES_PER_CALL,
                        "description": "How many trailing lines to return. Defaults to 200.",
                    },
                    "since_line_id": {
                        "type": "integer",
                        "minimum": 0,
                        "description": "Return only lines whose id is greater than this. Line ids \
                                        are stable across buffer eviction, but a line still being \
                                        rewritten in place (progress bars) keeps its id.",
                    },
                },
                "required": ["session_id"],
                "additionalProperties": false,
            })
        },
    },
    ToolSpec {
        name: "search_session_output",
        title: "Search session output",
        description: "Find matching lines in a session's output. Case-insensitive substring by \
                      default, or a case-insensitive regular expression when regex is true.",
        tier: McpPermission::ReadOnly,
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Session id (UUID)." },
                    "query": { "type": "string", "description": "Substring or regex to look for." },
                    "regex": {
                        "type": "boolean",
                        "description": "Treat query as a regular expression. Defaults to false.",
                    },
                },
                "required": ["session_id", "query"],
                "additionalProperties": false,
            })
        },
    },
];

/// 주어진 권한 단계에서 노출되는 툴 목록 (`tools/list` 결과).
pub fn tools_for(permission: McpPermission) -> Vec<Value> {
    TOOLS
        .iter()
        .filter(|tool| permission.allows(tool.tier))
        .map(ToolSpec::to_json)
        .collect()
}

/// 이름으로 툴 스펙 조회.
pub fn find(name: &str) -> Option<&'static ToolSpec> {
    TOOLS.iter().find(|tool| tool.name == name)
}

/// 브리지를 통해 앱 update 루프가 수행할 작업.
#[derive(Debug, Clone, PartialEq)]
pub enum McpOp {
    /// 권한 단계로 필터링된 툴 목록. 필터가 앱 상태(현재 권한)에 의존하므로 브리지를 탄다.
    ListTools,
    ListConfigurations,
    GetConfiguration {
        /// 구성 id(UUID) 또는 정확한 이름.
        target: String,
    },
    ListSessions,
    ReadSessionOutput {
        session_id: Uuid,
        tail_lines: usize,
        since_line_id: Option<usize>,
    },
    SearchSessionOutput {
        session_id: Uuid,
        query: String,
        regex: bool,
    },
}

impl McpOp {
    /// 이 작업을 발행한 카탈로그 툴의 이름. `ListTools`는 프로토콜 메서드이지 툴이 아니다.
    fn tool_name(&self) -> Option<&'static str> {
        match self {
            Self::ListTools => None,
            Self::ListConfigurations => Some("list_configurations"),
            Self::GetConfiguration { .. } => Some("get_configuration"),
            Self::ListSessions => Some("list_sessions"),
            Self::ReadSessionOutput { .. } => Some("read_session_output"),
            Self::SearchSessionOutput { .. } => Some("search_session_output"),
        }
    }

    /// 이 작업을 수행하기 위해 필요한 최소 권한 단계.
    ///
    /// tier의 출처는 카탈로그(`TOOLS`) 하나다. `tools/list` 필터와 `tools/call` 재검사가 같은
    /// 값을 읽으므로, 툴을 추가하면서 한쪽만 고쳐 "목록에서는 숨겼는데 호출은 통과하는" 상태가
    /// 생기지 않는다 — 그건 목록 캐시 공격을 막으려던 재검사가 서버 자신의 불일치로 뚫리는 것이다.
    pub fn required_tier(&self) -> McpPermission {
        // `tools/list` 자체는 어느 단계에서나 허용된다 — 노출되는 툴만 단계에 따라 달라진다.
        let Some(name) = self.tool_name() else {
            return McpPermission::ReadOnly;
        };
        // 카탈로그에 없는 이름은 도달 불가하지만(`parse_call`이 먼저 거르고 아래 테스트가
        // 매핑을 지킨다), 생긴다면 가장 높은 단계를 요구해 닫히는 쪽으로 실패한다.
        find(name).map_or(McpPermission::Edit, |tool| tool.tier)
    }
}

/// `tools/call`의 이름과 인자를 작업으로 변환한다.
///
/// 이름이 카탈로그에 없으면 `-32601`, 인자가 스키마에 맞지 않으면 `-32602`로 거부한다.
/// 권한 검사는 여기서 하지 않는다 — 현재 권한은 앱 상태이므로 브리지 뒤에서 판정한다.
pub fn parse_call(name: &str, args: &Value) -> Result<McpOp, JsonRpcError> {
    if find(name).is_none() {
        return Err(JsonRpcError::new(
            METHOD_NOT_FOUND,
            format!("Unknown tool: {name}"),
        ));
    }

    match name {
        "list_configurations" => Ok(McpOp::ListConfigurations),
        "get_configuration" => Ok(McpOp::GetConfiguration {
            target: required_str(args, "configuration")?.to_string(),
        }),
        "list_sessions" => Ok(McpOp::ListSessions),
        "read_session_output" => Ok(McpOp::ReadSessionOutput {
            session_id: required_uuid(args, "session_id")?,
            tail_lines: optional_usize(args, "tail_lines")?
                .unwrap_or(DEFAULT_TAIL_LINES)
                .clamp(1, MAX_LINES_PER_CALL),
            since_line_id: optional_usize(args, "since_line_id")?,
        }),
        "search_session_output" => {
            let session_id = required_uuid(args, "session_id")?;
            let query = required_str(args, "query")?.to_string();
            let regex = optional_bool(args, "regex")?.unwrap_or(false);
            if regex {
                validate_regex(&query)?;
            }
            Ok(McpOp::SearchSessionOutput {
                session_id,
                query,
                regex,
            })
        }
        // 카탈로그에는 있으나 여기 분기가 없는 이름 — 툴 추가 시 짝을 빠뜨린 경우다.
        _ => Err(JsonRpcError::new(
            INVALID_PARAMS,
            format!("Tool {name} is listed but not wired"),
        )),
    }
}

/// 정규식 패턴을 검색에 넘기기 전에 컴파일해 본다. `RunSession::search_matches`는 검색바가
/// 타이핑 중인 `[`를 정상 상태로 취급해야 하므로 잘못된 패턴을 빈 결과로 흘리는데, 에이전트에게
/// 그 결과는 "매치 없음"과 구별되지 않아 틀린 패턴을 확신에 찬 0건으로 답하게 만든다.
fn validate_regex(pattern: &str) -> Result<(), JsonRpcError> {
    compile_search_regex(pattern).map(|_| ()).map_err(|e| {
        invalid_params(format!(
            "query is not a valid regular expression: {}",
            truncate_regex_error(&e.to_string())
        ))
    })
}

/// `limit` chars를 넘으면 char 경계에서 자른 앞부분, 아니면 `None`. 패턴은 사용자 입력이라
/// 바이트 슬라이싱은 멀티바이트에서 패닉한다.
fn head_chars(text: &str, limit: usize) -> Option<&str> {
    text.char_indices().nth(limit).map(|(cut, _)| &text[..cut])
}

/// 잘라내면서 실패 사유는 남긴다. `regex-syntax`는 `regex parse error:` → 패턴 → 캐럿 →
/// `error: <사유>` 순으로 쓰므로 사유가 꼬리에 있다 — 앞부분만 남기면 호출자에게 "컴파일이
/// 실패했다"만 남고 무엇이 잘못됐는지가 사라진다. 사유도 같은 상한으로 자르는 것은, 사유
/// 문자열이 패턴을 품지 않는다는 보장을 `regex` crate의 현재 구현에 맡기지 않기 위한 것이다.
fn truncate_regex_error(text: &str) -> String {
    let Some(head) = head_chars(text, MAX_REGEX_ERROR_CHARS) else {
        return text.to_string();
    };
    let reason = text.lines().last().unwrap_or_default();
    format!(
        "{head}... {}",
        head_chars(reason, MAX_REGEX_ERROR_CHARS).unwrap_or(reason)
    )
}

fn invalid_params(message: impl Into<String>) -> JsonRpcError {
    JsonRpcError::new(INVALID_PARAMS, message)
}

fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, JsonRpcError> {
    match args.get(key) {
        Some(Value::String(s)) if !s.trim().is_empty() => Ok(s.as_str()),
        Some(Value::String(_)) => Err(invalid_params(format!("{key} must not be empty"))),
        Some(_) => Err(invalid_params(format!("{key} must be a string"))),
        None => Err(invalid_params(format!("Missing required argument: {key}"))),
    }
}

fn required_uuid(args: &Value, key: &str) -> Result<Uuid, JsonRpcError> {
    let raw = required_str(args, key)?;
    Uuid::parse_str(raw).map_err(|_| invalid_params(format!("{key} must be a UUID")))
}

fn optional_usize(args: &Value, key: &str) -> Result<Option<usize>, JsonRpcError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => n
            .as_u64()
            .map(|v| Some(v as usize))
            .ok_or_else(|| invalid_params(format!("{key} must be a non-negative integer"))),
        Some(_) => Err(invalid_params(format!("{key} must be an integer"))),
    }
}

fn optional_bool(args: &Value, key: &str) -> Result<Option<bool>, JsonRpcError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(invalid_params(format!("{key} must be a boolean"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool_names(permission: McpPermission) -> Vec<String> {
        tools_for(permission)
            .into_iter()
            .map(|tool| tool["name"].as_str().expect("name").to_string())
            .collect()
    }

    #[test]
    fn permission_default_is_read_only() {
        assert_eq!(McpPermission::default(), McpPermission::ReadOnly);
    }

    #[test]
    fn higher_tiers_include_lower_ones() {
        assert!(McpPermission::Edit.allows(McpPermission::Execute));
        assert!(McpPermission::Edit.allows(McpPermission::ReadOnly));
        assert!(McpPermission::Execute.allows(McpPermission::ReadOnly));
    }

    #[test]
    fn read_only_does_not_allow_higher_tiers() {
        assert!(!McpPermission::ReadOnly.allows(McpPermission::Execute));
        assert!(!McpPermission::ReadOnly.allows(McpPermission::Edit));
        assert!(!McpPermission::Execute.allows(McpPermission::Edit));
    }

    #[test]
    fn permission_serializes_as_snake_case() {
        // 설정 파일에 남는 표현이다 — 바뀌면 사용자 설정이 조용히 기본값으로 돌아간다.
        let json = serde_json::to_string(&McpPermission::ReadOnly).expect("serialize");
        assert_eq!(json, "\"read_only\"");
        let parsed: McpPermission = serde_json::from_str("\"edit\"").expect("deserialize");
        assert_eq!(parsed, McpPermission::Edit);
    }

    #[test]
    fn read_only_exposes_the_read_tools() {
        let names = tool_names(McpPermission::ReadOnly);
        assert!(names.contains(&"list_configurations".to_string()));
        assert!(names.contains(&"list_sessions".to_string()));
        assert!(names.contains(&"read_session_output".to_string()));
    }

    #[test]
    fn every_listed_tool_parses_from_its_own_name() {
        // 카탈로그와 parse_call 분기가 갈라지면 "목록에는 있는데 호출은 안 되는" 툴이 생긴다.
        for tool in TOOLS {
            let err = parse_call(tool.name, &json!({}));
            let unwired = matches!(&err, Err(e) if e.message.contains("listed but not wired"));
            assert!(!unwired, "{} is listed but not wired", tool.name);
        }
    }

    #[test]
    fn every_listed_tool_has_an_object_schema() {
        for tool in TOOLS {
            let json = tool.to_json();
            assert_eq!(json["inputSchema"]["type"], "object", "{}", tool.name);
            assert!(!tool.description.is_empty(), "{}", tool.name);
        }
    }

    #[test]
    fn unknown_tool_is_method_not_found() {
        let err = parse_call("rm_rf_everything", &json!({})).expect_err("must fail");
        assert_eq!(err.code, METHOD_NOT_FOUND);
    }

    #[test]
    fn get_configuration_requires_target() {
        let err = parse_call("get_configuration", &json!({})).expect_err("must fail");
        assert_eq!(err.code, INVALID_PARAMS);

        let err = parse_call("get_configuration", &json!({ "configuration": "  " }))
            .expect_err("blank rejected");
        assert_eq!(err.code, INVALID_PARAMS);

        let op = parse_call("get_configuration", &json!({ "configuration": "build" }))
            .expect("valid call");
        assert_eq!(
            op,
            McpOp::GetConfiguration {
                target: "build".to_string()
            }
        );
    }

    #[test]
    fn read_session_output_defaults_and_clamps_tail_lines() {
        let id = Uuid::new_v4();

        let default_call = parse_call(
            "read_session_output",
            &json!({ "session_id": id.to_string() }),
        )
        .expect("valid");
        assert_eq!(
            default_call,
            McpOp::ReadSessionOutput {
                session_id: id,
                tail_lines: DEFAULT_TAIL_LINES,
                since_line_id: None,
            }
        );

        let clamped = parse_call(
            "read_session_output",
            &json!({ "session_id": id.to_string(), "tail_lines": 10_000_000 }),
        )
        .expect("valid");
        assert_eq!(
            clamped,
            McpOp::ReadSessionOutput {
                session_id: id,
                tail_lines: MAX_LINES_PER_CALL,
                since_line_id: None,
            }
        );

        let zero = parse_call(
            "read_session_output",
            &json!({ "session_id": id.to_string(), "tail_lines": 0 }),
        )
        .expect("valid");
        assert_eq!(
            zero,
            McpOp::ReadSessionOutput {
                session_id: id,
                tail_lines: 1,
                since_line_id: None,
            }
        );
    }

    #[test]
    fn session_id_must_be_a_uuid() {
        let err = parse_call(
            "read_session_output",
            &json!({ "session_id": "not-a-uuid" }),
        )
        .expect_err("must fail");
        assert_eq!(err.code, INVALID_PARAMS);
        assert!(err.message.contains("UUID"));
    }

    #[test]
    fn wrong_argument_types_are_rejected() {
        let id = Uuid::new_v4().to_string();

        let err = parse_call(
            "read_session_output",
            &json!({ "session_id": &id, "tail_lines": "many" }),
        )
        .expect_err("string tail_lines rejected");
        assert_eq!(err.code, INVALID_PARAMS);

        let err = parse_call(
            "read_session_output",
            &json!({ "session_id": &id, "tail_lines": -5 }),
        )
        .expect_err("negative tail_lines rejected");
        assert_eq!(err.code, INVALID_PARAMS);

        let err = parse_call(
            "search_session_output",
            &json!({ "session_id": &id, "query": "x", "regex": "yes" }),
        )
        .expect_err("string regex rejected");
        assert_eq!(err.code, INVALID_PARAMS);

        let err = parse_call("get_configuration", &json!({ "configuration": 42 }))
            .expect_err("numeric target rejected");
        assert_eq!(err.code, INVALID_PARAMS);
    }

    #[test]
    fn search_defaults_to_substring_mode() {
        let id = Uuid::new_v4();
        let op = parse_call(
            "search_session_output",
            &json!({ "session_id": id.to_string(), "query": "error" }),
        )
        .expect("valid");
        assert_eq!(
            op,
            McpOp::SearchSessionOutput {
                session_id: id,
                query: "error".to_string(),
                regex: false,
            }
        );
    }

    /// 카탈로그를 훑는 테스트가 각 툴을 실제로 파싱시키기 위한 최소 인자.
    fn sample_args(name: &str) -> Value {
        let session_id = Uuid::new_v4().to_string();
        match name {
            "get_configuration" => json!({ "configuration": "build" }),
            "read_session_output" => json!({ "session_id": session_id }),
            "search_session_output" => json!({ "session_id": session_id, "query": "error" }),
            _ => json!({}),
        }
    }

    #[test]
    fn an_invalid_regex_is_rejected_instead_of_reporting_no_matches() {
        let session_id = Uuid::new_v4().to_string();
        let err = parse_call(
            "search_session_output",
            &json!({ "session_id": session_id, "query": "[", "regex": true }),
        )
        .expect_err("an uncompilable pattern must not reach the search");
        assert_eq!(err.code, INVALID_PARAMS);
        assert!(
            err.message.contains("regular expression"),
            "{}",
            err.message
        );
    }

    #[test]
    fn the_same_pattern_is_a_valid_substring_query() {
        // regex를 켜지 않으면 `[`는 그냥 찾을 문자다 — 검증은 regex 모드에만 걸려야 한다.
        let session_id = Uuid::new_v4().to_string();
        assert!(
            parse_call(
                "search_session_output",
                &json!({ "session_id": session_id, "query": "[" }),
            )
            .is_ok()
        );
    }

    #[test]
    fn a_compilable_pattern_passes() {
        let session_id = Uuid::new_v4().to_string();
        assert!(
            parse_call(
                "search_session_output",
                &json!({ "session_id": session_id, "query": r"err\w* \d+", "regex": true }),
            )
            .is_ok()
        );
    }

    #[test]
    fn a_pattern_over_the_compile_size_limit_is_rejected() {
        // 문법은 올바르지만 컴파일이 거부하는 축. 문법만 검사하는 검증기는 이걸 통과시켜
        // `search_matches`의 조용한 0건으로 되돌린다.
        let session_id = Uuid::new_v4().to_string();
        let bomb = format!("(?:{}){{1000}}", "a".repeat(200));
        let err = parse_call(
            "search_session_output",
            &json!({ "session_id": session_id, "query": bomb, "regex": true }),
        )
        .expect_err("a pattern the compiler refuses must not reach the search");
        assert_eq!(err.code, INVALID_PARAMS);
    }

    #[test]
    fn a_huge_pattern_does_not_echo_itself_into_the_error() {
        // 에러 위치가 패턴 끝이면 `regex-syntax`는 패턴과 같은 길이의 캐럿 라인을 덧붙인다.
        // 잘라내지 않으면 요청 하나가 호출자 컨텍스트에 패턴 2배 크기로 되돌아온다.
        let session_id = Uuid::new_v4().to_string();
        let query = format!("{}(", "가".repeat(50_000));
        let err = parse_call(
            "search_session_output",
            &json!({ "session_id": session_id, "query": query, "regex": true }),
        )
        .expect_err("an unclosed group must be rejected");
        assert!(
            err.message.chars().count() < MAX_REGEX_ERROR_CHARS * 3,
            "{} chars",
            err.message.chars().count()
        );
        assert!(err.message.contains("..."), "{}", err.message);
        assert!(
            err.message.contains("error: unclosed group"),
            "절단이 실패 사유를 삼켰다: {}",
            err.message
        );
    }

    #[test]
    fn edit_is_the_strictest_tier() {
        // `required_tier`의 fail-closed 기본값이 `Edit`이라는 전제를 고정한다. 더 높은 단계가
        // 추가되면 그 기본값은 조용히 fail-open으로 바뀌는데, 카탈로그를 훑는 아래 테스트는
        // 카탈로그 안의 툴만 보므로 그 변화를 잡지 못한다.
        assert!(
            McpPermission::ALL
                .iter()
                .all(|tier| McpPermission::Edit.allows(*tier))
        );
    }

    #[test]
    fn each_op_resolves_back_to_its_catalog_entry() {
        // tier가 카탈로그 하나에서만 나오는지를 지키는 테스트. 이름 매핑이 어긋나면
        // `tools/call` 재검사가 다른 툴의 tier로 판정해 목록에서 숨긴 툴을 통과시킨다.
        for tool in TOOLS {
            let op = parse_call(tool.name, &sample_args(tool.name)).expect(tool.name);
            assert_eq!(op.tool_name(), Some(tool.name), "{}", tool.name);
            assert_eq!(op.required_tier(), tool.tier, "{}", tool.name);
        }
    }

    #[test]
    fn read_tools_require_only_read_permission() {
        assert_eq!(McpOp::ListSessions.required_tier(), McpPermission::ReadOnly);
        assert_eq!(
            McpOp::ListConfigurations.required_tier(),
            McpPermission::ReadOnly
        );
    }
}
