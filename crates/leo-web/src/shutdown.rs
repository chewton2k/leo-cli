use std::time::Duration;

pub(crate) const GRACE: Duration = Duration::from_secs(5);
pub(crate) const CLEANUP: Duration = Duration::from_secs(1);
pub(crate) const LAST_BACKUP: Duration = Duration::from_secs(3);

pub(crate) async fn drain(grace: Duration, server: tokio::task::JoinHandle<std::io::Result<()>>) {
    let abort = server.abort_handle();
    if tokio::time::timeout(grace, server).await.is_err() {
        abort.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::IntoFuture;
    use std::sync::Arc;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn active_requests_and_streams_cannot_hold_shutdown_open() {
        for streaming in [false, true] {
            let started = Arc::new(tokio::sync::Notify::new());
            let handler = Arc::clone(&started);
            let app = axum::Router::new().route(
                "/",
                axum::routing::get(move || {
                    let started = Arc::clone(&handler);
                    async move {
                        started.notify_one();
                        if streaming {
                            axum::body::Body::from_stream(futures_util::stream::pending::<
                                Result<Vec<u8>, std::io::Error>,
                            >())
                        } else {
                            std::future::pending::<axum::body::Body>().await
                        }
                    }
                }),
            );
            let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
                .await
                .unwrap();
            let address = listener.local_addr().unwrap();
            let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
            let server = tokio::spawn(
                axum::serve(listener, app)
                    .with_graceful_shutdown(async {
                        let _ = stopped.await;
                    })
                    .into_future(),
            );
            let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
            client
                .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .await
                .unwrap();
            tokio::time::timeout(Duration::from_secs(1), started.notified())
                .await
                .unwrap();
            stop.send(()).unwrap();
            tokio::time::timeout(
                Duration::from_secs(1),
                drain(Duration::from_millis(50), server),
            )
            .await
            .unwrap();
        }
    }

    #[tokio::test]
    async fn a_finished_server_returns_at_once() {
        let server = tokio::spawn(async { Err(std::io::Error::other("fixture server failure")) });
        tokio::time::timeout(
            Duration::from_secs(1),
            drain(Duration::from_secs(60), server),
        )
        .await
        .unwrap();
    }
}
