use std::process::Command;

#[test]
fn the_markdown_renderer_passes_its_tests() {
    run_node_tests("markdown.test.js");
}

#[test]
fn the_editor_helpers_pass_their_tests() {
    run_node_tests("editing.test.js");
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
