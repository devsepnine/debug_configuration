//! 툴 카탈로그와 인자 파싱 — 앱 상태를 모르는 순수 계층.
//!
//! 권한 단계(`McpPermission`)가 노출 여부를 결정한다. 허용되지 않은 툴은 `tools/list`에
//! 아예 나타나지 않으며, `tools/call`에서도 독립적으로 다시 검사한다 — 목록을 캐시한
//! 클라이언트가 낡은 이름으로 호출할 수 있기 때문이다.
//!
//! 크기 임계값 예외(`coding-standards`의 `code-thresholds.md`): 이 파일은 File Length Hard(500 LOC)를,
//! `parse_call`과 `require_bounded_type_data`는 Function Length Hard(80 LOC)를, `parse_call`은 순환
//! 복잡도 Hard(15)까지 넘는다. `static TOOLS`가 `tools_for`·`McpOp::required_tier`·`parse_call`의 단일
//! 원본이라 카탈로그를 쪼개면 위의 일치(목록 필터 = 호출 재검사)를 구조로 보장할 수 없고,
//! `parse_call`은 툴마다 arm 하나인 표준의 `Unavoidable branch maps`이며,
//! `require_bounded_type_data`의 길이는 모든 변형을 `..` 없이 풀어 써 필드 추가를 컴파일 오류로
//! 만드는 대가다. 다음 기능을 얹기 전에 카탈로그·스키마와 인자 파싱·상한으로 파일을 가른다.

use super::protocol::{INVALID_PARAMS, JsonRpcError, METHOD_NOT_FOUND};
use crate::models::{
    ConfigTypeData, ConfigurationType, ExecuteMode, KotlinLaunchMode, compile_search_regex,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

/// `read_session_output`이 인자 없이 호출됐을 때 돌려주는 꼬리 라인 수.
/// 세션 버퍼는 기본 5만 라인까지 자라므로, 전체를 되돌리면 호출자의 컨텍스트를 잡아먹는다.
pub const DEFAULT_TAIL_LINES: usize = 200;
/// 한 번의 출력 조회로 되돌릴 수 있는 최대 라인 수 (요청이 더 크면 이 값으로 클램프).
pub const MAX_LINES_PER_CALL: usize = 5_000;
/// 한 번의 검색으로 되돌릴 수 있는 최대 **줄** 수. `session_search`가 같은 줄의 여러 매치를
/// 항목 하나로 접으므로(`match_count`) 이 상한과 `truncated`는 줄을 세고 `total_matches`는
/// 매치를 센다 — 한 줄에 매치가 몰린 검색은 `total_matches`가 이 값을 넘으면서도 잘리지 않는다.
pub const MAX_SEARCH_MATCHES: usize = 500;
/// 정규식 컴파일 에러에서 앞부분과 실패 사유에 각각 남길 최대 문자 수. `regex-syntax`는 잘못된
/// 패턴 전체를 되풀이한 뒤 offset만큼 공백을 채운 캐럿 라인을 붙이므로, 에러가 패턴 끝에 있으면
/// 메시지가 패턴 길이의 2배가 된다. `MAX_QUERY_CHARS`가 패턴 자체를 묶은 뒤로 최악은 1 MiB가
/// 아니라 그 상한의 2배지만, 증폭 자체는 남는다 — 통과한 질의 하나가 그만큼의 오류 문구로
/// 되돌아온다.
const MAX_REGEX_ERROR_CHARS: usize = 400;
/// 검색 질의에 허용하는 최대 문자 수. 이 문자열은 검색 응답에 그대로 되돌아가고
/// (`session_search`의 `query` 필드), regex 모드에서는 `compile_search_regex`가 iced update
/// 루프 안에서 그것을 컴파일한다 — body 상한 1 MiB를 통과한 패턴 하나가 응답을 그만큼 부풀리고
/// 컴파일하는 동안 앱 전체를 붙잡는다(`MAX_REGEX_ERROR_CHARS`는 **오류 문구**만 묶는다).
/// `search_session_output`은 기본 ReadOnly 티어에서 부를 수 있다. 4096은 실제 검색어일 수 있는
/// 길이를 넉넉히 덮는다 — 매치 가능성으로 정한 값이 아니다(저장되는 줄은 64 KiB까지 자란다).
const MAX_QUERY_CHARS: usize = 4096;
/// `request_id`에 허용하는 최대 문자 수. 이 문자열은 중복 흡수 창이 닫힐 때까지 앱 메모리에
/// 남으므로, body 상한 1 MiB를 통과한 값을 그대로 보관하면 창 크기만큼의 메모리를 호출자가
/// 붙잡아 둘 수 있다. UUID(36자)나 그보다 긴 관례적 식별자를 담기에는 충분하다.
const MAX_REQUEST_ID_CHARS: usize = 128;
/// serde 파싱 실패 문구에 남길 최대 문자 수. serde는 문제가 된 필드 이름·열거형 변형 이름을
/// 문구에 그대로 되풀이하므로, body 상한 1 MiB를 통과한 긴 키 하나가 그만큼의 오류 메시지가
/// 되어 호출자 컨텍스트를 채운다 — `MAX_REGEX_ERROR_CHARS`와 같은 종류의 증폭이다.
const MAX_PARSE_ERROR_CHARS: usize = 400;
/// 한 줄 입력에 허용하는 최대 바이트 수. 상한을 두는 근거는 이 호출이 노출하는 자원 경계다 —
/// PTY writer 쓰기는 슬레이브 버퍼가 차면 블록하고, 그 쓰기는 `spawn_blocking` 안에서 std
/// `Mutex`를 쥔 채 멈춘다. `write_pty_payload`의 제한 시간은 await만 포기하므로 그 스레드와
/// 락은 풀리지 않아, 큰 줄 하나가 blocking 스레드와 세션 stdin을 영구히 붙잡을 수 있다.
///
/// 4096은 tty 라인 한도를 참고해 고른 값이다(Linux `N_TTY_BUF_SIZE` 4096, macOS `MAX_CANON`
/// 1024). 이 값이 "한 줄로 전달된다"를 보장하지는 않는다 — macOS 한도가 더 작다.
///
/// 스키마에 `maxLength`로 싣지 않는다: JSON Schema의 `maxLength`는 **문자** 수이므로 바이트
/// 상한을 그 자리에 넣으면 CJK 입력에서 클라이언트 로컬 검증을 통과한 요청이 서버에서 -32602를
/// 받는다. 대신 툴 설명이 바이트 상한임을 밝힌다.
const MAX_INPUT_LINE_BYTES: usize = 4096;
/// 구성 이름과 `configuration`(대상 지목)에 허용하는 최대 문자 수. 이름은 저장소에 영속되고
/// 상태바 문구에 실리며(`mcp_refuse_edit`의 사유가 원문을 품는다) 이후 모든 `list_configurations`
/// 응답에 되돌아온다 — body 상한 1 MiB를 통과한 값을 그대로 받으면 그 한 번의 호출이 세 곳을
/// 함께 부풀린다. `configuration`은 이름 또는 UUID이므로 같은 상한으로 충분하다.
const MAX_NAME_CHARS: usize = 200;
/// `working_directory`에 허용하는 최대 문자 수. 상한의 근거는 플랫폼 경로 한도다
/// (Linux `PATH_MAX` 4096, Windows 확장 경로 32767). 실제 경로일 수 없는 길이를 저장소에
/// 영속시키지 않는 것이 목적이고, 경로의 **존재**는 여기서 검사하지 않는다.
const MAX_PATH_CHARS: usize = 4096;
/// 환경변수 이름에 허용하는 최대 문자 수. 실제 환경변수 이름은 이보다 훨씬 짧다.
const MAX_ENV_KEY_CHARS: usize = 256;
/// 환경변수 값에 허용하는 최대 문자 수. PEM 사문서 키나 긴 `PATH`가 들어갈 만큼 넉넉해야
/// 하므로 이름보다 크게 잡는다. 한 호출이 실을 수 있는 **총량**은 body 상한 1 MiB가 이미
/// 묶으므로, 이 상한이 맡는 것은 값 하나가 저장소에 영속되는 크기다.
const MAX_ENV_VALUE_CHARS: usize = 32768;
/// `type_data`의 자유 형식 텍스트(명령 인자·VM 옵션·인라인 스크립트 본문 등)에 허용하는 최대
/// 문자 수. 경로도 이름도 아닌 필드들이며, 저장소에 영속되고 이후 모든 조회·편집 응답에
/// 되돌아오고 매 프레임 편집기에 렌더되고 일부는 그대로 실행된다 — `MAX_ENV_VALUE_CHARS`와
/// 같은 종류의 증폭이라 같은 크기로 잡는다. 이보다 큰 스크립트는 `ScriptFile` 모드로 파일에
/// 두면 되므로 이 상한이 막는 사용 사례는 없다.
const MAX_CONFIG_TEXT_CHARS: usize = 32768;
/// compound 멤버 목록에 허용하는 최대 개수. 근거는 앱 계층 검사의 비용이다 —
/// `check_compound_members`는 멤버마다 구성을 조회하고(선형) 중복 여부를 앞쪽 전체와
/// 비교하므로(제곱) update 루프 안에서 돈다. body 상한 1 MiB는 UUID 약 28,000개를 통과시켜
/// 그 검사만으로 앱이 수 초간 멈춘다(D73과 같은 종류의 결함). 1024는 GUI로 만들 수 있는
/// compound를 거절하지 않을 만큼 크다 — 멤버는 존재하는 비-compound 구성이어야 하고 중복도
/// 거절되므로 실제 상한은 구성 개수이고, 멤버마다 세션과 페인이 하나씩 펼쳐진다.
const MAX_COMPOUND_MEMBERS: usize = 1024;

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
                      directory. Environment variable names are included; no values are, masked or \
                      otherwise — read one configuration with get_configuration for those.",
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
                        "maxLength": MAX_NAME_CHARS,
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
                      exit code, stopped by the user, or errored when the run produced no exit \
                      code at all — that reason is in the error field), start time, duration and \
                      buffered output line count.",
        tier: McpPermission::ReadOnly,
        schema: || json!({ "type": "object", "properties": {}, "additionalProperties": false }),
    },
    ToolSpec {
        name: "read_session_output",
        title: "Read session output",
        description: "Read a session's terminal output as plain text with ANSI styling removed. \
                      Use since_line_id with the last_line_id from a previous call to stream only \
                      what was appended since. The run banner's environment variable values are \
                      replaced with a placeholder unless the app's Expose env values setting is \
                      on; values a program prints itself are returned as printed.",
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
                        "description": format!(
                            "How many trailing lines to return. Defaults to {DEFAULT_TAIL_LINES}."
                        ),
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
                      default, or a case-insensitive regular expression when regex is true. Lines \
                      masked in read_session_output are left out of both the results and \
                      total_matches, so a masked value cannot be confirmed by searching for it.",
        tier: McpPermission::ReadOnly,
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Session id (UUID)." },
                    "query": {
                        "type": "string",
                        "maxLength": MAX_QUERY_CHARS,
                        "description": "Substring or regex to look for.",
                    },
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
    ToolSpec {
        name: "run_configuration",
        title: "Run a configuration",
        description: "Start a configuration and return as soon as the session exists — the run \
                      keeps going inside the app. Poll read_session_output or list_sessions for \
                      progress, the final state and the exit code; a command that fails to start \
                      is reported there as a session in the errored state carrying the reason, \
                      not as an error from this call. A \
                      compound configuration starts one session per member, so read the sessions \
                      array rather than session_id.",
        tier: McpPermission::Execute,
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "configuration": {
                        "type": "string",
                        "maxLength": MAX_NAME_CHARS,
                        "description": "Configuration id (UUID) or its exact name.",
                    },
                    "request_id": {
                        "type": "string",
                        "maxLength": MAX_REQUEST_ID_CHARS,
                        "description": "Caller-chosen id that makes a retry safe: the same value \
                                        within 60 seconds returns the sessions the first call \
                                        started instead of starting another run. Reusing it for a \
                                        different configuration is rejected. Without it, a \
                                        retried request runs the configuration again.",
                    },
                },
                "required": ["configuration"],
                "additionalProperties": false,
            })
        },
    },
    ToolSpec {
        name: "stop_session",
        title: "Stop a running session",
        description: "Terminate a session's process. Safe to repeat: a session that already \
                      finished is reported with its final state instead of an error.",
        tier: McpPermission::Execute,
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Session id (UUID)." },
                },
                "required": ["session_id"],
                "additionalProperties": false,
            })
        },
    },
    ToolSpec {
        name: "rerun_session",
        title: "Rerun a session",
        description: "Restart a session's configuration in the same pane. Output is kept: the \
                      new run appends below the old one. Pass the previous_last_line_id this call \
                      returns as since_line_id to read only the new run. The session gets a NEW \
                      id — poll the session_id this call returns, because the id passed in stops \
                      existing.",
        tier: McpPermission::Execute,
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Session id (UUID)." },
                    "request_id": {
                        "type": "string",
                        "maxLength": MAX_REQUEST_ID_CHARS,
                        "description": "Caller-chosen id that makes a retry safe: the same value \
                                        within 60 seconds returns the session the first call \
                                        restarted instead of restarting it again. Reusing it for a \
                                        different session is rejected. Without it, a retried \
                                        request restarts the session again.",
                    },
                },
                "required": ["session_id"],
                "additionalProperties": false,
            })
        },
    },
    ToolSpec {
        name: "send_session_input",
        title: "Send input to a session",
        description: "Write one line to a running session's stdin, as if typed into its input bar. \
                      The line terminator is added for you, so text must not contain newline or \
                      carriage-return characters — answer a multi-step prompt with one call per \
                      line. An empty string sends a bare newline. The call answers once the write \
                      resolves: status is delivered, or pending when the program has not read \
                      stdin yet (the write is still in flight, so resending would deliver the \
                      line twice).",
        tier: McpPermission::Execute,
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Session id (UUID)." },
                    "text": {
                        "type": "string",
                        "description": format!(
                            "The line to send. At most {MAX_INPUT_LINE_BYTES} bytes of UTF-8 — a \
                             longer line is rejected, never truncated, and a line of non-ASCII \
                             text reaches the limit before its character count does."
                        ),
                    },
                    "request_id": {
                        "type": "string",
                        "maxLength": MAX_REQUEST_ID_CHARS,
                        "description": "Caller-chosen id that makes a retry safe: repeating it \
                                        with the same session and the same text within 60 \
                                        seconds is answered with status duplicate instead of \
                                        writing the line twice. Reusing it for a different \
                                        session or a different line is rejected, so use a fresh \
                                        id for each line. Without it, a retried request sends \
                                        the line again.",
                    },
                },
                "required": ["session_id", "text"],
                "additionalProperties": false,
            })
        },
    },
    ToolSpec {
        name: "create_configuration",
        title: "Create a run configuration",
        description: "Create a new run/debug configuration and save the list to disk. The id is \
                      chosen by the app and returned; it cannot be supplied. Copy the type_data \
                      object from get_configuration on a configuration of the same type and edit \
                      its fields.",
        tier: McpPermission::Edit,
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "maxLength": MAX_NAME_CHARS,
                        "description": "Display name. Must not already be taken, and must not be \
                                        a UUID — names address configurations in the other tools.",
                    },
                    "working_directory": {
                        "type": "string",
                        "maxLength": MAX_PATH_CHARS,
                        "description": "Directory the program runs in. Existence is not \
                                        checked, but an empty value is refused: it is handed to the \
                                        process as its working directory and the spawn then fails. \
                                        Two types ignore it — Compound runs each member in the \
                                        member's own directory, and Node takes this from its \
                                        project_directory — so for those it may be an empty string.",
                    },
                    "environment_variables": env_map_schema(),
                    "type_data": type_data_schema(),
                    "request_id": request_id_schema(
                        "Optional idempotency key. Retrying the same request_id within 60 seconds \
                         answers with the configuration the first call created instead of creating \
                         a second one. Without it, a retried call creates a duplicate.",
                    ),
                },
                "required": ["name", "working_directory", "type_data"],
                "additionalProperties": false,
            })
        },
    },
    ToolSpec {
        name: "update_configuration",
        title: "Update a run configuration",
        description: "Change fields of one configuration, saving the list to disk when the call \
                      actually changed something. Only the fields present are touched, and at \
                      least one is required. Environment variables are edited by key \
                      (set_environment_variables / remove_environment_variables) rather than \
                      replaced wholesale, because values you read may be masked and writing them \
                      back would destroy the real ones. \
                      type_data, when present, replaces the whole object — copy it from \
                      get_configuration and edit it. The response's changed flag says whether the \
                      call altered anything; it is null when a retried request_id was absorbed, \
                      because that answer describes the first call, not this one.",
        tier: McpPermission::Edit,
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "configuration": {
                        "type": "string",
                        "maxLength": MAX_NAME_CHARS,
                        "description": "Configuration id (UUID) or its exact name.",
                    },
                    "name": {
                        "type": "string",
                        "maxLength": MAX_NAME_CHARS,
                        "description": "New display name. Must not be a UUID. Renaming detaches \
                                        the sessions already started from this configuration: \
                                        rerun_session resolves the configuration by name, so \
                                        their rerun fails and list_sessions keeps reporting the \
                                        old name. Restoring the name reattaches them — and so \
                                        does any other configuration that takes that name, \
                                        because a session carries only the name.",
                    },
                    "working_directory": {
                        "type": "string",
                        "maxLength": MAX_PATH_CHARS,
                        "description": "New working directory. Existence is not checked. Empty is \
                                        refused unless the resulting type ignores it (Compound, or \
                                        Node, which derives it from project_directory).",
                    },
                    "type_data": type_data_schema(),
                    "set_environment_variables": env_map_schema(),
                    "remove_environment_variables": {
                        "type": "array",
                        "items": { "type": "string", "maxLength": MAX_ENV_KEY_CHARS },
                        "description": "Variable names to delete. Names that are not set are \
                                        ignored — the response's changed flag says whether the \
                                        call altered anything.",
                    },
                    "request_id": request_id_schema(
                        "Optional idempotency key. Retrying the same request_id within 60 seconds \
                         answers with the first call's result instead of applying the patch twice.",
                    ),
                },
                "required": ["configuration"],
                "additionalProperties": false,
            })
        },
    },
    ToolSpec {
        name: "delete_configuration",
        title: "Delete a run configuration",
        description: "Delete one configuration and save the list to disk. This is not undoable and \
                      does not ask the user to confirm — the app's own Delete button does. Compound \
                      configurations that listed it as a member lose that member; the response \
                      names them. Sessions already running for it are left running and their \
                      rerun breaks, so stop them first if that matters.",
        tier: McpPermission::Edit,
        schema: || {
            json!({
                "type": "object",
                "properties": {
                    "configuration": {
                        "type": "string",
                        "maxLength": MAX_NAME_CHARS,
                        "description": "Configuration id (UUID) or its exact name.",
                    },
                    "request_id": request_id_schema(
                        "Optional idempotency key. Retrying the same request_id within 60 seconds \
                         is answered as already done instead of \"no configuration matches\", which \
                         is otherwise indistinguishable from a wrong target.",
                    ),
                },
                "required": ["configuration"],
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

/// 환경변수 맵 스키마 조각. 값이 문자열이라는 사실만 싣는다 — 키 이름에는 제약이 없다.
fn env_map_schema() -> Value {
    json!({
        "type": "object",
        "propertyNames": { "maxLength": MAX_ENV_KEY_CHARS },
        "additionalProperties": { "type": "string", "maxLength": MAX_ENV_VALUE_CHARS },
        "description": "Variable name to value. A value of \"<hidden>\" is rejected: that is the \
                        placeholder masked reads return, so accepting it would overwrite the real \
                        secret with the mask.",
    })
}

fn request_id_schema(description: &str) -> Value {
    json!({
        "type": "string",
        "maxLength": MAX_REQUEST_ID_CHARS,
        "description": description,
    })
}

/// 타입별 데이터 스키마. 필드를 타입마다 나열하지 않는 것은 의도적이다 — 다섯 타입의 중첩
/// 열거형을 손으로 옮긴 스키마는 모델이 늘어날 때 조용히 뒤처지고, 그 순간 유효한 구성이
/// 거절된다. 대신 `type` 목록만 모델(`ConfigurationType::ALL`)에서 끌어오고, 나머지는
/// `get_configuration`이 돌려준 형태를 그대로 쓰라고 설명에 싣는다.
///
/// 느슨한 스키마가 검증을 느슨하게 만들지 않는 근거는 `ConfigTypeData`의 `deny_unknown_fields`
/// 하나다 — 그것 없이는 여러 필드가 `#[serde(default)]`이므로 오타가 기본값으로 파싱된다.
/// 스키마를 느슨하게 두는 판단은 그 속성에 매여 있다.
fn type_data_schema() -> Value {
    let types: Vec<Value> = ConfigurationType::ALL
        .iter()
        .map(|kind| json!(kind))
        .collect();
    json!({
        "type": "object",
        "properties": {
            "type": {
                "enum": types,
                "description": "Which kind of configuration this is.",
            },
        },
        "required": ["type"],
        "description": format!(
            "Type-specific fields in exactly the shape get_configuration returns. Read a \
             configuration of the same type first and copy its type_data, then edit the fields \
             you want. Unknown fields are rejected rather than ignored. A Compound's members \
             must be existing configurations that are not compounds, not the configuration being \
             edited, and each listed once, because a compound member is skipped instead of run \
             and a repeated one would open two sessions for it. Strings here are bounded even \
             though this schema does not list them: paths at most {MAX_PATH_CHARS} characters, \
             names {MAX_NAME_CHARS}, other free-form text {MAX_CONFIG_TEXT_CHARS}, and at most \
             {MAX_COMPOUND_MEMBERS} members. The app applies no such limits to what a person \
             types into it, so a stored value can be longer than the limit and copying that \
             type_data back is refused even when you changed nothing in it — leave type_data out \
             when you are not changing it, and a value that is already too long has to be \
             shortened in the app."
        ),
    })
}

/// `create_configuration`의 인자.
///
/// `id`가 없는 것이 이 타입의 요점이다 — `deny_unknown_fields`가 호출자의 `id`를 조용히 무시
/// 하는 대신 거절하므로, "내가 정한 id로 만들어졌다"고 믿은 호출자가 그 id로 계속 실패하는
/// 경로가 타입 수준에서 사라진다.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CreateConfigurationArgs {
    pub name: String,
    pub working_directory: String,
    #[serde(default)]
    pub environment_variables: std::collections::HashMap<String, String>,
    pub type_data: ConfigTypeData,
    pub request_id: Option<String>,
}

/// `update_configuration`의 인자. 없는 필드는 "바꾸지 않는다"를 뜻한다.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateConfigurationArgs {
    pub configuration: String,
    pub name: Option<String>,
    pub working_directory: Option<String>,
    pub type_data: Option<ConfigTypeData>,
    #[serde(default)]
    pub set_environment_variables: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub remove_environment_variables: Vec<String>,
    pub request_id: Option<String>,
}

impl UpdateConfigurationArgs {
    /// 실제로 바꾸는 것이 하나라도 있는지. 없는 호출에 성공으로 답하면 호출자는 바뀌지 않은
    /// 구성을 바뀐 것으로 읽는다.
    fn changes_something(&self) -> bool {
        self.name.is_some()
            || self.working_directory.is_some()
            || self.type_data.is_some()
            || !self.set_environment_variables.is_empty()
            || !self.remove_environment_variables.is_empty()
    }
}

/// `delete_configuration`의 인자.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteConfigurationArgs {
    pub configuration: String,
    pub request_id: Option<String>,
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
    RunConfiguration {
        /// 구성 id(UUID) 또는 정확한 이름.
        target: String,
        /// 있으면 중복 흡수 창의 키. 없으면 재시도가 그대로 두 번째 실행이 된다.
        request_id: Option<String>,
    },
    StopSession {
        session_id: Uuid,
    },
    RerunSession {
        session_id: Uuid,
        request_id: Option<String>,
    },
    SendSessionInput {
        session_id: Uuid,
        /// 보낼 한 줄. 줄 종결자는 전송 계층이 붙이므로 개행을 담지 않는다.
        text: String,
        request_id: Option<String>,
    },
    /// 인자가 다른 변형보다 크므로 `Box`로 담는다 — 열거형 전체 크기가 가장 큰 변형에
    /// 맞춰지고, 이 열거형은 모든 요청이 거쳐 간다.
    CreateConfiguration(Box<CreateConfigurationArgs>),
    UpdateConfiguration(Box<UpdateConfigurationArgs>),
    DeleteConfiguration(DeleteConfigurationArgs),
}

impl McpOp {
    /// 이 작업을 발행한 카탈로그 툴의 이름. `ListTools`는 프로토콜 메서드이지 툴이 아니다.
    pub fn tool_name(&self) -> Option<&'static str> {
        match self {
            Self::ListTools => None,
            Self::ListConfigurations => Some("list_configurations"),
            Self::GetConfiguration { .. } => Some("get_configuration"),
            Self::ListSessions => Some("list_sessions"),
            Self::ReadSessionOutput { .. } => Some("read_session_output"),
            Self::SearchSessionOutput { .. } => Some("search_session_output"),
            Self::RunConfiguration { .. } => Some("run_configuration"),
            Self::StopSession { .. } => Some("stop_session"),
            Self::RerunSession { .. } => Some("rerun_session"),
            Self::SendSessionInput { .. } => Some("send_session_input"),
            Self::CreateConfiguration(_) => Some("create_configuration"),
            Self::UpdateConfiguration(_) => Some("update_configuration"),
            Self::DeleteConfiguration(_) => Some("delete_configuration"),
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
            target: required_configuration_target(args)?,
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
            require_length("query", &query, MAX_QUERY_CHARS)?;
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
        "run_configuration" => Ok(McpOp::RunConfiguration {
            target: required_configuration_target(args)?,
            request_id: optional_request_id(args)?,
        }),
        "stop_session" => Ok(McpOp::StopSession {
            session_id: required_uuid(args, "session_id")?,
        }),
        "rerun_session" => Ok(McpOp::RerunSession {
            session_id: required_uuid(args, "session_id")?,
            request_id: optional_request_id(args)?,
        }),
        "send_session_input" => Ok(McpOp::SendSessionInput {
            session_id: required_uuid(args, "session_id")?,
            text: required_single_line(args, "text")?.to_string(),
            request_id: optional_request_id(args)?,
        }),
        "create_configuration" => {
            let spec: CreateConfigurationArgs = parse_args(name, args)?;
            validate_request_id(spec.request_id.as_deref())?;
            require_configuration_name("name", &spec.name)?;
            require_working_directory(&spec.working_directory, &spec.type_data)?;
            require_bounded_type_data(&spec.type_data)?;
            require_bounded_env(&spec.environment_variables)?;
            Ok(McpOp::CreateConfiguration(Box::new(spec)))
        }
        "update_configuration" => {
            let patch: UpdateConfigurationArgs = parse_args(name, args)?;
            validate_request_id(patch.request_id.as_deref())?;
            require_bounded("configuration", &patch.configuration, MAX_NAME_CHARS)?;
            if let Some(new_name) = &patch.name {
                require_configuration_name("name", new_name)?;
            }
            if let Some(directory) = &patch.working_directory {
                // 결과 타입을 아는 것은 같은 패치가 `type_data`를 실은 경우뿐이다. 싣지 않았다면
                // 기존 타입이 무엇인지 파서는 모르므로 빈 값을 막는다 — compound의 디렉터리를
                // 비우는 것은 아무 실행도 바꾸지 않는 반면, Application의 cwd를 비우는 것은
                // 실행을 깨뜨린다.
                match &patch.type_data {
                    Some(type_data) => require_working_directory(directory, type_data)?,
                    None => require_bounded("working_directory", directory, MAX_PATH_CHARS)?,
                }
            }
            if let Some(type_data) = &patch.type_data {
                require_bounded_type_data(type_data)?;
            }
            require_bounded_env(&patch.set_environment_variables)?;
            for key in &patch.remove_environment_variables {
                require_bounded("environment variable name", key, MAX_ENV_KEY_CHARS)?;
            }
            if !patch.changes_something() {
                return Err(invalid_params(
                    "update_configuration needs at least one field to change",
                ));
            }
            Ok(McpOp::UpdateConfiguration(Box::new(patch)))
        }
        "delete_configuration" => {
            let target: DeleteConfigurationArgs = parse_args(name, args)?;
            validate_request_id(target.request_id.as_deref())?;
            require_bounded("configuration", &target.configuration, MAX_NAME_CHARS)?;
            Ok(McpOp::DeleteConfiguration(target))
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

/// 인자 객체를 타입으로 읽는다. 필드를 손으로 꺼내지 않는 것은 읽기 쪽(`configuration_detail`)이
/// 구성을 통째로 직렬화하는 것과 같은 이유다 — 두 방향이 같은 serde 표현을 쓰면, 모델이 늘어날 때
/// 한쪽만 뒤처지지 않는다. `deny_unknown_fields`가 스키마의 `additionalProperties: false`를
/// 서버에서 실제로 집행하는 역할까지 겸한다.
fn parse_args<'de, T: Deserialize<'de>>(tool: &str, args: &'de Value) -> Result<T, JsonRpcError> {
    T::deserialize(args)
        .map_err(|e| invalid_params(format!("{tool}: {}", truncate_parse_error(&e.to_string()))))
}

/// serde 문구를 상한 안으로 자른다. 사유가 앞에 오는 형식(`unknown field \`x\`, expected ...`)
/// 이므로 앞부분만 남겨도 무엇이 잘못됐는지가 남는다.
fn truncate_parse_error(text: &str) -> String {
    match head_chars(text, MAX_PARSE_ERROR_CHARS) {
        Some(head) => format!("{head}..."),
        None => text.to_string(),
    }
}

/// 공백만인 문자열과 상한을 넘는 문자열을 거절한다. serde는 빈 문자열을 유효한 `String`으로
/// 읽으므로, 이름이나 대상이 비어 있는 채로 통과하면 이름으로 구성을 찾는 다른 툴이 그 구성을
/// 부를 수 없다. 상한이 필요한 이유는 상한별 문서에 있다(`MAX_NAME_CHARS` 외).
fn require_bounded(key: &str, value: &str, limit: usize) -> Result<(), JsonRpcError> {
    if value.trim().is_empty() {
        return Err(invalid_params(format!("{key} must not be empty")));
    }
    require_length(key, value, limit)
}

/// 구성을 지목하는 호출자 문자열. `required_str`가 공백만인 값을 이미 거절하므로 여기서 더하는
/// 것은 길이 상한뿐이다 — 이 문자열은 실패 문구로 호출자에게 되돌아가고, `run_configuration`은
/// 그것을 상태바와 중복 흡수 창에도 남긴다(`Entry.target`). 편집 툴의 `configuration`은
/// `require_bounded`로 같은 상한을 이미 걸고 있어, 조회·실행만 열려 있으면 상한이 막으려는
/// 증폭이 티어와 무관하게 되돌아온다.
fn required_configuration_target(args: &Value) -> Result<String, JsonRpcError> {
    let target = required_str(args, "configuration")?;
    require_length("configuration", target, MAX_NAME_CHARS)?;
    Ok(target.to_string())
}

fn require_length(key: &str, value: &str, limit: usize) -> Result<(), JsonRpcError> {
    if value.chars().count() > limit {
        return Err(invalid_params(format!(
            "{key} must be at most {limit} characters"
        )));
    }
    Ok(())
}

/// 작업 디렉터리. 두 타입에서만 빈 값을 받아 준다.
///
/// Compound: `run_compound`가 멤버의 구성을 각자 실행하므로 compound 자신의 디렉터리는 어느
/// 실행에도 닿지 않고, GUI 편집기도 그래서 이 입력을 compound에 표시하지 않는다
/// (`view_configuration_editor`). 필수로 두면 에이전트가 쓰이지도 않을 경로를 지어내야 한다.
///
/// Node: 값을 이 필드에서 받지 않고 `project_directory`에서 파생시킨다
/// (`derive_node_working_directory`) — 무엇을 실어도 덮이므로 요구하지 않는다.
///
/// 나머지 타입에서 빈 값을 막는 것은 그것이 프로세스의 cwd이기 때문이다
/// (`cmd.current_dir(&config.working_directory)`). 결과 타입이 이 패치만으로 결정되지 않는
/// update 경로는 앱 계층이 같은 불변식을 다시 본다(`check_working_directory`).
fn require_working_directory(value: &str, type_data: &ConfigTypeData) -> Result<(), JsonRpcError> {
    if matches!(
        type_data,
        ConfigTypeData::Compound { .. } | ConfigTypeData::Node { .. }
    ) {
        return require_length("working_directory", value, MAX_PATH_CHARS);
    }
    require_bounded("working_directory", value, MAX_PATH_CHARS)
}

/// 새 이름 또는 개명할 이름. 상한에 더해 UUID 모양을 거절한다 — `find_configuration_index`는
/// 대상이 UUID로 파싱되면 이름 조회를 아예 하지 않으므로, UUID 모양 이름을 가진 구성은 이후
/// `configuration` 인자로 지목할 수 없다. 이름을 주소로 쓰는 계약(`check_name_available`이 그
/// 근거로 유일성을 강제한다)에서 부를 수 없는 이름은 이름이 아니다.
fn require_configuration_name(key: &str, value: &str) -> Result<(), JsonRpcError> {
    require_bounded(key, value, MAX_NAME_CHARS)?;
    if Uuid::parse_str(value).is_ok() {
        return Err(invalid_params(format!(
            "{key} must not look like a UUID; a configuration with a UUID-shaped name cannot be \
             addressed by name afterwards"
        )));
    }
    Ok(())
}

/// `type_data` 안쪽 문자열·목록의 상한. 스키마는 이 필드들을 선언하지 않으므로
/// (`type_data_schema`의 사유) 클라이언트 쪽 검증이 없고, 값은 저장소에 영속되고 이후 모든
/// 조회·편집 응답에 되돌아오고 매 프레임 편집기에 렌더되며 일부는 그대로 실행된다 —
/// `MAX_NAME_CHARS` 외 상한들이 막는 것과 같은 증폭이다.
///
/// 모든 변형의 모든 필드를 `..` 없이 풀어 쓰는 것이 이 함수의 커버리지 보증이다. 스키마가 훑을
/// 수 있는 표면이 아니므로 `no_declared_string_is_missing_from_the_bound_table`이 여기까지
/// 보지 못하는데, 세 열거형에 필드를 더하면 이 `match`에서 **컴파일이** 실패한다. 다만 그 보증의
/// 범위는 필드를 **언급하게** 하는 것까지다 — `new_field: _`로 묶으면 컴파일은 통과하므로,
/// 상한을 실제로 거는 것은 여기서 강제되지 않는 사람의 판단이다.
fn require_bounded_type_data(type_data: &ConfigTypeData) -> Result<(), JsonRpcError> {
    match type_data {
        ConfigTypeData::Application { command, arguments } => {
            require_length("command", command, MAX_PATH_CHARS)?;
            require_length("arguments", arguments, MAX_CONFIG_TEXT_CHARS)
        }
        ConfigTypeData::ShellScript { execute_mode } => match execute_mode {
            ExecuteMode::ScriptFile {
                script_path,
                script_options,
                interpreter_path,
                interpreter_options,
            } => {
                require_length("script_path", script_path, MAX_PATH_CHARS)?;
                require_length("script_options", script_options, MAX_CONFIG_TEXT_CHARS)?;
                require_optional_length(
                    "interpreter_path",
                    interpreter_path.as_deref(),
                    MAX_PATH_CHARS,
                )?;
                require_optional_length(
                    "interpreter_options",
                    interpreter_options.as_deref(),
                    MAX_CONFIG_TEXT_CHARS,
                )
            }
            ExecuteMode::ScriptText { script_text } => {
                require_length("script_text", script_text, MAX_CONFIG_TEXT_CHARS)
            }
        },
        ConfigTypeData::Node {
            project_directory,
            package_manager: _,
            node_runtime_path,
            command: _,
            script_name,
            arguments,
            node_options,
        } => {
            require_length("project_directory", project_directory, MAX_PATH_CHARS)?;
            require_optional_length(
                "node_runtime_path",
                node_runtime_path.as_deref(),
                MAX_PATH_CHARS,
            )?;
            require_optional_length("script_name", script_name.as_deref(), MAX_NAME_CHARS)?;
            require_length("arguments", arguments, MAX_CONFIG_TEXT_CHARS)?;
            require_length("node_options", node_options, MAX_CONFIG_TEXT_CHARS)
        }
        ConfigTypeData::Kotlin {
            jdk_path,
            launch_mode,
            vm_options,
            program_arguments,
        } => {
            require_optional_length("jdk_path", jdk_path.as_deref(), MAX_PATH_CHARS)?;
            match launch_mode {
                KotlinLaunchMode::MainClass {
                    main_class,
                    classpath,
                } => {
                    require_length("main_class", main_class, MAX_NAME_CHARS)?;
                    // classpath는 경로 여러 개를 플랫폼 구분자로 이은 값이므로 경로 하나의
                    // 상한으로는 묶을 수 없다.
                    require_length("classpath", classpath, MAX_CONFIG_TEXT_CHARS)?;
                }
                KotlinLaunchMode::Jar { jar_path } => {
                    require_length("jar_path", jar_path, MAX_PATH_CHARS)?;
                }
            }
            require_length("vm_options", vm_options, MAX_CONFIG_TEXT_CHARS)?;
            require_length(
                "program_arguments",
                program_arguments,
                MAX_CONFIG_TEXT_CHARS,
            )
        }
        ConfigTypeData::Compound { members, workspace } => {
            if members.len() > MAX_COMPOUND_MEMBERS {
                return Err(invalid_params(format!(
                    "members must list at most {MAX_COMPOUND_MEMBERS} configurations"
                )));
            }
            require_optional_length("workspace", workspace.as_deref(), MAX_NAME_CHARS)
        }
    }
}

/// 비어 있음이 유효한 상태인 필드의 길이 상한. `require_bounded`와 갈리는 이유는 `None`과 빈
/// 문자열이 여기서는 "지정하지 않았다"를 뜻한다는 것이다.
fn require_optional_length(
    key: &str,
    value: Option<&str>,
    limit: usize,
) -> Result<(), JsonRpcError> {
    match value {
        Some(text) => require_length(key, text, limit),
        None => Ok(()),
    }
}

/// 환경변수 맵의 이름·값에 상한을 적용한다. 오류 문구가 키를 싣기 때문에 키 자체의 상한을
/// 먼저 본다 — 그러지 않으면 1 MiB짜리 키가 그만큼의 오류 메시지로 되돌아간다.
fn require_bounded_env(
    map: &std::collections::HashMap<String, String>,
) -> Result<(), JsonRpcError> {
    for (key, value) in map {
        require_bounded("environment variable name", key, MAX_ENV_KEY_CHARS)?;
        if value.chars().count() > MAX_ENV_VALUE_CHARS {
            return Err(invalid_params(format!(
                "environment variable '{key}' must be at most {MAX_ENV_VALUE_CHARS} characters"
            )));
        }
    }
    Ok(())
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

/// 실행 툴의 `request_id` — 원시 JSON에서 직접 읽는다.
fn optional_request_id(args: &Value) -> Result<Option<String>, JsonRpcError> {
    match args.get("request_id") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => {
            validate_request_id(Some(s))?;
            Ok(Some(s.clone()))
        }
        Some(_) => Err(invalid_params("request_id must be a string")),
    }
}

/// `request_id` 값 자체의 규칙. 실행 툴은 원시 JSON에서, 편집 툴은 serde로 읽은 뒤 이 함수를
/// 부른다 — 규칙이 두 군데로 갈라지면 같은 값이 툴에 따라 통과·거절되어 멱등성 계약이 툴별로
/// 달라진다. 길이를 거르는 것은 이 값이 중복 흡수 창이 닫힐 때까지 남기 때문이다.
fn validate_request_id(request_id: Option<&str>) -> Result<(), JsonRpcError> {
    let Some(value) = request_id else {
        return Ok(());
    };
    if value.trim().is_empty() {
        return Err(invalid_params("request_id must not be empty"));
    }
    if value.chars().count() > MAX_REQUEST_ID_CHARS {
        return Err(invalid_params(format!(
            "request_id must be at most {MAX_REQUEST_ID_CHARS} characters"
        )));
    }
    Ok(())
}

/// 한 줄 입력. 빈 문자열은 허용한다 — 대화형 프롬프트에 Enter만 보내는 것이 유효한 응답이다.
/// 개행은 거부한다: 전송 계층이 줄 종결자 하나만 붙이므로, 담긴 개행은 한 번의 호출을 여러
/// 줄 입력으로 조용히 바꿔 버린다.
fn required_single_line<'a>(args: &'a Value, key: &str) -> Result<&'a str, JsonRpcError> {
    match args.get(key) {
        Some(Value::String(s)) if s.contains('\n') || s.contains('\r') => Err(invalid_params(
            format!("{key} must be a single line; send one call per line"),
        )),
        Some(Value::String(s)) if s.len() > MAX_INPUT_LINE_BYTES => Err(invalid_params(format!(
            "{key} must be at most {MAX_INPUT_LINE_BYTES} bytes"
        ))),
        Some(Value::String(s)) => Ok(s.as_str()),
        Some(_) => Err(invalid_params(format!("{key} must be a string"))),
        None => Err(invalid_params(format!("Missing required argument: {key}"))),
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
    use crate::models::{NodeCommand, PackageManager, RunConfiguration};

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
    fn read_only_hides_the_execute_tools() {
        // tier 게이트의 1차 방어선 — 목록에 보이면 에이전트가 호출을 시도하고, 거부는
        // 사용자 눈에 실패로 남는다.
        let names = tool_names(McpPermission::ReadOnly);
        for hidden in [
            "run_configuration",
            "stop_session",
            "rerun_session",
            "send_session_input",
        ] {
            assert!(
                !names.contains(&hidden.to_string()),
                "{hidden} must be hidden"
            );
        }
    }

    #[test]
    fn execute_tier_exposes_the_execute_tools() {
        let names = tool_names(McpPermission::Execute);
        for shown in [
            "run_configuration",
            "stop_session",
            "rerun_session",
            "send_session_input",
            "list_configurations",
        ] {
            assert!(names.contains(&shown.to_string()), "{shown} must be listed");
        }
    }

    #[test]
    fn execute_ops_report_the_execute_tier() {
        // `tools/call` 재검사가 읽는 값 — 목록 필터와 어긋나면 숨긴 툴이 호출로 통과한다.
        let session_id = Uuid::new_v4();
        for op in [
            McpOp::RunConfiguration {
                target: "build".to_string(),
                request_id: None,
            },
            McpOp::StopSession { session_id },
            McpOp::RerunSession {
                session_id,
                request_id: None,
            },
            McpOp::SendSessionInput {
                session_id,
                text: "y".to_string(),
                request_id: None,
            },
        ] {
            assert_eq!(op.required_tier(), McpPermission::Execute, "{op:?}");
        }
    }

    #[test]
    fn run_configuration_takes_an_optional_request_id() {
        let without = parse_call("run_configuration", &json!({ "configuration": "build" }))
            .expect("configuration alone is enough");
        assert_eq!(
            without,
            McpOp::RunConfiguration {
                target: "build".to_string(),
                request_id: None,
            }
        );

        let with = parse_call(
            "run_configuration",
            &json!({ "configuration": "build", "request_id": "retry-1" }),
        )
        .expect("request_id is accepted");
        assert_eq!(
            with,
            McpOp::RunConfiguration {
                target: "build".to_string(),
                request_id: Some("retry-1".to_string()),
            }
        );
    }

    #[test]
    fn an_oversized_request_id_is_rejected() {
        // 이 값은 요청보다 오래 남는다 — 1 MiB body를 통과한 문자열을 60초 동안 보관하지 않는다.
        let err = parse_call(
            "run_configuration",
            &json!({
                "configuration": "build",
                "request_id": "x".repeat(MAX_REQUEST_ID_CHARS + 1),
            }),
        )
        .expect_err("an unbounded request_id must not be stored");
        assert_eq!(err.code, INVALID_PARAMS);
    }

    #[test]
    fn a_blank_request_id_is_rejected() {
        // 공백만 담긴 id는 모든 재시도가 같은 키를 쓰게 만들어, 서로 다른 실행을 한 창에 묶는다.
        let err = parse_call(
            "run_configuration",
            &json!({ "configuration": "build", "request_id": "   " }),
        )
        .expect_err("a blank request_id must not become a dedup key");
        assert_eq!(err.code, INVALID_PARAMS);
    }

    #[test]
    fn session_input_rejects_embedded_newlines() {
        // 전송 계층은 줄 종결자를 하나만 붙인다 — 담긴 개행은 한 호출을 여러 줄 입력으로 바꾼다.
        let session_id = Uuid::new_v4().to_string();
        for text in ["yes\nno", "yes\r"] {
            let err = parse_call(
                "send_session_input",
                &json!({ "session_id": session_id, "text": text }),
            )
            .expect_err("multi-line input must be rejected");
            assert_eq!(err.code, INVALID_PARAMS, "{text:?}");
        }
    }

    #[test]
    fn an_oversized_input_line_is_rejected() {
        // cooked 터미널이 받지 못하는 길이다. 그대로 내려보내면 PTY 쓰기가 블록한 채
        // `spawn_blocking` 스레드와 stdin 락을 놓지 않아 그 세션의 입력이 영구히 막힌다.
        let session_id = Uuid::new_v4().to_string();
        let err = parse_call(
            "send_session_input",
            &json!({
                "session_id": session_id,
                "text": "x".repeat(MAX_INPUT_LINE_BYTES + 1),
            }),
        )
        .expect_err("a line no terminal can accept must not reach the writer");
        assert_eq!(err.code, INVALID_PARAMS);

        parse_call(
            "send_session_input",
            &json!({
                "session_id": session_id,
                "text": "x".repeat(MAX_INPUT_LINE_BYTES),
            }),
        )
        .expect("the cap itself is allowed");
    }

    #[test]
    fn session_input_accepts_an_empty_line() {
        // 대화형 프롬프트에 Enter만 보내는 것은 유효한 응답이다.
        let session_id = Uuid::new_v4();
        let op = parse_call(
            "send_session_input",
            &json!({ "session_id": session_id.to_string(), "text": "" }),
        )
        .expect("an empty line is a real answer");
        assert_eq!(
            op,
            McpOp::SendSessionInput {
                session_id,
                text: String::new(),
                request_id: None,
            }
        );
    }

    #[test]
    fn send_session_input_takes_an_optional_request_id() {
        let session_id = Uuid::new_v4();
        let op = parse_call(
            "send_session_input",
            &json!({ "session_id": session_id.to_string(), "text": "y", "request_id": "retry-1" }),
        )
        .expect("request_id is accepted");

        assert_eq!(
            op,
            McpOp::SendSessionInput {
                session_id,
                text: "y".to_string(),
                request_id: Some("retry-1".to_string()),
            }
        );
    }

    #[test]
    fn a_non_string_request_id_is_rejected() {
        // JSON 스키마를 무시하는 클라이언트가 있어도 파싱이 유일한 관문이다.
        let err = parse_call(
            "run_configuration",
            &json!({ "configuration": "build", "request_id": 7 }),
        )
        .expect_err("a number is not an id");

        assert!(err.message.contains("must be a string"), "{}", err.message);
    }

    #[test]
    fn a_non_string_input_line_is_rejected() {
        let err = parse_call(
            "send_session_input",
            &json!({ "session_id": Uuid::new_v4().to_string(), "text": true }),
        )
        .expect_err("a boolean is not a line");

        assert!(err.message.contains("must be a string"), "{}", err.message);
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
            "run_configuration" => json!({ "configuration": "build" }),
            "stop_session" | "rerun_session" => json!({ "session_id": session_id }),
            "send_session_input" => json!({ "session_id": session_id, "text": "y" }),
            "create_configuration" => json!({
                "name": "web",
                "working_directory": "/tmp/web",
                "type_data": { "type": "Application", "command": "echo", "arguments": "" },
            }),
            "update_configuration" => json!({ "configuration": "build", "name": "web" }),
            "delete_configuration" => json!({ "configuration": "build" }),
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
        // `MAX_QUERY_CHARS`가 통과시키는 최대 패턴을 그대로 쓴다 — 상한이 이 절단을 대신하지
        // 않는다는 것이 이 테스트가 지키는 사실이다.
        let session_id = Uuid::new_v4().to_string();
        let query = format!("{}(", "가".repeat(MAX_QUERY_CHARS - 1));
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

    fn create_args() -> Value {
        json!({
            "name": "web",
            "working_directory": "/tmp/web",
            "type_data": { "type": "Application", "command": "echo", "arguments": "" },
        })
    }

    #[test]
    fn edit_tools_appear_only_at_the_edit_tier() {
        let execute = tool_names(McpPermission::Execute);
        let edit = tool_names(McpPermission::Edit);
        for name in [
            "create_configuration",
            "update_configuration",
            "delete_configuration",
        ] {
            let name = name.to_string();
            assert!(!execute.contains(&name), "{name} must be hidden at Execute");
            assert!(edit.contains(&name), "{name} must appear at Edit");
        }
    }

    #[test]
    fn a_configuration_read_back_is_a_valid_write() {
        // 읽기 응답의 `type_data`를 그대로 고쳐 보내는 것이 이 API의 주 사용법이다. 두 표현이
        // 갈라지면 에이전트는 방금 읽은 값을 되돌려 쓸 수 없고, 타입별 필드를 손으로 옮기는
        // 스키마가 모델보다 뒤처지기 시작한다.
        let config = RunConfiguration {
            type_data: ConfigTypeData::Node {
                project_directory: String::from("/tmp/web"),
                package_manager: PackageManager::Pnpm,
                node_runtime_path: None,
                command: NodeCommand::default(),
                script_name: Some(String::from("dev")),
                arguments: String::new(),
                node_options: String::new(),
            },
            ..RunConfiguration::default()
        };
        let read_shape = serde_json::to_value(&config).expect("serialize");

        let mut args = create_args();
        args["type_data"] = read_shape["type_data"].clone();
        let op = parse_call("create_configuration", &args)
            .expect("the read shape must parse as a write");

        let McpOp::CreateConfiguration(created) = op else {
            panic!("expected a create op");
        };
        assert_eq!(created.type_data, config.type_data);
    }

    #[test]
    fn creating_a_configuration_needs_an_explicit_working_directory() {
        // GUI는 "."을 사용자가 편집하는 자리표시자로 보여 주지만, 에이전트에게 "."은 앱
        // 프로세스의 cwd — 이 API로는 관측할 수 없는 값이다. 조용히 기본값을 주면 프로그램이
        // 호출자가 의도하지 않은 디렉터리에서 돈다.
        let mut args = create_args();
        args.as_object_mut()
            .expect("object")
            .remove("working_directory");

        let err = parse_call("create_configuration", &args)
            .expect_err("working_directory must be required");

        assert_eq!(err.code, INVALID_PARAMS);
        assert!(err.message.contains("working_directory"), "{}", err.message);
    }

    #[test]
    fn a_create_call_cannot_choose_the_new_id() {
        // 조용히 무시하면 호출자는 자기가 정한 id로 구성이 생겼다고 믿고, 그 id로 실행을
        // 요청하다 계속 "구성 없음"을 받는다.
        let mut args = create_args();
        args["id"] = json!(Uuid::new_v4().to_string());

        let err = parse_call("create_configuration", &args)
            .expect_err("id must be rejected, not ignored");

        assert_eq!(err.code, INVALID_PARAMS);
        assert!(err.message.contains("id"), "{}", err.message);
    }

    #[test]
    fn an_update_that_changes_nothing_is_rejected() {
        // 바꿀 것이 없는 호출에 성공으로 답하면 호출자는 바뀌지 않은 구성을 바뀐 것으로 읽는다.
        let err = parse_call("update_configuration", &json!({ "configuration": "build" }))
            .expect_err("a no-op update must not report success");

        assert_eq!(err.code, INVALID_PARAMS);
    }

    #[test]
    fn an_update_cannot_change_the_id() {
        // 다른 필드를 함께 실어, 거절이 "바꿀 것이 없다"가 아니라 `id` 때문임을 못 박는다.
        let err = parse_call(
            "update_configuration",
            &json!({
                "configuration": "build",
                "name": "web",
                "id": Uuid::new_v4().to_string(),
            }),
        )
        .expect_err("id must be immutable");

        assert_eq!(err.code, INVALID_PARAMS);
        assert!(err.message.contains("id"), "{}", err.message);
    }

    #[test]
    fn a_typo_in_a_type_data_field_is_rejected_instead_of_defaulting() {
        // `members`가 `#[serde(default)]`이므로 `deny_unknown_fields` 없이는 오타 하나가 빈
        // 벡터로 조용히 파싱되어, 사용자의 compound가 멤버 0개로 교체되고 호출자는 성공을 받는다.
        for typo in ["member", "Members"] {
            let args = json!({
                "configuration": "bundle",
                "type_data": { "type": "Compound", typo: [Uuid::new_v4().to_string()] },
            });

            let err = parse_call("update_configuration", &args)
                .expect_err("오타 필드는 기본값으로 파싱되지 않고 거절돼야 한다");

            assert_eq!(err.code, INVALID_PARAMS, "{typo}");
            assert!(err.message.contains(typo), "{typo}: {}", err.message);
        }
    }

    #[test]
    fn a_uuid_shaped_name_is_rejected() {
        // 조회는 UUID로 파싱되는 대상을 이름으로 찾지 않는다(`find_configuration`) — 그런 이름을
        // 허용하면 그 구성은 자기 이름으로 다시 지목할 수 없다.
        let mut args = create_args();
        args["name"] = json!(Uuid::new_v4().to_string());

        let err = parse_call("create_configuration", &args).expect_err("UUID 이름은 거절");

        assert_eq!(err.code, INVALID_PARAMS);
        assert!(err.message.contains("UUID"), "{}", err.message);
    }

    #[test]
    fn a_compound_may_be_created_without_a_working_directory() {
        // compound 자신의 디렉터리는 어느 실행에도 닿지 않고(`run_compound`가 멤버의 구성을 각자
        // 실행한다) GUI는 그 입력을 표시하지 않는다 — 필수로 두면 지어낸 경로만 저장된다.
        let args = json!({
            "name": "bundle",
            "working_directory": "",
            "type_data": { "type": "Compound", "members": [], "workspace": null },
        });

        assert!(parse_call("create_configuration", &args).is_ok());
    }

    #[test]
    fn an_application_still_needs_a_working_directory() {
        let mut args = create_args();
        args["working_directory"] = json!("");

        let err = parse_call("create_configuration", &args).expect_err("빈 cwd는 실행을 깨뜨린다");

        assert!(err.message.contains("working_directory"), "{}", err.message);
    }

    /// 호출자 문자열이 상태바·디스크·이후 모든 응답으로 증폭되는 것을 막는 상한들. 한 요청은
    /// body 상한(1 MiB)까지 실어 올 수 있으므로, 필드마다 상한이 없으면 그만큼이 앱 상태에 남는다.
    /// 스키마가 선언한 호출자 문자열 하나의 상한 계약. 종류는 셋뿐이고, 새 문자열 속성은 반드시
    /// 어느 하나에 들어간다 — 빠지면 `every_caller_string_declares_its_bound`가 멈춘다.
    #[derive(Debug, Clone, Copy)]
    enum StringBound {
        /// 문자 수 상한. 스키마의 `maxLength`가 파서가 세는 값과 **같아야** 한다 — 갈리면 한쪽
        /// 검증을 통과한 요청이 다른 쪽에서 거절된다.
        Chars(usize),
        /// 형식 검사. UUID 파싱은 어떤 길이 상한보다 강하고 실패 문구가 값을 되풀이하지 않으므로
        /// `maxLength`를 싣지 않는다 — 실으면 `Uuid::parse_str`이 받아 주는 다른 표기(simple 32자,
        /// urn 45자)를 클라이언트가 먼저 거절한다.
        Format,
        /// 바이트 상한. JSON Schema의 `maxLength`는 **문자** 수이므로 실을 수 없다 — 실으면 CJK
        /// 입력이 로컬 검증을 통과한 뒤 서버에서 거절된다(D27이 겪은 실패다).
        Bytes(usize),
    }

    /// 문자열이 속성 안에서 앉은 자리. 상한을 스키마의 어디에서 읽고 값을 인자의 어디에 넣는지가
    /// 함께 정해진다.
    #[derive(Debug, Clone, Copy)]
    enum Slot {
        Value,
        Item,
        Key,
        Entry,
    }

    /// 스키마가 선언한 호출자 문자열 전부. 카탈로그를 훑는 완결성 검사가 이 표를 기준으로
    /// 판정하므로, 문자열 속성을 늘리고 행을 빠뜨리면 테스트가 멈춘다.
    const DECLARED_STRINGS: &[(&str, &str, Slot, StringBound)] = &[
        (
            "get_configuration",
            "configuration",
            Slot::Value,
            StringBound::Chars(MAX_NAME_CHARS),
        ),
        (
            "read_session_output",
            "session_id",
            Slot::Value,
            StringBound::Format,
        ),
        (
            "search_session_output",
            "session_id",
            Slot::Value,
            StringBound::Format,
        ),
        (
            "search_session_output",
            "query",
            Slot::Value,
            StringBound::Chars(MAX_QUERY_CHARS),
        ),
        (
            "run_configuration",
            "configuration",
            Slot::Value,
            StringBound::Chars(MAX_NAME_CHARS),
        ),
        (
            "run_configuration",
            "request_id",
            Slot::Value,
            StringBound::Chars(MAX_REQUEST_ID_CHARS),
        ),
        (
            "stop_session",
            "session_id",
            Slot::Value,
            StringBound::Format,
        ),
        (
            "rerun_session",
            "session_id",
            Slot::Value,
            StringBound::Format,
        ),
        (
            "rerun_session",
            "request_id",
            Slot::Value,
            StringBound::Chars(MAX_REQUEST_ID_CHARS),
        ),
        (
            "send_session_input",
            "session_id",
            Slot::Value,
            StringBound::Format,
        ),
        (
            "send_session_input",
            "text",
            Slot::Value,
            StringBound::Bytes(MAX_INPUT_LINE_BYTES),
        ),
        (
            "send_session_input",
            "request_id",
            Slot::Value,
            StringBound::Chars(MAX_REQUEST_ID_CHARS),
        ),
        (
            "create_configuration",
            "name",
            Slot::Value,
            StringBound::Chars(MAX_NAME_CHARS),
        ),
        (
            "create_configuration",
            "working_directory",
            Slot::Value,
            StringBound::Chars(MAX_PATH_CHARS),
        ),
        (
            "create_configuration",
            "environment_variables",
            Slot::Key,
            StringBound::Chars(MAX_ENV_KEY_CHARS),
        ),
        (
            "create_configuration",
            "environment_variables",
            Slot::Entry,
            StringBound::Chars(MAX_ENV_VALUE_CHARS),
        ),
        (
            "create_configuration",
            "request_id",
            Slot::Value,
            StringBound::Chars(MAX_REQUEST_ID_CHARS),
        ),
        (
            "update_configuration",
            "configuration",
            Slot::Value,
            StringBound::Chars(MAX_NAME_CHARS),
        ),
        (
            "update_configuration",
            "name",
            Slot::Value,
            StringBound::Chars(MAX_NAME_CHARS),
        ),
        (
            "update_configuration",
            "working_directory",
            Slot::Value,
            StringBound::Chars(MAX_PATH_CHARS),
        ),
        (
            "update_configuration",
            "set_environment_variables",
            Slot::Key,
            StringBound::Chars(MAX_ENV_KEY_CHARS),
        ),
        (
            "update_configuration",
            "set_environment_variables",
            Slot::Entry,
            StringBound::Chars(MAX_ENV_VALUE_CHARS),
        ),
        (
            "update_configuration",
            "remove_environment_variables",
            Slot::Item,
            StringBound::Chars(MAX_ENV_KEY_CHARS),
        ),
        (
            "update_configuration",
            "request_id",
            Slot::Value,
            StringBound::Chars(MAX_REQUEST_ID_CHARS),
        ),
        (
            "delete_configuration",
            "configuration",
            Slot::Value,
            StringBound::Chars(MAX_NAME_CHARS),
        ),
        (
            "delete_configuration",
            "request_id",
            Slot::Value,
            StringBound::Chars(MAX_REQUEST_ID_CHARS),
        ),
    ];

    fn declared_max(schema: &Value, property: &str, slot: Slot) -> Option<u64> {
        let spec = &schema["properties"][property];
        let at = match slot {
            Slot::Value => &spec["maxLength"],
            Slot::Item => &spec["items"]["maxLength"],
            Slot::Key => &spec["propertyNames"]["maxLength"],
            Slot::Entry => &spec["additionalProperties"]["maxLength"],
        };
        at.as_u64()
    }

    fn with_string(args: &mut Value, property: &str, slot: Slot, value: String) {
        args[property] = match slot {
            Slot::Value => json!(value),
            Slot::Item => json!([value]),
            Slot::Key => json!({ value: "v" }),
            Slot::Entry => json!({ "K": value }),
        };
    }

    #[test]
    fn every_caller_string_declares_its_bound() {
        // D51·D64가 상한을 하나씩 채워 넣던 자리를 일반화한다. 두 행이 남긴 실패 양식은 같았다 —
        // 상한이 **어떤 문자열에 걸려 있는지**를 사람이 세고 있었고, 툴이 늘 때마다 빠진 곳이
        // 나왔다. 여기서는 카탈로그가 선언한 문자열을 훑어 표와 맞추므로 세는 일이 없어진다.
        for (tool, property, slot, bound) in DECLARED_STRINGS {
            let schema = (find(tool).expect(tool).schema)();
            let over = match bound {
                StringBound::Chars(limit) => {
                    assert_eq!(
                        declared_max(&schema, property, *slot),
                        Some(*limit as u64),
                        "{tool}.{property}: 스키마 maxLength가 파서 상한과 다르다"
                    );
                    "가".repeat(limit + 1)
                }
                StringBound::Format | StringBound::Bytes(_) => {
                    assert_eq!(
                        declared_max(&schema, property, *slot),
                        None,
                        "{tool}.{property}: 문자 수로 옮길 수 없는 상한을 maxLength로 실었다"
                    );
                    match bound {
                        StringBound::Bytes(limit) => "a".repeat(limit + 1),
                        _ => "not-a-uuid".repeat(4),
                    }
                }
            };

            let mut args = sample_args(tool);
            with_string(&mut args, property, *slot, over);
            assert!(
                parse_call(tool, &args).is_err(),
                "{tool}.{property}: 상한을 넘긴 값이 통과했다"
            );

            // 상한이 문자 수라는 사실은 위쪽 단정만으로는 서지 않는다 — 파서가 바이트를 세도
            // 멀티바이트 초과값은 거절되기 때문이다. 정확히 상한만큼의 멀티바이트가 통과해야
            // 파서와 스키마가 같은 단위를 센다(D51(c)).
            if let StringBound::Chars(limit) = bound {
                let mut args = sample_args(tool);
                with_string(&mut args, property, *slot, "가".repeat(*limit));
                assert!(
                    parse_call(tool, &args).is_ok(),
                    "{tool}.{property}: 상한 안의 멀티바이트 값이 거절됐다"
                );
            }
        }
    }

    #[test]
    fn no_declared_string_is_missing_from_the_bound_table() {
        // 위 테스트는 표를 훑으므로 표에서 빠진 속성을 볼 수 없다. 이쪽이 카탈로그를 훑어
        // 그 방향을 막는다 — 문자열 속성을 늘리고 표에 적지 않으면 여기서 멈춘다.
        for tool in TOOLS {
            let schema = (tool.schema)();
            let Some(properties) = schema["properties"].as_object() else {
                continue;
            };
            for (key, spec) in properties {
                let slots: &[Slot] = match spec["type"].as_str() {
                    Some("string") => &[Slot::Value],
                    Some("array") if spec["items"]["type"] == "string" => &[Slot::Item],
                    Some("object") if spec["additionalProperties"]["type"] == "string" => {
                        &[Slot::Key, Slot::Entry]
                    }
                    Some("object") => {
                        // `type_data`만 여기 온다 — 스키마가 안쪽 필드를 선언하지 않으므로
                        // (`type_data_schema`의 사유) 훑어서 볼 수 있는 문자열이 없다. 그 표면을
                        // 지키는 것은 테스트가 아니라 컴파일러다: `require_bounded_type_data`가
                        // 세 열거형의 모든 변형을 `..` 없이 풀어 써서, 필드를 더하면 컴파일이
                        // 실패한다 — 필드를 언급하게 하는 데까지이고 상한을 걸었는지는 보지 않는다.
                        assert_eq!(
                            key, "type_data",
                            "{}.{key}: 문자열 상한을 훑을 수 없는 객체",
                            tool.name
                        );
                        continue;
                    }
                    other => {
                        // 남는 것은 상한이 필요 없는 스칼라뿐이다. 문자열을 품을 수 있는 새
                        // 모양(문자열 아닌 `items`의 배열, 중첩 객체)이 이 arm으로 흘러들면
                        // 표를 훑는 위 검사가 조용히 건너뛰므로 여기서 멈춘다.
                        assert!(
                            matches!(other, Some("boolean" | "integer" | "number")),
                            "{}.{key}: 훑을 수 없는 모양 {other:?} — 문자열을 품으면 \
                             DECLARED_STRINGS에 올리고 아니면 이 목록에 더한다",
                            tool.name
                        );
                        continue;
                    }
                };
                for slot in slots {
                    assert!(
                        DECLARED_STRINGS.iter().any(|(t, p, s, _)| {
                            *t == tool.name
                                && p == key
                                && std::mem::discriminant(s) == std::mem::discriminant(slot)
                        }),
                        "{}.{key} ({slot:?})가 DECLARED_STRINGS에 없다",
                        tool.name
                    );
                }
            }
        }
    }

    #[test]
    fn the_guide_restates_the_bounds_from_the_constants() {
        // 스키마 `description`은 값을 상수에서 보간할 수 있지만(D80) Markdown은 컴파일되지 않아
        // 가이드의 숫자는 손으로 적힌다 — 이 테스트가 그 유일한 가드다. 표식은 표 라벨과 코드
        // 스팬이고 판정은 그 줄이 상수의 십진 표기와 그 단위를 싣는지만 보므로, 문장을 다시 쓰는
        // 편집으로는 깨지지 않는다. 값이 여러 상수에 겹쳐도(4096이 셋) 행마다 자기 상수와 맞추므로
        // 공허하게 통과하지 않는다.
        const GUIDE: &str = include_str!("../../../docs/MCP.md");

        let documented: [(&str, &[usize], &str); 13] = [
            ("| `name` · `configuration` |", &[MAX_NAME_CHARS], "자"),
            ("| `working_directory` |", &[MAX_PATH_CHARS], "자"),
            ("| 환경변수 이름 |", &[MAX_ENV_KEY_CHARS], "자"),
            ("| 환경변수 값 |", &[MAX_ENV_VALUE_CHARS], "자"),
            ("| `request_id` |", &[MAX_REQUEST_ID_CHARS], "자"),
            (
                "`search_session_output`의 `query` |",
                &[MAX_QUERY_CHARS],
                "자",
            ),
            (
                "`send_session_input`의 `text` |",
                &[MAX_INPUT_LINE_BYTES],
                "바이트",
            ),
            ("| `type_data` 안의 경로", &[MAX_PATH_CHARS], "자"),
            ("| `type_data` 안의 이름", &[MAX_NAME_CHARS], "자"),
            ("| `type_data` 안의 그 밖의", &[MAX_CONFIG_TEXT_CHARS], "자"),
            ("| compound `members` 개수", &[MAX_COMPOUND_MEMBERS], "개"),
            (
                "`tail_lines` 기본",
                &[DEFAULT_TAIL_LINES, MAX_LINES_PER_CALL],
                "줄",
            ),
            ("따르는 상한", &[MAX_SEARCH_MATCHES], "줄"),
        ];

        for (marker, bounds, unit) in documented {
            let mut hits = GUIDE.lines().filter(|line| line.contains(marker));
            let line = hits.next().unwrap_or_else(|| {
                panic!("docs/MCP.md에 `{marker}` 줄이 없다 — 표식이 바뀌었으면 이 표를 고친다")
            });
            assert!(
                hits.next().is_none(),
                "표식 `{marker}`가 여러 줄에 맞아 어느 줄을 재는지 모호하다"
            );
            for bound in bounds {
                let digits = bound.to_string();
                // 단위를 숫자에 **붙어 있는지**로 재는 것은, 줄 어딘가에 있기만 하면 되는 검사가
                // 옆 문장의 낱말과 짝지어 통과하기 때문이다. 이 형태는 숫자가 그대로인 단위 변경
                // (`바이트` → `자`, D27이 겪은 불일치)을 잡는다.
                let carries_its_unit = line.match_indices(&digits).any(|(at, _)| {
                    let leading_digit = line[..at]
                        .chars()
                        .next_back()
                        .is_some_and(|c| c.is_ascii_digit());
                    let rest = &line[at + digits.len()..];
                    !leading_digit
                        && !rest.starts_with(|c: char| c.is_ascii_digit())
                        && rest.trim_start_matches([' ', '*']).starts_with(unit)
                });
                assert!(
                    carries_its_unit,
                    "docs/MCP.md의 `{marker}` 줄이 {bound}{unit}을 싣지 않는다: {line}"
                );
            }
        }
    }

    #[test]
    fn type_data_strings_are_bounded() {
        // 스키마가 선언하지 않는 표면이라 클라이언트 쪽 검증이 없다. 값은 저장소에 영속되고
        // 이후 모든 조회·편집 응답에 되돌아오고 매 프레임 편집기에 렌더되며 일부는 그대로
        // 실행된다 — `MAX_NAME_CHARS` 외 상한들이 막는 것과 같은 증폭이다.
        let path = "가".repeat(MAX_PATH_CHARS + 1);
        let text = "가".repeat(MAX_CONFIG_TEXT_CHARS + 1);
        let name = "가".repeat(MAX_NAME_CHARS + 1);
        let members: Vec<Value> = (0..=MAX_COMPOUND_MEMBERS)
            .map(|_| json!(Uuid::new_v4().to_string()))
            .collect();

        for (label, type_data) in [
            (
                "command",
                json!({ "type": "Application", "command": path, "arguments": "" }),
            ),
            (
                "arguments",
                json!({ "type": "Application", "command": "echo", "arguments": text }),
            ),
            (
                "script_path",
                json!({
                    "type": "ShellScript",
                    "execute_mode": { "mode": "ScriptFile", "script_path": path, "script_options": "" },
                }),
            ),
            (
                "script_text",
                json!({
                    "type": "ShellScript",
                    "execute_mode": { "mode": "ScriptText", "script_text": text },
                }),
            ),
            (
                "project_directory",
                json!({
                    "type": "Node",
                    "project_directory": path,
                    "command": "Install",
                    "arguments": "",
                    "node_options": "",
                }),
            ),
            (
                "script_name",
                json!({
                    "type": "Node",
                    "project_directory": "/tmp/web",
                    "command": "Run",
                    "script_name": name,
                    "arguments": "",
                    "node_options": "",
                }),
            ),
            (
                "vm_options",
                json!({
                    "type": "Kotlin",
                    "vm_options": text,
                    "program_arguments": "",
                }),
            ),
            (
                "classpath",
                json!({
                    "type": "Kotlin",
                    "launch_mode": { "mode": "MainClass", "main_class": "M", "classpath": text },
                    "vm_options": "",
                    "program_arguments": "",
                }),
            ),
            (
                "jar_path",
                json!({
                    "type": "Kotlin",
                    "launch_mode": { "mode": "Jar", "jar_path": path },
                    "vm_options": "",
                    "program_arguments": "",
                }),
            ),
            (
                "workspace",
                json!({ "type": "Compound", "workspace": name }),
            ),
            ("members", json!({ "type": "Compound", "members": members })),
        ] {
            let mut created = create_args();
            created["type_data"] = type_data.clone();
            assert!(
                parse_call("create_configuration", &created).is_err(),
                "create {label}"
            );

            let mut patched = json!({ "configuration": "build" });
            patched["type_data"] = type_data;
            assert!(
                parse_call("update_configuration", &patched).is_err(),
                "update {label}"
            );
        }
    }

    #[test]
    fn an_env_key_over_the_limit_is_not_echoed_back_in_full() {
        // 키 상한 검사가 문구에 키를 싣기 **전에** 와야 한다 — 아니면 상한이 막으려던 증폭이
        // 오류 메시지로 그대로 되돌아온다.
        let mut args = create_args();
        let key = "K".repeat(MAX_ENV_KEY_CHARS * 4);
        args["environment_variables"] = json!({ key.clone(): "v" });

        let err = parse_call("create_configuration", &args).expect_err("긴 키는 거절");

        assert!(
            err.message.chars().count() < MAX_ENV_KEY_CHARS,
            "{} chars",
            err.message.chars().count()
        );
    }

    #[test]
    fn the_type_data_schema_lists_every_configuration_type() {
        // 스키마의 타입 목록이 모델과 갈라지면, 에이전트는 유효한 타입을 거절당한 것으로 읽는다.
        let schema = (find("create_configuration").expect("tool").schema)();
        let listed = &schema["properties"]["type_data"]["properties"]["type"]["enum"];

        let expected: Vec<Value> = ConfigurationType::ALL
            .iter()
            .map(|kind| serde_json::to_value(kind).expect("serialize"))
            .collect();

        assert_eq!(listed, &Value::Array(expected), "{schema}");
    }

    #[test]
    fn a_blank_request_id_is_rejected_on_an_edit_tool_too() {
        // 실행 툴과 편집 툴이 인자를 다른 방식으로 읽으므로, 같은 값이 툴에 따라 통과·거절되면
        // 멱등성 계약이 툴별로 갈라진다.
        let mut args = create_args();
        args["request_id"] = json!("   ");

        let err = parse_call("create_configuration", &args).expect_err("blank request_id");

        assert_eq!(err.code, INVALID_PARAMS);
        assert!(err.message.contains("request_id"), "{}", err.message);
    }

    #[test]
    fn a_blank_name_is_rejected() {
        // serde는 빈 문자열을 유효한 `String`으로 읽는다. 통과시키면 이름으로 구성을 찾는 다른
        // 툴이 그 구성을 부를 수 없고, 목록에서도 빈 줄로 보인다.
        let mut args = create_args();
        args["name"] = json!("   ");

        let err = parse_call("create_configuration", &args).expect_err("blank name");

        assert_eq!(err.code, INVALID_PARAMS);
        assert!(err.message.contains("name"), "{}", err.message);
    }

    #[test]
    fn a_pathological_parse_error_is_bounded() {
        // serde는 알 수 없는 필드 이름을 문구에 그대로 되풀이한다 — body 상한 1 MiB를 통과한
        // 문자열이 그만큼의 오류 메시지가 되어 호출자 컨텍스트를 채운다.
        let mut args = create_args();
        args[&"k".repeat(50_000)] = json!("v");

        let err = parse_call("create_configuration", &args).expect_err("unknown field");

        // 상한은 serde 문구에만 걸리므로, 우리가 앞에 붙이는 툴 이름과 생략 부호만큼 여유를 둔다.
        let bound = MAX_PARSE_ERROR_CHARS + "create_configuration: ...".chars().count();
        assert!(
            err.message.chars().count() <= bound,
            "{} chars",
            err.message.chars().count()
        );
    }
}
