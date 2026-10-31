use super::catalog::{catalog, Catalog};
use super::library::Library;
use std::io::{Read, Write};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

fn fixture() -> (tempfile::TempDir, Library) {
    let dir = tempfile::tempdir().unwrap();
    let mut data = catalog().clone();
    data.models.retain(|m| m.id == "parakeet-tdt-0.6b-v3");
    let file = data.models[0].files.get_mut("Q8_0").unwrap();
    file.size = 5;
    file.sha256 = format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(b"model"));
    let library = Library::with_catalog(
        dir.path().join("models"),
        dir.path().join("config"),
        Arc::new(data),
    );
    (dir, library)
}

fn model_server(body: &'static [u8]) -> (String, std::thread::JoinHandle<()>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/model", listener.local_addr().unwrap());
    let thread = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let mut buf = [0; 4096];
        let _ = stream.read(&mut buf);
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .unwrap();
        let _ = stream.write_all(body);
    });
    (url, thread)
}

#[test]
fn catalog_has_the_29_curated_models_with_pinned_integrity() {
    let c = catalog();
    assert_eq!(c.curated().count(), 29);
    for m in c.curated() {
        assert!(!m.languages.is_empty());
        assert!(m.file().size > 0);
        assert_eq!(m.file().sha256.len(), 64);
        assert!(m.url().starts_with("https://huggingface.co/"));
        assert_eq!(m.revision.len(), 40);
    }
    for id in ["../x", "/x", "a/b", "a\\b", "", "unknown"] {
        assert!(c.get(id).is_err());
    }
    let roundtrip: Catalog = serde_json::from_str(include_str!("catalog.json")).unwrap();
    assert_eq!(roundtrip.curated().count(), 29);
}

#[test]
fn activation_requires_a_verified_model_and_supported_language() {
    let (_dir, l) = fixture();
    assert!(l.use_model("parakeet-tdt-0.6b-v3", "en").is_err());
    let (url, t) = model_server(b"model");
    l.fetch(
        "parakeet-tdt-0.6b-v3",
        &url,
        &AtomicBool::new(false),
        &|_, _| {},
    )
    .unwrap();
    t.join().unwrap();
    assert!(l.use_model("parakeet-tdt-0.6b-v3", "invalid").is_err());
    l.use_model("parakeet-tdt-0.6b-v3", "en").unwrap();
    assert_eq!(l.selected().unwrap().language, "en");
    let view = l.view().unwrap();
    assert_eq!(view["disk_bytes"], 5);
    assert_eq!(view["models"][0]["downloaded"], true);
}

#[test]
fn a_bad_download_never_replaces_a_good_model() {
    let (_dir, l) = fixture();
    for (bytes, good) in [
        (b"model".as_slice(), true),
        (b"wrong", false),
        (b"oversize", false),
        (b"tiny", false),
    ] {
        let (url, t) = model_server(bytes);
        assert_eq!(
            l.fetch(
                "parakeet-tdt-0.6b-v3",
                &url,
                &AtomicBool::new(false),
                &|_, _| {}
            )
            .is_ok(),
            good
        );
        t.join().unwrap();
        assert_eq!(
            std::fs::read(l.model_path("parakeet-tdt-0.6b-v3").unwrap()).unwrap(),
            b"model"
        );
    }
}

#[test]
fn cancellation_before_promotion_leaves_no_model_or_partial() {
    let (_dir, l) = fixture();
    let cancel = AtomicBool::new(false);
    let (url, t) = model_server(b"model");
    let result = l.fetch("parakeet-tdt-0.6b-v3", &url, &cancel, &|_, _| {
        cancel.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    t.join().unwrap();
    assert!(result.is_err());
    assert!(!l.model_path("parakeet-tdt-0.6b-v3").unwrap().exists());
    assert_eq!(std::fs::read_dir(l.models_dir()).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn model_roots_targets_and_selection_refuse_symbolic_links() {
    use std::os::unix::fs::symlink;
    let (dir, l) = fixture();
    std::fs::create_dir_all(l.models_dir()).unwrap();
    let outside = dir.path().join("outside");
    std::fs::write(&outside, "model").unwrap();
    symlink(&outside, l.model_path("parakeet-tdt-0.6b-v3").unwrap()).unwrap();
    assert!(l.selected().is_err());
    assert!(l.use_model("parakeet-tdt-0.6b-v3", "en").is_err());
    assert!(l.delete("parakeet-tdt-0.6b-v3").is_err());
    let linked = dir.path().join("linked");
    symlink(l.models_dir(), &linked).unwrap();
    let linked_library = Library::new(linked, dir.path().join("c2"));
    assert!(linked_library.model_path("parakeet-tdt-0.6b-v3").is_err());
    assert_eq!(std::fs::read_to_string(outside).unwrap(), "model");
}

#[test]
fn protocol_rejects_oversized_and_nonfinite_pcm() {
    use super::protocol::{encode_message, read_message, Request};
    let request = Request::Hello {
        list_devices: false,
    };
    assert!(encode_message(&request, Some(&[f32::NAN])).is_err());
    let bytes = (u32::MAX).to_le_bytes();
    assert!(read_message::<Request>(&mut bytes.as_slice()).is_err());
}

#[test]
fn lecture_chunking_preserves_all_300_seconds_in_order() {
    let pcm: Vec<f32> = (0..300 * 16_000)
        .map(|n| (n % 1000) as f32 / 1000.0)
        .collect();
    let chunks = super::prepare(pcm.clone()).unwrap();
    assert!(chunks.len() >= 8);
    assert!(chunks.iter().all(|c| c.len() <= 40 * 16_000));
    assert_eq!(chunks.concat(), pcm);
}

#[cfg(unix)]
#[test]
fn models_and_settings_refuse_linked_ancestors_and_hard_linked_files() {
    use std::os::unix::fs::symlink;
    let (dir, l) = fixture();
    let real = dir.path().join("real");
    std::fs::create_dir(&real).unwrap();
    let link = dir.path().join("link");
    symlink(&real, &link).unwrap();
    let linked = Library::new(link.join("speech"), dir.path().join("config"));
    assert!(linked.model_path("parakeet-tdt-0.6b-v3").is_err());
    std::fs::create_dir_all(l.models_dir()).unwrap();
    let outside = dir.path().join("outside");
    std::fs::write(&outside, "model").unwrap();
    std::fs::hard_link(&outside, l.model_path("parakeet-tdt-0.6b-v3").unwrap()).unwrap();
    assert!(l.use_model("parakeet-tdt-0.6b-v3", "en").is_err());
}

#[test]
fn a_download_cancel_interrupts_a_server_that_stops_sending() {
    let (_dir, l) = fixture();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/model", listener.local_addr().unwrap());
    let (ready, started) = std::sync::mpsc::channel();
    let (done, finish) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0; 4096];
        let _ = stream.read(&mut buf).unwrap();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nm")
            .unwrap();
        ready.send(()).unwrap();
        let _ = finish.recv_timeout(std::time::Duration::from_secs(3));
    });
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let worker =
        std::thread::spawn(move || l.fetch("parakeet-tdt-0.6b-v3", &url, &flag, &|_, _| {}));
    started
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(100));
    let now = std::time::Instant::now();
    cancel.store(true, std::sync::atomic::Ordering::Release);
    assert!(worker.join().unwrap().is_err());
    let elapsed = now.elapsed();
    let _ = done.send(());
    server.join().unwrap();
    assert!(
        elapsed < std::time::Duration::from_secs(1),
        "cancellation took {elapsed:?}"
    );
}

#[test]
fn a_damaged_model_in_use_can_be_deleted_and_downloaded_again() {
    let (_dir, l) = fixture();
    let (url, server) = model_server(b"model");
    l.fetch(
        "parakeet-tdt-0.6b-v3",
        &url,
        &AtomicBool::new(false),
        &|_, _| {},
    )
    .unwrap();
    server.join().unwrap();
    l.use_model("parakeet-tdt-0.6b-v3", "en").unwrap();
    std::fs::write(l.model_path("parakeet-tdt-0.6b-v3").unwrap(), "bad").unwrap();
    assert!(!l.view().unwrap()["models"][0]["in_use"].as_bool().unwrap());
    l.delete("parakeet-tdt-0.6b-v3").unwrap();
    assert!(!l.model_path("parakeet-tdt-0.6b-v3").unwrap().exists());
}

#[test]
fn native_speech_and_rust_bindings_have_the_same_version() {
    let lock: toml::Value = toml::from_str(include_str!("../../../../Cargo.lock")).unwrap();
    let packages = lock["package"].as_array().unwrap();
    let version = |name: &str| {
        packages
            .iter()
            .find(|p| p["name"].as_str() == Some(name))
            .unwrap()["version"]
            .as_str()
            .unwrap()
    };
    assert_eq!(version("transcribe-cpp"), version("transcribe-cpp-sys"));
}

#[test]
fn deleting_the_only_downloaded_model_clears_the_choice() {
    let (_dir, l) = fixture();
    let (url, server) = model_server(b"model");
    l.fetch(
        "parakeet-tdt-0.6b-v3",
        &url,
        &AtomicBool::new(false),
        &|_, _| {},
    )
    .unwrap();
    server.join().unwrap();
    l.use_model("parakeet-tdt-0.6b-v3", "en").unwrap();
    l.delete("parakeet-tdt-0.6b-v3").unwrap();
    assert!(l.selection().unwrap().id.is_empty());
    assert!(!l.view().unwrap()["ready"].as_bool().unwrap());
}

#[test]
fn the_old_onnx_folder_is_kept_until_a_verified_download_is_ready() {
    let (dir, l) = fixture();
    let old = dir.path().join("parakeet-tdt-0.6b-v3-int8");
    std::fs::create_dir(&old).unwrap();
    std::fs::write(old.join("encoder.int8.onnx"), "old model").unwrap();
    let (url, server) = model_server(b"wrong");
    assert!(l
        .fetch(
            "parakeet-tdt-0.6b-v3",
            &url,
            &AtomicBool::new(false),
            &|_, _| {}
        )
        .is_err());
    server.join().unwrap();
    assert!(old.exists());
    let (url, server) = model_server(b"model");
    l.fetch(
        "parakeet-tdt-0.6b-v3",
        &url,
        &AtomicBool::new(false),
        &|_, _| {},
    )
    .unwrap();
    server.join().unwrap();
    assert!(!old.exists());
}
