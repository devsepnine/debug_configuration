//! MCP 요청을 앱 상태로 처리하는 핸들러.
//!
//! `settings_modal`과 같은 구조로, 부모 모듈(`app.rs`)의 `RunConfigManager`에 대한 `impl`을
//! 자식 모듈에 둔다. 요청은 iced update 루프 안에서 처리되므로 GUI와 같은 상태를 보고,
//! 응답은 요청에 실려온 채널로 정확히 한 번 돌려준다.

use super::RunConfigManager;
use crate::ansi::TextSegment;
use crate::messages::Message;
use crate::models::{RunConfiguration, RunSession, SessionStatusKind};
use crate::services::{
    MAX_SEARCH_MATCHES, McpEvent, McpOp, McpOutcome, McpPermission, McpRequest, McpServerConfig,
    mcp_tools_for,
};
use iced::Task;
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// 환경변수 값이 마스킹될 때 노출되는 자리표시자. 값이 있다는 사실은 알려주되 내용은 감춘다.
const MASKED_VALUE: &str = "<hidden>";

/// 리스너 상태. 설정 모달이 사용자에게 보여줄 유일한 출처다.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum McpServerStatus {
    /// 서버가 꺼져 있음.
    #[default]
    Disabled,
    /// 켜졌지만 아직 바인드 결과를 받지 못함.
    Starting,
    /// 리스닝 중 (실제 바인드된 포트).
    Listening(u16),
    /// 시작 실패 (포트 점유, 토큰 파일 접근 불가 등).
    Failed(String),
}

impl McpServerStatus {
    /// 설정 모달에 표시할 한 줄 상태.
    pub fn label(&self) -> String {
        match self {
            Self::Disabled => String::from("Disabled"),
            Self::Starting => String::from("Starting…"),
            Self::Listening(port) => format!("Listening on http://127.0.0.1:{port}/mcp"),
            Self::Failed(reason) => format!("Failed: {reason}"),
        }
    }
}

impl RunConfigManager {
    pub(super) fn handle_mcp_event(&mut self, event: McpEvent) -> Task<Message> {
        match event {
            McpEvent::Started { port } => {
                self.mcp_status = McpServerStatus::Listening(port);
                self.status_message = format!("MCP server listening on 127.0.0.1:{port}");
                Task::none()
            }
            McpEvent::BindFailed(reason) => {
                self.mcp_status = McpServerStatus::Failed(reason.clone());
                self.status_message = format!("MCP server failed to start: {reason}");
                Task::none()
            }
            McpEvent::Request(request) => self.handle_mcp_request(request),
        }
    }

    /// 요청 하나를 처리하고 응답한다.
    fn handle_mcp_request(&mut self, request: McpRequest) -> Task<Message> {
        let op = request.op().clone();

        // 권한 재검사. `tools/list`가 이미 걸러내지만, 목록을 캐시한 클라이언트가 tier 밖
        // 이름으로 호출할 수 있으므로 **호출 시점의** 권한으로 다시 판정한다. 비교 대상 tier는
        // 목록 필터와 같은 카탈로그에서 온다 (`McpOp::required_tier`).
        if let Some(denial) = permission_denial(self.mcp_permission, op.required_tier()) {
            request.respond(McpOutcome::Denied(denial));
            return Task::none();
        }

        let outcome = match op {
            McpOp::ListTools => McpOutcome::ToolList(mcp_tools_for(self.mcp_permission)),
            McpOp::ListConfigurations => McpOutcome::Payload(json!({
                "configurations": self
                    .configurations
                    .iter()
                    .map(configuration_summary)
                    .collect::<Vec<_>>(),
            })),
            McpOp::GetConfiguration { target } => {
                match find_configuration(&self.configurations, &target) {
                    Some(config) => {
                        match configuration_detail(config, self.mcp_expose_env_values) {
                            Ok(detail) => McpOutcome::Payload(detail),
                            Err(e) => McpOutcome::Failure(e),
                        }
                    }
                    None => McpOutcome::Failure(format!("No configuration matches '{target}'")),
                }
            }
            McpOp::ListSessions => McpOutcome::Payload(json!({
                "sessions": self.sessions.iter().map(session_summary).collect::<Vec<_>>(),
            })),
            McpOp::ReadSessionOutput {
                session_id,
                tail_lines,
                since_line_id,
            } => match find_session(&self.sessions, session_id) {
                Some(session) => {
                    McpOutcome::Payload(session_output(session, tail_lines, since_line_id))
                }
                None => McpOutcome::Failure(session_not_found(session_id)),
            },
            McpOp::SearchSessionOutput {
                session_id,
                query,
                regex,
            } => match find_session(&self.sessions, session_id) {
                Some(session) => McpOutcome::Payload(session_search(session, &query, regex)),
                None => McpOutcome::Failure(session_not_found(session_id)),
            },
        };

        request.respond(outcome);
        Task::none()
    }

    /// 서버가 쓸 Bearer 토큰을 확보하는 Task.
    ///
    /// 파일 I/O를 Task 안에 두는 것은 이 앱의 초기화 규약이다 — Task를 폴링하지 않는 단위
    /// 테스트가 사용자 홈의 토큰 파일을 만들지 않는다. 토큰 파일 자체도 MCP를 처음 켤 때만
    /// 생기므로, 이 기능을 쓰지 않는 사용자의 홈에는 비밀 파일이 남지 않는다.
    pub(super) fn mcp_token_task(&mut self) -> Task<Message> {
        if self.mcp_token.is_some() {
            return Task::none();
        }
        self.mcp_status = McpServerStatus::Starting;
        Task::perform(
            async { crate::services::load_or_create_token() },
            Message::McpTokenLoaded,
        )
    }

    /// 토큰 로드 결과. 실패하면 토큰이 `None`으로 남아 subscription이 서버를 띄우지 않는다
    /// — 인증 없는 리스너가 열리는 경로를 만들지 않는다.
    pub(super) fn handle_mcp_token_loaded(
        &mut self,
        result: Result<String, String>,
    ) -> Task<Message> {
        match result {
            Ok(token) => self.mcp_token = Some(token),
            Err(reason) => {
                self.status_message = format!("MCP server failed to start: {reason}");
                self.mcp_status = McpServerStatus::Failed(reason);
            }
        }
        Task::none()
    }

    /// 리스너를 띄울 설정. 서버가 켜지고 토큰이 준비된 뒤에만 `Some`이다 — 인증 없는
    /// 리스너가 열리는 경로를 만들지 않기 위해 이 함수가 유일한 판정 지점이다.
    pub(super) fn mcp_listener_config(&self) -> Option<McpServerConfig> {
        let token = self.mcp_token.clone().filter(|_| self.mcp_enabled)?;
        Some(McpServerConfig {
            port: self.mcp_port,
            token,
        })
    }

    /// MCP 상태를 배포 기본값으로 되돌린다. `RunConfigManager::new()`가 사용자의 실제 설정
    /// 파일을 읽으므로, 이걸 거치지 않는 테스트는 개발 머신의 설정에 따라 결과가 달라진다.
    #[cfg(test)]
    pub(super) fn reset_mcp_state_for_test(&mut self) {
        let defaults = crate::services::AppSettings::default();
        self.mcp_enabled = defaults.mcp_enabled;
        self.mcp_port = defaults.mcp_port;
        self.mcp_permission = defaults.mcp_permission;
        self.mcp_expose_env_values = defaults.mcp_expose_env_values;
        self.mcp_token = None;
        self.mcp_status = McpServerStatus::default();
    }

    /// 등록용 `claude mcp add` 명령. `reveal_token`이 `false`면 토큰을 마스킹해, 화면에
    /// 그대로 표시해도 비밀이 노출되지 않는다(복사 버튼만 실제 토큰을 싣는다).
    pub(super) fn mcp_add_command(&self, reveal_token: bool) -> String {
        let token = match (&self.mcp_token, reveal_token) {
            (Some(token), true) => token.clone(),
            (Some(token), false) => crate::services::mask_token(token),
            (None, _) => String::from("<token unavailable>"),
        };
        format!(
            "claude mcp add --transport http runconfig http://127.0.0.1:{}/mcp -H \"Authorization: Bearer {token}\"",
            self.mcp_listening_port()
        )
    }

    /// 안내에 쓸 포트. 리스닝 중이면 실제 바인드된 포트, 아니면 설정값.
    fn mcp_listening_port(&self) -> u16 {
        match self.mcp_status {
            McpServerStatus::Listening(port) => port,
            _ => self.mcp_port,
        }
    }
}

/// 권한 부족 시 거부 사유. 충분하면 `None`.
fn permission_denial(current: McpPermission, required: McpPermission) -> Option<String> {
    if current.allows(required) {
        return None;
    }
    Some(format!(
        "This tool needs the '{}' permission tier, but the app is set to '{}'. \
         Change it in the app's Settings.",
        required.label(),
        current.label()
    ))
}

fn session_not_found(session_id: Uuid) -> String {
    format!("No session with id {session_id}")
}

fn find_configuration<'a>(
    configurations: &'a [RunConfiguration],
    target: &str,
) -> Option<&'a RunConfiguration> {
    if let Ok(id) = Uuid::parse_str(target) {
        return configurations.iter().find(|config| config.id == id);
    }
    configurations.iter().find(|config| config.name == target)
}

fn find_session(sessions: &[RunSession], session_id: Uuid) -> Option<&RunSession> {
    sessions.iter().find(|session| session.id == session_id)
}

/// 목록용 요약. 환경변수는 **키만** 싣는다 — 목록 조회 한 번으로 모든 비밀이 새는 것을 막는다.
fn configuration_summary(config: &RunConfiguration) -> Value {
    let mut env_keys: Vec<&str> = config
        .environment_variables
        .keys()
        .map(String::as_str)
        .collect();
    // HashMap 순회는 순서가 불정이라, 같은 구성을 두 번 조회하면 순서가 달라 보인다.
    env_keys.sort_unstable();

    json!({
        "id": config.id,
        "name": config.name,
        // 저장 포맷의 판별자를 그대로 쓴다 — 상세 조회의 `type_data.type`과 어긋나면
        // 에이전트가 두 응답을 같은 타입으로 인식하지 못한다. Display("Shell Script")나
        // Debug는 표시·진단용 표현이라 와이어 값으로 쓰지 않는다.
        "type": serde_json::to_value(config.config_type()).unwrap_or(Value::Null),
        "working_directory": config.working_directory,
        "environment_variable_keys": env_keys,
    })
}

/// 상세 조회. 저장 포맷을 그대로 직렬화한 뒤 환경변수 값만 후처리한다 — 타입별 필드를
/// 손으로 옮기지 않으므로 구성 모델이 늘어나도 이 함수가 뒤처지지 않는다.
fn configuration_detail(config: &RunConfiguration, expose_env: bool) -> Result<Value, String> {
    let mut value = serde_json::to_value(config)
        .map_err(|e| format!("Failed to serialize configuration '{}': {e}", config.name))?;

    if !expose_env
        && let Some(env) = value
            .get_mut("environment_variables")
            .and_then(Value::as_object_mut)
    {
        for slot in env.values_mut() {
            *slot = json!(MASKED_VALUE);
        }
    }
    Ok(value)
}

fn session_summary(session: &RunSession) -> Value {
    json!({
        "session_id": session.id,
        "config_name": session.config_name,
        "state": state_name(session.status_kind()),
        "exit_code": session.exit_code,
        "started_at_unix_ms": unix_millis(session.started_at),
        "duration_ms": session.run_duration().map(|d| millis(d.as_millis())),
        "line_count": session.output_lines.len(),
    })
}

/// 출력 조회 결과. ANSI 스타일은 텍스트로 평탄화한다(에이전트가 읽는 건 내용이다).
fn session_output(session: &RunSession, tail_lines: usize, since_line_id: Option<usize>) -> Value {
    let selected: Vec<&(usize, Vec<TextSegment>)> = session
        .output_lines
        .iter()
        .filter(|(line_id, _)| since_line_id.is_none_or(|since| *line_id > since))
        .collect();

    let omitted = selected.len().saturating_sub(tail_lines);
    let visible = &selected[omitted..];
    let text = visible
        .iter()
        .map(|(_, segments)| line_text(segments))
        .collect::<Vec<_>>()
        .join("\n");

    json!({
        "session_id": session.id,
        "config_name": session.config_name,
        "state": state_name(session.status_kind()),
        "buffered_lines": session.output_lines.len(),
        "returned_lines": visible.len(),
        "omitted_lines": omitted,
        // 다음 호출의 since_line_id로 그대로 넘기면 이후 추가분만 받는다.
        "last_line_id": visible.last().map(|(line_id, _)| *line_id),
        "text": text,
    })
}

/// 검색 결과. 매치를 라인 단위로 접어, 같은 줄의 여러 매치가 같은 텍스트를 반복하지 않게 한다.
fn session_search(session: &RunSession, query: &str, regex: bool) -> Value {
    let matches = session.search_matches(query, regex);

    let mut lines: Vec<Value> = Vec::new();
    let mut last_line_idx: Option<usize> = None;
    let mut truncated = false;

    for hit in &matches {
        if last_line_idx == Some(hit.line_idx) {
            if let Some(entry) = lines.last_mut() {
                entry["match_count"] = json!(entry["match_count"].as_u64().unwrap_or(0) + 1);
            }
            continue;
        }
        if lines.len() >= MAX_SEARCH_MATCHES {
            truncated = true;
            break;
        }
        let Some((line_id, segments)) = session.output_lines.get(hit.line_idx) else {
            continue;
        };
        last_line_idx = Some(hit.line_idx);
        lines.push(json!({
            "line_id": line_id,
            "text": line_text(segments),
            "match_count": 1,
        }));
    }

    json!({
        "session_id": session.id,
        "query": query,
        "regex": regex,
        "total_matches": matches.len(),
        "matched_lines": lines,
        "truncated": truncated,
    })
}

fn line_text(segments: &[TextSegment]) -> String {
    segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect()
}

fn state_name(kind: SessionStatusKind) -> &'static str {
    match kind {
        SessionStatusKind::Running => "running",
        SessionStatusKind::Succeeded => "succeeded",
        SessionStatusKind::Failed(_) => "failed",
        SessionStatusKind::Stopped => "stopped",
    }
}

/// epoch 밀리초. epoch 이전 시각(시스템 시계 이상)은 `None`.
fn unix_millis(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| millis(d.as_millis()))
}

/// JSON 수는 u64까지만 안전하므로 좁힌다. u64 밀리초는 서기 5억 년까지 표현한다.
fn millis(value: u128) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ConfigTypeData, ExecuteMode};
    use std::collections::HashMap;
    use std::time::Duration;

    fn configuration(name: &str) -> RunConfiguration {
        RunConfiguration {
            name: name.to_string(),
            working_directory: "/tmp/project".to_string(),
            environment_variables: HashMap::from([
                ("API_TOKEN".to_string(), "super-secret".to_string()),
                ("MODE".to_string(), "debug".to_string()),
            ]),
            type_data: ConfigTypeData::Application {
                command: "echo".to_string(),
                arguments: "hello".to_string(),
            },
            ..RunConfiguration::default()
        }
    }

    fn session_with_output(lines: &[&str]) -> RunSession {
        let mut session = RunSession::new("build".to_string());
        for line in lines {
            session.add_output_line(line);
        }
        session
    }

    #[test]
    fn read_only_tier_denies_higher_tier_tools() {
        let denial = permission_denial(McpPermission::ReadOnly, McpPermission::Execute)
            .expect("must be denied");
        assert!(denial.contains("Execute"));
        assert!(denial.contains("Read only"));
    }

    #[test]
    fn sufficient_tier_is_not_denied() {
        assert!(permission_denial(McpPermission::Edit, McpPermission::Execute).is_none());
        assert!(permission_denial(McpPermission::ReadOnly, McpPermission::ReadOnly).is_none());
    }

    #[test]
    fn summary_exposes_env_keys_but_no_values() {
        let summary = configuration_summary(&configuration("build"));
        let rendered = summary.to_string();

        assert_eq!(summary["environment_variable_keys"][0], "API_TOKEN");
        assert_eq!(summary["environment_variable_keys"][1], "MODE");
        assert!(
            !rendered.contains("super-secret"),
            "summary must never carry env values: {rendered}"
        );
    }

    #[test]
    fn detail_masks_env_values_by_default() {
        let config = configuration("build");
        let detail = configuration_detail(&config, false).expect("serialized");

        assert_eq!(detail["environment_variables"]["API_TOKEN"], MASKED_VALUE);
        assert_eq!(detail["environment_variables"]["MODE"], MASKED_VALUE);
        assert!(!detail.to_string().contains("super-secret"));
        // 타입별 필드는 저장 포맷(내부 태그) 그대로 실린다.
        assert_eq!(detail["type_data"]["type"], "Application");
        assert_eq!(detail["type_data"]["command"], "echo");
    }

    #[test]
    fn summary_and_detail_agree_on_the_configuration_type() {
        // 두 응답이 다른 이름을 쓰면 에이전트가 같은 구성을 다른 타입으로 인식한다.
        for type_data in [
            ConfigTypeData::Application {
                command: String::new(),
                arguments: String::new(),
            },
            ConfigTypeData::ShellScript {
                execute_mode: ExecuteMode::default(),
            },
            ConfigTypeData::Compound {
                members: vec![],
                workspace: None,
            },
        ] {
            let config = RunConfiguration {
                type_data,
                ..RunConfiguration::default()
            };
            let summary = configuration_summary(&config);
            let detail = configuration_detail(&config, false).expect("serialized");

            assert!(summary["type"].is_string(), "{summary}");
            assert_eq!(summary["type"], detail["type_data"]["type"]);
        }
    }

    #[test]
    fn detail_reveals_env_values_when_the_toggle_is_on() {
        let detail = configuration_detail(&configuration("build"), true).expect("serialized");
        assert_eq!(detail["environment_variables"]["API_TOKEN"], "super-secret");
    }

    #[test]
    fn configuration_lookup_accepts_id_and_name() {
        let configs = vec![configuration("build"), configuration("test")];

        assert_eq!(
            find_configuration(&configs, "test").map(|c| c.name.as_str()),
            Some("test")
        );
        assert_eq!(
            find_configuration(&configs, &configs[0].id.to_string()).map(|c| c.id),
            Some(configs[0].id)
        );
        assert!(find_configuration(&configs, "missing").is_none());
        assert!(
            find_configuration(&configs, &Uuid::new_v4().to_string()).is_none(),
            "an unknown UUID must not fall through to name matching"
        );
    }

    #[test]
    fn session_summary_reports_status_and_duration() {
        let mut session = session_with_output(&["one", "two"]);
        session.is_running = false;
        session.exit_code = Some(2);
        session.finished_at = Some(session.started_at + Duration::from_millis(1_500));

        let summary = session_summary(&session);
        assert_eq!(summary["state"], "failed");
        assert_eq!(summary["exit_code"], 2);
        assert_eq!(summary["duration_ms"], 1_500);
        assert_eq!(summary["line_count"], 2);
        assert!(summary["started_at_unix_ms"].is_u64());
    }

    #[test]
    fn running_session_has_no_duration() {
        let summary = session_summary(&session_with_output(&["one"]));
        assert_eq!(summary["state"], "running");
        assert!(summary["duration_ms"].is_null());
        assert!(summary["exit_code"].is_null());
    }

    #[test]
    fn output_returns_the_requested_tail() {
        let session = session_with_output(&["a", "b", "c", "d"]);
        let payload = session_output(&session, 2, None);

        assert_eq!(payload["text"], "c\nd");
        assert_eq!(payload["returned_lines"], 2);
        assert_eq!(payload["omitted_lines"], 2);
        assert_eq!(payload["buffered_lines"], 4);
    }

    #[test]
    fn output_since_line_id_returns_only_new_lines() {
        let session = session_with_output(&["a", "b", "c"]);
        let first = session_output(&session, 2, None);
        let cursor = first["last_line_id"].as_u64().expect("cursor") as usize;

        let follow_up = session_output(&session, 100, Some(cursor));
        assert_eq!(follow_up["returned_lines"], 0);
        assert_eq!(follow_up["text"], "");

        let from_start = session_output(&session, 100, Some(0));
        assert_eq!(from_start["returned_lines"], 2, "line ids start at 0");
    }

    #[test]
    fn output_of_an_empty_session_is_empty() {
        let payload = session_output(&session_with_output(&[]), 10, None);
        assert_eq!(payload["text"], "");
        assert_eq!(payload["returned_lines"], 0);
        assert!(payload["last_line_id"].is_null());
    }

    #[test]
    fn search_folds_multiple_matches_on_one_line() {
        let session = session_with_output(&["error here", "fine", "error error"]);
        let payload = session_search(&session, "error", false);

        assert_eq!(payload["total_matches"], 3);
        assert_eq!(payload["matched_lines"].as_array().expect("array").len(), 2);
        assert_eq!(payload["matched_lines"][0]["match_count"], 1);
        assert_eq!(payload["matched_lines"][1]["match_count"], 2);
        assert_eq!(payload["truncated"], false);
    }

    #[test]
    fn search_supports_regex_mode() {
        let session = session_with_output(&["exit code 3", "ok"]);
        let payload = session_search(&session, r"code \d", true);
        assert_eq!(payload["total_matches"], 1);
        assert_eq!(payload["matched_lines"][0]["text"], "exit code 3");
    }

    #[test]
    fn search_without_matches_is_empty() {
        let session = session_with_output(&["all good"]);
        let payload = session_search(&session, "panic", false);
        assert_eq!(payload["total_matches"], 0);
        assert!(
            payload["matched_lines"]
                .as_array()
                .expect("array")
                .is_empty()
        );
    }

    #[test]
    fn search_caps_the_returned_lines() {
        let lines: Vec<String> = (0..MAX_SEARCH_MATCHES + 10)
            .map(|i| format!("error {i}"))
            .collect();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let payload = session_search(&session_with_output(&refs), "error", false);

        assert_eq!(
            payload["matched_lines"].as_array().expect("array").len(),
            MAX_SEARCH_MATCHES
        );
        assert_eq!(payload["truncated"], true);
        assert_eq!(payload["total_matches"], MAX_SEARCH_MATCHES + 10);
    }

    #[test]
    fn requests_are_answered_through_the_app() {
        let (mut app, _task) = RunConfigManager::new();
        app.reset_mcp_state_for_test();
        app.configurations = vec![configuration("build")];

        let (request, mut answers) = McpRequest::channel_for_test(McpOp::ListConfigurations);
        let _ = app.handle_mcp_request(request);

        match answers.try_recv().expect("responded") {
            McpOutcome::Payload(payload) => {
                assert_eq!(payload["configurations"][0]["name"], "build");
            }
            other => panic!("expected a payload, got {other:?}"),
        }
    }

    #[test]
    fn tool_list_follows_the_permission_tier() {
        let (mut app, _task) = RunConfigManager::new();
        app.reset_mcp_state_for_test();
        app.mcp_permission = McpPermission::ReadOnly;

        let (request, mut answers) = McpRequest::channel_for_test(McpOp::ListTools);
        let _ = app.handle_mcp_request(request);

        match answers.try_recv().expect("responded") {
            McpOutcome::ToolList(tools) => {
                assert!(!tools.is_empty());
                let names: Vec<&str> = tools
                    .iter()
                    .filter_map(|tool| tool["name"].as_str())
                    .collect();
                assert!(names.contains(&"list_configurations"));
            }
            other => panic!("expected a tool list, got {other:?}"),
        }
    }

    #[test]
    fn missing_session_is_a_tool_failure_not_a_panic() {
        let (mut app, _task) = RunConfigManager::new();
        app.reset_mcp_state_for_test();
        let unknown = Uuid::new_v4();

        let (request, mut answers) = McpRequest::channel_for_test(McpOp::ReadSessionOutput {
            session_id: unknown,
            tail_lines: 10,
            since_line_id: None,
        });
        let _ = app.handle_mcp_request(request);

        match answers.try_recv().expect("responded") {
            McpOutcome::Failure(message) => assert!(message.contains(&unknown.to_string())),
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[test]
    fn bind_failure_is_surfaced_to_the_user() {
        let (mut app, _task) = RunConfigManager::new();
        app.reset_mcp_state_for_test();
        let _ = app.handle_mcp_event(McpEvent::BindFailed("127.0.0.1:47355: in use".to_string()));

        assert!(matches!(app.mcp_status, McpServerStatus::Failed(_)));
        assert!(app.status_message.contains("failed to start"));
    }

    #[test]
    fn listening_event_records_the_actual_port() {
        let (mut app, _task) = RunConfigManager::new();
        app.reset_mcp_state_for_test();
        let _ = app.handle_mcp_event(McpEvent::Started { port: 51234 });

        assert_eq!(app.mcp_status, McpServerStatus::Listening(51234));
        assert!(app.status_message.contains("51234"));
        assert!(app.mcp_add_command(false).contains("127.0.0.1:51234"));
    }

    #[test]
    fn displayed_command_masks_the_token() {
        let (mut app, _task) = RunConfigManager::new();
        app.reset_mcp_state_for_test();
        let token = "f".repeat(64);
        app.mcp_token = Some(token.clone());

        let shown = app.mcp_add_command(false);
        let copied = app.mcp_add_command(true);

        assert!(!shown.contains(&token), "displayed command must be masked");
        assert!(copied.contains(&token), "copied command must be usable");
    }

    #[test]
    fn token_failure_never_opens_a_listener() {
        let (mut app, _task) = RunConfigManager::new();
        app.reset_mcp_state_for_test();
        app.mcp_enabled = true;

        let _ = app.handle_mcp_token_loaded(Err("no home directory".to_string()));

        assert!(app.mcp_listener_config().is_none());
        assert!(matches!(app.mcp_status, McpServerStatus::Failed(_)));
        assert!(app.status_message.contains("failed to start"));
    }

    #[test]
    fn an_arriving_token_lets_the_listener_start() {
        let (mut app, _task) = RunConfigManager::new();
        app.reset_mcp_state_for_test();
        app.mcp_enabled = true;
        let token = "d".repeat(64);

        let _ = app.handle_mcp_token_loaded(Ok(token.clone()));

        let config = app.mcp_listener_config().expect("listener may start");
        assert_eq!(config.token, token);
    }

    #[test]
    fn listener_config_requires_both_the_toggle_and_a_token() {
        let (mut app, _task) = RunConfigManager::new();
        app.reset_mcp_state_for_test();
        let token = "e".repeat(64);

        assert!(app.mcp_listener_config().is_none(), "off, no token");

        app.mcp_token = Some(token.clone());
        assert!(
            app.mcp_listener_config().is_none(),
            "a token alone must not open a listener"
        );

        app.mcp_token = None;
        app.mcp_enabled = true;
        assert!(
            app.mcp_listener_config().is_none(),
            "the toggle alone must not open an unauthenticated listener"
        );

        app.mcp_token = Some(token.clone());
        app.mcp_port = 50_001;
        let config = app.mcp_listener_config().expect("both conditions met");
        assert_eq!(config.port, 50_001);
        assert_eq!(config.token, token);
    }
}
