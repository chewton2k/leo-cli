use axum::http::header;
use axum::response::{Html, IntoResponse, Response};

const HTML: &str = include_str!("../web/index.html");
const APP_JS: &str = concat!(
    "(function () {\n'use strict';\n",
    include_str!("../web/app/core.js"),
    include_str!("../web/app/trash.js"),
    include_str!("../web/app/map.js"),
    include_str!("../web/app/settings.js"),
    include_str!("../web/app/storage.js"),
    include_str!("../web/app/uploads.js"),
    include_str!("../web/app/sheets.js"),
    include_str!("../web/app/side.js"),
    include_str!("../web/app/drag.js"),
    include_str!("../web/app/diagrams.js"),
    include_str!("../web/app/actions.js"),
    include_str!("../web/app/events.js"),
    "})();\n",
);
const MARKDOWN_JS: &str = include_str!("../web/markdown.js");
const MERMAID_JS: &str = include_str!("../web/vendor/mermaid.min.js");
pub(crate) const MERMAID_PATH: &str = "/vendor/mermaid-11.4.1.js";
const EDITING_JS: &str = include_str!("../web/editing.js");
const SAVING_JS: &str = include_str!("../web/saving.js");
const DOC_JS: &str = include_str!("../web/doc.js");
const GRAPH_JS: &str = include_str!("../web/graph.js");
const CHAT_JS: &str = include_str!("../web/chat.js");
const RECORDER_JS: &str = include_str!("../web/recorder.js");
const RECORDING_JS: &str = include_str!("../web/recording.js");

pub(crate) async fn index() -> Html<&'static str> {
    Html(HTML)
}

fn javascript(source: &'static str) -> Response {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        source,
    )
        .into_response()
}

pub(crate) async fn app_js() -> Response {
    javascript(APP_JS)
}

pub(crate) async fn mermaid_js() -> Response {
    (
        [
            (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (
                header::CACHE_CONTROL,
                "private, max-age=31536000, immutable",
            ),
        ],
        MERMAID_JS,
    )
        .into_response()
}

pub(crate) async fn markdown_js() -> Response {
    javascript(MARKDOWN_JS)
}

pub(crate) async fn editing_js() -> Response {
    javascript(EDITING_JS)
}

pub(crate) async fn saving_js() -> Response {
    javascript(SAVING_JS)
}

pub(crate) async fn doc_js() -> Response {
    javascript(DOC_JS)
}

pub(crate) async fn graph_js() -> Response {
    javascript(GRAPH_JS)
}

const FAVICON: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"-1 -1 46 34\" shape-rendering=\"crispEdges\">\
<rect x=\"0\" y=\"18\" width=\"5\" height=\"6\" fill=\"#b4cfe7\"/>\
<rect x=\"39\" y=\"18\" width=\"5\" height=\"6\" fill=\"#b4cfe7\"/>\
<rect x=\"5\" y=\"2\" width=\"34\" height=\"28\" fill=\"#b4cfe7\"/>\
<g shape-rendering=\"geometricPrecision\" fill=\"#19191b\">\
<rect x=\"11\" y=\"16\" width=\"4\" height=\"4\"/><rect x=\"21\" y=\"16\" width=\"4\" height=\"4\"/></g></svg>";

pub(crate) async fn favicon() -> Response {
    (
        [
            (header::CONTENT_TYPE, "image/svg+xml"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        FAVICON,
    )
        .into_response()
}

pub(crate) async fn chat_js() -> Response {
    javascript(CHAT_JS)
}

pub(crate) async fn recorder_js() -> Response {
    javascript(RECORDER_JS)
}

pub(crate) async fn recording_js() -> Response {
    javascript(RECORDING_JS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_part_of_the_page_script_is_served() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/web/app");
        let mut parts = 0;
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "js") {
                let text = std::fs::read_to_string(&path).unwrap();
                assert!(
                    APP_JS.contains(&text),
                    "{} is not in /app.js",
                    path.display()
                );
                parts += 1;
            }
        }
        assert_eq!(parts, 12);
    }

    #[test]
    fn the_page_script_parses() {
        let file = tempfile::Builder::new().suffix(".js").tempfile().unwrap();
        std::fs::write(file.path(), APP_JS).unwrap();
        let Ok(out) = std::process::Command::new("node")
            .arg("--check")
            .arg(file.path())
            .output()
        else {
            eprintln!("skipping: node is not installed");
            return;
        };
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
