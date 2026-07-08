# Run/Debug Configuration Manager - RUNBOOK

> 운영 및 배포 가이드
> Last Updated: 2026-06-25

## 배포 절차

> **정식 배포는 GitHub Actions로 수행한다.** 릴리스 파이프라인(트리거, job 흐름,
> 산출물, 릴리스 수행 절차)은 [`DEPLOYMENT.md`](./DEPLOYMENT.md)를 참고한다.
> 아래 절차는 **로컬 개발/수동 빌드**용 참고 자료다.

### 1. 릴리스 빌드 (로컬)

#### Windows
```bash
# 릴리스 빌드
cargo build --release

# 실행 파일 위치
# target/release/run_config_manager.exe

# 의존성 확인
ldd target/release/run_config_manager.exe  # Unix
# Windows는 실행 파일이 독립적으로 동작
```

#### macOS
```bash
# 앱 번들 생성
./build_macos_app.sh

# 결과물
# run_config_manager.app/
```

#### Linux
```bash
# 릴리스 빌드
cargo build --release

# 실행 파일 위치
# target/release/run_config_manager

# 권한 설정
chmod +x target/release/run_config_manager
```

### 2. 배포 전 체크리스트

- [ ] `cargo test` 모든 테스트 통과
- [ ] `cargo clippy` 경고 없음
- [ ] `cargo build --release` 빌드 성공
- [ ] 각 플랫폼에서 실행 테스트
- [ ] 구성 저장/로드 정상 동작 확인
- [ ] 프로세스 실행 및 종료 정상 동작 확인
- [ ] 버전 번호 업데이트 (Cargo.toml)

### 3. 릴리스 패키징

#### Windows
```bash
# 실행 파일 + assets + fonts 번들
mkdir release
cp target/release/run_config_manager.exe release/
cp -r assets release/
cp -r fonts release/
zip -r run_config_manager-windows-v0.1.0.zip release/
```

#### macOS
```bash
# 앱 번들 압축
zip -r run_config_manager-macos-v0.1.0.zip run_config_manager.app
```

#### Linux
```bash
# AppImage 또는 tar.gz
tar -czf run_config_manager-linux-v0.1.0.tar.gz target/release/run_config_manager assets/ fonts/
```

## 모니터링 및 알림

### 로그 수집

현재 애플리케이션은 파일 로깅을 하지 않습니다. 필요 시 다음을 추가:

```rust
// 추가 의존성
// env_logger = "0.11"
// log = "0.4"

// main.rs
use env_logger;
use log::{info, error, warn};

fn main() {
    env_logger::init();
    info!("Application starting...");
    // ...
}
```

### 성능 메트릭

모니터링할 주요 메트릭:
- **메모리 사용량**: 세션 수에 비례하여 증가
- **CPU 사용량**: 프로세스 실행 시 스파이크
- **프로세스 수**: 실행 중인 세션 수
- **디스크 I/O**: 구성 저장/로드 시

### 헬스 체크

```bash
# 프로세스 실행 확인
ps aux | grep run_config_manager

# Windows
tasklist | findstr run_config_manager
```

## 일반적인 문제 및 해결 방법

### 0. Windows 터미널 출력 진단 (`RCM_PTY_DUMP`)

**용도**: Windows ConPTY 세션의 출력이 이상하게 렌더될 때, conhost가 실제로
내보내는 원시 바이트를 확인합니다.

```powershell
$env:RCM_PTY_DUMP = "1"
.\run_config_manager.exe   # 또는 cargo run --release
# 세션 실행 후:
Get-Content $env:TEMP\rcm-pty-dump-*.txt
```

**주의**:
- 세션 프로세스의 **모든 출력이 평문으로** `%TEMP%`에 저장됩니다 — 비밀값을
  다루는 세션에는 켜지 마세요. 확인 후 파일을 삭제하세요.
- 파일 크기 상한이 없으므로 장시간/대량 출력 세션에서는 사용하지 마세요.
- 진단 전용 플래그로, 평상시에는 설정하지 않습니다.

### 1. 앱이 시작되지 않음

**증상**: 실행 파일을 더블클릭해도 아무 일도 일어나지 않음

**원인**:
- 그래픽 드라이버 문제
- 폰트 파일 누락
- 권한 문제

**해결**:
```bash
# 터미널에서 직접 실행하여 에러 메시지 확인
./run_config_manager

# 폰트 파일 확인 (참고: 폰트는 바이너리에 임베드되므로 런타임에 이 파일은 필요 없음)
ls fonts/D2Coding.ttf

# 권한 확인
chmod +x run_config_manager
```

### 2. 구성 파일이 저장되지 않음

**증상**: 구성을 추가했지만 앱 재시작 시 사라짐

**원인**:
- 설정 디렉토리 권한 문제
- 디스크 공간 부족
- 파일 시스템 에러

**해결**:
```bash
# 설정 디렉토리 확인
# Windows
echo %APPDATA%\run_config_manager
dir %APPDATA%\run_config_manager

# Linux/macOS
ls -la ~/.config/run_config_manager/

# 권한 수정
chmod 755 ~/.config/run_config_manager/
chmod 644 ~/.config/run_config_manager/configs.json

# 디스크 공간 확인
df -h  # Unix
```

### 3. 프로세스가 종료되지 않음

**증상**: 앱을 닫았지만 실행한 프로세스가 계속 동작함

**원인**:
- Drop trait 실행 실패
- Graceful shutdown 타임아웃
- 자식 프로세스가 데몬으로 전환

**해결**:
```bash
# Windows
tasklist | findstr <프로세스명>
taskkill /F /PID <PID>

# Unix
ps aux | grep <프로세스명>
kill -9 <PID>

# 앱 로그 확인 (향후 추가 예정)
# 프로세스 종료 로직에 문제가 있을 수 있음
```

### 4. NPM 스크립트가 실행되지 않음

**증상**: NPM 구성 실행 시 에러 발생

**원인**:
- Node.js가 설치되지 않음
- package.json이 없음
- 패키지 매니저 감지 실패

**해결**:
```bash
# Node.js 설치 확인
node --version
npm --version

# package.json 확인
cat package.json

# 수동으로 명령어 테스트
npm run dev

# 패키지 매니저 lockfile 확인
ls -la package-lock.json yarn.lock pnpm-lock.yaml
```

### 5. 파일 다이얼로그가 느림

**증상**: 디렉토리 선택 시 오래 걸림

**원인**:
- 대용량 디렉토리 스캔
- package.json 재귀 탐색 깊이

**해결**:
- 더 작은 디렉토리 선택
- node_modules가 많은 경로 피하기
- 현재 깊이 제한: 5 레벨 (utils.rs에서 조정 가능)

### 6. PowerShell 경로 파싱 에러

**증상**: `Unexpected token` 에러

**원인**:
- 공백 포함 경로 따옴표 처리 누락

**해결**:
- 이미 수정됨 (call operator `&` 추가)
- 여전히 문제 발생 시: 경로에 특수문자 확인

### 7. 업데이트 알림이 뜨지 않음

**증상**: 새 릴리스를 올렸는데 앱 상태바/알림에 업데이트가 표시되지 않음

**원인**:
- 릴리스가 아직 **드래프트**이거나 **프리릴리스**로 표시됨 (`/releases/latest`가 제외)
- 태그가 `vX.Y.Z` 형식이 아님 (버전 비교 실패)
- 네트워크 차단/오프라인 (`api.github.com` 접근 불가) — 비치명적으로 처리되어 상태바에만 표시

**해결**:
```bash
# 최신 정식 릴리스가 무엇으로 보이는지 직접 확인
curl -s https://api.github.com/repos/devsepnine/debug_configuration/releases/latest | grep tag_name

# 드래프트면 GitHub UI에서 Publish, 프리릴리스 체크 해제
# 상태바 버전 라벨을 클릭하면 수동 재확인 가능
```

자세한 내용은 [`DEPLOYMENT.md`](./DEPLOYMENT.md)의 "인앱 업데이트 체커와의 관계" 참고.

## Rollback 절차

### 1. 이전 버전으로 복구

```bash
# Git 태그로 이전 버전 체크아웃
git checkout v0.0.9

# 빌드
cargo build --release

# 실행 파일 교체
cp target/release/run_config_manager /path/to/deployment/
```

### 2. 구성 파일 복구

```bash
# 백업본 복원
cp configs.json.backup configs.json

# Windows
copy %APPDATA%\run_config_manager\configs.json.backup %APPDATA%\run_config_manager\configs.json

# Linux/macOS
cp ~/.config/run_config_manager/configs.json.backup ~/.config/run_config_manager/configs.json
```

### 3. 의존성 이슈 해결

```bash
# Cargo.lock 복원
git checkout Cargo.lock

# 의존성 재설치
cargo clean
cargo build --release
```

## 성능 튜닝

### 메모리 사용량 최적화

현재 구현:
- 세션당 출력 버퍼 무제한 증가 (메모리 누수 위험)

개선 방안:
```rust
// session.rs에 추가
const MAX_OUTPUT_LINES: usize = 10000;

impl ExecutionSession {
    pub fn append_output(&mut self, text: String) {
        self.output.push_str(&text);

        // 라인 수 제한
        let lines: Vec<&str> = self.output.lines().collect();
        if lines.len() > MAX_OUTPUT_LINES {
            self.output = lines[lines.len() - MAX_OUTPUT_LINES..].join("\n");
        }
    }
}
```

### 초기화 속도 개선

현재 구현:
- Node Runtime 감지: 비동기 (✅)
- package.json 스캔: 비동기 (✅)
- 구성 파일 로드: 비동기 (✅)

추가 최적화:
- 캐싱: Node Runtime 목록을 로컬 파일에 캐시
- 지연 로드: 사용자가 NPM 구성 편집 시에만 스캔

### 프로세스 실행 성능

현재 구현:
- CREATE_NO_WINDOW 플래그 사용 (✅)
- 스트림 비동기 읽기 (✅)

추가 최적화:
- 출력 버퍼링: 100ms마다 UI 업데이트 (현재는 실시간)

## 보안 고려사항

### 1. 명령어 인젝션 방지

**현재 구현**: 사용자 입력을 그대로 셸에 전달
**위험**: 악의적인 명령어 실행 가능

**권장 사항**:
- 명령어 검증 추가
- 허용된 명령어 화이트리스트
- 셸 이스케이프 처리

### 2. 환경 변수 보안

**현재 구현**: 환경 변수를 평문으로 저장
**위험**: API 키, 비밀번호 노출

**권장 사항**:
- 민감한 환경 변수 암호화
- 시스템 키링 통합 (keyring 크레이트)
- 경고 메시지 표시

### 3. 파일 시스템 접근

**현재 구현**: 임의 경로 접근 가능
**위험**: 시스템 파일 변조

**권장 사항**:
- 작업 디렉토리 제한
- 상대 경로 검증
- 심볼릭 링크 따라가기 방지

## 디버깅 가이드

### 1. 로그 레벨 설정

```bash
# 향후 구현 시
RUST_LOG=debug cargo run

# 특정 모듈만
RUST_LOG=run_config_manager::executor=trace cargo run
```

### 2. 백트레이스 활성화

```bash
RUST_BACKTRACE=1 cargo run
RUST_BACKTRACE=full cargo run
```

### 3. 프로파일링

```bash
# CPU 프로파일링
cargo flamegraph

# 메모리 프로파일링
cargo install dhat
# dhat 크레이트 추가 후 사용
```

### 4. 빌드 최적화 디버그

```toml
# Cargo.toml에 추가
[profile.dev]
opt-level = 0  # 최적화 없음
debug = true   # 디버그 심볼 포함

[profile.release]
opt-level = 3      # 최대 최적화
debug = false      # 디버그 심볼 제거
lto = true         # Link-time optimization
codegen-units = 1  # 단일 코드 생성 유닛
```

## 연락처 및 지원

- **GitHub Issues**: [프로젝트 이슈 페이지]
- **이메일**: [유지보수 담당자 이메일]
- **문서**: README.md, docs/README.md

## 변경 이력

### v0.1.0 (2026-01-29)
- NPM Configuration 타입 추가
- 비동기 초기화 개선
- PowerShell 경로 처리 수정
- 성능 최적화

### v0.0.9 (2026-01-21)
- Shell Script Configuration 추가
- Persistent AppSettings
- 폴더 브라우저 구현
