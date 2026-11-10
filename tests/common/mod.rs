fn item_end(text: &str) -> usize {
    let bytes = text.as_bytes();
    let mut i = "#[cfg(test)]".len();
    let mut depth = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                i += text[i..].find('\n').unwrap_or(text.len() - i);
                continue;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += text[i..].find("*/").map_or(text.len() - i, |e| e + 2);
                continue;
            }
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
            }
            b'r' if matches!(bytes.get(i + 1), Some(b'#' | b'"')) => {
                let hashes = text[i + 1..].len() - text[i + 1..].trim_start_matches('#').len();
                if bytes.get(i + 1 + hashes) == Some(&b'"') {
                    let close = format!("\"{}", "#".repeat(hashes));
                    let from = i + 2 + hashes;
                    i = text[from..]
                        .find(&close)
                        .map_or(text.len(), |e| from + e + close.len());
                    continue;
                }
            }
            b'\'' => {
                if bytes.get(i + 1) == Some(&b'\\') {
                    i += 3 + text[i + 3..].find('\'').unwrap_or(0);
                } else if let Some(c) = text[i + 1..].chars().next() {
                    if bytes.get(i + 1 + c.len_utf8()) == Some(&b'\'') {
                        i += 1 + c.len_utf8();
                    }
                }
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                if depth == 0 {
                    return i;
                }
                depth -= 1;
                if depth == 0 && bytes[i] == b'}' {
                    return i + 1;
                }
            }
            b';' | b',' if depth == 0 => return i + 1,
            _ => {}
        }
        i += 1;
    }
    text.len()
}

pub fn shipped_part(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("#[cfg(test)]") {
        out.push_str(&rest[..at]);
        let end = (at + item_end(&rest[at..])).min(rest.len());
        out.extend(
            rest[at..end]
                .chars()
                .map(|c| if c == '\n' { c } else { ' ' }),
        );
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}
