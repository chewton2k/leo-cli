use std::process::Command;

#[test]
fn the_markdown_renderer_passes_its_tests() {
    let dir = env!("CARGO_MANIFEST_DIR");
    let Ok(out) = Command::new("node")
        .arg(format!("{dir}/tests/markdown.test.js"))
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
