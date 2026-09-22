//! MCP 요청을 앱 상태로 처리하는 핸들러.
//!
//! `settings_modal`과 같은 구조로, 부모 모듈(`app.rs`)의 `RunConfigManager`에 대한 `impl`을
//! 자식 모듈에 둔다. 요청은 iced update 루프 안에서 처리되므로 GUI와 같은 상태를 보고,
//! 응답은 요청에 실려온 채널로 정확히 한 번 돌려준다.
//!
//! 크기 임계값 예외(`coding-standards`의 `code-thresholds.md`): 이 파일은 File Length Hard(500 LOC)를,
//! `handle_mcp_request`와 `mcp_update_configuration`은 Function Length Hard(80 LOC)와 순환 복잡도
//! Hard(15)를 함께 넘는다. 파일은 이미 티어별 핸들러와 payload 빌더로 갈라져 있어 쪼개도 얻는 것이
//! 파일 경계뿐이므로, 다음 기능을 얹기 전에 그 선으로 먼저 가른다. `handle_mcp_request`는 op마다
//! arm 하나인 디스패치 표이므로 그 복잡도는 표준의 `Unavoidable branch maps`에 든다 — 그 범주가
//! 면제하는 것은 복잡도뿐이므로, 길이는 arm 수가 op 개수를 따른다는 같은 근거로 따로 수락한다.
//! `mcp_update_configuration`은 부채다 — 모든 검증이 첫 변형보다 앞에 온다는 순서 계약과 필드별
//! `changed` 추적이 한 함수에 있어, 검증 절을 헬퍼로 빼면 그 계약이 구조로 표현되며 길이와
//! 복잡도가 함께 내려간다. 다음 변경이 이 함수에 손대는 순간 그 추출을 함께 한다.

use super::RunConfigManager;
use crate::ansi::TextSegment;
use crate::messages::Message;
use crate::models::{
    ConfigTypeData, RunConfiguration, RunSession, SessionStatusKind, StdinWriteError,
};
use crate::services::{
    CreateConfigurationArgs, DedupLookup, DedupScope, DeleteConfigurationArgs, MAX_SEARCH_MATCHES,
    MCP_DEDUP_MAX_ENTRIES, MCP_DEDUP_WINDOW, McpEvent, McpOp, McpOutcome, McpPermission,
    McpRequest, McpServerConfig, UpdateConfigurationArgs, mcp_tools_for, save_to_store,
};
use iced::Task;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// 환경변수 값이 마스킹될 때 노출되는 자리표시자. 값이 있다는 사실은 알려주되 내용은 감춘다.
const MASKED_VALUE: &str = "<hidden>";

/// 가려진 환경변수 줄을 다시 찍을 때 쓰는 접두사. 어느 줄이 배너인지는 이 접두사로 판정하지
/// 않는다 — 프로그램도 같은 모양의 줄을 찍을 수 있어, 텍스트로 되짚으면 사용자 출력이 사라진다.
/// 판정은 찍는 쪽이 기록한 줄 id(`RunSession::is_env_banner_line`)로만 한다.
const ENV_BANNER_PREFIX: &str = "Environment: ";

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
            // 거부도 상태바에 남긴다. stage 2까지는 거부 대상이 실행 툴이었지만 편집 툴이
            // 들어온 뒤로는 "권한 밖 삭제 시도"가 여기서 걸리며, 그것은 성공한 삭제보다 감사
            // 가치가 낮지 않다(보안 항목 8).
            //
            // 이 줄은 필요한 tier를 싣지 않는다 — 고칠 수 있는 쪽(호출자)에게 필요한 문구는
            // 와이어 응답이 이미 싣고(`permission_denial`이 필요·현재 tier와 설정 위치를 준다),
            // 이 줄은 자기 설정을 아는 사용자에게 "무엇이 시도됐고 막혔다"를 남기는 감사
            // 기록이다.
            if let Some(tool) = op.tool_name() {
                self.status_message =
                    format!("[MCP] Denied {tool}: beyond the current permission tier");
            }
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
                Some(session) => McpOutcome::Payload(session_output(
                    session,
                    tail_lines,
                    since_line_id,
                    self.mcp_expose_env_values,
                )),
                None => McpOutcome::Failure(session_not_found(session_id)),
            },
            McpOp::SearchSessionOutput {
                session_id,
                query,
                regex,
            } => match find_session(&self.sessions, session_id) {
                Some(session) => McpOutcome::Payload(session_search(
                    session,
                    &query,
                    regex,
                    self.mcp_expose_env_values,
                )),
                None => McpOutcome::Failure(session_not_found(session_id)),
            },
            // 변경성 툴은 실행 Task를 돌려줘야 하고 stdin은 쓰기가 끝난 뒤에 응답하므로,
            // 각 핸들러가 요청을 넘겨받아 스스로 답한다.
            McpOp::RunConfiguration { target, request_id } => {
                return self.mcp_run_configuration(request, &target, request_id);
            }
            McpOp::StopSession { session_id } => return self.mcp_stop_session(request, session_id),
            McpOp::RerunSession {
                session_id,
                request_id,
            } => return self.mcp_rerun_session(request, session_id, request_id),
            McpOp::SendSessionInput {
                session_id,
                text,
                request_id,
            } => {
                return self.mcp_send_session_input(request, session_id, text, request_id);
            }
            McpOp::CreateConfiguration(spec) => {
                return self.mcp_create_configuration(request, *spec);
            }
            McpOp::UpdateConfiguration(patch) => {
                return self.mcp_update_configuration(request, *patch);
            }
            McpOp::DeleteConfiguration(target) => {
                return self.mcp_delete_configuration(request, target);
            }
        };

        request.respond(outcome);
        Task::none()
    }

    /// 구성 실행. 세션이 생긴 직후 응답하고 실행은 앱 안에서 계속된다.
    ///
    /// 실행 자체는 GUI 버튼과 같은 `handle_run_configuration`에 맡기고, 새로 생긴 세션은
    /// 호출 전후의 `sessions` 길이 차로 알아낸다 — 그 핸들러는 세션을 뒤에 push하므로,
    /// 반환 타입을 바꾸지 않고도 이 경로가 필요한 id를 얻는다.
    fn mcp_run_configuration(
        &mut self,
        request: McpRequest,
        target: &str,
        request_id: Option<String>,
    ) -> Task<Message> {
        // 중복 흡수는 다른 어떤 판정보다 앞에 온다. 재시도가 도착하는 시점에는 그 사이 구성이
        // 삭제·개명됐을 수 있고, 그러면 이미 실행된 요청에 "구성 없음"으로 답하게 된다.
        // 그래서 키는 해석 전 문자열이다. 같은 구성을 처음엔 이름, 다음엔 id로 부르면 충돌로
        // 거절되지만, 그것은 재시도가 아니라 새 의도다 — 해석 후 id로 키를 잡으면 위 순서가
        // 깨져 삭제 후 재시도가 다시 "구성 없음"이 된다.
        match self.mcp_dedup_lookup(DedupScope::RunConfiguration, request_id.as_deref(), target) {
            DedupLookup::Fresh => {}
            DedupLookup::Duplicate(sessions) => {
                self.status_message = format!("[MCP] Duplicate run request for '{target}' ignored");
                let payload = self.started_sessions_payload(&sessions, true);
                request.respond(McpOutcome::Payload(payload));
                return Task::none();
            }
            DedupLookup::TargetMismatch { recorded_target } => {
                return self.mcp_refuse_target_mismatch(
                    request,
                    "run_configuration",
                    request_id.as_deref().unwrap_or_default(),
                    &recorded_target,
                    target,
                );
            }
            DedupLookup::WindowFull => return self.mcp_refuse_window_full(request),
        }

        let Some(index) = find_configuration_index(&self.configurations, target) else {
            request.respond(McpOutcome::Failure(format!(
                "No configuration matches '{target}'"
            )));
            return Task::none();
        };
        let name = self.configurations[index].name.clone();

        let before = self.sessions.len();
        let task = self.handle_run_configuration(Some(index));
        let started: Vec<Uuid> = self
            .sessions
            .get(before..)
            .unwrap_or_default()
            .iter()
            .map(|s| s.id)
            .collect();

        if started.is_empty() {
            // 세션을 하나도 만들지 않는 경로(인앱 업데이트 진행 중, 실행 가능한 compound 멤버
            // 없음)는 모두 이유를 `status_message`에 남긴다. 그 이유를 실패로 실어 보내지
            // 않으면 호출자에게는 "성공했는데 세션이 없다"로 보인다.
            let reason = scavenged_reason(&mut self.status_message);
            self.status_message = format!("[MCP] Failed to run '{name}': {reason}");
            request.respond(McpOutcome::Failure(format!(
                "Failed to run '{name}': {reason}"
            )));
            return task;
        }

        if let Some(request_id) = request_id {
            self.mcp_request_log.record(
                DedupScope::RunConfiguration,
                request_id,
                target.to_string(),
                started.clone(),
                Instant::now(),
            );
        }
        self.mcp_tag_status();
        let payload = self.started_sessions_payload(&started, false);
        request.respond(McpOutcome::Payload(payload));
        task
    }

    /// 세션 중지. 이미 끝난 세션은 실패가 아니라 최종 상태로 답한다 — 호출자가 원한 결과와
    /// 같고, 재시도가 그대로 안전해진다.
    fn mcp_stop_session(&mut self, request: McpRequest, session_id: Uuid) -> Task<Message> {
        let Some(session) = find_session(&self.sessions, session_id) else {
            request.respond(McpOutcome::Failure(session_not_found(session_id)));
            return Task::none();
        };
        let config_name = session.config_name.clone();
        let was_running = session.is_running;

        let task = self.handle_stop_session(session_id);
        self.status_message = if was_running {
            format!("[MCP] Stopped session: {config_name}")
        } else {
            format!("[MCP] Session '{config_name}' had already finished")
        };
        let payload = self.session_entry(session_id);
        request.respond(McpOutcome::Payload(payload));
        task
    }

    /// 세션 재실행. `handle_rerun_session`은 세션 객체를 재사용하면서 id만 새로 발급하므로
    /// 호출자가 넘긴 id는 이 호출 뒤에 존재하지 않는다 — 새 id를 돌려주는 것이 이 툴의 계약이다.
    fn mcp_rerun_session(
        &mut self,
        request: McpRequest,
        session_id: Uuid,
        request_id: Option<String>,
    ) -> Task<Message> {
        // 중복 흡수가 세션 조회보다 먼저 와야 한다. 첫 호출이 이 세션의 id를 새로 발급했으므로
        // 재시도가 실어 오는 옛 id는 그 시점에 이미 존재하지 않는다 — 순서가 바뀌면 창이 정작
        // 그것을 위해 만들어진 경로에서 절대 발화하지 않는다.
        let target = session_id.to_string();
        match self.mcp_dedup_lookup(DedupScope::RerunSession, request_id.as_deref(), &target) {
            DedupLookup::Fresh => {}
            DedupLookup::Duplicate(sessions) => {
                // rerun은 세션 하나만 기록한다. 그래도 패닉으로 단정하지 않는 것은 이 코드가
                // iced update 루프 안에서 돌기 때문이다 — 여기서 패닉하면 재시도 한 번 때문에
                // 살아 있는 모든 세션이 앱과 함께 사라진다.
                let [restarted] = sessions[..] else {
                    request.respond(McpOutcome::Failure(String::from(
                        "The duplicate rerun record is malformed. Send a different request_id.",
                    )));
                    return Task::none();
                };
                self.status_message = String::from("[MCP] Duplicate rerun request ignored");
                let entry = self.session_entry(restarted);
                // 경계는 첫 호출이 이미 받았다. 지금 다시 재면 그 사이 새 실행이 찍은 줄을
                // 가리켜, 호출자가 그만큼의 출력을 조용히 건너뛴다 — 없다고 답하는 편이 안전하다.
                request.respond(McpOutcome::Payload(rerun_payload(
                    entry, session_id, None, true,
                )));
                return Task::none();
            }
            DedupLookup::TargetMismatch { recorded_target } => {
                return self.mcp_refuse_target_mismatch(
                    request,
                    "rerun_session",
                    request_id.as_deref().unwrap_or_default(),
                    &recorded_target,
                    &target,
                );
            }
            DedupLookup::WindowFull => return self.mcp_refuse_window_full(request),
        }

        let Some(position) = self.sessions.iter().position(|s| s.id == session_id) else {
            request.respond(McpOutcome::Failure(session_not_found(session_id)));
            return Task::none();
        };

        let config_name = self.sessions[position].config_name.clone();
        // 재실행이 출력을 지우지 않으므로 한 버퍼에 두 실행이 남는다. 여기서 잡아 두는 마지막
        // 줄 id가 그 경계다 — 이 값이 없으면 재실행 뒤에 처음 읽는 호출자가 이전 실행의 실패
        // 출력을 새 실행의 것으로 읽는다.
        let boundary = self.sessions[position]
            .output_lines
            .back()
            .map(|(line_id, _)| *line_id);
        let task = self.handle_rerun_session(session_id);
        // 재실행은 세션을 제거하거나 순서를 바꾸지 않으므로 같은 위치에 그대로 있다.
        let restarted = self.sessions[position].id;

        if restarted == session_id {
            // id가 그대로면 재실행이 시작되지 않았다(세션의 구성이 그 사이 삭제된 경우).
            let reason = scavenged_reason(&mut self.status_message);
            self.status_message = format!("[MCP] Failed to rerun '{config_name}': {reason}");
            request.respond(McpOutcome::Failure(format!(
                "Failed to rerun '{config_name}': {reason}"
            )));
            return task;
        }

        if let Some(request_id) = request_id {
            self.mcp_request_log.record(
                DedupScope::RerunSession,
                request_id,
                target,
                vec![restarted],
                Instant::now(),
            );
        }
        self.mcp_tag_status();
        let entry = self.session_entry(restarted);
        request.respond(McpOutcome::Payload(rerun_payload(
            entry, session_id, boundary, false,
        )));
        task
    }

    /// 세션 stdin에 한 줄. 쓰기가 끝난 뒤에 응답한다 — 즉시 성공으로 답하면 끊어진 stdin이
    /// 호출자에게 "전달됨"으로 보인다.
    fn mcp_send_session_input(
        &mut self,
        request: McpRequest,
        session_id: Uuid,
        text: String,
        request_id: Option<String>,
    ) -> Task<Message> {
        let target = input_target(session_id, &text);
        match self.mcp_dedup_lookup(DedupScope::SendSessionInput, request_id.as_deref(), &target) {
            DedupLookup::Fresh => {}
            DedupLookup::Duplicate(_) => {
                self.status_message = String::from("[MCP] Duplicate input request ignored");
                let entry = self.session_entry(session_id);
                request.respond(McpOutcome::Payload(input_payload(
                    entry,
                    "duplicate",
                    Some(
                        "this request_id already submitted this same line to this session and \
                         may still be in flight, so it was not written again; read the \
                         session output to see whether the program consumed it",
                    ),
                )));
                return Task::none();
            }
            DedupLookup::TargetMismatch { recorded_target } => {
                return self.mcp_refuse_target_mismatch(
                    request,
                    "send_session_input",
                    request_id.as_deref().unwrap_or_default(),
                    &recorded_target,
                    &target,
                );
            }
            DedupLookup::WindowFull => return self.mcp_refuse_window_full(request),
        }

        let Some(session) = find_session(&self.sessions, session_id) else {
            request.respond(McpOutcome::Failure(session_not_found(session_id)));
            return Task::none();
        };
        if !session.is_running {
            request.respond(McpOutcome::Failure(format!(
                "Session {session_id} is not running; input has nowhere to go"
            )));
            return Task::none();
        }
        let config_name = session.config_name.clone();

        // 기록은 쓰기를 걸기 **전에** 남긴다. 완료 시점에 기록하면 이 기능이 존재하는 이유인
        // 바로 그 창 — 응답을 받기 전에 재시도가 도착하는 구간 — 이 기록 없이 지나간다.
        if let Some(request_id) = request_id.clone() {
            self.mcp_request_log.record(
                DedupScope::SendSessionInput,
                request_id,
                target,
                vec![session_id],
                Instant::now(),
            );
        }

        self.status_message = format!("[MCP] Sending input to {config_name}");
        let line = text.clone();
        Task::perform(
            crate::services::write_session_stdin(session_id, text),
            move |result| Message::McpSessionInputWritten {
                request: request.clone(),
                session_id,
                request_id: request_id.clone(),
                line: line.clone(),
                result,
            },
        )
    }

    /// stdin 쓰기 결과를 호출자에게 돌려준다.
    ///
    /// GUI의 `handle_session_stdin_write_completed`를 재사용하지 않는 것은 그 핸들러가 하드
    /// 실패 시 사용자의 입력 드래프트를 복원하기 때문이다 — 에이전트가 보낸 텍스트가 사용자의
    /// 입력창에 들어가면 사용자의 다음 Enter가 그걸 다시 보낸다.
    pub(super) fn handle_mcp_session_input_written(
        &mut self,
        request: McpRequest,
        session_id: Uuid,
        request_id: Option<String>,
        line: String,
        result: Result<bool, StdinWriteError>,
    ) -> Task<Message> {
        let outcome = match result {
            Ok(needs_local_echo) => {
                if needs_local_echo {
                    self.echo_stdin_locally(session_id, &line);
                }
                let entry = self.session_entry(session_id);
                McpOutcome::Payload(input_payload(entry, "delivered", None))
            }
            // 제한 시간 초과는 하드 실패가 아니라 "자식이 아직 stdin을 읽지 않았다"는 조기
            // 신호다 — 쓰기는 백그라운드에서 완료되므로 다시 보내면 입력이 두 번 들어간다.
            Err(StdinWriteError::Timeout) => {
                self.status_message =
                    String::from("[MCP] Input pending — the process is not reading stdin yet");
                let entry = self.session_entry(session_id);
                McpOutcome::Payload(input_payload(
                    entry,
                    "pending",
                    Some(
                        "the process has not read stdin yet; the write is still in flight, \
                         so sending it again would deliver the line twice",
                    ),
                ))
            }
            Err(StdinWriteError::Broken(error)) => {
                // 파이프가 닫혔으니 이 줄은 확실히 도착하지 않았다. 기록을 지워 같은
                // `request_id`로 다시 시도할 수 있게 한다 — 남겨 두면 재시도가 "이미 보냈다"로
                // 흡수되어, 한 줄도 전달되지 않은 요청이 성공으로 굳는다.
                if let Some(request_id) = request_id {
                    self.mcp_request_log
                        .forget(DedupScope::SendSessionInput, &request_id);
                }
                self.status_message = format!("[MCP] Input failed: {error}");
                McpOutcome::Failure(format!("Failed to send input: {error}"))
            }
        };
        request.respond(outcome);
        Task::none()
    }

    /// 중복 흡수 창 조회. `request_id`가 없으면 흡수하지 않는다 — 툴 설명이 그 재시도 위험을
    /// 명시하고, 창에 넣을지는 호출자가 결정한다.
    fn mcp_dedup_lookup(
        &mut self,
        scope: DedupScope,
        request_id: Option<&str>,
        target: &str,
    ) -> DedupLookup {
        let Some(request_id) = request_id else {
            return DedupLookup::Fresh;
        };
        self.mcp_request_log
            .lookup(scope, request_id, target, Instant::now())
    }

    /// 창이 가득 차 호출을 거절한다. 상태바에도 남긴다 — 에이전트가 창을 채우고 있다는 사실은
    /// 사용자가 앱에서 볼 수 있어야 한다.
    fn mcp_refuse_window_full(&mut self, request: McpRequest) -> Task<Message> {
        self.status_message = String::from("[MCP] Retry window full — request refused");
        request.respond(McpOutcome::Failure(dedup_window_full()));
        Task::none()
    }

    /// 같은 `request_id`가 다른 대상으로 온 요청의 거절. 형제 판정인 `Duplicate`·`WindowFull`과
    /// 마찬가지로 상태바에 남긴다 — 파괴적 툴에 대한 대상 혼동은 감사 가치가 가장 높은 사건에
    /// 속한다. 대상을 문구에 싣지 않는 것은 두 대상 중 어느 쪽도 사용자가 확인해 줄 수 없는
    /// 정보이고, 대상 문자열에는 호출자 원문(구성 이름·요청 지문)이 접혀 있기 때문이다.
    fn mcp_refuse_target_mismatch(
        &mut self,
        request: McpRequest,
        tool: &str,
        request_id: &str,
        recorded_target: &str,
        target: &str,
    ) -> Task<Message> {
        self.status_message =
            format!("[MCP] Rejected {tool}: request_id reused for a different target");
        request.respond(McpOutcome::Failure(dedup_conflict(
            request_id,
            recorded_target,
            target,
        )));
        Task::none()
    }

    /// 구성 생성. 저장까지 함께 발행한다 — 에이전트에게는 GUI의 Save 버튼이 없다.
    fn mcp_create_configuration(
        &mut self,
        request: McpRequest,
        spec: CreateConfigurationArgs,
    ) -> Task<Message> {
        // 지문은 `spec`에서 필드가 옮겨 나가기 전에 뜬다.
        let target = edit_target(&spec.name, &spec);
        // 중복 흡수가 다른 어떤 판정보다 앞에 오는 이유는 실행 툴과 같다(`mcp_run_configuration`).
        // 생성에서는 그 대가가 특히 크다 — 흡수하지 않은 재시도는 목록에 조용히 쌍둥이를 남기고,
        // 그때부터 이름으로 부르는 호출이 둘 중 하나를 임의로 고른다.
        match self.mcp_dedup_lookup(
            DedupScope::CreateConfiguration,
            spec.request_id.as_deref(),
            &target,
        ) {
            DedupLookup::Fresh => {}
            DedupLookup::Duplicate(produced) => {
                self.status_message =
                    format!("[MCP] Duplicate create request for '{}' ignored", spec.name);
                let payload = self.duplicate_edit_payload(&produced);
                request.respond(McpOutcome::Payload(payload));
                return Task::none();
            }
            DedupLookup::TargetMismatch { recorded_target } => {
                return self.mcp_refuse_target_mismatch(
                    request,
                    "create_configuration",
                    spec.request_id.as_deref().unwrap_or_default(),
                    &recorded_target,
                    &target,
                );
            }
            DedupLookup::WindowFull => return self.mcp_refuse_window_full(request),
        }

        if let Err(reason) = self
            .edit_precondition()
            .and_then(|()| reject_masked_env(&spec.environment_variables))
            .and_then(|()| self.check_name_available(&spec.name, None))
            .and_then(|()| self.check_compound_members(&spec.type_data, None))
            .and_then(|()| check_working_directory(&spec.type_data, &spec.working_directory))
        {
            return self.mcp_refuse_edit(request, "create", reason);
        }

        let mut config = RunConfiguration {
            name: spec.name,
            working_directory: spec.working_directory,
            environment_variables: spec.environment_variables,
            type_data: spec.type_data,
            // id는 앱이 정한다 — `Default`가 만드는 v4를 쓰므로 생성 규칙이 한 군데에만 있다.
            ..RunConfiguration::default()
        };
        derive_node_working_directory(&mut config);
        let config_id = config.id;
        let name = config.name.clone();
        // 목록에만 넣는다 — `selected_config_index`를 옮기면 편집기가 이 구성을 렌더하고
        // `Message::NameChanged`도 같은 인덱스로 되쓰므로, 이름을 타이핑하던 사용자의 다음 키가
        // 에이전트의 구성에 들어간다. "GUI/MCP 동일 경로"의 의도적 예외다.
        // Node 메타데이터도 채우지 않는다: 그 스캔(`load_node_package_jsons`)은 하위 방향 동기
        // 재귀 디렉터리 워크여서 에이전트가 넘긴 경로로 update 루프 안에서 돌면 그 트리를 훑는
        // 동안 루프가 붙잡힌다 — 깊이 5와 제외 목록이 있어 무한하지는 않지만(`find_all_package_jsons`)
        // 비용은 호출자가 고른 트리에 비례한다. 사용자가 이 구성을 선택하면
        // `select_configuration`이 채운다.
        self.configurations.push(config);

        self.status_message = format!("Created configuration '{name}'");
        self.record_edit(
            DedupScope::CreateConfiguration,
            spec.request_id,
            &target,
            config_id,
        );
        self.mcp_tag_status();
        let payload = self.edit_payload(config_id, false);
        request.respond(McpOutcome::Payload(payload));
        self.save_configurations_for_mcp()
    }

    /// 구성 부분 수정. 실려 온 필드만 건드린다.
    fn mcp_update_configuration(
        &mut self,
        request: McpRequest,
        patch: UpdateConfigurationArgs,
    ) -> Task<Message> {
        // 지문은 `patch`에서 필드가 옮겨 나가기 전에 뜬다.
        let target = edit_target(&patch.configuration, &patch);
        match self.mcp_dedup_lookup(
            DedupScope::UpdateConfiguration,
            patch.request_id.as_deref(),
            &target,
        ) {
            DedupLookup::Fresh => {}
            DedupLookup::Duplicate(produced) => {
                self.status_message = format!(
                    "[MCP] Duplicate update request for '{}' ignored",
                    patch.configuration
                );
                let mut payload = self.duplicate_edit_payload(&produced);
                // 흡수된 재시도는 아무것도 바꾸지 않았지만 `changed: false`는 "이 패치는 효과가
                // 없었다"는 뜻이어서 첫 호출의 효과까지 부정한다. 알 수 없음을 null로 밝힌다 —
                // 삭제가 `referencing_compounds`에 하는 것과 같은 판단이다.
                payload["changed"] = Value::Null;
                request.respond(McpOutcome::Payload(payload));
                return Task::none();
            }
            DedupLookup::TargetMismatch { recorded_target } => {
                return self.mcp_refuse_target_mismatch(
                    request,
                    "update_configuration",
                    patch.request_id.as_deref().unwrap_or_default(),
                    &recorded_target,
                    &target,
                );
            }
            DedupLookup::WindowFull => return self.mcp_refuse_window_full(request),
        }

        if let Err(reason) = self
            .edit_precondition()
            .and_then(|()| reject_masked_env(&patch.set_environment_variables))
        {
            return self.mcp_refuse_edit(request, "update", reason);
        }
        let Some(index) = find_configuration_index(&self.configurations, &patch.configuration)
        else {
            return self.mcp_refuse_edit(
                request,
                "update",
                format!("No configuration matches '{}'", patch.configuration),
            );
        };
        let config_id = self.configurations[index].id;
        if let Err(reason) = patch
            .name
            .as_deref()
            .map_or(Ok(()), |name| {
                self.check_name_available(name, Some(config_id))
            })
            .and_then(|()| {
                patch.type_data.as_ref().map_or(Ok(()), |type_data| {
                    self.check_compound_members(type_data, Some(config_id))
                })
            })
            .and_then(|()| {
                // 이 패치가 결과 디렉터리를 실제로 정할 때만 본다. 두 필드 중 아무것도 싣지
                // 않은 호출까지 판정하면, 개명만 하는 요청이 패치와 무관한 기존 값 때문에
                // 거절된다 — 프로젝트를 아직 고르지 않은 Node 구성은 GUI로 만들 수 있다.
                if patch.type_data.is_none() && patch.working_directory.is_none() {
                    return Ok(());
                }
                let config = &self.configurations[index];
                check_working_directory(
                    patch.type_data.as_ref().unwrap_or(&config.type_data),
                    patch
                        .working_directory
                        .as_deref()
                        .unwrap_or(&config.working_directory),
                )
            })
        {
            return self.mcp_refuse_edit(request, "update", reason);
        }

        let config = &mut self.configurations[index];
        // 제거를 먼저 적용하므로, 한 키가 양쪽 목록에 다 실려 오면 설정이 이긴다.
        let mut env_changed = false;
        for key in &patch.remove_environment_variables {
            env_changed |= config.environment_variables.remove(key).is_some();
        }
        for (key, value) in patch.set_environment_variables {
            env_changed |= config.environment_variables.get(&key) != Some(&value);
            config.environment_variables.insert(key, value);
        }
        let mut renamed = false;
        if let Some(new_name) = patch.name {
            renamed = config.name != new_name;
            config.name = new_name;
        }
        let previous_directory = config.working_directory.clone();
        if let Some(directory) = patch.working_directory {
            config.working_directory = directory;
        }
        // `type_data`는 통째로 교체한다. 타입별 필드를 부분 병합하면 타입을 바꾸는 수정에서
        // 어느 필드가 남는지가 타입 조합마다 달라져, 호출자가 결과를 예측할 수 없다.
        let mut type_data_changed = false;
        if let Some(type_data) = patch.type_data {
            type_data_changed = config.type_data != type_data;
            config.type_data = type_data;
        }
        derive_node_working_directory(config);
        // 파생까지 끝난 뒤에 재므로 이 플래그는 "결과 디렉터리가 달라졌다"를 뜻한다 — 요청이
        // 실은 값이 아니라 실제 효과다(`changed`의 규칙과 같다).
        let directory_changed = config.working_directory != previous_directory;
        let name = config.name.clone();
        let changed = env_changed || renamed || directory_changed || type_data_changed;

        if env_changed {
            // 대량 편집기의 초안은 구성의 환경변수를 직렬화해 채워 둔 사본이다. 남겨 두면
            // 사용자가 Apply를 누르는 순간 이 변경이 조용히 되돌아간다. 초안을 버리는 대가는
            // 사용자가 입력하던 내용이지만, 그쪽은 눈에 보이고 되돌림은 보이지 않는다.
            self.env_bulk_inputs.remove(&config_id);
            // 열린 모달의 `entries`도 같은 종류의 사본이고, Confirm은 그것으로 맵을 **통째로**
            // 교체한다(`handle_confirm_env_modal`) — 초안보다 피해가 크다. 갱신하지 않고 닫는
            // 것은 사용자가 입력하던 내용과의 병합 규칙을 새로 만들지 않기 위한 것이며, 삭제
            // 경로가 같은 상황에서 하는 일과 같다(`handle_delete_configuration`).
            if self
                .env_modal
                .as_ref()
                .is_some_and(|modal| modal.config_id == config_id)
            {
                self.env_modal = None;
            }
        }
        if directory_changed || type_data_changed {
            // Node 메타데이터 캐시는 구성 id로 키잉되어 있어 저절로 낡는다. 남겨 두면
            // `sync_editor_select_state_for_selected_config`가 옛 디렉터리의 스크립트 목록을
            // 편집기에 다시 설치하고, 사용자가 그중 하나를 고르면 새 프로젝트에 없는 스크립트가
            // 구성에 박힌다 — 빈 목록(보이는 부재)보다 나쁜 틀린 목록이다.
            //
            // 여기서 다시 재지는 않는다. `load_node_scripts`는 `find_package_json`으로 부모
            // 방향을 root까지 훑고 `parse_scripts`가 크기 상한 없이 파일을 읽는데, 둘 다 동기
            // 호출이어서 호출자가 넘긴 경로가 그동안 update 루프 전체를 붙잡는다 — D45가 배제한
            // 것과 같은 종류다. 비운 캐시는 사용자가 그 구성을 고를 때 `select_configuration`이
            // 다시 채운다.
            self.node_available_scripts.remove(&config_id);
            self.node_available_package_jsons.remove(&config_id);
        }
        // 편집기 패널의 select 상태는 선택된 구성에서 파생되므로 다시 맞춘다.
        self.sync_editor_select_state_for_selected_config();

        // 파싱은 요청의 **모양**만 본다(`changes_something`) — 설정되지 않은 변수를 지우거나 같은
        // 값을 다시 넣는 호출은 그 검사를 통과하고도 아무것도 바꾸지 않는다. 그 경우를 변경과
        // 같은 문장·같은 응답으로 답하면 호출자가 "적용됨"과 "무변경"을 구별할 수 없고, 저장까지
        // 발행하면 사용자가 저장하지 않고 두었던 GUI 편집이 아무것도 바꾸지 않은 호출 때문에
        // 디스크에 커밋된다(D41의 부작용).
        self.status_message = if changed {
            format!("Updated configuration '{name}'")
        } else {
            format!("Update left configuration '{name}' unchanged")
        };
        self.record_edit(
            DedupScope::UpdateConfiguration,
            patch.request_id,
            &target,
            config_id,
        );
        self.mcp_tag_status();
        let mut payload = self.edit_payload(config_id, false);
        payload["changed"] = json!(changed);
        request.respond(McpOutcome::Payload(payload));
        if changed {
            self.save_configurations_for_mcp()
        } else {
            Task::none()
        }
    }

    /// 구성 삭제.
    ///
    /// GUI의 Delete 버튼은 확인 모달을 띄우지만 이 경로는 띄우지 않는다 — 에이전트는 10초
    /// 요청 상한 안에 모달에 답할 수 없다. 모달이 경고로 보여 주던 사실(어느 compound가 멤버를
    /// 잃는지)은 응답에 실어, 호출자가 자기 조작의 파급을 볼 수 있게 한다.
    fn mcp_delete_configuration(
        &mut self,
        request: McpRequest,
        target: DeleteConfigurationArgs,
    ) -> Task<Message> {
        // create·update와 달리 요청 내용을 대상에 접어 넣지 않는다(`edit_target`) — 삭제 요청은
        // 식별자가 곧 요청 전부여서, 같은 식별자를 가리키는 두 호출은 같은 요청이다.
        match self.mcp_dedup_lookup(
            DedupScope::DeleteConfiguration,
            target.request_id.as_deref(),
            &target.configuration,
        ) {
            DedupLookup::Fresh => {}
            DedupLookup::Duplicate(produced) => {
                self.status_message = format!(
                    "[MCP] Duplicate delete request for '{}' ignored",
                    target.configuration
                );
                let mut payload = self.duplicate_edit_payload(&produced);
                // 첫 호출에서 이미 멤버가 빠졌으므로 빈 목록은 "잃은 것이 없다"는 거짓이 된다.
                // 키를 빼지 않는 것은 호출자가 응답 모양을 두 가지로 다루지 않게 하기 위함이다.
                payload["referencing_compounds"] = Value::Null;
                request.respond(McpOutcome::Payload(payload));
                return Task::none();
            }
            DedupLookup::TargetMismatch { recorded_target } => {
                return self.mcp_refuse_target_mismatch(
                    request,
                    "delete_configuration",
                    target.request_id.as_deref().unwrap_or_default(),
                    &recorded_target,
                    &target.configuration,
                );
            }
            DedupLookup::WindowFull => return self.mcp_refuse_window_full(request),
        }

        if let Err(reason) = self.edit_precondition() {
            return self.mcp_refuse_edit(request, "delete", reason);
        }
        let Some(index) = find_configuration_index(&self.configurations, &target.configuration)
        else {
            return self.mcp_refuse_edit(
                request,
                "delete",
                format!("No configuration matches '{}'", target.configuration),
            );
        };
        let config_id = self.configurations[index].id;
        let name = self.configurations[index].name.clone();
        let referencing_compounds: Vec<String> = self
            .configurations
            .iter()
            .filter(|config| {
                config
                    .type_data
                    .compound_members()
                    .is_some_and(|members| members.contains(&config_id))
            })
            .map(|config| config.name.clone())
            .collect();

        // GUI 핸들러를 그대로 쓴다 — 삭제는 구성 하나를 지우는 것으로 끝나지 않고 Node 캐시,
        // 환경변수 초안, compound 멤버 참조, 열려 있는 모달까지 함께 정리해야 한다.
        let task = self.handle_delete_configuration(Some(index));

        self.status_message = format!("Deleted configuration '{name}'");
        self.record_edit(
            DedupScope::DeleteConfiguration,
            target.request_id,
            &target.configuration,
            config_id,
        );
        self.mcp_tag_status();
        let mut payload = self.edit_payload(config_id, false);
        payload["referencing_compounds"] = json!(referencing_compounds);
        request.respond(McpOutcome::Payload(payload));
        Task::batch([task, self.save_configurations_for_mcp()])
    }

    /// 편집 툴의 공통 선행 조건.
    ///
    /// 로드가 끝나기 전의 편집은 `handle_configurations_loaded`가 `configurations`를 통째로
    /// 교체할 때 조용히 사라진다 — 그때 호출자는 성공 응답을 이미 받은 뒤다.
    ///
    /// 로드가 **실패로** 끝난 경우까지 막는 것은 `handle_save_configurations`와 갈리는 지점이다.
    /// 실패하면 `configurations`는 빈 채로 남고 편집 툴은 스스로 저장을 발행하므로
    /// (`save_configurations_for_mcp`), 그 상태에서 구성 하나를 만들면 저장소가 한 개짜리 목록으로
    /// 교체된다 — `write_configs_at`은 백업을 남기지 않으니 수기로 복구할 수 있었던 손상 파일이
    /// 사라진다. GUI Save는 같은 상황에서 열려 있지만 그것은 상태바의 실패 문구를 **읽은 사람**이
    /// 누른 쓰기다. 에이전트에게는 그 문구가 보이지 않는다.
    fn edit_precondition(&self) -> Result<(), String> {
        if !self.configs_ready {
            return Err(String::from(
                "Configurations are still loading; retry after the app has finished loading them",
            ));
        }
        if self.configs_load_failed {
            return Err(String::from(
                "The stored configurations could not be read, so the in-memory list is empty and \
                 writing now would replace the store. Fix or move the store file first.",
            ));
        }
        Ok(())
    }

    /// 이름이 비어 있는 자리인지. 이름은 다른 툴이 구성을 지목하는 주소이므로
    /// (`find_configuration_index`), 중복을 허용하면 이름으로 부른 호출이 둘 중 하나를 임의로
    /// 고른다 — 그 선택은 호출자에게 보이지 않는다.
    fn check_name_available(&self, name: &str, keeping: Option<Uuid>) -> Result<(), String> {
        let taken = self
            .configurations
            .iter()
            .any(|config| config.name == name && Some(config.id) != keeping);
        if taken {
            return Err(format!("A configuration named '{name}' already exists"));
        }
        Ok(())
    }

    /// Compound 멤버가 GUI 편집기가 고를 수 있는 것들인지. `view_compound_fields`가 후보에서
    /// 자기 자신과 다른 compound를 걸러 내므로(`src/views/configuration_editor.rs`) 세 조건은 모두
    /// GUI가 이미 지키는 불변식이고, 여기서 받아 주면 **GUI로는 만들 수도 고칠 수도 없는** 구성이
    /// 남는다 — 그 멤버 행은 목록에 나타나지 않아 사용자가 체크를 해제할 수 없다.
    ///
    /// 존재 검사: 앱은 삭제할 때 댕글링 멤버를 걷어내(`handle_delete_configuration`) 이 불변식을
    /// 능동적으로 지킨다. 없는 멤버는 실행에서 조용히 건너뛰어지고(`run_compound`) 호출자는 무엇이
    /// 빠졌는지 알 수 없다.
    ///
    /// 중복 멤버 검사: `run_compound`는 멤버 목록을 순회하며 항목마다 세션을 띄우므로 같은
    /// 구성이 두 번 실려 오면 한 번 눌러 두 세션이 뜬다. GUI 편집기의 멤버 목록은 체크박스
    /// 토글이어서 중복을 만들 수도, 만들어진 중복 하나만 지울 수도 없다.
    ///
    /// 자기 참조·중첩 compound 검사: `run_compound`가 compound 멤버를 실행하지 않고 건너뛰므로
    /// (`src/app.rs`의 중첩 분기) 결과는 존재하지 않는 멤버와 같다 — 조용히 빠진 멤버다. D37이
    /// 검사하지 않기로 한 것은 **순환 탐색**이었고, 이 두 조건은 멤버당 조회 한 번이어서 그
    /// 복잡도가 아니다. `editing`이 `None`인 create에서 자기 참조는 애초에 불가능하다(id는 서버가
    /// 만들고 호출자가 실은 `id`는 `deny_unknown_fields`가 거절한다).
    fn check_compound_members(
        &self,
        type_data: &ConfigTypeData,
        editing: Option<Uuid>,
    ) -> Result<(), String> {
        let Some(members) = type_data.compound_members() else {
            return Ok(());
        };
        for (position, member) in members.iter().enumerate() {
            let Some(config) = self.configurations.iter().find(|it| it.id == *member) else {
                return Err(format!("No configuration has id {member}"));
            };
            if members[..position].contains(member) {
                return Err(format!(
                    "Configuration '{}' is listed twice: each entry runs once",
                    config.name
                ));
            }
            if editing == Some(*member) {
                return Err(String::from("A compound cannot contain itself"));
            }
            if matches!(config.type_data, ConfigTypeData::Compound { .. }) {
                return Err(format!(
                    "Compound '{}' cannot be a member: compound members are not run",
                    config.name
                ));
            }
        }
        Ok(())
    }

    /// 편집 거절. 사용자가 앱에서 보는 것이 성공한 조작만이어서는 안 된다 — 에이전트가 무엇을
    /// 시도했는지도 감사 대상이다(보안 항목 8).
    fn mcp_refuse_edit(
        &mut self,
        request: McpRequest,
        action: &str,
        reason: String,
    ) -> Task<Message> {
        self.status_message = format!("[MCP] Refused to {action} configuration: {reason}");
        request.respond(McpOutcome::Failure(reason));
        Task::none()
    }

    /// 편집 요청을 중복 흡수 창에 남긴다. 실행 툴이 세션 id를 남기는 자리에 대상 구성 id를
    /// 남긴다 — 재시도가 첫 호출과 같은 답을 받는 것이 목적이다.
    fn record_edit(
        &mut self,
        scope: DedupScope,
        request_id: Option<String>,
        target: &str,
        config_id: Uuid,
    ) {
        if let Some(request_id) = request_id {
            self.mcp_request_log.record(
                scope,
                request_id,
                target.to_string(),
                vec![config_id],
                Instant::now(),
            );
        }
    }

    /// 편집 응답. `configuration`은 변경 뒤의 상세이고, 목록에 없으면(삭제됨) null이다 —
    /// 키를 빼면 호출자가 응답 모양을 두 가지로 다뤄야 한다(`session_entry`와 같은 규칙).
    /// 직렬화 실패도 null로 접는다: 편집은 이미 적용됐으므로 전체를 실패로 답하면 호출자가
    /// 일어난 변경을 일어나지 않은 것으로 읽는다.
    fn edit_payload(&self, config_id: Uuid, deduplicated: bool) -> Value {
        let detail = self
            .configurations
            .iter()
            .find(|config| config.id == config_id)
            .and_then(|config| configuration_detail(config, self.mcp_expose_env_values).ok());
        json!({
            "configuration_id": config_id,
            "configuration": detail,
            "deduplicated": deduplicated,
        })
    }

    fn duplicate_edit_payload(&self, produced: &[Uuid]) -> Value {
        match produced {
            [config_id] => self.edit_payload(*config_id, true),
            // 편집 툴은 언제나 구성 하나를 기록한다. 도달 불가하지만, 기록이 비어 있다면
            // 응답에 id를 지어내지 않는다.
            _ => json!({
                "configuration_id": Value::Null,
                "configuration": Value::Null,
                "deduplicated": true,
            }),
        }
    }

    /// 편집 결과를 디스크에 남긴다. GUI는 자동 저장하지 않고 에이전트에게는 Save 버튼이 없으므로,
    /// 편집 툴이 저장을 스스로 발행한다.
    ///
    /// 완료를 기다리지 않는다: `Message::McpConfigurationsSaved`에는 어느 요청의 저장인지
    /// 되짚을 열쇠가 없어 저장이 겹치면 결과를 잘못 짝지을 수 있고, 기다리면 저장 I/O가 10초
    /// 요청 상한을 먹는다. 호출자는 편집이 적용됐다는 답을 받고, 저장 실패는 상태바에 남는다.
    ///
    /// 부작용: 저장 단위가 구성 목록 전체이므로, 이 저장은 사용자가 아직 저장하지 않고 두었던
    /// GUI 편집까지 함께 디스크에 남긴다.
    fn save_configurations_for_mcp(&self) -> Task<Message> {
        Task::perform(
            save_to_store(self.configurations.clone()),
            Message::McpConfigurationsSaved,
        )
    }

    /// MCP 편집이 발행한 저장의 완료. 성공은 상태바를 건드리지 않는다 — 그 자리에는 방금 남긴
    /// `[MCP]` 감사 줄이 있고, 그것이 사용자가 에이전트의 조작을 앱에서 보는 유일한 경로다
    /// (보안 항목 8). 실패만 그 줄을 대체한다.
    pub(super) fn handle_mcp_configurations_saved(
        &mut self,
        result: Result<PathBuf, String>,
    ) -> Task<Message> {
        if let Err(error) = result {
            self.status_message = format!("[MCP] Failed to save configurations: {error}");
        }
        Task::none()
    }

    /// 변경성 MCP 호출을 상태바에 남긴다. 무슨 일이 있었는지는 이미 `status_message`에 있고 —
    /// 실행 툴은 재사용한 GUI 핸들러가 썼고, 편집 툴은 이 모듈이 직접 썼다 — 여기서는 출처만
    /// 앞에 붙여 사용자가 자기 조작과 에이전트의 조작을 구별할 수 있게 한다(보안 항목 8).
    ///
    /// 그러므로 호출 시점에 상태바에는 **이번 호출의** 결과 문구가 들어 있어야 한다. 앞선 문구가
    /// 남아 있으면 그것에 `[MCP]`가 붙어 일어나지 않은 일이 에이전트의 조작으로 보고된다.
    fn mcp_tag_status(&mut self) {
        self.status_message = format!("[MCP] {}", self.status_message);
    }

    /// 실행 응답. `session_id`는 세션이 정확히 하나일 때만 싣는다 — compound 구성은 멤버마다
    /// 세션이 하나씩 생기므로 하나로 접으면 나머지가 조용히 사라진다.
    fn started_sessions_payload(&self, started: &[Uuid], deduplicated: bool) -> Value {
        let mut payload = json!({
            "sessions": started
                .iter()
                .map(|id| self.session_entry(*id))
                .collect::<Vec<_>>(),
            "deduplicated": deduplicated,
        });
        if let [only] = started {
            payload["session_id"] = json!(only);
        }
        payload
    }

    /// 세션 하나의 현재 상태. 실행 툴의 공통 응답 조각이다 — 실행을 요청한 호출자가 결과를
    /// "성공했는데 출력이 없음"과 구별할 수 있도록 상태를 응답에 함께 싣는다.
    fn session_entry(&self, session_id: Uuid) -> Value {
        match find_session(&self.sessions, session_id) {
            Some(session) => json!({
                "session_id": session_id,
                "config_name": session.config_name,
                "state": state_name(session.status_kind()),
                "exit_code": session.exit_code,
                "error": session.run_error,
            }),
            // 중복 흡수 창 안이지만 사용자가 그 사이 세션을 목록에서 지웠다. 다시 실행하지
            // 않는 것이 이 창의 존재 이유이므로, 사라졌다는 사실을 그대로 알린다.
            // 필드 집합은 세션이 남아 있을 때와 같게 유지한다 — 키가 사라지면 호출자가
            // 응답 모양을 두 가지로 다뤄야 한다.
            None => json!({
                "session_id": session_id,
                "config_name": Value::Null,
                "state": "removed",
                "exit_code": Value::Null,
                "error": Value::Null,
            }),
        }
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
        self.mcp_request_log = crate::services::McpRequestLog::default();
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

/// 실패 사유를 GUI 핸들러가 남긴 상태바 문구에서 가져온다. 문구가 비어 있는 분기가 남아
/// 있어도 `Failed to run 'x': `처럼 끝이 잘린 응답을 만들지 않도록 대체 문장을 둔다.
fn scavenged_reason(status_message: &mut String) -> String {
    let reason = std::mem::take(status_message);
    if reason.is_empty() {
        return String::from("the app reported no reason");
    }
    reason
}

/// 같은 `request_id`가 다른 대상으로 재사용됐을 때의 실패 사유. 첫 결과를 돌려주지도, 새로
/// 실행하지도 않는 이유를 호출자가 고칠 수 있는 형태로 말해 준다.
fn dedup_conflict(request_id: &str, recorded_target: &str, target: &str) -> String {
    format!(
        "request_id '{request_id}' was already used for '{recorded_target}' within the last \
         minute, so it cannot be reused for '{target}'. Send a different request_id."
    )
}

/// 창이 가득 찼을 때의 거절 사유. 아직 유효한 기록을 밀어내는 대신 거절하는 근거와, 호출자가
/// 고를 수 있는 두 가지 회복 방법을 함께 준다. 상한은 실행 툴과 편집 툴이 **함께** 쓰므로, 이
/// 호출을 막은 기록이 다른 툴에서 왔을 수 있다는 사실도 밝힌다 — 그러지 않으면 한 번도 재시도하지
/// 않은 호출자가 자기 `request_id` 때문에 막힌 것으로 읽고, 편집 툴에는 별도 예산이 있다고
/// 결론한다.
fn dedup_window_full() -> String {
    format!(
        "The retry window is full: it already holds {MCP_DEDUP_MAX_ENTRIES} request_ids from the \
         last {secs} seconds, and every tool that runs or edits something shares it — the ids \
         filling it may come from other tools. Dropping one to make room would let its retry run twice, so this call was \
         refused without doing anything. Try again once the oldest expires (within {secs} \
         seconds), or send it without a request_id — a retried call would then run again.",
        secs = MCP_DEDUP_WINDOW.as_secs()
    )
}

/// 편집 요청의 dedup 대상. 구성 식별자만으로는 부족하다 — 같은 `request_id`로 같은 구성에
/// **다른 내용**이 오면 두 번째 요청이 `Duplicate`로 흡수되어 조용히 버려지고, 호출자는 그 사실을
/// 모른 채 성공을 받는다. stdin이 줄 지문을 대상에 접어 넣는 것과 같은 판단이며(`input_target`),
/// 내용이 다르면 흡수가 아니라 `TargetMismatch`로 호출자에게 알린다.
///
/// 지문을 `serde_json::Value`로 한 번 옮겨 뜨는 것은 `HashMap` 때문이다 — 맵을 직접 순회하면
/// 순회 순서가 인스턴스마다 달라 같은 내용의 재시도가 다른 지문을 얻는다. `serde_json`의 `Map`은
/// `BTreeMap`이므로(이 크레이트에 `preserve_order` 피처를 켜지 않았다) 키가 정렬되어 같은 내용이
/// 항상 같은 문자열이 된다. 원문을 대상에 그대로 담지 않는 이유는 body 상한이 1 MiB이고, 대상
/// 문자열은 로그에 남아 `dedup_conflict` 문구에까지 실리기 때문이다.
///
/// 직렬화가 실패하면 매번 다른 지문을 써서 흡수하지 않는다. 그 방향이 안전한 근거: update는 같은
/// 패치를 두 번 적용해도 결과가 같고, create의 두 번째 호출은 `check_name_available`이 이름
/// 중복으로 거절한다 — 어느 쪽도 조용한 유실로 끝나지 않는다.
fn edit_target<T: Serialize>(identifier: &str, request: &T) -> String {
    let canonical = serde_json::to_value(request)
        .map(|value| value.to_string())
        .unwrap_or_else(|_| Uuid::new_v4().to_string());
    let mut hasher = DefaultHasher::new();
    canonical.hash(&mut hasher);
    format!("{identifier} #{:016x}", hasher.finish())
}

/// stdin 요청의 중복 판정 대상. 세션 id만으로는 부족하다 — `request_id`를 세션당 하나 쓰는
/// 자연스러운 사용에서 두 번째 줄부터 전부 "이미 보냈다"로 흡수되어, 대화형 프롬프트가 답을
/// 받지 못하고 멈춘다.
///
/// 줄을 그대로 기록하지 않고 지문만 섞는 것은 `MAX_INPUT_LINE_BYTES`가 이미 기록 크기를 묶어
/// 둔 뒤에도 남는 두 가지 때문이다 — 항목 크기가 입력 상한 변경과 무관하게 고정되고, 충돌 문구가
/// 호출자의 원문을 되돌려 싣지 않는다. 지문 비교는 같은 scope·같은 `request_id`를 찾은 뒤에만
/// 1대1로 일어나므로(`RequestLog::lookup`), 서로 다른 줄이 같은 요청으로 읽힐 확률은 항목 수와
/// 무관하게 사건당 2^-64다.
fn input_target(session_id: Uuid, text: &str) -> String {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    format!("session {session_id} line #{:016x}", hasher.finish())
}

fn session_not_found(session_id: Uuid) -> String {
    format!("No session with id {session_id}")
}

fn find_configuration<'a>(
    configurations: &'a [RunConfiguration],
    target: &str,
) -> Option<&'a RunConfiguration> {
    find_configuration_index(configurations, target).map(|index| &configurations[index])
}

/// 결과 상태의 작업 디렉터리 불변식. 파서는 `type_data`를 함께 싣지 않은 패치에서 결과 타입을
/// 알 수 없으므로(`require_working_directory`), 최종 타입과 최종 디렉터리를 함께 아는 자리는
/// 여기뿐이다. 빈 값으로는 설정된 디렉터리에서 돌 수 없다 — 비-PTY 폴백은 `ENOENT`로 실패하고,
/// 기본 PTY 경로는 그 값을 버리고 `$HOME`으로 접었다(그래서 스폰 전 `verify_working_directory`).
///
/// Node에서 보는 것이 `project_directory`인 이유는 `derive_node_working_directory`가 싣는다.
fn check_working_directory(
    type_data: &ConfigTypeData,
    working_directory: &str,
) -> Result<(), String> {
    let (field, value) = match type_data {
        // compound 자신의 디렉터리는 어느 실행에도 닿지 않는다 — `run_compound`가 멤버의 구성을
        // 각자 실행하고, GUI 편집기도 그래서 이 입력을 compound에 표시하지 않는다.
        ConfigTypeData::Compound { .. } => return Ok(()),
        ConfigTypeData::Node {
            project_directory, ..
        } => ("project_directory", project_directory.as_str()),
        _ => ("working_directory", working_directory),
    };
    if value.trim().is_empty() {
        return Err(format!(
            "{field} cannot be empty: it becomes the process working directory"
        ));
    }
    Ok(())
}

/// Node의 작업 디렉터리를 `project_directory`에서 파생시킨다.
///
/// GUI 편집기는 Node에 이 입력을 주지 않는다 — `view_node_working_directory_row`가 읽기 전용으로
/// 보여 주고, 값은 고른 package.json의 부모로 채워진다. 호출자에게서 따로 받으면 화면이 가리키는
/// 프로젝트와 실제 cwd가 갈리는데, 실행에 쓰이는 것은 이 필드다(executor의 Node 분기가
/// `project_directory`를 버린다). 하위 디렉터리에서 돌리려는 호출자는 `project_directory`를 그
/// 디렉터리로 잡는다 — GUI가 monorepo에서 하는 일과 같다.
fn derive_node_working_directory(config: &mut RunConfiguration) {
    if let ConfigTypeData::Node {
        project_directory, ..
    } = &config.type_data
    {
        let derived = project_directory.clone();
        config.working_directory = derived;
    }
}

/// 마스킹된 값을 그대로 되돌려 쓰는 것을 막는다.
///
/// 조건 없이 거절하는 것은 호출자가 `Expose env values` 토글 상태를 관측할 수 없기 때문이다 —
/// 토글에 따라 통과·거절이 갈리면 같은 호출이 앱 설정에 따라 다르게 동작하고, 호출자는 그 이유를
/// 알 수 없다. 토글이 꺼진 채로 조회한 구성을 그대로 되돌려 쓰면 실제 비밀이 자리표시자로
/// 덮이는데, 그 손실은 되돌릴 수 없다.
fn reject_masked_env(values: &HashMap<String, String>) -> Result<(), String> {
    for (key, value) in values {
        if value == MASKED_VALUE {
            return Err(format!(
                "Environment variable '{key}' was sent as the mask placeholder '{MASKED_VALUE}'; \
                 send the real value or leave the variable out of the call"
            ));
        }
    }
    Ok(())
}

/// `find_configuration`과 같은 조회 규칙(id 우선, 그다음 정확한 이름)을 인덱스로 돌려준다.
/// 실행·편집 경로가 목록을 제자리에서 고치거나 그 자리를 지우므로 인덱스 형태가 필요하다.
///
/// 대상이 UUID로 파싱되면 이름 조회를 하지 않는다 — 그래서 UUID 모양 이름은 애초에 만들 수
/// 없게 막는다(`require_configuration_name`).
fn find_configuration_index(configurations: &[RunConfiguration], target: &str) -> Option<usize> {
    if let Ok(id) = Uuid::parse_str(target) {
        return configurations.iter().position(|config| config.id == id);
    }
    configurations
        .iter()
        .position(|config| config.name == target)
}

/// 재실행 응답. 넘겨받은 id가 더 이상 존재하지 않는다는 사실과, 두 실행의 출력이 한 버퍼에
/// 남는 경계를 함께 보이게 한다. `previous_last_line_id`를 `since_line_id`로 넘기면 새 실행분만
/// 읽힌다.
fn rerun_payload(
    mut entry: Value,
    previous_session_id: Uuid,
    previous_last_line_id: Option<usize>,
    deduplicated: bool,
) -> Value {
    entry["previous_session_id"] = json!(previous_session_id);
    entry["previous_last_line_id"] = json!(previous_last_line_id);
    entry["deduplicated"] = json!(deduplicated);
    entry
}

/// stdin 응답. `status`가 `pending`이면 아직 전달되지 않았을 뿐 실패가 아니다.
fn input_payload(mut entry: Value, status: &str, detail: Option<&str>) -> Value {
    entry["status"] = json!(status);
    if let Some(detail) = detail {
        entry["detail"] = json!(detail);
    }
    entry
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
        // 종료 코드가 없는 실패의 사유.
        "error": session.run_error,
        "started_at_unix_ms": unix_millis(session.started_at),
        "duration_ms": session.run_duration().map(|d| millis(d.as_millis())),
        "line_count": session.output_lines.len(),
    })
}

/// 출력 조회 결과. ANSI 스타일은 텍스트로 평탄화한다(에이전트가 읽는 건 내용이다).
fn session_output(
    session: &RunSession,
    tail_lines: usize,
    since_line_id: Option<usize>,
    expose_env: bool,
) -> Value {
    let selected: Vec<&(usize, Vec<TextSegment>)> = session
        .output_lines
        .iter()
        .filter(|(line_id, _)| since_line_id.is_none_or(|since| *line_id > since))
        .collect();

    let omitted = selected.len().saturating_sub(tail_lines);
    let visible = &selected[omitted..];
    let text = visible
        .iter()
        .map(|line| masked_line(session, line, expose_env))
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
fn session_search(session: &RunSession, query: &str, regex: bool, expose_env: bool) -> Value {
    let all = session.search_matches(query, regex);
    // 마스킹된 배너 줄은 매치 집계에서도 뺀다. 개수만 돌려줘도 호출자가 값을 하나씩 넣어 보고
    // 맞았는지 확인할 수 있어(oracle), 텍스트만 가려서는 마스킹이 성립하지 않는다.
    let matches: Vec<_> = all
        .iter()
        .filter(|hit| {
            expose_env
                || !session
                    .output_lines
                    .get(hit.line_idx)
                    .is_some_and(|(line_id, _)| session.is_env_banner_line(*line_id))
        })
        .collect();

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

/// 읽기 응답에 실을 한 줄. 마스킹이 켜져 있으면 앱이 찍은 환경변수 배너를 키까지 포함해 줄째로 가린다.
///
/// GUI의 `show_environment_on_run`은 사용자가 **화면에서** 볼 것을 정한 설정이므로, 배너가
/// 출력 버퍼에 남아 있다는 사실이 그 값을 에이전트에게까지 넘길 근거가 되지는 않는다.
/// `k=v; k=v`를 쌍으로 쪼개 키만 남기지 않는 것은 값 자체가 `; k=`를 담을 수 있어 그때 키로
/// 인쇄되는 바이트가 비밀에서 나오기 때문이다 — 키 목록은 `list_configurations`가 이미 준다.
/// 프로그램이 스스로 찍은 환경변수(`env` 실행 등)는 앱이 기록한 배너가 아니므로 가리지 않는다.
fn masked_line(session: &RunSession, line: &(usize, Vec<TextSegment>), expose_env: bool) -> String {
    let (line_id, segments) = line;
    if expose_env || !session.is_env_banner_line(*line_id) {
        return line_text(segments);
    }
    format!("{ENV_BANNER_PREFIX}{MASKED_VALUE}")
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
        SessionStatusKind::Errored => "errored",
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
    use crate::app::export_modal::ExportModalState;
    use crate::models::{ConfigTypeData, ExecuteMode, RunFailure};
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;
    use std::time::Duration;
    use tokio::sync::mpsc;

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

    /// 어떤 구성도 가리키지 않는 세션. 출력을 읽고 검색하는 테스트용이며, 재실행을 타는
    /// 테스트는 `attach_first_session_to_first_configuration`으로 앱의 구성에 이어 준다.
    fn session_with_output(lines: &[&str]) -> RunSession {
        let mut session = RunSession::new(Uuid::new_v4(), "build".to_string());
        for line in lines {
            session.add_output_line(line);
        }
        session
    }

    /// 픽스처 세션을 앱의 구성에 잇는다. 재실행은 세션이 담은 구성 id로 조회하므로, 이 결합이
    /// 없으면 재실행 경로가 "구성 없음"으로 끝나고 테스트가 그 경로를 보지 못한다.
    fn attach_first_session_to_first_configuration(app: &mut RunConfigManager) {
        app.sessions[0].config_id = app.configurations[0].id;
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
        let payload = session_output(&session, 2, None, false);

        assert_eq!(payload["text"], "c\nd");
        assert_eq!(payload["returned_lines"], 2);
        assert_eq!(payload["omitted_lines"], 2);
        assert_eq!(payload["buffered_lines"], 4);
    }

    #[test]
    fn output_since_line_id_returns_only_new_lines() {
        let session = session_with_output(&["a", "b", "c"]);
        let first = session_output(&session, 2, None, false);
        let cursor = first["last_line_id"].as_u64().expect("cursor") as usize;

        let follow_up = session_output(&session, 100, Some(cursor), false);
        assert_eq!(follow_up["returned_lines"], 0);
        assert_eq!(follow_up["text"], "");

        let from_start = session_output(&session, 100, Some(0), false);
        assert_eq!(from_start["returned_lines"], 2, "line ids start at 0");
    }

    #[test]
    fn output_of_an_empty_session_is_empty() {
        let payload = session_output(&session_with_output(&[]), 10, None, false);
        assert_eq!(payload["text"], "");
        assert_eq!(payload["returned_lines"], 0);
        assert!(payload["last_line_id"].is_null());
    }

    #[test]
    fn search_folds_multiple_matches_on_one_line() {
        let session = session_with_output(&["error here", "fine", "error error"]);
        let payload = session_search(&session, "error", false, false);

        assert_eq!(payload["total_matches"], 3);
        assert_eq!(payload["matched_lines"].as_array().expect("array").len(), 2);
        assert_eq!(payload["matched_lines"][0]["match_count"], 1);
        assert_eq!(payload["matched_lines"][1]["match_count"], 2);
        assert_eq!(payload["truncated"], false);
    }

    #[test]
    fn search_supports_regex_mode() {
        let session = session_with_output(&["exit code 3", "ok"]);
        let payload = session_search(&session, r"code \d", true, false);
        assert_eq!(payload["total_matches"], 1);
        assert_eq!(payload["matched_lines"][0]["text"], "exit code 3");
    }

    #[test]
    fn search_without_matches_is_empty() {
        let session = session_with_output(&["all good"]);
        let payload = session_search(&session, "panic", false, false);
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
        let payload = session_search(&session_with_output(&refs), "error", false, false);

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

    /// Execute 단계로 올린 앱 + 구성 하나. 실행 툴 테스트의 공통 출발점이다.
    ///
    /// 반환되는 Task는 **폴링하지 않는다** — `Task::run`의 스트림은 게으르므로 프로세스가
    /// 실제로 뜨지 않고, 테스트가 개발 머신에서 명령을 실행하지 않는다.
    fn app_at_execute_tier() -> RunConfigManager {
        let (mut app, _task) = RunConfigManager::new();
        app.reset_mcp_state_for_test();
        app.mcp_permission = McpPermission::Execute;
        app.configurations = vec![configuration("build")];
        app
    }

    fn respond_to(app: &mut RunConfigManager, op: McpOp) -> McpOutcome {
        let (request, mut answers) = McpRequest::channel_for_test(op);
        let _task = app.handle_mcp_request(request);
        answers.try_recv().expect("responded")
    }

    fn payload_of(outcome: McpOutcome) -> Value {
        match outcome {
            McpOutcome::Payload(payload) => payload,
            other => panic!("expected a payload, got {other:?}"),
        }
    }

    fn failure_of(outcome: McpOutcome) -> String {
        match outcome {
            McpOutcome::Failure(message) => message,
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    fn run_op(request_id: Option<&str>) -> McpOp {
        McpOp::RunConfiguration {
            target: "build".to_string(),
            request_id: request_id.map(str::to_string),
        }
    }

    #[test]
    fn running_a_configuration_answers_with_the_session_and_its_state() {
        // 실행 툴은 완료를 기다리지 않으므로, 상태를 함께 싣지 않으면 호출자가 "시작됨"과
        // "끝났는데 출력이 없음"을 구별할 수 없다.
        let mut app = app_at_execute_tier();
        let payload = payload_of(respond_to(&mut app, run_op(None)));

        assert_eq!(app.sessions.len(), 1);
        assert_eq!(payload["session_id"], json!(app.sessions[0].id));
        assert_eq!(payload["sessions"][0]["state"], "running");
        assert_eq!(payload["sessions"][0]["config_name"], "build");
        assert_eq!(payload["deduplicated"], false);
    }

    #[test]
    fn a_mutating_call_is_visible_in_the_status_bar() {
        // 사용자가 앱에서 에이전트의 조작을 직접 보는 유일한 경로다.
        let mut app = app_at_execute_tier();
        let _ = respond_to(&mut app, run_op(None));

        assert!(
            app.status_message.starts_with("[MCP]"),
            "{}",
            app.status_message
        );
        assert!(
            app.status_message.contains("build"),
            "{}",
            app.status_message
        );
    }

    #[test]
    fn the_same_request_id_does_not_start_a_second_run() {
        // 응답 전에 연결이 끊기면 HTTP 클라이언트가 같은 POST를 다시 보낸다.
        let mut app = app_at_execute_tier();
        let first = payload_of(respond_to(&mut app, run_op(Some("retry-1"))));
        let second = payload_of(respond_to(&mut app, run_op(Some("retry-1"))));

        assert_eq!(app.sessions.len(), 1, "the retry must not run it again");
        assert_eq!(first["session_id"], second["session_id"]);
        assert_eq!(second["deduplicated"], true);
    }

    #[test]
    fn a_full_retry_window_refuses_the_call_instead_of_running_it() {
        // 자리를 만들려고 아직 유효한 기록을 버리면 그 id의 재시도가 조용히 두 번 실행된다.
        // 거절은 "지켜지거나 거절되거나"로 멱등성을 좁힌다.
        let mut app = app_at_execute_tier();
        let now = Instant::now();
        for i in 0..MCP_DEDUP_MAX_ENTRIES {
            app.mcp_request_log.record(
                DedupScope::RunConfiguration,
                format!("filler-{i}"),
                "build".to_string(),
                vec![Uuid::new_v4()],
                now,
            );
        }

        let message = failure_of(respond_to(&mut app, run_op(Some("retry-1"))));

        assert!(app.sessions.is_empty(), "a refused call must not run");
        assert!(message.contains("request_id"), "{message}");
        assert!(
            app.status_message.contains("[MCP]"),
            "{}",
            app.status_message
        );
    }

    #[test]
    fn a_full_retry_window_still_lets_a_call_without_a_request_id_through() {
        // 거절 문구가 제시하는 회복 경로다. 창이 실행 툴 전체를 잠그면 거절이 DoS가 된다.
        let mut app = app_at_execute_tier();
        let now = Instant::now();
        for i in 0..MCP_DEDUP_MAX_ENTRIES {
            app.mcp_request_log.record(
                DedupScope::RunConfiguration,
                format!("filler-{i}"),
                "build".to_string(),
                vec![Uuid::new_v4()],
                now,
            );
        }

        let payload = payload_of(respond_to(&mut app, run_op(None)));

        assert_eq!(app.sessions.len(), 1);
        assert_eq!(payload["deduplicated"], false);
    }

    #[test]
    fn a_different_request_id_runs_it_again() {
        let mut app = app_at_execute_tier();
        let _ = respond_to(&mut app, run_op(Some("retry-1")));
        let _ = respond_to(&mut app, run_op(Some("retry-2")));

        assert_eq!(app.sessions.len(), 2, "a new request is a new run");
    }

    #[test]
    fn without_a_request_id_a_repeat_runs_again() {
        // 문서화된 동작이다 — 흡수할 키가 없으면 두 호출은 서로를 알 수 없다.
        let mut app = app_at_execute_tier();
        let _ = respond_to(&mut app, run_op(None));
        let _ = respond_to(&mut app, run_op(None));

        assert_eq!(app.sessions.len(), 2);
    }

    #[test]
    fn a_run_that_starts_nothing_is_a_failure_not_an_empty_success() {
        // 인앱 업데이트 가드가 실행을 거절하는 경로. 성공으로 답하면 호출자는 없는 세션을
        // 영원히 폴링한다.
        let mut app = app_at_execute_tier();
        app.is_updating = true;

        let message = failure_of(respond_to(&mut app, run_op(None)));

        assert!(app.sessions.is_empty());
        assert!(message.contains("build"), "{message}");
        assert!(message.contains("Update in progress"), "{message}");
    }

    #[test]
    fn a_compound_run_reports_every_member_session() {
        // 멤버마다 세션이 하나씩 생긴다 — 하나로 접으면 나머지가 조용히 사라진다.
        let mut app = app_at_execute_tier();
        let member = configuration("member");
        let member_id = member.id;
        app.configurations.push(member);
        app.configurations.push(RunConfiguration {
            name: "everything".to_string(),
            type_data: ConfigTypeData::Compound {
                members: vec![member_id, app.configurations[0].id],
                workspace: None,
            },
            ..RunConfiguration::default()
        });

        let payload = payload_of(respond_to(
            &mut app,
            McpOp::RunConfiguration {
                target: "everything".to_string(),
                request_id: None,
            },
        ));

        assert_eq!(app.sessions.len(), 2);
        assert_eq!(payload["sessions"].as_array().expect("array").len(), 2);
        assert!(
            payload["session_id"].is_null(),
            "a single id would hide the other session: {payload}"
        );
    }

    #[test]
    fn an_unknown_configuration_never_starts_a_session() {
        let mut app = app_at_execute_tier();
        let message = failure_of(respond_to(
            &mut app,
            McpOp::RunConfiguration {
                target: "missing".to_string(),
                request_id: None,
            },
        ));

        assert!(app.sessions.is_empty());
        assert!(message.contains("missing"), "{message}");
    }

    #[test]
    fn read_only_tier_refuses_to_run_anything() {
        // tier 재검사가 뚫리면 읽기 전용 설정이 임의 명령 실행을 허용한다.
        let mut app = app_at_execute_tier();
        app.mcp_permission = McpPermission::ReadOnly;

        match respond_to(&mut app, run_op(None)) {
            McpOutcome::Denied(reason) => assert!(reason.contains("Execute"), "{reason}"),
            other => panic!("expected a denial, got {other:?}"),
        }
        assert!(
            app.sessions.is_empty(),
            "a denied call must not run anything"
        );
    }

    #[test]
    fn rerun_answers_with_the_new_session_id() {
        // `handle_rerun_session`이 id를 새로 발급하므로, 옛 id를 돌려주면 호출자는 존재하지
        // 않는 세션을 폴링한다.
        let mut app = app_at_execute_tier();
        app.sessions = vec![session_with_output(&["old output"])];
        attach_first_session_to_first_configuration(&mut app);
        let old_id = app.sessions[0].id;

        let payload = payload_of(respond_to(
            &mut app,
            McpOp::RerunSession {
                session_id: old_id,
                request_id: None,
            },
        ));

        let new_id = app.sessions[0].id;
        assert_ne!(new_id, old_id);
        assert_eq!(payload["session_id"], json!(new_id));
        assert_eq!(payload["previous_session_id"], json!(old_id));
        assert_eq!(payload["state"], "running");
    }

    #[test]
    fn a_rerun_keeps_the_output_the_user_was_reading() {
        // 재실행이 출력을 지우면 에이전트의 호출이 사용자가 읽고 있던 화면을 파괴한다.
        // 경계는 새 실행이 찍는 배너(`═══` + `Configuration:`)가 그린다.
        let mut app = app_at_execute_tier();
        app.sessions = vec![session_with_output(&["first run output"])];
        attach_first_session_to_first_configuration(&mut app);
        let old_id = app.sessions[0].id;

        let _ = respond_to(
            &mut app,
            McpOp::RerunSession {
                session_id: old_id,
                request_id: None,
            },
        );

        let new_id = app.sessions[0].id;
        assert_ne!(
            new_id, old_id,
            "재실행이 시작되지 않으면 출력은 저절로 남으므로 아래 단정이 공허해진다"
        );
        assert!(
            read_output(&mut app, new_id).contains("first run output"),
            "the previous run's output must survive a rerun"
        );
    }

    #[test]
    fn a_rerun_hands_back_the_line_id_that_separates_the_two_runs() {
        // 출력을 보존하면 한 버퍼에 두 실행이 남는다. 응답이 경계를 주지 않으면, 재실행 뒤에
        // 처음 읽는 에이전트는 이전 실행의 실패 출력을 새 실행의 것으로 읽는다.
        let mut app = app_at_execute_tier();
        app.sessions = vec![session_with_output(&["first run output"])];
        attach_first_session_to_first_configuration(&mut app);
        let old_id = app.sessions[0].id;
        let boundary = app.sessions[0]
            .output_lines
            .back()
            .map(|(line_id, _)| *line_id);

        let payload = payload_of(respond_to(
            &mut app,
            McpOp::RerunSession {
                session_id: old_id,
                request_id: None,
            },
        ));

        assert_eq!(
            payload["previous_last_line_id"].as_u64(),
            boundary.map(|line_id| line_id as u64),
            "{payload}"
        );

        let new_id = app.sessions[0].id;
        let after = payload_of(respond_to(
            &mut app,
            McpOp::ReadSessionOutput {
                session_id: new_id,
                tail_lines: 10,
                since_line_id: boundary,
            },
        ));
        assert_eq!(
            after["returned_lines"], 0,
            "경계 이후로 읽으면 이전 실행의 출력이 섞이지 않는다: {after}"
        );
    }

    #[test]
    fn the_same_rerun_request_id_does_not_restart_twice() {
        let mut app = app_at_execute_tier();
        app.sessions = vec![session_with_output(&["old output"])];
        attach_first_session_to_first_configuration(&mut app);
        let old_id = app.sessions[0].id;

        let first = payload_of(respond_to(
            &mut app,
            McpOp::RerunSession {
                session_id: old_id,
                request_id: Some("retry-1".to_string()),
            },
        ));
        let restarted = app.sessions[0].id;
        let second = payload_of(respond_to(
            &mut app,
            McpOp::RerunSession {
                session_id: old_id,
                request_id: Some("retry-1".to_string()),
            },
        ));

        assert_eq!(
            app.sessions[0].id, restarted,
            "the retry must not restart it again"
        );
        assert_eq!(first["session_id"], second["session_id"]);
        assert_eq!(second["deduplicated"], true);
    }

    #[test]
    fn rerun_without_its_configuration_is_a_failure() {
        // 구성이 삭제되면 재실행이 시작되지 않는다 — id가 그대로인 것이 그 신호다.
        let mut app = app_at_execute_tier();
        app.sessions = vec![session_with_output(&["old output"])];
        app.configurations.clear();
        let old_id = app.sessions[0].id;

        let message = failure_of(respond_to(
            &mut app,
            McpOp::RerunSession {
                session_id: old_id,
                request_id: None,
            },
        ));

        assert_eq!(app.sessions[0].id, old_id, "nothing was restarted");
        assert!(message.contains("not found"), "{message}");
    }

    #[test]
    fn stopping_an_already_finished_session_is_not_an_error() {
        // 재시도가 그대로 안전해야 한다 — 이미 끝난 것은 호출자가 원한 결과와 같다.
        let mut app = app_at_execute_tier();
        let mut session = session_with_output(&["done"]);
        session.is_running = false;
        session.exit_code = Some(0);
        let session_id = session.id;
        app.sessions = vec![session];

        let payload = payload_of(respond_to(&mut app, McpOp::StopSession { session_id }));

        assert_eq!(payload["state"], "succeeded");
        assert_eq!(payload["exit_code"], 0);
        assert!(
            app.status_message.contains("had already finished"),
            "{}",
            app.status_message
        );
    }

    #[test]
    fn stopping_a_running_session_reports_it_stopped() {
        let mut app = app_at_execute_tier();
        let session = session_with_output(&["working"]);
        let session_id = session.id;
        app.sessions = vec![session];

        let payload = payload_of(respond_to(&mut app, McpOp::StopSession { session_id }));

        assert_eq!(payload["state"], "stopped");
        assert!(!app.sessions[0].is_running);
        assert!(
            app.status_message.starts_with("[MCP]"),
            "{}",
            app.status_message
        );
    }

    #[test]
    fn stopping_a_missing_session_is_a_failure() {
        let mut app = app_at_execute_tier();
        let unknown = Uuid::new_v4();
        let message = failure_of(respond_to(
            &mut app,
            McpOp::StopSession {
                session_id: unknown,
            },
        ));
        assert!(message.contains(&unknown.to_string()), "{message}");
    }

    #[test]
    fn input_to_a_finished_session_is_a_failure() {
        // 끝난 세션의 stdin은 받는 쪽이 없다 — 성공으로 답하면 에이전트는 답을 기다린다.
        let mut app = app_at_execute_tier();
        let mut session = session_with_output(&["done"]);
        session.is_running = false;
        let session_id = session.id;
        app.sessions = vec![session];

        let message = failure_of(respond_to(
            &mut app,
            McpOp::SendSessionInput {
                session_id,
                text: "y".to_string(),
                request_id: None,
            },
        ));
        assert!(message.contains("not running"), "{message}");
    }

    #[test]
    fn a_broken_stdin_write_is_reported_as_a_failure() {
        // 쓰기 결과를 기다리는 이유 — 즉시 성공으로 답하면 끊어진 stdin이 "전달됨"이 된다.
        let mut app = app_at_execute_tier();
        let session = session_with_output(&["prompt?"]);
        let session_id = session.id;
        app.sessions = vec![session];

        let (request, mut answers) = McpRequest::channel_for_test(McpOp::SendSessionInput {
            session_id,
            text: "y".to_string(),
            request_id: None,
        });
        let _task = app.handle_mcp_session_input_written(
            request,
            session_id,
            None,
            "y".to_string(),
            Err(StdinWriteError::Broken("EPIPE".to_string())),
        );

        let message = failure_of(answers.try_recv().expect("responded"));
        assert!(message.contains("EPIPE"), "{message}");
    }

    #[test]
    fn a_timed_out_stdin_write_is_pending_not_delivered() {
        // 제한 시간 초과는 쓰기가 아직 읽히지 않았다는 신호다 — 실패로 답하면 에이전트가
        // 다시 보내 입력이 두 번 들어간다.
        let mut app = app_at_execute_tier();
        let session = session_with_output(&["prompt?"]);
        let session_id = session.id;
        app.sessions = vec![session];

        let (request, mut answers) = McpRequest::channel_for_test(McpOp::SendSessionInput {
            session_id,
            text: "y".to_string(),
            request_id: None,
        });
        let _task = app.handle_mcp_session_input_written(
            request,
            session_id,
            None,
            "y".to_string(),
            Ok(false),
        );
        assert_eq!(
            payload_of(answers.try_recv().expect("responded"))["status"],
            "delivered"
        );

        let (request, mut answers) = McpRequest::channel_for_test(McpOp::SendSessionInput {
            session_id,
            text: "y".to_string(),
            request_id: None,
        });
        let _task = app.handle_mcp_session_input_written(
            request,
            session_id,
            None,
            "y".to_string(),
            Err(StdinWriteError::Timeout),
        );
        let payload = payload_of(answers.try_recv().expect("responded"));
        assert_eq!(payload["status"], "pending");
        assert!(payload["detail"].is_string(), "{payload}");
    }

    #[test]
    fn a_dedup_hit_reports_a_session_the_user_removed() {
        // 창 안이지만 사용자가 세션을 지웠다. 다시 실행하지 않는 것이 창의 존재 이유이므로,
        // 사라졌다는 사실을 그대로 알린다.
        let mut app = app_at_execute_tier();
        let _ = respond_to(&mut app, run_op(Some("retry-1")));
        app.sessions.clear();

        let payload = payload_of(respond_to(&mut app, run_op(Some("retry-1"))));

        assert!(app.sessions.is_empty(), "the retry must not run it again");
        assert_eq!(payload["sessions"][0]["state"], "removed");
        assert_eq!(payload["deduplicated"], true);
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

    /// 앱이 배너를 찍는 것과 같은 순서로 세션을 만든다. 환경변수 줄만 `add_env_banner_line`으로
    /// 들어가야 마스킹 판정이 실제 경로(`send_command_info`의 `EnvBanner` 이벤트)와 같아진다.
    fn session_with_env_banner() -> RunSession {
        let mut session = RunSession::new(Uuid::new_v4(), "build".to_string());
        session.add_output_line("Configuration: build");
        session.add_env_banner_line("Environment: API_TOKEN=super-secret; MODE=debug");
        session.add_output_line("Command: echo hello");
        session
    }

    fn read_output(app: &mut RunConfigManager, session_id: Uuid) -> String {
        let payload = payload_of(respond_to(
            app,
            McpOp::ReadSessionOutput {
                session_id,
                tail_lines: 10,
                since_line_id: None,
            },
        ));
        payload["text"].as_str().expect("text").to_string()
    }

    fn input_op(session_id: Uuid, request_id: Option<&str>) -> McpOp {
        McpOp::SendSessionInput {
            session_id,
            text: "y".to_string(),
            request_id: request_id.map(str::to_string),
        }
    }

    /// 응답을 받지 않고 요청만 통과시킨다. 답이 **아직 오지 않았다**는 것 자체를 검증하는
    /// 테스트가 필요해 `respond_to`(즉시 답을 요구)와 나눈다. Task를 함께 돌려주는 것은
    /// 그 안에 응답 채널이 들어 있기 때문이다 — 여기서 떨어뜨리면 아직 답하지 않은 요청이
    /// 채널 종료로 보인다. 이 Task는 폴링하지 않으므로 stdin 쓰기가 실제로 일어나지 않는다.
    fn submit(
        app: &mut RunConfigManager,
        op: McpOp,
    ) -> (Task<Message>, mpsc::Receiver<McpOutcome>) {
        let (request, answers) = McpRequest::channel_for_test(op);
        (app.handle_mcp_request(request), answers)
    }

    fn is_unanswered(answers: &mut mpsc::Receiver<McpOutcome>) -> bool {
        matches!(answers.try_recv(), Err(mpsc::error::TryRecvError::Empty))
    }

    #[test]
    fn reading_output_masks_the_env_banner() {
        // 실행 배너는 사용자가 화면에서 볼 것을 정한 설정에 따라 찍히지만, 그 사실이 값을
        // 에이전트에게 넘길 근거는 아니다.
        let mut app = app_at_execute_tier();
        app.sessions = vec![session_with_env_banner()];
        let session_id = app.sessions[0].id;

        let text = read_output(&mut app, session_id);

        assert!(!text.contains("super-secret"), "{text}");
        assert!(text.contains("Environment: <hidden>"), "{text}");
        assert!(
            text.contains("Command: echo hello"),
            "the rest of the banner must survive: {text}"
        );
    }

    #[test]
    fn a_program_printing_its_own_environment_line_is_not_masked() {
        // 배너 판정을 접두사로 하면 프로그램이 찍은 `Environment: ...`까지 가려, 에이전트가
        // 읽는 실행 출력에서 사용자 로그가 사라지고 검색에서도 사라진다. 판정은 앱이 찍을 때
        // 기록한 줄 id로만 한다.
        let mut app = app_at_execute_tier();
        app.sessions = vec![session_with_output(&[
            "Environment: production",
            "Deploying to Environment: production",
        ])];
        let session_id = app.sessions[0].id;

        let text = read_output(&mut app, session_id);
        assert!(text.contains("Environment: production"), "{text}");
        assert!(!text.contains(MASKED_VALUE), "{text}");

        let payload = payload_of(respond_to(
            &mut app,
            McpOp::SearchSessionOutput {
                session_id,
                query: "production".to_string(),
                regex: false,
            },
        ));
        assert_eq!(payload["total_matches"], 2);
    }

    #[test]
    fn exposing_env_values_returns_the_banner_as_printed() {
        let mut app = app_at_execute_tier();
        app.mcp_expose_env_values = true;
        app.sessions = vec![session_with_env_banner()];
        let session_id = app.sessions[0].id;

        let text = read_output(&mut app, session_id);

        assert!(text.contains("API_TOKEN=super-secret"), "{text}");
    }

    #[test]
    fn searching_cannot_confirm_a_masked_env_value() {
        // 매치 개수만으로도 값을 하나씩 넣어 보고 맞았는지 확인할 수 있다(oracle) — 텍스트만
        // 가려서는 마스킹이 성립하지 않는다.
        let mut app = app_at_execute_tier();
        app.sessions = vec![session_with_env_banner()];
        let session_id = app.sessions[0].id;

        let payload = payload_of(respond_to(
            &mut app,
            McpOp::SearchSessionOutput {
                session_id,
                query: "super-secret".to_string(),
                regex: false,
            },
        ));

        assert_eq!(payload["total_matches"], 0, "{payload}");
        assert!(
            payload["matched_lines"]
                .as_array()
                .expect("array")
                .is_empty(),
            "{payload}"
        );

        // 금지 조항을 실행 가능한 형태로 고정한다: 마스킹 여부에 따라 값이 달라지는 카운트가
        // 응답에 하나라도 생기면 위 oracle이 그대로 되살아난다. 키를 늘리는 변경은 여기서 멈춘다.
        let mut keys: Vec<&str> = payload
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "matched_lines",
                "query",
                "regex",
                "session_id",
                "total_matches",
                "truncated"
            ],
            "{payload}"
        );
    }

    #[test]
    fn masking_the_banner_leaves_the_rest_of_the_output_searchable() {
        let mut app = app_at_execute_tier();
        app.sessions = vec![session_with_env_banner()];
        let session_id = app.sessions[0].id;

        let payload = payload_of(respond_to(
            &mut app,
            McpOp::SearchSessionOutput {
                session_id,
                query: "echo".to_string(),
                regex: false,
            },
        ));

        assert_eq!(payload["total_matches"], 1, "{payload}");
        assert_eq!(payload["matched_lines"][0]["text"], "Command: echo hello");
    }

    #[test]
    fn a_stopped_session_still_reads_as_stopped_after_the_run_reports_back() {
        // stop 이후 실행기는 종료 코드 없이 끝났다고 보고한다 — 실행 실패와 같은 Err 경로다.
        // 그 보고가 사유를 세션에 남기면 상태가 `errored`로 뒤집혀, stop을 시킨 에이전트가
        // 자기 요청을 실패로 읽는다.
        let mut app = app_at_execute_tier();
        let session = session_with_output(&["working"]);
        let session_id = session.id;
        app.sessions = vec![session];

        let _ = respond_to(&mut app, McpOp::StopSession { session_id });
        let _ = app.handle_run_completed(session_id, Err(RunFailure::StoppedByUser));

        let payload = payload_of(respond_to(&mut app, McpOp::ListSessions));
        let entry = &payload["sessions"][0];

        assert_eq!(entry["state"], "stopped", "{entry}");
        assert!(entry["error"].is_null(), "{entry}");
    }

    #[test]
    fn the_stop_audit_line_survives_the_run_report() {
        // 상태바는 사용자가 앱에서 에이전트의 조작을 보는 유일한 줄이다. 실행 보고가 1~2초
        // 뒤 도착해 이 줄을 "Failed: ..."로 덮으면 사용자는 누가 세션을 멈췄는지 못 본다.
        let mut app = app_at_execute_tier();
        let session = session_with_output(&["working"]);
        let session_id = session.id;
        app.sessions = vec![session];

        let _ = respond_to(&mut app, McpOp::StopSession { session_id });
        let _ = app.handle_run_completed(session_id, Err(RunFailure::StoppedByUser));

        assert!(
            app.status_message.starts_with("[MCP] Stopped session"),
            "{}",
            app.status_message
        );
        assert!(
            !read_output(&mut app, session_id).contains("Error:"),
            "a deliberate stop must not be labelled an error"
        );
    }

    #[test]
    fn a_run_that_never_started_is_not_reported_as_a_user_stop() {
        // 종료 코드 없는 실패가 `stopped`로 접히면 에이전트는 사용자가 멈춘 것으로 읽는다.
        let mut app = app_at_execute_tier();
        let mut session = session_with_output(&["starting"]);
        session.is_running = false;
        session.run_error = Some("Failed to spawn process: No such file".to_string());
        app.sessions = vec![session];

        let payload = payload_of(respond_to(&mut app, McpOp::ListSessions));
        let entry = &payload["sessions"][0];

        assert_eq!(entry["state"], "errored");
        assert!(entry["exit_code"].is_null(), "{entry}");
        assert!(
            entry["error"]
                .as_str()
                .expect("the reason must travel with the state")
                .contains("No such file"),
            "{entry}"
        );
    }

    #[test]
    fn a_retry_is_absorbed_even_after_the_configuration_disappears() {
        // 재시도가 도착하는 시점에 구성이 삭제·개명됐을 수 있다. 판정 순서가 뒤바뀌면 이미
        // 실행된 요청에 "구성 없음"으로 답한다.
        let mut app = app_at_execute_tier();
        let first = payload_of(respond_to(&mut app, run_op(Some("retry-1"))));
        app.configurations.clear();

        let second = payload_of(respond_to(&mut app, run_op(Some("retry-1"))));

        assert_eq!(app.sessions.len(), 1, "the retry must not run it again");
        assert_eq!(first["session_id"], second["session_id"]);
        assert_eq!(second["deduplicated"], true);
    }

    #[test]
    fn a_request_id_reused_for_another_configuration_is_rejected() {
        // 첫 결과를 돌려주면 호출자가 요청하지 않은 구성의 세션을 받고, 새로 실행하면
        // `request_id`가 약속한 멱등성이 깨진다.
        let mut app = app_at_execute_tier();
        app.configurations.push(configuration("deploy"));
        let _ = respond_to(&mut app, run_op(Some("retry-1")));

        let message = failure_of(respond_to(
            &mut app,
            McpOp::RunConfiguration {
                target: "deploy".to_string(),
                request_id: Some("retry-1".to_string()),
            },
        ));

        assert_eq!(app.sessions.len(), 1, "the conflict must not run anything");
        assert!(message.contains("retry-1"), "{message}");
        assert!(message.contains("build"), "{message}");
    }

    #[test]
    fn a_rerun_that_cannot_start_leaves_the_process_alone() {
        // 구성 조회가 취소보다 뒤에 오면, 재실행되지 못할 세션의 프로세스만 죽는다 —
        // 호출자에게는 실패로 보이므로 아무 일도 없었던 것처럼 읽힌다.
        let mut app = app_at_execute_tier();
        app.sessions = vec![session_with_output(&["working"])];
        app.configurations.clear();
        let session_id = app.sessions[0].id;

        let message = failure_of(respond_to(
            &mut app,
            McpOp::RerunSession {
                session_id,
                request_id: None,
            },
        ));

        assert!(message.contains("not found"), "{message}");
        assert_eq!(app.sessions[0].id, session_id, "nothing was restarted");
        assert!(app.sessions[0].is_running);
        assert!(
            !app.sessions[0]
                .cancel_flag
                .load(std::sync::atomic::Ordering::Relaxed),
            "the surviving session's process must not have been cancelled"
        );
    }

    #[test]
    fn rerunning_a_missing_session_is_a_failure() {
        let mut app = app_at_execute_tier();
        let unknown = Uuid::new_v4();

        let message = failure_of(respond_to(
            &mut app,
            McpOp::RerunSession {
                session_id: unknown,
                request_id: None,
            },
        ));

        assert!(message.contains(&unknown.to_string()), "{message}");
    }

    #[test]
    fn input_to_a_missing_session_is_a_failure() {
        let mut app = app_at_execute_tier();
        let unknown = Uuid::new_v4();

        let message = failure_of(respond_to(&mut app, input_op(unknown, None)));

        assert!(message.contains(&unknown.to_string()), "{message}");
    }

    #[test]
    fn input_is_not_answered_before_the_write_resolves() {
        // 즉시 성공으로 답하면 끊어진 stdin이 호출자에게 "전달됨"으로 보인다.
        let mut app = app_at_execute_tier();
        let session = session_with_output(&["prompt?"]);
        let session_id = session.id;
        app.sessions = vec![session];

        let (_task, mut answers) = submit(&mut app, input_op(session_id, None));

        assert!(
            is_unanswered(&mut answers),
            "the caller must wait for the write"
        );
    }

    #[test]
    fn a_pipe_fallback_write_is_echoed_into_the_output() {
        // 폴백 pipe(Windows <1809)에는 터미널 에코가 없으므로 — `write_session_stdin`이 그
        // 경로에서만 `Ok(true)`를 돌려준다 — 사용자와 에이전트가 같은 화면을 보려면 앱이
        // 직접 남겨야 한다.
        let mut app = app_at_execute_tier();
        let session = session_with_output(&["prompt?"]);
        let session_id = session.id;
        app.sessions = vec![session];

        let (request, mut answers) = McpRequest::channel_for_test(input_op(session_id, None));
        let _task = app.handle_mcp_session_input_written(
            request,
            session_id,
            None,
            "y".to_string(),
            Ok(true),
        );

        assert_eq!(
            payload_of(answers.try_recv().expect("responded"))["status"],
            "delivered"
        );
        let text = read_output(&mut app, session_id);
        assert!(text.ends_with("y"), "{text}");
    }

    #[test]
    fn a_pty_write_is_not_echoed_by_the_app() {
        // PTY는 쓴 줄을 스스로 되돌려준다(`write_session_stdin`의 `Ok(false)`). 앱이 또 남기면
        // 같은 입력이 두 번 찍혀, 출력을 읽는 에이전트가 두 번 보낸 것으로 읽는다.
        let mut app = app_at_execute_tier();
        let session = session_with_output(&["prompt?"]);
        let session_id = session.id;
        app.sessions = vec![session];

        let (request, _answers) = McpRequest::channel_for_test(input_op(session_id, None));
        let _task = app.handle_mcp_session_input_written(
            request,
            session_id,
            None,
            "y".to_string(),
            Ok(false),
        );

        assert_eq!(read_output(&mut app, session_id), "prompt?");
    }

    #[test]
    fn a_broken_mcp_write_leaves_the_users_draft_alone() {
        // GUI 핸들러는 하드 실패 시 입력 드래프트를 복원한다. 그 경로를 재사용하면 에이전트가
        // 보낸 텍스트가 사용자의 입력창에 들어가고, 사용자의 다음 Enter가 그걸 다시 보낸다.
        let mut app = app_at_execute_tier();
        let mut session = session_with_output(&["prompt?"]);
        session.stdin_input = Some(String::new());
        let session_id = session.id;
        app.sessions = vec![session];

        let (request, _answers) = McpRequest::channel_for_test(input_op(session_id, None));
        let _task = app.handle_mcp_session_input_written(
            request,
            session_id,
            None,
            "y".to_string(),
            Err(StdinWriteError::Broken("EPIPE".to_string())),
        );

        assert_eq!(
            app.sessions[0].stdin_input.as_deref(),
            Some(""),
            "the agent's line must not land in the user's input bar"
        );
    }

    #[test]
    fn the_same_input_request_id_does_not_write_the_line_twice() {
        // 응답 전에 도착한 재시도가 흡수돼야 한다 — 대화형 프롬프트에 같은 답이 두 번 들어가면
        // 두 번째는 다음 질문의 답이 된다.
        let mut app = app_at_execute_tier();
        let session = session_with_output(&["prompt?"]);
        let session_id = session.id;
        app.sessions = vec![session];

        let (_task, mut answers) = submit(&mut app, input_op(session_id, Some("retry-1")));
        assert!(is_unanswered(&mut answers), "the first call is in flight");

        let payload = payload_of(respond_to(&mut app, input_op(session_id, Some("retry-1"))));

        assert_eq!(payload["status"], "duplicate");
        assert!(payload["detail"].is_string(), "{payload}");
    }

    #[test]
    fn a_broken_write_lets_the_same_input_request_id_try_again() {
        // 파이프가 닫혔으니 줄은 확실히 도착하지 않았다. 기록을 남겨 두면 재시도가 "이미
        // 보냈다"로 흡수되어, 한 줄도 전달되지 않은 요청이 성공으로 굳는다.
        let mut app = app_at_execute_tier();
        let session = session_with_output(&["prompt?"]);
        let session_id = session.id;
        app.sessions = vec![session];

        let _first = submit(&mut app, input_op(session_id, Some("retry-1")));
        let (request, mut answers) =
            McpRequest::channel_for_test(input_op(session_id, Some("retry-1")));
        let _task = app.handle_mcp_session_input_written(
            request,
            session_id,
            Some("retry-1".to_string()),
            "y".to_string(),
            Err(StdinWriteError::Broken("EPIPE".to_string())),
        );
        let _ = failure_of(answers.try_recv().expect("responded"));

        let (_task, mut retried) = submit(&mut app, input_op(session_id, Some("retry-1")));

        assert!(
            is_unanswered(&mut retried),
            "the retry must write again instead of being absorbed"
        );
    }

    #[test]
    fn a_request_id_reused_for_another_line_is_rejected() {
        // 에이전트는 세션당 하나의 id — `request_id = session_id` — 를 쓰는 것이 자연스럽다.
        // 그때 두 번째 줄부터 "이미 보냈다"로 흡수되면 프로그램은 답을 기다리고 에이전트는
        // 보냈다고 믿어, 대화형 프롬프트가 교착한다. 흡수 대신 거절해야 알아차릴 수 있다.
        let mut app = app_at_execute_tier();
        let session = session_with_output(&["name?"]);
        let session_id = session.id;
        app.sessions = vec![session];

        let _in_flight = submit(&mut app, input_op(session_id, Some("retry-1")));
        let message = failure_of(respond_to(
            &mut app,
            McpOp::SendSessionInput {
                session_id,
                text: "n".to_string(),
                request_id: Some("retry-1".to_string()),
            },
        ));

        assert!(message.contains("retry-1"), "{message}");
        assert!(message.contains(&session_id.to_string()), "{message}");
    }

    #[test]
    fn a_request_id_reused_for_another_session_is_rejected() {
        let mut app = app_at_execute_tier();
        let first = session_with_output(&["prompt?"]);
        let second = session_with_output(&["prompt?"]);
        let (first_id, second_id) = (first.id, second.id);
        app.sessions = vec![first, second];

        let _in_flight = submit(&mut app, input_op(first_id, Some("retry-1")));
        let message = failure_of(respond_to(&mut app, input_op(second_id, Some("retry-1"))));

        assert!(message.contains(&first_id.to_string()), "{message}");
    }

    #[test]
    fn every_input_outcome_is_visible_in_the_status_bar() {
        // 사용자가 앱에서 에이전트의 조작을 직접 보는 유일한 경로다.
        let mut app = app_at_execute_tier();
        let session = session_with_output(&["prompt?"]);
        let session_id = session.id;
        app.sessions = vec![session];

        let _in_flight = submit(&mut app, input_op(session_id, Some("retry-1")));
        assert!(
            app.status_message.starts_with("[MCP] Sending input"),
            "{}",
            app.status_message
        );

        let _ = respond_to(&mut app, input_op(session_id, Some("retry-1")));
        assert!(
            app.status_message.starts_with("[MCP] Duplicate input"),
            "{}",
            app.status_message
        );

        for (result, expected) in [
            (Err(StdinWriteError::Timeout), "[MCP] Input pending"),
            (
                Err(StdinWriteError::Broken("EPIPE".to_string())),
                "[MCP] Input failed",
            ),
        ] {
            let (request, _answers) = McpRequest::channel_for_test(input_op(session_id, None));
            let _task = app.handle_mcp_session_input_written(
                request,
                session_id,
                None,
                "y".to_string(),
                result,
            );
            assert!(
                app.status_message.starts_with(expected),
                "{}",
                app.status_message
            );
        }
    }

    /// Edit 단계로 올린 앱 + 구성 하나. 편집 툴 테스트의 공통 출발점이다.
    ///
    /// `configs_ready`를 세우는 것은 흉내가 아니라 전제다 — 로드가 끝나기 전의 편집은
    /// 거절되므로(`an_edit_is_refused_until_the_stored_configurations_have_loaded`),
    /// 그 상태에서는 나머지 동작을 아예 관찰할 수 없다.
    fn app_at_edit_tier() -> RunConfigManager {
        let mut app = app_at_execute_tier();
        app.mcp_permission = McpPermission::Edit;
        app.configs_ready = true;
        app
    }

    fn create_args(name: &str) -> CreateConfigurationArgs {
        CreateConfigurationArgs {
            name: name.to_string(),
            working_directory: "/tmp/new".to_string(),
            environment_variables: HashMap::new(),
            type_data: ConfigTypeData::Application {
                command: "echo".to_string(),
                arguments: String::new(),
            },
            request_id: None,
        }
    }

    fn create_op(name: &str) -> McpOp {
        McpOp::CreateConfiguration(Box::new(create_args(name)))
    }

    /// 아무것도 바꾸지 않는 패치. 테스트가 바꾸려는 필드만 덮어쓴다.
    fn patch(configuration: &str) -> UpdateConfigurationArgs {
        UpdateConfigurationArgs {
            configuration: configuration.to_string(),
            name: None,
            working_directory: None,
            type_data: None,
            set_environment_variables: HashMap::new(),
            remove_environment_variables: Vec::new(),
            request_id: None,
        }
    }

    fn delete_op(configuration: &str, request_id: Option<&str>) -> McpOp {
        McpOp::DeleteConfiguration(DeleteConfigurationArgs {
            configuration: configuration.to_string(),
            request_id: request_id.map(str::to_string),
        })
    }

    #[test]
    fn creating_a_configuration_adds_it_with_a_server_chosen_id() {
        let mut app = app_at_edit_tier();

        let payload = payload_of(respond_to(&mut app, create_op("deploy")));

        assert_eq!(app.configurations.len(), 2);
        let created = app.configurations.last().expect("created");
        assert_eq!(created.name, "deploy");
        assert_eq!(created.working_directory, "/tmp/new");
        assert_eq!(payload["configuration_id"], json!(created.id));
        assert_eq!(payload["configuration"]["name"], json!("deploy"));
        assert_eq!(payload["deduplicated"], json!(false));
    }

    #[test]
    fn an_edit_is_refused_until_the_stored_configurations_have_loaded() {
        // 로드 핸들러는 `configurations`를 통째로 교체한다 — 로드 전에 만든 구성은 그 교체에
        // 조용히 지워지고, 호출자는 성공 응답을 이미 받은 뒤다.
        let mut app = app_at_edit_tier();
        app.configs_ready = false;

        let failure = failure_of(respond_to(&mut app, create_op("deploy")));

        assert_eq!(app.configurations.len(), 1, "아무것도 만들지 않는다");
        assert!(failure.contains("loading"), "{failure}");
    }

    #[test]
    fn a_masked_env_value_is_refused_instead_of_overwriting_the_secret() {
        // 조회 응답을 고쳐 되돌려 보내는 것이 가장 자연스러운 편집 방법이다. 마스킹된 값을
        // 그대로 받으면 에이전트가 볼 수도 없었던 실제 비밀이 자리표시자로 파괴된다.
        let mut app = app_at_edit_tier();
        let op = McpOp::UpdateConfiguration(Box::new(UpdateConfigurationArgs {
            set_environment_variables: HashMap::from([(
                "API_TOKEN".to_string(),
                MASKED_VALUE.to_string(),
            )]),
            ..patch("build")
        }));

        let failure = failure_of(respond_to(&mut app, op));

        assert_eq!(
            app.configurations[0].environment_variables["API_TOKEN"], "super-secret",
            "실제 값이 남아 있어야 한다"
        );
        assert!(failure.contains(MASKED_VALUE), "{failure}");
    }

    #[test]
    fn creating_a_configuration_under_a_taken_name_is_refused() {
        // 이름은 `run_configuration`·`get_configuration`이 대상을 찾는 주소다. 중복을 허용하면
        // 이름으로 부른 호출이 둘 중 하나를 조용히 고른다.
        let mut app = app_at_edit_tier();

        let failure = failure_of(respond_to(&mut app, create_op("build")));

        assert_eq!(app.configurations.len(), 1);
        assert!(failure.contains("build"), "{failure}");
    }

    #[test]
    fn a_compound_member_that_does_not_exist_is_refused() {
        // 앱은 삭제할 때 댕글링 멤버를 능동적으로 걷어낸다 — 그 불변식을 쓰기 경로에서도 지킨다.
        // 받아 주면 실행이 그 멤버를 조용히 건너뛰고, 호출자는 무엇이 빠졌는지 알 수 없다.
        let mut app = app_at_edit_tier();
        let missing = Uuid::new_v4();
        let op = McpOp::CreateConfiguration(Box::new(CreateConfigurationArgs {
            type_data: ConfigTypeData::Compound {
                members: vec![missing],
                workspace: None,
            },
            ..create_args("bundle")
        }));

        let failure = failure_of(respond_to(&mut app, op));

        assert_eq!(app.configurations.len(), 1);
        assert!(failure.contains(&missing.to_string()), "{failure}");
    }

    #[test]
    fn an_update_touches_only_the_fields_it_carries() {
        let mut app = app_at_edit_tier();
        let op = McpOp::UpdateConfiguration(Box::new(UpdateConfigurationArgs {
            working_directory: Some("/tmp/moved".to_string()),
            ..patch("build")
        }));

        payload_of(respond_to(&mut app, op));

        let config = &app.configurations[0];
        assert_eq!(config.working_directory, "/tmp/moved");
        assert_eq!(config.name, "build");
        assert_eq!(config.environment_variables["API_TOKEN"], "super-secret");
        assert!(matches!(
            config.type_data,
            ConfigTypeData::Application { .. }
        ));
    }

    #[test]
    fn an_update_sets_and_removes_environment_variables() {
        // 환경변수를 통째로 교체받지 않는 이유가 여기 있다 — 값이 마스킹되는 호출자는 남길
        // 값을 되쓸 수 없으므로, 바꿀 키만 지목하게 한다.
        let mut app = app_at_edit_tier();
        let op = McpOp::UpdateConfiguration(Box::new(UpdateConfigurationArgs {
            set_environment_variables: HashMap::from([("MODE".to_string(), "release".to_string())]),
            remove_environment_variables: vec!["API_TOKEN".to_string()],
            ..patch("build")
        }));

        payload_of(respond_to(&mut app, op));

        let env = &app.configurations[0].environment_variables;
        assert_eq!(env["MODE"], "release");
        assert!(!env.contains_key("API_TOKEN"));
    }

    #[test]
    fn an_env_update_drops_the_users_stale_bulk_draft() {
        // 대량 편집기의 초안은 구성의 환경변수를 직렬화해 채워 둔 사본이다. 남겨 두면 사용자가
        // Apply를 누르는 순간 에이전트의 변경이 조용히 되돌아간다.
        let mut app = app_at_edit_tier();
        let config_id = app.configurations[0].id;
        app.env_bulk_inputs
            .insert(config_id, String::from("API_TOKEN=super-secret"));

        let op = McpOp::UpdateConfiguration(Box::new(UpdateConfigurationArgs {
            set_environment_variables: HashMap::from([("MODE".to_string(), "release".to_string())]),
            ..patch("build")
        }));
        payload_of(respond_to(&mut app, op));

        assert!(!app.env_bulk_inputs.contains_key(&config_id));
    }

    #[test]
    fn a_rename_keeps_the_users_bulk_env_draft() {
        // 초안을 버리는 것은 환경변수가 실제로 바뀌었을 때의 대가다 — 무관한 변경까지 버리면
        // 에이전트의 개명이 사용자가 입력 중인 내용을 삼킨다.
        let mut app = app_at_edit_tier();
        let config_id = app.configurations[0].id;
        app.env_bulk_inputs
            .insert(config_id, String::from("DRAFT=1"));

        let op = McpOp::UpdateConfiguration(Box::new(UpdateConfigurationArgs {
            name: Some("build-2".to_string()),
            ..patch("build")
        }));
        payload_of(respond_to(&mut app, op));

        assert_eq!(app.env_bulk_inputs[&config_id], "DRAFT=1");
    }

    #[test]
    fn deleting_a_configuration_reports_the_compounds_that_lost_a_member() {
        // GUI는 이 목록을 확인 모달에서 경고로 보여 준다. MCP 삭제는 모달을 거치지 않으므로
        // 같은 사실을 응답에 실어야 호출자가 자기 조작의 부수 효과를 본다.
        let mut app = app_at_edit_tier();
        let member_id = app.configurations[0].id;
        app.configurations.push(RunConfiguration {
            name: "bundle".to_string(),
            type_data: ConfigTypeData::Compound {
                members: vec![member_id],
                workspace: None,
            },
            ..RunConfiguration::default()
        });

        let payload = payload_of(respond_to(&mut app, delete_op("build", None)));

        assert_eq!(payload["configuration_id"], json!(member_id));
        assert_eq!(payload["referencing_compounds"], json!(["bundle"]));
        let ConfigTypeData::Compound { members, .. } = &app.configurations[0].type_data else {
            panic!("expected the compound to survive");
        };
        assert!(members.is_empty(), "댕글링 참조가 남지 않는다");
    }

    #[test]
    fn deleting_a_configuration_keeps_the_export_selection_a_subset() {
        // GUI에서는 삭제 확인 모달을 여는 `close_all_modals()`가 이 불변식을 우연히 지켜
        // 주었다. MCP 삭제는 모달을 거치지 않으므로 삭제 경로가 직접 지켜야 한다.
        let mut app = app_at_edit_tier();
        let config_id = app.configurations[0].id;
        app.export_modal = Some(ExportModalState {
            selected: HashSet::from([config_id]),
        });

        payload_of(respond_to(&mut app, delete_op("build", None)));

        let modal = app
            .export_modal
            .as_ref()
            .expect("모달은 사용자 것이므로 닫지 않는다");
        assert!(
            modal.selected.is_empty(),
            "사라진 구성의 id가 선택에 남아 전체 선택 판정을 깨뜨린다"
        );
    }

    #[test]
    fn an_edit_leaves_an_audit_line_in_the_status_bar() {
        // 보안 항목 8: 변경성 호출은 사용자가 앱에서 직접 봐야 한다.
        let mut app = app_at_edit_tier();

        payload_of(respond_to(&mut app, create_op("deploy")));

        assert!(
            app.status_message.starts_with("[MCP] "),
            "{}",
            app.status_message
        );
        assert!(
            app.status_message.contains("deploy"),
            "{}",
            app.status_message
        );
    }

    #[test]
    fn a_finished_save_leaves_the_audit_line_alone() {
        // 편집 툴은 저장을 스스로 발행한다(에이전트에게는 Save 버튼이 없다). 저장 완료가
        // 상태바를 덮으면 방금 남긴 감사 줄이 사라져 보안 항목 8이 무력해진다.
        let mut app = app_at_edit_tier();
        payload_of(respond_to(&mut app, create_op("deploy")));
        let audit = app.status_message.clone();

        let _task = app.handle_mcp_configurations_saved(Ok(PathBuf::from("/tmp/configs.json")));

        assert_eq!(app.status_message, audit);
    }

    #[test]
    fn a_failed_save_is_reported_as_an_mcp_failure() {
        // 성공만 조용하다 — 저장이 실패했는데도 감사 줄이 그대로면 사용자는 변경이 디스크에
        // 남은 줄로 읽는다.
        let mut app = app_at_edit_tier();
        payload_of(respond_to(&mut app, create_op("deploy")));

        let _task = app.handle_mcp_configurations_saved(Err(String::from("disk full")));

        assert!(
            app.status_message.starts_with("[MCP] "),
            "{}",
            app.status_message
        );
        assert!(
            app.status_message.contains("disk full"),
            "{}",
            app.status_message
        );
    }

    #[test]
    fn a_reused_update_request_id_with_a_different_patch_is_refused() {
        // 대상만으로 중복을 판정하면 두 번째 패치가 흡수되어 조용히 버려지고, 호출자는 성공을
        // 받는다 — 에이전트가 대상별로 request_id 하나를 쓰며 두 값을 잇달아 보내면 나중 값이
        // 사라진 채 적용됐다고 믿는다. stdin이 줄 지문으로 막은 것과 같은 경로다.
        let mut app = app_at_edit_tier();
        let first = McpOp::UpdateConfiguration(Box::new(UpdateConfigurationArgs {
            request_id: Some("req-1".to_string()),
            set_environment_variables: HashMap::from([("MODE".to_string(), "debug".to_string())]),
            ..patch("build")
        }));
        let second = McpOp::UpdateConfiguration(Box::new(UpdateConfigurationArgs {
            request_id: Some("req-1".to_string()),
            set_environment_variables: HashMap::from([("MODE".to_string(), "release".to_string())]),
            ..patch("build")
        }));

        payload_of(respond_to(&mut app, first));
        let message = failure_of(respond_to(&mut app, second));

        assert!(message.contains("request_id"), "{message}");
        assert_eq!(
            app.configurations[0].environment_variables["MODE"], "debug",
            "거절된 패치는 적용되지 않는다"
        );
    }

    #[test]
    fn the_same_update_request_id_with_the_same_patch_is_absorbed() {
        // 지문이 재시도를 깨뜨리지 않아야 한다. 두 op의 `HashMap`은 서로 다른 인스턴스이므로
        // 순회 순서가 다를 수 있다 — 지문을 맵 순회로 뜨면 정당한 재시도가 충돌로 뒤집힌다.
        let mut app = app_at_edit_tier();
        const PAIRS: [(&str, &str); 5] = [
            ("ALPHA", "1"),
            ("BETA", "2"),
            ("GAMMA", "3"),
            ("DELTA", "4"),
            ("EPSILON", "5"),
        ];
        // 삽입 순서까지 뒤집어, 순서 독립성이 인스턴스마다 다른 해시 시드에만 기대지 않게 한다.
        let env = |reversed: bool| {
            let mut map = HashMap::new();
            let mut pairs = PAIRS;
            if reversed {
                pairs.reverse();
            }
            for (key, value) in pairs {
                map.insert(key.to_string(), value.to_string());
            }
            map
        };
        let op = |reversed: bool| {
            McpOp::UpdateConfiguration(Box::new(UpdateConfigurationArgs {
                request_id: Some("req-1".to_string()),
                set_environment_variables: env(reversed),
                ..patch("build")
            }))
        };

        payload_of(respond_to(&mut app, op(false)));
        let again = payload_of(respond_to(&mut app, op(true)));

        assert_eq!(again["deduplicated"], json!(true));
    }

    #[test]
    fn a_reused_create_request_id_with_a_different_spec_is_refused() {
        // 이름이 같아도 내용이 다르면 다른 요청이다. 흡수하면 두 번째 spec이 사라진 채
        // 첫 구성의 id가 성공으로 돌아간다.
        let mut app = app_at_edit_tier();
        let first = McpOp::CreateConfiguration(Box::new(CreateConfigurationArgs {
            request_id: Some("req-1".to_string()),
            ..create_args("deploy")
        }));
        let second = McpOp::CreateConfiguration(Box::new(CreateConfigurationArgs {
            request_id: Some("req-1".to_string()),
            type_data: ConfigTypeData::Application {
                command: "different".to_string(),
                arguments: String::new(),
            },
            ..create_args("deploy")
        }));

        payload_of(respond_to(&mut app, first));
        let message = failure_of(respond_to(&mut app, second));

        assert!(message.contains("request_id"), "{message}");
    }

    #[test]
    fn setting_and_removing_the_same_env_key_lets_the_set_win() {
        // 원장 D40(a)의 순서 규칙. 두 루프를 맞바꿔도 서로소 키만 쓰는 테스트는 초록이므로,
        // 한 키를 양쪽에 실어 순서를 직접 고정한다.
        let mut app = app_at_edit_tier();
        let op = McpOp::UpdateConfiguration(Box::new(UpdateConfigurationArgs {
            set_environment_variables: HashMap::from([("MODE".to_string(), "release".to_string())]),
            remove_environment_variables: vec!["MODE".to_string()],
            ..patch("build")
        }));

        payload_of(respond_to(&mut app, op));

        assert_eq!(
            app.configurations[0].environment_variables["MODE"], "release",
            "제거가 먼저 적용되므로 설정이 이긴다"
        );
    }

    #[test]
    fn the_same_create_request_id_does_not_create_twice() {
        // 재시도가 두 번째 구성을 만들면 그 사실을 알리는 오류가 없다 — 목록에 조용히 쌍둥이가
        // 남고, 이름으로 부르는 호출이 둘 중 하나를 임의로 고르기 시작한다.
        let mut app = app_at_edit_tier();
        let op = McpOp::CreateConfiguration(Box::new(CreateConfigurationArgs {
            request_id: Some("req-1".to_string()),
            ..create_args("deploy")
        }));

        let first = payload_of(respond_to(&mut app, op.clone()));
        let again = payload_of(respond_to(&mut app, op));

        assert_eq!(app.configurations.len(), 2, "두 번째 호출은 만들지 않는다");
        assert_eq!(again["configuration_id"], first["configuration_id"]);
        assert_eq!(again["deduplicated"], json!(true));
    }

    #[test]
    fn a_retried_delete_reports_the_first_deletion_instead_of_not_found() {
        // 창을 거치지 않으면 재시도는 "구성 없음"을 받는데, 호출자는 그것이 "이미 지웠다"인지
        // "잘못된 대상을 보냈다"인지 구별할 수 없다.
        let mut app = app_at_edit_tier();
        let op = delete_op("build", Some("req-1"));

        let first = payload_of(respond_to(&mut app, op.clone()));
        let again = payload_of(respond_to(&mut app, op));

        assert_eq!(again["configuration_id"], first["configuration_id"]);
        assert_eq!(again["deduplicated"], json!(true));
        assert!(app.configurations.is_empty());
    }

    #[test]
    fn an_edit_call_below_the_edit_tier_is_denied() {
        // `tools/list`를 캐시한 클라이언트가 tier 밖 이름으로 호출할 수 있다.
        let mut app = app_at_edit_tier();
        app.mcp_permission = McpPermission::Execute;

        let outcome = respond_to(&mut app, create_op("deploy"));

        assert!(matches!(outcome, McpOutcome::Denied(_)), "{outcome:?}");
        assert_eq!(app.configurations.len(), 1);
    }

    #[test]
    fn a_denied_edit_leaves_an_audit_line_in_the_status_bar() {
        // 권한 밖 삭제 시도는 성공한 삭제보다 감사 가치가 낮지 않다(보안 항목 8).
        let mut app = app_at_edit_tier();
        app.mcp_permission = McpPermission::Execute;
        app.status_message = String::from("Ready");

        let _denied = respond_to(&mut app, delete_op("build", None));

        assert!(
            app.status_message
                .starts_with("[MCP] Denied delete_configuration"),
            "{}",
            app.status_message
        );
    }

    #[test]
    fn an_edit_is_refused_when_the_stored_configurations_could_not_be_read() {
        // 로드가 실패하면 목록은 빈 채로 남고 편집 툴은 스스로 저장을 발행한다 — 구성 하나를
        // 만들면 저장소가 한 개짜리 목록으로 교체되고, `write_configs_at`은 백업을 남기지 않는다.
        let mut app = app_at_edit_tier();
        app.configurations.clear();
        app.configs_load_failed = true;

        let failure = failure_of(respond_to(&mut app, create_op("deploy")));

        assert!(app.configurations.is_empty(), "아무것도 만들지 않는다");
        assert!(failure.contains("could not be read"), "{failure}");
    }

    #[test]
    fn a_compound_cannot_contain_itself() {
        // `run_compound`가 compound 멤버를 건너뛰므로 결과는 없는 멤버와 같다 — 게다가 GUI
        // 편집기는 자기 자신을 후보에서 빼므로 사용자가 그 멤버를 해제할 방법이 없다.
        let mut app = app_at_edit_tier();
        let config_id = app.configurations[0].id;
        let op = McpOp::UpdateConfiguration(Box::new(UpdateConfigurationArgs {
            type_data: Some(ConfigTypeData::Compound {
                members: vec![config_id],
                workspace: None,
            }),
            ..patch("build")
        }));

        let failure = failure_of(respond_to(&mut app, op));

        assert!(failure.contains("itself"), "{failure}");
        assert!(matches!(
            app.configurations[0].type_data,
            ConfigTypeData::Application { .. }
        ));
    }

    #[test]
    fn a_compound_member_that_is_itself_a_compound_is_refused() {
        let mut app = app_at_edit_tier();
        app.configurations.push(RunConfiguration {
            name: String::from("inner"),
            type_data: ConfigTypeData::Compound {
                members: vec![],
                workspace: None,
            },
            ..RunConfiguration::default()
        });
        let inner = app.configurations[1].id;
        let op = McpOp::CreateConfiguration(Box::new(CreateConfigurationArgs {
            type_data: ConfigTypeData::Compound {
                members: vec![inner],
                workspace: None,
            },
            ..create_args("bundle")
        }));

        let failure = failure_of(respond_to(&mut app, op));

        assert_eq!(app.configurations.len(), 2);
        assert!(failure.contains("inner"), "{failure}");
    }

    #[test]
    fn an_env_update_closes_the_users_open_env_modal() {
        // 모달의 `entries`는 상태의 사본이고 Confirm은 그것으로 맵을 통째로 교체한다 —
        // 열어 둔 채로 두면 사용자의 Confirm이 에이전트의 변경을 조용히 되돌린다.
        let mut app = app_at_edit_tier();
        let config_id = app.configurations[0].id;
        app.env_modal = Some(crate::app::env_modal::EnvModalState {
            config_id,
            entries: vec![(String::from("MODE"), String::from("debug"))],
            focused_cell: crate::app::env_modal::ModalCell {
                row: 0,
                kind: crate::app::env_modal::ModalCellKind::Key,
            },
        });
        let op = McpOp::UpdateConfiguration(Box::new(UpdateConfigurationArgs {
            set_environment_variables: HashMap::from([(
                String::from("MODE"),
                String::from("release"),
            )]),
            ..patch("build")
        }));

        payload_of(respond_to(&mut app, op));

        assert!(app.env_modal.is_none());
        assert_eq!(
            app.configurations[0].environment_variables["MODE"],
            "release"
        );
    }

    #[test]
    fn a_rename_keeps_the_users_open_env_modal() {
        // 반대 방향 — 환경변수를 건드리지 않는 편집은 사용자가 입력하던 모달을 닫지 않는다.
        let mut app = app_at_edit_tier();
        let config_id = app.configurations[0].id;
        app.env_modal = Some(crate::app::env_modal::EnvModalState {
            config_id,
            entries: vec![(String::from("MODE"), String::from("typing"))],
            focused_cell: crate::app::env_modal::ModalCell {
                row: 0,
                kind: crate::app::env_modal::ModalCellKind::Key,
            },
        });
        let op = McpOp::UpdateConfiguration(Box::new(UpdateConfigurationArgs {
            name: Some(String::from("renamed")),
            ..patch("build")
        }));

        payload_of(respond_to(&mut app, op));

        let modal = app
            .env_modal
            .as_ref()
            .expect("사용자의 모달이 살아 있어야 한다");
        assert_eq!(modal.entries[0].1, "typing");
    }

    /// 낡은 목록이 남으면 사용자가 편집기에서 이전 프로젝트의 스크립트를 고를 수 있다.
    const STALE_SCRIPT: &str = "script-from-the-old-project";

    #[test]
    fn moving_a_configuration_drops_the_stale_node_metadata() {
        let mut app = app_at_edit_tier();
        let config_id = app.configurations[0].id;
        app.node_available_scripts
            .insert(config_id, vec![String::from(STALE_SCRIPT)]);
        app.node_available_package_jsons
            .insert(config_id, vec![String::from("/tmp/project/package.json")]);
        let op = McpOp::UpdateConfiguration(Box::new(UpdateConfigurationArgs {
            working_directory: Some(String::from("/tmp/somewhere-else")),
            ..patch("build")
        }));

        payload_of(respond_to(&mut app, op));

        // Application 타입은 재스캔하지 않으므로 두 캐시가 비어야 한다.
        assert!(!app.node_available_scripts.contains_key(&config_id));
        assert!(!app.node_available_package_jsons.contains_key(&config_id));
    }

    fn node_type_data(project_directory: &str) -> ConfigTypeData {
        ConfigTypeData::Node {
            project_directory: project_directory.to_string(),
            package_manager: crate::models::PackageManager::default(),
            node_runtime_path: None,
            command: crate::models::NodeCommand::default(),
            script_name: None,
            arguments: String::new(),
            node_options: String::new(),
        }
    }

    #[test]
    fn moving_a_node_project_drops_the_scripts_without_rescanning() {
        // 다시 재지 않는다: `load_node_scripts`는 `find_package_json`으로 부모 방향을 root까지
        // 훑고 `parse_scripts`가 크기 상한 없이 파일을 읽는데, 둘 다 동기여서 호출자가 넘긴
        // 경로가 그동안 update 루프 전체를 붙잡는다. 빈 캐시는 사용자가 그 구성을 고를 때
        // `select_configuration`이 채운다 — 보이는 부재이고, 틀린 목록이 아니다.
        let mut app = app_at_edit_tier();
        app.configurations[0].type_data = node_type_data("/tmp/project");
        let config_id = app.configurations[0].id;
        app.node_available_scripts
            .insert(config_id, vec![String::from(STALE_SCRIPT)]);
        app.node_available_package_jsons
            .insert(config_id, vec![String::from("/tmp/project/package.json")]);
        let op = McpOp::UpdateConfiguration(Box::new(UpdateConfigurationArgs {
            type_data: Some(node_type_data("/tmp/somewhere-else")),
            ..patch("build")
        }));

        payload_of(respond_to(&mut app, op));

        assert!(!app.node_available_scripts.contains_key(&config_id));
        assert!(!app.node_available_package_jsons.contains_key(&config_id));
    }

    #[test]
    fn a_node_configuration_runs_in_its_project_directory() {
        // 편집기는 Node에 이 입력을 주지 않고 고른 package.json의 부모로 채운다
        // (`view_node_working_directory_row`). 호출자가 실은 값을 그대로 두면 화면이 가리키는
        // 프로젝트와 실제 cwd가 갈린다 — 실행에 쓰이는 것은 `working_directory`다.
        let mut app = app_at_edit_tier();
        let op = McpOp::CreateConfiguration(Box::new(CreateConfigurationArgs {
            working_directory: String::from("/tmp/unrelated"),
            type_data: node_type_data("/tmp/project"),
            ..create_args("web")
        }));

        payload_of(respond_to(&mut app, op));

        let created = app
            .configurations
            .iter()
            .find(|it| it.name == "web")
            .expect("created");
        assert_eq!(created.working_directory, "/tmp/project");
    }

    #[test]
    fn moving_a_node_project_moves_the_working_directory() {
        let mut app = app_at_edit_tier();
        app.configurations[0].type_data = node_type_data("/tmp/project");
        app.configurations[0].working_directory = String::from("/tmp/project");
        let op = McpOp::UpdateConfiguration(Box::new(UpdateConfigurationArgs {
            type_data: Some(node_type_data("/tmp/moved")),
            ..patch("build")
        }));

        let payload = payload_of(respond_to(&mut app, op));

        assert_eq!(app.configurations[0].working_directory, "/tmp/moved");
        assert_eq!(payload["changed"], json!(true));
    }

    #[test]
    fn a_node_configuration_needs_a_project_directory() {
        // Node의 cwd는 이 필드에서 파생되므로, 빈 값을 받아 주면 빈 cwd가 필드만 옮겨 그대로
        // 남는다 — 그 값으로는 어느 스폰 경로도 설정된 디렉터리에서 돌 수 없고
        // (`verify_working_directory`), 쓰기 경계에서 막지 않으면 그 실패가 호출자가 아니라
        // 사용자에게 돌아간다.
        let mut app = app_at_edit_tier();
        let op = McpOp::CreateConfiguration(Box::new(CreateConfigurationArgs {
            type_data: node_type_data("   "),
            ..create_args("web")
        }));

        let reason = failure_of(respond_to(&mut app, op));

        assert!(reason.contains("project_directory"), "{reason}");
        assert!(!app.configurations.iter().any(|it| it.name == "web"));
    }

    #[test]
    fn a_type_switch_cannot_leave_an_empty_working_directory() {
        // compound는 디렉터리를 비운 채로 존재할 수 있다(어느 실행에도 닿지 않는다). 그것을
        // Application으로 바꾸는 패치는 디렉터리를 실을 의무가 없어 파서만으로는 빈 cwd가
        // 통과한다 — 결과 타입과 결과 디렉터리를 함께 아는 자리는 앱 계층뿐이다.
        let mut app = app_at_edit_tier();
        app.configurations[0].working_directory = String::new();
        app.configurations[0].type_data = ConfigTypeData::Compound {
            members: Vec::new(),
            workspace: None,
        };
        let op = McpOp::UpdateConfiguration(Box::new(UpdateConfigurationArgs {
            type_data: Some(ConfigTypeData::Application {
                command: String::from("echo"),
                arguments: String::new(),
            }),
            ..patch("build")
        }));

        let reason = failure_of(respond_to(&mut app, op));

        assert!(reason.contains("working_directory"), "{reason}");
        // 거절은 반쯤 적용된 패치를 남기지 않는다.
        assert!(matches!(
            app.configurations[0].type_data,
            ConfigTypeData::Compound { .. }
        ));
    }

    #[test]
    fn a_compound_cannot_list_the_same_member_twice() {
        // `run_compound`는 항목마다 세션을 띄우므로 중복은 한 번 눌러 두 세션이 된다. GUI의
        // 멤버 목록은 체크박스 토글이어서 그 상태를 만들 수도, 둘 중 하나만 지울 수도 없다.
        let mut app = app_at_edit_tier();
        let member = app.configurations[0].id;
        let op = McpOp::CreateConfiguration(Box::new(CreateConfigurationArgs {
            type_data: ConfigTypeData::Compound {
                members: vec![member, member],
                workspace: None,
            },
            ..create_args("bundle")
        }));

        let reason = failure_of(respond_to(&mut app, op));

        assert!(reason.contains("twice"), "{reason}");
        assert!(!app.configurations.iter().any(|it| it.name == "bundle"));
    }

    #[test]
    fn creating_a_configuration_with_a_masked_value_is_refused() {
        // 조회한 값을 새 구성으로 옮기는 경로도 같은 파괴다 — 리터럴 `<hidden>`이 시크릿인 것처럼
        // 저장되고, 사용자는 실행이 틀린 값으로 실패할 때까지 알 수 없다.
        let mut app = app_at_edit_tier();
        let op = McpOp::CreateConfiguration(Box::new(CreateConfigurationArgs {
            environment_variables: HashMap::from([(
                String::from("TOKEN"),
                String::from(MASKED_VALUE),
            )]),
            ..create_args("deploy")
        }));

        let reason = failure_of(respond_to(&mut app, op));

        assert!(reason.contains("TOKEN"), "{reason}");
        assert!(!app.configurations.iter().any(|it| it.name == "deploy"));
    }

    #[test]
    fn creating_a_configuration_leaves_the_users_editor_selection_alone() {
        // 편집기는 `selected_config_index`가 가리키는 구성을 렌더하고 `Message::NameChanged`도
        // 같은 인덱스로 되쓴다. 선택이 새 구성으로 옮겨지면 이름을 타이핑하던 사용자의 다음
        // 키가 에이전트의 구성에 들어간다 — 보기가 바뀌는 것이 아니라 데이터가 바뀐다.
        let mut app = app_at_edit_tier();
        app.selected_config_index = Some(0);
        let editing = app.configurations[0].id;

        payload_of(respond_to(&mut app, create_op("deploy")));

        assert_eq!(app.configurations.len(), 2);
        assert_eq!(app.configurations[1].name, "deploy");
        assert_eq!(
            app.selected_config_index
                .and_then(|idx| app.configurations.get(idx))
                .map(|it| it.id),
            Some(editing)
        );
    }

    #[test]
    fn renaming_a_configuration_onto_a_taken_name_is_refused() {
        // 이름은 다른 툴이 구성을 지목하는 주소다(`find_configuration_index`). 중복을 허용하면
        // 이름으로 부른 호출이 둘 중 하나를 임의로 고르고 그 선택은 호출자에게 보이지 않는다.
        let mut app = app_at_edit_tier();
        payload_of(respond_to(&mut app, create_op("deploy")));
        let op = McpOp::UpdateConfiguration(Box::new(UpdateConfigurationArgs {
            name: Some(String::from("deploy")),
            ..patch("build")
        }));

        let reason = failure_of(respond_to(&mut app, op));

        assert!(reason.contains("deploy"), "{reason}");
        assert_eq!(app.configurations[0].name, "build");
    }

    #[test]
    fn a_full_retry_window_refuses_an_edit_instead_of_applying_it() {
        // 만석 거절은 실행 툴만의 계약이 아니다 — 편집 툴이 같은 창을 쓰므로, 여기서 통과시키면
        // 상한을 지키는 유일한 수단이 편집 경로에서 사라진다.
        let mut app = app_at_edit_tier();
        let now = Instant::now();
        for i in 0..MCP_DEDUP_MAX_ENTRIES {
            app.mcp_request_log.record(
                DedupScope::UpdateConfiguration,
                format!("filler-{i}"),
                "build".to_string(),
                vec![Uuid::new_v4()],
                now,
            );
        }
        let op = McpOp::UpdateConfiguration(Box::new(UpdateConfigurationArgs {
            name: Some(String::from("renamed")),
            request_id: Some(String::from("late")),
            ..patch("build")
        }));

        let reason = failure_of(respond_to(&mut app, op));

        assert!(reason.contains("request_id"), "{reason}");
        assert_eq!(app.configurations[0].name, "build");
    }

    #[test]
    fn an_absorbed_retry_does_not_report_the_patch_as_a_no_op() {
        // `changed: false`는 "이 패치는 아무것도 바꾸지 않았다"는 뜻이다. 흡수된 재시도에 그것을
        // 실으면 첫 호출의 효과까지 부정해, `changed`를 둔 목적이 정확히 재시도에서 뒤집힌다.
        let mut app = app_at_edit_tier();
        let op = || {
            McpOp::UpdateConfiguration(Box::new(UpdateConfigurationArgs {
                name: Some(String::from("renamed")),
                request_id: Some(String::from("req-1")),
                ..patch("build")
            }))
        };

        let first = payload_of(respond_to(&mut app, op()));
        let again = payload_of(respond_to(&mut app, op()));

        assert_eq!(first["changed"], json!(true));
        assert_eq!(again["deduplicated"], json!(true));
        // 키를 빼면 호출자가 응답 모양을 두 가지로 다뤄야 한다(`session_entry`와 같은 규칙).
        assert!(again.get("changed").is_some(), "{again}");
        assert!(again["changed"].is_null(), "{again}");
    }

    #[test]
    fn the_save_reply_reaches_the_mcp_handler_through_update() {
        // 다른 두 저장 테스트는 핸들러를 직접 부르므로, `update()`의 arm이 공용
        // `ConfigurationsSaved` 핸들러로 옮겨져도 초록인 채 감사 줄이 성공 문구로 덮인다.
        let mut app = app_at_edit_tier();
        payload_of(respond_to(&mut app, create_op("deploy")));
        let audit = app.status_message.clone();

        let _task = app.update(Message::McpConfigurationsSaved(Ok(PathBuf::from(
            "/tmp/configs.json",
        ))));

        assert_eq!(app.status_message, audit);
    }

    #[test]
    fn an_update_that_changes_nothing_says_so() {
        // 파싱은 요청의 모양만 본다 — 설정되지 않은 변수를 지우는 호출은 그 검사를 통과하고도
        // 아무것도 바꾸지 않는다. 성공으로 보고하면 에이전트가 적용됐다고 믿는다.
        let mut app = app_at_edit_tier();
        let before = app.configurations[0].clone();
        let op = McpOp::UpdateConfiguration(Box::new(UpdateConfigurationArgs {
            remove_environment_variables: vec![String::from("NEVER_SET")],
            ..patch("build")
        }));

        let payload = payload_of(respond_to(&mut app, op));

        let after = &app.configurations[0];
        assert_eq!(payload["changed"], json!(false));
        assert_eq!(after.name, before.name);
        assert_eq!(after.working_directory, before.working_directory);
        assert_eq!(after.type_data, before.type_data);
        assert_eq!(after.environment_variables, before.environment_variables);
        assert!(
            app.status_message.contains("unchanged"),
            "{}",
            app.status_message
        );
        assert!(
            app.status_message.starts_with("[MCP] "),
            "{}",
            app.status_message
        );
    }

    #[test]
    fn an_update_that_changes_something_reports_it() {
        let mut app = app_at_edit_tier();
        let op = McpOp::UpdateConfiguration(Box::new(UpdateConfigurationArgs {
            remove_environment_variables: vec![String::from("MODE")],
            ..patch("build")
        }));

        let payload = payload_of(respond_to(&mut app, op));

        assert_eq!(payload["changed"], json!(true));
        assert!(
            app.status_message.contains("Updated"),
            "{}",
            app.status_message
        );
    }

    #[test]
    fn renaming_a_configuration_to_its_own_name_is_allowed() {
        // 이름 중복 검사는 자기 자신을 제외해야 한다 — 아니면 다른 필드를 함께 바꾸는 패치가
        // 같은 이름을 실었다는 이유로 거절된다.
        let mut app = app_at_edit_tier();
        let op = McpOp::UpdateConfiguration(Box::new(UpdateConfigurationArgs {
            name: Some(String::from("build")),
            working_directory: Some(String::from("/tmp/moved")),
            ..patch("build")
        }));

        payload_of(respond_to(&mut app, op));

        assert_eq!(app.configurations[0].working_directory, "/tmp/moved");
    }

    #[test]
    fn a_refused_edit_can_be_retried_with_the_same_request_id() {
        // 거절은 아무것도 만들지 않았으므로 창에 남겨서는 안 된다 — 남기면 원인을 고친 재시도가
        // "이미 했다"로 흡수되어, 일어나지 않은 생성이 성공으로 굳는다.
        let mut app = app_at_edit_tier();
        let taken = McpOp::CreateConfiguration(Box::new(CreateConfigurationArgs {
            request_id: Some(String::from("req-1")),
            ..create_args("build")
        }));
        failure_of(respond_to(&mut app, taken));

        let retried = McpOp::CreateConfiguration(Box::new(CreateConfigurationArgs {
            request_id: Some(String::from("req-1")),
            ..create_args("deploy")
        }));
        let payload = payload_of(respond_to(&mut app, retried));

        assert_eq!(payload["deduplicated"], json!(false));
        assert_eq!(app.configurations.len(), 2);
    }

    #[test]
    fn a_reused_edit_request_id_for_another_target_leaves_an_audit_line() {
        // 파괴적 툴에 대한 대상 혼동은 감사 가치가 가장 높은 사건에 속한다.
        let mut app = app_at_edit_tier();
        app.configurations.push(RunConfiguration {
            name: String::from("other"),
            ..configuration("other")
        });
        payload_of(respond_to(&mut app, delete_op("build", Some("req-1"))));

        let failure = failure_of(respond_to(&mut app, delete_op("other", Some("req-1"))));

        assert_eq!(
            app.configurations.len(),
            1,
            "두 번째 삭제는 일어나지 않는다"
        );
        assert!(failure.contains("req-1"), "{failure}");
        assert!(
            app.status_message
                .starts_with("[MCP] Rejected delete_configuration"),
            "{}",
            app.status_message
        );
    }
}
