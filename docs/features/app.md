# App chrome and modals

### `settings-modal`: 앱 설정 모달 열기/staging/Apply/OK/Cancel 흐름

- **Where**: Configurations 화면 → 툴바 Settings 버튼 → 중앙 모달 오버레이
- **Aliases**: 설정, 환경설정, 옵션, 설정창, preferences, settings, Apply 안 눌림, 설정이 안 먹음
- **UI strings**: "Settings", "Cancel", "Apply", "OK", "AI (MCP)"
- **Controls**: "Settings" (`ICON_SETTINGS`) → `Message::OpenSettingsModal` / `handle_open_settings_modal`, Cmd+, → `Message::OpenSettingsModal` (`nav_shortcut_message`), "OK" → `Message::ConfirmSettingsModal` / `handle_confirm_settings_modal`, "Apply" → `Message::ApplySettingsModal` / `handle_apply_settings_modal`, "Cancel" / `ICON_CLOSE` / Esc → `Message::CancelSettingsModal` / `handle_cancel_settings_modal`
- **Code path**:
  - UI: `src/views/configuration_editor.rs` → `view_settings_modal`, 레이어 조립 `src/app.rs` → `view`
  - State / handler: `src/app/settings_modal.rs` → `SettingsModalState`, `apply_settings`, `settings_modal_is_dirty`, `settings_modal_can_apply`; 모달 단일화 `src/app.rs` → `close_all_modals`; Esc 구독 `src/app.rs` → `subscription`
  - Data: Apply/OK 시 `src/app.rs` → `save_app_settings` 호출 (`settings-persistence` 참고)
- **Tests**: `src/app/settings_modal.rs` → `a_freshly_opened_modal_has_no_changes`, `a_toggled_field_counts_as_a_change`, `retyping_the_applied_value_is_not_a_change`, `applying_without_changes_does_nothing`, `applying_clears_the_pending_changes`, `cancelling_after_apply_keeps_the_applied_values`, `confirming_closes_the_modal`, `applying_keeps_the_modal_open_and_normalizes_staged_text`, `cancelling_discards_staged_mcp_changes`; `src/app.rs` → `nav_shortcut_cmd_letters_map_to_actions`
- **Pitfalls**: Apply 버튼이 회색(비활성) → staging이 적용값과 같으면 비활성, 클램프 후 같은 값을 다시 친 것도 변경 아님, source: `src/app/settings_modal.rs` → `settings_modal_is_dirty`; Apply 후 Cancel 했는데 값이 남음 → Cancel은 마지막 Apply 이후 변경만 버림, source: `src/app/settings_modal.rs` → `SettingsModalState`
- **Shares**: 모달 오버레이 스타일 `src/views/configuration_editor.rs` → `modal_dialog_style`, `modal_backdrop_style`, `modal_primary_button_style`, `modal_secondary_button_style`; MCP 섹션(Enable local MCP server, Permission, Port, Token)은 같은 모달이지만 이 문서 범위 밖
- **Related**: `settings-run-defaults`, `settings-persistence`, `update-check`

### `settings-run-defaults`: 실행 관련 기본값 (Environment 라인 표시, 최대 출력 라인, 자동 스크롤, 시작 시 업데이트 확인)

- **Where**: Settings 모달 → 상단 체크박스/입력 영역
- **Aliases**: 환경변수 줄, Environment 라인 숨기기, 출력 줄 수, 로그 잘림, 최대 라인, 자동 스크롤, 스크롤 따라가기, 업데이트 자동 확인, env line, max lines, auto scroll, check updates on startup
- **UI strings**: "Show environment line on run", "Max output lines per session", "50000", "Auto-scroll new sessions", "Check for updates on startup"
- **Controls**: "Show environment line on run" → `Message::SettingsToggleEnvironment` / `handle_settings_toggle_environment`, "Max output lines per session" 입력 → `Message::SettingsMaxLinesChanged` / `handle_settings_max_lines_changed`, "Auto-scroll new sessions" → `Message::SettingsToggleAutoScroll` / `handle_settings_toggle_auto_scroll`, "Check for updates on startup" → `Message::SettingsToggleAutoCheckUpdates` / `handle_settings_toggle_auto_check_updates`
- **Code path**:
  - UI: `src/views/configuration_editor.rs` → `view_settings_modal`
  - State / handler: `src/app/settings_modal.rs` → `apply_settings`, `parse_max_output_lines` (`MIN_OUTPUT_LINES`..=`MAX_OUTPUT_LINES_LIMIT` 클램프); 소비처 `src/app.rs` → `new_session` (`max_output_lines`, `auto_scroll`), `handle_run_configuration`·`handle_rerun_session` (`show_environment_on_run` 전달), 배너 생성 `src/services/executor.rs` (`"Environment: {rendered}"`)
  - Data: `AppSettings.show_environment_on_run` / `max_output_lines` / `default_auto_scroll` / `auto_check_updates`, written in `src/app.rs` → `save_app_settings`, read in `src/app.rs` → `new`
- **Tests**: `src/app/settings_modal.rs` → `parse_max_output_lines_clamps_to_range`, `parse_max_output_lines_keeps_fallback_on_invalid`, `applying_normalizes_the_output_lines_text`
- **Pitfalls**: 1000 미만이나 문자를 넣었는데 다른 값으로 바뀜 → 파싱 실패면 이전값 유지, 성공이면 범위 클램프, source: `src/app/settings_modal.rs` → `parse_max_output_lines`; 설정을 바꿨는데 기존 세션에 반영 안 됨 → 값은 세션 생성 시점에 복사됨, source: `src/app.rs` → `new_session`; 업데이트 확인을 켜자마자 확인이 한 번 돎 → 켜는 Apply가 즉시 확인을 발행, source: `src/app/settings_modal.rs` → `apply_settings`
- **Shares**: none
- **Related**: `settings-modal`, `settings-persistence`, `update-check`

### `settings-persistence`: 앱 설정·창 크기/위치를 홈 디렉터리 JSON에 저장/복원

- **Where**: 화면 없음 (Settings Apply/OK, 창 blur, 닫기 버튼, 구성 pane 리사이즈 시 저장)
- **Aliases**: 설정이 저장 안 됨, 재시작하면 설정 초기화, 창 크기 기억, 창 위치 복원, settings not saved, window position reset
- **UI strings**: none (실패는 stderr `"[Settings] Failed to parse {}: {e}; using defaults"` 등)
- **Controls**: none
- **Code path**:
  - State / handler: `src/app.rs` → `save_app_settings` (호출처: `apply_settings`, `handle_close_window`, `WindowFocusChanged` blur 분기, `handle_configuration_pane_resized`)
  - Data: ~/.run_config_settings.json (`AppSettings`), written in `src/services/storage.rs` → `save_settings`, read in `src/services/storage.rs` → `load_settings`; 경로 `settings_path`; 창 배치 복원 `src/main.rs` → `restore_window_geometry`
- **Tests**: `src/services/storage.rs` → `app_settings_legacy_json_defaults_new_fields`, `app_settings_round_trip_preserves_fields`, `app_settings_mcp_fields_round_trip`; `src/main.rs` → `geometry_defaults_when_unset`, `geometry_clamps_size_to_minimum`, `geometry_keeps_negative_multi_monitor_position`, `geometry_rejects_nonfinite_and_out_of_range`
- **Pitfalls**: 설정 파일이 깨지면 조용히 기본값으로 시작 → 파싱 실패 시 `AppSettings::default()` 폴백, source: `src/services/storage.rs` → `load_settings`; 설정이 저장되지 않음 → 홈 디렉터리를 못 찾으면 CWD로 폴백하지 않고 저장을 건너뜀, source: `src/services/storage.rs` → `settings_path`
- **Shares**: `src/services/storage.rs` (구성 저장소 `configs.json`과 같은 모듈이지만 파일은 별개)
- **Related**: `settings-modal`, `window-chrome`

### `import-configurations`: 파일에서 구성을 골라 현재 목록에 병합 (같은 id는 교체)

- **Where**: Configurations 화면 → 툴바 Import → 파일 선택 다이얼로그 → Import Configurations 모달
- **Aliases**: 가져오기, 불러오기, 임포트, 파일 열기, 구성 가져오기, import, open file, 덮어쓰기, 교체됨
- **UI strings**: "Import", "Import Configurations", "From: {source_name}", "Select all", "{selected_count} / {total} selected", "replaces existing", "replaces '{current_name}'", "Compound '{}' members not selected: {}", "Cancel", "Importing configurations...", "No configurations found in {}", "Import: {}", "Import cancelled", "Imported {} configuration(s) ({replaced} replaced)", "Imported {added} configuration(s)"
- **Controls**: "Import" (`ICON_IMPORT`) → `Message::ImportConfigurations` / `handle_import_configurations`, 파일 로드 결과 → `Message::ImportFileLoaded` / `handle_import_file_loaded`, 행 체크박스 → `Message::ImportModalToggleConfig` / `handle_import_modal_toggle_config`, "Select all" → `Message::ImportModalToggleAll` / `handle_import_modal_toggle_all`, "Import" (모달) → `Message::ConfirmImportModal` / `handle_confirm_import_modal`, "Cancel" / `ICON_CLOSE` / Esc → `Message::CancelImportModal` / `handle_cancel_import_modal`
- **Code path**:
  - UI: `src/views/configuration_editor.rs` → `view_import_modal`, `view_import_modal_row`
  - State / handler: `src/app/import_modal.rs` → `ImportModalState`, `handle_import_file_loaded`, `handle_confirm_import_modal`; 상태 문구 `src/app.rs` → `cancellable_status`
  - Data: 파일 선택·파싱 `src/services/storage.rs` → `import_configurations` (`AsyncFileDialog`, JSON 필터, `parse_config_file`); 병합 결과는 메모리 목록만 바뀌고 저장은 `SaveConfigurations` 경로
- **Tests**: `src/app/import_modal.rs` → `file_loaded_selects_only_new_ids_by_default`, `file_loaded_dedupes_duplicate_ids_keeping_first`, `empty_file_shows_status_without_modal`, `load_error_sets_status`, `checking_compound_auto_selects_file_members_except_already_existing`, `confirm_appends_new_and_replaces_existing_in_place`, `confirm_invalidates_env_cache_for_replaced_config`, `confirm_selects_first_config_when_nothing_was_selected`, `confirm_with_empty_selection_closes_without_merge`, `cancel_discards_staging_and_sets_status`, `file_loaded_closes_other_open_modals`, `configurations_reload_closes_open_import_modal`
- **Pitfalls**: 이미 있는 구성이 체크 안 된 채로 뜸 → 같은 id(교체 대상)는 기본 미선택, 명시적으로 체크해야 교체, source: `src/app/import_modal.rs` → `handle_import_file_loaded`; 파일에 같은 id가 둘이면 하나만 가져와짐 → 첫 항목만 남김, source: `src/app/import_modal.rs` → `handle_import_file_loaded`; Compound를 체크해도 이미 있는 멤버는 체크되지 않음 → 기존 멤버는 자동 선택에서 제외, source: `src/app/import_modal.rs` → `handle_import_modal_toggle_config`
- **Shares**: 모달 공용 `src/views/configuration_editor.rs` → `modal_select_all_row`, `modal_list_scrollable`, `modal_footer`, `modal_dialog`, `compound_member_warnings`
- **Related**: `export-configurations`, `toolbar`

### `export-configurations`: 선택한 구성을 JSON 파일로 내보내기

- **Where**: Configurations 화면 → 툴바 Export → Export Configurations 모달 → 저장 다이얼로그
- **Aliases**: 내보내기, 익스포트, 백업, 공유용 파일, export, save as, 내보낼 게 없음
- **UI strings**: "Export", "Export Configurations", "Select all", "{selected_count} / {total} selected", "Compound '{}' members not selected: {}", "Cancel", "No configurations to export", "Exporting {} configuration(s)...", "Exported: {}", "Export cancelled"
- **Controls**: "Export" (`ICON_EXPORT`) → `Message::OpenExportModal` / `handle_open_export_modal`, 행 체크박스 → `Message::ExportModalToggleConfig` / `handle_export_modal_toggle_config`, "Select all" → `Message::ExportModalToggleAll` / `handle_export_modal_toggle_all`, "Export" (모달) → `Message::ConfirmExportModal` / `handle_confirm_export_modal`, 결과 → `Message::ConfigurationsExported` / `handle_configurations_exported`, "Cancel" / `ICON_CLOSE` / Esc → `Message::CancelExportModal` / `handle_cancel_export_modal`
- **Code path**:
  - UI: `src/views/configuration_editor.rs` → `view_export_modal`, `view_export_modal_row`
  - State / handler: `src/app/export_modal.rs` → `ExportModalState`, `handle_confirm_export_modal`
  - Data: 파일 쓰기 `src/services/storage.rs` → `export_configurations` → `save_configs_to` (기본 파일명 `configurations-export.json`, JSON 필터)
- **Tests**: `src/app/export_modal.rs` → `open_export_modal_selects_all_by_default`, `open_export_modal_without_configs_shows_status_and_stays_closed`, `toggle_config_unchecks_and_rechecks`, `checking_compound_auto_selects_existing_members_only`, `toggle_all_fills_and_clears_selection`, `confirm_with_empty_selection_closes_without_export_task`, `confirm_closes_modal_and_sets_exporting_status`, `configurations_reload_closes_open_export_modal`, `exported_result_updates_status`
- **Pitfalls**: Export 눌러도 모달이 안 뜸 → 구성이 0개면 상태바 메시지만, source: `src/app/export_modal.rs` → `handle_open_export_modal`; 모달 Export 버튼 비활성 → 선택 0개, source: `src/views/configuration_editor.rs` → `view_export_modal`; Compound만 내보내면 멤버 참조가 끊김 → 경고 문구로 알림, source: `src/views/configuration_editor.rs` → `compound_member_warnings`
- **Shares**: `import-configurations`와 같은 모달 공용 함수
- **Related**: `import-configurations`, `toolbar`

### `update-check`: GitHub 최신 릴리스 확인 (시작 시 자동, 상태바 버전 라벨 클릭 시 수동)

- **Where**: 하단 상태바 → 우측 버전 라벨
- **Aliases**: 업데이트 확인, 새 버전, 최신 버전, 버전 표시, 업데이트 알림, check for updates, version label, checking 계속 돎
- **UI strings**: "v{current}", "{frame} checking", "v{current} → v{latest} ⬆", "Checking for updates…", "Update available: v{latest}", "You're on the latest version (v{current})", "Update check failed: {error}", "Update available" (OS 알림)
- **Controls**: 버전 라벨 "v{current}" → `Message::CheckForUpdates` / `handle_check_for_updates`, 결과 → `Message::UpdateCheckCompleted` / `handle_update_check_completed`, 스피너 → `Message::UpdateSpinnerTick` / `handle_update_spinner_tick`
- **Code path**:
  - UI: `src/app.rs` → `view_status_bar_version`
  - State / handler: `src/app.rs` → `new` (`auto_check_updates`면 시작 시 `check_latest_release`), `handle_update_check_completed`, `notify_update_available`; Settings에서 새로 켤 때 `src/app/settings_modal.rs` → `apply_settings`
  - Data: 없음 (메모리 `update_available`); 네트워크 `src/services/update_check.rs` → `check_latest_release`, `select_download`, `platform_asset_name`, `is_newer`
- **Tests**: `src/services/update_check.rs` → `parses_with_and_without_prefix`, `parses_prerelease_patch`, `non_numeric_patch_returns_none`, `rejects_malformed`, `detects_newer_version`, `ignores_same_or_older`, `malformed_is_not_newer`, `selects_download_when_all_assets_present`, `missing_signature_or_checksums_disables_in_app_update`, `rejects_asset_urls_outside_github`, `platform_asset_name_maps_arch_aliases`
- **Pitfalls**: 확인 중 라벨 클릭이 안 먹음 → 확인 중엔 버튼 비활성·중복 요청 무시, source: `src/app.rs` → `handle_check_for_updates`; 새 버전인데 인앱 설치 대신 수동 모드 → 플랫폼 자산·`checksums.txt`·`.minisig` 중 하나라도 없거나 GitHub 밖 URL이면 `download = None`, source: `src/services/update_check.rs` → `select_download`; 업데이트 확인이 실패로 끝남 → 릴리스 URL이 GitHub prefix가 아니면 거절, source: `src/services/update_check.rs` → `check_latest_release`
- **Shares**: 상태바 `src/app.rs` → `view_status_bar`, 스타일 `src/app/chrome.rs` → `status_bar_version_style`
- **Related**: `update-install`, `settings-run-defaults`, `status-bar`

### `update-install`: 확인 모달 → 다운로드·서명/체크섬 검증 → 설치 → 재실행 (self-update)

- **Where**: 상태바 버전 라벨 "v{current} → v{latest} ⬆" → Update Available 모달
- **Aliases**: 업데이트 설치, 자동 업데이트, 업데이트 버튼 비활성, 재시작 안 됨, 릴리스 페이지, 서명 검증 실패, update, install update, relaunch, minisign, checksum mismatch
- **UI strings**: "Update Available", "Update from v{} to v{}?", "The app will restart automatically to finish the update.", "In-app install isn", "{} running session(s) — stop them to enable Update.", "Update", "Open Release Page", "Cancel", "updating…", "Downloading and verifying update…", "Stop running sessions before updating", "Update in progress — wait before running", "Update failed: {error}", "Update installed — stop sessions and restart the app to finish", "Update downloaded — stop sessions, then run the installer: {}", "Relaunch failed ({error}); run the installer manually: {}", "Update installed, but relaunch failed ({error}) — restart the app manually", "checksum mismatch for {} — aborting update"
- **Controls**: 버전 라벨(업데이트 있음) → `Message::RequestInstallUpdate` / `handle_request_install_update`, "Update" / "Open Release Page" → `Message::ConfirmInstallUpdate` / `handle_confirm_install_update`, "Cancel" / `ICON_CLOSE` / Esc → `Message::CancelInstallUpdate` / `handle_cancel_install_update`, 설치 결과 → `Message::UpdateInstallCompleted` / `handle_update_install_completed`
- **Code path**:
  - UI: `src/views/configuration_editor.rs` → `view_confirm_update_modal`
  - State / handler: `src/app/confirm_update_modal.rs` → `ConfirmUpdateModalState` (`manual`), `handle_confirm_install_update`; `src/app.rs` → `handle_install_update`, `handle_update_install_completed`, `execute_relaunch` (macOS `open`, Windows `msiexec`, Linux exe 직접 실행), `handle_open_url`
  - Data: 설치 대상 파일 교체 `src/services/self_update.rs` → `download_and_apply` (`verify_signature_with_key` + `MINISIGN_PUBLIC_KEY`, `parse_checksums`, `sha256_hex`, `apply_macos`/`apply_windows`/`apply_linux`, `swap_bundle_at`), 잔여물 정리 `cleanup_stale_update_artifacts` (시작 시 `src/app.rs` → `new`)
- **Tests**: `src/app/confirm_update_modal.rs` → `request_opens_modal_without_starting_install`, `confirm_starts_install_and_closes_modal`, `cancel_closes_modal_without_installing`, `request_opens_modal_even_while_sessions_running`, `confirm_refuses_if_session_started_while_modal_open`, `request_without_download_opens_manual_modal_not_browser`, `request_after_failed_install_opens_manual_modal`, `manual_confirm_opens_release_page_even_while_sessions_running`; `src/services/self_update.rs` → `signature_verifies_with_correct_key`, `signature_rejects_tampered_data`, `signature_rejects_wrong_key`, `signature_rejects_garbage`, `full_verification_chain_accepts_good_and_rejects_bad_payload`, `parses_sha256sum_output`, `accumulate_rejects_beyond_cap`, `rejects_path_traversal_entries`, `rejects_symlink_entries`, `rejects_archives_expanding_beyond_cap`, `swap_installs_new_and_keeps_backup`, `swap_rolls_back_when_install_fails`, `apply_macos_end_to_end_with_fake_bundle`, `replaces_binary_content_and_exec_bit`, `cleanup_removes_backups_and_staging_but_not_current`, `cleanup_never_touches_lookalike_names`
- **Pitfalls**: 업데이트 버튼을 눌러도 반응 없음 → 실행 중 세션이 있으면 모달 안에서 Update 비활성+사유 표시로 변경, source: `9afefd2`; 버전 라벨 오클릭 시 브라우저가 멋대로 열림 → 수동 모드도 확인 모달을 거치도록 변경, source: `57fa8c4`; 설치 실패 후 다시 누르면 릴리스 페이지로 감 → `update_install_failed`면 수동 모드, source: `src/app/confirm_update_modal.rs` → `handle_request_install_update`; 서명 키 교체 시 기존 설치본은 새 릴리스 검증 실패 → 공개키가 바이너리에 고정, source: `src/services/self_update.rs` → `MINISIGN_PUBLIC_KEY`
- **Shares**: 모달 공용 `src/views/configuration_editor.rs` → `modal_footer`, `modal_dialog`; 모달 단일화 `src/app.rs` → `close_all_modals`
- **Related**: `update-check`, `status-bar`

### `window-chrome`: 커스텀 타이틀바 (드래그 이동, 더블클릭 최대화, 닫기/최소화/최대화, 가장자리 리사이즈)

- **Where**: 창 최상단 타이틀바, 창 가장자리 5px 리사이즈 영역
- **Aliases**: 창 이동, 드래그 안 됨, 창 크기 조절, 최대화, 최소화, 닫기 버튼, 신호등 버튼, 터치 드래그 멈춤, title bar, resize, window freeze
- **UI strings**: "Run/Debug Configuration Manager", "✕", "—", "❐", "✢"
- **Controls**: "✕" → `Message::CloseWindow` / `handle_close_window`, "—" → `Message::MinimizeWindow` / `handle_minimize_window`, "❐"/"✢" 및 타이틀바 더블클릭 → `Message::ToggleWindowMaximize` / `handle_toggle_window_maximize`, 타이틀바 마우스 press → `Message::StartWindowDrag` / `handle_start_window_drag`, 터치 → `Message::TitleBarTouchDragStart` / `TitleBarTouchDragMove` / `TitleBarTouchDragEnd` (`handle_title_bar_touch_drag_start`, `handle_title_bar_touch_drag_move`), 가장자리 grip → `Message::ResizeWindow` / `handle_resize_window`
- **Code path**:
  - UI: `src/app.rs` → `view_window_title_bar`, `view_window_control_button`, `view_window_resize_grips`, `view_window_resize_grip`; 위젯 `src/widgets/title_bar_drag.rs` → `TitleBarDragArea` (`on_mouse_press`, `on_double`, `on_touch_start`, `on_touch_move`, `on_touch_end`)
  - State / handler: `src/app.rs` → `handle_window_maximized`, `WindowFocusChanged` 처리(blur 시 터치 드래그 종료)
  - Data: 닫기·blur 시 창 크기/위치 저장 (`settings-persistence`)
- **Tests**: `src/app.rs` → `title_bar_touch_drag_moves_window_by_finger_delta`, `title_bar_touch_drag_recurrence_is_stable_across_steps`, `title_bar_touch_drag_ignored_when_maximized`, `title_bar_touch_drag_move_bails_if_maximized_mid_drag`, `losing_focus_ends_stuck_touch_drag`, `window_moved_ignored_while_touch_dragging`
- **Pitfalls**: Windows 터치로 타이틀바를 끌면 앱이 멈춤 → OS 모달 이동 루프가 터치 종료를 못 받음, 터치는 `window::move_to`로 직접 이동, source: `b35cdde`; macOS에서 가장자리 커서만 바뀌고 리사이즈 안 됨 → macOS는 grip을 깔지 않고 네이티브 리사이즈 사용, source: `e56c03c`; 최대화 상태에서 드래그·리사이즈가 안 됨 → 최대화 중에는 두 핸들러가 무시, source: `src/app.rs` → `handle_resize_window`; 창을 닫으면 실행 중 프로그램이 확인 없이 종료됨 → 닫기가 실행 중 세션의 cancel 플래그를 바로 세움, source: `src/app.rs` → `prepare_running_sessions_for_shutdown`
- **Shares**: 창 스타일 `src/app/chrome.rs` → `window_chrome_style`, `window_border_overlay_style`, `window_radius`
- **Related**: `main-view-tabs`, `settings-persistence`

### `status-bar`: 하단 상태바 한 줄 메시지와 버전 라벨

- **Where**: 창 최하단 상태바 (좌측 메시지, 우측 버전 라벨)
- **Aliases**: 상태 메시지, 하단 메시지, 에러 메시지 어디, 알림 줄, status bar, status message
- **UI strings**: 핸들러별 `status_message` 문자열 (예: "Import cancelled", "Exported: {}", "Update available: v{latest}"), "v{current}"
- **Controls**: 버전 라벨만 클릭 가능 (`update-check`, `update-install`); 메시지 영역은 클릭/복사/닫기 없음
- **Code path**:
  - UI: `src/app.rs` → `view_status_bar`, `view_status_bar_version`
  - State / handler: 각 핸들러가 `self.status_message`를 덮어씀; 취소/실패 문구 공통 `src/app.rs` → `cancellable_status`
  - Data: 없음 (메모리만, 히스토리 없음)
- **Tests**: 개별 기능 테스트가 `status_message`를 단언 (예: `src/app/export_modal.rs` → `exported_result_updates_status`, `src/app/import_modal.rs` → `load_error_sets_status`)
- **Pitfalls**: 메시지가 금방 다른 문구로 바뀜 → 단일 문자열을 마지막 핸들러가 덮어씀, source: `src/app.rs` → `view_status_bar`
- **Shares**: `src/app/chrome.rs` → `status_bar_style`, `status_bar_top_border_style`, `status_bar_version_style`
- **Related**: `update-check`, `update-install`

### `main-view-tabs`: 타이틀바 우측 Configurations / Sessions 화면 전환 탭

- **Where**: 타이틀바 → 우측 탭 2개 (Sessions 탭에 실행 중 점 표시)
- **Aliases**: 화면 전환, 세션 화면, 구성 화면, 탭 전환, F1, F2, switch view, sessions tab, 빨간 점
- **UI strings**: "Configurations", "Sessions"
- **Controls**: "Configurations" (`ICON_CONFIGURATIONS`) / F1 → `Message::SwitchView(ViewMode::Configuration)`, "Sessions" (`ICON_SESSIONS`) / F2 → `Message::SwitchView(ViewMode::Sessions)`; 처리 `src/app.rs` → `update` 내 `Message::SwitchView` arm, 단축키 `nav_shortcut_message`
- **Code path**:
  - UI: `src/views/main_tabs.rs` → `view_main_tabs`, `view_main_tab_button`, `main_tab_icon`, `live_session_dot`; 배치 `src/app.rs` → `view_window_title_bar`
  - State / handler: `src/app.rs` → `current_view`; 실행 시 자동 전환 `handle_run_configuration` (`ViewMode::Sessions`)
  - Data: 없음 (현재 화면은 저장 안 됨)
- **Tests**: `src/app.rs` → `nav_shortcut_f1_f2_switch_views`, `jump_to_workspace_switches_view_and_selects_tab`
- **Pitfalls**: Sessions 탭의 점이 세션 종료 후에도 남음 → 점은 "실행 중"이 아니라 세션이 하나라도 있으면 표시 (`!self.sessions.is_empty()`), source: `src/app.rs` → `view_window_title_bar`; Cmd+F1이 안 먹음 → F1/F2는 수식키 없을 때만, source: `src/app.rs` → `nav_shortcut_message`
- **Shares**: none
- **Related**: `window-chrome`

### `toolbar`: Configurations 목록 상단 아이콘 툴바 (Add / Import / Export / Save / Settings)

- **Where**: Configurations 화면 → 좌측 구성 목록 pane 헤더
- **Aliases**: 툴바, 상단 버튼, 추가 버튼, 저장 버튼, 설정 버튼, toolbar
- **UI strings**: "Add", "Import", "Export", "Save", "Settings"
- **Controls**: "Add" (`ICON_ADD`) → `Message::AddConfiguration`, "Import" (`ICON_IMPORT`) → `Message::ImportConfigurations`, "Export" (`ICON_EXPORT`) → `Message::OpenExportModal`, "Save" (`ICON_SAVE`) / Cmd+S → `Message::SaveConfigurations`, "Settings" (`ICON_SETTINGS`) / Cmd+, → `Message::OpenSettingsModal`
- **Code path**:
  - UI: `src/views/toolbar.rs` → `view_toolbar`, `toolbar_button`; 배치 `src/app.rs` → `view` (`ConfigurationScreenPane::Configurations`)
  - State / handler: `src/app.rs` → `handle_add_configuration`, `handle_save_configurations`; 나머지는 각 모달 항목
  - Data: none
- **Tests**: `src/app.rs` → `nav_shortcut_cmd_letters_map_to_actions`
- **Pitfalls**: Sessions 화면에서는 툴바가 안 보임 → Configurations pane 헤더에만 배치, source: `src/app.rs` → `view`
- **Shares**: none
- **Related**: `import-configurations`, `export-configurations`, `settings-modal`

### 없는 기능 (expected-but-absent)

- Settings 모달에 "Reset to defaults" 버튼 없음 → nearest: `settings-modal`
- Settings 모달에 "Check for updates now" 버튼 없음, 수동 확인은 상태바 버전 라벨 클릭뿐 → nearest: `update-check`
- 상태바 메시지 복사/닫기/히스토리 없음 → nearest: `status-bar`
- 창 닫기 시 실행 중 세션 확인 대화상자 없음 → nearest: `window-chrome`
