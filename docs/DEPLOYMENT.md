# 배포 가이드 — GitHub Actions

> Last Updated: 2026-06-25
> 대상: `run_config_manager` (Rust + iced 데스크톱 앱)

이 문서는 GitHub Actions 기반 CI/CD 파이프라인과 릴리스 수행 절차를 설명한다.
수동 빌드 절차는 [`RUNBOOK.md`](./RUNBOOK.md)를 참고한다.

## 워크플로우 개요

`.github/workflows/`에 3개의 워크플로우가 있다.

| 워크플로우 | 파일 | 트리거 | 역할 |
|---|---|---|---|
| **Build** | `build.yml` | `pull_request`, `push`(develop) | CI 품질 게이트 (check/clippy/test/fmt) |
| **Release** | `release.yml` | `push`(`release/v*` 브랜치), `workflow_dispatch` | 검증 → 품질 → 4개 플랫폼 빌드 → 드래프트 릴리스 발행 |
| **Checksum** | `checksum.yml` | `workflow_dispatch` | 기존 릴리스의 `checksums.txt` 재생성 |

공통 사항:
- 모든 서드파티 액션은 **commit SHA로 핀**되어 있다(공급망 재현성).
- 권한은 최소화되어 있다(기본 `contents: read`, 발행 단계만 `contents: write`).
- `CARGO_TERM_COLOR=always`, `BINARY_NAME=run_config_manager` 환경변수 공유.

## CI — Build (`build.yml`)

PR과 `develop` 푸시마다 `ubuntu-latest`에서 단일 `quality` job을 실행한다.

```
cargo check  --locked
cargo clippy --locked --all-targets -- -D warnings
cargo test   --locked
cargo fmt --all -- --check
```

Linux 시스템 의존성(`libfontconfig1-dev`, `libwayland-dev`, `libx11-xcb-dev`,
`libxkbcommon-dev`, `pkg-config`)을 사전 설치한다. 이 게이트가 곧 머지 기준이다.

> 주의: `cargo fmt --all -- --check`는 **전체 트리**를 검사한다. 일부 파일이라도
> 포맷이 어긋나면 CI가 실패하므로, 커밋 전 `cargo fmt`로 전체를 정리한다.

## 릴리스 파이프라인 — Release (`release.yml`)

### 트리거

1. **권장**: `release/v*` 형식 브랜치 푸시
   - 예: `release/v0.2.5` 푸시 → 태그 `v0.2.5`로 해석 (`release/` 접두사 제거)
2. **수동 폴백**: `workflow_dispatch` + `tag` 입력
   - 예: `v0.2.5` — **태그가 이미 존재해야** 한다(미존재 시 실패)

`concurrency`로 같은 태그의 동시 실행을 직렬화한다(`cancel-in-progress: false`).

### Job 흐름

```
validate ──> quality ──┐
   │                    ├──> build (4 platforms) ──> publish (draft release)
   └────────────────────┘
```

| Job | runner | needs | 내용 |
|---|---|---|---|
| `validate` | ubuntu | — | 태그 해석·형식 검증, 중복 릴리스 차단 |
| `quality` | ubuntu | `validate` | check/clippy/test/fmt (CI와 동일) |
| `build` | matrix | `validate, quality` | 플랫폼별 빌드·패키징·업로드 |
| `publish` | ubuntu | `validate, build` | 체크섬 생성 + 드래프트 릴리스 발행 |

### 1. validate — 릴리스 입력 검증

- 태그 해석: `workflow_dispatch`면 입력값, 브랜치 푸시면 `release/` 제거.
- 형식 검증: 정규식 `^v[0-9]+\.[0-9]+\.[0-9]+([-.+].+)?$` (예: `v0.2.5`, `v1.0.0-rc.1`).
- `workflow_dispatch`일 때 원격에 해당 태그가 존재하는지 확인.
- 동일 이름의 릴리스가 **이미 있으면 실패** — 체크섬만 갱신하려면 Checksum 워크플로우 사용.

### 2. quality — 품질 게이트

`build.yml`과 동일한 4단계(check/clippy/test/fmt). `validate` 통과 후에만 실행되어
빌드·패키징 비용을 낭비하지 않는다. `rustfmt` 검사도 여기서 한 번만 수행한다.

### 3. build — 플랫폼별 빌드 매트릭스

`fail-fast: false` (한 플랫폼 실패가 나머지를 중단시키지 않음).

| name | runner | 산출물 | 패키징 스크립트 |
|---|---|---|---|
| `windows-x64` | `windows-latest` | `run_config_manager-windows-x64.msi` | `scripts/build-windows-installer.ps1` (WiX MSI) |
| `linux-x64` | `ubuntu-latest` | `run_config_manager-linux-x64.tar.gz` | `scripts/package-linux.sh` |
| `macos-x64` | `macos-15-intel` | `run_config_manager-macos-x64.dmg` | `build_macos_app.sh` + `scripts/package-macos.sh` |
| `macos-arm64` | `macos-latest` | `run_config_manager-macos-arm64.dmg` | 동일 |

각 job 공통:
- `cargo check --locked` → `cargo build --release --locked`
- Linux는 패키징 후 `desktop-file-validate`와 `tar` 내용으로 `.desktop`/아이콘/설치
  스크립트 포함 여부를 검증한다.
- 산출물을 `upload-artifact`로 업로드(`if-no-files-found: error`).

### 4. publish — 드래프트 릴리스 발행

- 모든 빌드 산출물을 `dist/`로 다운로드(`merge-multiple: true`).
- `sha256sum * > checksums.txt`로 체크섬 생성.
- `gh release create`로 발행:
  ```bash
  gh release create "$TAG_NAME" dist/* \
    --title "$TAG_NAME" \
    --generate-notes \
    --draft \
    --target "$TARGET_REF"
  ```
- **릴리스는 `--draft`로 생성된다.** 자동 게시되지 않으므로 검토 후 GitHub UI에서
  수동으로 Publish 해야 한다.

## 릴리스 수행 절차 (권장)

```bash
# 1. 버전 갱신: Cargo.toml의 version을 올린다 (예: 0.2.4 → 0.2.5)
#    Cargo.lock도 함께 갱신되도록 빌드 후 커밋
cargo build
git add Cargo.toml Cargo.lock
git commit -m "chore: [TICKET] bump version to v0.2.5"

# 2. develop에 머지 (PR → Build CI 통과 확인)

# 3. 릴리스 브랜치 푸시 → Release 워크플로우 자동 시작
git switch -c release/v0.2.5
git push origin release/v0.2.5

# 4. Actions에서 validate → quality → build → publish 통과 확인

# 5. GitHub Releases에서 생성된 드래프트 검토 → Publish
#    (자동 생성된 릴리스 노트 보완, 산출물/checksums.txt 확인)
```

> **태그는 미리 만들 필요가 없다.** 브랜치 푸시 경로에서는 `validate`가 태그 존재를
> 검사하지 않고, `publish`의 `gh release create "$TAG_NAME" --target "$TARGET_REF"`가
> 태그가 없으면 **release 브랜치의 HEAD 커밋에 태그를 자동 생성**한다. 즉 브랜치 푸시 →
> (태그 자동 생성 + 드래프트 릴리스 생성)이 한 번에 일어난다. 따라서 푸시 전에 그 브랜치
> HEAD가 릴리스할 정확한 커밋인지 확인한다. 반대로 수동 `workflow_dispatch` 경로는 태그가
> **이미 존재해야** 한다(아래 참고).

### 수동 폴백 (workflow_dispatch)

이미 태그가 존재하는데 릴리스만 다시 만들고 싶을 때:

1. 대상 태그를 먼저 푸시(`git tag v0.2.5 && git push origin v0.2.5`).
2. Actions → **Release** → **Run workflow** → `tag`에 `v0.2.5` 입력.

## 체크섬 갱신 — Checksum (`checksum.yml`)

산출물을 교체했거나 `checksums.txt`만 다시 만들어야 할 때 사용한다.

1. Actions → **Checksum** → **Run workflow** → `tag` 입력(예: `v0.2.5`).
2. 해당 릴리스의 `run_config_manager-*` 자산을 내려받아 `sha256sum`으로 재계산.
3. `gh release upload ... --clobber`로 `checksums.txt`를 덮어쓴다.

## 인앱 업데이트 체커와의 관계

앱은 시작 시(및 상태바 버전 클릭 시) GitHub Releases API의
`/repos/devsepnine/debug_configuration/releases/latest`를 조회해 현재 버전과 비교한다
(구현: `src/services/update_check.rs`).

> **중요**: `/releases/latest`는 **드래프트와 프리릴리스를 제외**한 최신 정식
> 릴리스를 반환한다. 따라서 publish 단계에서 만든 드래프트를 **수동으로 게시**하기
> 전까지는 사용자에게 업데이트 알림이 뜨지 않는다. 또한 태그가 `vX.Y.Z` 형식이어야
> 버전 비교(`parse_version`)가 정상 동작한다(프리릴리스 접미사는 패치에서 무시됨).

## 설계 메모

- **품질 게이트 분리**: 릴리스는 `validate → quality`를 먼저 통과해야 빌드가 시작되어,
  포맷/클리피 실패로 인한 멀티플랫폼 빌드 낭비를 막는다. 플랫폼 `build` job에는
  OS 무관한 `fmt` 검사를 중복하지 않고 `check`만 둔다.
- **드래프트 우선**: 사람이 검토 후 게시하므로, 잘못된 산출물이 즉시 공개되지 않는다.
- **SHA 핀**: 액션 버전을 commit SHA로 고정해 공급망 변조에 대비한다(주기적 갱신 필요).

## 남은 개선 작업

코드 서명(Windows/macOS), notarization, AppImage/Flatpak, `cargo audit` 등
릴리스 패키징 후속 작업은 [`RELEASE_TODO.md`](./RELEASE_TODO.md)에서 추적한다.
