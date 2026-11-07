use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const START_LIMIT: Duration = Duration::from_secs(15);
const REQUEST_LIMIT: Duration = Duration::from_secs(30);

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn engine(home: &Path) -> (Server, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut command = Command::new(env!("CARGO_BIN_EXE_leo-test-server"));
    command
        .args(["--port", &port.to_string()])
        .env("LEO_HOME", home)
        .env("LEO_INSTALL_NO_MODEL", "1")
        .env("LEO_NO_MICROPHONE", "1")
        .env("LEO_NO_UPDATE_CHECK", "1")
        .env("LEO_NO_OPEN", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut server = Server(command.spawn().unwrap());
    let deadline = Instant::now() + START_LIMIT;
    while TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).is_err() {
        assert!(server.0.try_wait().unwrap().is_none(), "engine exited");
        assert!(Instant::now() < deadline, "engine did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    (server, port)
}

#[test]
fn the_browser_test_engine_starts_with_its_session_gate() {
    let home = tempfile::tempdir().unwrap();
    let (_server, port) = engine(home.path());
    let mut stream = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).unwrap();
    stream.set_read_timeout(Some(REQUEST_LIMIT)).unwrap();
    stream.set_write_timeout(Some(REQUEST_LIMIT)).unwrap();
    write!(
        stream,
        "GET /api/notes HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.take(16_384).read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 401"), "session gate missing");
}
