# Configurations

### `config-list-selection`: 사이드바 구성 목록 표시와 클릭 선택

- **Where**: Configurations 화면 → 좌측 "Configurations" pane 목록
- **Aliases**: 구성 목록, 사이드바, 리스트, 선택이 안 돼, 이름 잘림, 툴팁, 타입 아이콘, config list, sidebar, select
- **UI strings**: "Configurations", "Configuration Details", "Select a configuration from the list to edit its command,"
- **Controls**: 항목 클릭 → `Message::StartConfigurationDrag` / `handle_start_configuration_drag` (선택 + 드래그 대기), 이름 hover → `tooltip` (`view_name_tooltip`), 타입 뱃지 `ICON_TYPE_APPLICATION` / `ICON_TYPE_SHELL` / `ICON_TYPE_NODE` / `ICON_TYPE_KOTLIN` / `ICON_TYPE_SPRING_BOOT` / `ICON_TYPE_COMPOUND`
- **Code path**:
  - UI: `src/views/configuration_list.rs` → `view_configuration_list`, `src/widgets/configuration_list.rs` → `configuration_list`, `view_configuration_item`, `view_configuration_summary`, `type_icon`; pane 조립 `src/app.rs` → `view_configuration_screen`
  - State / handler: `src/app.rs` → `select_configuration`, `sync_editor_select_state_for_selected_config`
  - Data: `selected_config_index`는 메모리만 (저장 안 됨)
- **Tests**: `src/utils.rs` → `truncate_text_keeps_short_strings`, `truncate_text_appends_ellipsis_reserving_three_columns`, `truncate_text_accounts_for_full_width_columns`
- **Pitfalls**: 툴팁이 이름 시작점과 어긋남 → iced tooltip이 중앙 정렬만 지원해 폭을 맞춤, source: `src/widgets/configuration_list.rs` → `view_name_tooltip`
- **Shares**: `src/utils.rs` → `truncate_text`, `monospace_char_width`; `view_name_tooltip`는 세션 목록과 공유
- **Related**: `config-drag-reorder`, `config-editor-basics`
- 부재: 목록 검색/필터 입력 없음, 키보드 ↑/↓로 구성 선택 없음 (ArrowUp/Down은 stdin 히스토리 전용)

### `config-add`: 새 구성 추가

- **Where**: Configurations pane 헤더 툴바
- **Aliases**: 추가, 새 구성, 플러스 버튼, add, new configuration
- **UI strings**: "Add", "New configuration added", "New Configuration"
- **Controls**: `ICON_ADD` (tooltip "Add") → `Message::AddConfiguration` / `handle_add_configuration`
- **Code path**:
  - UI: `src/views/toolbar.rs` → `view_toolbar`
  - State / handler: `src/app.rs` → `handle_add_configuration`
  - Data: `RunConfiguration::default()` (`src/models/configuration.rs`), Save 전까지 메모리만
- **Tests**: none
- **Pitfalls**: none
- **Shares**: `src/views/toolbar.rs` → `toolbar_button`
- **Related**: `config-save-load`

### `config-clone`: 구성 복제

- **Where**: 목록 항목 우측 액션 버튼
- **Aliases**: 복제, 복사, 클론, duplicate config, clone, copy
- **UI strings**: "Configuration cloned", "{base_name} (copy)", "{base_name} (copy {suffix})"
- **Controls**: `ICON_COPY` → `Message::CloneConfiguration(Some(index))` / `handle_clone_configuration`
- **Code path**:
  - UI: `src/widgets/configuration_list.rs` → `view_configuration_item`, `action_button`
  - State / handler: `src/app.rs` → `handle_clone_configuration`, `unique_clone_name`; 후보 캐시 `src/app/detected_candidates.rs` → `DetectedCandidates::copy`
  - Data: 원본 바로 아래 index에 새 UUID로 삽입, Save 전까지 메모리만
- **Tests**: `src/app.rs` → `unique_clone_name_appends_copy_then_escalates`, `unique_clone_name_of_a_copy_does_not_collide`; `src/app/detected_candidates.rs` → `copy_gives_the_clone_every_kind_without_touching_the_source`
- **Pitfalls**: 복제본에 Node scripts·감지 후보가 비어 보임 → 원본의 캐시(`node_available_scripts` 등)를 복사하지 않으면 생기며 `handle_clone_configuration`이 이를 복사함, source: `src/app.rs` → `handle_clone_configuration`
- **Shares**: `src/app/detected_candidates.rs`
- **Related**: `config-list-selection`

### `config-delete-confirm`: 확인 모달을 거치는 구성 삭제

- **Where**: 목록 항목 우측 액션 버튼 → "Delete Configuration" 모달
- **Aliases**: 삭제, 지우기, 휴지통, 삭제 확인, delete, remove, confirm delete
- **UI strings**: "Delete Configuration", "This cannot be undone.", "Referenced by compound: {} — the reference will be removed.", "Delete", "Cancel", "Configuration deleted"
- **Controls**: `ICON_DELETE` → `Message::RequestDeleteConfiguration(Some(index))` / `handle_request_delete_configuration`; "Delete" → `Message::ConfirmDeleteConfiguration` / `handle_confirm_delete_configuration`; "Cancel" / `ICON_CLOSE` / Esc → `Message::CancelDeleteConfiguration` / `handle_cancel_delete_configuration`
- **Code path**:
  - UI: `src/views/configuration_editor.rs` → `view_confirm_delete_modal`, `modal_footer`, `modal_dialog`; 레이어 `src/app.rs` → `view` (`view_confirm_delete_modal(props)`)
  - State / handler: `src/app/confirm_delete_modal.rs` → `ConfirmDeleteModalState`, `handle_request_delete_configuration`, `handle_confirm_delete_configuration`; 실제 삭제 `src/app.rs` → `handle_delete_configuration`; Esc `src/app.rs` → `subscription` (`confirm_delete_keyboard_subscription`)
  - Data: 메모리 목록에서 제거, Save 전까지 파일 불변
- **Tests**: `src/app/confirm_delete_modal.rs` → `request_opens_modal_and_does_not_delete`, `confirm_deletes_and_cancel_keeps`, `request_without_target_is_noop`; `src/app.rs` → `delete_middle_keeps_index_pointing_to_next`, `delete_last_clamps_selection`, `delete_only_config_clears_selection`, `delete_strips_member_from_compound`
- **Pitfalls**: 삭제 후 Compound 멤버가 사라짐 → 삭제가 모든 Compound에서 그 id를 제거, source: `src/app.rs` → `handle_delete_configuration`
- **Shares**: 모달 공통 `src/app.rs` → `close_all_modals`, `any_modal_open`; `src/views/configuration_editor.rs` → `modal_footer`
- **Related**: `type-compound`
- 부재: 삭제 undo 없음

### `config-drag-reorder`: 드래그로 구성 순서 변경

- **Where**: 좌측 목록 항목 간 드래그
- **Aliases**: 순서 변경, 드래그, 끌어서 옮기기, 정렬, reorder, drag and drop
- **UI strings**: "Dragging configuration...", "Configuration reordered", "Configuration drag canceled"
- **Controls**: 항목 press → `Message::StartConfigurationDrag` / `handle_start_configuration_drag`; 이동 → `Message::CursorMoved` / `handle_cursor_moved`; hover → `Message::ConfigurationDragHovered` / `handle_configuration_drag_hovered`; 이탈 → `Message::ConfigurationDragExited` / `handle_configuration_drag_exited`; 놓기 → `Message::MouseReleased` / `handle_mouse_released`
- **Code path**:
  - UI: `src/widgets/configuration_list.rs` → `view_configuration_item`, `view_trailing_drop_target`, `drop_preview_position`, `drop_gap`
  - State / handler: `src/app.rs` → `handle_cursor_moved`, `handle_mouse_released`, `configuration_drop_index`, `move_configuration_to`
  - Data: `configurations` Vec 순서, Save 시 파일 순서로 기록
- **Tests**: `src/app.rs` → `configuration_drop_index_self_and_adjacent_are_noops`, `configuration_drop_index_maps_position_to_insertion`, `move_configuration_down_reorders_and_tracks_selection`, `move_configuration_up_reorders`, `move_configuration_noop_when_from_equals_to`, `move_configuration_shifts_selection_when_crossing`
- **Pitfalls**: 클릭만 해도 드래그로 잡힘 → 6px 임계값 전에는 pending 상태, source: `src/app.rs` → `handle_cursor_moved` (`CONFIGURATION_DRAG_THRESHOLD`)
- **Shares**: drag 상태 struct (`src/app.rs` 의 `drag` 필드, `pending_configuration_origin`)
- **Related**: `config-list-selection`

### `config-run-button`: 목록에서 구성 실행

- **Where**: 목록 항목 우측 액션 버튼, 단축키 Cmd+R
- **Aliases**: 실행, 재생 버튼, 플레이, 런, run, play, start
- **UI strings**: "Update in progress — wait before running"
- **Controls**: `ICON_PLAY` → `Message::RunConfiguration(Some(index))` / `handle_run_configuration`; Cmd+R → `Message::RunShortcut` / `handle_run_shortcut`
- **Code path**:
  - UI: `src/widgets/configuration_list.rs` → `view_configuration_item`
  - State / handler: `src/app.rs` → `handle_run_configuration`, `new_session`, `run_compound`; 단축키 `src/app.rs` → `nav_shortcut_message`
  - Data: 세션은 메모리만. env는 `src/services/executor.rs` → `effective_extra_env`가 활성 항목만 사용
- **Tests**: `src/app.rs` → `nav_shortcut_cmd_letters_map_to_actions`, `run_compound_fans_out_to_member_sessions`
- **Pitfalls**: 실행이 안 되고 상태바에 경고만 → 인앱 업데이트 중 실행 차단, source: `src/app.rs` → `handle_run_configuration` (`is_updating`)
- **Shares**: 세션 실행부는 Sessions 화면 소관 (`src/services/executor.rs`)
- **Related**: `type-compound`

### `config-editor-basics`: 에디터 Basics 섹션 (이름, 타입 전환, 작업 디렉터리)

- **Where**: 우측 "Editor" pane → "Basics" 섹션
- **Aliases**: 이름 변경, 타입 바꾸기, 작업 디렉터리, 폴더 선택, cwd, rename, type switch, working directory, browse
- **UI strings**: "Basics", "Name:", "Configuration name", "Type:", "Select type", "Working Directory:", "Working directory path", "Opening folder dialog...", "Working directory set: {path}", "Application", "Shell Script", "Node", "Kotlin", "Spring Boot", "Compound"
- **Controls**: Name input → `Message::NameChanged` / `handle_name_changed`; Type combo → `Message::TypeChanged` / `handle_type_changed`; Working dir input → `Message::WorkingDirectoryChanged` / `handle_working_directory_changed`; `ICON_FOLDER_OPEN` → `Message::BrowseWorkingDirectory` / `handle_browse_working_directory` → `Message::WorkingDirectorySelected` / `handle_working_directory_selected`; Tab → `Message::MoveEditorFocus` / `handle_move_editor_focus`
- **Code path**:
  - UI: `src/views/configuration_editor.rs` → `view_configuration_editor`, `view_name_row`, `view_type_row`, `view_working_directory_row`, `view_empty_configuration_editor`
  - State / handler: `src/app.rs` → `handle_name_changed`, `handle_type_changed`, `handle_working_directory_changed`, `handle_working_directory_selected`, `handle_editor_focus_area_changed`
  - Data: `RunConfiguration.name` / `type_data` / `working_directory` (`src/models/configuration.rs`), Save 시 기록
- **Tests**: `src/app.rs` → `switching_to_spring_boot_starts_from_auto_with_empty_fields`, `browsing_to_another_directory_or_switching_type_forgets_detected_modules`; `src/models/configuration.rs` → `configuration_type_all_includes_kotlin`, `configuration_type_all_includes_spring_boot`
- **Pitfalls**: 타입을 바꿨다 되돌리면 입력값이 사라짐 → `handle_type_changed`가 `type_data`를 새 기본값으로 교체, source: `src/app.rs` → `handle_type_changed`; Tab이 콤보박스로 안 감 → combo_box는 focus traversal 대상 아님, source: `src/app.rs` → `handle_move_editor_focus`; Working Directory 행이 안 보임 → Node·Compound에선 숨김, source: `src/views/configuration_editor.rs` → `view_configuration_editor`
- **Shares**: `src/views/configuration_editor.rs` → `view_editor_row`, `editor_input`, `editor_combo_box`, `EditorSelectState`; `src/app.rs` → `cancellable_status`
- **Related**: `type-application`, `type-shell-script`, `type-node`, `type-kotlin`, `type-spring-boot`, `type-compound`

### `type-application`: Application 타입 필드 (명령, 인자)

- **Where**: Editor → Basics → Type "Application"
- **Aliases**: 명령어, 커맨드, 인자, 아규먼트, command, arguments, application
- **UI strings**: "Command:", "Command to execute", "Arguments:", "Program arguments"
- **Controls**: Command input → `Message::CommandChanged` / `handle_command_changed`; Arguments input → `Message::ArgumentsChanged` / `handle_arguments_changed`
- **Code path**:
  - UI: `src/views/configuration_editor.rs` → `view_application_fields`
  - State / handler: `src/app.rs` → `handle_command_changed`, `handle_arguments_changed`
  - Data: `ConfigTypeData::Application { command, arguments }`
- **Tests**: none
- **Pitfalls**: none
- **Shares**: `view_editor_row`
- **Related**: `config-editor-basics`

### `type-shell-script`: Shell Script 타입 (실행 모드, 스크립트 파일, 인라인 스크립트 에디터)

- **Where**: Editor → Basics → Type "Shell Script"
- **Aliases**: 셸 스크립트, 쉘, 스크립트 파일, 인라인 스크립트, 인터프리터, bash, sh, script text, execute mode, interpreter
- **UI strings**: "Execute Mode:", "Select execute mode", "Script File", "Script Text", "Script Path:", "Script file path", "Script Options:", "Options (e.g., -v --release)", "Interpreter:", "Interpreter path (optional)", "Interpreter Options:", "Interpreter options (optional)", "Script Text:", "Enter script here..."
- **Controls**: Execute Mode combo → `Message::ExecuteModeChanged` / `handle_execute_mode_changed`; Script Path → `Message::ScriptPathChanged`; `ICON_FOLDER_OPEN` → `Message::BrowseScriptPath` / `handle_browse_script_path` → `handle_script_path_selected`; Script Options → `Message::ScriptOptionsChanged`; Interpreter → `Message::InterpreterPathChanged`; `ICON_FOLDER_OPEN` → `Message::BrowseInterpreterPath` / `handle_interpreter_path_selected`; Interpreter Options → `Message::InterpreterOptionsChanged`; text_editor → `Message::ScriptTextEdited` / `handle_script_text_edited`
- **Code path**:
  - UI: `src/views/configuration_editor.rs` → `view_shell_script_fields`, `view_script_file_fields`, `view_script_text_fields`
  - State / handler: `src/app.rs` → `handle_execute_mode_changed`, `handle_script_text_edited`, `sync_script_editor`
  - Data: `ConfigTypeData::ShellScript { execute_mode }` (`ExecuteMode::ScriptFile` / `ExecuteMode::ScriptText`)
- **Tests**: `src/app.rs` → `script_editor_edit_roundtrips_newlines_and_preserves_buffer`, `script_editor_wiring_flows_through_update`, `script_editor_rebuilds_on_external_mutation_and_selection_change`, `script_editor_mode_wipe_rebuilds_empty`, `script_editor_drops_stale_action_after_selection_change`
- **Pitfalls**: 모드를 바꾸면 경로/스크립트가 비워짐 → `handle_execute_mode_changed`가 같은 모드 재선택도 새 값으로 교체, source: `src/app.rs` → `handle_execute_mode_changed`; 다른 구성 선택 후 편집이 엉뚱한 구성에 들어감 → synced id 불일치 시 action 폐기, source: `src/app.rs` → `handle_script_text_edited`; MCP update의 모드 필드 오타가 저장된 인터프리터 경로를 지우고 성공 응답 → 중첩 모드 열거형이 미지 필드를 무시했음(지금은 `deny_unknown_fields`로 거절), source: `b2edd9e`
- **Shares**: `src/app.rs` → `sync_script_editor` (update 말미 중앙 훅)
- **Related**: `config-editor-basics`

### `type-node`: Node 타입 (프로젝트 디렉터리, package.json 드롭다운, 런타임, 패키지 매니저, scripts 새로고침)

- **Where**: Editor → Basics → Type "Node"
- **Aliases**: 노드, npm, pnpm, yarn, bun, package.json, 스크립트 목록, 새로고침, 런타임, nvm, node runtime, package manager, refresh scripts
- **UI strings**: "Project Directory:", "Not selected", "package.json:", "Search package.json", "Command:", "Search command", "Package Manager:", "Search package manager", "Script:", "No scripts found", "Search script", "Select a package.json from the dropdown above", "Arguments:", "--flag value", "Node Runtime:", "Search Node runtime", "Node Options:", "--max-old-space-size=4096", "Opening project directory selection...", "Scanning package.json files...", "Found {} package.json file(s) in: {project_directory}", "No package.json found in: {project_directory}", "Selected package.json: {relative_path}", "Invalid package.json path: {relative_path}", "Auto", "npm", "pnpm", "yarn", "bun"
- **Controls**: `ICON_FOLDER_OPEN` → `Message::BrowseProjectDirectory` / `handle_browse_project_directory` → `Message::ProjectDirectorySelected` / `handle_project_directory_selected` → `Message::PackageJsonsScanned` / `handle_package_jsons_scanned`; package.json combo → `Message::PackageJsonDropdownChanged` / `handle_package_json_dropdown_changed`; Command combo → `Message::NodeCommandChanged` / `handle_node_command_changed`; Package Manager combo → `Message::PackageManagerChanged` / `handle_package_manager_changed`; Script combo → `Message::NodeScriptNameChanged` / `handle_node_script_name_changed`; `ICON_REFRESH` → `Message::NodeRefreshScripts` / `handle_node_refresh_scripts`; Node Runtime combo → `Message::NodeRuntimeChanged` / `handle_node_runtime_changed`; Arguments → `Message::NodeArgumentsChanged`; Node Options → `Message::NodeOptionsChanged`
- **Code path**:
  - UI: `src/views/configuration_editor.rs` → `view_node_fields`, `view_node_project_directory_row`, `view_node_package_json_row`, `view_node_working_directory_row`, `view_node_command_row`, `view_node_package_manager_row`, `view_node_script_row`, `view_node_script_hint_row`, `view_node_runtime_row`, `selected_package_json`
  - State / handler: `src/app.rs` → `load_node_metadata`, `load_node_package_jsons`, `load_node_scripts`, `scan_node_metadata`, `handle_node_metadata_loaded`, `handle_node_runtimes_detected`
  - Data: `ConfigTypeData::Node { project_directory, package_manager, node_runtime_path, command, script_name, arguments, node_options }` + `working_directory`(선택한 package.json 폴더). 스캔 캐시 `node_available_scripts`/`node_available_package_jsons`는 메모리만
  - Detection: `src/utils.rs` → `find_all_package_jsons`, `parse_scripts`, `detect_package_manager`, `detect_node_runtimes`, `to_relative_path`
- **Tests**: `src/utils.rs` → `to_relative_path_child_uses_forward_slashes`, `to_relative_path_self_returns_dot_package_json`, `to_relative_path_outside_returns_normalized_absolute`
- **Pitfalls**: Node 구성을 열면 창이 멈춤 → package.json 재귀 스캔이 UI 스레드에서 돌았음, source: `dbcfd88`; Script 행이 안 보임 → `command.requires_script()`가 false면 숨기고 `script_name`도 None으로 지움, source: `src/app.rs` → `handle_node_command_changed`; `npm run ""` 빈 인자 → 빈 문자열을 None으로 정규화, source: `src/app.rs` → `handle_node_script_name_changed`
- **Shares**: `src/views/configuration_editor.rs` → `EditorSelectState::set_node_runtimes`
- **Related**: `config-editor-basics`

### `type-kotlin`: Kotlin 타입 (실행 모드, main class/JAR 감지, JDK)

- **Where**: Editor → Basics → Type "Kotlin"
- **Aliases**: 코틀린, 메인 클래스, JAR, 자동 감지, 클래스패스, JDK 선택, VM 옵션, main class detect, classpath, jdk
- **UI strings**: "Launch Mode:", "Select launch mode", "Main class", "JAR", "Main Class:", "com.example.MainKt", "Search main class", "The refresh button scans .kt/.java sources for top-level main functions.", "Classpath:", "build/libs/*:libs/*", "Dependencies are not detected - list compiled classes and dependency jars here.", "JAR Path:", "Path to .jar", "Search JAR", "VM Options:", "-Xmx2g -Dkey=value", "Program Args:", "arg1 arg2", "JDK:", "Search JDK", "Scanning sources for main functions...", "Scanning build outputs for JAR files...", "No main function found under {root}", "No JAR found in build/libs or target under {root}", "Found {noun} {only}", "Found {} candidates - pick one from the list", "Scan discarded because the configuration changed"
- **Controls**: Launch Mode combo → `Message::KotlinLaunchModeChanged` / `handle_kotlin_launch_mode_changed`; Main Class → `Message::KotlinMainClassChanged`; `ICON_REFRESH` (main class) → `Message::DetectKotlinMainClasses` / `handle_detect_kotlin_candidates` → `Message::KotlinMainClassesScanned` / `handle_kotlin_candidates_scanned`; Classpath → `Message::KotlinClasspathChanged`; JAR Path → `Message::KotlinJarPathChanged`; `ICON_FOLDER_OPEN` → `Message::BrowseKotlinJarPath` / `handle_kotlin_jar_path_selected`; `ICON_REFRESH` (JAR) → `Message::DetectKotlinJars` → `Message::KotlinJarsScanned`; VM Options → `Message::KotlinVmOptionsChanged`; Program Args → `Message::KotlinProgramArgumentsChanged`; JDK combo → `Message::KotlinJdkChanged` / `handle_kotlin_jdk_changed`; `ICON_FOLDER_OPEN` (JDK) → `Message::BrowseKotlinJdk` / `handle_kotlin_jdk_path_selected`
- **Code path**:
  - UI: `src/views/configuration_editor.rs` → `view_kotlin_fields`, `view_kotlin_launch_mode_row`, `view_kotlin_main_class_row`, `view_kotlin_classpath_row`, `view_kotlin_jar_path_row`, `view_kotlin_jdk_row`, `candidate_picker`, `detect_button`
  - State / handler: `src/app.rs` → `handle_detect_kotlin_candidates`, `handle_kotlin_candidates_scanned`, `kotlin_mode_matches`, `scan_kotlin_candidates_task`, `handle_jdks_detected`; 캐시 `src/app/detected_candidates.rs` → `DetectedCandidates`
  - Data: `ConfigTypeData::Kotlin { jdk_path, launch_mode, vm_options, program_arguments }`; 감지 후보는 메모리만
  - Detection: `src/utils.rs` → `scan_kotlin_main_classes`, `scan_jar_artifacts`, `detect_jdks`
- **Tests**: `src/app.rs` → `a_single_detected_main_class_fills_an_empty_field`, `detected_main_classes_never_overwrite_a_typed_value_and_several_only_fill_the_list`, `a_single_detected_jar_fills_the_jar_path_in_jar_mode`, `a_scan_result_for_the_other_launch_mode_is_discarded`, `a_kotlin_scan_result_for_another_directory_or_a_deleted_configuration_is_discarded`, `a_failed_kotlin_scan_reports_its_reason`, `kotlin_detection_only_starts_for_the_matching_mode_and_never_twice`, `kotlin_candidate_kinds_do_not_leak_into_each_other`, `changing_the_directory_forgets_kotlin_candidates_too`, `the_background_kotlin_scan_validates_the_directory_and_scans_by_kind`; `src/utils.rs` → `main_class_of_a_top_level_kotlin_main_uses_the_file_class_name`, `main_class_skips_private_indented_and_lookalike_mains`, `jar_scan_finds_gradle_and_maven_outputs_at_any_module_depth`, `jar_scan_skips_auxiliary_artifacts_and_other_files`, `detect_jdks_always_includes_system_default`; `src/models/configuration.rs` → `kotlin_config_type_data_round_trips`, `kotlin_jar_launch_mode_round_trips`, `kotlin_launch_mode_defaults_to_main_class`
- **Pitfalls**: `@JvmStatic main`이나 .kts가 감지 안 됨 → 문서화된 한계 (hint 문구), source: `src/views/configuration_editor.rs` → `view_kotlin_main_class_row`; 감지 결과가 다른 구성/디렉터리로 들어감 → stale 결과 폐기, source: `src/app.rs` → `handle_kotlin_candidates_scanned`
- **Shares**: `src/app/detected_candidates.rs`; `candidate_picker`/`detect_button`은 Spring Boot와 공유; `EditorSelectState::set_jdks`
- **Related**: `type-spring-boot`

### `type-spring-boot`: Spring Boot 타입 (빌드 도구, 모듈 감지, 프로파일, JAR, JDK)

- **Where**: Editor → Basics → Type "Spring Boot"
- **Aliases**: 스프링 부트, 그레이들, 메이븐, 모듈, 프로파일, active profile, bootRun, build tool, module detect, profiles
- **UI strings**: "Build Tool:", "Select build tool", "Auto", "Gradle", "Maven", "JAR", "Module:", ":app", "Search module", "Maven -pl value, e.g. app/api or :artifactId.", "Gradle project path, e.g. :app or :services:api.", "Gradle project path (e.g. :app) or Maven -pl value (e.g. app/api).", "Leave empty to run the working directory's project. Use the refresh", "Profiles:", "dev,local", "JAR Path:", "Path to .jar", "VM Options:", "Gradle: passed via JAVA_TOOL_OPTIONS. A running Gradle daemon may ignore it,", "Program Args:", "JDK:", "Search JDK", "Scanning for @SpringBootApplication modules...", "No @SpringBootApplication module found under {root}", "Found bootable module {only}", "Module set to {only}", "Modules are not used with the JAR build tool", "Module scan discarded because the configuration changed"
- **Controls**: Build Tool combo → `Message::SpringBootBuildToolChanged` / `handle_spring_boot_build_tool_changed`; Module → `Message::SpringBootModuleChanged`; `ICON_REFRESH` → `Message::DetectSpringBootModules` / `handle_detect_spring_boot_modules` → `Message::SpringBootModulesScanned` / `handle_spring_boot_modules_scanned`; Profiles → `Message::SpringBootProfilesChanged`; JAR Path → `Message::SpringBootJarPathChanged`; `ICON_FOLDER_OPEN` → `Message::BrowseSpringBootJarPath` / `handle_spring_boot_jar_path_selected`; VM Options → `Message::SpringBootVmOptionsChanged`; Program Args → `Message::SpringBootProgramArgumentsChanged`; JDK combo → `Message::SpringBootJdkChanged` / `handle_spring_boot_jdk_changed`; `ICON_FOLDER_OPEN` (JDK) → `Message::BrowseSpringBootJdk` / `handle_spring_boot_jdk_path_selected`
- **Code path**:
  - UI: `src/views/configuration_editor.rs` → `view_spring_boot_fields`, `view_spring_boot_build_tool_row`, `view_spring_boot_module_row`, `view_spring_boot_profiles_row`, `view_spring_boot_jar_path_row`, `view_spring_boot_vm_options_row`, `view_spring_boot_jdk_row`
  - State / handler: `src/app.rs` → `handle_detect_spring_boot_modules`, `handle_spring_boot_modules_scanned`, `scan_spring_boot_modules_task`, `forget_detected_candidates_of_selected_config`
  - Data: `ConfigTypeData::SpringBoot { build_tool, module, jar_path, profiles, jdk_path, vm_options, program_arguments }`; 검증 `src/models/configuration.rs` → `is_valid_spring_profiles`, `is_valid_spring_module` (실행 시 `src/services/executor.rs` → `validate_profiles`, `checked_spring_module`)
  - Detection: `src/utils.rs` → `scan_spring_boot_modules`
- **Tests**: `src/app.rs` → `switching_to_spring_boot_starts_from_auto_with_empty_fields`, `spring_boot_text_handlers_write_their_own_field`, `changing_the_build_tool_keeps_the_jar_path`, `changing_the_build_tool_keeps_the_module`, `a_single_detected_module_fills_an_empty_module`, `a_single_detected_module_does_not_overwrite_a_typed_value`, `several_detected_modules_only_fill_the_list`, `a_truncated_scan_says_so`, `a_scan_result_for_another_directory_is_discarded`, `a_scan_result_in_the_wrong_notation_for_the_current_tool_is_discarded`, `detecting_a_jar_configuration_is_refused_before_scanning`, `detecting_is_not_started_twice`, `spring_boot_jdk_changed_maps_java_to_none_and_resolves_labels`; `src/utils.rs` → `spring_scan_finds_only_modules_with_a_boot_application`, `spring_scan_reports_a_root_app_as_the_empty_module`, `spring_scan_does_not_follow_symlinks`; `src/models/configuration.rs` → `spring_profiles_accept_names_lists_and_empty_only`, `module_accepts_gradle_paths_maven_paths_and_empty_only`, `spring_boot_rejects_unknown_fields`
- **Pitfalls**: Module 행이 사라짐 → JAR 도구에선 숨김(값은 유지), JAR Path는 JAR일 때만 표시, source: `src/views/configuration_editor.rs` → `view_spring_boot_fields`; 빌드 도구 변경 후 모듈 후보가 사라짐 → 표기(`:a:b` / `a/b`)가 도구에 묶여 후보 폐기, source: `src/app.rs` → `handle_spring_boot_build_tool_changed`; VM 옵션이 Gradle에서 무시됨 → JAVA_TOOL_OPTIONS + 데몬, source: `src/views/configuration_editor.rs` → `view_spring_boot_vm_options_row`; 잘못된 프로파일 문자 → 실행 시 거부, source: `src/services/executor.rs` → `validate_profiles`
- **Shares**: `src/app/detected_candidates.rs`; `candidate_picker`, `detect_button`, `hint_text`
- **Related**: `type-kotlin`

### `type-compound`: Compound 타입 (멤버 토글, 워크스페이스 탭)

- **Where**: Editor → Basics → Type "Compound"
- **Aliases**: 컴파운드, 묶음 실행, 동시 실행, 그룹, 멤버, multi run, compound members, workspace
- **UI strings**: "Tasks:", "Create other configurations first, then add them here.", "Workspace:", "(optional) run in a workspace tab named…", "{member_count} selected — click to add/remove. Members run together when this compound runs.", "Compound '{compound_name}' has no runnable members", "Running compound '{compound_name}': {launched} task(s)"
- **Controls**: 멤버 행 버튼 ("✓"/"+") → `Message::CompoundMemberAdded` / `handle_compound_member_added`, `Message::CompoundMemberRemoved` / `handle_compound_member_removed`; Workspace input → `Message::CompoundWorkspaceChanged` / `handle_compound_workspace_changed`
- **Code path**:
  - UI: `src/views/configuration_editor.rs` → `view_compound_fields`, `compound_member_row_style`
  - State / handler: `src/app.rs` → `handle_compound_member_added`, `run_compound`, `unique_workspace_name`
  - Data: `ConfigTypeData::Compound { members: Vec<Uuid>, workspace: Option<String> }`
- **Tests**: `src/app.rs` → `compound_member_add_dedups_and_excludes_self`, `run_compound_fans_out_to_member_sessions`, `run_compound_skips_missing_and_nested_members`, `run_compound_with_workspace_opens_new_named_tab`, `unique_workspace_name_keeps_base_then_numbers_on_collision`, `delete_strips_member_from_compound`
- **Pitfalls**: 다른 Compound가 멤버 목록에 없음 → 자기 자신·Compound 제외(중첩 방지), source: `src/views/configuration_editor.rs` → `view_compound_fields`, `src/app.rs` → `run_compound`; Environment/Working Directory 섹션이 없음 → 멤버가 각자 보유, source: `src/views/configuration_editor.rs` → `view_configuration_editor`
- **Shares**: none
- **Related**: `config-delete-confirm`, `config-run-button`
- 부재: 멤버 실행 순서·지연 설정 없음 (동시 실행만)

### `env-main-input`: Environment 섹션 한 줄 입력

- **Where**: Editor → "Environment" 섹션
- **Aliases**: 환경변수, env, 환경 변수 입력, 엔터 안 눌러도, KEY=value, environment variables
- **UI strings**: "Environment", "KEY1=value1;KEY2=value2;...", "A key here is disabled in the editor; enable it there to change it.", "{disabled_count} disabled variable(s) not shown; open the editor to manage them."
- **Controls**: input → `Message::EnvBulkInputChanged` / `handle_env_bulk_input_changed`; Enter → `Message::EnvBulkInputSubmitted` / `handle_env_bulk_input_submitted`; `ICON_EDIT` → `Message::OpenEnvModal` / `handle_open_env_modal`
- **Code path**:
  - UI: `src/views/configuration_editor.rs` → `view_environment_bulk_input`
  - State / handler: `src/app/env_modal.rs` → `handle_env_bulk_input_changed`, `handle_env_bulk_input_submitted`, `active_env_text`
  - Data: `RunConfiguration.environment_variables` (`EnvVars`), 표시 버퍼 `env_bulk_inputs` (메모리). 파싱/직렬화 `src/env_string.rs` → `parse_env_string`, `serialize_env_pairs`; 병합 `src/models/env_vars.rs` → `EnvVars::replace_active`, `collides_with_disabled`
- **Tests**: `src/app/env_modal.rs` → `bulk_input_applied_immediately_without_submit`, `bulk_input_edit_keeps_disabled_entries`, `submit_keeps_a_draft_that_collides_with_a_disabled_key`; `src/models/env_vars.rs` → `replace_active_keeps_disabled_entries_in_place`, `replace_active_never_overwrites_a_disabled_entry`; `src/env_string.rs` → `parses_basic_pairs`, `handles_escaped_semicolon_in_value`, `supports_newline_separator`, `round_trip_preserves_values`
- **Pitfalls**: Enter 없이 Run/Save하면 env가 빠짐 → 예전엔 submit 때만 모델에 반영했음, source: `2ec79d2`; 비활성 키와 같은 키를 입력했는데 반영이 안 됨 → 비활성 항목 보호를 위해 그 입력을 버리고 경고 테두리 표시, source: `src/models/env_vars.rs` → `EnvVars::replace_active`; 비활성 변수가 input에 안 보임 → input은 활성 항목만, source: `src/app/env_modal.rs` → `active_env_text`
- **Shares**: `src/env_string.rs`, `src/models/env_vars.rs`
- **Related**: `env-modal`

### `env-modal`: 환경변수 편집 모달 (활성 체크박스, 행 복제/삭제, 중복 키 OK 차단, Tab 트랩)

- **Where**: Environment 섹션 `ICON_EDIT` → "Environment Variables" 모달
- **Aliases**: 환경변수 편집기, env 체크박스, 비활성화, 끄기, 행 복제, 중복 키, OK가 안 눌려, 탭 이동, enable checkbox, duplicate row, env editor
- **UI strings**: "Environment Variables", "Key", "Value", "Each key can appear only once.", "Cancel", "OK"
- **Controls**: checkbox → `Message::EnvModalToggleEntry` / `handle_env_modal_toggle_entry`; Key input → `Message::EnvModalRowKeyChanged` / `handle_env_modal_row_key_changed`; Value input → `Message::EnvModalRowValueChanged` / `handle_env_modal_row_value_changed`; `ICON_COPY` → `Message::EnvModalDuplicateEntry` / `handle_env_modal_duplicate_entry`; `ICON_DELETE` → `Message::EnvModalRemoveEntry` / `handle_env_modal_remove_entry`; "OK" → `Message::ConfirmEnvModal` / `handle_confirm_env_modal`; "Cancel" / `ICON_CLOSE` / Esc → `Message::CancelEnvModal` / `handle_cancel_env_modal`; Tab / Shift+Tab → `Message::EnvModalFocusNext` / `Message::EnvModalFocusPrev` / `handle_env_modal_focus_shift`
- **Code path**:
  - UI: `src/views/configuration_editor.rs` → `view_env_modal`, `view_env_modal_row`, `env_modal_key_id`, `env_modal_value_id`, `text_input_disabled_entry_style`, `text_input_warn_style`
  - State / handler: `src/app/env_modal.rs` → `EnvModalState`, `ModalCell`, `handle_open_env_modal`, `handle_confirm_env_modal`, `handle_env_modal_focus_shift`, `set_modal_row`; 키보드 `src/app.rs` → `subscription` (`env_modal_keyboard_subscription`)
  - Data: OK 시 `EnvVars::from_entries`로 `environment_variables` 교체(빈 키 제거, trim), Cancel 시 폐기. 중복 판정 `src/models/env_vars.rs` → `duplicate_key_indices`
- **Tests**: `src/app/env_modal.rs` → `toggling_a_row_off_excludes_it_from_the_run_after_confirm`, `cancel_discards_a_toggle`, `confirm_is_refused_while_keys_are_duplicated`, `toggling_an_out_of_range_row_is_ignored`, `a_duplicated_row_starts_enabled`, `modal_cell_linear_index_round_trips`; `src/models/env_vars.rs` → `active_skips_disabled_entries`, `set_enabled_reports_only_real_changes`
- **Pitfalls**: 체크박스를 껐는데 실행에 반영 안 됨 → OK를 눌러야 반영되고 Cancel·Esc는 버림, source: `src/app/env_modal.rs` → `handle_cancel_env_modal`; OK 버튼 비활성 → 중복 키(trim 기준) 존재, 핸들러도 방어, source: `src/app/env_modal.rs` → `handle_confirm_env_modal`; Tab이 모달 밖 에디터로 빠짐 → 모달 열림 시 editor Tab 구독 비활성 + 자체 cell cycle, source: `src/app.rs` → `subscription` (`editor_focus_subscription`, `!self.any_modal_open()`); 행 삭제 후 focus가 다른 데이터에 머묾 → clamp + 명시 focus, source: `src/app/env_modal.rs` → `handle_env_modal_remove_entry`; 마지막 빈 행 체크박스/복제/삭제 비활성 → placeholder row, source: `src/views/configuration_editor.rs` → `view_env_modal_row`
- **Shares**: 모달 공통 `src/app.rs` → `close_all_modals`; `modal_backdrop_style`, `modal_dialog_style`
- **Related**: `env-main-input`
- 부재: .env 파일 불러오기 없음, 모달 외부 클릭으로 닫기 없음 (의도)

### `config-save-load`: 구성 저장(Save 버튼/Cmd+S)과 시작 시 로드, 버전 3 파일

- **Where**: Configurations pane 헤더 툴바 "Save", 단축키 Cmd+S, 하단 상태바 메시지
- **Aliases**: 저장, 세이브, 저장이 안 돼, 자동 저장, 구성이 사라졌어, 로드 실패, 버전 오류, save, autosave, load, configs.json
- **UI strings**: "Save", "Saving configurations...", "Saved: {}", "Still loading configurations; please wait before saving", "Loaded {} configuration(s)", "Load failed: {error}", "Configuration file version {} is newer than this app supports ({}). Please update the app.", "No data directory available for the configuration store", "Deserialization error: {e}"
- **Controls**: `ICON_SAVE` (tooltip "Save") → `Message::SaveConfigurations` / `handle_save_configurations` → `Message::ConfigurationsSaved` / `handle_configurations_saved`; Cmd+S → `Message::SaveConfigurations` (`nav_shortcut_message`); 시작 → `Message::ConfigurationsLoaded` / `handle_configurations_loaded`
- **Code path**:
  - UI: `src/views/toolbar.rs` → `view_toolbar`
  - State / handler: `src/app.rs` → `handle_save_configurations`, `handle_configurations_saved`, `handle_configurations_loaded`, `new` (`load_or_migrate_store`)
  - Data: `{version: 3, configurations: [...]}`, written in `src/services/storage.rs` → `save_to_store`, `write_configs_at`, `serialize_config_file`, read in `src/services/storage.rs` → `load_or_migrate_store`, `load_or_migrate_at`, `parse_config_file`, `migrate_config_file` (`CURRENT_CONFIG_VERSION`)
- **Tests**: `src/app.rs` → `save_is_blocked_until_initial_load_completes`, `nav_shortcut_cmd_letters_map_to_actions`; `src/services/storage.rs` → `round_trips_through_versioned_envelope`, `serialized_file_declares_current_version`, `loads_legacy_bare_array_with_removed_config_type_field`, `rejects_newer_version_gracefully`, `rejects_the_version_right_above_current`, `version_1_file_still_loads`, `loads_version_2_file_with_a_spring_boot_configuration`, `version_2_env_map_loads_as_enabled_entries`, `disabled_env_entries_survive_a_save_and_load`, `parses_content_with_leading_bom`, `store_write_then_read_round_trips`, `store_read_corrupt_surfaces_error`, `migrate_seeds_store_from_legacy_when_store_missing`, `migrate_prefers_existing_store_over_legacy`
- **Pitfalls**: 시작 직후 Save가 빈 목록으로 덮어씀 → 로드 완료 전 Save 차단, source: `src/app.rs` → `handle_save_configurations` (`configs_ready`); 옛 앱이 구성 파일을 "newer version"으로 거절 → env 배열 형식 도입으로 버전 3, source: `src/services/storage.rs` → `CURRENT_CONFIG_VERSION`; 중첩 모드 필드를 더한 새 파일을 옛 앱이 원인 없는 Deserialization error로 거절 → 모드 열거형이 `deny_unknown_fields`라 필드 추가 시 버전을 올려야 함, source: `src/services/storage.rs` → `CURRENT_CONFIG_VERSION`
- **Shares**: `src/services/storage.rs` (Import/Export와 envelope 공유: `serialize_config_file`)
- **Related**: `config-add`, `env-modal`
- 부재: 자동 저장 없음, 미저장 표시(dirty) 없음, 창 닫기 시 구성 저장 안 함 (`src/app.rs` → `handle_close_window`는 `save_app_settings`만 호출)
