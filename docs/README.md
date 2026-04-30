# Run/Debug Configuration Manager - 기술 문서

> Last Updated: 2026-01-30

## 개요

Rust와 Iced GUI 프레임워크로 구축된 크로스 플랫폼 데스크톱 애플리케이션입니다.
다양한 타입의 실행 구성(Application, Shell Script, NPM)을 관리하고 멀티 세션 실행을 지원합니다.

## 프로젝트 구조

```
debug_configuration/
├── src/
│   ├── main.rs              # 애플리케이션 진입점
│   ├── app.rs               # 메인 애플리케이션 상태 및 업데이트 로직
│   ├── messages.rs          # Elm Architecture 메시지 타입 정의
│   ├── ansi.rs              # ANSI 색상 코드 파싱
│   ├── utils.rs             # NPM 유틸리티 함수 (Node Runtime 감지, package.json 파싱)
│   ├── models/
│   │   ├── mod.rs           # 모델 모듈 재export
│   │   ├── configuration.rs # RunConfiguration, ConfigTypeData, NpmCommand
│   │   ├── session.rs       # ExecutionSession 모델
│   │   └── pane.rs          # Pane 레이아웃 상태
│   ├── services/
│   │   ├── mod.rs           # 서비스 모듈 재export
│   │   ├── executor.rs      # 프로세스 실행 및 명령어 빌드 로직
│   │   └── storage.rs       # 구성 파일 저장/로드
│   └── views/
│       ├── mod.rs                  # 뷰 모듈 재export
│       ├── configuration_editor.rs # 구성 편집기 UI
│       ├── configuration_list.rs   # 구성 목록 UI
│       ├── main_tabs.rs            # 메인 탭 (Configuration/Sessions)
│       ├── toolbar.rs              # 상단 툴바
│       ├── workspace_tabs.rs       # 워크스페이스 탭 관리
│       ├── pane_view.rs            # Pane 레이아웃 렌더링
│       └── terminal.rs             # 터미널 출력 렌더링
├── assets/                  # SVG 아이콘 (mingcute, remix icon)
├── fonts/                   # D2Coding 폰트
├── docs/                    # 프로젝트 문서
└── Cargo.toml               # 의존성 및 메타데이터
```

## 주요 컴포넌트

### 구성 타입 (Configuration Types)

1. **Application**
   - 일반 실행 파일 실행
   - 필드: command, arguments

2. **Shell Script**
   - 셸 스크립트 실행 (ScriptFile/ScriptText 모드)
   - 필드: execute_mode, script_path, script_text, script_options, interpreter_path, interpreter_options

3. **NPM** (최신 추가)
   - NPM 프로젝트 실행
   - 필드: project_directory, node_runtime_path, command, script_name, arguments, node_options
   - 43개 NPM 명령어 지원 (run, install, start, test, build 등)
   - Node Runtime 자동 감지 (system PATH, nvm, nvm-windows)
   - package.json 자동 스캔 및 scripts 파싱
   - 패키지 매니저 자동 감지 (npm, yarn, pnpm)

### 비동기 작업 (Async Tasks)

- **Node Runtime 감지**: 앱 초기화 시 백그라운드에서 실행
- **package.json 스캔**: 디렉토리 선택 후 즉시 UI 업데이트, 백그라운드 스캔
- **Task::perform()**: Iced의 비동기 Task 시스템 활용
- **Task::batch()**: 여러 초기화 작업 병렬 실행

### 실행 세션 관리

- **멀티 프로세스 실행**: 여러 구성을 동시에 실행
- **실시간 출력**: ANSI 색상 코드 지원
- **프로세스 제어**: 재실행, 중지, graceful shutdown
- **세션 이동**: 드래그 앤 드롭으로 Pane 간 이동

### Pane 레이아웃 시스템

- **동적 분할**: 수평/수직 분할 지원
- **드래그 앤 드롭**: Pane 재배치 및 리사이징
- **반투명 효과**: Pane 드래그 시 전체 UI가 50% 투명도로 변경되어 시각적 피드백 제공
- **탭 관리**: 여러 세션을 탭으로 구성
- **Drop Zone**: 시각적 드롭 영역 표시

## 의존성

```toml
[dependencies]
iced = { version = "0.14", features = ["tokio", "canvas", "svg", "advanced"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
uuid = { version = "1.20.0", features = ["v4", "serde"] }
dirs = "6.0"
tokio = { version = "1.49", features = ["full"] }
ttf-parser = "0.25.1"
unicode-width = "0.2.2"
regex = "1.12"
open = "5.3"
ctrlc = "3.5"
rfd = "0.17"

[target.'cfg(unix)'.dependencies]
libc = "0.2"
```

## 최근 변경 사항

### 2026-01-30

#### 추가됨
- **Pane 드래그 반투명 효과**
  - pane_grid 네이티브 드래그 중 시각적 피드백
  - TitleBar, 터미널, 컨트롤 버튼, 배경 모두 50% 투명도 적용
  - `dragging_pane_id` 상태를 view 계층 전체에 전달
  - 드래그 중인 pane만 선택적으로 반투명 처리

#### 수정됨
- **코드 품질 개선**
  - clippy 경고 97개 → 4개로 감소 (95.9% 개선)
  - 불필요한 factory 메서드 제거 (`new_application`, `new_shell_script`, `new_npm`)
  - 중복 코드 제거 및 간결화
  - 문서 주석 포맷팅 개선

### 2026-01-29

#### 추가됨
- **NPM Configuration 타입 구현**
  - ConfigurationType::Npm, ConfigTypeData::Npm 추가
  - NpmCommand enum (43개 명령어: run, install, start, test, build 등)
  - NPM 구성 편집 UI (7개 입력 필드)

- **Node Runtime 자동 감지 시스템**
  - `detect_node_runtimes()`: system PATH 및 nvm 설치 스캔
  - Windows: `where node`, nvm-windows 디렉토리 스캔
  - Unix: `which -a node`, ~/.nvm/versions/node 스캔
  - 비동기 초기화로 UI 블로킹 방지

- **package.json 관리 기능**
  - `find_all_package_jsons()`: 재귀 탐색 (깊이 제한 5, 20+ ignore 패턴)
  - `parse_scripts()`: package.json scripts 섹션 파싱
  - `detect_package_manager()`: lockfile 기반 패키지 매니저 감지
  - 비동기 스캔으로 즉시 경로 표시 후 백그라운드 처리

- **PowerShell 명령어 처리 개선**
  - `quote_if_needed()`: 공백 포함 경로 자동 따옴표 처리
  - Call operator (`&`) 추가로 PowerShell 파싱 에러 수정

- **새 메시지 타입**
  - `NodeRuntimesDetected`: Node Runtime 감지 완료
  - `NodeRuntimeChanged`: Node Runtime 선택 변경
  - `PackageJsonsScanned`: package.json 스캔 완료

#### 수정됨
- **Shell Script 실행 로직**
  - ScriptFile/ScriptText 모드 지원
  - Interpreter 경로 처리 개선
  - 환경 변수 설정 로직 통합

- **비동기 초기화**
  - `detect_node_runtimes()` 동기 호출 → 비동기 Task로 변경
  - `find_all_package_jsons()` 동기 호출 → 비동기 Task로 변경
  - 앱 초기화 블로킹 제거 (파일 다이얼로그 즉시 열림)

- **성능 최적화**
  - package.json 스캔 깊이 제한 (5 레벨)
  - ignore 패턴 확장 (node_modules, .git, dist, build, .next 등 20개)
  - CREATE_NO_WINDOW 플래그 추가 (콘솔 창 깜빡임 방지)

#### 버그 수정
- `to_relative_path()` 경로 중복 표시 문제 해결
- PowerShell 따옴표 경로 파싱 에러 수정 (`& "path"` 형식)
- 구성 로드 시 package.json 목록 자동 로드 누락 수정
- 모든 `Command::new()` 호출에 CREATE_NO_WINDOW 플래그 추가

### 2026-01-21 ~ 2026-01-29

#### 추가됨
- **Shell Script Configuration 타입**
  - ExecuteMode (ScriptFile/ScriptText) 지원
  - Interpreter 경로 선택 및 옵션 설정
  - Script 옵션 및 텍스트 편집

- **폴더 브라우저**
  - 작업 디렉토리 선택 다이얼로그
  - 스크립트 파일 선택 다이얼로그
  - 인터프리터 경로 선택 다이얼로그

- **Persistent AppSettings**
  - 마지막 저장 경로 기억
  - 앱 재시작 시 자동 로드

#### 수정됨
- **WorkspaceTab 패턴 매칭 개선**
  - 코드 가독성 향상
  - 중복 제거

- **의존성 업데이트**
  - iced 0.14
  - tokio 1.49
  - uuid 1.20.0

## 데이터 저장 위치

구성 파일은 OS별 설정 디렉토리에 자동 저장됩니다:
- **Linux**: `~/.config/run_config_manager/configs.json`
- **macOS**: `~/Library/Application Support/run_config_manager/configs.json`
- **Windows**: `%APPDATA%\run_config_manager\configs.json`

## 빌드 및 실행

### 빌드
```bash
cargo build --release
```

### 실행
```bash
cargo run --release
```

### macOS 앱 번들 생성
```bash
./build_macos_app.sh
```

## 아키텍처 패턴

### Elm Architecture
- **Model**: `RunConfigManager` 구조체 (app.rs)
- **View**: `views/` 모듈 (선언적 UI)
- **Update**: `update()` 메서드 (메시지 기반 상태 변경)
- **Task**: 비동기 작업 처리

### 성능 최적화
- 비동기 초기화로 앱 시작 속도 개선
- 백그라운드 Task로 UI 블로킹 방지
- 깊이 제한 및 ignore 패턴으로 파일 시스템 스캔 최적화
- CREATE_NO_WINDOW 플래그로 콘솔 창 깜빡임 제거
- 코드 품질 개선으로 컴파일 경고 최소화

### 크로스 플랫폼 지원
- Windows: PowerShell, taskkill, CREATE_NO_WINDOW 플래그
- Unix: bash, SIGTERM/SIGKILL, process groups
- 조건부 컴파일 (`#[cfg(target_os = "...")]`)

## 향후 계획

- [ ] Docker Configuration 타입 추가
- [ ] Python Virtual Environment 지원
- [ ] 구성 그룹화 및 태그 기능
- [ ] 키보드 단축키 커스터마이징
- [ ] 테마 선택 기능
