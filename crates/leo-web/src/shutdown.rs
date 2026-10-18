use std::future::{Future, IntoFuture};
use std::time::Duration;

const GRACE: Duration = Duration::from_secs(5);
pub(crate) const CLEANUP: Duration = Duration::from_secs(1);

pub(crate) async fn wait<S, F, Q>(
    server: S,
    first: F,
    second: Q,
    stop: impl FnOnce(),
) -> std::io::Result<()>
where
    S: IntoFuture<Output = std::io::Result<()>>,
    F: Future,
    Q: Future,
{
    wait_for(GRACE, server, first, second, stop).await
}

async fn wait_for<S, F, Q>(
    grace: Duration,
    server: S,
    first: F,
    second: Q,
    stop: impl FnOnce(),
) -> std::io::Result<()>
where
    S: IntoFuture<Output = std::io::Result<()>>,
    F: Future,
    Q: Future,
{
    let server = server.into_future();
    tokio::pin!(server, first, second);
    tokio::select! {
        result = &mut server => return result,
        _ = &mut first => {},
    }
    stop();
    tokio::select! {
        result = &mut server => result,
        _ = &mut second => Ok(()),
        _ = tokio::time::sleep(grace) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            let (first, interrupted) = tokio::sync::oneshot::channel::<()>();
            let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
            let server = axum::serve(listener, app).with_graceful_shutdown(async {
                let _ = stopped.await;
            });
            let serving = tokio::spawn(wait_for(
                Duration::from_millis(50),
                server,
                interrupted,
                std::future::pending::<()>(),
                move || {
                    let _ = stop.send(());
                },
            ));
            let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
            client
                .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .await
                .unwrap();
            tokio::time::timeout(Duration::from_secs(1), started.notified())
                .await
                .unwrap();
            first.send(()).unwrap();
            tokio::time::timeout(Duration::from_secs(1), serving)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
        }
    }

    #[tokio::test]
    async fn a_second_interrupt_skips_the_grace_period() {
        let (first, interrupted) = tokio::sync::oneshot::channel::<()>();
        let (second, forced) = tokio::sync::oneshot::channel::<()>();
        let (began, stopping) = tokio::sync::oneshot::channel::<()>();
        let serving = tokio::spawn(wait_for(
            Duration::from_secs(60),
            std::future::pending::<std::io::Result<()>>(),
            interrupted,
            forced,
            move || {
                let _ = began.send(());
            },
        ));
        first.send(()).unwrap();
        stopping.await.unwrap();
        second.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(1), serving)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn server_failures_return_without_waiting_for_a_signal() {
        let result = wait_for(
            Duration::from_secs(60),
            async { Err(std::io::Error::other("fixture server failure")) },
            std::future::pending::<()>(),
            std::future::pending::<()>(),
            || panic!("shutdown did not start"),
        )
        .await;
        assert_eq!(result.unwrap_err().to_string(), "fixture server failure");
    }
}
