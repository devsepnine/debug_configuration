# RunConfigManager Feature Map

Verified at `8168d15` · `2026-10-12` · Structural view: none
Premises: 서버 없음. 구성은 data dir 아래 run_config_manager/configs.json, 앱 설정은 ~/.run_config_settings.json에 저장(둘 다 `src/services/storage.rs`), MCP 토큰 파일은 별도(`mcp-token` 항목) · i18n 없음, UI 문구는 Rust 소스의 영어 하드코딩 · 테스트는 각 파일 안 `#[cfg(test)] mod tests`, `path` → `test_fn`으로 인용
Note: 루트 `README.md`·`docs/README.md`의 기능 목록은 사용자용 소개다. 에이전트용 MCP 계약은 `docs/MCP.md`. 이 문서는 증상에서 코드로 가는 색인만 담는다.

## How to use this map

On a vague report such as "something in the sidebar is broken" or "the memo field won't take input":

1. **Where** did it happen? → Screen index → candidate feature IDs.
2. **What words** did they use? → Alias index → narrow the candidates. An alias marked `(ambiguous)` maps to several features, so check each, or ask one question that tells them apart. A row marked `→ none` is something the product does not have.
3. **What text** did they see? → grep the literal strings listed in the entry.
4. Open the entry's code path top-down; read **Pitfalls** before debugging.
5. No match anywhere → search the code, then add the missing feature, alias, or string here.

## Screen index

| Screen / region | Feature IDs |
|---|---|
| Title bar | `window-chrome`, `main-view-tabs` |
| Window edges (resize grips) | `window-chrome` |
| Status bar | `status-bar`, `update-check`, `update-install`, `mcp-audit-status` |
| Configurations → 목록 헤더 툴바 | `toolbar`, `config-add`, `config-save-load`, `import-configurations`, `export-configurations`, `settings-modal` |
| Configurations → 좌측 구성 목록 | `config-list-selection`, `config-clone`, `config-delete-confirm`, `config-drag-reorder`, `config-run-button`, `run-configuration` |
| Configurations → 삭제 확인 모달 | `config-delete-confirm` |
| Configurations → Editor → Basics | `config-editor-basics`, `type-application`, `type-shell-script`, `type-node`, `type-kotlin`, `type-spring-boot`, `type-compound` |
| Configurations → Editor → Environment | `env-main-input` |
| Environment Variables 모달 | `env-modal` |
| Import Configurations 모달 | `import-configurations` |
| Export Configurations 모달 | `export-configurations` |
| Update Available 모달 | `update-install` |
| Settings 모달 → 일반 | `settings-modal`, `settings-run-defaults`, `settings-persistence` |
| Settings 모달 → AI (MCP) | `mcp-server-lifecycle`, `mcp-permission-tiers`, `mcp-token`, `mcp-read-configurations`, `mcp-read-session-output` |
| Sessions → 좌측 세션 패널 | `session-list`, `session-rerun-bulk` |
| Sessions → 워크스페이스 탭 바 | `workspace-tabs` |
| Sessions → pane 타이틀바 | `pane-controls`, `session-rerun-bulk`, `terminal-auto-scroll`, `output-clear-export` |
| Sessions → pane 본문 터미널 | `terminal-output`, `run-env-banner`, `run-working-dir-check` |
| Sessions → pane 검색바 | `session-search` |
| Sessions → pane stdin 바 | `session-stdin` |
| Sessions → pane grid 레이아웃 | `pane-grid-layout` |
| 전역 키보드 | `keyboard-shortcuts` |
| (화면 없음) 설정 파일 · 창 위치 | `settings-persistence` |
| (화면 없음) 에이전트 HTTP `POST /mcp` | `mcp-http-auth`, `mcp-permission-tiers` |
| (화면 없음) 에이전트 툴 → 읽기 | `mcp-read-configurations`, `mcp-read-session-output` |
| (화면 없음) 에이전트 툴 → 실행 | `mcp-execute-sessions`, `mcp-session-input`, `mcp-request-dedup` |
| (화면 없음) 에이전트 툴 → 편집 | `mcp-edit-configurations`, `mcp-request-dedup` |

## Alias index

Aliases are inferred, not grepped.

| User says | Feature IDs |
|---|---|
| 구성 목록, 사이드바, 선택이 안 돼, 이름 잘림, 툴팁 | `config-list-selection` |
| 추가, 새 구성 | `config-add` |
| 구성 복제, clone | `config-clone` |
| 복사, 복제 `(ambiguous)` | `config-clone`, `env-modal`, `terminal-output`, `mcp-token` |
| 구성 삭제, 삭제 확인 | `config-delete-confirm` |
| 삭제, 지우기 `(ambiguous)` | `config-delete-confirm`, `env-modal`, `session-list`, `workspace-tabs`, `mcp-edit-configurations` |
| 순서 변경, 드래그해서 옮기기 | `config-drag-reorder` |
| 실행이 안 돼, run, 재생 버튼, 프로세스가 안 떠 | `config-run-button`, `run-configuration` |
| Cmd+R `(ambiguous)` | `config-run-button`, `session-rerun-bulk`, `keyboard-shortcuts` |
| 이름 변경, 타입 바꾸기, 작업 디렉터리, 폴더 선택, 타입 바꿨더니 값이 사라짐 | `config-editor-basics` |
| 이름 바꾸고 재실행이 안 돼 `(ambiguous)` | `session-rerun-bulk`, `mcp-execute-sessions` |
| 명령어, 인자 | `type-application` |
| 셸 스크립트, 인라인 스크립트, 인터프리터 | `type-shell-script` |
| package.json, npm, 스크립트 목록 새로고침, 노드 런타임, 창이 멈춤(Node) | `type-node` |
| 코틀린, 메인 클래스 감지, JAR, JDK | `type-kotlin`, `type-spring-boot` |
| 스프링 부트, 모듈 감지, 프로파일, 그레이들, 메이븐 | `type-spring-boot` |
| 컴파운드, 묶음 실행, 멤버 | `type-compound` |
| 환경변수 입력, 엔터 안 눌렀는데, 비활성 키와 같은 키 | `env-main-input` |
| env 체크박스, 환경변수 끄기/켜기, 비활성화, 중복 키, OK 안 눌림 | `env-modal` |
| Tab 이동, Tab이 안 가 `(ambiguous)` | `env-modal`, `config-editor-basics` |
| 환경변수가 실행에 안 들어가 `(ambiguous)` | `env-modal`, `env-main-input`, `run-env-banner` |
| Cmd+S, 구성 사라짐, 버전 오류, newer version, 로드 실패 | `config-save-load` |
| 저장, 저장이 안 돼 `(ambiguous)` | `config-save-load`, `output-clear-export`, `settings-persistence` |
| 경로 없음, 홈에서 실행됨, working directory | `run-working-dir-check` |
| Environment 줄, Environment: `(ambiguous)` | `run-env-banner`, `settings-run-defaults` |
| JAVA_HOME | `run-env-banner` |
| JAVA_TOOL_OPTIONS `(ambiguous)` | `run-env-banner`, `type-spring-boot` |
| VM 옵션, VM options | `type-spring-boot`, `type-kotlin` |
| 정지, 중지, 세션 삭제, 경과 시간, 상태 배지 | `session-list` |
| 재실행, rerun, 실패한 것만, 모두 중지 | `session-rerun-bulk` |
| 색이 깨져, 복사가 안 돼, 링크 클릭, 화면이 튐, 로그가 멈춤 | `terminal-output` |
| 최대 줄 수, 로그 잘림, max output lines `(ambiguous)` | `terminal-output`, `settings-run-defaults` |
| 자동 스크롤, auto-scroll, 맨 아래로, follow `(ambiguous)` | `terminal-auto-scroll`, `settings-run-defaults` |
| 로그 지우기, 로그 저장, export output | `output-clear-export` |
| 검색이 안 돼, 찾기, 정규식, 필터, Cmd+F | `session-search` |
| 입력, stdin, Ctrl+C, EOF, 입력 기록, 세션이 멈춤(입력) | `session-stdin` |
| pane 닫기, ⋯ 메뉴 | `pane-controls` |
| 닫기 `(ambiguous)` | `window-chrome`, `pane-controls`, `workspace-tabs` |
| 최대화 `(ambiguous)` | `window-chrome`, `pane-controls` |
| 화면 분할, pane 드래그, 크기 조절 | `pane-grid-layout` |
| 탭 추가, 탭 이름 바꾸기, 워크스페이스 이동, Cmd+숫자 | `workspace-tabs` |
| 단축키가 안 먹어 | `keyboard-shortcuts` |
| F1, F2, 화면 전환, Sessions 탭 `(ambiguous)` | `main-view-tabs`, `keyboard-shortcuts` |
| 설정, 환경설정, preferences, Cmd+, , Apply가 안 눌림, 설정이 안 먹음 | `settings-modal` |
| 시작 시 업데이트 확인 끄기 | `settings-run-defaults`, `update-check` |
| 재시작하면 설정 초기화, 창 위치 기억 안 함, 설정이 저장되지 않음 | `settings-persistence` |
| 가져오기, import, 덮어쓰기, replaces existing | `import-configurations` |
| 내보내기, export, 백업 | `export-configurations` |
| 업데이트 확인, 새 버전, 버전 라벨 | `update-check` |
| 업데이트 설치, 재시작 안 됨, Update 버튼 회색, 서명 검증, minisign, checksum mismatch, 브라우저가 멋대로 열림 | `update-install` |
| 창 드래그, 터치로 끌면 멈춤, 창 크기 조절 안 됨, 최소화, 창 닫으니 프로그램이 꺼짐 | `window-chrome` |
| 하단 메시지, status bar, 메시지가 금방 바뀜 | `status-bar` |
| 툴바, Add/Save 버튼 | `toolbar` |
| MCP 서버가 안 켜져, Starting에서 멈춤, port in use, 포트 | `mcp-server-lifecycle` |
| 401, Missing or invalid bearer token, Origin not allowed `(ambiguous)` | `mcp-http-auth`, `mcp-token` |
| 클라이언트가 응답을 거부, resultType, protocol version | `mcp-http-auth` |
| 토큰 복사, 재발급, claude mcp add 명령 | `mcp-token` |
| 툴이 안 보여, tool not found, Denied | `mcp-permission-tiers` |
| 환경변수 값이 <hidden>, 값 노출 `(ambiguous)` | `mcp-read-configurations`, `mcp-read-session-output`, `mcp-edit-configurations` |
| 세션 로그 읽기, 출력 검색 | `mcp-read-session-output` |
| 에이전트로 실행·중지·재실행 | `mcp-execute-sessions` |
| 프롬프트 응답, input pending | `mcp-session-input` |
| 에이전트로 구성 생성·수정·삭제, env 끄기 켜기(MCP), UUID 이름 | `mcp-edit-configurations` |
| 두 번 실행됨, request_id, retry window full | `mcp-request-dedup` |
| 에이전트가 뭘 했는지, 상태바 [MCP] | `mcp-audit-status` |
| 구성 검색, 목록 필터 | → none; nearest: `config-list-selection` |
| 키보드 화살표로 구성 선택 | → none; nearest: `config-list-selection` |
| 삭제 되돌리기, undo | → none; nearest: `config-delete-confirm` |
| 자동 저장, 저장 안 하고 닫음 | → none; nearest: `config-save-load` |
| .env 파일 불러오기 | → none; nearest: `env-modal` |
| 컴파운드 실행 순서 | → none; nearest: `type-compound` |
| 출력 복사 버튼 | → none; nearest: `terminal-output` |
| 탭 드래그 재정렬 | → none; nearest: `workspace-tabs` |
| 종료된 세션 모두 지우기 | → none; nearest: `session-list` |
| 세션·탭 레이아웃 저장(재시작 후 복원) | → none; nearest: `workspace-tabs` |
| 단축키 변경 | → none; nearest: `keyboard-shortcuts` |
| 설정 초기화 버튼, reset to defaults | → none; nearest: `settings-modal` |
| 지금 업데이트 확인 버튼(설정 안) | → none; nearest: `update-check` |
| 상태 메시지 복사, 메시지 기록 | → none; nearest: `status-bar` |
| 닫을 때 실행 중 세션 경고 | → none; nearest: `window-chrome` |
| MCP 호출 이력 보기 | → none; nearest: `mcp-audit-status` |
| 툴별로 켜고 끄기, per-tool allowlist | → none; nearest: `mcp-permission-tiers` |
| 연결 테스트 버튼, 연결된 클라이언트 목록 | → none; nearest: `mcp-server-lifecycle` |
| 원격(다른 기기)에서 접속, 0.0.0.0 bind | → none; nearest: `mcp-http-auth` |
| SSE 스트림, GET /mcp | → none; nearest: `mcp-http-auth` |

## Features

Entries live in one file per screen under `docs/features/`.

| ID | Purpose | File |
|---|---|---|
| `config-list-selection` | 사이드바 구성 목록 표시와 클릭 선택 | [`docs/features/configurations.md`](features/configurations.md) |
| `config-add` | 새 구성 추가 | [`docs/features/configurations.md`](features/configurations.md) |
| `config-clone` | 구성 복제 | [`docs/features/configurations.md`](features/configurations.md) |
| `config-delete-confirm` | 확인 모달을 거치는 구성 삭제 | [`docs/features/configurations.md`](features/configurations.md) |
| `config-drag-reorder` | 드래그로 구성 순서 변경 | [`docs/features/configurations.md`](features/configurations.md) |
| `config-run-button` | 목록에서 구성 실행 | [`docs/features/configurations.md`](features/configurations.md) |
| `config-editor-basics` | 에디터 Basics 섹션 (이름, 타입 전환, 작업 디렉터리) | [`docs/features/configurations.md`](features/configurations.md) |
| `type-application` | Application 타입 필드 (명령, 인자) | [`docs/features/configurations.md`](features/configurations.md) |
| `type-shell-script` | Shell Script 타입 (실행 모드, 스크립트 파일, 인라인 스크립트 에디터) | [`docs/features/configurations.md`](features/configurations.md) |
| `type-node` | Node 타입 (프로젝트 디렉터리, package.json 드롭다운, 런타임, 패키지 매니저, scripts 새로고침) | [`docs/features/configurations.md`](features/configurations.md) |
| `type-kotlin` | Kotlin 타입 (실행 모드, main class/JAR 감지, JDK) | [`docs/features/configurations.md`](features/configurations.md) |
| `type-spring-boot` | Spring Boot 타입 (빌드 도구, 모듈 감지, 프로파일, JAR, JDK) | [`docs/features/configurations.md`](features/configurations.md) |
| `type-compound` | Compound 타입 (멤버 토글, 워크스페이스 탭) | [`docs/features/configurations.md`](features/configurations.md) |
| `env-main-input` | Environment 섹션 한 줄 입력 | [`docs/features/configurations.md`](features/configurations.md) |
| `env-modal` | 환경변수 편집 모달 (활성 체크박스, 행 복제/삭제, 중복 키 OK 차단, Tab 트랩) | [`docs/features/configurations.md`](features/configurations.md) |
| `config-save-load` | 구성 저장(Save 버튼/Cmd+S)과 시작 시 로드, 버전 3 파일 | [`docs/features/configurations.md`](features/configurations.md) |
| `run-configuration` | 구성을 실행해 새 세션을 만들고 PTY로 프로세스를 띄운다 | [`docs/features/sessions.md`](features/sessions.md) |
| `run-working-dir-check` | 실행 전 작업 디렉터리 존재 여부를 검사해 잘못된 cwd 실행을 막는다 | [`docs/features/sessions.md`](features/sessions.md) |
| `run-env-banner` | 실행 배너(구성명·타입·cwd·명령)와 "Environment:" 줄, JAVA_HOME/JAVA_TOOL_OPTIONS 병합 | [`docs/features/sessions.md`](features/sessions.md) |
| `session-list` | 좌측 세션 목록(상태 점·배지·경과시간), 개별 정지/제거, 클릭으로 워크스페이스에 열기 | [`docs/features/sessions.md`](features/sessions.md) |
| `session-rerun-bulk` | 세션 재실행(개별)과 전체 재실행 / 실패만 재실행 / 전체 정지 | [`docs/features/sessions.md`](features/sessions.md) |
| `terminal-output` | 세션 출력 렌더링(ANSI 색, 줄바꿈, 스크롤바, URL 클릭, 선택·복사)과 출력 버퍼 상한 | [`docs/features/sessions.md`](features/sessions.md) |
| `terminal-auto-scroll` | 새 출력 따라가기(자동 스크롤) 토글과 스크롤 시 자동 해제 | [`docs/features/sessions.md`](features/sessions.md) |
| `output-clear-export` | 세션 출력 지우기와 파일로 내보내기 | [`docs/features/sessions.md`](features/sessions.md) |
| `session-search` | 출력 내 검색(하이라이트, 다음/이전, 정규식, 매치 줄만 보기, Cmd+F) | [`docs/features/sessions.md`](features/sessions.md) |
| `session-stdin` | 실행 중 프로세스에 입력 보내기, 인터럽트(Ctrl+C), EOF(Ctrl+D), 입력 히스토리(↑/↓) | [`docs/features/sessions.md`](features/sessions.md) |
| `pane-controls` | pane 타이틀바(세션명·상태 배지) 컨트롤, 좁을 때 ⋯ 메뉴, 최대화/복원, 닫기(숨기기) | [`docs/features/sessions.md`](features/sessions.md) |
| `pane-grid-layout` | 세션 열 때 자동 분할, pane 드래그로 재배치, 분할선 리사이즈 | [`docs/features/sessions.md`](features/sessions.md) |
| `workspace-tabs` | 워크스페이스 탭 추가·이름 변경(더블클릭)·닫기·전환, Cmd+1~9 점프 | [`docs/features/sessions.md`](features/sessions.md) |
| `keyboard-shortcuts` | 전역 단축키(F1/F2 화면 전환, Cmd+1~9, Cmd+S/R/W/I/,, Cmd+F, Esc, Enter) | [`docs/features/sessions.md`](features/sessions.md) |
| `settings-modal` | 앱 설정 모달 열기/staging/Apply/OK/Cancel 흐름 | [`docs/features/app.md`](features/app.md) |
| `settings-run-defaults` | 실행 관련 기본값 (Environment 라인 표시, 최대 출력 라인, 자동 스크롤, 시작 시 업데이트 확인) | [`docs/features/app.md`](features/app.md) |
| `settings-persistence` | 앱 설정·창 크기/위치를 홈 디렉터리 JSON에 저장/복원 | [`docs/features/app.md`](features/app.md) |
| `import-configurations` | 파일에서 구성을 골라 현재 목록에 병합 (같은 id는 교체) | [`docs/features/app.md`](features/app.md) |
| `export-configurations` | 선택한 구성을 JSON 파일로 내보내기 | [`docs/features/app.md`](features/app.md) |
| `update-check` | GitHub 최신 릴리스 확인 (시작 시 자동, 상태바 버전 라벨 클릭 시 수동) | [`docs/features/app.md`](features/app.md) |
| `update-install` | 확인 모달 → 다운로드·서명/체크섬 검증 → 설치 → 재실행 (self-update) | [`docs/features/app.md`](features/app.md) |
| `window-chrome` | 커스텀 타이틀바 (드래그 이동, 더블클릭 최대화, 닫기/최소화/최대화, 가장자리 리사이즈) | [`docs/features/app.md`](features/app.md) |
| `status-bar` | 하단 상태바 한 줄 메시지와 버전 라벨 | [`docs/features/app.md`](features/app.md) |
| `main-view-tabs` | 타이틀바 우측 Configurations / Sessions 화면 전환 탭 | [`docs/features/app.md`](features/app.md) |
| `toolbar` | Configurations 목록 상단 아이콘 툴바 (Add / Import / Export / Save / Settings) | [`docs/features/app.md`](features/app.md) |
| `mcp-server-lifecycle` | 로컬 MCP 서버 켜기·포트 설정·리스너 상태 표시 | [`docs/features/mcp.md`](features/mcp.md) |
| `mcp-http-auth` | HTTP 엔드포인트 `POST /mcp`의 Origin·Bearer·본문 상한·프로토콜 버전 검사 | [`docs/features/mcp.md`](features/mcp.md) |
| `mcp-token` | Bearer 토큰 생성·복사·재발급과 `claude mcp add` 명령 복사 | [`docs/features/mcp.md`](features/mcp.md) |
| `mcp-permission-tiers` | 권한 단계(Read only / Execute / Edit)와 `tools/list` 필터·호출 시점 재검사 | [`docs/features/mcp.md`](features/mcp.md) |
| `mcp-read-configurations` | `list_configurations`·`get_configuration`과 환경변수 값 마스킹 토글 | [`docs/features/mcp.md`](features/mcp.md) |
| `mcp-read-session-output` | `list_sessions`·`read_session_output`·`search_session_output`과 env 배너 마스킹 | [`docs/features/mcp.md`](features/mcp.md) |
| `mcp-execute-sessions` | `run_configuration`·`stop_session`·`rerun_session` (Execute 단계) | [`docs/features/mcp.md`](features/mcp.md) |
| `mcp-session-input` | `send_session_input`으로 실행 중 세션 stdin에 한 줄 전송 | [`docs/features/mcp.md`](features/mcp.md) |
| `mcp-edit-configurations` | `create_configuration`·`update_configuration`·`delete_configuration` (Edit 단계, env enable/disable 포함) | [`docs/features/mcp.md`](features/mcp.md) |
| `mcp-request-dedup` | 변경성 툴의 `request_id` 재시도 흡수 (60초 창, 512개 상한) | [`docs/features/mcp.md`](features/mcp.md) |
| `mcp-audit-status` | 에이전트 조작을 상태바에 `[MCP]` 접두사로 남기는 감사 줄 | [`docs/features/mcp.md`](features/mcp.md) |
