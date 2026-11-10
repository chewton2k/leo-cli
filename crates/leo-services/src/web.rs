use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use anyhow::{bail, Context, Result};

const TIMEOUT: Duration = Duration::from_secs(10);
const AGENT: &str = "Mozilla/5.0 (compatible; leo notes app)";

pub fn allowed(url: &reqwest::Url) -> bool {
    if !matches!(url.scheme(), "http" | "https") {
        return false;
    }
    match url.host() {
        Some(url::Host::Domain(name)) => {
            let name = name.trim_end_matches('.').to_ascii_lowercase();
            !(name == "localhost"
                || name.ends_with(".localhost")
                || name.ends_with(".local")
                || name.ends_with(".internal")
                || name.ends_with(".lan")
                || !name.contains('.'))
        }
        Some(url::Host::Ipv4(ip)) => public(IpAddr::V4(ip)),
        Some(url::Host::Ipv6(ip)) => public(IpAddr::V6(ip)),
        None => false,
    }
}

fn public_v4(v4: Ipv4Addr) -> bool {
    let [a, b, _, _] = v4.octets();
    !(a == 0
        || v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_broadcast()
        || v4.is_multicast()
        || a == 100 && (64..128).contains(&b)
        || a == 192 && b == 0 && v4.octets()[2] == 0
        || a == 198 && (18..20).contains(&b)
        || a >= 240)
}

fn public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => public_v4(v4),
        IpAddr::V6(v6) => {
            let s = v6.segments();
            let wrapped = |hi: u16, lo: u16| {
                Ipv4Addr::new((hi >> 8) as u8, hi as u8, (lo >> 8) as u8, lo as u8)
            };
            if v6.is_loopback() || v6.is_unspecified() || v6.is_multicast() {
                return false;
            }
            if (s[0] & 0xfe00) == 0xfc00 || (s[0] & 0xffc0) == 0xfe80 || (s[0] & 0xffc0) == 0xfec0 {
                return false;
            }
            if let Some(v4) = v6.to_ipv4() {
                return public_v4(v4);
            }
            if s[0] == 0x0064 && s[1] == 0xff9b {
                return public_v4(wrapped(s[6], s[7]));
            }
            if s[0] == 0x2002 {
                return public_v4(wrapped(s[1], s[2]));
            }
            true
        }
    }
}

pub fn public_addresses(host: &str) -> Result<Vec<SocketAddr>> {
    let all: Vec<SocketAddr> = (host, 0)
        .to_socket_addrs()
        .with_context(|| format!("could not find {host}"))?
        .take(MOST_ADDRESSES + 1)
        .collect();
    if all.len() > MOST_ADDRESSES || all.is_empty() || !all.iter().all(|a| public(a.ip())) {
        bail!("{host} is not a public web address");
    }
    Ok(all)
}

const MOST_DNS: usize = 4;
const MOST_ADDRESSES: usize = 32;
static DNS_SLOTS: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();

async fn resolve_with(
    host: String,
    slots: Arc<tokio::sync::Semaphore>,
    timeout: Duration,
    lookup: impl FnOnce(&str) -> Result<Vec<SocketAddr>> + Send + 'static,
) -> Result<Vec<SocketAddr>> {
    tokio::time::timeout(timeout, async move {
        let permit = slots
            .acquire_owned()
            .await
            .context("Web address lookup is unavailable. Try again.")?;
        let (sender, reply) = tokio::sync::oneshot::channel();
        std::thread::Builder::new()
            .name("leo-public-dns".into())
            .spawn(move || {
                let _permit = permit;
                let _ = sender.send(lookup(&host));
            })
            .context("Web address lookup could not start. Try again.")?;
        reply
            .await
            .context("Web address lookup stopped. Try again.")?
    })
    .await
    .context("Web address lookup took too long. Try again.")?
}

pub async fn public_dns(host: String) -> Result<Vec<std::net::SocketAddr>> {
    let slots =
        Arc::clone(DNS_SLOTS.get_or_init(|| Arc::new(tokio::sync::Semaphore::new(MOST_DNS))));
    resolve_with(host, slots, TIMEOUT, public_addresses).await
}
pub fn public_https(address: &str) -> bool {
    reqwest::Url::parse(address).is_ok_and(|url| url.scheme() == "https" && allowed(&url))
}

struct PublicOnly;

impl reqwest::dns::Resolve for PublicOnly {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            let slots = Arc::clone(
                DNS_SLOTS.get_or_init(|| Arc::new(tokio::sync::Semaphore::new(MOST_DNS))),
            );
            resolve_with(host, slots, TIMEOUT, public_addresses)
                .await
                .map(|found| Box::new(found.into_iter()) as reqwest::dns::Addrs)
                .map_err(|e| e.into())
        })
    }
}

fn follows(url: &reqwest::Url, previous: usize, https_only: bool) -> bool {
    previous < 5 && allowed(url) && (!https_only || url.scheme() == "https")
}

fn redirect_policy(https_only: bool) -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(move |attempt| {
        if follows(attempt.url(), attempt.previous().len(), https_only) {
            attempt.follow()
        } else {
            attempt.stop()
        }
    })
}

fn protected_client(
    builder: reqwest::blocking::ClientBuilder,
    https_only: bool,
) -> Result<reqwest::blocking::Client> {
    Ok(builder
        .no_proxy()
        .https_only(https_only)
        .timeout(TIMEOUT)
        .user_agent(AGENT)
        .dns_resolver(Arc::new(PublicOnly))
        .redirect(redirect_policy(https_only))
        .build()?)
}

#[cfg(not(test))]
pub(crate) fn model_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .no_proxy()
        .https_only(true)
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .user_agent(AGENT)
        .dns_resolver(Arc::new(PublicOnly))
        .redirect(redirect_policy(true))
        .build()?)
}

fn client(https_only: bool) -> Result<reqwest::blocking::Client> {
    protected_client(reqwest::blocking::Client::builder(), https_only)
}

fn limited(response: reqwest::blocking::Response, most: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(
        &mut std::io::Read::take(response, most.saturating_add(1)),
        &mut bytes,
    )?;
    if bytes.len() as u64 > most {
        bail!("that address sent back more than leo will read");
    }
    Ok(bytes)
}

pub fn fetch_public(address: &str, most: u64) -> Result<Vec<u8>> {
    let url = reqwest::Url::parse(address).context("that is not a web address")?;
    if url.scheme() != "https" || !allowed(&url) {
        bail!("leo only opens public https addresses");
    }
    let response = client(true)?.get(url).send()?.error_for_status()?;
    limited(response, most)
}

pub struct Since {
    pub body: Option<Vec<u8>>,
    pub etag: Option<String>,
    pub modified: Option<String>,
}

pub fn fetch_public_since(
    address: &str,
    most: u64,
    etag: Option<&str>,
    modified: Option<&str>,
) -> Result<Since> {
    let url = reqwest::Url::parse(address).context("that is not a web address")?;
    if url.scheme() != "https" || !allowed(&url) {
        bail!("leo only opens public https addresses");
    }
    let mut request = client(true)?.get(url);
    if let Some(tag) = etag.filter(|t| t.len() < 512) {
        request = request.header(reqwest::header::IF_NONE_MATCH, tag);
    }
    if let Some(when) = modified.filter(|t| t.len() < 128) {
        request = request.header(reqwest::header::IF_MODIFIED_SINCE, when);
    }
    let response = request.send()?;
    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(Since {
            body: None,
            etag: etag.map(str::to_string),
            modified: modified.map(str::to_string),
        });
    }
    let response = response.error_for_status()?;
    let header = |name: reqwest::header::HeaderName| {
        response
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .filter(|v| v.len() < 512)
            .map(str::to_string)
    };
    let tag = header(reqwest::header::ETAG);
    let when = header(reqwest::header::LAST_MODIFIED);
    let bytes = limited(response, most)?;
    Ok(Since {
        body: Some(bytes),
        etag: tag,
        modified: when,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_public_web_pages_can_be_opened() {
        let ok = |u: &str| allowed(&reqwest::Url::parse(u).unwrap());
        assert!(ok("https://en.wikipedia.org/wiki/Heap"));
        assert!(ok("http://8.8.8.8/x"));
        for bad in [
            "http://localhost:31831/api/notes",
            "http://127.0.0.1/",
            "http://192.168.1.1/",
            "http://10.0.0.5/",
            "http://169.254.169.254/latest/meta-data",
            "http://[::1]/",
            "http://[::ffff:127.0.0.1]/",
            "http://0.1.2.3/",
            "http://[64:ff9b::7f00:1]/",
            "http://[2002:c0a8:101::]/",
            "http://[::127.0.0.1]/",
            "http://224.0.0.1/",
            "http://printer.local/",
            "http://intranet/",
            "file:///etc/passwd",
            "ftp://example.com/x",
        ] {
            assert!(!ok(bad), "{bad}");
        }
        assert!(
            public_addresses("localhost").is_err(),
            "a name that leads to this computer is refused"
        );
        assert!(public(IpAddr::V6("2606:4700::1111".parse().unwrap())));
        assert!(
            public(IpAddr::V6("2002:0808:0808::".parse().unwrap())),
            "6to4 of a public address is fine"
        );
    }
}

#[cfg(test)]
mod boundaries {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn secure_fetches_never_follow_http_or_private_redirects() {
        let https = reqwest::Url::parse("https://example.com/calendar").unwrap();
        let http = reqwest::Url::parse("http://example.com/calendar").unwrap();
        let private = reqwest::Url::parse("https://127.0.0.1/private").unwrap();
        assert!(follows(&https, 1, true));
        assert!(!follows(&http, 1, true));
        assert!(follows(&http, 1, false));
        assert!(!follows(&private, 1, false));
        for ip in ["fec0::1", "fed0::1", "feff::1"] {
            assert!(!allowed(
                &reqwest::Url::parse(&format!("https://[{ip}]/")).unwrap()
            ));
            assert!(!public(ip.parse().unwrap()));
        }
        assert!(!follows(&https, 5, true));
        assert!(protected_client(reqwest::blocking::Client::builder(), true)
            .unwrap()
            .get("http://127.0.0.1:9/")
            .send()
            .is_err());
    }

    #[test]
    fn the_public_fetch_client_ignores_proxies_and_bounds_response_bodies() {
        let target = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        proxy.set_nonblocking(true).unwrap();
        let address = target.local_addr().unwrap();
        target.set_nonblocking(true).unwrap();
        let server = std::thread::spawn(move || {
            let until = std::time::Instant::now() + Duration::from_secs(2);
            let mut stream = loop {
                match target.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && std::time::Instant::now() < until =>
                    {
                        std::thread::sleep(Duration::from_millis(5))
                    }
                    Err(error) => panic!("the fake target did not receive a request: {error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut bytes = [0; 2048];
            assert!(stream.read(&mut bytes).unwrap() > 0);
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nabcdef",
                )
                .unwrap();
        });
        let builder = reqwest::blocking::Client::builder()
            .proxy(reqwest::Proxy::all(format!("http://{}", proxy.local_addr().unwrap())).unwrap());
        let response = protected_client(builder, false)
            .unwrap()
            .get(format!("http://{address}/"))
            .send()
            .unwrap();
        assert!(limited(response, 4).is_err());
        assert_eq!(
            proxy.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
        server.join().unwrap();
    }

    #[test]
    fn slow_dns_has_a_deadline_and_keeps_its_slot_until_the_lookup_finishes() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let slots = Arc::new(tokio::sync::Semaphore::new(1));
        let (release, hold) = std::sync::mpsc::channel();
        let lookup_slots = Arc::clone(&slots);
        let failed = runtime.block_on(resolve_with(
            "fake.example".into(),
            lookup_slots,
            Duration::from_millis(25),
            move |_| {
                let _ = hold.recv_timeout(Duration::from_secs(2));
                Ok(vec!["8.8.8.8:0".parse().unwrap()])
            },
        ));
        let still_held = slots.available_permits() == 0;
        let other_slots = Arc::clone(&slots);
        let waiting = runtime.block_on(resolve_with(
            "fake.example".into(),
            other_slots,
            Duration::from_millis(25),
            |_| panic!("no second lookup may start while the only slot is held"),
        ));
        release.send(()).unwrap();
        runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(2), slots.acquire())
                .await
                .unwrap()
                .unwrap()
                .forget();
        });
        assert!(failed.is_err());
        assert!(waiting.is_err());
        assert!(still_held);
    }
}
