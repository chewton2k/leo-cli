mod auth;
mod felix;
mod housekeeping;
mod notes;
mod recording;
mod trash;
mod uploads;

use std::sync::{Arc, Mutex};

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderValue};
use axum::response::Response;
use axum::Json;
use leo_core::store::Store;

use crate::routes::activity::activity_tasks;
use crate::routes::auth::{
    end_sessions, list_sessions, local_request, new_link, secure_request, CurrentSession,
    EndSessions,
};
use crate::routes::downloads::attachment_header;
use crate::routes::felix::{add_chat_file, chat_reply, list_chat_files, remove_chat_file};
use crate::routes::housekeeping::{get_keep, set_keep};
use crate::routes::notes::{
    create_dir, create_note, delete_note, get_note, list_dirs, list_folders, list_notes, move_note,
    update_note, CreateBody, CreateDirBody, DirParams, ListParams, MoveBody, NoteResponse,
    UpdateBody,
};
use crate::routes::trash::{list_trash, move_to_trash, restore_note, TrashMove};
use crate::routes::uploads::{
    get_original, list_originals, safe_file_name, start_import, upload_label, ImportBody,
    ImportFileBody, ImportJob,
};
use crate::*;

fn state_with(notes: &[(&str, &str)]) -> (AppState, tempfile::TempDir, Vec<String>) {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::load_from(&dir.path().join("notes")).unwrap();
    let ids = notes
        .iter()
        .map(|(title, d)| store.create_note(*title, "", vec![], d).unwrap().id.clone())
        .collect();
    store.save().unwrap();
    let graphs = Arc::new(graph::Graphs::for_notes(&store.notes_dir, None));
    let state = AppState {
        store: Arc::new(Mutex::new(Storage {
            store,
            reload: false,
        })),
        gate: Arc::new(sessions::Gate::new(
            "0123456789abcdef0123456789abcdef".into(),
            None,
            sessions::Sessions::in_memory(),
        )),
        graphs,
        chat: None,
        settings: None,
        importer: None,
        imports: Default::default(),
        listener: None,
        recording: Default::default(),
        chats: dir.path().join("chats"),
        housekeeper: None,
        reader: None,
    };
    (state, dir, ids)
}

fn run<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Runtime::new().unwrap().block_on(f)
}

fn wait_for(state: &AppState, id: &str) -> ImportJob {
    let started = std::time::Instant::now();
    loop {
        let job = state.imports.lock().unwrap().get(id).cloned().unwrap();
        if job.state != "working" {
            return job;
        }
        assert!(
            started.elapsed().as_secs() < 10,
            "the import never finished"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn host(name: &str) -> axum::http::HeaderMap {
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(header::HOST, HeaderValue::from_str(name).unwrap());
    headers
}

fn json_of(response: Response) -> serde_json::Value {
    let bytes = run(axum::body::to_bytes(response.into_body(), usize::MAX)).unwrap();
    serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
}
