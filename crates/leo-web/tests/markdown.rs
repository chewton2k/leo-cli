use std::process::Command;

#[test]
fn the_markdown_renderer_passes_its_tests() {
    run_node_tests("markdown.test.js");
}

#[test]
fn the_editor_helpers_pass_their_tests() {
    run_node_tests("editing.test.js");
    run_node_tests("margin.test.js");
    run_node_tests("drag.test.js");
    run_node_tests("pane-resize.test.js");
}

#[test]
fn the_chat_with_felix_passes_its_tests() {
    run_node_tests("chat.test.js");
    run_node_tests("study-survival.test.js");
    run_node_tests("retirement.test.js");
}

#[test]
fn the_calendar_helpers_pass_their_tests() {
    run_node_tests("schedule.test.js");
}

#[test]
fn saving_and_draft_recovery_pass_their_tests() {
    run_node_tests("printing.test.js");
    run_node_tests("coding-plans.test.js");
    run_node_tests("search-highlights.test.js");
    run_node_tests("motion.test.js");
    run_node_tests("browser-server.test.js");
    run_node_tests("saving.test.js");
    run_node_tests("website-leave.test.js");
    run_node_tests("browser-storage.test.js");
}

#[test]
fn recording_in_the_browser_passes_its_tests() {
    run_node_tests("recording.test.js");
    run_node_tests("native-recording.test.js");
    run_node_tests("dictation.test.js");
    run_node_tests("settings-navigation.test.js");
    run_node_tests("model-library.test.js");
    run_node_tests("onboarding.test.js");
    run_node_tests("app-settings.test.js");
    run_node_tests("settings-lifecycle.test.js");
    run_node_tests("settings-route-state.test.js");
    run_node_tests("route-lifecycle.test.js");
    run_node_tests("menu-motion.test.js");
    run_node_tests("audio-queue.test.js");
}

fn run_node_tests(file: &str) {
    let dir = env!("CARGO_MANIFEST_DIR");
    let Ok(out) = Command::new("node")
        .arg(format!("{dir}/tests/{file}"))
        .output()
    else {
        eprintln!("skipping: node is not installed");
        return;
    };
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{said}");
}

#[test]
fn navigation_passes_its_tests() {
    run_node_tests("navigation.test.js");
}
