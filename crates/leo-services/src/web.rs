use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use anyhow::{bail, Context, Result};

use crate::config::provider::ProviderKind;
use crate::config::Config;
use crate::import::entities;

const TIMEOUT: Duration = Duration::from_secs(10);
const MOST_PAGE_BYTES: u64 = 2 * 1024 * 1024;
const MOST_HITS: usize = 8;
const AGENT: &str = "Mozilla/5.0 (compatible; leo notes app)";

#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

pub fn has_own_web(config: &Config) -> bool {
    has_own_web_with(
        config,
        &|bin| crate::ai::provider::agent_cli::locate(bin).is_some(),
        &crate::health::port_open,
    )
}

pub fn has_own_web_with(
    config: &Config,
    installed: &dyn Fn(&str) -> bool,
    running: &dyn Fn(&str) -> bool,
) -> bool {
    for name in &config.chat.chain {
        let Some(provider) = config
            .provider(name)
            .cloned()
            .or_else(|| Config::built_in_provider(name))
        else {
            continue;
        };
        match provider.kind {
            Some(ProviderKind::Codex | ProviderKind::ClaudeCode) => {
                let bin = provider
                    .bin
                    .clone()
                    .filter(|b| !b.trim().is_empty())
                    .unwrap_or_else(|| {
                        if provider.kind == Some(ProviderKind::Codex) {
                            "codex".into()
                        } else {
                            "claude".into()
                        }
                    });
                if installed(&bin) {
                    return true;
                }
            }
            _ => {
                let local = provider.base_url.as_deref().is_some_and(|u| {
                    let u = u.to_ascii_lowercase();
                    u.contains("localhost") || u.contains("127.0.0.1") || u.contains("[::1]")
                });
                if local && !running(provider.base_url.as_deref().unwrap_or_default()) {
                    continue;
                }
                return false;
            }
        }
    }
    false
}

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
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if !follows(attempt.url(), attempt.previous().len(), https_only) {
                attempt.stop()
            } else {
                attempt.follow()
            }
        }))
        .build()?)
}

#[cfg(not(test))]
pub(crate) fn model_client(timeout: Duration) -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .no_proxy()
        .https_only(true)
        .connect_timeout(Duration::from_secs(15))
        .timeout(timeout)
        .user_agent(AGENT)
        .dns_resolver(Arc::new(PublicOnly))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if follows(attempt.url(), attempt.previous().len(), true) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
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

fn text_of(html: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for c in html.chars() {
        match c {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => out.push(c),
            _ => {}
        }
    }
    entities(&out)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let at = tag.find(&format!("{name}=\""))? + name.len() + 2;
    let rest = &tag[at..];
    Some(entities(&rest[..rest.find('"')?]))
}

fn unwrap_link(href: &str) -> Option<String> {
    let full = if href.starts_with("//") {
        format!("https:{href}")
    } else {
        href.to_string()
    };
    let url = reqwest::Url::parse(&full).ok()?;
    if url
        .host_str()
        .is_some_and(|h| h.ends_with("duckduckgo.com"))
    {
        if url.path() == "/y.js" {
            return None;
        }
        let target = url.query_pairs().find(|(k, _)| k == "uddg")?.1.to_string();
        return Some(target);
    }
    Some(full)
}

pub fn parse_duckduckgo(html: &str) -> Vec<Hit> {
    let mut hits = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find("class=\"result__a\"") {
        let tag_start = rest[..at].rfind('<').unwrap_or(at);
        let after = &rest[at..];
        let Some(tag_end) = after.find('>') else {
            break;
        };
        let tag = &rest[tag_start..at + tag_end];
        let body = &after[tag_end + 1..];
        let title = text_of(&body[..body.find("</a>").unwrap_or(0)]);
        let next = rest[at + 1..]
            .find("class=\"result__a\"")
            .map_or(rest.len(), |n| at + 1 + n);
        let block = &rest[at..next];
        let snippet = block
            .find("class=\"result__snippet\"")
            .and_then(|s| {
                let open = block[s..].find('>')? + s + 1;
                let close = block[open..]
                    .find("</a>")
                    .or_else(|| block[open..].find("</td>"))
                    .or_else(|| block[open..].find("</div>"))?;
                Some(text_of(&block[open..open + close]))
            })
            .unwrap_or_default();
        if let Some(url) = attribute(tag, "href").and_then(|h| unwrap_link(&h)) {
            if !title.is_empty() && !hits.iter().any(|h: &Hit| h.url == url) {
                hits.push(Hit {
                    title,
                    url,
                    snippet,
                });
            }
        }
        rest = &rest[next..];
        if hits.len() >= MOST_HITS {
            break;
        }
    }
    hits
}

pub fn parse_wikipedia(json: &serde_json::Value) -> Vec<Hit> {
    json["query"]["search"]
        .as_array()
        .map(|results| {
            results
                .iter()
                .filter_map(|r| {
                    let title = r["title"].as_str()?.to_string();
                    let mut url = reqwest::Url::parse("https://en.wikipedia.org/wiki/").ok()?;
                    url.path_segments_mut()
                        .ok()?
                        .pop()
                        .push(&title.replace(' ', "_"));
                    Some(Hit {
                        url: url.to_string(),
                        snippet: text_of(r["snippet"].as_str().unwrap_or("")),
                        title,
                    })
                })
                .take(MOST_HITS)
                .collect()
        })
        .unwrap_or_default()
}

fn duckduckgo(query: &str) -> Result<Vec<Hit>> {
    let mut url = reqwest::Url::parse("https://html.duckduckgo.com/html/")?;
    url.query_pairs_mut().append_pair("q", query);
    let html = String::from_utf8_lossy(&limited(
        client(false)?.get(url).send()?.error_for_status()?,
        MOST_PAGE_BYTES,
    )?)
    .into_owned();
    Ok(parse_duckduckgo(&html))
}

fn wikipedia(query: &str) -> Result<Vec<Hit>> {
    let mut url = reqwest::Url::parse("https://en.wikipedia.org/w/api.php")?;
    url.query_pairs_mut()
        .append_pair("action", "query")
        .append_pair("list", "search")
        .append_pair("format", "json")
        .append_pair("srlimit", "6")
        .append_pair("srsearch", query);
    let json: serde_json::Value = serde_json::from_slice(&limited(
        client(false)?.get(url).send()?.error_for_status()?,
        MOST_PAGE_BYTES,
    )?)?;
    Ok(parse_wikipedia(&json))
}

pub fn search(query: &str) -> Result<Vec<Hit>> {
    let query = query.trim();
    if query.is_empty() {
        bail!("the search is empty");
    }
    match duckduckgo(query) {
        Ok(hits) if !hits.is_empty() => Ok(hits),
        first => match wikipedia(query) {
            Ok(hits) => Ok(hits),
            Err(e) => match first {
                Err(d) => Err(d).context(format!("and Wikipedia did not answer either: {e}")),
                Ok(_) => Err(e),
            },
        },
    }
}

fn strip_ignored_blocks(html: &str) -> String {
    const IGNORED: [&str; 7] = [
        "script", "style", "noscript", "svg", "head", "nav", "footer",
    ];
    let mut out = String::with_capacity(html.len());
    let mut pos = 0;
    while let Some(rel) = html[pos..].find('<') {
        let start = pos + rel;
        out.push_str(&html[pos..start]);
        let rest = &html[start + 1..];
        let matched = IGNORED.into_iter().find(|&tag| {
            rest.get(..tag.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(tag))
                && rest[tag.len()..]
                    .chars()
                    .next()
                    .is_some_and(|c| c == '>' || c.is_whitespace() || c == '/')
        });
        if let Some(tag) = matched {
            out.push(' ');
            let mut search = start + 1 + tag.len();
            let close_len = tag.len() + 3;
            pos = loop {
                let Some(close_rel) = html[search..].find("</") else {
                    break html.len();
                };
                let close_start = search + close_rel;
                let after_slash = &html[close_start + 2..];
                if after_slash
                    .get(..tag.len())
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(tag))
                    && after_slash.as_bytes().get(tag.len()) == Some(&b'>')
                {
                    break close_start + close_len;
                }
                search = close_start + 2;
            };
        } else {
            out.push('<');
            pos = start + 1;
        }
    }
    out.push_str(&html[pos..]);
    out
}

pub fn readable(html: &str) -> String {
    let text = strip_ignored_blocks(html);
    let mut lines = Vec::new();
    let mut piece = String::new();
    let mut inside = false;
    let mut tag = String::new();
    for c in text.chars() {
        match c {
            '<' => {
                inside = true;
                tag.clear();
            }
            '>' if inside => {
                inside = false;
                let name = tag
                    .trim_start_matches('/')
                    .split(|c: char| c.is_whitespace() || c == '/')
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if matches!(
                    name.as_str(),
                    "p" | "br"
                        | "li"
                        | "div"
                        | "tr"
                        | "h1"
                        | "h2"
                        | "h3"
                        | "h4"
                        | "h5"
                        | "h6"
                        | "section"
                        | "article"
                ) {
                    lines.push(std::mem::take(&mut piece));
                }
            }
            _ if inside => tag.push(c),
            _ => piece.push(c),
        }
    }
    lines.push(piece);
    lines
        .iter()
        .map(|l| entities(l).split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
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

pub fn page(address: &str) -> Result<String> {
    let url = reqwest::Url::parse(address).context("that is not a web address")?;
    if !allowed(&url) {
        bail!("leo only opens public web pages");
    }
    let response = client(false)?.get(url).send()?.error_for_status()?;
    let kind = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !(kind.starts_with("text/html") || kind.starts_with("text/plain") || kind.is_empty()) {
        bail!("that page is not text ({kind})");
    }
    let bytes = limited(response, MOST_PAGE_BYTES)?;
    let body = String::from_utf8_lossy(&bytes);
    Ok(if kind.starts_with("text/plain") {
        body.into_owned()
    } else {
        readable(&body)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RESULTS: &str = r#"<div class="result results_links results_links_deep result--ad">
<a rel="nofollow" class="result__a" href="https://duckduckgo.com/y.js?ad_domain=x&amp;u3=1">Buy algorithms now</a>
<a class="result__snippet" href="x">An ad.</a></div>
<div class="result results_links results_links_deep web-result">
<h2 class="result__title"><a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fen.wikipedia.org%2Fwiki%2FEdsger_W._Dijkstra&amp;rut=abc">Edsger W. <b>Dijkstra</b> - Wikipedia</a></h2>
<a class="result__snippet" href="//duckduckgo.com/l/?uddg=x">He received the <b>Turing Award</b> in 1972 &amp; more.</a></div>
<div class="result"><h2><a rel="nofollow" class="result__a" href="https://amturing.acm.org/award_winners/dijkstra_1053701.cfm">A.M. Turing Award</a></h2>
<a class="result__snippet">ACM page.</a></div>"#;

    #[test]
    fn search_results_are_read_with_ads_left_out_and_links_unwrapped() {
        let hits = parse_duckduckgo(RESULTS);
        assert_eq!(hits.len(), 2, "{hits:?}");
        assert_eq!(hits[0].title, "Edsger W. Dijkstra - Wikipedia");
        assert_eq!(
            hits[0].url,
            "https://en.wikipedia.org/wiki/Edsger_W._Dijkstra"
        );
        assert_eq!(
            hits[0].snippet,
            "He received the Turing Award in 1972 & more."
        );
        assert_eq!(
            hits[1].url,
            "https://amturing.acm.org/award_winners/dijkstra_1053701.cfm"
        );
        assert!(parse_duckduckgo("<html>blocked</html>").is_empty());
        let wiki = serde_json::json!({"query": {"search": [{"title": "Dijkstra's algorithm", "snippet": "an <span class=\"searchmatch\">algorithm</span> for"}]}});
        let hits = parse_wikipedia(&wiki);
        assert_eq!(
            hits[0].url,
            "https://en.wikipedia.org/wiki/Dijkstra's_algorithm"
        );
        assert_eq!(hits[0].snippet, "an algorithm for");
    }

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
        assert!(page("http://127.0.0.1:9/").is_err());
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

    #[test]
    fn a_page_is_read_as_its_text() {
        let html = "<html><head><title>T</title><style>p{}</style></head><body><nav>Menu</nav><h1>Heaps</h1><p>A heap &amp; a <b>tree</b>.</p><script>alert(1)</script><ul><li>one</li><li>two</li></ul></body></html>";
        assert_eq!(readable(html), "Heaps\nA heap & a tree.\none\ntwo");
        let page = "<html><head><title>T</title></head><body><header class=\"top\">Site</header><main><p>Kept text.</p></main><navigation>also kept</navigation></body></html>";
        assert_eq!(readable(page), "Site\nKept text.\nalso kept");
    }

    #[test]
    fn the_ai_that_will_answer_decides_whether_leo_offers_web_tools() {
        let mut config: Config = toml::from_str("").unwrap();
        let own = |config: &Config, installed: bool, running: bool| {
            has_own_web_with(config, &|_| installed, &|_| running)
        };
        config.chat.chain = vec!["codex".into()];
        assert!(own(&config, true, false), "Codex searches the web itself");
        assert!(
            !own(&config, false, false),
            "a Codex that is not installed cannot answer"
        );
        config.chat.chain = vec!["claude_code".into(), "openai".into()];
        assert!(own(&config, true, false));
        assert!(
            !own(&config, false, false),
            "then OpenAI answers, and it has no web search"
        );
        config.chat.chain = vec!["ollama".into(), "codex".into()];
        assert!(
            own(&config, true, false),
            "Ollama is not running, so Codex answers"
        );
        assert!(
            !own(&config, true, true),
            "Ollama is running and answers first"
        );
        config.chat.chain = vec!["anthropic".into()];
        assert!(!own(&config, true, true));
        config.chat.chain = vec![];
        assert!(!own(&config, true, true));
    }

    #[test]
    #[ignore]
    fn the_real_web_answers() {
        let hits = search("Edsger Dijkstra Turing Award year").unwrap();
        assert!(!hits.is_empty());
        println!("{hits:#?}");
        let local = page("http://localtest.me/").unwrap_err();
        assert!(
            format!("{local:#}").contains("not a public web address"),
            "{local:#}"
        );
        let readable_hit = hits
            .iter()
            .find(|h| h.url.contains("wikipedia.org"))
            .unwrap_or(&hits[0]);
        let text = page(&readable_hit.url).unwrap();
        assert!(text.len() > 200);
        println!("{}", text.chars().take(600).collect::<String>());
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

    #[test]
    fn readable_strips_many_ignored_blocks_in_linear_time() {
        let mut html = String::with_capacity(200_000);
        html.push_str("<p>Keep Café</p>");
        for _ in 0..2_000 {
            html.push_str("<SCRIPT type=\"text/javascript\">ignored();</SCRIPT><svg><path/></svg>");
        }
        html.push_str("<p>Done</p>");
        let started = std::time::Instant::now();
        let text = readable(&html);
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(text.contains("Keep Café"));
        assert!(text.contains("Done"));
        assert!(!text.contains("ignored"));
    }
}
