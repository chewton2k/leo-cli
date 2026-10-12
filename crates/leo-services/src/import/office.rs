use std::collections::HashMap;
use std::io::{Cursor, Read};

use anyhow::{bail, Context, Result};
use roxmltree::{Document, Node, ParsingOptions};

const MOST_FILES: usize = 4_096;
const MOST_PART_BYTES: usize = 16 * 1024 * 1024;
const MOST_READ_BYTES: usize = 64 * 1024 * 1024;
const MOST_NODES: u32 = 300_000;
const MOST_DEPTH: usize = 32;
const MOST_COLUMNS: usize = 256;
const MOST_LEVEL: usize = 8;
const MOST_STYLE_HOPS: usize = 16;
const MOST_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
const MOST_TABLE_CELLS: usize = 300_000;

struct Package<'a> {
    zip: zip::ZipArchive<Cursor<&'a [u8]>>,
    read: usize,
}

impl<'a> Package<'a> {
    fn open(bytes: &'a [u8]) -> Result<Self> {
        let zip = zip::ZipArchive::new(Cursor::new(bytes)).context(
            "this Office file could not be opened; export it again and upload the new file",
        )?;
        if zip.len() > MOST_FILES {
            bail!("this Office file has too many parts; split it into smaller documents");
        }
        Ok(Self { zip, read: 0 })
    }

    fn text(&mut self, name: &str) -> Result<Option<String>> {
        let mut file = match self.zip.by_name(name) {
            Ok(file) => file,
            Err(zip::result::ZipError::FileNotFound) => return Ok(None),
            Err(_) => bail!("a part of this Office file could not be read; export it again"),
        };
        if file.size() > MOST_PART_BYTES as u64 || self.read >= MOST_READ_BYTES {
            bail!("this Office file expands to too much text; split it into smaller documents");
        }
        let limit = MOST_PART_BYTES.min(MOST_READ_BYTES - self.read);
        let mut bytes = Vec::new();
        (&mut file)
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .context("a part of this Office file is damaged; export it again")?;
        if bytes.len() > limit {
            bail!("this Office file expands to too much text; split it into smaller documents");
        }
        self.read += bytes.len();
        String::from_utf8(bytes)
            .map(Some)
            .context("this Office file has unreadable text; export it again as a new Office file")
    }

    fn required(&mut self, name: &str) -> Result<String> {
        self.text(name)?
            .context("this Office file is missing a required part; export it again")
    }

    fn relationships(&mut self, name: &str) -> Result<HashMap<String, Link>> {
        let parent = name.rsplit_once('/').map_or("", |(p, _)| p);
        let file = name.rsplit('/').next().unwrap_or(name);
        let rels = format!("{parent}/_rels/{file}.rels");
        let Some(xml) = self.text(&rels)? else {
            return Ok(HashMap::new());
        };
        let doc = parse(&xml)?;
        let mut links = HashMap::new();
        for node in doc.descendants().filter(|n| is(*n, "Relationship")) {
            let (Some(id), Some(kind), Some(target)) =
                (attr(node, "Id"), attr(node, "Type"), attr(node, "Target"))
            else {
                continue;
            };
            let external = attr(node, "TargetMode") == Some("External");
            let target = if external {
                target.to_string()
            } else {
                joined(parent, target)?
            };
            links.insert(
                id.to_string(),
                Link {
                    kind: kind.rsplit('/').next().unwrap_or(kind).to_string(),
                    target,
                    external,
                },
            );
        }
        Ok(links)
    }
}

struct Link {
    kind: String,
    target: String,
    external: bool,
}

fn joined(parent: &str, target: &str) -> Result<String> {
    if target.contains(['\\', ':', '\0', '?', '#']) {
        bail!("this Office file has an invalid internal link; export it again");
    }
    let mut parts: Vec<&str> = if target.starts_with('/') {
        Vec::new()
    } else {
        parent.split('/').filter(|p| !p.is_empty()).collect()
    };
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    bail!("this Office file has a link outside the document; export it again");
                }
            }
            part => parts.push(part),
        }
    }
    if !matches!(parts.first(), Some(&"word" | &"ppt")) {
        bail!("this Office file has a link outside its document parts; export it again");
    }
    Ok(parts.join("/"))
}

fn parse(xml: &str) -> Result<Document<'_>> {
    if xml.len() > MOST_PART_BYTES {
        bail!("this Office file expands to too much text; split it into smaller documents");
    }
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut depth = 0usize;
    let mut events = 0u32;
    loop {
        events += 1;
        if events > MOST_NODES.saturating_mul(3) {
            bail!("this Office file has too many XML elements; split it into smaller documents");
        }
        match reader.read_event() {
            Ok(quick_xml::events::Event::Start(_)) => {
                depth += 1;
                if depth >= MOST_DEPTH {
                    bail!("this Office file is nested too deeply; export a simpler document");
                }
            }
            Ok(quick_xml::events::Event::End(_)) => depth = depth.saturating_sub(1),
            Ok(quick_xml::events::Event::DocType(_)) => {
                bail!("this Office file has unsupported XML entities; export it again")
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(_) => bail!("this Office file has damaged XML; export it again"),
            _ => {}
        }
    }
    let doc = Document::parse_with_options(
        xml,
        ParsingOptions {
            nodes_limit: MOST_NODES,
            ..ParsingOptions::default()
        },
    )
    .map_err(|_| {
        anyhow::anyhow!("this Office file has damaged or unsupported XML; export it again")
    })?;
    if doc
        .descendants()
        .any(|n| n.ancestors().take(MOST_DEPTH + 1).count() > MOST_DEPTH)
    {
        bail!("this Office file is nested too deeply; export a simpler document");
    }
    Ok(doc)
}

fn is(node: Node<'_, '_>, name: &str) -> bool {
    node.is_element() && node.tag_name().name() == name
}

fn attr<'a>(node: Node<'a, '_>, name: &str) -> Option<&'a str> {
    if name == "id" {
        if let Some(attribute) = node.attributes().find(|a| {
            a.name() == name
                && a.namespace()
                    .is_some_and(|ns| ns.ends_with("/relationships"))
        }) {
            return Some(attribute.value());
        }
    }
    node.attributes()
        .find(|a| a.name() == name)
        .map(|a| a.value())
}

fn child<'a, 'input>(node: Node<'a, 'input>, name: &str) -> Option<Node<'a, 'input>> {
    node.children().find(|n| is(*n, name))
}

fn value<'a>(node: Node<'a, '_>, name: &str) -> Option<&'a str> {
    child(node, name).and_then(|n| attr(n, "val"))
}

fn escape(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if matches!(c, '\\' | '*' | '_' | '[' | ']' | '<' | '>' | '`') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn destination(target: &str) -> Option<String> {
    if target.starts_with('#') && !target.chars().any(char::is_control) {
        return Some(target.replace(['(', ')', ' ', '<', '>'], ""));
    }
    let url = url::Url::parse(target).ok()?;
    if !matches!(url.scheme(), "https" | "http" | "mailto") {
        return None;
    }
    Some(url.as_str().replace('(', "%28").replace(')', "%29"))
}

fn linked(text: String, target: Option<&str>) -> String {
    match target.and_then(destination) {
        Some(target) if !text.trim().is_empty() => format!("[{text}]({target})"),
        _ => text,
    }
}

fn enabled(node: Node<'_, '_>, name: &str) -> bool {
    child(node, name).is_some_and(|n| !matches!(attr(n, "val"), Some("0" | "false" | "off")))
}

fn math(node: Node<'_, '_>, depth: usize) -> String {
    if depth >= MOST_DEPTH {
        return String::new();
    }
    let children = || {
        node.children()
            .filter(Node::is_element)
            .map(|n| math(n, depth + 1))
            .collect::<String>()
    };
    let part = |name| {
        child(node, name)
            .map(|n| math(n, depth + 1))
            .unwrap_or_default()
    };
    match node.tag_name().name() {
        "t" => node.text().unwrap_or_default().to_string(),
        "f" => format!("\\frac{{{}}}{{{}}}", part("num"), part("den")),
        "sSup" => format!("{{{}}}^{{{}}}", part("e"), part("sup")),
        "sSub" => format!("{{{}}}_{{{}}}", part("e"), part("sub")),
        "sSubSup" => format!("{{{}}}_{{{}}}^{{{}}}", part("e"), part("sub"), part("sup")),
        "rad" => {
            let degree = part("deg");
            if degree.is_empty() {
                format!("\\sqrt{{{}}}", part("e"))
            } else {
                format!("\\sqrt[{degree}]{{{}}}", part("e"))
            }
        }
        name if name.ends_with("Pr") => String::new(),
        _ => children(),
    }
}

fn inline(node: Node<'_, '_>, links: &HashMap<String, Link>, depth: usize) -> String {
    if depth >= MOST_DEPTH {
        return String::new();
    }
    let content = || {
        node.children()
            .filter(Node::is_element)
            .map(|n| inline(n, links, depth + 1))
            .collect::<String>()
    };
    match node.tag_name().name() {
        "t" => escape(node.text().unwrap_or_default()),
        "tab" => "\t".to_string(),
        "br" | "cr" => "\n".to_string(),
        "del" | "delText" | "instrText" | "drawing" | "pict" => String::new(),
        "oMath" => format!("${}$", math(node, 0)),
        "r" => {
            let text = content();
            let Some(props) = child(node, "rPr") else {
                return text;
            };
            let bold = enabled(props, "b") || attr(props, "b") == Some("1");
            let italic = enabled(props, "i") || attr(props, "i") == Some("1");
            let text = if text.trim().is_empty() {
                text
            } else {
                let leading: String = text.chars().take_while(|c| c.is_whitespace()).collect();
                let trailing: String = text
                    .chars()
                    .rev()
                    .take_while(|c| c.is_whitespace())
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect();
                let mark = match (bold, italic) {
                    (true, true) => "***",
                    (true, false) => "**",
                    (false, true) => "*",
                    _ => "",
                };
                format!("{leading}{mark}{}{mark}{trailing}", text.trim())
            };
            let target = child(props, "hlinkClick")
                .and_then(|n| attr(n, "id"))
                .and_then(|id| links.get(id))
                .filter(|l| l.kind == "hyperlink")
                .map(|l| l.target.as_str());
            linked(text, target)
        }
        "hyperlink" => {
            let anchor = attr(node, "anchor").map(|a| format!("#{a}"));
            let target = attr(node, "id")
                .and_then(|id| links.get(id))
                .filter(|l| l.kind == "hyperlink")
                .map(|l| l.target.as_str())
                .or(anchor.as_deref());
            linked(content(), target)
        }
        name if name.ends_with("Pr") => String::new(),
        _ => content(),
    }
}

fn table(rows: Vec<Vec<String>>) -> Result<String> {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    if columns == 0 {
        return Ok(String::new());
    }
    if columns > MOST_COLUMNS || columns.saturating_mul(rows.len()) > MOST_TABLE_CELLS {
        bail!("this document has a table with too many columns; split the table before uploading");
    }
    let mut out = String::new();
    for (i, row) in rows.iter().enumerate() {
        out.push('|');
        for col in 0..columns {
            let cell = row.get(col).map(String::as_str).unwrap_or_default();
            let cell = cell
                .trim()
                .replace('|', "\\|")
                .replace(['\n', '\r'], "<br>");
            out.push_str(&format!(" {cell} |"));
        }
        out.push('\n');
        if i == 0 {
            out.push('|');
            for _ in 0..columns {
                out.push_str(" --- |");
            }
            out.push('\n');
        }
        if out.len() > MOST_OUTPUT_BYTES {
            bail!("this table has too much text; split it before uploading");
        }
    }
    Ok(out.trim_end().to_string())
}

#[derive(Default)]
struct Style {
    base: String,
    heading: Option<usize>,
}

fn heading_name(name: &str) -> Option<usize> {
    name.to_ascii_lowercase()
        .replace(' ', "")
        .strip_prefix("heading")?
        .parse::<usize>()
        .ok()
        .filter(|n| (1..=6).contains(n))
}

fn styles(xml: Option<&str>) -> Result<HashMap<String, Style>> {
    let mut styles = HashMap::new();
    if let Some(xml) = xml {
        let doc = parse(xml)?;
        for n in doc.descendants().filter(|n| is(*n, "style")) {
            let Some(id) = attr(n, "styleId") else {
                continue;
            };
            let heading = child(n, "pPr")
                .and_then(|p| value(p, "outlineLvl"))
                .and_then(|v| v.parse::<usize>().ok())
                .filter(|n| *n < 6)
                .map(|n| n + 1)
                .or_else(|| value(n, "name").and_then(heading_name))
                .or_else(|| heading_name(id));
            styles.insert(
                id.to_string(),
                Style {
                    base: value(n, "basedOn").unwrap_or_default().to_string(),
                    heading,
                },
            );
        }
    }
    Ok(styles)
}

#[derive(Clone)]
struct Level {
    ordered: bool,
    start: usize,
}

fn numbering(xml: Option<&str>) -> Result<HashMap<(String, usize), Level>> {
    let mut levels = HashMap::new();
    let Some(xml) = xml else {
        return Ok(levels);
    };
    let doc = parse(xml)?;
    let mut abstracts = HashMap::new();
    for n in doc.descendants().filter(|n| is(*n, "abstractNum")) {
        let Some(id) = attr(n, "abstractNumId") else {
            continue;
        };
        for lvl in n.children().filter(|n| is(*n, "lvl")) {
            let depth = attr(lvl, "ilvl")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0)
                .min(MOST_LEVEL);
            let ordered = !matches!(value(lvl, "numFmt"), Some("bullet" | "none"));
            let start = value(lvl, "start")
                .and_then(|v| v.parse().ok())
                .unwrap_or(1);
            abstracts.insert((id.to_string(), depth), Level { ordered, start });
        }
    }
    for n in doc.descendants().filter(|n| is(*n, "num")) {
        let (Some(id), Some(base)) = (attr(n, "numId"), value(n, "abstractNumId")) else {
            continue;
        };
        for depth in 0..=MOST_LEVEL {
            if let Some(level) = abstracts.get(&(base.to_string(), depth)) {
                let mut level = level.clone();
                if let Some(over) = n.children().find(|n| {
                    is(*n, "lvlOverride")
                        && attr(*n, "ilvl").and_then(|v| v.parse::<usize>().ok()) == Some(depth)
                }) {
                    if let Some(start) = value(over, "startOverride").and_then(|v| v.parse().ok()) {
                        level.start = start;
                    }
                }
                levels.insert((id.to_string(), depth), level);
            }
        }
    }
    Ok(levels)
}

struct Word {
    links: HashMap<String, Link>,
    styles: HashMap<String, Style>,
    levels: HashMap<(String, usize), Level>,
    counts: HashMap<(String, usize), usize>,
}

impl Word {
    fn paragraph(&mut self, node: Node<'_, '_>) -> String {
        let text = inline(node, &self.links, 0);
        let Some(props) = child(node, "pPr") else {
            return text;
        };
        let style = value(props, "pStyle").unwrap_or_default();
        let mut heading = value(props, "outlineLvl")
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|n| *n < 6)
            .map(|n| n + 1)
            .or_else(|| heading_name(style));
        let mut current = style;
        for _ in 0..MOST_STYLE_HOPS {
            let Some(style) = self.styles.get(current) else {
                break;
            };
            heading = heading.or(style.heading);
            if heading.is_some() {
                break;
            }
            current = &style.base;
        }
        if let Some(level) = heading {
            return format!("{} {}", "#".repeat(level), text.trim());
        }
        if let Some(num) = child(props, "numPr") {
            let id = value(num, "numId").unwrap_or("0");
            if id != "0" {
                let depth = value(num, "ilvl")
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(0)
                    .min(MOST_LEVEL);
                let key = (id.to_string(), depth);
                let level = self.levels.get(&key);
                let mark = if level.is_some_and(|l| l.ordered) {
                    let count = self
                        .counts
                        .entry(key)
                        .or_insert_with(|| level.map_or(1, |l| l.start));
                    let mark = format!("{count}.");
                    *count = count.saturating_add(1);
                    mark
                } else {
                    "-".to_string()
                };
                self.counts
                    .retain(|(other, d), _| other != id || *d <= depth);
                return format!("{}{mark} {}", "  ".repeat(depth), text.trim());
            }
        }
        text
    }

    fn blocks(&mut self, node: Node<'_, '_>, depth: usize) -> Result<String> {
        if depth >= MOST_DEPTH {
            bail!("this Word file is nested too deeply; export a simpler document");
        }
        let mut out = Vec::new();
        let mut size = 0usize;
        for n in node.children().filter(Node::is_element) {
            let before = out.len();
            match n.tag_name().name() {
                "p" => out.push(self.paragraph(n)),
                "tbl" => {
                    let mut rows = Vec::new();
                    for row in n.children().filter(|n| is(*n, "tr")) {
                        let mut cells = Vec::new();
                        for cell in row.children().filter(|n| is(*n, "tc")) {
                            cells.push(self.blocks(cell, depth + 1)?);
                            let span = child(cell, "tcPr")
                                .and_then(|p| value(p, "gridSpan"))
                                .and_then(|v| v.parse::<usize>().ok())
                                .unwrap_or(1);
                            if span > MOST_COLUMNS
                                || cells.len().saturating_add(span.saturating_sub(1)) > MOST_COLUMNS
                            {
                                bail!("this Word table has too many columns; split it before uploading");
                            }
                            cells.extend((1..span).map(|_| String::new()));
                        }
                        rows.push(cells);
                    }
                    out.push(format!("\n{}\n", table(rows)?));
                }
                "oMathPara" => out.push(format!("\n$$\n{}\n$$\n", math(n, 0))),
                "sdt" | "sdtContent" | "ins" | "customXml" => out.push(self.blocks(n, depth + 1)?),
                _ => {}
            }
            size = size.saturating_add(out.iter().skip(before).map(String::len).sum::<usize>());
            if size > MOST_OUTPUT_BYTES {
                bail!("this Word file has too much text; split it into smaller documents");
            }
        }
        let mut blank = false;
        Ok(out
            .join("\n")
            .lines()
            .filter(|line| {
                let empty = line.trim().is_empty();
                let keep = !empty || !blank;
                blank = empty;
                keep
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

pub(super) fn docx(bytes: &[u8]) -> Result<String> {
    let mut package = Package::open(bytes)?;
    let xml = package.required("word/document.xml")?;
    let doc = parse(&xml)?;
    let body = doc
        .descendants()
        .find(|n| is(*n, "body"))
        .context("this Word file has no document body; export it again")?;
    let mut word = Word {
        links: package.relationships("word/document.xml")?,
        styles: styles(package.text("word/styles.xml")?.as_deref())?,
        levels: numbering(package.text("word/numbering.xml")?.as_deref())?,
        counts: HashMap::new(),
    };
    Ok(word.blocks(body, 0)?.trim().to_string())
}

fn slides(package: &mut Package<'_>) -> Result<Vec<String>> {
    if let Some(xml) = package.text("ppt/presentation.xml")? {
        let doc = parse(&xml)?;
        let links = package.relationships("ppt/presentation.xml")?;
        let mut slides = Vec::new();
        for n in doc.descendants().filter(|n| is(*n, "sldId")) {
            let link = attr(n, "id")
                .and_then(|id| links.get(id))
                .filter(|l| !l.external && l.kind == "slide")
                .context("this PowerPoint file has a missing slide link; export it again")?;
            slides.push(link.target.clone());
        }
        return Ok(slides);
    }
    let mut slides: Vec<(u32, String)> = package
        .zip
        .file_names()
        .filter_map(|name| {
            let number = name
                .strip_prefix("ppt/slides/slide")?
                .strip_suffix(".xml")?
                .parse()
                .ok()?;
            Some((number, name.to_string()))
        })
        .collect();
    slides.sort();
    Ok(slides.into_iter().map(|(_, name)| name).collect())
}

pub(crate) fn slide_order(bytes: &[u8]) -> Result<Vec<String>> {
    slides(&mut Package::open(bytes)?)
}

fn slide_paragraph(node: Node<'_, '_>, links: &HashMap<String, Link>) -> String {
    let text = inline(node, links, 0);
    let Some(props) = child(node, "pPr") else {
        return text;
    };
    let depth = attr(props, "lvl")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0)
        .min(MOST_LEVEL);
    if child(props, "buChar").is_some() || child(props, "buAutoNum").is_some() {
        let mark = child(props, "buAutoNum")
            .map(|n| {
                format!(
                    "{}.",
                    attr(n, "startAt")
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(1)
                )
            })
            .unwrap_or_else(|| "-".to_string());
        format!("{}{mark} {}", "  ".repeat(depth), text.trim())
    } else {
        text
    }
}

fn slide_table(node: Node<'_, '_>, links: &HashMap<String, Link>) -> Result<String> {
    let rows = node
        .children()
        .filter(|n| is(*n, "tr"))
        .map(|row| {
            row.children()
                .filter(|n| is(*n, "tc"))
                .map(|cell| {
                    cell.descendants()
                        .filter(|n| is(*n, "p"))
                        .map(|p| slide_paragraph(p, links))
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .collect()
        })
        .collect();
    table(rows)
}

fn chart(package: &mut Package<'_>, path: &str) -> Result<String> {
    let xml = package.required(path)?;
    let doc = parse(&xml)?;
    let mut series = Vec::new();
    for node in doc.descendants().filter(|n| is(*n, "ser")) {
        let name = child(node, "tx")
            .and_then(|n| n.descendants().find(|n| is(*n, "v")))
            .and_then(|n| n.text())
            .unwrap_or("Series");
        let points = |field| {
            let mut points = HashMap::new();
            if let Some(n) = child(node, field) {
                for point in n.descendants().filter(|n| is(*n, "pt")) {
                    if let Some(index) = attr(point, "idx").and_then(|s| s.parse::<usize>().ok()) {
                        let text = child(point, "v").and_then(|n| n.text()).unwrap_or_default();
                        points.insert(index, escape(text));
                    }
                }
            }
            points
        };
        series.push((escape(name), points("cat"), points("val")));
    }
    if series.is_empty() {
        return Ok(String::new());
    }
    if series.len() >= MOST_COLUMNS {
        bail!("this chart has too many series; split it before uploading");
    }
    let mut indices: Vec<usize> = series
        .iter()
        .flat_map(|(_, cats, vals)| cats.keys().chain(vals.keys()).copied())
        .collect();
    indices.sort_unstable();
    indices.dedup();
    if indices
        .len()
        .saturating_add(1)
        .saturating_mul(series.len() + 1)
        > MOST_TABLE_CELLS
    {
        bail!("this chart has too much data; split it before uploading");
    }
    let mut rows = vec![std::iter::once("Category".to_string())
        .chain(series.iter().map(|s| s.0.clone()))
        .collect()];
    for i in indices {
        let label = series
            .iter()
            .find_map(|s| s.1.get(&i))
            .cloned()
            .unwrap_or_else(|| (i.saturating_add(1)).to_string());
        rows.push(
            std::iter::once(label)
                .chain(
                    series
                        .iter()
                        .map(|s| s.2.get(&i).cloned().unwrap_or_default()),
                )
                .collect(),
        );
    }
    Ok(format!("### Chart\n\n{}", table(rows)?))
}

fn slide_blocks(
    node: Node<'_, '_>,
    links: &HashMap<String, Link>,
    package: &mut Package<'_>,
    depth: usize,
) -> Result<String> {
    if depth >= MOST_DEPTH {
        bail!("this slide is nested too deeply; export a simpler presentation");
    }
    let mut out = Vec::new();
    let mut nodes: Vec<Node<'_, '_>> = node.children().filter(Node::is_element).collect();
    if is(node, "spTree") || is(node, "grpSp") {
        nodes.sort_by_key(|n| {
            let off = n.descendants().find(|n| is(*n, "off"));
            let coordinate = |key| {
                off.and_then(|n| attr(n, key))
                    .and_then(|v| v.parse::<i64>().ok())
                    .unwrap_or(i64::MAX)
            };
            (coordinate("y"), coordinate("x"))
        });
    }
    for n in nodes {
        match n.tag_name().name() {
            "sp" => {
                let title = n
                    .descendants()
                    .any(|n| is(n, "ph") && matches!(attr(n, "type"), Some("title" | "ctrTitle")));
                if let Some(body) = child(n, "txBody") {
                    for p in body.children().filter(|n| is(*n, "p")) {
                        let text = slide_paragraph(p, links);
                        out.push(
                            if title
                                && !text.trim().is_empty()
                                && child(p, "pPr").is_none_or(|pr| {
                                    child(pr, "buChar").is_none()
                                        && child(pr, "buAutoNum").is_none()
                                })
                            {
                                format!("### {}", text.trim())
                            } else {
                                text
                            },
                        );
                    }
                }
            }
            "p" => out.push(slide_paragraph(n, links)),
            "tbl" => out.push(format!("\n{}\n", slide_table(n, links)?)),
            "chart" => {
                if let Some(link) = attr(n, "id")
                    .and_then(|id| links.get(id))
                    .filter(|l| !l.external && l.kind == "chart")
                {
                    out.push(format!("\n{}\n", chart(package, &link.target)?));
                }
            }
            "pic" | "nvSpPr" | "nvGrpSpPr" | "spPr" | "grpSpPr" => {}
            _ => out.push(slide_blocks(n, links, package, depth + 1)?),
        }
        if out.last().is_some_and(|s| s.len() > MOST_OUTPUT_BYTES) {
            bail!("this slide has too much text; split it before uploading");
        }
    }
    if out.iter().map(String::len).sum::<usize>() > MOST_OUTPUT_BYTES {
        bail!("this slide has too much text; split it before uploading");
    }
    Ok(out
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n"))
}

fn speaker_notes(xml: &str) -> Result<String> {
    let doc = parse(xml)?;
    let bodies: Vec<Node<'_, '_>> = doc
        .descendants()
        .filter(|n| {
            is(*n, "sp")
                && n.descendants()
                    .any(|n| is(n, "ph") && attr(n, "type") == Some("body"))
        })
        .collect();
    let roots = if bodies.is_empty() {
        vec![doc.root_element()]
    } else {
        bodies
    };
    let links = HashMap::new();
    Ok(roots
        .into_iter()
        .flat_map(|n| n.descendants())
        .filter(|n| is(*n, "p"))
        .map(|n| inline(n, &links, 0))
        .filter(|s| !s.trim().is_empty() && !s.chars().all(|c| c.is_ascii_digit()))
        .collect::<Vec<_>>()
        .join("\n"))
}

pub(super) fn pptx(bytes: &[u8]) -> Result<String> {
    let mut package = Package::open(bytes)?;
    let slides = slides(&mut package)?;
    if slides.is_empty() {
        bail!("this PowerPoint file has no slides in it");
    }
    let legacy = !package
        .zip
        .file_names()
        .any(|n| n == "ppt/presentation.xml");
    let mut out = String::new();
    for (index, path) in slides.iter().enumerate() {
        let number = if legacy {
            path.strip_prefix("ppt/slides/slide")
                .and_then(|s| s.strip_suffix(".xml"))
                .and_then(|s| s.parse().ok())
                .unwrap_or(index + 1)
        } else {
            index + 1
        };
        let xml = package.required(path)?;
        let doc = parse(&xml)?;
        let links = package.relationships(path)?;
        let text = slide_blocks(doc.root_element(), &links, &mut package, 0)?;
        out.push_str(&format!("## Slide {number}\n{}\n", text.trim()));
        let notes = links
            .values()
            .find(|l| !l.external && l.kind == "notesSlide")
            .map(|l| l.target.clone())
            .or_else(|| legacy.then(|| format!("ppt/notesSlides/notesSlide{number}.xml")));
        if let Some(path) = notes {
            if let Some(xml) = package.text(&path)? {
                let said = speaker_notes(&xml)?;
                if !said.is_empty() {
                    out.push_str(&format!("Speaker notes: {said}\n"));
                }
            }
        }
        out.push('\n');
        if out.len() > MOST_READ_BYTES {
            bail!("this presentation has too much text; split it into smaller files");
        }
    }
    Ok(out.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn package(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in files {
            writer
                .start_file(
                    *name,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn damaged_xml_entities_and_deep_documents_are_rejected() {
        for xml in [
            "<body><p></body>",
            "<!DOCTYPE body [<!ENTITY secret SYSTEM 'file:///private/key'>]><body>&secret;</body>",
            "<!DOCTYPE body [<!ENTITY x 'expanded'>]><body>&x;</body>",
        ] {
            assert!(parse(xml).is_err(), "{xml}");
        }
        let xml = format!(
            "{}text{}",
            "<p>".repeat(MOST_DEPTH + 1),
            "</p>".repeat(MOST_DEPTH + 1)
        );
        assert!(parse(&xml).unwrap_err().to_string().contains("deeply"));
        let bytes = package(&[("word/document.xml", b"\xff")]);
        assert!(docx(&bytes)
            .unwrap_err()
            .to_string()
            .contains("unreadable text"));
        assert!(docx(&package(&[])).is_err());
        assert!(pptx(&package(&[])).is_err());
    }

    #[test]
    fn internal_links_stay_inside_the_archive_and_external_links_are_data() {
        assert_eq!(
            joined("ppt/slides", "../notesSlides/notesSlide9.xml").unwrap(),
            "ppt/notesSlides/notesSlide9.xml"
        );
        assert_eq!(
            joined("word", "/word/media/image.png").unwrap(),
            "word/media/image.png"
        );
        for target in [
            "../../../private/key",
            "../../word/key",
            "file:///private/key",
            "C:\\key",
            "//private/key",
            "../media/../..",
        ] {
            assert!(joined("word", target).is_err(), "{target}");
        }
        for target in [
            "javascript:alert(1)",
            "file:///private/key",
            "data:text/html,bad",
            "https://example.com/a(b)",
        ] {
            let text = linked("Read".to_string(), Some(target));
            if target.starts_with("https") {
                assert!(text.contains("a%28b%29"));
            } else {
                assert_eq!(text, "Read");
            }
        }
        let bytes = package(&[("word/_rels/document.xml.rels", br#"<Relationships><Relationship Id="r1" Type="http://x/image" Target="../../../secret"/></Relationships>"#)]);
        assert!(Package::open(&bytes)
            .unwrap()
            .relationships("word/document.xml")
            .is_err());
    }

    #[test]
    fn compressed_parts_and_sparse_tables_have_limits() {
        let large = vec![b'x'; MOST_PART_BYTES + 1];
        let bytes = package(&[("word/document.xml", &large)]);
        assert!(bytes.len() < MOST_PART_BYTES);
        assert!(docx(&bytes)
            .unwrap_err()
            .to_string()
            .contains("too much text"));
        let bytes = package(&[("word/document.xml", b"<document/>")]);
        let mut read = Package::open(&bytes).unwrap();
        read.read = MOST_READ_BYTES;
        assert!(read.text("word/document.xml").is_err());
        assert!(table(vec![vec![String::new(); MOST_COLUMNS + 1]]).is_err());
        assert!(table(vec![
            vec![String::new(); MOST_COLUMNS];
            MOST_TABLE_CELLS / MOST_COLUMNS + 1
        ])
        .is_err());
        assert_eq!(
            table(vec![vec!["A | B".into(), "C\nD".into()], vec!["é".into()]]).unwrap(),
            "| A \\| B | C<br>D |\n| --- | --- |\n| é |  |"
        );
    }

    #[test]
    fn namespace_prefixes_and_literal_markdown_do_not_change_the_source() {
        let xml = r#"<x:document xmlns:x="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><x:body><x:p><x:pPr><x:pStyle x:val="Heading2"/></x:pPr><x:r><x:t>Café *[literal]* &lt;tag&gt;</x:t></x:r></x:p></x:body></x:document>"#;
        assert_eq!(
            docx(&package(&[("word/document.xml", xml.as_bytes())])).unwrap(),
            "## Café \\*\\[literal\\]\\* \\<tag\\>"
        );
    }

    #[test]
    fn positioned_slide_shapes_are_read_top_to_bottom_then_left_to_right() {
        let xml = r#"<p:sld xmlns:p="p" xmlns:a="a"><p:spTree><p:sp><p:spPr><a:xfrm><a:off x="100" y="200"/></a:xfrm></p:spPr><p:txBody><a:p><a:r><a:t>Bottom</a:t></a:r></a:p></p:txBody></p:sp><p:sp><p:spPr><a:xfrm><a:off x="200" y="100"/></a:xfrm></p:spPr><p:txBody><a:p><a:r><a:t>Right</a:t></a:r></a:p></p:txBody></p:sp><p:sp><p:spPr><a:xfrm><a:off x="100" y="100"/></a:xfrm></p:spPr><p:txBody><a:p><a:r><a:t>Left</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:sld>"#;
        assert_eq!(
            pptx(&package(&[("ppt/slides/slide1.xml", xml.as_bytes())])).unwrap(),
            "## Slide 1\nLeft\nRight\nBottom"
        );
    }
}
