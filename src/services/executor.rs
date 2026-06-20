use crate::messages::Message;
use crate::models::{ConfigTypeData, ExecuteMode, NodeCommand, PackageManager, RunConfiguration};
use iced::stream;
use std::collections::HashSet;
use std::fmt::Write as _;
use std::sync::{
    Arc, LazyLock, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio::process::{Child, ChildStderr, ChildStdout};
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

/// PID로 프로세스 트리를 강제 종료 (비동기 컨텍스트 밖에서도 호출 가능).
fn force_kill_process_tree(pid: u32) {
    #[cfg(unix)]
    unsafe {
        // process_group(0)으로 생성했으므로 pid == pgid. 그룹 전체에 SIGKILL.
        libc::killpg(pid as i32, libc::SIGKILL);
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
pub fn run_configuration_stream(
    config: RunConfiguration,
    session_id: Uuid,
    cancel_flag: Arc<AtomicBool>,
) -> impl iced::futures::Stream<Item = Message> {
    stream::channel(
        100,
        move |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
            let (command_str, extra_env) = build_command(&config);
            let mut cmd = create_process_command(&config, &command_str, &extra_env);
            send_command_info(&mut output, session_id, &config, &command_str, &extra_env).await;

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
        },
    )
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
        c.args([
            "-NoProfile",
            "-NonInteractive",
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
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

async fn send_command_info(
    output: &mut iced::futures::channel::mpsc::Sender<Message>,
    session_id: Uuid,
    config: &RunConfiguration,
    command_str: &str,
    extra_env: &[(String, String)],
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
    if !config.environment_variables.is_empty() || !extra_env.is_empty() {
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
        .send(Message::OutputReceived(session_id, command_info))
        .await;
}

async fn handle_spawned_process(
    output: &mut iced::futures::channel::mpsc::Sender<Message>,
    session_id: Uuid,
    mut child: Child,
    cancel_flag: Arc<AtomicBool>,
) {
    send_process_started(output, session_id, &child).await;
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
        wait_for_process(output, session_id, &mut child).await;
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

/// 출력 펌프 루프. 취소되면 `true`, stdout/stderr가 모두 닫혀 정상 종료되면 `false` 반환.
/// 실제 프로세스 종료(kill/reap)는 호출자(`handle_spawned_process`)가 담당한다.
async fn process_output_loop(
    output: &mut iced::futures::channel::mpsc::Sender<Message>,
    session_id: Uuid,
    stdout_reader: &mut Option<tokio::io::BufReader<ChildStdout>>,
    stderr_reader: &mut Option<tokio::io::BufReader<ChildStderr>>,
    cancel_flag: &Arc<AtomicBool>,
) -> bool {
    loop {
        tokio::select! {
            // 취소 신호를 출력 읽기보다 우선 처리.
            biased;
            () = wait_for_cancel(cancel_flag) => {
                return true;
            }
            result = read_next_line(stdout_reader) => {
                if handle_line_result(output, session_id, result).await {
                    *stdout_reader = None;
                }
            }
            result = read_next_line(stderr_reader) => {
                if handle_line_result(output, session_id, result).await {
                    *stderr_reader = None;
                }
            }
        }

        if stdout_reader.is_none() && stderr_reader.is_none() {
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

    if let Some(reader) = reader.as_mut() {
        reader.lines().next_line().await
    } else {
        Ok(None)
    }
}

async fn handle_line_result(
    output: &mut iced::futures::channel::mpsc::Sender<Message>,
    session_id: Uuid,
    result: std::io::Result<Option<String>>,
) -> bool {
    use iced::futures::SinkExt;

    if let Ok(Some(line)) = result {
        let _ = output
            .send(Message::OutputReceived(session_id, format!("{line}\n")))
            .await;
        false
    } else {
        true
    }
}

async fn wait_for_process(
    output: &mut iced::futures::channel::mpsc::Sender<Message>,
    session_id: Uuid,
    child: &mut Child,
) {
    match child.wait().await {
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
            generic_interpreter_command("python", interp_opts, &script_path_quoted, script_options)
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

#[cfg(test)]
mod tests {
    use super::*;

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
