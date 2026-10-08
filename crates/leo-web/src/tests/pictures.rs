use super::*;

const PICTURE: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x10\0\0\0\x10\x08\x02\0\0\0";

fn picture_at(state: &AppState, path: &str, from: &str) -> Response {
    run(get_picture(
        State(state.clone()),
        Query(PictureAt {
            path: path.into(),
            from: from.into(),
        }),
    ))
}

#[test]
fn a_pasted_picture_is_saved_beside_the_notes_and_served_back() {
    use base64::Engine;
    let (state, _d, _ids) = state_with(&[]);
    let added = run(add_picture(
        State(state.clone()),
        Json(NewPicture {
            name: "pasted-1.png".into(),
            data: base64::engine::general_purpose::STANDARD.encode(PICTURE),
        }),
    ));
    assert_eq!(added.status(), StatusCode::CREATED);
    let path = json_of(added)["path"].as_str().unwrap().to_string();
    assert!(
        path.starts_with("attachments/") && path.ends_with("-pasted-1.png"),
        "{path}"
    );

    let served = picture_at(&state, &path, "cs130");
    assert_eq!(served.status(), StatusCode::OK);
    assert_eq!(served.headers()[header::CONTENT_TYPE], "image/png");
    let bytes = run(axum::body::to_bytes(served.into_body(), usize::MAX)).unwrap();
    assert_eq!(&bytes[..], PICTURE);

    let notes_dir = state.fresh().notes_dir.clone();
    std::fs::write(notes_dir.join("fake.png"), b"<svg onload=alert(1)>").unwrap();
    for (bad, from) in [
        ("../graph.json", ""),
        ("fake.png", ""),
        ("missing.png", ""),
        (path.as_str(), "../.."),
    ] {
        assert_eq!(
            picture_at(&state, bad, from).status(),
            StatusCode::NOT_FOUND,
            "{bad} from {from}"
        );
    }
    for (name, data) in [("x.svg", "PHN2Zy8+"), ("x.png", "not base64!")] {
        let refused = run(add_picture(
            State(state.clone()),
            Json(NewPicture {
                name: name.into(),
                data: data.into(),
            }),
        ));
        assert_eq!(refused.status(), StatusCode::BAD_REQUEST, "{name}");
    }
}
