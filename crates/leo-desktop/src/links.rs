use std::path::{Path, PathBuf};

use url::Url;

pub fn stays_inside(url: &Url, port: u16) -> bool {
    if url.scheme() == "tauri" || url.host_str() == Some("tauri.localhost") {
        return true;
    }
    url.scheme() == "http"
        && matches!(url.host_str(), Some("127.0.0.1") | Some("localhost"))
        && url.port() == Some(port)
}

pub fn download_path(folder: &Path, suggested: &str) -> PathBuf {
    let name = Path::new(suggested)
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty())
        .unwrap_or("download");
    let first = folder.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, extension) = match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem, format!(".{extension}")),
        _ => (name, String::new()),
    };
    (2..)
        .map(|n| folder.join(format!("{stem} ({n}){extension}")))
        .find(|path| !path.exists())
        .unwrap_or(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_apps_own_pages_stay_inside() {
        assert!(stays_inside(
            &"http://127.0.0.1:47318/#/n/abc".parse().unwrap(),
            47318
        ));
        assert!(stays_inside(
            &"http://localhost:47318/".parse().unwrap(),
            47318
        ));
        assert!(!stays_inside(
            &"http://127.0.0.1:9999/".parse().unwrap(),
            47318
        ));
        assert!(!stays_inside(
            &"https://calendar.google.com/".parse().unwrap(),
            47318
        ));
        assert!(!stays_inside(&"mailto:a@b.c".parse().unwrap(), 47318));
        assert!(stays_inside(
            &"tauri://localhost/index.html".parse().unwrap(),
            47318
        ));
    }

    #[test]
    fn a_download_never_replaces_a_file_already_there() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            download_path(dir.path(), "leo-export.zip"),
            dir.path().join("leo-export.zip")
        );
        std::fs::write(dir.path().join("leo-export.zip"), "").unwrap();
        assert_eq!(
            download_path(dir.path(), "leo-export.zip"),
            dir.path().join("leo-export (2).zip")
        );
        assert_eq!(
            download_path(dir.path(), "../../etc/passwd"),
            dir.path().join("passwd")
        );
        assert_eq!(download_path(dir.path(), ""), dir.path().join("download"));
    }
}
