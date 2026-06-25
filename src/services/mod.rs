mod executor;
mod storage;
mod update_check;

pub use executor::{
    kill_all_running_processes, prewarm_shell_detection, register_running_pid,
    run_configuration_stream, unregister_running_pid,
};
pub use storage::{
    AppSettings, export_text, load_from_path, load_settings, open_configurations,
    save_configurations, save_settings,
};
pub use update_check::{CURRENT_VERSION, UpdateOutcome, check_latest_release};
