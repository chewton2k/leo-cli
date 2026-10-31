use anyhow::{bail, Context, Result};

fn port_from(args: &[String]) -> Result<u16> {
    match args {
        [flag, port] if flag == "--port" => port.parse().context("--port needs a number"),
        _ => bail!("usage: leo-test-server --port <port>"),
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let port = port_from(&args)?;
    tokio::runtime::Runtime::new()?.block_on(async {
        let running = leo_web::start(
            leo_web::StartOptions {
                port,
                token: leo_web::saved_token()?,
            },
            leo_host::powers(),
        )
        .await?;
        tokio::signal::ctrl_c().await?;
        running.stop().await;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_port_comes_from_its_flag() {
        assert_eq!(
            port_from(&["--port".into(), "31831".into()]).unwrap(),
            31831
        );
        assert!(port_from(&[]).is_err());
        assert!(port_from(&["--port".into(), "x".into()]).is_err());
    }
}
