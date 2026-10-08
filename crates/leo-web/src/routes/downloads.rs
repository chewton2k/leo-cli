use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::routes::uploads::{originals_dir, safe_file_name};
use crate::{export, AppState};

async fn zip_download(
    build: impl FnOnce(&std::fs::File) -> anyhow::Result<()> + Send + 'static,
    file_name: String,
) -> Response {
    let made = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        use std::io::Seek;
        let tmp = tempfile::NamedTempFile::new()?;
        build(tmp.as_file())?;
        let (mut file, path) = tmp.into_parts();
        file.rewind()?;
        let size = file.metadata()?.len();
        Ok((file, path, size))
    })
    .await;
    let Ok(Ok((file, path, size))) = made else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "The zip could not be made." })),
        )
            .into_response();
    };
    let file = tokio::fs::File::from_std(file);
    let stream = futures_util::stream::unfold(Some((file, path)), |held| async move {
        use tokio::io::AsyncReadExt;
        let (mut file, path) = held?;
        let mut chunk = vec![0u8; 64 * 1024];
        match file.read(&mut chunk).await {
            Ok(0) => {
                drop(path);
                None
            }
            Ok(n) => {
                chunk.truncate(n);
                Some((Ok::<_, std::io::Error>(chunk), Some((file, path))))
            }
            Err(e) => Some((Err(e), None)),
        }
    });
    let mut response = axum::body::Body::from_stream(stream).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/zip"),
    );
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(size));
    if let Ok(value) = HeaderValue::from_str(&attachment_header(&file_name)) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    response
}

pub(crate) fn attachment_header(file_name: &str) -> String {
    let plain: String = file_name
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() && c != '"' && c != '\\' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let encoded: String = file_name
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    format!("attachment; filename=\"{plain}\"; filename*=UTF-8''{encoded}")
}

pub(crate) async fn export_zip(
    State(state): State<AppState>,
    Query(parts): Query<export::Parts>,
) -> Response {
    let notes_dir = match state.with_store(|store| Ok(store.notes_dir.clone())).await {
        Ok(dir) => dir,
        Err(code) => return code.into_response(),
    };
    let chats = state.chats.clone();
    zip_download(
        move |file| export::write_zip(file, &notes_dir, &chats, parts).map(|_| ()),
        format!("leo-export-{}.zip", chrono::Local::now().format("%Y-%m-%d")),
    )
    .await
}

pub(crate) async fn originals_zip(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let found = state
        .with_store(move |store| {
            let note = store.find_note(&id).ok_or(StatusCode::NOT_FOUND)?;
            let folder = originals_dir(&store.notes_dir, &note.id).ok_or(StatusCode::NOT_FOUND)?;
            Ok((folder, note.title.clone()))
        })
        .await;
    let (folder, title) = match found {
        Ok(found) if found.0.is_dir() => found,
        Ok(_) => return StatusCode::NOT_FOUND.into_response(),
        Err(code) => return code.into_response(),
    };
    let base = safe_file_name(&title);
    let name = format!("{} originals.zip", base.trim_end_matches(".zip"));
    zip_download(
        move |file| export::write_folder(file, &folder, &base).map(|_| ()),
        name,
    )
    .await
}
