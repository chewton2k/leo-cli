#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if leo_services::speech::is_worker_invocation() {
        std::process::exit(leo_services::speech::run_worker());
    }
    leo_desktop::run()
}
