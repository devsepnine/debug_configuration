# RunConfigManager Architecture Rules

Verified at `97e9cfc` · `2026-10-12` · Structural view: none · Feature map: `docs/FEATURES.md`
Premises: 단일 바이너리 크레이트(`Cargo.toml`, workspace 없음), iced 0.14 Elm 구조(`update`/`view`/`subscription`). 경계 lint는 없다. 단위는 `src/main.rs`가 선언하는 최상위 모듈이다.

## How to use these rules

Before adding a file in a new place, a new module, or an import that crosses units:

1. Find the unit you are in and the unit you would import from in **Units**.
2. Check every rule that names either unit. A `MUST` violation is a defect. A `SHOULD` violation needs a reason in the PR.
3. A file listed under **Known violations** is not a precedent. Do not copy its pattern.

## Units

| Unit | Path | Responsibility | May depend on |
|---|---|---|---|
| `app` | `src/app.rs`, `src/app/` | `RunConfigManager` 상태, `update` 디스패치, 핸들러, `view` 조립, `subscription` | 모든 단위 |
| `views` | `src/views/` | 상태 조각을 받아 `Element`를 만드는 뷰 함수와 `*View` props | `messages`, `models`, `widgets`, `utils`, `ansi`, `env_string`, `services`(타입만) |
| `widgets` | `src/widgets/` | 재사용 커스텀 위젯(`pane_grid`, `configuration_list`, `title_bar_drag`)과 pane 레이아웃 상태(`pane_layout`) | `messages`, `models`, `utils`, `services`(타입만) |
| `messages` | `src/messages.rs` | `Message` 열거형과 메시지 페이로드 타입 | `models`, `services`(이벤트 타입), `widgets`(위젯 이벤트 타입), `utils` |
| `services` | `src/services/` | 부수 효과: 프로세스 실행, 파일 저장, 로컬 환경 감지, 업데이트, MCP 서버 | `models`, `utils` |
| `models` | `src/models/` | 도메인 데이터 타입(구성, 세션, 환경변수)과 그 불변식 | `ansi` |
| `utils` | `src/utils.rs` | 아이콘 상수와 순수 텍스트·경로 헬퍼 | none |
| `ansi` | `src/ansi.rs` | ANSI 이스케이프를 `TextSegment`로 파싱 | none |
| `env_string` | `src/env_string.rs` | `KEY=value;...` 문자열 파싱·직렬화 | none |

`src/main.rs`는 진입점이며 단위가 아니다. 창 설정, 폰트, 알림 앱 등록을 다룬다.

`messages`와 `widgets`는 서로를 참조한다(`src/messages.rs` → `pane_grid` 이벤트 타입, `src/widgets/configuration_list.rs` → `Message`). 메시지 열거형이 위젯 이벤트를 싣는 iced 관례라 허용한다. 이 둘 밖으로 순환을 늘리지 않는다.

## Rules

### `arch-app-is-top`

- **Level**: MUST
- **Rule**: `src/app.rs`와 `src/app/` 밖의 어떤 파일도 `crate::app`을 참조하지 않는다.
- **Why**: Elm 구조에서 상태와 디스패치는 최상위다. 아래 단위가 `app`을 알면 뷰나 서비스가 상태를 직접 고칠 수 있게 된다.
- **Evidence**: 0 files · `grep -rlE 'crate::app(::|\b)' src | grep -vE '^src/app(\.rs|/)'`
- **Exceptions**: none
- **Enforced by**: draft: CI grep

### `arch-services-no-ui`

- **Level**: MUST
- **Rule**: `src/services/`는 `crate::app`, `crate::views`, `crate::widgets`, `crate::messages`를 참조하지 않는다. 서비스는 자기 이벤트 타입을 내고 `app`이 그것을 `Message`로 감싼다.
- **Why**: 2026-10-12 결정(표준 방식). MCP(`src/services/mcp/mod.rs` → `McpEvent`, `McpRequest`)와 실행(`src/services/executor.rs` → `RunEvent`)이 이 방식이다. 서비스가 `Message`를 알면 `messages ↔ services` 순환이 생기고 서비스를 UI 없이 테스트할 수 없다.
- **Evidence**: 0 files · `grep -rlE 'crate::(app|views|widgets|messages)(::|\b)' src/services`
- **Exceptions**: none
- **Enforced by**: draft: CI grep

### `arch-models-no-ui`

- **Level**: MUST
- **Rule**: `src/models/`는 `crate::app`, `crate::views`, `crate::widgets`, `crate::messages`, `crate::services`를 참조하지 않는다.
- **Why**: 2026-10-12 결정(표준 방식). 도메인 타입은 직렬화·테스트·MCP가 함께 쓰므로 UI 위젯 구현에 묶이면 안 된다.
- **Evidence**: 0 of 4 files · `grep -rlE 'crate::(app|views|widgets|messages|services)(::|\b)' src/models`
- **Exceptions**: none
- **Enforced by**: draft: CI grep

### `arch-views-pure`

- **Level**: MUST
- **Rule**: `src/views/`와 `src/widgets/`는 `crate::app`을 참조하지 않고 `Task`를 반환하지 않는다. 상태 변경은 `Message`로만 요청한다. `crate::services`에서는 데이터 타입만 가져오고 함수는 호출하지 않는다.
- **Why**: iced의 `view`는 상태의 순수 함수다. 증거상 예외가 없다.
- **Evidence**: `crate::app` 0 files · `grep -rlE 'crate::app(::|\b)' src/views src/widgets`; `Task<` 0 files · `grep -rlE '\bTask<' src/views src/widgets`; services 참조 1 file(타입 `McpPermission`) · `grep -rnE 'crate::services' src/views src/widgets`
- **Exceptions**: none
- **Enforced by**: draft: CI grep (함수 호출 여부는 review)

### `arch-ui-types-in-ui-units`

- **Level**: MUST
- **Rule**: `Element<`는 `src/views/`, `src/widgets/`, `src/app.rs`, `src/app/`, `src/main.rs`에만, `Task<`는 `src/app.rs`, `src/app/`, `src/main.rs`에만 나온다.
- **Why**: 증거상 예외가 없다. 지금 `Task<`는 `src/app.rs`, `src/app/` 아래 모듈, `src/main.rs`에만 나온다.
- **Evidence**: `Element<` 0 files · `grep -rlE '\bElement<' src | grep -vE '^src/(views/|widgets/|app\.rs|app/|main\.rs)'`; `Task<` 0 files · `grep -rlE '\bTask<' src | grep -vE '^src/(app\.rs|app/|main\.rs)'`
- **Exceptions**: none
- **Enforced by**: draft: CI grep

### `arch-io-in-services`

- **Level**: MUST
- **Rule**: 파일 시스템(`std::fs`, `tokio::fs`), 프로세스 실행(`process::Command`, `tokio::process`, `portable_pty`), 파일 다이얼로그(`rfd`), 네트워크(`reqwest`, `hyper`), 플랫폼 경로(`dirs`), URL 열기(`open`), OS 알림(`notify_rust`)은 `src/services/` 안에서만 쓴다. 환경변수 읽기(`std::env`)와 스레드 생성은 이 규칙 밖이다.
- **Why**: 2026-10-12 결정(표준 방식). 부수 효과를 한 단위에 모아야 테스트에서 막을 지점이 하나가 된다. 지금은 테스트 빌드에서 URL을 열지 않으려고 `src/app.rs`가 `#[cfg(not(test))]` 가드를 직접 두고 있다.
- **Evidence**: 3 files · `grep -rlE 'std::fs|\bfs::|tokio::(fs|process)|process::Command|portable_pty|rfd::|AsyncFileDialog|reqwest::|hyper::|dirs::|\bopen::|notify_rust' src | grep -v '^src/services/'`
- **Exceptions**: `src/main.rs`, 진입점에서 `notify_rust::set_application`으로 알림 앱을 한 번 등록한다(앱 실행 전에만 가능)
- **Enforced by**: draft: CI grep

### `arch-modal-module-shape`

- **Level**: MUST
- **Rule**: 새 모달은 `src/app/<name>_modal.rs`에 `pub struct <Name>ModalState`와 `pub(super) fn handle_*` 핸들러를 두고, 여는 핸들러에서 `self.close_all_modals()`를 호출한다.
- **Why**: 6개 모달 전부가 이 형태다. 한 번에 하나의 모달만 열린다는 불변식을 `close_all_modals`가 지킨다.
- **Evidence**: 6 of 6 · `ls src/app/*_modal.rs`, `grep -lE 'pub struct [A-Za-z]+ModalState' src/app/*_modal.rs`, `grep -l 'self.close_all_modals()' src/app/*_modal.rs` (호출 존재만 측정, 호출 순서는 review)
- **Exceptions**: none
- **Enforced by**: review

### `arch-modal-views-file`

- **Level**: SHOULD
- **Rule**: 새 모달의 뷰(`<Name>ModalView` props와 `view_<name>_modal`)는 `<new> src/views/modals.rs`에 둔다.
- **Why**: 2026-10-12 결정(마이그레이션). 지금은 6개 모달 뷰가 모두 구성 편집기와 함께 `src/views/configuration_editor.rs`에 있어 파일이 커졌다.
- **Evidence**: 6 of 6 modal views in `src/views/configuration_editor.rs` · `grep -nE 'pub fn view_[a-z_]+_modal' src/views/*.rs`
- **Exceptions**: 기존 6개 모달 뷰는 `<new> src/views/modals.rs`로 옮길 때까지 `src/views/configuration_editor.rs`에 둔다
- **Enforced by**: review

### `arch-inline-tests`

- **Level**: SHOULD
- **Rule**: 단위 테스트는 대상 파일 안의 `#[cfg(test)] mod tests`에 둔다. 루트에 `<none> tests/` 디렉터리를 만들지 않는다.
- **Why**: 바이너리 크레이트라 통합 테스트 디렉터리에서 내부 항목에 접근할 수 없다. 31개 파일이 이 방식이고 루트 tests 디렉터리는 없다.
- **Evidence**: 31 files with `#[cfg(test)]`, 0 tests dir · `grep -rl '#\[cfg(test)\]' src`, `ls -d tests`
- **Exceptions**: none
- **Enforced by**: review

## Known violations

| Path | Rule | Decision |
|---|---|---|
| `src/app.rs` | `arch-io-in-services` | fix: 파일 다이얼로그(`rfd`), OS 알림(`notify_rust`), URL 열기(`open::that`), 업데이트 후 재실행(`process::Command`)을 `services`로 옮긴다 |
| `src/views/configuration_editor.rs` | `arch-modal-views-file` | accepted until moved: 기존 6개 모달 뷰 |

## Decisions

| Date | Question | Answer |
|---|---|---|
| `2026-10-12` | models/pane.rs(현 `src/widgets/pane_layout.rs`)가 `crate::widgets::pane_grid`를 직접 쓴다. 모델이 UI 위젯에 의존해도 되는가 | 표준 방식을 따른다. models는 UI에 의존하지 않는 MUST 규칙으로 두고 pane.rs는 고칠 위반으로 기록한다 |
| `2026-10-12` | `executor.rs`는 `Message`를 직접 보내고 MCP는 자기 이벤트 타입을 낸다. 새 service는 어느 쪽인가 | 표준 방식으로 규칙을 추가한다. services는 자기 이벤트 타입을 내고 app이 감싼다(MUST). executor.rs는 고칠 위반 |
| `2026-10-12` | 파일 I/O·프로세스 실행이 `utils.rs`와 `app.rs`에도 있다 | 표준 방식으로 작성한다. I/O는 services에만 둔다(MUST). 두 파일은 고칠 위반 |
| `2026-10-12` | 모달 뷰 6개가 `configuration_editor.rs`에 있다. 새 모달 뷰는 어디인가 | 마이그레이션. 새 모달 뷰는 `<new> src/views/modals.rs`, 기존 6개는 옮길 때까지 허용 |

"표준 방식"은 iced/Elm 계층 구조의 일반 관례를 뜻한다: 도메인 모델은 UI를 모르고, 서비스는 UI 메시지가 아니라 자기 이벤트를 내며, 부수 효과는 서비스 계층에 모은다. 2026-10-12에 사용자가 이 해석과 세 규칙의 MUST 수준을 확인했다.
