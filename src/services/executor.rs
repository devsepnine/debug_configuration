use crate::messages::Message;
use crate::models::{ConfigTypeData, ExecuteMode, NodeCommand, PackageManager, RunConfiguration};
use iced::stream;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::process::{Child, ChildStderr, ChildStdout};
use uuid::Uuid;

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
            let command_str = build_command_string(&config);
            let mut cmd = create_process_command(&config, &command_str);
            send_command_info(&mut output, session_id, &config, &command_str).await;

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

fn build_command_string(config: &RunConfiguration) -> String {
    match &config.type_data {
        ConfigTypeData::Application { command, arguments } => {
            build_application_command(&config.environment_variables, command, arguments)
        }
        ConfigTypeData::ShellScript { execute_mode } => {
            build_shell_script_command(&config.environment_variables, execute_mode)
        }
        ConfigTypeData::Node {
            project_directory: _,
            package_manager,
            node_runtime_path,
            command,
            script_name,
            arguments,
            node_options,
        } => build_node_command(
            &config.environment_variables,
            NodeCommandParts {
                package_manager,
                node_runtime_path: node_runtime_path.as_ref(),
                command: *command,
                script_name: script_name.as_ref(),
                arguments,
                node_options,
                working_directory: &config.working_directory,
            },
        ),
    }
}

fn create_process_command(config: &RunConfiguration, command_str: &str) -> tokio::process::Command {
    use std::process::Stdio;
    use tokio::process::Command;

    let mut cmd = if cfg!(target_os = "windows") {
        let mut c = Command::new("powershell");
        c.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!("[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; {command_str}"),
        ]);
        c
    } else {
        let mut c = Command::new("sh");
        c.args(["-l", "-c", command_str]);
        c
    };

    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    #[cfg(unix)]
    {
        cmd.process_group(0);
    }

    cmd.current_dir(&config.working_directory);
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
) {
    use iced::futures::SinkExt;

    let mut command_info = String::new();
    command_info.push_str("═══════════════════════════════════════════════════════\n");
    let _ = writeln!(command_info, "Configuration: {}", config.name);
    let _ = writeln!(command_info, "Type: {:?}", config.config_type);
    let _ = writeln!(
        command_info,
        "Working Directory: {}",
        config.working_directory
    );
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

    if process_output_loop(
        output,
        session_id,
        &mut child,
        &mut stdout_reader,
        &mut stderr_reader,
        &cancel_flag,
    )
    .await
    {
        return;
    }

    wait_for_process(output, session_id, &mut child).await;
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

async fn process_output_loop(
    output: &mut iced::futures::channel::mpsc::Sender<Message>,
    session_id: Uuid,
    child: &mut Child,
    stdout_reader: &mut Option<tokio::io::BufReader<ChildStdout>>,
    stderr_reader: &mut Option<tokio::io::BufReader<ChildStderr>>,
    cancel_flag: &Arc<AtomicBool>,
) -> bool {
    loop {
        tokio::select! {
            () = wait_for_cancel(cancel_flag) => {
                #[cfg(any(unix, windows))]
                {
                    stop_process(child);
                }
                #[cfg(not(any(unix, windows)))]
                {
                    stop_process(child).await;
                }
                send_run_result(
                    output,
                    session_id,
                    Err(String::from("Process stopped by user")),
                )
                .await;
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

async fn wait_for_cancel(cancel_flag: &Arc<AtomicBool>) {
    loop {
        if cancel_flag.load(Ordering::Relaxed) {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
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

#[cfg(any(unix, windows))]
fn stop_process(child: &mut Child) {
    #[cfg(unix)]
    {
        if let Some(pid) = child.id() {
            unsafe {
                libc::killpg(pid as i32, libc::SIGTERM);
            }
        }
    }
    #[cfg(windows)]
    {
        if let Some(pid) = child.id() {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            let _ = std::process::Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .creation_flags(CREATE_NO_WINDOW)
                .output();
        }
    }
}

#[cfg(not(any(unix, windows)))]
async fn stop_process(child: &mut Child) {
    let _ = child.kill().await;
}

/// 환경 변수를 플랫폼별 설정 명령어로 변환
fn build_env_prefix(env_vars: &HashMap<String, String>) -> String {
    if env_vars.is_empty() {
        return String::new();
    }

    #[cfg(target_os = "windows")]
    {
        env_vars
            .iter()
            .map(|(k, v)| {
                // 값에 따옴표가 있을 경우 이스케이프 처리
                let escaped_value = v.replace('\'', "''");
                format!("$env:{k}='{escaped_value}'")
            })
            .collect::<Vec<_>>()
            .join("; ")
    }

    #[cfg(not(target_os = "windows"))]
    {
        env_vars
            .iter()
            .map(|(k, v)| format!("export {k}='{v}'"))
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// Application 타입의 명령어 생성
fn build_application_command(
    env_vars: &HashMap<String, String>,
    command: &str,
    arguments: &str,
) -> String {
    let env_prefix = build_env_prefix(env_vars);

    let command_with_args = if arguments.is_empty() {
        command.to_string()
    } else {
        format!("{command} {arguments}")
    };

    // PowerShell 5.x는 && 연산자를 지원하지 않으므로 ; 로 변환
    #[cfg(target_os = "windows")]
    let command_with_args = command_with_args.replace(" && ", "; ");

    if env_prefix.is_empty() {
        command_with_args
    } else {
        format!("{env_prefix}; {command_with_args}")
    }
}

/// Shell Script 타입의 명령어 생성
fn build_shell_script_command(
    env_vars: &HashMap<String, String>,
    execute_mode: &ExecuteMode,
) -> String {
    match execute_mode {
        ExecuteMode::ScriptFile {
            script_path,
            script_options,
            interpreter_path,
            interpreter_options,
        } => build_script_file_command(
            env_vars,
            script_path,
            script_options,
            interpreter_path.as_deref(),
            interpreter_options.as_deref(),
        ),
        ExecuteMode::ScriptText { script_text } => build_script_text_command(env_vars, script_text),
    }
}

/// Script File 모드의 명령어 생성
fn build_script_file_command(
    env_vars: &HashMap<String, String>,
    script_path: &str,
    script_options: &str,
    interpreter_path: Option<&str>,
    interpreter_options: Option<&str>,
) -> String {
    use std::path::Path;

    let env_prefix = build_env_prefix(env_vars);
    let script_extension = Path::new(script_path)
        .extension()
        .and_then(|ext| ext.to_str());

    // 인터프리터 자동 감지
    let interpreter_base = interpreter_path.unwrap_or_else(|| {
        if script_extension.is_some_and(|ext| ext.eq_ignore_ascii_case("py")) {
            "python"
        } else if script_extension.is_some_and(|ext| ext.eq_ignore_ascii_case("js")) {
            "node"
        } else if script_extension.is_some_and(|ext| ext.eq_ignore_ascii_case("rb")) {
            "ruby"
        } else if script_extension.is_some_and(|ext| ext.eq_ignore_ascii_case("ps1")) {
            "powershell"
        } else if script_extension
            .is_some_and(|ext| ext.eq_ignore_ascii_case("bat") || ext.eq_ignore_ascii_case("cmd"))
        {
            "cmd"
        } else {
            "sh"
        }
    });

    // 공백이 포함된 경로 처리
    let interpreter = quote_if_needed(interpreter_base);
    let script_path_quoted = quote_if_needed(script_path);

    // Windows PowerShell에서는 공백이 포함된 경로를 & "경로" 형태로 실행
    #[cfg(target_os = "windows")]
    let interpreter_cmd = if interpreter.starts_with('"') {
        format!("& {interpreter}")
    } else {
        interpreter
    };

    #[cfg(not(target_os = "windows"))]
    let interpreter_cmd = interpreter;

    let interp_opts = interpreter_options.unwrap_or("");

    let command = format!("{interpreter_cmd} {interp_opts} {script_path_quoted} {script_options}")
        .trim()
        .to_string();

    if env_prefix.is_empty() {
        command
    } else {
        format!("{env_prefix}; {command}")
    }
}

/// Script Text 모드의 명령어 생성
fn build_script_text_command(env_vars: &HashMap<String, String>, script_text: &str) -> String {
    let env_prefix = build_env_prefix(env_vars);

    if env_prefix.is_empty() {
        script_text.to_string()
    } else {
        format!("{env_prefix}; {script_text}")
    }
}

/// Node package manager 타입의 명령어 생성
#[derive(Clone, Copy)]
struct NodeCommandParts<'a> {
    package_manager: &'a PackageManager,
    node_runtime_path: Option<&'a String>,
    command: NodeCommand,
    script_name: Option<&'a String>,
    arguments: &'a str,
    node_options: &'a str,
    working_directory: &'a str,
}

fn build_node_command(env_vars: &HashMap<String, String>, parts: NodeCommandParts<'_>) -> String {
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
        package_manager.clone()
    };

    #[cfg(not(target_os = "windows"))]
    let package_manager_cmd = package_manager.clone();

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

    let base_command = cmd_parts.join(" ");

    // 환경 변수 추가
    let env_prefix = build_env_prefix(env_vars);

    // NODE_OPTIONS 처리
    let node_opts_env = if parts.node_options.is_empty() {
        String::new()
    } else {
        #[cfg(target_os = "windows")]
        {
            format!("$env:NODE_OPTIONS='{}'", parts.node_options)
        }

        #[cfg(not(target_os = "windows"))]
        {
            format!("export NODE_OPTIONS='{}'", parts.node_options)
        }
    };

    // 조합
    let mut parts = Vec::new();
    if !env_prefix.is_empty() {
        parts.push(env_prefix);
    }
    if !node_opts_env.is_empty() {
        parts.push(node_opts_env);
    }
    parts.push(base_command);

    parts.join("; ")
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
}
