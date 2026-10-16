use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

pub const SERVER: &str = "leo";
const MOST_HEADERS: usize = 32 * 1024;
const MOST_BODY: usize = 8 * 1024 * 1024;
const CALL_LIMIT: Duration = Duration::from_secs(15 * 60);
const IDLE: Duration = Duration::from_secs(20 * 60);
const MOST_CONNECTIONS: usize = 8;
const MOST_REQUESTS: usize = 1024;
const MOST_BATCH: usize = 32;
const WRITE_LIMIT: Duration = Duration::from_secs(5);
const STOP_POLL: Duration = Duration::from_millis(50);
const WAKE_LIMIT: Duration = Duration::from_millis(250);

pub enum Incoming {
    Line(String),
    Failed,
    Call {
        name: String,
        args: Value,
        reply: Sender<String>,
    },
}

#[derive(Clone)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub schema: Value,
}

pub struct Bridge {
    pub url: String,
    stop: Arc<AtomicBool>,
    port: u16,
    connections: Arc<Connections>,
}

#[derive(Default)]
struct Connections {
    sockets: Mutex<BTreeMap<usize, TcpStream>>,
    active: AtomicUsize,
}

struct Connection {
    id: usize,
    connections: Arc<Connections>,
}

impl Connections {
    fn register(self: &Arc<Self>, stream: &TcpStream, stopped: &AtomicBool) -> Option<Connection> {
        let mut socket = Some(stream.try_clone().ok()?);
        let id = {
            let mut sockets = self.sockets.lock().ok()?;
            if stopped.load(Ordering::SeqCst) {
                None
            } else {
                let id = (0..MOST_CONNECTIONS).find(|id| !sockets.contains_key(id));
                if let Some(id) = id {
                    sockets.insert(id, socket.take()?);
                    self.active.fetch_add(1, Ordering::SeqCst);
                }
                id
            }
        };
        Some(Connection {
            id: id?,
            connections: Arc::clone(self),
        })
    }

    fn close(&self) {
        let sockets = self
            .sockets
            .lock()
            .map(|mut sockets| std::mem::take(&mut *sockets))
            .unwrap_or_default();
        for stream in sockets.into_values() {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        let socket = self
            .connections
            .sockets
            .lock()
            .ok()
            .and_then(|mut sockets| sockets.remove(&self.id));
        drop(socket);
        self.connections.active.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Bridge {
    pub fn start(tools: Vec<Tool>, to: SyncSender<Incoming>) -> std::io::Result<Bridge> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        let secret = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let path = format!("/mcp/{secret}");
        let stop = Arc::new(AtomicBool::new(false));
        let connections = Arc::new(Connections::default());
        let served = Served {
            path: path.clone(),
            tools: Arc::new(tools),
            to,
            stopped: Arc::clone(&stop),
        };
        let stopped = Arc::clone(&stop);
        let clients = Arc::clone(&connections);
        std::thread::Builder::new()
            .name("leo-mcp".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    if stopped.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(stream) = stream else { continue };
                    if !stream.peer_addr().is_ok_and(|a| a.ip().is_loopback()) {
                        let _ = stream.shutdown(Shutdown::Both);
                        continue;
                    }
                    let Some(connection) = clients.register(&stream, &stopped) else {
                        let _ = stream.shutdown(Shutdown::Both);
                        continue;
                    };
                    let served = served.clone();
                    let _ = std::thread::Builder::new()
                        .name("leo-mcp-conn".into())
                        .spawn(move || {
                            let _connection = connection;
                            served.connection(stream);
                        });
                }
            })?;
        Ok(Bridge {
            url: format!("http://127.0.0.1:{port}{path}"),
            stop,
            port,
            connections,
        })
    }

    pub fn config(&self) -> String {
        json!({ "mcpServers": { SERVER: { "type": "http", "url": self.url } } }).to_string()
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.connections.close();
        let address = std::net::SocketAddr::from(([127, 0, 0, 1], self.port));
        let _ = TcpStream::connect_timeout(&address, WAKE_LIMIT);
    }
}

#[derive(Clone)]
struct Served {
    path: String,
    tools: Arc<Vec<Tool>>,
    to: SyncSender<Incoming>,
    stopped: Arc<AtomicBool>,
}

struct Request {
    method: String,
    path: String,
    origin: bool,
    body: Vec<u8>,
    close: bool,
}

fn read_request(reader: &mut BufReader<TcpStream>) -> Option<Request> {
    let mut head = Vec::new();
    loop {
        let mut line = Vec::new();
        let read = reader
            .by_ref()
            .take((MOST_HEADERS + 1 - head.len().min(MOST_HEADERS)) as u64)
            .read_until(b'\n', &mut line)
            .ok()?;
        if read == 0 {
            return None;
        }
        head.extend_from_slice(&line);
        if head.len() > MOST_HEADERS {
            return None;
        }
        if line == b"\r\n" || line == b"\n" {
            break;
        }
    }
    let head = String::from_utf8(head).ok()?;
    let mut lines = head.lines();
    let mut first = lines.next()?.split_whitespace();
    let method = first.next()?.to_string();
    let path = first.next()?.to_string();
    let mut length = 0usize;
    let mut origin = false;
    let mut close = false;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "content-length" => length = value.parse().ok()?,
            "origin" => origin = true,
            "connection" => close = value.eq_ignore_ascii_case("close"),
            "transfer-encoding" => return None,
            _ => {}
        }
    }
    if length > MOST_BODY {
        return None;
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).ok()?;
    Some(Request {
        method,
        path,
        origin,
        body,
        close,
    })
}

fn respond(stream: &mut TcpStream, status: &str, body: Option<&Value>) -> std::io::Result<()> {
    let text = body.map(Value::to_string).unwrap_or_default();
    let kind = if body.is_some() {
        "Content-Type: application/json\r\n"
    } else {
        ""
    };
    write!(
        stream,
        "HTTP/1.1 {status}\r\n{kind}Content-Length: {}\r\nCache-Control: no-store\r\n\r\n{text}",
        text.len()
    )?;
    stream.flush()
}

impl Served {
    fn connection(&self, stream: TcpStream) {
        let _ = stream.set_read_timeout(Some(IDLE));
        let _ = stream.set_write_timeout(Some(WRITE_LIMIT));
        let Ok(mut writer) = stream.try_clone() else {
            return;
        };
        let mut reader = BufReader::new(stream);
        for _ in 0..MOST_REQUESTS {
            if self.stopped.load(Ordering::SeqCst) {
                return;
            }
            let Some(request) = read_request(&mut reader) else {
                return;
            };
            let sent = if request.path != self.path || request.origin {
                respond(&mut writer, "404 Not Found", None)
            } else if request.method != "POST" {
                respond(&mut writer, "405 Method Not Allowed", None)
            } else {
                match serde_json::from_slice::<Value>(&request.body) {
                    Ok(Value::Array(batch)) => {
                        if batch.len() > MOST_BATCH {
                            let _ = respond(&mut writer, "413 Content Too Large", None);
                            return;
                        }
                        let answers: Vec<Value> =
                            batch.iter().filter_map(|m| self.answer(m)).collect();
                        if answers.is_empty() {
                            respond(&mut writer, "202 Accepted", None)
                        } else {
                            respond(&mut writer, "200 OK", Some(&Value::Array(answers)))
                        }
                    }
                    Ok(message) => match self.answer(&message) {
                        Some(answer) => respond(&mut writer, "200 OK", Some(&answer)),
                        None => respond(&mut writer, "202 Accepted", None),
                    },
                    Err(_) => respond(
                        &mut writer,
                        "400 Bad Request",
                        Some(
                            &json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": "not JSON" } }),
                        ),
                    ),
                }
            };
            if sent.is_err() || request.close {
                return;
            }
        }
    }

    fn answer(&self, message: &Value) -> Option<Value> {
        let id = message.get("id").cloned()?;
        let method = message["method"].as_str().unwrap_or_default();
        let result = match method {
            "initialize" => Ok(json!({
                "protocolVersion": message["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18"),
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": SERVER, "version": env!("CARGO_PKG_VERSION") },
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({
                "tools": self.tools.iter().map(|t| json!({
                    "name": t.name,
                    "description": t.description,
                    "inputSchema": t.schema,
                })).collect::<Vec<_>>(),
            })),
            "tools/call" => {
                let name = message["params"]["name"].as_str().unwrap_or_default();
                if self.tools.iter().any(|t| t.name == name) {
                    let args = message["params"]
                        .get("arguments")
                        .cloned()
                        .filter(Value::is_object)
                        .unwrap_or_else(|| json!({}));
                    let text = self.call(name, args);
                    Ok(json!({ "content": [{ "type": "text", "text": text }], "isError": false }))
                } else {
                    Err((-32602, format!("leo has no tool named {name}")))
                }
            }
            _ => Err((-32601, format!("leo does not answer {method}"))),
        };
        Some(match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, text)) => {
                json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": text } })
            }
        })
    }

    fn call(&self, name: &str, args: Value) -> String {
        if self.stopped.load(Ordering::SeqCst) {
            return "That did not work: this answer has already finished.".into();
        }
        let (reply, answer) = mpsc::channel();
        let asked = self.to.try_send(Incoming::Call {
            name: name.to_string(),
            args,
            reply,
        });
        if asked.is_err() {
            return "That did not work: this answer has already finished.".into();
        }
        let until = Instant::now() + CALL_LIMIT;
        loop {
            if self.stopped.load(Ordering::SeqCst) {
                return "That did not work: this answer has already finished.".into();
            }
            let Some(remaining) = until.checked_duration_since(Instant::now()) else {
                return "That did not work: leo could not run the tool in time.".into();
            };
            match answer.recv_timeout(remaining.min(STOP_POLL)) {
                Ok(answer) => return answer,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return "That did not work: this answer has already finished.".into();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn post(url: &str, body: &Value, origin: bool) -> (String, String) {
        let rest = url.trim_start_matches("http://");
        let (host, path) = rest.split_at(rest.find('/').unwrap());
        let mut stream = TcpStream::connect(host).unwrap();
        let text = body.to_string();
        let extra = if origin {
            "Origin: http://evil.example\r\n"
        } else {
            ""
        };
        write!(
            stream,
            "POST {path} HTTP/1.1\r\nHost: {host}\r\n{extra}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
            text.len()
        )
        .unwrap();
        let mut answer = String::new();
        stream.read_to_string(&mut answer).unwrap();
        let (head, body) = answer.split_once("\r\n\r\n").unwrap();
        (head.lines().next().unwrap().to_string(), body.to_string())
    }

    fn bridge() -> (Bridge, mpsc::Receiver<Incoming>) {
        let (to, from) = mpsc::sync_channel(super::super::process::QUEUED_MESSAGES);
        let tools = vec![Tool {
            name: "search_notes".into(),
            description: "Search the notes".into(),
            schema: json!({ "type": "object", "properties": { "query": { "type": "string" } }, "required": ["query"] }),
        }];
        (Bridge::start(tools, to).unwrap(), from)
    }

    #[test]
    fn claude_code_lists_and_calls_leos_tools_through_the_bridge() {
        let (bridge, from) = bridge();
        let (status, body) = post(
            &bridge.url,
            &json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18" } }),
            false,
        );
        assert!(status.contains("200"), "{status}");
        let init: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
        let (status, _) = post(
            &bridge.url,
            &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
            false,
        );
        assert!(status.contains("202"), "{status}");
        let (_, body) = post(
            &bridge.url,
            &json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
            false,
        );
        let listed: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(listed["result"]["tools"][0]["name"], "search_notes");
        assert_eq!(
            listed["result"]["tools"][0]["inputSchema"]["required"][0],
            "query"
        );
        let worker = std::thread::spawn(move || match from.recv().unwrap() {
            Incoming::Call { name, args, reply } => {
                reply.send(format!("{name}: {}", args["query"])).unwrap();
            }
            Incoming::Line(_) | Incoming::Failed => panic!("expected a call"),
        });
        let url = bridge.url.clone();
        let (_, body) = post(
            &url,
            &json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "name": "search_notes", "arguments": { "query": "heaps" } } }),
            false,
        );
        worker.join().unwrap();
        let called: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            called["result"]["content"][0]["text"],
            "search_notes: \"heaps\""
        );
        let (_, body) = post(
            &url,
            &json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": { "name": "rm_rf", "arguments": {} } }),
            false,
        );
        assert!(body.contains("no tool named rm_rf"), "{body}");
    }

    #[test]
    fn only_the_secret_address_answers_and_never_a_browser() {
        let (bridge, _from) = bridge();
        let list = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });
        let wrong = bridge.url.rsplit_once('/').unwrap().0.to_string() + "/guess";
        assert!(post(&wrong, &list, false).0.contains("404"));
        assert!(post(&bridge.url, &list, true).0.contains("404"));
        assert!(post(&bridge.url, &list, false).0.contains("200"));
        assert!(bridge.config().contains(&bridge.url));
    }

    fn address(bridge: &Bridge) -> String {
        bridge
            .url
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap()
            .to_string()
    }

    fn wait_for(mut ready: impl FnMut() -> bool) {
        let until = Instant::now() + Duration::from_secs(2);
        while !ready() {
            assert!(
                Instant::now() < until,
                "the bridge did not release its connection workers"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn assert_closed(stream: &mut TcpStream) {
        stream
            .set_read_timeout(Some(Duration::from_millis(500)))
            .unwrap();
        let mut byte = [0];
        match stream.read(&mut byte) {
            Ok(0) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::NotConnected
                        | std::io::ErrorKind::BrokenPipe
                ) => {}
            result => panic!("the bridge left a connection open: {result:?}"),
        }
    }

    #[test]
    fn idle_connections_are_capped_and_drop_closes_every_worker() {
        let (bridge, _from) = bridge();
        let connections = Arc::clone(&bridge.connections);
        let address = address(&bridge);
        let mut streams: Vec<TcpStream> = (0..MOST_CONNECTIONS)
            .map(|_| TcpStream::connect(&address).unwrap())
            .collect();
        wait_for(|| connections.active.load(Ordering::SeqCst) == MOST_CONNECTIONS);
        let mut overflow = TcpStream::connect(&address).unwrap();
        assert_closed(&mut overflow);
        assert_eq!(connections.active.load(Ordering::SeqCst), MOST_CONNECTIONS);
        drop(bridge);
        for stream in &mut streams {
            assert_closed(stream);
        }
        wait_for(|| connections.active.load(Ordering::SeqCst) == 0);
    }

    #[test]
    fn dropping_a_bridge_cancels_a_tool_wait_without_waiting_for_its_reply() {
        let (bridge, from) = bridge();
        let connections = Arc::clone(&bridge.connections);
        let mut stream = TcpStream::connect(address(&bridge)).unwrap();
        let path = bridge
            .url
            .trim_start_matches("http://")
            .split_once('/')
            .unwrap()
            .1;
        let body = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "search_notes", "arguments": { "query": "fixture" } } }).to_string();
        write!(
            stream,
            "POST /{path} HTTP/1.1\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        let pending_reply = match from.recv_timeout(Duration::from_secs(2)).unwrap() {
            Incoming::Call { reply, .. } => reply,
            _ => panic!("expected a pending tool call"),
        };
        drop(bridge);
        assert_closed(&mut stream);
        wait_for(|| connections.active.load(Ordering::SeqCst) == 0);
        assert!(pending_reply.send("too late".into()).is_err());
    }

    #[test]
    fn excessive_batches_are_rejected_before_any_tool_runs() {
        let (bridge, from) = bridge();
        let call = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "search_notes", "arguments": {} } });
        let (status, _) = post(
            &bridge.url,
            &Value::Array(vec![call; MOST_BATCH + 1]),
            false,
        );
        assert!(status.contains("413"));
        assert!(from.try_recv().is_err());
    }
}
