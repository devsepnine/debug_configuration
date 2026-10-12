# MCP server (AI agents)

에이전트용 사용 가이드와 툴별 계약은 [`docs/MCP.md`](../MCP.md)에 있다. 이 문서는 증상에서 코드로 가는 색인만 담는다.

### `mcp-server-lifecycle`: 로컬 MCP 서버 켜기·포트 설정·리스너 상태 표시

- **Where**: Settings 모달 → "AI (MCP)" 섹션 (enable 체크박스, Port, Status 행)
- **Aliases**: MCP 서버 안 켜짐, 포트 변경, 리스닝 안 됨, 서버 상태, "MCP server failed", port in use, enable MCP
- **UI strings**: "AI (MCP)", "Enable local MCP server", "Lets an AI agent on this machine read your configurations and session output over ", "Port", "47355", "Status", "Disabled", "Starting…", "Listening on http://127.0.0.1:{port}/mcp", "Failed: {reason}", "MCP server listening on 127.0.0.1:{port}", "MCP server failed to start: {reason}", "127.0.0.1:{}: {e}"
- **Controls**: "Enable local MCP server" → `Message::SettingsToggleMcpEnabled` / `handle_settings_toggle_mcp_enabled`, "Port" text_input → `Message::SettingsMcpPortChanged` / `handle_settings_mcp_port_changed`, "Apply"/"OK" → `apply_settings`
- **Code path**:
  - UI: `src/views/configuration_editor.rs` → `mcp_port_row`, `mcp_status_row`
  - State / handler: `src/app/settings_modal.rs` → `apply_settings`, `parse_mcp_port` (`MIN_MCP_PORT` 1024 clamp); `src/app/mcp.rs` → `handle_mcp_event`, `mcp_listener_config`, `McpServerStatus::label`; `src/app.rs` → `subscription` 안의 `mcp_subscription` (`Subscription::run_with(config, ...)`, `mcp_server`)
  - Server: `src/services/mcp/mod.rs` → `server`, `DEFAULT_PORT`; `src/services/mcp/http.rs` → `serve` (`Ipv4Addr::LOCALHOST` bind, `McpEvent::Started`/`BindFailed`)
  - Data: `AppSettings.mcp_enabled`/`mcp_port`, written in `src/app.rs` → `save_app_settings`, read in `src/services/storage.rs` → `load_settings` (`default_mcp_port`)
- **Tests**: `src/app/mcp.rs` → `bind_failure_is_surfaced_to_the_user`, `listening_event_records_the_actual_port`, `listener_config_requires_both_the_toggle_and_a_token`, `token_failure_never_opens_a_listener`, `an_arriving_token_lets_the_listener_start`; `src/app/settings_modal.rs` → `parse_mcp_port_clamps_and_keeps_fallback`, `opening_the_modal_stages_the_live_mcp_settings`, `cancelling_discards_staged_mcp_changes`, `confirming_applies_the_staged_mcp_settings`, `disabling_the_server_resets_the_status`; `src/services/mcp/http.rs` → `bind_failure_is_reported`
- **Pitfalls**: 켰는데 Status가 "Starting…"에 머묾 → 리스너는 토큰과 enable이 둘 다 있어야 열리므로 토큰 로드가 끝나지 않은 것(`mcp_token_loading`부터 확인), source: `src/app/mcp.rs` → `mcp_listener_config`; 포트에 1024 미만이나 문자를 넣으면 다른 값으로 바뀜 → 1024..=65535 clamp, 파싱 실패 시 이전 값 유지, source: `src/app/settings_modal.rs` → `parse_mcp_port`; 포트·토큰 변경 시 연결 중인 클라이언트가 끊김 → subscription 정체성이 `McpServerConfig`라 낡은 스트림이 drop되고 `JoinSet`이 연결을 abort, source: `src/services/mcp/http.rs` → `serve`; 테스트 중 설정 파일이 덮이지 않음 → `cfg!(test)`에서 저장 생략, source: `src/app.rs` → `save_app_settings`
- **Shares**: Settings 모달 staging (`SettingsModalState`, `settings_modal_is_dirty`, `settings_modal_can_apply`)
- **Related**: `mcp-token`, `mcp-http-auth`, `mcp-permission-tiers`

### `mcp-http-auth`: HTTP 엔드포인트 `POST /mcp`의 Origin·Bearer·본문 상한·프로토콜 버전 검사

- **Where**: 화면 없음 → 에이전트(MCP 클라이언트)가 받는 HTTP/JSON-RPC 응답
- **Aliases**: 401, unauthorized, 403 Origin, 405, 연결 안 됨, 클라이언트가 응답을 거부, protocol version mismatch, timeout, resultType
- **UI strings**: "Origin not allowed", "Not found", "Missing or invalid bearer token", "Only POST is supported; this server does not offer SSE streams", "Request body exceeds 1 MiB limit", "MCP-Protocol-Version header ({header}) does not match the request body ({body})", "Missing tool name", "Method not found: {other}", "The app is no longer accepting MCP requests", "The app dropped the request without responding", "The app did not respond within {}s", "{method} received an unexpected {kind} response"
- **Controls**: none (와이어 프로토콜)
- **Code path**:
  - Transport: `src/services/mcp/http.rs` → `route`, `origin_allowed` (`LOOPBACK_ORIGIN_HOSTS`), `authorized`, `payload_too_large` (`MAX_BODY_BYTES`), `method_not_allowed`, `dispatch`, `bridge` (`REQUEST_TIMEOUT` 10s), `HEADER_READ_TIMEOUT`
  - Protocol: `src/services/mcp/protocol.rs` → `parse_request`, `check_protocol_version`, `unsupported_version_error`, `discover_result`, `initialize_result`, `success`, `failure`, `tools_list_result`, `tool_result`, `tool_error` (`PROTOCOL_MODERN` "2026-07-28", `PROTOCOL_LEGACY` "2025-11-25")
  - Token compare: `src/services/mcp/token.rs` → `matches`
  - Data: none
- **Tests**: `src/services/mcp/http.rs` → `missing_token_is_rejected`, `wrong_token_is_rejected`, `non_loopback_origin_is_rejected`, `loopback_origin_is_accepted`, `get_and_delete_are_not_allowed`, `unknown_path_is_not_found`, `oversized_body_is_rejected`, `chunked_body_over_the_cap_is_rejected`, `an_unanswered_request_becomes_an_internal_error`, `malformed_json_is_a_parse_error`, `protocol_version_header_mismatch_is_a_bad_request`, `unsupported_protocol_version_is_reported_with_alternatives`, `every_dispatched_result_declares_its_result_type`, `origin_matrix`, `authorization_header_matrix`; `src/services/mcp/protocol.rs` → `version_check_flags_header_body_mismatch`, `a_success_envelope_declares_the_complete_result_type`, `the_tool_list_result_carries_the_cache_fields`
- **Pitfalls**: 등록된 클라이언트가 모든 툴 응답을 거부 → 2026-07-28 리비전은 모든 result에 `resultType`, tools/list에 `ttlMs`·`cacheScope`를 요구, source: `bc16c03`; GUI가 모달 등으로 막히면 에이전트가 10초 뒤 에러를 받음 → update 루프 응답 대기 상한, source: `src/services/mcp/http.rs` → `bridge`; 원격 접속 불가 → 루프백 전용 bind, source: `src/services/mcp/http.rs` → `serve`
- **Shares**: `src/services/mcp/mod.rs` → `McpRequest::respond` (응답 채널 1회 사용)
- **Related**: `mcp-server-lifecycle`, `mcp-token`

### `mcp-token`: Bearer 토큰 생성·복사·재발급과 `claude mcp add` 명령 복사

- **Where**: Settings 모달 → "AI (MCP)" → "Register with Claude Code" 박스, "Token" 행
- **Aliases**: 토큰 복사, 토큰 재생성, 등록 명령 복사, claude mcp add, 토큰 마스킹, token regenerate, 401 after regenerate
- **UI strings**: "Register with Claude Code", "Token", "Not created yet", "<token unavailable>", "claude mcp add --transport http runconfig http://127.0.0.1:{}/mcp -H ", "MCP token is not available yet", "MCP token copied to clipboard", "MCP registration command copied to clipboard", "MCP token regenerated — re-register your MCP client", "No home directory for the MCP token file"
- **Controls**: `ICON_COPY` (command 행) → `Message::SettingsCopyMcpCommand` / `handle_settings_copy_mcp_command`, `ICON_COPY` (token 행) → `Message::SettingsCopyMcpToken` / `handle_settings_copy_mcp_token`, `ICON_REFRESH` → `Message::SettingsRegenerateMcpToken` / `handle_settings_regenerate_mcp_token`; 로드 결과 → `Message::McpTokenLoaded` / `handle_mcp_token_loaded`
- **Code path**:
  - UI: `src/views/configuration_editor.rs` → `mcp_command_row`, `mcp_token_row`
  - State / handler: `src/app/settings_modal.rs` → `handle_settings_copy_mcp_token`, `handle_settings_copy_mcp_command`, `handle_settings_regenerate_mcp_token`; `src/app/mcp.rs` → `mcp_token_task`, `handle_mcp_token_loaded`, `mcp_add_command`, `mcp_listening_port`
  - Data: 토큰 파일 `~/.run_config_mcp_token` (64 hex, owner-only 권한), written in `src/services/mcp/token.rs` → `write_at`/`regenerate`, read in `src/services/mcp/token.rs` → `load_or_create` (`tighten_permissions`, `is_well_formed`); 화면 표시는 `src/services/mcp/token.rs` → `mask`
- **Tests**: `src/services/mcp/token.rs` → `generated_token_is_64_hex_chars`, `load_creates_then_reuses_the_same_token`, `corrupted_token_file_is_replaced`, `created_token_file_is_owner_only`, `loose_permissions_are_tightened_on_load`, `mask_hides_the_middle_of_the_token`; `src/app/mcp.rs` → `displayed_command_masks_the_token`; `src/app/settings_modal.rs` → `regenerating_drops_the_current_token_immediately`
- **Pitfalls**: 복사 버튼이 비활성 → 토큰이 없음: MCP를 아직 켜지 않았거나, 재발급 중이거나(`mcp_token = None`), 토큰 로드가 실패함, source: `src/views/configuration_editor.rs` → `mcp_token_row`, `src/app/settings_modal.rs` → `handle_settings_regenerate_mcp_token`; 재발급 후 기존 클라이언트가 401 → 이전 토큰을 즉시 버리고 리스너 재시작, 클라이언트 재등록 필요, source: `src/app/settings_modal.rs` → `handle_settings_regenerate_mcp_token`; 화면 명령에 토큰이 가려져 있음 → 표시용은 `mask_token`, Copy만 실제 토큰을 실음, source: `src/app/mcp.rs` → `mcp_add_command`
- **Shares**: `iced::clipboard::write`
- **Related**: `mcp-server-lifecycle`, `mcp-http-auth`

### `mcp-permission-tiers`: 권한 단계(Read only / Execute / Edit)와 `tools/list` 필터·호출 시점 재검사

- **Where**: Settings 모달 → "AI (MCP)" → "Permission" segmented 버튼; 에이전트 쪽 tools/list
- **Aliases**: 권한, 툴이 안 보임, tool not found, 실행 툴 없음, permission tier, read only, denied
- **UI strings**: "Permission", "Read only", "Execute", "Edit", "This tool needs the '{}' permission tier, but the app is set to '{}'. ", "Change it in the app's Settings.", "[MCP] Denied {tool}: beyond the current permission tier"
- **Controls**: "Read only"/"Execute"/"Edit" → `Message::SettingsMcpPermissionChanged` / `handle_settings_mcp_permission_changed`
- **Code path**:
  - UI: `src/views/configuration_editor.rs` → `mcp_permission_row`
  - Catalog: `src/services/mcp/tools.rs` → `McpPermission` (`allows`, `label`, `ALL`), `static TOOLS` (`ToolSpec.tier`), `tools_for`, `McpOp::required_tier`, `parse_call`
  - State / handler: `src/app/mcp.rs` → `handle_mcp_request` (`permission_denial`), `McpOp::ListTools` → `mcp_tools_for`; `src/services/mcp/http.rs` → `dispatch` (`McpOutcome::Denied` → `METHOD_NOT_FOUND`)
  - Data: `AppSettings.mcp_permission`, written in `src/app.rs` → `save_app_settings`, read in `src/services/storage.rs` → `load_settings`
- **Tests**: `src/services/mcp/tools.rs` → `permission_default_is_read_only`, `higher_tiers_include_lower_ones`, `read_only_hides_the_execute_tools`, `execute_tier_exposes_the_execute_tools`, `edit_tools_appear_only_at_the_edit_tier`, `edit_is_the_strictest_tier`; `src/app/mcp.rs` → `read_only_tier_denies_higher_tier_tools`, `tool_list_follows_the_permission_tier`, `an_edit_call_below_the_edit_tier_is_denied`, `a_denied_edit_leaves_an_audit_line_in_the_status_bar`; `src/services/mcp/http.rs` → `denied_tool_is_reported_as_method_not_found`, `unknown_tool_never_reaches_the_app`
- **Pitfalls**: 권한을 올렸는데 에이전트가 여전히 툴을 못 봄 → 클라이언트가 tools/list를 캐시함, 호출은 현재 tier로 재검사되므로 낡은 이름 호출도 막힘, source: `src/app/mcp.rs` → `handle_mcp_request`
- **Shares**: Settings 모달 staging
- **Related**: `mcp-read-configurations`, `mcp-execute-sessions`, `mcp-edit-configurations`

### `mcp-read-configurations`: `list_configurations`·`get_configuration`과 환경변수 값 마스킹 토글

- **Where**: 에이전트 툴 응답; Settings 모달 → "AI (MCP)" → "Expose environment variable values" 체크박스
- **Aliases**: 구성 목록 조회, 환경변수 값이 <hidden>, 값 노출, expose env, get configuration not found, 시크릿 마스킹
- **UI strings**: "Expose environment variable values", "Off: names are shared, values are replaced with <hidden>.", "<hidden>", "No configuration matches '{target}'", "Failed to serialize configuration '{}': {e}", tool titles "List run configurations", "Get a run configuration"
- **Controls**: "Expose environment variable values" → `Message::SettingsToggleMcpExposeEnv` / `handle_settings_toggle_mcp_expose_env`
- **Code path**:
  - UI: `src/views/configuration_editor.rs` → settings 모달 body (`checkbox(props.mcp_expose_env_values)`)
  - State / handler: `src/app/mcp.rs` → `handle_mcp_request` (`McpOp::ListConfigurations`, `McpOp::GetConfiguration`), `configuration_summary`, `configuration_detail` (`MASKED_VALUE`), `find_configuration` (id 우선, 그다음 정확한 이름)
  - Args: `src/services/mcp/tools.rs` → `required_configuration_target`, `reject_unknown_arguments`
  - Data: `AppSettings.mcp_expose_env_values`, written in `src/app.rs` → `save_app_settings`, read in `src/services/storage.rs` → `load_settings`
- **Tests**: `src/app/mcp.rs` → `summary_exposes_env_keys_but_no_values`, `detail_masks_env_values_by_default`, `detail_reveals_env_values_when_the_toggle_is_on`, `summary_and_detail_report_disabled_variables`, `configuration_lookup_accepts_id_and_name`, `summary_and_detail_agree_on_the_configuration_type`; `src/services/mcp/tools.rs` → `get_configuration_requires_target`, `an_unknown_argument_is_rejected_for_every_tool`
- **Pitfalls**: 인자 오타가 조용히 기본값으로 처리됨 → 스키마의 `additionalProperties: false`를 읽기·실행 툴엔 강제하지 않았음, source: `e193a41`; `list_configurations`엔 값이 아예 없음 → 이름만 싣고 값은 `get_configuration`에서만, source: `src/app/mcp.rs` → `configuration_summary`
- **Shares**: `MASKED_VALUE` (`src/app/mcp.rs`)
- **Related**: `mcp-read-session-output`, `mcp-edit-configurations`

### `mcp-read-session-output`: `list_sessions`·`read_session_output`·`search_session_output`과 env 배너 마스킹

- **Where**: 에이전트 툴 응답 (GUI 터미널 세션 버퍼를 읽음)
- **Aliases**: 세션 출력 읽기, 로그 읽기, tail, since_line_id, 출력 검색, regex 검색, Environment 줄이 <hidden>, 검색 결과 잘림
- **UI strings**: "Environment: ", "<hidden>", "No session with id {session_id}"; 응답 필드 "last_line_id", "omitted_lines", "total_matches", "matched_lines", "truncated"
- **Controls**: none (툴 `list_sessions`, `read_session_output`, `search_session_output`)
- **Code path**:
  - State / handler: `src/app/mcp.rs` → `handle_mcp_request` (`McpOp::ListSessions`, `ReadSessionOutput`, `SearchSessionOutput`), `session_summary`, `session_output`, `session_search`, `masked_line`, `line_text`, `state_name`
  - Args: `src/services/mcp/tools.rs` → `DEFAULT_TAIL_LINES` (200), `MAX_LINES_PER_CALL` (5000), `MAX_SEARCH_MATCHES` (500), `MAX_QUERY_CHARS`, `validate_regex`
  - Banner 판정: `src/models/session.rs` → `is_env_banner_line`
  - Data: none (in-memory 세션 버퍼)
- **Tests**: `src/app/mcp.rs` → `session_summary_reports_status_and_duration`, `output_returns_the_requested_tail`, `output_since_line_id_returns_only_new_lines`, `search_folds_multiple_matches_on_one_line`, `search_supports_regex_mode`, `search_caps_the_returned_lines`, `reading_output_masks_the_env_banner`, `a_program_printing_its_own_environment_line_is_not_masked`, `exposing_env_values_returns_the_banner_as_printed`, `searching_cannot_confirm_a_masked_env_value`, `missing_session_is_a_tool_failure_not_a_panic`; `src/services/mcp/tools.rs` → `read_session_output_defaults_and_clamps_tail_lines`, `search_defaults_to_substring_mode`, `an_invalid_regex_is_rejected_instead_of_reporting_no_matches`, `a_huge_pattern_does_not_echo_itself_into_the_error`
- **Pitfalls**: env 배너 줄이 키까지 통째로 가려짐 → 값이 `; k=`를 담을 수 있어 키만 남기지 않음, 키 목록은 `list_configurations`가 줌, source: `src/app/mcp.rs` → `masked_line`; 마스킹된 값이 검색으로 확인됨 → 매치 개수도 oracle이 되므로 배너 줄을 집계에서 제외, source: `src/app/mcp.rs` → `session_search`; 프로그램이 직접 찍은 `env` 출력은 안 가려짐 → 판정은 텍스트가 아니라 앱이 기록한 줄 id, source: `src/app/mcp.rs` → `ENV_BANNER_PREFIX`; 잘못된 regex가 "매치 없음"이 아니라 에러 → source: `src/services/mcp/tools.rs` → `validate_regex`
- **Shares**: `RunSession::search_matches` (GUI 검색과 공유), `MASKED_VALUE`
- **Related**: `mcp-read-configurations`, `mcp-execute-sessions`

### `mcp-execute-sessions`: `run_configuration`·`stop_session`·`rerun_session` (Execute 단계)

- **Where**: 에이전트 툴 → GUI 터미널에 세션 탭이 생김; 상태바 `[MCP]` 줄
- **Aliases**: 에이전트로 실행, 실행 중지, 재실행, run from agent, 두 번 실행됨, rerun not found, compound run
- **UI strings**: "No configuration matches '{target}'", "Failed to run '{name}': {reason}", "[MCP] Failed to run '{name}': {reason}", "[MCP] Duplicate run request for '{target}' ignored", "[MCP] Stopped session: {config_name}", "[MCP] Session '{config_name}' had already finished", "Failed to rerun '{config_name}': {reason}", "[MCP] Duplicate rerun request ignored", "The duplicate rerun record is malformed. Send a different request_id.", "the app reported no reason"
- **Controls**: none (툴 `run_configuration`, `stop_session`, `rerun_session`)
- **Code path**:
  - State / handler: `src/app/mcp.rs` → `mcp_run_configuration`, `mcp_stop_session`, `mcp_rerun_session`, `started_sessions_payload`, `session_entry`, `rerun_payload`, `scavenged_reason`
  - GUI 재사용: `src/app.rs` → `handle_run_configuration`, `handle_stop_session`, `handle_rerun_session`
  - Data: none (세션은 in-memory)
- **Tests**: `src/app/mcp.rs` → `running_a_configuration_answers_with_the_session_and_its_state`, `a_run_that_starts_nothing_is_a_failure_not_an_empty_success`, `a_compound_run_reports_every_member_session`, `an_unknown_configuration_never_starts_a_session`, `read_only_tier_refuses_to_run_anything`, `rerun_answers_with_the_new_session_id`, `a_rerun_hands_back_the_line_id_that_separates_the_two_runs`, `rerun_without_its_configuration_is_a_failure`, `stopping_an_already_finished_session_is_not_an_error`, `stopping_a_running_session_reports_it_stopped`, `a_stopped_session_still_reads_as_stopped_after_the_run_reports_back`, `a_rerun_that_cannot_start_leaves_the_process_alone`
- **Pitfalls**: 작업 디렉터리가 없는데 실행이 $HOME에서 성공 → portable-pty가 cwd를 버리고 폴백, 이제 spawn 전에 검사해 실패, source: `fd0ced2`; 이름 변경 후 rerun이 끊기거나 같은 이름의 다른 구성을 실행 → 세션이 이름으로 구성을 찾았음, 이제 id로 찾음, source: `e750c11`; compound 실행 응답에 `session_id`가 없음 → 세션이 여러 개면 하나로 접지 않음, source: `src/app/mcp.rs` → `started_sessions_payload`
- **Shares**: `mcp-request-dedup`, `mcp_tag_status`
- **Related**: `mcp-session-input`, `mcp-request-dedup`, `mcp-audit-status`

### `mcp-session-input`: `send_session_input`으로 실행 중 세션 stdin에 한 줄 전송

- **Where**: 에이전트 툴 → GUI 터미널 세션의 stdin
- **Aliases**: 입력 보내기, stdin, 프롬프트 응답, 입력 pending, 입력이 두 번 들어감, send input
- **UI strings**: "Session {session_id} is not running; input has nowhere to go", "Failed to send input: {error}", "[MCP] Sending input to {config_name}", "[MCP] Input pending — the process is not reading stdin yet", "[MCP] Input failed: {error}", "[MCP] Duplicate input request ignored"; 응답 status "delivered", "pending", "duplicate"
- **Controls**: none; 쓰기 완료 → `Message::McpSessionInputWritten` / `handle_mcp_session_input_written`
- **Code path**:
  - State / handler: `src/app/mcp.rs` → `mcp_send_session_input`, `handle_mcp_session_input_written`, `input_payload`, `input_target`
  - Args: `src/services/mcp/tools.rs` → `required_single_line`, `MAX_INPUT_LINE_BYTES` (4096 바이트)
  - Data: none
- **Tests**: `src/app/mcp.rs` → `input_to_a_finished_session_is_a_failure`, `a_broken_stdin_write_is_reported_as_a_failure`, `a_timed_out_stdin_write_is_pending_not_delivered`, `input_is_not_answered_before_the_write_resolves`, `a_pipe_fallback_write_is_echoed_into_the_output`, `a_pty_write_is_not_echoed_by_the_app`, `a_broken_mcp_write_leaves_the_users_draft_alone`, `the_same_input_request_id_does_not_write_the_line_twice`, `a_broken_write_lets_the_same_input_request_id_try_again`, `every_input_outcome_is_visible_in_the_status_bar`; `src/services/mcp/tools.rs` → `session_input_rejects_embedded_newlines`, `an_oversized_input_line_is_rejected`, `session_input_accepts_an_empty_line`
- **Pitfalls**: 긴 줄이 거절됨 → PTY 쓰기 블록이 blocking 스레드와 stdin 락을 영구히 붙잡을 수 있어 바이트 상한, 스키마 `maxLength`엔 싣지 않음(문자 수 단위라 CJK에서 어긋남), source: `src/services/mcp/tools.rs` → `MAX_INPUT_LINE_BYTES`; 같은 `request_id`로 다른 줄을 보내면 흡수되지 않음 → 줄 지문을 dedup 대상에 접어 넣음, source: `src/app/mcp.rs` → `input_target`
- **Shares**: `mcp-request-dedup`
- **Related**: `mcp-execute-sessions`

### `mcp-edit-configurations`: `create_configuration`·`update_configuration`·`delete_configuration` (Edit 단계, env enable/disable 포함)

- **Where**: 에이전트 툴 → 사이드바 구성 목록·편집기에 반영, configs.json 저장
- **Aliases**: 에이전트가 구성 생성, 구성 수정, 구성 삭제, 환경변수 끄기/켜기, disable env, <hidden> 거절, 이름 중복, 저장 실패, unchanged
- **UI strings**: "Created configuration '{name}'", "Updated configuration '{name}'", "Update left configuration '{name}' unchanged", "Deleted configuration '{name}'", "[MCP] Refused to {action} configuration: {reason}", "[MCP] Failed to save configurations: {error}", "[MCP] Duplicate create request for '{}' ignored", "[MCP] Duplicate update request for '{}' ignored", "[MCP] Duplicate delete request for '{}' ignored", "Configurations are still loading; retry after the app has finished loading them", "The stored configurations could not be read, so the in-memory list is empty and ", "A configuration named '{name}' already exists", "No configuration has id {member}", "A compound cannot contain itself", "Environment variable '{key}' was sent as the mask placeholder '{MASKED_VALUE}'; ", "disabled_environment_variables names '{key}', which is not in ", "disable_environment_variables"
- **Controls**: none; 저장 완료 → `Message::McpConfigurationsSaved` / `handle_mcp_configurations_saved`
- **Code path**:
  - Args: `src/services/mcp/tools.rs` → `CreateConfigurationArgs` (`disabled_environment_variables`), `UpdateConfigurationArgs` (`set_/remove_/enable_/disable_environment_variables`, `changes_something`), `DeleteConfigurationArgs`, `parse_call`, `require_bounded_type_data`, `require_configuration_name`, `require_working_directory`
  - State / handler: `src/app/mcp.rs` → `mcp_create_configuration`, `mcp_update_configuration`, `mcp_delete_configuration`, `edit_precondition`, `check_name_available`, `check_compound_members`, `reject_masked_env`, `mcp_refuse_edit`, `record_edit`, `edit_payload`, `derive_node_working_directory`
  - Data: configs.json, written in `src/app/mcp.rs` → `save_configurations_for_mcp` (`save_to_store`)
- **Tests**: `src/app/mcp.rs` → `creating_a_configuration_adds_it_with_a_server_chosen_id`, `an_edit_is_refused_until_the_stored_configurations_have_loaded`, `an_edit_is_refused_when_the_stored_configurations_could_not_be_read`, `a_masked_env_value_is_refused_instead_of_overwriting_the_secret`, `creating_a_configuration_with_a_masked_value_is_refused`, `a_configuration_can_be_created_with_disabled_variables`, `an_update_disables_and_enables_environment_variables`, `disabling_an_already_disabled_variable_changes_nothing`, `an_update_that_changes_nothing_says_so`, `deleting_a_configuration_reports_the_compounds_that_lost_a_member`, `a_failed_save_is_reported_as_an_mcp_failure`, `an_env_update_closes_the_users_open_env_modal`, `renaming_a_configuration_onto_a_taken_name_is_refused`; `src/services/mcp/tools.rs` → `create_refuses_to_disable_a_variable_it_does_not_set`, `enabling_and_disabling_the_same_variable_is_refused`, `a_toggle_alone_counts_as_a_change`, `a_typo_in_a_nested_mode_field_is_rejected_instead_of_erasing_the_stored_value`, `a_create_call_cannot_choose_the_new_id`, `a_uuid_shaped_name_is_rejected`
- **Pitfalls**: 중첩 mode 필드 오타로 저장된 인터프리터 경로가 지워지고 성공 응답 → `deny_unknown_fields`가 `ConfigTypeData`에서 멈췄음, source: `b2edd9e`; get으로 읽은 `<hidden>`을 되돌려 쓰면 비밀이 덮임 → 자리표시자 값 거절, source: `src/app/mcp.rs` → `reject_masked_env`; 시작 직후나 store 손상 시 편집 거절 → 빈 목록으로 store를 덮지 않기 위함, source: `src/app/mcp.rs` → `edit_precondition`; 무변경 update가 사용자의 미저장 GUI 편집을 디스크에 커밋 → `changed`가 false면 저장 생략, source: `src/app/mcp.rs` → `mcp_update_configuration`; UUID 모양 이름은 만들 수 없음 → 대상이 UUID로 파싱되면 이름 조회를 안 함, source: `src/app/mcp.rs` → `find_configuration_index`
- **Shares**: `mcp-request-dedup`, `src/services/storage.rs` → `save_to_store`, `EnvVars::set_enabled` (GUI env 체크박스와 같은 모델)
- **Related**: `mcp-read-configurations`, `mcp-request-dedup`, `mcp-audit-status`

### `mcp-request-dedup`: 변경성 툴의 `request_id` 재시도 흡수 (60초 창, 512개 상한)

- **Where**: 에이전트 툴 응답 (`deduplicated` 필드), 상태바 `[MCP]` 줄
- **Aliases**: 중복 실행 방지, 재시도, request_id, idempotency, retry window full, request_id reused
- **UI strings**: "request_id '{request_id}' was already used for '{recorded_target}' within the last ", "[MCP] Rejected {tool}: request_id reused for a different target", "The retry window is full: it already holds {MCP_DEDUP_MAX_ENTRIES} request_ids from the ", "[MCP] Retry window full — request refused"
- **Controls**: none
- **Code path**:
  - Log: `src/services/mcp/dedup.rs` → `RequestLog::lookup`, `RequestLog::record`, `RequestLog::forget`, `DedupScope`, `DedupLookup` (`Fresh`/`Duplicate`/`TargetMismatch`/`WindowFull`), `DEDUP_WINDOW`, `MAX_ENTRIES`
  - State / handler: `src/app/mcp.rs` → `mcp_dedup_lookup`, `mcp_refuse_window_full`, `mcp_refuse_target_mismatch`, `dedup_conflict`, `dedup_window_full`, `edit_target`, `input_target`, `duplicate_edit_payload`
  - Args: `src/services/mcp/tools.rs` → `optional_request_id`, `validate_request_id` (`MAX_REQUEST_ID_CHARS` 128)
  - Data: none (in-memory `McpRequestLog`)
- **Tests**: `src/services/mcp/dedup.rs` → `the_same_request_id_returns_the_first_sessions`, `a_reused_request_id_with_another_target_is_a_conflict`, `the_window_expires`, `a_window_full_of_valid_records_refuses_a_new_id`, `a_full_window_still_answers_a_duplicate`; `src/app/mcp.rs` → `the_same_request_id_does_not_start_a_second_run`, `a_full_retry_window_refuses_the_call_instead_of_running_it`, `a_full_retry_window_still_lets_a_call_without_a_request_id_through`, `a_dedup_hit_reports_a_session_the_user_removed`, `a_retry_is_absorbed_even_after_the_configuration_disappears`, `a_reused_update_request_id_with_a_different_patch_is_refused`, `the_same_create_request_id_does_not_create_twice`, `a_retried_delete_reports_the_first_deletion_instead_of_not_found`, `a_refused_edit_can_be_retried_with_the_same_request_id`; `src/services/mcp/tools.rs` → `a_blank_request_id_is_rejected`, `an_oversized_request_id_is_rejected`
- **Pitfalls**: 창이 가득 차면 새 `request_id` 호출이 거절됨 → 유효 기록을 축출하면 재시도가 두 번 실행되므로 축출 대신 거절, 실행·편집 툴이 상한을 공유, source: `src/services/mcp/dedup.rs` → `MAX_ENTRIES`; `request_id` 없는 호출은 재시도 시 두 번 실행 → 기록을 거치지 않음, source: `src/services/mcp/dedup.rs` 모듈 doc; 흡수된 응답에 세션 state가 "removed" → 창 안에서 사용자가 세션을 지웠음, source: `src/app/mcp.rs` → `session_entry`
- **Shares**: `src/app/mcp.rs`의 모든 변경성 핸들러
- **Related**: `mcp-execute-sessions`, `mcp-session-input`, `mcp-edit-configurations`

### `mcp-audit-status`: 에이전트 조작을 상태바에 `[MCP]` 접두사로 남기는 감사 줄

- **Where**: 메인 창 → 하단 상태바
- **Aliases**: 에이전트가 뭘 했는지, 상태바 MCP 표시, audit, 누가 실행했지
- **UI strings**: "[MCP] {}", "[MCP] Denied {tool}: beyond the current permission tier"
- **Controls**: none
- **Code path**:
  - State / handler: `src/app/mcp.rs` → `mcp_tag_status`, `handle_mcp_configurations_saved` (실패만 줄을 대체), `handle_mcp_request` (거부 줄)
  - Data: none (`status_message`)
- **Tests**: `src/app/mcp.rs` → `a_mutating_call_is_visible_in_the_status_bar`, `an_edit_leaves_an_audit_line_in_the_status_bar`, `a_finished_save_leaves_the_audit_line_alone`, `the_stop_audit_line_survives_the_run_report`, `a_reused_edit_request_id_for_another_target_leaves_an_audit_line`
- **Pitfalls**: 일어나지 않은 일이 `[MCP]`로 보고됨 → 호출 시점 상태바에 이번 호출의 문구가 있어야 함, source: `src/app/mcp.rs` → `mcp_tag_status`; 감사 줄은 다음 상태 메시지에 덮여 이력이 남지 않음 → 별도 MCP 로그 화면 없음 (absent)
- **Shares**: 앱 전역 `status_message`
- **Related**: `mcp-execute-sessions`, `mcp-edit-configurations`, `mcp-permission-tiers`
