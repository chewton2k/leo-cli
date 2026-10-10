use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use std::sync::Arc;
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
            if (s[0] & 0xfe00) == 0xfc00 || (s[0] & 0xffc0) == 0xfe80 {
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
        .collect();
    if all.is_empty() || !all.iter().all(|a| public(a.ip())) {
        bail!("{host} is not a public web address");
    }
    Ok(all)
}

struct PublicOnly;

impl reqwest::dns::Resolve for PublicOnly {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            public_addresses(&host)
                .map(|found| Box::new(found.into_iter()) as reqwest::dns::Addrs)
                .map_err(|e| e.into())
        })
    }
}

fn client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .timeout(TIMEOUT)
        .user_agent(AGENT)
        .dns_resolver(Arc::new(PublicOnly))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 || !allowed(attempt.url()) {
                attempt.stop()
            } else {
                attempt.follow()
            }
        }))
        .build()?)
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
    let html = client()?.get(url).send()?.error_for_status()?.text()?;
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
    let json: serde_json::Value = client()?.get(url).send()?.error_for_status()?.json()?;
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

pub fn readable(html: &str) -> String {
    let mut text = html.to_string();
    for tag in [
        "script", "style", "noscript", "svg", "head", "nav", "footer",
    ] {
        loop {
            let lower = text.to_ascii_lowercase();
            let open = format!("<{tag}");
            let Some(start) = lower.match_indices(&open).map(|(i, _)| i).find(|&i| {
                lower[i + open.len()..]
                    .chars()
                    .next()
                    .is_some_and(|c| c == '>' || c.is_whitespace() || c == '/')
            }) else {
                break;
            };
            let end = lower[start..]
                .find(&format!("</{tag}>"))
                .map_or(text.len(), |e| start + e + tag.len() + 3);
            text.replace_range(start..end, " ");
        }
    }
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
    let response = client()?.get(url).send()?.error_for_status()?;
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut std::io::Read::take(response, most + 1), &mut bytes)?;
    if bytes.len() as u64 > most {
        bail!("that address sent back more than leo will read");
    }
    Ok(bytes)
}

pub fn page(address: &str) -> Result<String> {
    let url = reqwest::Url::parse(address).context("that is not a web address")?;
    if !allowed(&url) {
        bail!("leo only opens public web pages");
    }
    let response = client()?.get(url).send()?.error_for_status()?;
    let kind = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !(kind.starts_with("text/html") || kind.starts_with("text/plain") || kind.is_empty()) {
        bail!("that page is not text ({kind})");
    }
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(
        &mut std::io::Read::take(response, MOST_PAGE_BYTES),
        &mut bytes,
    )?;
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
