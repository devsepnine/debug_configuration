use crate::messages::Message;
use crate::models::{
    ConfigTypeData, ExecuteMode, KotlinLaunchMode, NodeCommand, OutputEvent, PackageManager,
    RunConfiguration,
};
use iced::stream;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::sync::{
    Arc, LazyLock, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio::process::{Child, ChildStderr, ChildStdout};
use tokio::time::{Instant, sleep_until};
use uuid::Uuid;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 현재 실행 중인 자식 프로세스들의 PID 레지스트리.
///
/// 시그널 핸들러(Ctrl+C)는 앱 상태(`RunConfigManager`)에 접근할 수 없고
/// `std::process::exit`는 `Drop`을 실행하지 않으므로, 시그널 경로에서 자식
/// 프로세스를 정리하려면 전역 레지스트리에서 PID를 읽어 강제 종료해야 한다.
static RUNNING_PIDS: LazyLock<Mutex<HashSet<u32>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

/// 실행 시작된 자식 프로세스 PID 등록 (`ProcessStarted` 처리 시 호출).
pub fn register_running_pid(pid: u32) {
    if let Ok(mut pids) = RUNNING_PIDS.lock() {
        pids.insert(pid);
    }
}

/// 종료된 자식 프로세스 PID 등록 해제 (`RunCompleted` 처리 시 호출).
pub fn unregister_running_pid(pid: u32) {
    if let Ok(mut pids) = RUNNING_PIDS.lock() {
        pids.remove(&pid);
    }
}

/// 등록된 모든 실행 중 프로세스를 강제 종료 (시그널 핸들러 전용).
pub fn kill_all_running_processes() {
    let pids: Vec<u32> = RUNNING_PIDS
        .lock()
        .map(|pids| pids.iter().copied().collect())
        .unwrap_or_default();
    for pid in pids {
        force_kill_process_tree(pid);
    }
}

/// 프로세스 그룹에 시그널을 보낸다 — 단, **자기 그룹 사살 방지 런타임 가드** 포함.
/// pid==pgid(그룹 리더) 전제는 pipe 경로의 `process_group(0)`과 PTY 경로의
/// portable-pty `setsid()`(소스 검증됨)가 보장하지만, 향후 의존성 회귀로 전제가
/// 깨지면 killpg가 앱 자신의 그룹(=앱+모든 세션)을 죽일 수 있다. 가드가 성립하지
/// 않으면 단일 프로세스 kill로 폴백한다 (debug_assert는 release에서 소거되므로 부적합).
#[cfg(unix)]
fn signal_process_group(pid: u32, signal: i32) {
    unsafe {
        let pgid = libc::getpgid(pid as i32);
        if pgid == pid as i32 && pgid != libc::getpgid(0) {
            libc::killpg(pid as i32, signal);
        } else {
            libc::kill(pid as i32, signal);
        }
    }
}

/// PID로 프로세스 트리를 강제 종료 (비동기 컨텍스트 밖에서도 호출 가능).
fn force_kill_process_tree(pid: u32) {
    #[cfg(unix)]
    {
        // 그룹 리더 전제(process_group(0)/setsid) 하에 그룹 전체 SIGKILL — 가드 포함.
        signal_process_group(pid, libc::SIGKILL);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
    }
}

// ---- 세션 I/O 레지스트리 (PTY master: resize / stdin writer) --------------------
//
// 스트림 태스크가 spawn 직후 등록하고 `CompletionGuard::drop`이 무조건 해제한다 —
// rerun의 세션 id 스왑(app.rs)은 옛 스트림이 옛 id를 정리하므로 앱측 관리가 불필요하고,
// 패닉 경로에서도 guard가 정리한다. pipe 세션은 master=None(resize no-op).

/// 세션 stdin으로 쓸 수 있는 핸들. PTY는 blocking `Write`(spawn_blocking에서 사용),
/// Windows pipe는 tokio `ChildStdin`(AsyncWrite) — 통합 blocking 타입이 불가능해 분기.
// TODO(v0.5.0 슬라이스4): write_session_stdin이 소비 — 그 전까지 필드 미사용 allow.
#[allow(dead_code)]
enum SessionWriter {
    /// PTY master writer — 라인 디시플린이 에코를 담당한다 (앱 에코 금지).
    Pty(Arc<Mutex<Box<dyn std::io::Write + Send>>>),
    /// Windows pipe stdin — 에코가 없으므로 제출 시 앱이 로컬 에코한다.
    #[cfg(windows)]
    Pipe(Arc<tokio::sync::Mutex<tokio::process::ChildStdin>>),
}

/// 세션 하나의 프로세스 I/O 핸들 묶음.
struct SessionIo {
    /// PTY master (resize용). pipe 세션은 None.
    master: Option<Box<dyn portable_pty::MasterPty + Send>>,
    /// stdin writer. unix pipe 폴백(출력 전용)은 None.
    // TODO(v0.5.0 슬라이스4): write_session_stdin이 소비 — 그 전까지 임시 allow.
    #[allow(dead_code)]
    writer: Option<SessionWriter>,
}

static SESSION_IO: LazyLock<Mutex<HashMap<Uuid, SessionIo>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn register_session_io(session_id: Uuid, io: SessionIo) {
    if let Ok(mut map) = SESSION_IO.lock() {
        map.insert(session_id, io);
    }
}

fn unregister_session_io(session_id: Uuid) {
    if let Ok(mut map) = SESSION_IO.lock() {
        map.remove(&session_id);
    }
}

/// 세션 PTY의 화면 크기를 갱신한다. pipe 세션/미등록 세션은 no-op.
// TODO(v0.5.0 슬라이스5): 뷰포트 배선에서 호출 — 그 전까지 임시 allow.
#[allow(dead_code)]
pub fn resize_session_pty(session_id: Uuid, cols: u16, rows: u16) {
    if let Ok(map) = SESSION_IO.lock()
        && let Some(io) = map.get(&session_id)
        && let Some(master) = &io.master
    {
        let _ = master.resize(portable_pty::PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
    }
}

/// 경로에 공백이 포함되어 있으면 따옴표로 감싸기
fn quote_if_needed(path: &str) -> String {
    if path.contains(' ') {
        format!("\"{path}\"")
    } else {
        path.to_string()
    }
}

/// 구성을 실행하고 출력을 스트림으로 반환
///
/// 프로세스를 비동기로 실행하고 stdout/stderr 출력을 실시간으로 메시지로 전달
/// Unix 시스템에서는 프로세스 그룹을 생성하여 자식 프로세스 관리
///
/// # Arguments
/// * `config` - 실행할 구성
/// * `session_id` - 세션 식별자
/// * `cancel_flag` - 프로세스 취소를 위한 플래그
///
/// # Returns
/// 실행 중 발생하는 이벤트를 담은 Message 스트림
/// 스트림 태스크가 RunCompleted를 보내지 못한 채 종료되면(패닉 unwind 등) 세션이
/// `is_running` 상태로 영구 고착되고 종료 알림/exit code도 영영 오지 않는다. 이 가드는
/// 정상 종료 시 `disarm`되며, armed인 채로 drop되면 종료 보장용 RunCompleted(Err)를
/// best-effort(try_send)로 발행한다.
struct CompletionGuard {
    output: iced::futures::channel::mpsc::Sender<Message>,
    session_id: Uuid,
    armed: bool,
}

impl CompletionGuard {
    fn new(output: iced::futures::channel::mpsc::Sender<Message>, session_id: Uuid) -> Self {
        Self {
            output,
            session_id,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for CompletionGuard {
    fn drop(&mut self) {
        // 세션 I/O 핸들(PTY master/stdin writer)은 스트림 수명과 함께 무조건 정리한다
        // (armed 여부 무관 — 정상 종료·취소·패닉 전 경로). master drop이 PTY를 닫아
        // 아직 살아 있는 reader 스레드도 EOF로 풀려난다.
        unregister_session_io(self.session_id);
        if self.armed {
            let _ = self.output.try_send(Message::RunCompleted(
                self.session_id,
                Err(String::from("Run interrupted unexpectedly")),
            ));
        }
    }
}

pub fn run_configuration_stream(
    config: RunConfiguration,
    session_id: Uuid,
    cancel_flag: Arc<AtomicBool>,
    show_env: bool,
) -> impl iced::futures::Stream<Item = Message> {
    stream::channel(
        100,
        move |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
            // 종료 보장 가드: 정상 경로의 끝에서 disarm한다.
            let mut completion = CompletionGuard::new(output.clone(), session_id);

            let (command_str, extra_env) = build_command(&config);
            send_command_info(
                &mut output,
                session_id,
                &config,
                &command_str,
                &extra_env,
                show_env,
            )
            .await;

            // unix 기본 경로: PTY — isatty=true로 색·진행바·프롬프트가 살아난다.
            // openpty 실패(fd 고갈 등 희귀)만 pipe로 폴백하고, spawn 실패는 명령 문제라
            // 폴백 없이 그대로 보고한다. Windows는 pipe 경로 고정(ConPTY는 후속).
            #[cfg(unix)]
            {
                match spawn_in_pty(&config, &command_str, &extra_env, session_id, (120, 40)) {
                    Ok(pty) => {
                        handle_pty_process(&mut output, session_id, pty, cancel_flag).await;
                        completion.disarm();
                        return;
                    }
                    Err(PtySpawnError::Spawn(e)) => {
                        send_run_result(&mut output, session_id, Err(e)).await;
                        completion.disarm();
                        return;
                    }
                    Err(PtySpawnError::PtyUnavailable(e)) => {
                        // 출력 전용 pipe 폴백 — stdin/라이브 진행바 없음(문서화된 한계).
                        use iced::futures::SinkExt;
                        let _ = output
                            .send(Message::OutputReceived(
                                session_id,
                                vec![OutputEvent::Line(format!(
                                    "[pty unavailable ({e}); falling back to pipes — \
                                     stdin input is disabled for this run]"
                                ))],
                            ))
                            .await;
                    }
                }
            }

            let mut cmd = create_process_command(&config, &command_str, &extra_env);
            match cmd.spawn() {
                Ok(child) => {
                    handle_spawned_process(&mut output, session_id, child, cancel_flag).await;
                }
                Err(e) => {
                    send_run_result(
                        &mut output,
                        session_id,
                        Err(format!("Failed to spawn process: {e}")),
                    )
                    .await;
                }
            }

            completion.disarm();
        },
    )
}

/// PowerShell 실행 파일 탐지(LazyLock)를 미리 초기화한다. 이 탐지는 한 번 blocking
/// subprocess(pwsh 스폰+대기)를 수행하므로, 앱 시작 시 메인 스레드에서 미리 호출해
/// 두면 첫 구성 실행이 tokio 워커에서 블로킹되는 것을 방지한다. Windows 외에는 no-op.
pub fn prewarm_shell_detection() {
    #[cfg(windows)]
    {
        let _ = windows_powershell_exe();
    }
}

/// 구성으로부터 셸에서 실행할 명령 문자열과, 프로세스에 직접 주입할 환경변수 목록을 생성.
///
/// 환경변수는 셸 명령 문자열에 보간하지 않고 `Command::env`로 전달한다. 이렇게 하면
/// 따옴표 이스케이프 누락이나 셸 메타문자 재해석으로 인한 명령 주입을 원천 차단할 수 있다.
fn build_command(config: &RunConfiguration) -> (String, Vec<(String, String)>) {
    match &config.type_data {
        ConfigTypeData::Application { command, arguments } => {
            (build_application_command(command, arguments), Vec::new())
        }
        ConfigTypeData::ShellScript { execute_mode } => {
            (build_shell_script_command(execute_mode), Vec::new())
        }
        ConfigTypeData::Node {
            project_directory: _,
            package_manager,
            node_runtime_path,
            command,
            script_name,
            arguments,
            node_options,
        } => {
            let command_str = build_node_command(NodeCommandParts {
                package_manager,
                node_runtime_path: node_runtime_path.as_ref(),
                command: *command,
                script_name: script_name.as_ref(),
                arguments,
                working_directory: &config.working_directory,
            });
            // NODE_OPTIONS도 셸 보간 대신 환경변수로 전달.
            let extra_env = if node_options.is_empty() {
                Vec::new()
            } else {
                vec![(String::from("NODE_OPTIONS"), node_options.clone())]
            };
            (command_str, extra_env)
        }
        ConfigTypeData::Kotlin {
            jdk_path,
            launch_mode,
            vm_options,
            program_arguments,
        } => {
            let command_str = build_kotlin_command(KotlinCommandParts {
                jdk_path: jdk_path.as_ref(),
                launch_mode,
                vm_options,
                program_arguments,
            });
            // VM options는 명령줄에 직접 노출하므로 별도 환경변수 불필요.
            (command_str, Vec::new())
        }
        // Compound 구성은 셸 명령이 없다 — app.rs가 멤버별로 펼쳐 실행하므로
        // 이 스트림 경로에는 도달하지 않는다 (방어적으로 빈 명령 반환).
        ConfigTypeData::Compound { .. } => (String::new(), Vec::new()),
    }
}

/// Windows에서 사용할 PowerShell 실행 파일을 결정한다 (세션당 1회 감지 후 캐시).
///
/// PowerShell 7+(`pwsh`)는 `&&`/`||` 체이닝을 네이티브 지원하므로 우선 사용하고,
/// 설치되어 있지 않으면 Windows PowerShell 5.x(`powershell`)로 폴백한다.
#[cfg(windows)]
fn windows_powershell_exe() -> &'static str {
    use std::os::windows::process::CommandExt;

    static EXE: LazyLock<&'static str> = LazyLock::new(|| {
        let pwsh_available = std::process::Command::new("pwsh")
            .args(["-NoProfile", "-Command", "exit"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false);

        if pwsh_available { "pwsh" } else { "powershell" }
    });

    *EXE
}

/// Windows PowerShell 5.x는 `&&`를 지원하지 않으므로 `;`(순차 실행)로 대체한다.
/// 단락 평가(앞 명령 실패 시 중단)는 보존되지 않는 최선의 폴백이며, pwsh 7이 감지되면
/// 이 변환은 호출되지 않는다.
#[cfg(windows)]
fn rewrite_chaining_for_legacy_powershell(command_str: &str) -> String {
    command_str.replace(" && ", "; ")
}

fn create_process_command(
    config: &RunConfiguration,
    command_str: &str,
    extra_env: &[(String, String)],
) -> tokio::process::Command {
    use std::process::Stdio;
    use tokio::process::Command;

    #[cfg(windows)]
    let mut cmd = {
        let exe = windows_powershell_exe();
        // pwsh(7+)는 && 를 네이티브 지원하므로 변환하지 않는다. 5.x 폴백 시에만 변환.
        let adapted = if exe == "pwsh" {
            command_str.to_string()
        } else {
            rewrite_chaining_for_legacy_powershell(command_str)
        };
        let mut c = Command::new(exe);
        // -NonInteractive 미사용: stdin pipe로 입력을 받을 수 있게 한다 (stdin 입력바).
        // 예상치 못한 cmdlet 프롬프트가 조용히 대기할 수 있는 트레이드오프는 릴리스노트 명시.
        c.args([
            "-NoProfile",
            "-Command",
            &format!("[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; {adapted}"),
        ]);
        c.creation_flags(CREATE_NO_WINDOW);
        c
    };

    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = Command::new("sh");
        c.args(["-l", "-c", command_str]);
        c
    };

    #[cfg(unix)]
    {
        cmd.process_group(0);
    }

    cmd.current_dir(&config.working_directory);
    // 환경변수는 셸 문자열이 아닌 프로세스 환경으로 직접 주입 (이스케이프/주입 방지).
    cmd.envs(config.environment_variables.iter());
    cmd.envs(extra_env.iter().map(|(k, v)| (k, v)));
    // stdin: Windows는 pipe(입력바 지원), unix pipe 경로는 폴백 전용이라 null(출력 전용).
    #[cfg(windows)]
    cmd.stdin(Stdio::piped());
    #[cfg(not(windows))]
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd
}

// ---- PTY 스폰 (unix 기본 경로) --------------------------------------------------

/// PTY에서 돌릴 명령을 구성한다. pipe 경로와 동일한 셸 래핑(sh -l -c) + 환경변수
/// 직접 주입 불변식에, 진짜 터미널임을 알리는 TERM을 더한다.
#[cfg(unix)]
fn create_pty_command(
    config: &RunConfiguration,
    command_str: &str,
    extra_env: &[(String, String)],
) -> portable_pty::CommandBuilder {
    let mut cmd = portable_pty::CommandBuilder::new("sh");
    cmd.args(["-l", "-c", command_str]);
    cmd.cwd(&config.working_directory);
    cmd.env("TERM", "xterm-256color");
    for (key, value) in &config.environment_variables {
        cmd.env(key, value);
    }
    for (key, value) in extra_env {
        cmd.env(key, value);
    }
    cmd
}

/// PTY 스폰 결과 — 스트림 태스크가 소비할 채널들과 kill용 pid.
#[cfg(unix)]
struct PtyProcess {
    pid: Option<u32>,
    chunks_rx: tokio::sync::mpsc::Receiver<Vec<u8>>,
    exit_rx: tokio::sync::oneshot::Receiver<Result<i32, String>>,
}

/// PTY reader 스레드: blocking read → bounded 채널로 전달.
/// `blocking_send`(드롭 아님)라 UI가 밀리면 커널 pty 버퍼→자식 write 블록으로
/// 배압이 전파된다 — 진짜 터미널과 동일한 시맨틱, 메모리 상수 유지.
/// 종료: EOF/EIO(자식 종료 후 slave 닫힘) 또는 수신측 drop(스트림 태스크 종료).
#[cfg(unix)]
fn pty_reader_thread(
    mut reader: Box<dyn std::io::Read + Send>,
    tx: tokio::sync::mpsc::Sender<Vec<u8>>,
) {
    let mut buf = [0u8; 8192];
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if tx.blocking_send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        }
    }
}

/// PTY를 열고 명령을 스폰한다. 성공 시 세션 I/O(마스터·writer)를 레지스트리에 등록하고
/// reader 스레드·waiter(blocking wait 소유)를 기동한다.
///
/// 에러 구분: `openpty` 실패는 `Err(PtyUnavailable)`로 돌려 pipe 폴백을 허용하고,
/// spawn 실패는 명령 문제이므로 폴백 없이 사용자에게 그대로 보고한다.
#[cfg(unix)]
enum PtySpawnError {
    PtyUnavailable(String),
    Spawn(String),
}

#[cfg(unix)]
fn spawn_in_pty(
    config: &RunConfiguration,
    command_str: &str,
    extra_env: &[(String, String)],
    session_id: Uuid,
    viewport: (u16, u16),
) -> Result<PtyProcess, PtySpawnError> {
    use portable_pty::{PtySize, native_pty_system};

    let pair = native_pty_system()
        .openpty(PtySize {
            rows: viewport.1,
            cols: viewport.0,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| PtySpawnError::PtyUnavailable(format!("openpty failed: {e}")))?;

    let cmd = create_pty_command(config, command_str, extra_env);
    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| PtySpawnError::Spawn(format!("Failed to spawn process: {e}")))?;
    // slave를 부모에서 즉시 닫아야 자식 종료 시 master reader가 EOF를 받는다.
    drop(pair.slave);

    let pid = child.process_id();

    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| PtySpawnError::Spawn(format!("Failed to open pty reader: {e}")))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|e| PtySpawnError::Spawn(format!("Failed to open pty writer: {e}")))?;

    // ProcessStarted 발송 전에 등록 — 첫 프레임의 resize가 핸들을 찾을 수 있게.
    register_session_io(
        session_id,
        SessionIo {
            master: Some(pair.master),
            writer: Some(SessionWriter::Pty(Arc::new(Mutex::new(writer)))),
        },
    );

    let (chunks_tx, chunks_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
    let _ = std::thread::Builder::new()
        .name(format!("pty-read-{session_id}"))
        .spawn(move || pty_reader_thread(reader, chunks_tx));

    // waiter가 child를 소유하고 blocking wait로 reap한다 (좀비 방지, 스트림 패닉과 무관).
    let (exit_tx, exit_rx) = tokio::sync::oneshot::channel();
    tokio::task::spawn_blocking(move || {
        let result = child
            .wait()
            .map(|status| i32::try_from(status.exit_code()).unwrap_or(i32::MAX))
            .map_err(|e| format!("Failed to wait for process: {e}"));
        let _ = exit_tx.send(result);
    });

    Ok(PtyProcess {
        pid,
        chunks_rx,
        exit_rx,
    })
}

async fn send_command_info(
    output: &mut iced::futures::channel::mpsc::Sender<Message>,
    session_id: Uuid,
    config: &RunConfiguration,
    command_str: &str,
    extra_env: &[(String, String)],
    show_env: bool,
) {
    use iced::futures::SinkExt;

    let mut command_info = String::new();
    command_info.push_str("═══════════════════════════════════════════════════════\n");
    let _ = writeln!(command_info, "Configuration: {}", config.name);
    let _ = writeln!(command_info, "Type: {:?}", config.config_type());
    let _ = writeln!(
        command_info,
        "Working Directory: {}",
        config.working_directory
    );
    // 환경변수는 더 이상 Command 문자열에 보이지 않으므로 별도 라인으로 표시해 가시성 유지.
    // 단, 설정에서 끄면(show_env=false) 출력하지 않는다.
    if show_env && (!config.environment_variables.is_empty() || !extra_env.is_empty()) {
        let mut pairs: Vec<(String, String)> = config
            .environment_variables
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        pairs.extend(extra_env.iter().cloned());
        let rendered = pairs
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("; ");
        let _ = writeln!(command_info, "Environment: {rendered}");
    }
    let _ = writeln!(command_info, "Command: {command_str}");
    command_info.push_str("═══════════════════════════════════════════════════════\n");
    let _ = output
        .send(Message::OutputReceived(
            session_id,
            lines_to_events(&command_info),
        ))
        .await;
}

/// PTY 세션 파이프라인: ProcessStarted → 조립 루프 → (취소 시 종료 처리) → RunCompleted.
#[cfg(unix)]
async fn handle_pty_process(
    output: &mut iced::futures::channel::mpsc::Sender<Message>,
    session_id: Uuid,
    mut pty: PtyProcess,
    cancel_flag: Arc<AtomicBool>,
) {
    use iced::futures::SinkExt;

    if let Some(pid) = pty.pid {
        let _ = output.send(Message::ProcessStarted(session_id, pid)).await;
    }

    match pty_output_loop(
        output,
        session_id,
        &mut pty.chunks_rx,
        &mut pty.exit_rx,
        &cancel_flag,
    )
    .await
    {
        PtyLoopEnd::Cancelled => {
            terminate_and_reap_pty(pty.pid, &mut pty.exit_rx).await;
            send_run_result(
                output,
                session_id,
                Err(String::from("Process stopped by user")),
            )
            .await;
        }
        PtyLoopEnd::Completed(result) => {
            send_run_result(output, session_id, result).await;
        }
    }
}

/// PTY 조립 루프의 종료 사유.
#[cfg(unix)]
enum PtyLoopEnd {
    Cancelled,
    Completed(Result<i32, String>),
}

/// PTY 출력 조립 루프: 취소(50ms poll)·flush 마감(16ms)·바이트 수신·자식 종료를
/// 단일 select로 처리한다. 종료 코드가 먼저 와도 **EOF까지 드레인**한다 — 손자
/// 프로세스가 pty를 쥐고 있는 동안의 출력을 잃지 않기 위함 (pipe 경로와 동일 시맨틱).
/// 모든 종료 경로는 반환 전에 잔여 배치를 flush한다 (RunCompleted보다 출력이 먼저).
#[cfg(unix)]
async fn pty_output_loop(
    output: &mut iced::futures::channel::mpsc::Sender<Message>,
    session_id: Uuid,
    chunks_rx: &mut tokio::sync::mpsc::Receiver<Vec<u8>>,
    exit_rx: &mut tokio::sync::oneshot::Receiver<Result<i32, String>>,
    cancel_flag: &Arc<AtomicBool>,
) -> PtyLoopEnd {
    let mut assembler = LineAssembler::new();
    let mut batch = EventBatch::default();
    let mut flush_at: Option<Instant> = None;
    let mut exit_status: Option<Result<i32, String>> = None;

    loop {
        tokio::select! {
            biased;

            () = wait_for_cancel(cancel_flag) => {
                assembler.emit_partial(&mut batch);
                flush_event_batch(output, session_id, &mut batch).await;
                return PtyLoopEnd::Cancelled;
            }

            () = wait_flush_deadline(flush_at), if !batch.is_empty() || assembler.has_pending() => {
                assembler.emit_partial(&mut batch);
                flush_event_batch(output, session_id, &mut batch).await;
                flush_at = None;
            }

            chunk = chunks_rx.recv() => {
                match chunk {
                    Some(bytes) => {
                        let was_empty = batch.is_empty() && !assembler.has_pending();
                        assembler.push_bytes(&bytes, &mut batch);
                        if was_empty && (!batch.is_empty() || assembler.has_pending()) {
                            flush_at = Some(Instant::now() + OUTPUT_FLUSH_INTERVAL);
                        }
                    }
                    // EOF: 이 분기의 모든 경로가 return하므로 루프에 eof 상태는 불필요.
                    None => {
                        assembler.finalize(&mut batch);
                        flush_event_batch(output, session_id, &mut batch).await;
                        if let Some(status) = exit_status.take() {
                            return PtyLoopEnd::Completed(status);
                        }
                        // EOF인데 자식이 아직 안 끝남(드묾): 종료 또는 취소 대기.
                        tokio::select! {
                            biased;
                            () = wait_for_cancel(cancel_flag) => return PtyLoopEnd::Cancelled,
                            status = &mut *exit_rx => {
                                return PtyLoopEnd::Completed(status.unwrap_or_else(|_| {
                                    Err(String::from("Process exit status unavailable"))
                                }));
                            }
                        }
                    }
                }
            }

            status = &mut *exit_rx, if exit_status.is_none() => {
                // 종료 코드 기록 후에도 EOF까지 계속 드레인 (손자 프로세스 출력 보존).
                exit_status = Some(status.unwrap_or_else(|_| {
                    Err(String::from("Process exit status unavailable"))
                }));
            }
        }

        // 크기 임계 초과 시 즉시 flush (마감 대기 없이).
        if batch.len() >= OUTPUT_FLUSH_MAX_LINES || batch.bytes >= OUTPUT_FLUSH_MAX_BYTES {
            flush_event_batch(output, session_id, &mut batch).await;
            flush_at = None;
        }
    }
}

/// 이벤트 배치를 한 개의 `OutputReceived`로 전송. 비어 있으면 no-op.
#[cfg(unix)]
async fn flush_event_batch(
    output: &mut iced::futures::channel::mpsc::Sender<Message>,
    session_id: Uuid,
    batch: &mut EventBatch,
) {
    use iced::futures::SinkExt;

    if batch.is_empty() {
        return;
    }
    let events = batch.take();
    let _ = output
        .send(Message::OutputReceived(session_id, events))
        .await;
}

/// PTY 자식 종료: SIGTERM(그룹, 가드 포함) → 2s 대기 → SIGKILL → waiter reap 대기.
/// child는 waiter(spawn_blocking)가 소유하므로 여기서는 pid+exit 채널만 다룬다.
#[cfg(unix)]
async fn terminate_and_reap_pty(
    pid: Option<u32>,
    exit_rx: &mut tokio::sync::oneshot::Receiver<Result<i32, String>>,
) {
    if let Some(pid) = pid {
        signal_process_group(pid, libc::SIGTERM);
    }
    if tokio::time::timeout(Duration::from_secs(2), &mut *exit_rx)
        .await
        .is_err()
    {
        if let Some(pid) = pid {
            signal_process_group(pid, libc::SIGKILL);
        }
        let _ = (&mut *exit_rx).await;
    }
}

async fn handle_spawned_process(
    output: &mut iced::futures::channel::mpsc::Sender<Message>,
    session_id: Uuid,
    mut child: Child,
    cancel_flag: Arc<AtomicBool>,
) {
    send_process_started(output, session_id, &child).await;
    // Windows: stdin pipe를 세션 I/O 레지스트리에 등록 (stdin 입력바 지원).
    #[cfg(windows)]
    if let Some(stdin) = child.stdin.take() {
        register_session_io(
            session_id,
            SessionIo {
                master: None,
                writer: Some(SessionWriter::Pipe(Arc::new(tokio::sync::Mutex::new(
                    stdin,
                )))),
            },
        );
    }
    let (mut stdout_reader, mut stderr_reader) = take_process_streams(&mut child);

    let cancelled = process_output_loop(
        output,
        session_id,
        &mut stdout_reader,
        &mut stderr_reader,
        &cancel_flag,
    )
    .await;

    if cancelled {
        // 시그널 전송 후 반드시 wait로 reap (Unix 좀비 방지) + SIGTERM 무시 시 SIGKILL 에스컬레이션.
        terminate_and_reap(&mut child).await;
        send_run_result(
            output,
            session_id,
            Err(String::from("Process stopped by user")),
        )
        .await;
    } else {
        wait_for_process(output, session_id, &mut child, &cancel_flag).await;
    }
}

async fn send_process_started(
    output: &mut iced::futures::channel::mpsc::Sender<Message>,
    session_id: Uuid,
    child: &Child,
) {
    use iced::futures::SinkExt;

    if let Some(pid) = child.id() {
        let _ = output.send(Message::ProcessStarted(session_id, pid)).await;
    }
}

fn take_process_streams(
    child: &mut Child,
) -> (
    Option<tokio::io::BufReader<ChildStdout>>,
    Option<tokio::io::BufReader<ChildStderr>>,
) {
    use tokio::io::BufReader;

    (
        child.stdout.take().map(BufReader::new),
        child.stderr.take().map(BufReader::new),
    )
}

/// 출력 배치 flush 주기(약 한 프레임). 자식이 로그를 폭주시킬 때 stdout 한 줄마다
/// Message를 보내면 iced UI 스레드가 줄 수만큼 update()+view()+draw()를 돌려 포화되고
/// 입력(스크롤/클릭)이 starvation 된다. 줄을 모아 이 주기마다 한 번에 보내 UI 메시지 비율을
/// 출력량과 무관하게 ~60/s로 묶는다. app 측은 이미 멀티라인 페이로드를 처리한다
/// (handle_output_received가 output.lines() 순회, add_output_line이 '\n' 분할).
/// 줄당 누적 상한. 개행 없이 대량 출력하는 자식(progress bar의 \r-only 출력,
/// `yes | tr -d '\n'` 등)이 버퍼를 무한히 키워 OOM으로 앱 전체가 죽는 것을 막는다.
/// pipe 읽기 루프(read_next_line)와 PTY 조립기(LineAssembler)가 공유한다.
/// 상한 도달 시 개행을 기다리지 않고 현재까지를 한 줄로 확정한다.
const MAX_LINE_BYTES: usize = 1024 * 1024;

const OUTPUT_FLUSH_INTERVAL: Duration = Duration::from_millis(16);
/// 배치가 이 줄 수에 도달하면 타이머를 기다리지 않고 즉시 flush (지연 최소화).
const OUTPUT_FLUSH_MAX_LINES: usize = 64;
/// 배치가 이 바이트에 도달하면 즉시 flush (flush 틱 사이 폭주로 버퍼가 무한히 커지는 것 방지).
const OUTPUT_FLUSH_MAX_BYTES: usize = 16 * 1024;

/// 출력 펌프 루프. 취소되면 `true`, stdout/stderr가 모두 닫혀 정상 종료되면 `false` 반환.
/// 실제 프로세스 종료(kill/reap)는 호출자(`handle_spawned_process`)가 담당한다.
///
/// 줄 단위로 읽되 전송은 배치로 묶어 UI 메시지 폭주를 막는다. 배치 버퍼(`batch`)는 이
/// 태스크에 지역적이고(세션당 스트림 태스크 1개) UI 스레드와 공유되지 않으므로 동기화가
/// 필요 없다. 종료(취소/EOF) 직전에는 반드시 남은 배치를 flush해, 마지막 출력이
/// 호출자가 보내는 RunCompleted(종료 배너)보다 먼저 도착하도록 한다.
async fn process_output_loop<O, E>(
    output: &mut iced::futures::channel::mpsc::Sender<Message>,
    session_id: Uuid,
    stdout_reader: &mut Option<tokio::io::BufReader<O>>,
    stderr_reader: &mut Option<tokio::io::BufReader<E>>,
    cancel_flag: &Arc<AtomicBool>,
) -> bool
where
    O: tokio::io::AsyncRead + Unpin,
    E: tokio::io::AsyncRead + Unpin,
{
    let mut batch = String::new();
    let mut batch_lines: usize = 0;
    // 배치가 비어 있으면 None(타이머 비무장). 첫 줄이 들어올 때 무장한다.
    let mut flush_at: Option<Instant> = None;

    loop {
        tokio::select! {
            // 취소를 최우선 처리(기존 동작 보존). 종료 전 남은 배치를 flush.
            biased;
            () = wait_for_cancel(cancel_flag) => {
                flush_batch(output, session_id, &mut batch, &mut batch_lines).await;
                return true;
            }
            // flush 마감 도달 → 배치 전송. `if !batch.is_empty()`로 idle 시 이 분기를
            // 비활성화해 불필요한 기상/busy-spin을 막고(타이머는 batch 비었을 때 비무장),
            // read 분기를 starve하지 않는다(flush 후 batch가 비면 다음 루프에서 자동 비활성).
            () = wait_flush_deadline(flush_at), if !batch.is_empty() => {
                flush_batch(output, session_id, &mut batch, &mut batch_lines).await;
                flush_at = None;
            }
            // `if .is_some()` 전제조건 필수: reader가 None이면 read_next_line이 즉시
            // Ok(None)을 반환해, biased select가 매 루프마다 이 분기만 선택하고 다른
            // reader를 영영 폴링하지 않는 livelock(한쪽 EOF 후 CPU 100% busy-spin,
            // RunCompleted 미발송)이 발생한다. 전제조건으로 None 분기를 비활성화한다.
            result = read_next_line(stdout_reader), if stdout_reader.is_some() => {
                if accumulate_line(&mut batch, &mut batch_lines, &mut flush_at, result) {
                    *stdout_reader = None;
                }
            }
            result = read_next_line(stderr_reader), if stderr_reader.is_some() => {
                if accumulate_line(&mut batch, &mut batch_lines, &mut flush_at, result) {
                    *stderr_reader = None;
                }
            }
        }

        // 크기 임계 도달 시 타이머를 기다리지 않고 즉시 flush (지연 최소화 + 메모리 상한).
        if batch_lines >= OUTPUT_FLUSH_MAX_LINES || batch.len() >= OUTPUT_FLUSH_MAX_BYTES {
            flush_batch(output, session_id, &mut batch, &mut batch_lines).await;
            flush_at = None;
        }

        if stdout_reader.is_none() && stderr_reader.is_none() {
            // 정상 종료: 남은 배치를 flush한 뒤 완료 보고(RunCompleted)로 넘어간다.
            flush_batch(output, session_id, &mut batch, &mut batch_lines).await;
            return false;
        }
    }
}

/// 취소 플래그를 50ms 주기로 폴링. Stop 반영까지 최대 ~50ms 지연은 의도된 것이며
/// (OS 프로세스 teardown 시간에 비하면 무시 가능) cancel_flag 외 공유 상태가 없어
/// `Ordering::Relaxed`로 충분하다.
async fn wait_for_cancel(cancel_flag: &Arc<AtomicBool>) {
    loop {
        if cancel_flag.load(Ordering::Relaxed) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn read_next_line<R>(
    reader: &mut Option<tokio::io::BufReader<R>>,
) -> std::io::Result<Option<String>>
where
    R: tokio::io::AsyncRead + Unpin,
{
    use tokio::io::AsyncBufReadExt;

    let Some(reader) = reader.as_mut() else {
        return Ok(None);
    };

    // 바이트 단위로 한 줄을 읽어 lossy 디코딩한다. AsyncBufReadExt::lines()는 유효한
    // UTF-8만 허용해, 비UTF-8 로케일(LANG=C, ISO-8859, Shift-JIS 등) 출력의 첫
    // 유효하지 않은 바이트에서 Err를 반환 → 호출부가 EOF로 처리해 이후 출력이 전부
    // 유실된다. fill_buf/consume로 직접 읽고 from_utf8_lossy로 변환하면 유실 없이
    // 표시하면서 줄 길이 상한도 강제할 수 있다.
    let mut buf = Vec::new();
    let mut found_newline = false;
    loop {
        let consumed = {
            let available = reader.fill_buf().await?;
            if available.is_empty() {
                break; // EOF
            }
            match available.iter().position(|&b| b == b'\n') {
                Some(pos) => {
                    buf.extend_from_slice(&available[..=pos]);
                    found_newline = true;
                    pos + 1
                }
                None => {
                    buf.extend_from_slice(available);
                    available.len()
                }
            }
        };
        reader.consume(consumed);
        if found_newline || buf.len() >= MAX_LINE_BYTES {
            break;
        }
    }

    if buf.is_empty() {
        return Ok(None); // EOF, 더 읽을 것 없음
    }

    // 개행은 호출부에서 다시 부여하므로 트림한다 (CRLF/LF). 상한으로 잘린 청크는
    // 개행이 없으므로 트림하지 않는다.
    if found_newline {
        while matches!(buf.last(), Some(b'\n' | b'\r')) {
            buf.pop();
        }
    }
    Ok(Some(String::from_utf8_lossy(&buf).into_owned()))
}

/// 읽은 한 줄을 배치 버퍼에 누적한다. EOF/에러(=Ok(Some) 아님)면 해당 reader를 종료해야
/// 하므로 `true`를 반환한다. 배치가 비어 있다가 첫 줄이 들어오면 flush 마감 시각을 무장한다.
/// (개행은 호출부에서 트림됐으므로 여기서 다시 부여 — app 측이 lines()/'\n'로 재분할한다.)
fn accumulate_line(
    batch: &mut String,
    batch_lines: &mut usize,
    flush_at: &mut Option<Instant>,
    result: std::io::Result<Option<String>>,
) -> bool {
    if let Ok(Some(line)) = result {
        batch.push_str(&line);
        batch.push('\n');
        *batch_lines += 1;
        if flush_at.is_none() {
            *flush_at = Some(Instant::now() + OUTPUT_FLUSH_INTERVAL);
        }
        false
    } else {
        true
    }
}

/// 배치 문자열(줄마다 trailing '\n')을 `Line` 이벤트 벡터로 변환.
/// pipes 경로는 Replace를 만들지 않는다 — PTY 경로의 LineAssembler가 담당.
fn lines_to_events(payload: &str) -> Vec<OutputEvent> {
    payload
        .lines()
        .map(|line| OutputEvent::Line(line.to_string()))
        .collect()
}

// ---- PTY 라인 조립 (CR-live) --------------------------------------------------

/// flush 대기 중인 출력 이벤트 배치. 같은 논리 라인에 대한 연속 `Replace`는 직전
/// 이벤트에 제자리 병합된다 — Replace는 항상 "직전에 방출된 라인"을 겨냥하므로 배치
/// 안에서는 마지막 상태만 의미가 있다 ([Line a, Replace a', Replace a''] → [Line a'']).
#[derive(Default)]
struct EventBatch {
    events: Vec<OutputEvent>,
    bytes: usize,
}

impl EventBatch {
    fn push(&mut self, event: OutputEvent) {
        if let OutputEvent::Replace(new_text) = &event
            && let Some(last) = self.events.last_mut()
        {
            let (OutputEvent::Line(text) | OutputEvent::Replace(text)) = last;
            self.bytes = self.bytes.saturating_sub(text.len()) + new_text.len();
            // 직전 이벤트의 종류(Line/Replace)는 유지한 채 내용만 최신으로.
            *text = new_text.clone();
            return;
        }
        self.bytes += match &event {
            OutputEvent::Line(text) | OutputEvent::Replace(text) => text.len(),
        };
        self.events.push(event);
    }

    fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    fn len(&self) -> usize {
        self.events.len()
    }

    fn take(&mut self) -> Vec<OutputEvent> {
        self.bytes = 0;
        std::mem::take(&mut self.events)
    }
}

/// PTY raw 바이트 스트림을 라인 이벤트로 조립하는 상태기계.
///
/// - `\n` = 라인 확정 (직전이 `\r`이어도 CRLF는 평범한 개행 — Replace 아님)
/// - `\r` = pending: 다음 텍스트 바이트가 오면 현재 라인을 비우고 다시 쓴다
///   (**전체 라인 교체 근사** — "abc\rd" → "d". col0 문자단위 덮어쓰기가 아님)
/// - flush tick마다 미완 라인을 방출: 첫 방출은 `Line`(라이브 라인 열림), 이후 변경은
///   `Replace` — 진행바가 개행 없이도 실시간으로 한 줄에서 갱신된다
/// - 방출 텍스트에 `\r`은 절대 포함되지 않는다 (session측 collapse는 방어선)
/// - UTF-8 경계: flush 방출은 유효 prefix까지만 표시하고 잔여 바이트는 보류
///   (멀티바이트 문자가 청크에 걸릴 때 U+FFFD 깜빡임 방지); 라인 확정 시엔 lossy 전체
struct LineAssembler {
    /// 현재 미완 논리 라인의 바이트 (\r 미포함).
    partial: Vec<u8>,
    /// 직전 바이트가 `\r`(다음 텍스트가 라인을 되감아 덮어씀).
    pending_cr: bool,
    /// 현재 논리 라인을 이미 라이브로 방출했는지 (이후 방출은 Replace).
    shipped_open: bool,
    /// 마지막 방출 이후 partial이 변했는지.
    dirty: bool,
}

impl LineAssembler {
    fn new() -> Self {
        Self {
            partial: Vec::new(),
            pending_cr: false,
            shipped_open: false,
            dirty: false,
        }
    }

    /// flush tick에서 미방출 변경이 있는지 (select 분기 가드용).
    fn has_pending(&self) -> bool {
        self.dirty && !self.partial.is_empty()
    }

    fn push_bytes(&mut self, chunk: &[u8], batch: &mut EventBatch) {
        for &byte in chunk {
            match byte {
                b'\n' => {
                    // CRLF: 직전 \r는 개행의 일부 — 덮어쓰기 아님.
                    self.pending_cr = false;
                    let text = String::from_utf8_lossy(&self.partial).into_owned();
                    let event = if self.shipped_open {
                        OutputEvent::Replace(text)
                    } else {
                        OutputEvent::Line(text)
                    };
                    batch.push(event);
                    self.partial.clear();
                    self.shipped_open = false;
                    self.dirty = false;
                }
                b'\r' => {
                    self.pending_cr = true;
                }
                _ => {
                    if self.pending_cr {
                        // 되감기 실행: 전체 라인 교체 근사.
                        self.partial.clear();
                        self.pending_cr = false;
                        self.dirty = true;
                    }
                    self.partial.push(byte);
                    self.dirty = true;
                    // 개행 없는 폭주 라인 방어: 상한 도달 시 강제 확정.
                    if self.partial.len() >= MAX_LINE_BYTES {
                        let text = String::from_utf8_lossy(&self.partial).into_owned();
                        let event = if self.shipped_open {
                            OutputEvent::Replace(text)
                        } else {
                            OutputEvent::Line(text)
                        };
                        batch.push(event);
                        self.partial.clear();
                        self.shipped_open = false;
                        self.dirty = false;
                    }
                }
            }
        }
    }

    /// flush tick: 미완 라인을 라이브 방출 (첫 회 Line, 이후 Replace).
    fn emit_partial(&mut self, batch: &mut EventBatch) {
        if !self.has_pending() {
            return;
        }
        // 청크에 걸린 멀티바이트 문자는 보류 — 유효 prefix만 표시.
        let valid_len = match std::str::from_utf8(&self.partial) {
            Ok(_) => self.partial.len(),
            Err(e) if e.error_len().is_none() => e.valid_up_to(),
            // 중간의 진짜 잘못된 바이트는 lossy로 전체 표시 (보류해도 회복 불가).
            Err(_) => self.partial.len(),
        };
        if valid_len == 0 {
            return;
        }
        let text = String::from_utf8_lossy(&self.partial[..valid_len]).into_owned();
        let event = if self.shipped_open {
            OutputEvent::Replace(text)
        } else {
            OutputEvent::Line(text)
        };
        batch.push(event);
        self.shipped_open = true;
        self.dirty = false;
    }

    /// EOF: 잔여 미완 라인을 최종 확정한다.
    fn finalize(&mut self, batch: &mut EventBatch) {
        if self.partial.is_empty() {
            return;
        }
        let text = String::from_utf8_lossy(&self.partial).into_owned();
        let event = if self.shipped_open {
            OutputEvent::Replace(text)
        } else {
            OutputEvent::Line(text)
        };
        batch.push(event);
        self.partial.clear();
        self.shipped_open = false;
        self.dirty = false;
        self.pending_cr = false;
    }
}

/// 누적된 배치를 한 개의 `OutputReceived` 메시지로 전송하고 버퍼를 비운다. 비어 있으면 no-op.
async fn flush_batch(
    output: &mut iced::futures::channel::mpsc::Sender<Message>,
    session_id: Uuid,
    batch: &mut String,
    batch_lines: &mut usize,
) {
    use iced::futures::SinkExt;

    if batch.is_empty() {
        return;
    }
    let payload = std::mem::take(batch);
    *batch_lines = 0;
    let _ = output
        .send(Message::OutputReceived(
            session_id,
            lines_to_events(&payload),
        ))
        .await;
}

/// flush 마감까지 대기. 무장된 마감이 없으면 영원히 pending 한다 — select! 분기의
/// `if !batch.is_empty()` 가드와 함께 idle 시 불필요한 기상/busy-spin을 방지한다.
async fn wait_flush_deadline(flush_at: Option<Instant>) {
    match flush_at {
        Some(at) => sleep_until(at).await,
        None => std::future::pending::<()>().await,
    }
}

async fn wait_for_process(
    output: &mut iced::futures::channel::mpsc::Sender<Message>,
    session_id: Uuid,
    child: &mut Child,
    cancel_flag: &Arc<AtomicBool>,
) {
    // 종료 대기 중에도 cancel_flag를 관찰한다. 자식이 stdout/stderr를 닫았지만(EOF로
    // process_output_loop가 빠져나옴) 아직 종료하지 않은 경우, cancel 관찰이 없으면
    // child.wait()가 영원히 블록되어 Stop이 무반응이 되고 세션이 is_running에 고착된다.
    tokio::select! {
        biased;
        () = wait_for_cancel(cancel_flag) => {
            terminate_and_reap(child).await;
            send_run_result(
                output,
                session_id,
                Err(String::from("Process stopped by user")),
            )
            .await;
        }
        wait_result = child.wait() => match wait_result {
            Ok(status) => {
                send_run_result(output, session_id, Ok(status.code().unwrap_or(-1))).await;
            }
            Err(e) => {
                send_run_result(
                    output,
                    session_id,
                    Err(format!("Failed to wait for process: {e}")),
                )
                .await;
            }
        },
    }
}

async fn send_run_result(
    output: &mut iced::futures::channel::mpsc::Sender<Message>,
    session_id: Uuid,
    result: Result<i32, String>,
) {
    use iced::futures::SinkExt;

    let _ = output.send(Message::RunCompleted(session_id, result)).await;
}

/// 취소된 프로세스를 종료하고 반드시 reap한다.
///
/// Unix: SIGTERM → 최대 2초 대기 → 여전히 살아있으면 SIGKILL (Stop 경로에도 강제 종료 보장).
/// Windows: taskkill /T /F로 트리 강제 종료 후 reap.
async fn terminate_and_reap(child: &mut Child) {
    #[cfg(unix)]
    {
        // SIGTERM 전에 PID를 보존한다. wait가 프로세스를 reap한 뒤에는 child.id()가
        // None을 반환할 수 있어, 보존하지 않으면 SIGKILL이 누락될 수 있다.
        let pid = child.id();
        if let Some(p) = pid {
            unsafe {
                libc::killpg(p as i32, libc::SIGTERM);
            }
        }
        if tokio::time::timeout(Duration::from_secs(2), child.wait())
            .await
            .is_err()
        {
            // SIGTERM 무시 → SIGKILL 에스컬레이션 후 reap.
            if let Some(p) = pid {
                unsafe {
                    libc::killpg(p as i32, libc::SIGKILL);
                }
            }
            let _ = child.wait().await;
        }
    }

    #[cfg(windows)]
    {
        if let Some(pid) = child.id() {
            force_kill_process_tree(pid);
        }
        let _ = child.wait().await;
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = child.kill().await;
    }
}

/// Application 타입의 명령어 생성.
///
/// `&&` 등 셸 체이닝은 변환하지 않고 그대로 둔다 — Windows PowerShell 5.x 폴백 시의
/// `&&` → `;` 변환은 `create_process_command`에서 모든 구성 타입에 일관 적용된다.
fn build_application_command(command: &str, arguments: &str) -> String {
    if arguments.is_empty() {
        command.to_string()
    } else {
        format!("{command} {arguments}")
    }
}

/// Shell Script 타입의 명령어 생성
fn build_shell_script_command(execute_mode: &ExecuteMode) -> String {
    match execute_mode {
        ExecuteMode::ScriptFile {
            script_path,
            script_options,
            interpreter_path,
            interpreter_options,
        } => build_script_file_command(
            script_path,
            script_options,
            interpreter_path.as_deref(),
            interpreter_options.as_deref(),
        ),
        ExecuteMode::ScriptText { script_text } => script_text.clone(),
    }
}

/// Script File 모드의 명령어 생성
fn build_script_file_command(
    script_path: &str,
    script_options: &str,
    interpreter_path: Option<&str>,
    interpreter_options: Option<&str>,
) -> String {
    use std::path::Path;

    let script_path_quoted = quote_if_needed(script_path);
    let interp_opts = interpreter_options.unwrap_or("");

    // 사용자가 인터프리터를 명시하면 자동 감지를 건너뛰고 그대로 사용.
    if let Some(interpreter) = interpreter_path {
        return generic_interpreter_command(
            interpreter,
            interp_opts,
            &script_path_quoted,
            script_options,
        );
    }

    // 확장자 기반 인터프리터 자동 감지
    let extension = Path::new(script_path)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase);

    match extension.as_deref() {
        Some("py") => {
            // Windows는 `python` 런처, 그 외(Linux/macOS)는 `python3`가 표준.
            // (Ubuntu는 python3만 제공, macOS 12.3+는 /usr/bin/python 제거)
            #[cfg(target_os = "windows")]
            const PYTHON: &str = "python";
            #[cfg(not(target_os = "windows"))]
            const PYTHON: &str = "python3";
            generic_interpreter_command(PYTHON, interp_opts, &script_path_quoted, script_options)
        }
        Some("js") => {
            generic_interpreter_command("node", interp_opts, &script_path_quoted, script_options)
        }
        Some("rb") => {
            generic_interpreter_command("ruby", interp_opts, &script_path_quoted, script_options)
        }
        // .ps1: -File로 스크립트를 실행하고, 미서명 스크립트 차단을 피하기 위해 ExecutionPolicy Bypass.
        // Windows는 Windows PowerShell(powershell), 그 외 플랫폼은 PowerShell Core(pwsh).
        Some("ps1") => {
            #[cfg(target_os = "windows")]
            {
                format!(
                    "powershell -NoProfile -ExecutionPolicy Bypass -File {script_path_quoted} {script_options}"
                )
                .trim()
                .to_string()
            }
            #[cfg(not(target_os = "windows"))]
            {
                format!("pwsh -NoProfile -File {script_path_quoted} {script_options}")
                    .trim()
                    .to_string()
            }
        }
        // .bat/.cmd: cmd.exe는 /c 없이는 배치 파일을 실행하지 않는다 (Windows 전용).
        // 비-Windows에서는 의미가 없으므로 기본 분기(sh)로 폴백한다.
        #[cfg(target_os = "windows")]
        Some("bat" | "cmd") => format!("cmd /c {script_path_quoted} {script_options}")
            .trim()
            .to_string(),
        _ => generic_interpreter_command("sh", interp_opts, &script_path_quoted, script_options),
    }
}

/// `<interpreter> <opts> <script> <script_opts>` 형태의 일반 인터프리터 명령 생성.
fn generic_interpreter_command(
    interpreter_base: &str,
    interpreter_options: &str,
    script_path_quoted: &str,
    script_options: &str,
) -> String {
    let interpreter = quote_if_needed(interpreter_base);

    // Windows PowerShell에서는 공백이 포함된 경로를 & "경로" 형태로 실행
    #[cfg(target_os = "windows")]
    let interpreter_cmd = if interpreter.starts_with('"') {
        format!("& {interpreter}")
    } else {
        interpreter
    };

    #[cfg(not(target_os = "windows"))]
    let interpreter_cmd = interpreter;

    format!("{interpreter_cmd} {interpreter_options} {script_path_quoted} {script_options}")
        .trim()
        .to_string()
}

/// Node package manager 타입의 명령어 생성
#[derive(Clone, Copy)]
struct NodeCommandParts<'a> {
    package_manager: &'a PackageManager,
    node_runtime_path: Option<&'a String>,
    command: NodeCommand,
    script_name: Option<&'a String>,
    arguments: &'a str,
    working_directory: &'a str,
}

fn build_node_command(parts: NodeCommandParts<'_>) -> String {
    use crate::utils::detect_package_manager;
    use std::path::Path;

    // 패키지 매니저 결정
    let pm_type = parts.package_manager.executable_name().map_or_else(
        || detect_package_manager(Path::new(parts.working_directory)),
        str::to_owned,
    );

    let package_manager = resolve_package_manager_executable(&pm_type, parts.node_runtime_path);

    // 명령어 조립
    // Windows PowerShell에서는 공백이 포함된 경로를 & "경로" 형태로 실행해야 함
    #[cfg(target_os = "windows")]
    let package_manager_cmd = if package_manager.starts_with('"') {
        format!("& {package_manager}")
    } else {
        package_manager
    };

    #[cfg(not(target_os = "windows"))]
    let package_manager_cmd = package_manager;

    let mut cmd_parts = vec![package_manager_cmd];
    cmd_parts.extend(package_manager_command_args(
        &pm_type,
        parts.command,
        parts.script_name.map(String::as_str),
    ));

    // arguments
    if !parts.arguments.is_empty() {
        cmd_parts.push(parts.arguments.to_string());
    }

    cmd_parts.join(" ")
}

fn resolve_package_manager_executable(pm_type: &str, node_runtime_path: Option<&String>) -> String {
    let Some(node_path) = node_runtime_path else {
        return pm_type.to_string();
    };

    let Some(node_dir) = std::path::Path::new(node_path).parent() else {
        return pm_type.to_string();
    };

    for executable in package_manager_executable_candidates(pm_type) {
        let pm_path = node_dir.join(executable);
        if pm_path.exists() {
            return quote_if_needed(&pm_path.to_string_lossy());
        }
    }

    pm_type.to_string()
}

fn package_manager_executable_candidates(pm_type: &str) -> Vec<String> {
    #[cfg(target_os = "windows")]
    {
        vec![format!("{pm_type}.cmd"), format!("{pm_type}.exe")]
    }

    #[cfg(not(target_os = "windows"))]
    {
        vec![pm_type.to_string()]
    }
}

fn package_manager_command_args(
    pm_type: &str,
    command: NodeCommand,
    script_name: Option<&str>,
) -> Vec<String> {
    let script_command = match command {
        NodeCommand::Run => script_name,
        NodeCommand::Build => Some("build"),
        NodeCommand::Start if pm_type == "bun" => Some("start"),
        NodeCommand::Test if pm_type == "bun" => Some("test"),
        _ => None,
    };

    if let Some(script) = script_command {
        return vec![String::from("run"), script.to_string()];
    }

    vec![command.as_str().to_string()]
}

/// Kotlin 타입의 명령어 생성에 필요한 필드 묶음.
struct KotlinCommandParts<'a> {
    jdk_path: Option<&'a String>,
    launch_mode: &'a KotlinLaunchMode,
    vm_options: &'a str,
    program_arguments: &'a str,
}

/// Kotlin 구성의 셸 명령 문자열 생성 (`java` 경유).
///
/// MainClass: `java [vm_options] -cp "<classpath>" <main_class> [program_arguments]`
/// Jar:       `java [vm_options] -jar <jar_path> [program_arguments]`
fn build_kotlin_command(parts: KotlinCommandParts<'_>) -> String {
    let java = resolve_java_executable(parts.jdk_path);

    // Windows PowerShell에서는 공백이 포함된 경로를 & "경로" 형태로 실행해야 함
    #[cfg(target_os = "windows")]
    let java_cmd = if java.starts_with('"') {
        format!("& {java}")
    } else {
        java
    };

    #[cfg(not(target_os = "windows"))]
    let java_cmd = java;

    let mut cmd_parts = vec![java_cmd];

    // VM options(-Xmx 등)는 main class / -jar 앞에 위치
    if !parts.vm_options.trim().is_empty() {
        cmd_parts.push(parts.vm_options.trim().to_string());
    }

    match parts.launch_mode {
        KotlinLaunchMode::MainClass {
            main_class,
            classpath,
        } => {
            // classpath는 항상 따옴표로 감싼다 — `build/libs/*` 같은 와일드카드를
            // 셸이 glob 확장하지 않고 java로 그대로 전달해야 하기 때문. 내부 `"`는
            // 이스케이프해 따옴표 breakout(파싱 오류/주입)을 막는다.
            if !classpath.trim().is_empty() {
                cmd_parts.push(String::from("-cp"));
                let escaped = classpath.trim().replace('"', "\\\"");
                cmd_parts.push(format!("\"{escaped}\""));
            }
            if !main_class.trim().is_empty() {
                cmd_parts.push(main_class.trim().to_string());
            }
        }
        KotlinLaunchMode::Jar { jar_path } => {
            cmd_parts.push(String::from("-jar"));
            cmd_parts.push(quote_if_needed(jar_path.trim()));
        }
    }

    if !parts.program_arguments.trim().is_empty() {
        cmd_parts.push(parts.program_arguments.trim().to_string());
    }

    cmd_parts.join(" ")
}

/// `jdk_path`(JDK home 디렉터리 또는 `java` 실행 파일 경로)로부터 실행할 `java` 명령을 해석.
///
/// - None/빈 문자열 → 시스템 PATH의 `java`
/// - 디렉터리 → `<dir>/bin/java`(JDK home) 또는 `<dir>/java`(bin 디렉터리) 탐색,
///   못 찾으면 시스템 `java`로 폴백 (디렉터리 경로를 명령으로 넘기지 않음)
/// - 그 외(파일 경로) → 입력값을 그대로 사용
fn resolve_java_executable(jdk_path: Option<&String>) -> String {
    let Some(path) = jdk_path else {
        return String::from("java");
    };

    let trimmed = path.trim();
    if trimmed.is_empty() {
        return String::from("java");
    }

    let candidate = std::path::Path::new(trimmed);
    if candidate.is_dir() {
        let name = crate::utils::java_executable_name();
        let home_bin = candidate.join("bin").join(name);
        if home_bin.exists() {
            return quote_if_needed(&home_bin.to_string_lossy());
        }
        let direct = candidate.join(name);
        if direct.exists() {
            return quote_if_needed(&direct.to_string_lossy());
        }
        // 디렉터리지만 java 바이너리를 못 찾으면 디렉터리 경로를 명령으로 넘기는 대신
        // 시스템 java로 폴백한다 ("is a directory" 실행 오류 방지).
        return String::from("java");
    }

    quote_if_needed(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- LineAssembler: CR/LF 조립 매트릭스 ----

    /// 바이트를 한 번에 밀어넣고 배치 이벤트를 꺼내는 헬퍼.
    fn assemble(chunks: &[&[u8]], flush_between: bool) -> Vec<OutputEvent> {
        let mut assembler = LineAssembler::new();
        let mut batch = EventBatch::default();
        let mut out = Vec::new();
        for chunk in chunks {
            assembler.push_bytes(chunk, &mut batch);
            if flush_between {
                assembler.emit_partial(&mut batch);
                out.extend(batch.take());
            }
        }
        assembler.finalize(&mut batch);
        out.extend(batch.take());
        out
    }

    fn line(s: &str) -> OutputEvent {
        OutputEvent::Line(s.to_string())
    }
    fn replace(s: &str) -> OutputEvent {
        OutputEvent::Replace(s.to_string())
    }

    #[test]
    fn assembler_plain_newlines_emit_lines() {
        assert_eq!(assemble(&[b"a\nb\n"], false), vec![line("a"), line("b")]);
    }

    #[test]
    fn assembler_cr_replaces_whole_line() {
        // 전체 라인 교체 근사: "abc\rd" → "d" (col0 문자단위 덮어쓰기 아님).
        assert_eq!(assemble(&[b"abc\rd\n"], false), vec![line("d")]);
        // 진행바: 중간 상태들은 한 flush 안에서 붕괴, 최종만 남는다.
        assert_eq!(assemble(&[b"10%\r55%\r100%\n"], false), vec![line("100%")]);
    }

    #[test]
    fn assembler_crlf_is_newline_not_replace() {
        assert_eq!(
            assemble(&[b"a\r\nb\r\n"], false),
            vec![line("a"), line("b")]
        );
        // 청크 경계에 걸린 CRLF ("a\r" | "\nb")도 개행으로 처리.
        assert_eq!(
            assemble(&[b"a\r", b"\nb\n"], false),
            vec![line("a"), line("b")]
        );
    }

    #[test]
    fn assembler_live_line_opens_then_replaces() {
        // flush tick마다: 첫 방출은 Line(라이브 열림), 이후 갱신은 Replace,
        // 개행 확정도 Replace(같은 논리 라인), 다음 텍스트는 새 Line.
        let events = assemble(&[b"10%", b"\r55%", b"\r100%\ndone\n"], true);
        assert_eq!(
            events,
            vec![line("10%"), replace("55%"), replace("100%"), line("done")]
        );
    }

    #[test]
    fn assembler_holds_split_multibyte_at_flush() {
        // 한글 3바이트가 청크에 걸리면 flush 방출은 유효 prefix까지만, 잔여는 보류.
        let bytes = "가나".as_bytes(); // 6 bytes
        let events = assemble(&[&bytes[..4], &bytes[4..], b"\n"], true);
        // 첫 flush: "가" + 잘린 1바이트 보류 → Line("가"); 둘째 flush: Replace("가나")… 개행 확정 Replace.
        assert_eq!(events[0], line("가"));
        assert!(
            events.iter().all(|e| match e {
                OutputEvent::Line(t) | OutputEvent::Replace(t) => !t.contains('\u{FFFD}'),
            }),
            "경계 분할이 U+FFFD로 새면 안 됨: {events:?}"
        );
        assert_eq!(events.last(), Some(&replace("가나")));
    }

    #[test]
    fn assembler_caps_runaway_line() {
        // 개행 없는 폭주 라인은 MAX_LINE_BYTES에서 강제 확정된다.
        let big = vec![b'x'; MAX_LINE_BYTES + 10];
        let events = assemble(&[&big], false);
        assert!(events.len() >= 2, "상한 확정 + 잔여 라인");
        match &events[0] {
            OutputEvent::Line(t) => assert_eq!(t.len(), MAX_LINE_BYTES),
            other => panic!("first must be capped Line, got {other:?}"),
        }
    }

    #[test]
    fn assembler_emitted_text_never_contains_cr() {
        let events = assemble(&[b"a\rb", b"c\rd\ne\r"], true);
        for event in &events {
            let (OutputEvent::Line(t) | OutputEvent::Replace(t)) = event;
            assert!(!t.contains('\r'), "CR leaked: {t:?}");
        }
    }

    // ---- EventBatch: Replace 제자리 병합 ----

    #[test]
    fn event_batch_merges_consecutive_replaces_into_previous() {
        let mut batch = EventBatch::default();
        batch.push(line("a"));
        batch.push(replace("a2"));
        batch.push(replace("a3"));
        assert_eq!(batch.take(), vec![line("a3")], "Line 종류 유지 + 최종 내용");

        // 배치 선두의 Replace(대상은 이전 flush에서 전달됨)는 Replace로 보존.
        let mut batch = EventBatch::default();
        batch.push(replace("x"));
        batch.push(line("b"));
        batch.push(replace("b2"));
        assert_eq!(batch.take(), vec![replace("x"), line("b2")]);
    }

    #[test]
    fn event_batch_tracks_bytes_under_merge() {
        let mut batch = EventBatch::default();
        batch.push(line("aaaa")); // 4
        batch.push(replace("bb")); // 병합 → 2
        assert_eq!(batch.bytes, 2);
        batch.push(line("ccc")); // +3
        assert_eq!(batch.bytes, 5);
    }

    // ---- PTY 통합 (unix 실프로세스; openpty 불가 환경은 self-skip) ----

    #[cfg(unix)]
    mod pty_integration {
        use super::*;

        fn test_config(dir: &str) -> RunConfiguration {
            RunConfiguration {
                working_directory: dir.to_string(),
                ..RunConfiguration::default()
            }
        }

        /// PTY 스폰 → 모든 청크 수집 → (합쳐진 출력, exit 결과).
        async fn run_pty(command: &str) -> Option<(String, Result<i32, String>)> {
            let config = test_config("/tmp");
            let session_id = Uuid::new_v4();
            let pty = match spawn_in_pty(&config, command, &[], session_id, (80, 24)) {
                Ok(p) => p,
                Err(PtySpawnError::PtyUnavailable(_)) => return None, // CI 등 pty 불가 → skip
                Err(PtySpawnError::Spawn(e)) => panic!("spawn failed: {e}"),
            };
            let mut chunks_rx = pty.chunks_rx;
            let mut collected = Vec::new();
            while let Some(bytes) = chunks_rx.recv().await {
                collected.extend_from_slice(&bytes);
            }
            let status = pty.exit_rx.await.unwrap_or(Err(String::from("no status")));
            unregister_session_io(session_id);
            Some((String::from_utf8_lossy(&collected).into_owned(), status))
        }

        #[tokio::test]
        async fn pty_child_sees_a_tty_and_term() {
            // isatty=true + TERM 주입 — PTY 전환의 존재 이유 검증.
            let Some((out, status)) =
                run_pty("test -t 1 && printf ok-tty; printf ' term=%s', \"$TERM\"").await
            else {
                eprintln!("[skip] pty unavailable in this environment");
                return;
            };
            assert!(out.contains("ok-tty"), "stdout must be a tty: {out:?}");
            assert!(
                out.contains("term=xterm-256color"),
                "TERM must be set: {out:?}"
            );
            assert_eq!(status, Ok(0));
        }

        #[tokio::test]
        async fn pty_child_is_its_own_process_group_leader() {
            // killpg 전제(pid==pgid) 실증 — portable-pty의 setsid가 보장해야 한다.
            let Some((out, _)) = run_pty("ps -o pid=,pgid= -p $$").await else {
                eprintln!("[skip] pty unavailable in this environment");
                return;
            };
            let nums: Vec<i64> = out
                .split_whitespace()
                .filter_map(|t| t.parse().ok())
                .collect();
            assert!(
                nums.len() >= 2 && nums[0] == nums[1],
                "shell pid must equal pgid (session leader), got: {out:?}"
            );
        }

        #[tokio::test]
        async fn pty_progress_cr_collapses_to_final_state() {
            // 실제 pty 왕복에서 \r 진행바가 조립기에서 최종 상태로 붕괴하는지 종단 확인.
            let Some((raw, status)) = run_pty("printf 'a\\rb\\rc\\n'").await else {
                eprintln!("[skip] pty unavailable in this environment");
                return;
            };
            assert_eq!(status, Ok(0));
            let mut assembler = LineAssembler::new();
            let mut batch = EventBatch::default();
            assembler.push_bytes(raw.as_bytes(), &mut batch);
            assembler.finalize(&mut batch);
            let events = batch.take();
            assert_eq!(events, vec![OutputEvent::Line(String::from("c"))]);
        }

        #[tokio::test]
        async fn pty_stdin_echo_roundtrip() {
            // stdin writer로 쓴 입력이 (a) 자식에 도달하고 (b) tty 에코로 출력에 나타난다.
            let config = test_config("/tmp");
            let session_id = Uuid::new_v4();
            let pty = match spawn_in_pty(
                &config,
                "read x; printf \"got:%s\\n\" \"$x\"",
                &[],
                session_id,
                (80, 24),
            ) {
                Ok(p) => p,
                Err(PtySpawnError::PtyUnavailable(_)) => {
                    eprintln!("[skip] pty unavailable in this environment");
                    return;
                }
                Err(PtySpawnError::Spawn(e)) => panic!("spawn failed: {e}"),
            };
            // 레지스트리에서 writer를 꺼내 직접 쓴다 (슬라이스4의 write 경로 원형).
            {
                let map = SESSION_IO.lock().unwrap();
                let io = map.get(&session_id).expect("registered");
                match io.writer.as_ref().expect("pty writer") {
                    SessionWriter::Pty(w) => {
                        use std::io::Write;
                        let mut w = w.lock().unwrap();
                        w.write_all(b"hello\n").unwrap();
                        w.flush().unwrap();
                    }
                    #[cfg(windows)]
                    _ => unreachable!(),
                }
            }
            let mut chunks_rx = pty.chunks_rx;
            let mut collected = Vec::new();
            while let Some(bytes) = chunks_rx.recv().await {
                collected.extend_from_slice(&bytes);
            }
            let out = String::from_utf8_lossy(&collected).into_owned();
            let status = pty.exit_rx.await.unwrap_or(Err(String::from("no status")));
            unregister_session_io(session_id);
            assert_eq!(status, Ok(0));
            assert!(
                out.contains("got:hello"),
                "child must receive stdin: {out:?}"
            );
            assert!(
                out.matches("hello").count() >= 2,
                "tty echo + child print expected: {out:?}"
            );
        }
    }

    #[test]
    fn build_command_uses_run_script_for_build_across_package_managers() {
        assert_eq!(
            package_manager_command_args("npm", NodeCommand::Build, None),
            vec![String::from("run"), String::from("build")]
        );
        assert_eq!(
            package_manager_command_args("bun", NodeCommand::Build, None),
            vec![String::from("run"), String::from("build")]
        );
    }

    fn kotlin_main_class(main_class: &str, classpath: &str) -> KotlinLaunchMode {
        KotlinLaunchMode::MainClass {
            main_class: String::from(main_class),
            classpath: String::from(classpath),
        }
    }

    #[test]
    fn kotlin_main_class_command_quotes_classpath_and_omits_empty_fields() {
        let cmd = build_kotlin_command(KotlinCommandParts {
            jdk_path: None,
            launch_mode: &kotlin_main_class("com.example.MainKt", "build/libs/*:libs/*"),
            vm_options: "",
            program_arguments: "",
        });
        // classpath는 glob 보존을 위해 항상 따옴표로 감싼다.
        assert_eq!(cmd, "java -cp \"build/libs/*:libs/*\" com.example.MainKt");
    }

    #[test]
    fn kotlin_main_class_command_includes_vm_options_and_args() {
        let cmd = build_kotlin_command(KotlinCommandParts {
            jdk_path: None,
            launch_mode: &kotlin_main_class("MainKt", "out"),
            vm_options: "-Xmx2g",
            program_arguments: "--debug input.txt",
        });
        assert_eq!(cmd, "java -Xmx2g -cp \"out\" MainKt --debug input.txt");
    }

    #[test]
    fn kotlin_main_class_command_without_classpath_omits_cp() {
        let cmd = build_kotlin_command(KotlinCommandParts {
            jdk_path: None,
            launch_mode: &kotlin_main_class("MainKt", ""),
            vm_options: "",
            program_arguments: "",
        });
        assert_eq!(cmd, "java MainKt");
    }

    #[test]
    fn kotlin_jar_command_quotes_path_with_space() {
        let jar = KotlinLaunchMode::Jar {
            jar_path: String::from("my apps/app.jar"),
        };
        let cmd = build_kotlin_command(KotlinCommandParts {
            jdk_path: None,
            launch_mode: &jar,
            vm_options: "-Xmx1g",
            program_arguments: "run",
        });
        assert_eq!(cmd, "java -Xmx1g -jar \"my apps/app.jar\" run");
    }

    #[test]
    fn kotlin_custom_jdk_path_used_as_is_when_not_a_dir() {
        // 존재하지 않는 경로는 is_dir=false → 입력값 그대로 사용 (결정적).
        let jdk = String::from("/opt/nonexistent-jdk/bin/java");
        let jar = KotlinLaunchMode::Jar {
            jar_path: String::from("app.jar"),
        };
        let cmd = build_kotlin_command(KotlinCommandParts {
            jdk_path: Some(&jdk),
            launch_mode: &jar,
            vm_options: "",
            program_arguments: "",
        });
        assert_eq!(cmd, "/opt/nonexistent-jdk/bin/java -jar app.jar");
    }

    #[test]
    fn kotlin_classpath_escapes_inner_quotes() {
        let cmd = build_kotlin_command(KotlinCommandParts {
            jdk_path: None,
            launch_mode: &kotlin_main_class("MainKt", "a\"b:c"),
            vm_options: "",
            program_arguments: "",
        });
        // 내부 따옴표는 이스케이프되어 따옴표 breakout이 발생하지 않아야 한다.
        assert_eq!(cmd, "java -cp \"a\\\"b:c\" MainKt");
    }

    #[test]
    fn resolve_java_falls_back_when_dir_has_no_java_binary() {
        // 존재하는 디렉터리지만 bin/java가 없으면 시스템 java로 폴백 (디렉터리 경로 미사용).
        let dir = std::env::temp_dir().to_string_lossy().to_string();
        assert_eq!(resolve_java_executable(Some(&dir)), "java");
    }

    #[test]
    fn build_command_routes_kotlin_and_returns_no_extra_env() {
        let config = RunConfiguration {
            id: Uuid::new_v4(),
            name: String::from("k"),
            working_directory: String::from("."),
            environment_variables: std::collections::HashMap::new(),
            type_data: ConfigTypeData::Kotlin {
                jdk_path: None,
                launch_mode: kotlin_main_class("MainKt", "out"),
                vm_options: String::new(),
                program_arguments: String::new(),
            },
        };
        let (cmd, env) = build_command(&config);
        assert_eq!(cmd, "java -cp \"out\" MainKt");
        assert!(env.is_empty());
    }

    /// 회귀 방지: 한쪽 reader가 먼저 EOF(None)가 돼도 출력 루프가 정상 종료해야 한다.
    /// 과거 `tokio::select! { biased; ... }`가 None reader 분기(즉시 Ok(None))만 계속
    /// 선택해 다른 reader를 영영 폴링하지 않는 livelock이 있었다 (CPU 100%, RunCompleted
    /// 미발송 → 알림/exit code 미표시). `if reader.is_some()` 전제조건으로 해소됨.
    #[tokio::test]
    async fn output_loop_terminates_when_one_reader_eofs_first() {
        use tokio::io::BufReader;

        let (mut tx, _rx) = iced::futures::channel::mpsc::channel::<Message>(100);
        let cancel = Arc::new(AtomicBool::new(false));

        // stdout: 데이터 후 EOF / stderr: 즉시 EOF (실제 "echo 후 종료" 시나리오 재현).
        let stdout_data: &[u8] = b"out1\nout2\n";
        let stderr_data: &[u8] = b"";
        let mut stdout = Some(BufReader::new(stdout_data));
        let mut stderr = Some(BufReader::new(stderr_data));

        let completed = tokio::time::timeout(
            Duration::from_secs(5),
            process_output_loop(&mut tx, Uuid::new_v4(), &mut stdout, &mut stderr, &cancel),
        )
        .await
        .expect("output loop livelocked (timed out) — biased select starvation regressed");

        assert!(
            !completed,
            "both readers at EOF should report normal completion (false), not cancel"
        );
        assert!(
            stdout.is_none() && stderr.is_none(),
            "both readers should be drained to None"
        );
    }

    /// 회귀 방지(C2 메시지 폭주): 출력 줄을 배치로 묶어 메시지 수를 줄이되, 내용과 순서는
    /// 보존해야 한다. 200줄을 즉시 제공 → 64줄 임계 flush + EOF 시 잔여 flush로 200개보다
    /// 훨씬 적은 메시지에 담기며, 모든 페이로드를 이어 붙이면 원본과 동일해야 한다.
    #[tokio::test]
    async fn output_loop_coalesces_lines_preserving_content_and_order() {
        use iced::futures::StreamExt;
        use tokio::io::BufReader;

        let (mut tx, mut rx) = iced::futures::channel::mpsc::channel::<Message>(1000);
        let cancel = Arc::new(AtomicBool::new(false));

        let mut data = Vec::new();
        for i in 0..200 {
            data.extend_from_slice(format!("line{i}\n").as_bytes());
        }
        let mut stdout = Some(BufReader::new(data.as_slice()));
        let mut stderr: Option<BufReader<&[u8]>> = Some(BufReader::new(b"".as_slice()));

        let completed = tokio::time::timeout(
            Duration::from_secs(5),
            process_output_loop(&mut tx, Uuid::new_v4(), &mut stdout, &mut stderr, &cancel),
        )
        .await
        .expect("output loop timed out");
        assert!(
            !completed,
            "both readers at EOF → normal completion (false)"
        );

        // 송신측 drop 후 수신 스트림을 모두 수집.
        drop(tx);
        let mut messages = Vec::new();
        while let Some(msg) = rx.next().await {
            messages.push(msg);
        }

        // 내용/순서 보존: 모든 OutputReceived 이벤트(Line)를 이어 붙이면 원본과 동일해야 한다
        // (EOF 시 잔여 부분 배치 flush가 누락 없이 마지막 줄들까지 보내는지도 검증).
        let mut reconstructed = String::new();
        for msg in &messages {
            if let Message::OutputReceived(_, events) = msg {
                for event in events {
                    match event {
                        crate::models::OutputEvent::Line(text)
                        | crate::models::OutputEvent::Replace(text) => {
                            reconstructed.push_str(text);
                            reconstructed.push('\n');
                        }
                    }
                }
            }
        }
        let expected: String = (0..200).map(|i| format!("line{i}\n")).collect();
        assert_eq!(
            reconstructed, expected,
            "배치 후에도 줄 내용/순서가 보존되어야 함"
        );

        // 배치 효과: 200줄이 줄 수보다 훨씬 적은 메시지로 묶여야 한다 (64줄 임계 → 약 4개;
        // 인메모리 read는 즉시 완료되어 16ms 타이머가 거의 발화하지 않으므로 여유 상한 20).
        let output_msgs = messages
            .iter()
            .filter(|m| matches!(m, Message::OutputReceived(_, _)))
            .count();
        assert!(
            output_msgs < 200,
            "배치로 메시지 수가 줄어야 함 (got {output_msgs})"
        );
        assert!(
            output_msgs <= 20,
            "200줄이 배치로 크게 줄어야 함 (got {output_msgs})"
        );
    }

    /// 회귀 방지: 개행 없는 대량 출력이 줄당 상한(1 MiB)에서 flush되어야 한다
    /// (무한 버퍼 증가 → OOM 방지). 상한으로 잘려도 바이트 유실은 없어야 한다.
    #[tokio::test]
    async fn read_next_line_caps_unbounded_newlineless_output() {
        use tokio::io::BufReader;

        let big = vec![b'x'; 1024 * 1024 + 5000]; // 개행 없는 1 MiB + 5000 바이트
        let mut reader = Some(BufReader::new(big.as_slice()));

        // 첫 호출: 상한에서 잘린 청크 (정확히 상한이거나 한 버퍼 청크 이내 초과).
        let chunk = read_next_line(&mut reader).await.unwrap().unwrap();
        assert!(
            chunk.len() >= 1024 * 1024,
            "줄당 상한에서 flush되어야 함 (got {})",
            chunk.len()
        );

        // 나머지는 다음 호출에서 반환 — 합치면 원본 전체 (유실 없음).
        let rest = read_next_line(&mut reader).await.unwrap().unwrap();
        assert_eq!(chunk.len() + rest.len(), big.len(), "바이트 유실 없어야 함");

        // 그 다음은 EOF.
        assert!(read_next_line(&mut reader).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn send_command_info_env_line_gated_by_show_env() {
        use iced::futures::StreamExt;
        use iced::futures::channel::mpsc;

        let mut config = RunConfiguration {
            name: "T".to_string(),
            ..RunConfiguration::default()
        };
        config
            .environment_variables
            .insert("FOO".to_string(), "bar".to_string());

        async fn header(config: &RunConfiguration, show_env: bool) -> String {
            let (mut tx, mut rx) = mpsc::channel(16);
            send_command_info(&mut tx, Uuid::new_v4(), config, "echo hi", &[], show_env).await;
            drop(tx);
            let mut out = String::new();
            while let Some(msg) = rx.next().await {
                if let Message::OutputReceived(_, events) = msg {
                    for event in events {
                        match event {
                            crate::models::OutputEvent::Line(text)
                            | crate::models::OutputEvent::Replace(text) => {
                                out.push_str(&text);
                                out.push('\n');
                            }
                        }
                    }
                }
            }
            out
        }

        // show_env=true면 Environment 라인 포함, false면 제외
        assert!(header(&config, true).await.contains("Environment:"));
        assert!(!header(&config, false).await.contains("Environment:"));
    }

    #[test]
    fn bun_start_and_test_use_package_scripts() {
        assert_eq!(
            package_manager_command_args("bun", NodeCommand::Start, None),
            vec![String::from("run"), String::from("start")]
        );
        assert_eq!(
            package_manager_command_args("bun", NodeCommand::Test, None),
            vec![String::from("run"), String::from("test")]
        );
    }

    #[test]
    fn run_command_keeps_selected_script() {
        assert_eq!(
            package_manager_command_args("pnpm", NodeCommand::Run, Some("dev")),
            vec![String::from("run"), String::from("dev")]
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_package_manager_candidates_include_cmd_and_exe() {
        assert_eq!(
            package_manager_executable_candidates("bun"),
            vec![String::from("bun.cmd"), String::from("bun.exe")]
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn unix_package_manager_candidates_use_plain_executable() {
        assert_eq!(
            package_manager_executable_candidates("bun"),
            vec![String::from("bun")]
        );
    }

    #[test]
    fn application_command_joins_command_and_arguments() {
        assert_eq!(build_application_command("ls", ""), "ls");
        assert_eq!(
            build_application_command("echo", "hello world"),
            "echo hello world"
        );
    }

    #[test]
    fn application_command_preserves_chaining_operator() {
        // && 는 빌드 단계에서 변환하지 않는다. (pwsh는 네이티브 지원, 5.x 폴백 변환은
        // create_process_command가 모든 타입에 일관 적용)
        assert_eq!(
            build_application_command("npm i", "&& npm test"),
            "npm i && npm test"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn legacy_powershell_rewrites_and_to_semicolon() {
        assert_eq!(
            rewrite_chaining_for_legacy_powershell("npm i && npm test"),
            "npm i; npm test"
        );
        // && 가 없으면 그대로
        assert_eq!(rewrite_chaining_for_legacy_powershell("echo hi"), "echo hi");
    }

    #[test]
    fn script_text_mode_runs_verbatim() {
        let mode = ExecuteMode::ScriptText {
            script_text: String::from("echo hi"),
        };
        assert_eq!(build_shell_script_command(&mode), "echo hi");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn bat_script_is_launched_via_cmd_slash_c() {
        let cmd = build_script_file_command("build.bat", "", None, None);
        assert_eq!(cmd, "cmd /c build.bat");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn ps1_script_uses_file_and_bypass_policy() {
        let cmd = build_script_file_command("deploy.ps1", "", None, None);
        assert_eq!(
            cmd,
            "powershell -NoProfile -ExecutionPolicy Bypass -File deploy.ps1"
        );
    }

    #[test]
    fn explicit_interpreter_overrides_extension_detection() {
        let cmd = build_script_file_command("script.py", "--flag", Some("python3"), Some("-u"));
        assert_eq!(cmd, "python3 -u script.py --flag");
    }

    #[test]
    fn node_options_routed_to_env_not_command_string() {
        let config = RunConfiguration {
            type_data: ConfigTypeData::Node {
                project_directory: String::from("."),
                package_manager: PackageManager::Npm,
                node_runtime_path: None,
                command: NodeCommand::Run,
                script_name: Some(String::from("dev")),
                arguments: String::new(),
                node_options: String::from("--max-old-space-size=4096"),
            },
            ..RunConfiguration::default()
        };
        let (command_str, extra_env) = build_command(&config);
        assert!(!command_str.contains("NODE_OPTIONS"));
        assert_eq!(
            extra_env,
            vec![(
                String::from("NODE_OPTIONS"),
                String::from("--max-old-space-size=4096")
            )]
        );
    }
}
