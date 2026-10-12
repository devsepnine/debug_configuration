# Sessions

### `run-configuration`: 구성을 실행해 새 세션을 만들고 PTY로 프로세스를 띄운다

- **Where**: Configurations → 목록 항목의 재생 아이콘 / Cmd+R → Sessions 화면으로 전환
- **Aliases**: 실행, 런, 실행이 안 돼, 시작, run, start, launch, spawn, 프로세스가 안 떠
- **UI strings**: "Running: {}", "Update in progress — wait before running", "Failed to spawn process: {e}", "Process exited with code: {code}", "Error: {reason}", "Completed (exit code: {code})", "Failed: {reason}", "Run finished", "Run failed", "falling back to pipes"
- **Controls**: `ICON_PLAY` (목록 항목) → `Message::RunConfiguration(Some(index))` / `handle_run_configuration`; Cmd+R → `Message::RunShortcut` / `handle_run_shortcut` (Sessions 뷰에서 포커스 세션이 있으면 `handle_rerun_session`, 아니면 선택 구성 실행)
- **Code path**:
  - UI: `src/widgets/configuration_list.rs` → `view_configuration_item`
  - State / handler: `src/app.rs` → `handle_run_configuration`, `new_session`, `run_compound`, `handle_process_started`, `handle_output_received`, `handle_run_completed`, `notify_run_finished`
  - Executor: `src/services/executor.rs` → `run_configuration_stream`, `build_command`, `spawn_in_pty`, `handle_pty_process`, `create_process_command` (pipe 폴백), `handle_spawned_process`
  - Data: 세션은 메모리만 (`RunSession` in `src/models/session.rs` → `RunSession::new`); 영속 없음
- **Tests**: `src/services/executor.rs` → `pty_child_sees_a_tty_and_term`, `pty_child_is_its_own_process_group_leader`, `pty_progress_cr_collapses_to_final_state`, `build_command_wraps_existing_types_in_ok`; `src/app.rs` → `a_run_that_could_not_start_keeps_its_reason`, `run_compound_fans_out_to_member_sessions`, `run_compound_skips_missing_and_nested_members`, `run_compound_with_workspace_opens_new_named_tab`
- **Pitfalls**: 첫 출력이 화면 아래로 밀림(Windows) → ConPTY가 리사이즈마다 전체 리페인트, source: `30e98cd` / `src/app.rs` → `handle_run_configuration` (`last_seen_viewport`로 스폰); 업데이트 설치 중 실행 버튼이 무반응 → `is_updating` 가드, source: `src/app.rs` → `handle_run_configuration`; ConPTY 자식이 시작조차 안 함 → inherit-cursor 질의 미응답, source: `2a1f270`; [fixed] 대량 로그에서 UI가 멈추고 스크롤이 안 됨 → 줄 단위 `OutputReceived`가 메시지 폭주를 일으켰음(현재 가드: 배치 전송), source: `2c2fd10`; [fixed] 실행이 끝나도 완료 표시·exit code가 안 뜨고 CPU 100% → 한쪽 reader EOF 뒤 biased select가 그 분기만 계속 골랐음(현재 가드: 분기별 `reader.is_some()`), source: `4f15575`
- **Shares**: `src/services/executor.rs` (PTY 전송 계층, stdin/interrupt/eof 공용)
- **Related**: `run-working-dir-check`, `run-env-banner`, `session-list`

### `run-working-dir-check`: 실행 전 작업 디렉터리 존재 여부를 검사해 잘못된 cwd 실행을 막는다

- **Where**: Sessions → 터미널 출력 (배너 직후 실패 사유)
- **Aliases**: 작업 디렉터리, 경로 없음, 홈에서 실행됨, working directory, cwd, directory not found
- **UI strings**: "Working directory does not exist or is not a directory: {directory}", "Working Directory: {}"
- **Controls**: none (실행 시 자동)
- **Code path**:
  - State / handler: `src/services/executor.rs` → `verify_working_directory` (호출 위치 `run_configuration_stream`, `send_command_info` 뒤)
- **Tests**: `src/services/executor.rs` → `a_run_in_a_missing_directory_fails_before_spawning`, `a_missing_working_directory_fails_the_check`, `an_empty_working_directory_is_refused`
- **Pitfalls**: 없는 경로로 실행했는데 프로세스가 `$HOME`에서 돎 → `portable_pty::CommandBuilder::cwd`가 디렉터리가 아니면 값을 버림, source: `fd0ced2` / `src/services/executor.rs` → `verify_working_directory`
- **Shares**: `src/services/executor.rs`
- **Related**: `run-configuration`

### `run-env-banner`: 실행 배너(구성명·타입·cwd·명령)와 "Environment:" 줄, JAVA_HOME/JAVA_TOOL_OPTIONS 병합

- **Where**: Sessions → 터미널 출력 맨 위 `═══` 배너
- **Aliases**: 환경변수 줄, 환경변수가 안 보여, env 표시, JAVA_HOME 안 먹어, VM 옵션 사라짐, environment line, banner, JAVA_TOOL_OPTIONS
- **UI strings**: "Configuration: {}", "Working Directory: {}", "Command: {command_str}", "Environment: {rendered}", "Show environment line on run"
- **Controls**: Settings 모달 "Show environment line on run" → `Message::SettingsToggleEnvironment`
- **Code path**:
  - State / handler: `src/services/executor.rs` → `send_command_info`, `single_line`, `effective_extra_env`, `env_key_eq`, `resolved_process_env`, `derive_java_home`, `build_spring_boot_command`
  - Model: `src/models/session.rs` → `add_env_banner_line`, `is_env_banner_line` (MCP 마스킹용 `src/app/mcp.rs`)
  - Data: `show_environment_on_run` in settings, written in `src/services/storage.rs` → `save_settings`, read in `src/services/storage.rs` → `load_settings`
- **Tests**: `src/services/executor.rs` → `send_command_info_env_line_gated_by_show_env`, `a_newline_or_cr_in_an_env_value_does_not_split_the_banner`, `resolved_process_env_feeds_spawn_and_merges_java_options`, `disabled_env_entries_are_not_injected_or_merged`, `effective_env_appends_java_tool_options_and_user_java_home_wins`, `effective_extra_env_is_not_idempotent_so_it_must_run_once`, `effective_env_matches_names_case_insensitively_on_windows`; `src/models/session.rs` → `an_env_banner_event_marks_its_line_for_masking`, `evicting_a_banner_line_forgets_its_masking_record`
- **Pitfalls**: 사용자 JAVA_TOOL_OPTIONS 값이 중복됨 → `effective_extra_env`는 멱등이 아니라 `run_configuration_stream`에서 한 번만 호출해야 함, source: `src/services/executor.rs` → `resolved_process_env`; 꺼진(disabled) JAVA_HOME이 파생값을 막음 → `active()`만 사용자 지정으로 침, source: `src/services/executor.rs` → `effective_extra_env`; env 값의 개행이 배너를 쪼갬 → `single_line` 이스케이프, source: `src/services/executor.rs` → `single_line`
- **Shares**: `src/models/env_vars.rs` (`EnvVars::active`)
- **Related**: `run-configuration`, `terminal-output`

### `session-list`: 좌측 세션 목록(상태 점·배지·경과시간), 개별 정지/제거, 클릭으로 워크스페이스에 열기

- **Where**: Sessions → 좌측 "Sessions" 패널 (260px)
- **Aliases**: 세션 목록, 실행 목록, 정지, 중지, 삭제, 타이머, 경과 시간, 종료됨, session list, stop, kill, remove, elapsed, duration, status badge
- **UI strings**: "Sessions", "Run a configuration to create a session.", "Ended", "Stop session", "Remove session", "Session stopped", "Session removed", "Session opened in workspace", "Session already open in workspace", "Running {}", "✕ exit {code}", "Stopped", "✕ error"
- **Controls**: `ICON_STOP` "Stop session" → `Message::StopSession` / `handle_stop_session`; `ICON_DELETE` "Remove session" → `Message::RemoveSession` / `handle_remove_session`; 항목 클릭 → `Message::OpenSessionInWorkspace` / `handle_open_session_in_workspace`; hover → `Message::SessionListItemHovered`
- **Code path**:
  - UI: `src/app.rs` → `view_session_list_panel`, `view_session_list_item`, `view_session_ended_divider`, `split_sessions_by_run_state`, `view_session_action_button`
  - State / handler: `src/app.rs` → `handle_stop_session`, `handle_remove_session`; 1초 tick `Message::SessionTimerTick` in `subscription`
  - Model: `src/models/session.rs` → `status_kind`, `status_badge_label_compact`, `status_badge_label`, `format_duration`; 메인 탭 라이브 점 `src/views/main_tabs.rs` → `live_session_dot`
  - Executor: `src/services/executor.rs` → `terminate_session_process`, `unregister_running_pid`
- **Tests**: `src/app.rs` → `split_sessions_keeps_store_indices_and_order`, `hovered_index_follows_the_session_moved_to_the_ended_group`, `remove_session_by_id_removes_the_correct_session`, `remove_session_shifts_hovered_index`, `stop_session_by_id_sets_cancel_flag`, `a_user_stop_is_not_reported_as_a_failure`, `stop_discards_output_queued_behind_cancel`; `src/models/session.rs` → `status_kind_reflects_state`, `compact_badge_stays_short_for_fixed_width_sidebar`, `status_badge_shows_live_elapsed_while_running`, `format_duration_is_human_readable`
- **Pitfalls**: [fixed] 출력 폭주 중 Stop이 수 초 늦음 → 큐에 쌓인 배치 뒤에 RunCompleted가 도착했음(현재 가드: Stop 클릭 시 UI 상태 즉시 확정, 취소 뒤 배치 폐기), source: `8c68362`, `566291d` / `src/app.rs` → `handle_output_received`; hover 강조가 엉뚱한 항목 → 표시 순서와 저장 인덱스 혼동, source: `src/app.rs` → `split_sessions_by_run_state`; exit code 배지가 잘려 보임 → 이름 영역을 지키려고 배지를 11자로 절단, source: `src/app.rs` → `view_session_list_item`
- **Shares**: `src/models/session.rs`
- **Related**: `session-rerun-bulk`, `pane-controls`

### `session-rerun-bulk`: 세션 재실행(개별)과 전체 재실행 / 실패만 재실행 / 전체 정지

- **Where**: Sessions → 좌측 패널 헤더 아이콘 3개, pane 타이틀바 재실행 아이콘
- **Aliases**: 재실행, 다시 실행, 모두 재시작, 실패한 것만, 모두 중지, rerun, restart, rerun failed, stop all, Cmd+R
- **UI strings**: "Stop all running", "Rerun all", "Rerun failed", "Stopping session for restart...", "Rerunning: {}", "Configuration '{config_name}' not found", "No running sessions to stop", "Stopping {count} session(s)...", "No {noun} to rerun", "Rerunning {} {noun}..."
- **Controls**: Cmd+R(Sessions 화면이면 포커스 세션 재실행) → `Message::RunShortcut` / `handle_run_shortcut`; `ICON_STOP` "Stop all running" → `Message::StopAllSessions` / `handle_stop_all_sessions`; `ICON_REFRESH` "Rerun all" → `Message::RerunAllSessions` / `handle_rerun_all_sessions`; `ICON_PLAY` "Rerun failed" → `Message::RerunFailedSessions` / `handle_rerun_failed_sessions`; pane `ICON_REFRESH` → `Message::RerunSession` / `handle_rerun_session`
- **Code path**:
  - UI: `src/app.rs` → `view_session_list_panel`; `src/views/pane_view.rs` → `view_session_controls`
  - State / handler: `src/app.rs` → `handle_rerun_session`, `rerun_sessions`
- **Tests**: `src/app.rs` → `stop_all_cancels_only_running_sessions`, `rerun_failed_reruns_only_nonzero_exit_sessions`, `rerun_all_reruns_every_session`, `rerun_failed_with_no_failures_is_noop`, `a_rerun_follows_a_renamed_configuration`, `a_rerun_refuses_another_configuration_that_took_the_name`, `a_lost_configurations_rerun_names_the_session_creation_time_name`, `a_session_the_app_started_reruns_after_a_rename`
- **Pitfalls**: 구성 이름을 바꾸면 재실행이 "not found" 또는 다른 구성을 실행 → 이름 대신 config id로 조회, source: `e750c11`; 재실행 시 설정 변경(max lines/auto-scroll)이 안 반영 → 기존 세션 객체 재사용, source: `src/app.rs` → `handle_rerun_session`
- **Shares**: `src/services/executor.rs` → `run_configuration_stream`
- **Related**: `session-list`, `run-configuration`

### `terminal-output`: 세션 출력 렌더링(ANSI 색, 줄바꿈, 스크롤바, URL 클릭, 선택·복사)과 출력 버퍼 상한

- **Where**: Sessions → 워크스페이스 pane 본문 (canvas)
- **Aliases**: 로그, 출력, 콘솔, 색이 깨져, 글자 깨짐, 복사가 안 돼, 링크 클릭, 오래된 로그 사라짐, 최대 줄 수, output, terminal, ANSI color, copy, max lines, scrollback
- **UI strings**: "Max output lines per session"
- **Controls**: Cmd+C (포커스 pane, 선택 영역) → `Message::CopyToClipboard` → `iced::clipboard::write`; URL 클릭 → `Message::OpenUrl` / `handle_open_url`; 리사이즈 → `Message::SessionViewportResized` / `handle_session_viewport_resized`; Settings "Max output lines per session" → `Message::SettingsMaxLinesChanged`
- **Code path**:
  - UI: `src/views/terminal.rs` → `view_terminal_for_session`, `prepare_lines`, `extract_selected_text`, `urls_in_line`, `render_lines`
  - ANSI: `src/ansi.rs` → `parse_ansi_text`, `parse_sgr_params`
  - Model: `src/models/session.rs` → `apply_output_event`, `add_output_line`, `replace_last_line`, `evict_over_budget` (`DEFAULT_MAX_OUTPUT_LINES`, `MAX_OUTPUT_BYTES`, `MAX_STORED_LINE_BYTES`)
  - Executor: `src/services/executor.rs` → `pty_output_loop` (CR/백스페이스 조립), `resize_session_pty`
  - Data: `max_output_lines` in settings, `src/services/storage.rs` → `save_settings` / `load_settings`; 새 세션부터 적용 `src/app.rs` → `new_session`
- **Tests**: `src/ansi.rs` → `test_basic_color_parsing`, `test_256_color`, `osc_sequences_are_swallowed`, `bare_bel_is_dropped_but_osc_terminator_still_works`; `src/views/terminal.rs` → `unfocused_pane_does_not_answer_copy_shortcut`, `focused_pane_copies_its_own_selection`, `eviction_shifts_selection_like_offset`, `eviction_shifts_offset_up_to_preserve_view`, `incremental_wrap_update_matches_full_rebuild`; `src/models/session.rs` → `custom_max_output_lines_caps_buffer`, `test_max_lines_limit`, `byte_budget_evicts_before_line_cap`, `long_line_is_truncated_with_marker`, `cr_collapses_progress_bar_to_final_state`; `src/services/executor.rs` → `assembler_cr_replaces_whole_line`, `assembler_backspace_is_utf8_boundary_aware`
- **Pitfalls**: Cmd+C가 다른 pane 내용을 복사 → 포커스 pane만 선택 보유, source: `d81c1a7` / `src/views/pane_view.rs` → `view_pane_content`; OSC 시퀀스가 화면에 새어 나옴 → BEL이 조립기에서 사라져 OSC 미종결, source: `561e0d0`; 출력이 쏟아지는 중 위로 스크롤하면 화면이 튐 → 오래된 줄 evict가 절대 offset이 가리키는 위치를 바꿈, source: `05063a6`; [fixed] 대량 로그에서 화면 멈춤 → 줄 단위 메시지 폭주(현재 가드: 배치 전송), source: `2c2fd10`; 설정 변경이 기존 세션에 안 먹음 → `new_session`에서만 주입, source: `src/app.rs` → `new_session`
- **Shares**: `src/views/terminal.rs` (검색 하이라이트·자동 스크롤 공용)
- **Related**: `terminal-auto-scroll`, `output-clear-export`, `session-search`

### `terminal-auto-scroll`: 새 출력 따라가기(자동 스크롤) 토글과 스크롤 시 자동 해제

- **Where**: Sessions → pane 타이틀바 아래 화살표 아이콘 (또는 ⋯ 메뉴)
- **Aliases**: 자동 스크롤, 맨 아래로, 스크롤이 안 따라와, 스크롤 고정, auto-scroll, follow, tail, pin to bottom
- **UI strings**: "Auto-scroll new sessions"
- **Controls**: `ICON_ARROW_DOWN_FILL` / `ICON_ARROW_DOWN_LINE` → `Message::ToggleAutoScroll` / `handle_toggle_auto_scroll`; 휠/드래그 → `Message::SessionScrollChanged` / `handle_session_scroll_changed`; Settings "Auto-scroll new sessions" → `Message::SettingsToggleAutoScroll`
- **Code path**:
  - UI: `src/views/pane_view.rs` → `view_session_controls`, `view_session_controls_menu`; `src/views/terminal.rs` → `apply_auto_scroll`, `is_at_bottom`, `publish_scroll_progress`
  - State / handler: `src/app.rs` → `handle_toggle_auto_scroll`, `handle_session_scroll_changed`
  - Data: `default_auto_scroll` in settings, `src/services/storage.rs` → `save_settings` / `load_settings`
- **Tests**: `src/views/terminal.rs` → `wheel_is_not_preempted_by_auto_scroll_during_streaming`, `redraw_repins_to_bottom_when_auto_scroll`, `redraw_does_not_repin_when_auto_scroll_disabled`, `at_bottom_uses_absolute_distance_not_ratio`
- **Pitfalls**: 큰 버퍼에서 위로 올려도 자동 스크롤이 안 풀림 → 비율(≥0.99) 판정이라 바닥 근처 수백 줄이 모두 바닥으로 취급됨(절대 거리로 수정), source: `0b36677` / `src/app.rs` → `handle_session_scroll_changed`
- **Shares**: `src/views/terminal.rs`
- **Related**: `terminal-output`, `session-search`

### `output-clear-export`: 세션 출력 지우기와 파일로 내보내기

- **Where**: Sessions → pane 타이틀바 지우개·저장 아이콘 (또는 ⋯ 메뉴)
- **Aliases**: 로그 지우기, 클리어, 로그 저장, 내보내기, 파일로 저장, clear, export log, save output
- **UI strings**: "Exporting output...", "Output saved: {}", "{safe_name}-output.log"
- **Controls**: `ICON_ERASER` → `Message::ClearSessionOutput` / `handle_clear_session_output`; `ICON_SAVE` → `Message::ExportSessionOutput` / `handle_export_session_output` → `Message::SessionOutputExported` / `handle_session_output_exported` (둘 다 출력이 비면 비활성)
- **Code path**:
  - UI: `src/views/pane_view.rs` → `view_session_controls`, `view_session_controls_menu`
  - State / handler: `src/app.rs` → `handle_clear_session_output`, `handle_export_session_output`
  - Model: `src/models/session.rs` → `clear_output`
  - Data: 사용자가 고른 경로에 평문 저장, written in `src/services/storage.rs` → `export_text`
- **Tests**: `src/app.rs` → `clear_session_output_empties_buffer_and_search_cache`; `src/models/session.rs` → `test_clear_output`, `clear_output_drops_pending_scroll_target`; `src/views/terminal.rs` → `output_clear_drops_selection`
- **Pitfalls**: 내보낸 파일에 색 코드 없음 → 세그먼트 텍스트만 이어 붙임, source: `src/app.rs` → `handle_export_session_output`; 저장/지우기 버튼이 회색 → 출력 없음 가드, source: `src/views/pane_view.rs` → `view_session_controls`
- **Shares**: `src/services/storage.rs` → `export_text` (구성 내보내기와 공용 다이얼로그 경로)
- **Related**: `terminal-output`

### `session-search`: 출력 내 검색(하이라이트, 다음/이전, 정규식, 매치 줄만 보기, Cmd+F)

- **Where**: Sessions → pane 상단 검색바
- **Aliases**: 검색, 찾기, 검색이 안 돼, 필터, 정규식, 다음 찾기, find, search, grep, regex, filter lines, Cmd+F
- **UI strings**: "Find in output...", "bad regex", "0/0", "Previous match (Shift+Enter)", "Next match (Enter)", "Regex (case-insensitive)", "Show only matching lines", "Close search (Esc)", "No lines match "
- **Controls**: `ICON_SEARCH` → `Message::OpenSessionSearch` / `Message::CloseSessionSearch`; 입력 → `Message::SessionSearchChanged` / `handle_session_search_changed`; "<" / ">" → `Message::SessionSearchPrev` / `Message::SessionSearchNext` → `handle_session_search_step`; ".*" → `Message::ToggleSessionSearchRegex`; "filter" → `Message::ToggleSessionSearchFilter`; "x" → `Message::CloseSessionSearch`; Cmd+F → `Message::OpenSearchInActivePane`; Enter/Shift+Enter → `Message::SearchNextInActivePane` / `Message::SearchPrevInActivePane`; Esc → `Message::CloseActiveBar`
- **Code path**:
  - UI: `src/views/pane_view.rs` → `view_session_search_bar`, `glyph_button`; `src/views/terminal.rs` → `prepare_lines`, `render_line_highlights`
  - State / handler: `src/app.rs` → `handle_open_session_search`, `handle_session_search_step`, `search_open_target`, `search_nav_target`, `search_nav_message`, `handle_close_active_bar`
  - Model: `src/models/session.rs` → `compile_search_regex`, `search_matches`, `refresh_search_matches`, `refresh_search_matches_incremental`
- **Tests**: `src/app.rs` → `search_step_cycles_matches_and_scrolls_to_match`, `search_nav_enter_next_shift_enter_prev`, `search_current_clamps_when_matches_shrink`, `search_open_targets_clicked_session_not_first`, `search_nav_follows_open_search_even_after_focus_moves`, `search_nav_prefers_focused_pane_when_it_is_searching`, `search_open_falls_back_when_focused_session_closed`, `close_active_bar_prefers_menu_then_stdin_then_search`; `src/models/session.rs` → `search_matches_substring_returns_line_and_char_ranges`, `search_matches_supports_regex_case_insensitive`, `incremental_search_equals_full_rescan_randomized`; `src/views/terminal.rs` → `scroll_target_resolves_within_filtered_rows`, `filter_toggle_drops_selection`
- **Pitfalls**: Cmd+F/Enter가 엉뚱한 pane을 검색 → 포커스 pane 우선 해석, source: `383cc77`; 매치로 점프했는데 스크롤 안 됨 → 앱(비율)과 터미널 위젯(절대 행)의 스크롤 상태가 따로 놀아 동기화가 한 방향뿐이었음, source: `0b36677`; stdin 입력 중 Enter가 검색 다음으로 이중 발화 → `Status::Ignored` 게이트, source: `src/app.rs` → `subscription` (`search_nav_subscription`)
- **Shares**: `src/views/shared.rs` → `session_search_input_id`
- **Related**: `terminal-output`, `keyboard-shortcuts`

### `session-stdin`: 실행 중 프로세스에 입력 보내기, 인터럽트(Ctrl+C), EOF(Ctrl+D), 입력 히스토리(↑/↓)

- **Where**: Sessions → pane 하단 stdin 바
- **Aliases**: 입력, 표준입력, 키 입력이 안 돼, 컨트롤C, 중단, 입력 기록, stdin, send input, interrupt, Ctrl+C, EOF, Ctrl+D, history, REPL
- **UI strings**: "Send input to process (Enter)", "Process not running", "Interrupt (Ctrl+C)", "Send EOF (Ctrl+D)", "Send (Enter)", "Close input (Esc)", "Input pending — the process is not reading stdin yet", "Input failed: {error}", "Interrupt pending — the process is not reading stdin yet", "Interrupt is not available for this session", "EOF pending — the process is not reading stdin yet", "EOF is not available for this session", "stdin input is disabled for this run"
- **Controls**: `ICON_TERMINAL_INPUT` / Cmd+I → `Message::OpenSessionStdin` / `Message::OpenStdinInActivePane`; 입력 → `Message::SessionStdinChanged`; Enter / "⏎" → `Message::SessionStdinSubmitted` / `handle_session_stdin_submitted`; "^C" / Ctrl+C → `Message::SessionInterruptRequested` / `handle_session_interrupt_requested`; "^D" / Ctrl+D → `Message::SessionEofRequested` / `handle_session_eof_requested`; ↑/↓ → `Message::StdinBarKeyPressed` → `handle_stdin_bar_key_resolved`; "x" / Esc → `Message::CloseSessionStdin` / `Message::CloseActiveBar`
- **Code path**:
  - UI: `src/views/pane_view.rs` → `view_session_stdin_bar`
  - State / handler: `src/app.rs` → `handle_session_stdin_submitted`, `handle_session_stdin_write_completed`, `echo_stdin_locally`, `stdin_bar_key_message`, `handle_stdin_bar_key_pressed`, `handle_session_interrupt_completed`, `try_signal_interrupt`, `handle_session_eof_completed`
  - Model: `src/models/session.rs` → `push_stdin_history`, `navigate_stdin_history`, `reset_stdin_history_navigation`
  - Executor: `src/services/executor.rs` → `write_session_stdin`, `interrupt_session`, `eof_session`, `interrupt_session_process`, `acquire_session_writer`
- **Tests**: `src/app.rs` → `stdin_open_close_and_draft_lifecycle`, `stdin_submit_pushes_history`, `stdin_history_survives_rerun`, `stdin_bar_key_message_maps_ctrl_c_regardless_of_status`, `stdin_bar_key_message_maps_ctrl_d_to_eof`, `stdin_bar_key_message_maps_arrows_only_when_ignored`, `interrupt_completed_broken_without_pid_sets_status`, `eof_completed_reports_only_while_running`, `stdin_write_completed_restore_and_echo_rules`, `stdin_submit_takes_draft_only_while_running`; `src/models/session.rs` → `stdin_history_dedupes_consecutive_and_caps`, `stdin_history_navigation_walks_and_restores_draft`; `src/services/executor.rs` → `pty_stdin_echo_roundtrip`, `pty_interrupt_stops_foreground_child`, `pty_eof_ends_stdin_reader`, `interrupt_session_unregistered_session_is_broken`
- **Pitfalls**: 자식이 stdin을 안 읽으면 Ctrl+C가 안 먹음 → 앞선 쓰기가 writer mutex를 잡은 채 막혀 ETX가 타임아웃(그룹 SIGINT로 에스컬레이션), source: `5573af7`; Windows에서 큰 입력을 바로 보내면 세션이 멈춤 → ConPTY DSR 핸드셰이크 전 쓰기가 입력 파이프를 채워 reader의 DSR 응답을 막음, source: `9e0bcdf`; Dvorak 등 다른 배열에서 Ctrl+C 미인식 → 예전엔 물리 KeyC만 봤음(지금은 물리 키 ∪ 논리 문자 "c", 한글 배열은 물리 키로 인식), source: `src/app.rs` → `stdin_bar_key_message`; unix pipe 폴백 세션에서 입력·Ctrl+C가 안 됨 → 폴백은 출력 전용이라 stdin writer가 없음, source: `src/services/executor.rs` → `interrupt_session`
- **Shares**: `src/views/shared.rs` → `session_stdin_input_id`
- **Related**: `keyboard-shortcuts`, `run-configuration`

### `pane-controls`: pane 타이틀바(세션명·상태 배지) 컨트롤, 좁을 때 ⋯ 메뉴, 최대화/복원, 닫기(숨기기)

- **Where**: Sessions → 워크스페이스 pane 타이틀바
- **Aliases**: 창 최대화, 패널 닫기, 더보기 메뉴, 버튼이 안 보여, maximize, restore, close pane, overflow menu, hide session
- **UI strings**: "Empty", "Empty pane", "Session not found", "Pane maximized", "Pane restored", "Session hidden from workspace", "No session in pane"
- **Controls**: `ICON_PANE_MAXIMIZE` / `ICON_PANE_RESTORE` → `Message::TogglePaneMaximize` / `handle_toggle_pane_maximize`; `ICON_CLOSE` → `Message::ClosePane` / `handle_close_pane`; Cmd+W → `Message::CloseFocusedPane` / `handle_close_focused_pane`; `ICON_MORE` → `Message::ToggleSessionControlsMenu` / `handle_toggle_session_controls_menu`; pane 클릭 → `Message::PaneClicked` / `handle_pane_clicked`
- **Code path**:
  - UI: `src/views/pane_view.rs` → `view_pane_content`, `view_session_controls`, `view_session_controls_compact`, `view_session_controls_menu`, `session_title`, `pane_focus`
  - State / handler: `src/app.rs` → `handle_toggle_pane_maximize`, `handle_close_pane`, `close_controls_menu_on_action`
  - Widget: `src/widgets/pane_grid/controls.rs` → `dynamic`; `src/widgets/pane_grid/title_bar.rs` → `always_show_controls`
- **Tests**: `src/views/pane_view.rs` → `pane_focus_inactive_for_single_pane`, `pane_focus_splits_focused_and_unfocused_with_multiple_panes`; `src/app.rs` → `close_focused_pane_hides_session_but_keeps_it_in_list`, `close_focused_pane_is_noop_outside_sessions_view`, `controls_menu_persists_across_item_actions_but_closes_on_layout_change`, `controls_menus_open_independently_per_session`
- **Pitfalls**: 한 pane의 ⋯ 메뉴를 열면 다른 pane 메뉴가 닫힘 → 토글이 전역 단일 열림을 강제했음, source: `ed6c2ca`; 탭 닫기·Cmd+W 뒤 다시 연 세션에 ⋯ 메뉴가 남아 있음 → 두 경로가 전역 메뉴 닫기 목록에서 빠졌음, source: `30723ee`; 닫기 눌렀는데 프로세스가 계속 돎 → 닫기는 숨기기일 뿐(세션·프로세스 유지), source: `src/app.rs` → `handle_close_focused_pane`; pane 1개일 때 최대화 무반응 → `panes.len() <= 1` 가드, source: `src/app.rs` → `handle_toggle_pane_maximize`
- **Shares**: `src/widgets/pane_grid/` (vendored iced pane_grid, 타이틀바 어디서나 드래그)
- **Related**: `pane-grid-layout`, `session-list`

### `pane-grid-layout`: 세션 열 때 자동 분할, pane 드래그로 재배치, 분할선 리사이즈

- **Where**: Sessions → 워크스페이스 본문 pane grid
- **Aliases**: 화면 분할, 분할, 드래그로 옮기기, 크기 조절, 레이아웃, split, drag pane, resize, layout, grid
- **UI strings**: "Dragging pane: {pane:?}", "Pane dropped", "Drag canceled", "No sessions in this workspace", "Select a session from the left list to open it here."
- **Controls**: 타이틀바 드래그 → `Message::PaneGridDragged` / `handle_pane_grid_dragged`; 분할선 → `Message::PaneGridResized` / `handle_pane_grid_resized`; 커서 → `Message::CursorMoved` / `handle_cursor_moved`
- **Code path**:
  - UI: `src/views/pane_view.rs` → `view_pane_layout`; `src/app.rs` → `view_sessions_screen`, `view_empty_workspace_content`
  - State / handler: `src/app.rs` → `handle_pane_grid_dragged`, `handle_pane_grid_resized`, `workspace_content_aspect`
  - Model: `src/widgets/pane_layout.rs` → `WorkspaceTab::open_session`, `remove_session`, `sync_layout_tree_from_pane_grid`, `LayoutTree::best_split_target`, `split_leaf`, `remove_empty_leaves`
  - Widget: `src/widgets/pane_grid/mod.rs`, `src/widgets/pane_grid/state.rs`
- **Tests**: `src/widgets/pane_layout.rs` → `best_split_target_splits_largest_along_longer_side`, `open_session_keeps_layout_balanced`, `open_session_fills_empty_leaf_first`
- **Pitfalls**: none
- **Shares**: `src/widgets/pane_grid/`
- **Related**: `pane-controls`, `workspace-tabs`

### `workspace-tabs`: 워크스페이스 탭 추가·이름 변경(더블클릭)·닫기·전환, Cmd+1~9 점프

- **Where**: Sessions → 워크스페이스 탭 바 (pane grid 위)
- **Aliases**: 탭, 워크스페이스, 탭 이름 바꾸기, 탭 추가, 탭 닫기, 탭 이동, workspace, tab, rename tab, new tab, jump to workspace
- **UI strings**: "Tab name", "Workspace {n}", "Workspace added", "Workspace cleared", "Tab closed", "No workspace tab available."
- **Controls**: `ICON_ADD` → `Message::AddWorkspaceTab` / `handle_add_workspace_tab`; 탭 클릭(300ms 내 더블클릭 = 편집) → `Message::TabNameClicked` / `handle_tab_name_clicked`; 입력 → `Message::TabNameInputChanged`; Enter → `Message::FinishEditingTabName` / `finish_editing_tab_name`; 바깥 클릭 → `Message::CancelEditingTabNameOnOutsideClick`; `ICON_CLOSE` (hover 시) → `Message::CloseTab` / `handle_close_tab` (편집 중엔 `Message::CancelEditingTabName`); Cmd+1~9 → `Message::JumpToWorkspace` / `handle_jump_to_workspace`
- **Code path**:
  - UI: `src/views/workspace_tabs.rs` → `view_workspace_tab_bar`, `view_tab_label`, `view_close_button`, `view_add_tab_button`
  - State / handler: `src/app.rs` → `handle_tab_name_clicked`, `finish_editing_tab_name`, `cancel_editing_tab_name_on_outside_click`, `handle_close_tab`, `next_workspace_name`, `unique_workspace_name`, `ensure_workspace_tab`
  - Model: `src/widgets/pane_layout.rs` → `WorkspaceTab::empty`, `clear`
  - Data: 탭은 메모리만 (영속 없음)
- **Tests**: `src/app.rs` → `jump_to_workspace_switches_view_and_selects_tab`, `jump_to_workspace_out_of_range_is_noop`, `jump_to_workspace_from_sessions_only_changes_tab`, `next_workspace_name_numbers_and_fills_gaps`, `next_workspace_name_excludes_self_so_renamed_tab_reclaims_its_number`, `unique_workspace_name_keeps_base_then_numbers_on_collision`
- **Pitfalls**: 마지막 탭 닫기가 탭을 안 없앰 → 1개면 비우고 "Workspace 1"로 리셋, source: `src/app.rs` → `handle_close_tab`; 이름을 지웠더니 "Workspace N"으로 바뀜 → 공백 이름은 번호 이름으로 대체, source: `src/app.rs` → `finish_editing_tab_name`; 탭 편집 중 단축키 무반응 → nav 단축키 비활성, source: `src/app.rs` → `subscription`
- **Shares**: `src/widgets/pane_layout.rs`
- **Related**: `pane-grid-layout`, `keyboard-shortcuts`

### `keyboard-shortcuts`: 전역 단축키(F1/F2 화면 전환, Cmd+1~9, Cmd+S/R/W/I/,, Cmd+F, Esc, Enter)

- **Where**: 전역 (Sessions 전용 키 포함) → `subscription`
- **Aliases**: 단축키, 키보드, 단축키가 안 먹어, 핫키, shortcut, hotkey, keybinding, F1, F2, Cmd+R
- **UI strings**: "Configurations", "Sessions"
- **Controls**: F1 / F2 → `Message::SwitchView`; Cmd+1~9 → `Message::JumpToWorkspace`; Cmd+S → `Message::SaveConfigurations`; Cmd+R → `Message::RunShortcut`; Cmd+W → `Message::CloseFocusedPane`; Cmd+I → `Message::OpenStdinInActivePane`; Cmd+, → `Message::OpenSettingsModal`; Cmd+F → `Message::OpenSearchInActivePane`; Esc → `Message::CloseActiveBar`; 메인 탭 버튼 → `Message::SwitchView`
- **Code path**:
  - UI: `src/views/main_tabs.rs` → `view_main_tabs`
  - State / handler: `src/app.rs` → `subscription`, `nav_shortcut_message`, `stdin_bar_key_message`, `search_nav_message`, `any_modal_open`
- **Tests**: `src/app.rs` → `nav_shortcut_f1_f2_switch_views`, `nav_shortcut_cmd_digits_jump_to_workspace`, `nav_shortcut_rejects_cmd_zero_and_modifier_combos`, `nav_shortcut_cmd_letters_map_to_actions`, `stdin_open_target_requires_sessions_view`
- **Pitfalls**: 모달이 열려 있으면 모든 단축키 무반응 → `any_modal_open` 가드, source: `src/app.rs` → `any_modal_open`; Cmd+Shift+숫자 무반응 → Shift/Alt 조합 의도적 배제, source: `src/app.rs` → `nav_shortcut_message`; Cmd+F1 무반응 → F1/F2는 수식키 없을 때만, source: `src/app.rs` → `nav_shortcut_message`
- **Shares**: `src/app.rs` → `subscription`
- **Related**: `session-search`, `session-stdin`, `workspace-tabs`
