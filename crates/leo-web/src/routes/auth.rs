use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Redirect, Response};
use std::net::SocketAddr;

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

pub(crate) async fn same_host(port: u16, request: Request, next: Next) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let ours = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    if !ours.iter().any(|h| h.eq_ignore_ascii_case(host)) {
        return StatusCode::MISDIRECTED_REQUEST.into_response();
    }
    next.run(request).await
}

fn site(gate: &sessions::Gate) -> String {
    gate.site_for(true, false)
}

pub(crate) fn user_agent(request: &Request) -> String {
    request
        .headers()
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string()
}

pub(crate) async fn auth_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let now = chrono::Utc::now();
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
            let secret =
                match &signed_in {
                    Some(secret) => secret.clone(),
                    None => match state.gate.sessions.start(
                        &user_agent(&request),
                        &site(&state.gate),
                        now,
                    ) {
                        Ok(secret) => secret,
                        Err(_) => return session_error(),
                    },
                };
            let mut response =
                if request.method() == axum::http::Method::GET && request.uri().path() == "/" {
                    Redirect::to("/").into_response()
                } else {
                    next.run(request).await
                };
            set_session_cookies(&mut response, &secret);
            return response;
        }
    }

    if signed_in.is_some() {
        return next.run(request).await;
    }
    let legacy = sessions::cookie_value(&cookies, sessions::LEGACY_COOKIE)
        .is_some_and(|v| state.gate.token_matches(v));
    if legacy {
        let secret = match state
            .gate
            .sessions
            .start(&user_agent(&request), &site(&state.gate), now)
        {
            Ok(secret) => secret,
            Err(_) => return session_error(),
        };
        let mut response = next.run(request).await;
        set_session_cookies(&mut response, &secret);
        return response;
    }

    if request.uri().path() == "/" {
        return (StatusCode::UNAUTHORIZED, Html(LOCKED)).into_response();
    }
    StatusCode::UNAUTHORIZED.into_response()
}

fn set_session_cookies(response: &mut Response, secret: &str) {
    for cookie in [
        format!(
            "{}={secret}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}",
            sessions::COOKIE,
            COOKIE_DAYS * 24 * 3600
        ),
        format!(
            "{}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0",
            sessions::LEGACY_COOKIE
        ),
    ] {
        if let Ok(value) = HeaderValue::from_str(&cookie) {
            response.headers_mut().append(header::SET_COOKIE, value);
        }
    }
}

fn session_error() -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        axum::Json(serde_json::json!({"error": "Sign-in changes could not be saved. Check Leo's config folder permissions and try again."})),
    ).into_response()
}

const LOCKED: &str = "<!doctype html><meta name=viewport content='width=device-width'>\
<title>leo</title><body style='font-family:system-ui;padding:2em;line-height:1.5'>\
<h2>leo opens in its own window</h2><p>Open the leo app to see your notes.</p>";

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
             img-src 'self' data:; font-src 'self'; connect-src 'self' ipc: http://ipc.localhost; frame-src 'self'; base-uri 'none'; form-action 'self'; \
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
