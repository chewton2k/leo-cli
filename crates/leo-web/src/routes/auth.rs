use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::Json;
use colored::Colorize;
use serde::Deserialize;

use crate::terminal::clickable;
use crate::{sessions, AppState};

const COOKIE_DAYS: u32 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Peer {
    pub(crate) loopback: bool,
}

pub(crate) fn peer_of(connected: Option<&ConnectInfo<SocketAddr>>) -> Peer {
    Peer {
        loopback: connected.is_some_and(|ConnectInfo(addr)| addr.ip().is_loopback()),
    }
}

pub(crate) async fn note_peer(mut request: Request, next: Next) -> Response {
    let peer = peer_of(request.extensions().get::<ConnectInfo<SocketAddr>>());
    request.extensions_mut().insert(peer);
    next.run(request).await
}

pub(crate) fn secure_request(headers: &axum::http::HeaderMap, peer: Peer) -> bool {
    let forwarded = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("https"));
    peer.loopback && (forwarded || local_host(headers))
}

pub(crate) fn local_request(headers: &axum::http::HeaderMap, peer: Peer) -> bool {
    peer.loopback
        && !headers.contains_key("x-forwarded-proto")
        && !headers.contains_key("x-forwarded-for")
        && !headers.contains_key("cf-connecting-ip")
        && local_host(headers)
}

fn local_host(headers: &axum::http::HeaderMap) -> bool {
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let name = if host.starts_with('[') {
        host.split(']')
            .next()
            .map(|h| format!("{h}]"))
            .unwrap_or_default()
    } else {
        host.split(':').next().unwrap_or("").to_string()
    };
    matches!(name.as_str(), "localhost" | "127.0.0.1" | "[::1]")
}

#[derive(Clone)]
pub(crate) struct CurrentSession(pub(crate) String);

fn site(gate: &sessions::Gate, request: &Request) -> String {
    let local = request
        .extensions()
        .get::<Peer>()
        .is_some_and(|peer| peer.loopback);
    gate.site_for(local, local && tunnelled(request))
}

pub(crate) fn user_agent(request: &Request) -> String {
    request
        .headers()
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string()
}

pub(crate) fn tunnelled(request: &Request) -> bool {
    request
        .headers()
        .get("x-forwarded-proto")
        .is_some_and(|v| v.as_bytes().eq_ignore_ascii_case(b"https"))
}

pub(crate) async fn auth_middleware(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let now = chrono::Utc::now();
    let https = tunnelled(&request);
    let query = request.uri().query().unwrap_or("");
    let cookies = request
        .headers()
        .get(header::COOKIE)
        .and_then(|c| c.to_str().ok())
        .unwrap_or("")
        .to_string();
    let signed_in = sessions::cookie_value(&cookies, sessions::COOKIE)
        .filter(|secret| state.gate.sessions.check(secret, now))
        .map(str::to_string);
    if let Some(given) = query_param(query, "token") {
        if state.gate.token_matches(given) {
            let secret = match &signed_in {
                Some(secret) => secret.clone(),
                None => state.gate.sessions.start(
                    &user_agent(&request),
                    &site(&state.gate, &request),
                    now,
                ),
            };
            request
                .extensions_mut()
                .insert(CurrentSession(secret.clone()));
            let mut response =
                if request.method() == axum::http::Method::GET && request.uri().path() == "/" {
                    Redirect::to("/").into_response()
                } else {
                    next.run(request).await
                };
            set_session_cookies(&mut response, &secret, https);
            return response;
        }
    }

    if let Some(secret) = signed_in {
        request.extensions_mut().insert(CurrentSession(secret));
        return next.run(request).await;
    }
    let legacy = sessions::cookie_value(&cookies, sessions::LEGACY_COOKIE)
        .is_some_and(|v| state.gate.token_matches(v));
    if legacy {
        let secret =
            state
                .gate
                .sessions
                .start(&user_agent(&request), &site(&state.gate, &request), now);
        request
            .extensions_mut()
            .insert(CurrentSession(secret.clone()));
        let mut response = next.run(request).await;
        set_session_cookies(&mut response, &secret, https);
        return response;
    }

    if request.uri().path() == "/" {
        return (StatusCode::UNAUTHORIZED, Html(LOCKED)).into_response();
    }
    StatusCode::UNAUTHORIZED.into_response()
}

pub(crate) async fn list_sessions(
    State(state): State<AppState>,
    current: Option<axum::Extension<CurrentSession>>,
) -> Json<serde_json::Value> {
    let current = current.map(|axum::Extension(c)| c.0);
    Json(serde_json::json!({
        "sessions": state.gate.sessions.list(current.as_deref(), chrono::Utc::now()),
    }))
}

#[derive(Deserialize)]
pub(crate) struct EndSessions {
    #[serde(default)]
    pub(crate) handle: Option<String>,
    #[serde(default)]
    pub(crate) others: bool,
}

pub(crate) async fn end_sessions(
    State(state): State<AppState>,
    current: Option<axum::Extension<CurrentSession>>,
    Json(body): Json<EndSessions>,
) -> Response {
    let current = current.map(|axum::Extension(c)| c.0);
    let ended = match (body.others, body.handle, current) {
        (true, _, Some(mine)) => state.gate.sessions.end_others(&mine),
        (false, Some(handle), _) => usize::from(state.gate.sessions.end(&handle)),
        _ => return StatusCode::BAD_REQUEST.into_response(),
    };
    Json(serde_json::json!({ "ended": ended })).into_response()
}

pub(crate) async fn new_link(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Response {
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .filter(|h| !h.is_empty() && !h.contains(['/', ' ', '\\']))
        .unwrap_or("127.0.0.1")
        .to_string();
    let scheme = if headers
        .get("x-forwarded-proto")
        .is_some_and(|v| v.as_bytes().eq_ignore_ascii_case(b"https"))
    {
        "https"
    } else {
        "http"
    };
    let gate = Arc::clone(&state.gate);
    match tokio::task::spawn_blocking(move || gate.new_link_code()).await {
        Ok(Ok(code)) => {
            let link = format!("{scheme}://{host}/?token={code}");
            if !leo_core::diag::is_quiet() {
                println!();
                println!("  {}", "A new link was made from the website:".bold());
                println!("    {}", clickable(&link));
                println!(
                    "  {}",
                    "Older links stop working for browsers that are not signed in yet.".dimmed()
                );
            }
            Json(serde_json::json!({ "link": link })).into_response()
        }
        _ => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

fn set_session_cookies(response: &mut Response, secret: &str, https: bool) {
    let secure = if https { "; Secure" } else { "" };
    for cookie in [
        format!(
            "{}={secret}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{secure}",
            sessions::COOKIE,
            COOKIE_DAYS * 24 * 3600
        ),
        format!(
            "{}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure}",
            sessions::LEGACY_COOKIE
        ),
    ] {
        if let Ok(value) = HeaderValue::from_str(&cookie) {
            response.headers_mut().append(header::SET_COOKIE, value);
        }
    }
}

const LOCKED: &str = "<!doctype html><meta name=viewport content='width=device-width'>\
<title>leo</title><body style='font-family:system-ui;padding:2em;line-height:1.5'>\
<h2>This page needs its link</h2><p>Open the link <code>leo serve</code> printed, \
or scan its QR code again. If the link was changed with \
<code>leo serve --new-token</code>, older links no longer work.</p>";

pub(crate) async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    for (name, value) in [
        ("referrer-policy", "no-referrer"),
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        ("cache-control", "no-store"),
        (
            "content-security-policy",
            "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
             img-src 'self' data:; connect-src 'self'; frame-src 'self'; base-uri 'none'; form-action 'self'; \
             frame-ancestors 'none'",
        ),
    ] {
        if !headers.contains_key(name) {
            headers.insert(name, HeaderValue::from_static(value));
        }
    }
    response
}

fn query_param<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == key {
                return Some(v);
            }
        }
    }
    None
}
