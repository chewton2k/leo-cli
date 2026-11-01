fn main() {
    #[cfg(target_os = "macos")]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg=-Wl,-weak_framework,CoreAudio");
        link_clang_runtime();
    }
    tauri_build::build()
}

#[cfg(target_os = "macos")]
fn link_clang_runtime() {
    use std::io::Read;
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    const MOST_OUTPUT: u64 = 8192;
    const TIMEOUT: Duration = Duration::from_secs(5);
    const POLL: Duration = Duration::from_millis(20);
    let mut child = leo_core::children::quiet_command("clang")
        .arg("--print-resource-dir")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .expect("Install the Xcode command line tools to locate the macOS compiler runtime.");
    let stdout = child.stdout.take().expect("clang output is piped");
    let (sender, received) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .take(MOST_OUTPUT + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = sender.send(result);
    });
    let deadline = Instant::now() + TIMEOUT;
    let status = loop {
        if let Some(status) = child.try_wait().expect("Check the compiler helper") {
            break status;
        }
        if Instant::now() >= deadline {
            leo_core::children::stop_tree(&mut child);
            let _ = child.wait();
            panic!("The compiler helper timed out. Check the Xcode command line tools.");
        }
        std::thread::sleep(POLL);
    };
    assert!(
        status.success(),
        "The compiler could not locate its macOS runtime. Check the Xcode command line tools."
    );
    let bytes = received
        .recv_timeout(TIMEOUT)
        .expect("The compiler output did not finish")
        .expect("Read the compiler runtime location");
    assert!(
        bytes.len() <= MOST_OUTPUT as usize,
        "The compiler returned an oversized runtime location."
    );
    let location = String::from_utf8(bytes).expect("The compiler runtime location must be UTF-8");
    let directory = std::path::Path::new(location.trim()).join("lib/darwin");
    assert!(
        directory.join("libclang_rt.osx.a").is_file(),
        "The macOS compiler runtime is missing. Reinstall the Xcode command line tools."
    );
    println!("cargo:rustc-link-search=native={}", directory.display());
    println!("cargo:rustc-link-lib=static=clang_rt.osx");
}
