use std::io::Read;

use anyhow::{bail, Context, Result};

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::ai::budget::Budget;
use crate::ai::chat::{self, Prompt};
use crate::ai::provider::Image;
use crate::figures::{self, Figure};

const PAGES_PER_LOOK: usize = 6;
const MOST_IMAGES: usize = 40;
const SCAN_CHARS_PER_PAGE: usize = 40;
const NOTE_TOKENS: u32 = 8_000;

#[derive(Debug, Clone)]
pub struct Upload {
    pub name: String,
    pub mime: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Default, PartialEq)]
pub struct Material {
    pub text: String,
    pub images: Vec<Image>,
    pub figures: Vec<Figure>,
}

#[derive(Debug, Default, PartialEq)]
pub struct Imported {
    pub title: String,
    pub body: String,
    pub figures: Vec<Figure>,
}

fn extension(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(_, e)| e.to_lowercase())
        .unwrap_or_default()
}

pub(crate) fn entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let after = &rest[start..];
        let Some(end) = after.find(';').filter(|&e| e <= 10) else {
            out.push('&');
            rest = &after[1..];
            continue;
        };
        let name = &after[1..end];
        let decoded = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ if name.starts_with("#x") => u32::from_str_radix(&name[2..], 16)
                .ok()
                .and_then(char::from_u32),
            _ if name.starts_with('#') => name[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = &after[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

pub fn xml_text(xml: &str, text_tag: &str, paragraph_tag: &str) -> String {
    let open = format!("<{text_tag}");
    let close = format!("</{text_tag}>");
    let end_paragraph = format!("</{paragraph_tag}>");
    let empty_paragraph = format!("<{paragraph_tag}/>");
    let mut out = String::new();
    let mut i = 0;
    while i < xml.len() {
        let rest = &xml[i..];
        if rest.starts_with(&end_paragraph) {
            out.push('\n');
            i += end_paragraph.len();
        } else if rest.starts_with(&empty_paragraph) {
            out.push('\n');
            i += empty_paragraph.len();
        } else if rest.starts_with("<w:tab/>") || rest.starts_with("<w:tab ") {
            out.push('\t');
            i += 1;
        } else if rest.starts_with("<w:br/>")
            || rest.starts_with("<a:br/>")
            || rest.starts_with("<a:br>")
        {
            out.push('\n');
            i += 1;
        } else if rest.starts_with(&open) && rest[open.len()..].starts_with(['>', ' ']) {
            let Some(body_start) = rest.find('>') else {
                break;
            };
            if rest[..body_start].ends_with('/') {
                i += body_start + 1;
                continue;
            }
            let body = &rest[body_start + 1..];
            let Some(body_end) = body.find(&close) else {
                break;
            };
            out.push_str(&entities(&body[..body_end]));
            i += body_start + 1 + body_end + close.len();
        } else {
            i += rest.chars().next().map_or(1, char::len_utf8);
        }
    }
    let lines: Vec<&str> = out.lines().map(str::trim_end).collect();
    let mut tidy = String::new();
    let mut blank = 0;
    for line in lines {
        if line.trim().is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        tidy.push_str(line);
        tidy.push('\n');
    }
    tidy.trim().to_string()
}

fn zip_file(archive: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>, name: &str) -> Option<String> {
    let file = archive.by_name(name).ok()?;
    let mut text = String::new();
    file.take(64 * 1024 * 1024).read_to_string(&mut text).ok()?;
    Some(text)
}

fn docx(bytes: &[u8]) -> Result<String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .context("this Word file could not be opened")?;
    let xml = zip_file(&mut archive, "word/document.xml")
        .context("this Word file has no document in it")?;
    Ok(xml_text(&xml, "w:t", "w:p"))
}

fn numbered(names: impl Iterator<Item = String>, prefix: &str) -> Vec<(u32, String)> {
    let mut out: Vec<(u32, String)> = names
        .filter_map(|n| {
            let number = n.strip_prefix(prefix)?.strip_suffix(".xml")?.parse().ok()?;
            Some((number, n))
        })
        .collect();
    out.sort();
    out
}

fn pptx(bytes: &[u8]) -> Result<String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .context("this PowerPoint file could not be opened")?;
    let names: Vec<String> = archive.file_names().map(str::to_string).collect();
    let slides = numbered(names.iter().cloned(), "ppt/slides/slide");
    if slides.is_empty() {
        bail!("this PowerPoint file has no slides in it");
    }
    let notes = numbered(names.into_iter(), "ppt/notesSlides/notesSlide");
    let mut out = String::new();
    for (n, name) in slides {
        let text = zip_file(&mut archive, &name)
            .map(|x| xml_text(&x, "a:t", "a:p"))
            .unwrap_or_default();
        out.push_str(&format!("## Slide {n}\n{}\n", text.trim()));
        if let Some((_, notes_name)) = notes.iter().find(|(m, _)| *m == n) {
            let said = zip_file(&mut archive, notes_name)
                .map(|x| xml_text(&x, "a:t", "a:p"))
                .unwrap_or_default();
            let said = said.trim();
            if !said.is_empty() && !said.chars().all(|c| c.is_ascii_digit()) {
                out.push_str(&format!("Speaker notes: {said}\n"));
            }
        }
        out.push('\n');
    }
    Ok(out.trim().to_string())
}

fn pdf(bytes: &[u8]) -> Result<Material> {
    let doc = lopdf::Document::load_mem(bytes)
        .context("this PDF could not be opened (it may be damaged or password-protected)")?;
    let pages = doc.get_pages();
    if pages.is_empty() {
        bail!("this PDF has no pages");
    }
    let mut text = String::new();
    for &number in pages.keys() {
        if let Ok(page) = doc.extract_text(&[number]) {
            let page = page.trim();
            if !page.is_empty() {
                text.push_str(&format!("[page {number}]\n{page}\n\n"));
            }
        }
    }
    let letters = text.chars().filter(|c| c.is_alphanumeric()).count();
    if letters >= SCAN_CHARS_PER_PAGE * pages.len() {
        return Ok(Material {
            text: text.trim().to_string(),
            images: Vec::new(),
            figures: figures::from_pdf(&doc),
        });
    }
    let mut images = Vec::new();
    for &page in pages.values() {
        for image in doc.get_page_images(page).unwrap_or_default() {
            let jpeg = image
                .filters
                .as_ref()
                .is_some_and(|f| f.len() == 1 && f[0] == "DCTDecode");
            if jpeg && image.width >= 200 && image.height >= 200 {
                images.push(Image {
                    mime: "image/jpeg".into(),
                    bytes: image.content.to_vec(),
                });
            }
        }
    }
    if images.is_empty() {
        if letters > 0 {
            return Ok(Material {
                text: text.trim().to_string(),
                images: Vec::new(),
                figures: figures::from_pdf(&doc),
            });
        }
        bail!("this PDF looks scanned, but leo cannot read its pages; take photos of them, or export them as images, and upload those");
    }
    Ok(Material {
        text: text.trim().to_string(),
        images,
        figures: Vec::new(),
    })
}

pub fn extract(upload: &Upload) -> Result<Material> {
    let ext = extension(&upload.name);
    let mime = upload.mime.to_lowercase();
    let text = |t: String| Material {
        text: t,
        ..Material::default()
    };
    let photo = |mime: String| Material {
        text: String::new(),
        images: vec![Image {
            mime: mime.clone(),
            bytes: upload.bytes.clone(),
        }],
        figures: vec![Figure {
            mime,
            bytes: upload.bytes.clone(),
            place: upload.name.clone(),
            photo: true,
        }],
    };
    if matches!(
        mime.as_str(),
        "image/jpeg" | "image/png" | "image/webp" | "image/gif"
    ) {
        return Ok(photo(mime));
    }
    match ext.as_str() {
        "pdf" => pdf(&upload.bytes),
        "docx" => docx(&upload.bytes).map(|t| Material {
            figures: figures::from_docx(&upload.bytes),
            ..text(t)
        }),
        "pptx" => pptx(&upload.bytes).map(|t| Material {
            figures: figures::from_pptx(&upload.bytes),
            ..text(t)
        }),
        "txt" | "md" | "markdown" | "text" => Ok(text(String::from_utf8_lossy(&upload.bytes).into_owned())),
        "jpg" | "jpeg" | "png" | "webp" | "gif" => Ok(photo(format!(
            "image/{}",
            if ext == "jpg" { "jpeg" } else { ext.as_str() }
        ))),
        "heic" | "heif" => bail!("{} is a HEIC photo; upload it from the phone's photo picker, which converts it, or save it as JPEG", upload.name),
        "doc" | "ppt" => bail!("{} is an old Office format; save it as .{}x and upload that", upload.name, ext),
        _ => bail!("leo cannot read {} yet: upload a PDF, Word, PowerPoint, text file, or photos", upload.name),
    }
}

pub fn gather(uploads: &[Upload]) -> Result<Material> {
    let mut all = Material::default();
    for upload in uploads {
        let found = extract(upload)?;
        if !found.text.trim().is_empty() {
            if uploads.len() > 1 {
                all.text.push_str(&format!("# {}\n\n", upload.name));
            }
            all.text.push_str(found.text.trim());
            all.text.push_str("\n\n");
        }
        all.images.extend(found.images);
        let many = uploads.len() > 1;
        all.figures.extend(found.figures.into_iter().map(|mut f| {
            if many && !f.photo {
                f.place = format!("{} of {}", f.place, upload.name);
            }
            f
        }));
    }
    let mut documents = 0;
    all.figures.retain(|f| {
        documents += usize::from(!f.photo);
        f.photo || documents <= figures::MOST_FIGURES
    });
    if all.images.len() > MOST_IMAGES {
        bail!("that is {} pages of images; leo reads up to {MOST_IMAGES} at a time, so upload them in smaller groups", all.images.len());
    }
    if all.text.trim().is_empty() && all.images.is_empty() {
        bail!("there is no text in that file for leo to read");
    }
    Ok(all)
}

const DOC_RULES: &str = "\
You turn material a student uploaded (lecture slides, a handout, a paper, a worksheet, a textbook page, or photos of a whiteboard or handwritten notes) into study notes in Markdown.

- Use interpretable language: plain words someone new to the subject can follow, with each technical term explained the first time it appears.
- Keep everything that matters for learning: definitions, steps, formulas, worked examples, results, and what figures or diagrams show, described in words.
- Leave out page furniture: headers, footers, page numbers, repeated slide titles, copyright lines.
- Organise with ## headings and bullet points (- ); bold a term where it is defined; put formulas and code in code blocks or inline code.
- Put tasks or deadlines that the material states in a final \"## Action items\" section as checkboxes (- [ ] ); leave the section out otherwise.
- Do not add material that is not in the source. If something is unreadable, write [unreadable] rather than guessing.";

const PLACING: &str = "\
Pictures from the material can go in the notes. They are listed in <figures> as figure:N, with the slide or page each comes from. Put a picture where it helps someone understand, on its own line, as ![what it shows, in a few words](figure:N), next to the notes for its slide or page. Use each one at most once and only the ones listed, and leave out pictures that are only decoration.";

const SEEING: &str = "\
The images are photos or scans of pages, in order. Read all of them, including handwriting, tables and diagrams.";

fn whole_shape() -> &'static str {
    "Shape of the reply:\n1. The first line is the title, as plain text: no \"Title:\", no #, no quotes, no bold.\n2. A blank line, then a 2-3 sentence summary.\n3. ## sections with bullet points.\n\nReply with the note only: no preamble, no remarks after it, and do not wrap it in a code block."
}

fn part_shape(part: usize, parts: usize) -> String {
    format!("This is part {part} of {parts} of the material; the other parts are handled separately. Write ## sections for this part only: no title and no summary of the whole.\n\nReply with the notes only: no preamble, no remarks after them, and do not wrap them in a code block.")
}

pub fn text_parts(text: &str, most: usize) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    for paragraph in text.split("\n\n") {
        if current.chars().count() + paragraph.chars().count() > most && !current.is_empty() {
            parts.push(std::mem::take(&mut current));
        }
        if paragraph.chars().count() > most {
            let chars: Vec<char> = paragraph.chars().collect();
            for chunk in chars.chunks(most.max(1)) {
                parts.push(chunk.iter().collect());
            }
            continue;
        }
        current.push_str(paragraph);
        current.push_str("\n\n");
    }
    if !current.trim().is_empty() {
        parts.push(current);
    }
    parts
}

pub fn offered(figures: &[Figure]) -> String {
    figures
        .iter()
        .enumerate()
        .filter(|(_, f)| !f.photo)
        .map(|(i, f)| format!("figure:{} is from {}", i + 1, f.place))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn text_prompt(text: &str, name: &str, part: Option<(usize, usize)>, figures: &str) -> Prompt {
    let shape = match part {
        Some((p, n)) => part_shape(p, n),
        None => whole_shape().to_string(),
    };
    let (rules, listed) = if figures.is_empty() {
        (String::new(), String::new())
    } else {
        (
            format!("\n\n{PLACING}"),
            format!("\n\n<figures>\n{figures}\n</figures>"),
        )
    };
    Prompt {
        system: format!("{DOC_RULES}{rules}\n\n{shape}"),
        user: format!("<material name=\"{name}\">\n{}\n</material>{listed}\n\nWrite the study notes for this material.", text.trim()),
    }
}

pub fn image_prompt(
    name: &str,
    part: Option<(usize, usize)>,
    pages: (usize, usize, usize),
) -> Prompt {
    let shape = match part {
        Some((p, n)) => part_shape(p, n),
        None => whole_shape().to_string(),
    };
    Prompt {
        system: format!("{DOC_RULES}\n\n{SEEING}\n\n{shape}"),
        user: format!(
            "These are pages {}-{} of {} from \"{name}\". Write the study notes for them.",
            pages.0, pages.1, pages.2
        ),
    }
}

pub type Write<'a> = &'a (dyn Fn(Prompt, u32) -> Result<String> + Sync);
pub type See<'a> = &'a (dyn Fn(Prompt, &[Image], u32) -> Result<String> + Sync);

pub const CHAT_DOC_CHARS: usize = 2_000_000;
const TRANSCRIBE_TOKENS: u32 = 6_000;

const TRANSCRIBING: &str = "\
You copy out what is on photographed or scanned pages so someone can ask questions about them later. Write the text exactly as it appears, in reading order, in Markdown: keep headings, lists, tables and formulas. Describe each diagram or figure in one short sentence in [brackets]. Write [unreadable] for anything you cannot read. Reply with the text only.";

pub fn clip_document(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= CHAT_DOC_CHARS {
        return text.to_string();
    }
    let mut out: String = text.chars().take(CHAT_DOC_CHARS).collect();
    out.push_str("\n\n[…the rest of this document is left out]");
    out
}

pub fn read_for_chat(
    upload: &Upload,
    see: See<'_>,
    progress: &mut dyn FnMut(&str),
) -> Result<String> {
    let material = extract(upload)?;
    let mut text = material.text.trim().to_string();
    let groups: Vec<&[Image]> = material.images.chunks(PAGES_PER_LOOK).collect();
    for (i, pages) in groups.iter().enumerate() {
        progress(&if groups.len() > 1 {
            format!("Reading the pages {}/{}", i + 1, groups.len())
        } else {
            "Reading the image".to_string()
        });
        let prompt = Prompt {
            system: TRANSCRIBING.to_string(),
            user: format!(
                "These are pages from \"{}\". Copy out what they say.",
                upload.name
            ),
        };
        let seen = see(prompt, pages, TRANSCRIBE_TOKENS)?;
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(seen.trim());
    }
    if text.trim().is_empty() {
        bail!("there is no text in {} for Felix to read", upload.name);
    }
    Ok(clip_document(&text))
}

pub fn write_note(
    material: &Material,
    name: &str,
    write: Write<'_>,
    see: See<'_>,
    progress: &mut dyn FnMut(&str, usize, usize),
    budget: Budget,
) -> Result<(String, String)> {
    let texts = if material.text.trim().is_empty() {
        Vec::new()
    } else {
        text_parts(&material.text, budget.chars())
    };
    let looks: Vec<&[Image]> = material.images.chunks(PAGES_PER_LOOK).collect();
    let figures = offered(&material.figures);
    let jobs = texts.len() + looks.len();
    if jobs == 0 {
        bail!("there is nothing in that file for leo to read");
    }
    if jobs == 1 {
        progress("Writing the note", 0, 1);
        let reply = if let Some(text) = texts.first() {
            write(text_prompt(text, name, None, &figures), NOTE_TOKENS)?
        } else {
            let n = material.images.len();
            see(image_prompt(name, None, (1, n, n)), looks[0], NOTE_TOKENS)?
        };
        progress("Writing the note", 1, 1);
        let (title, body) = chat::split_title_body(&chat::clean_reply(&reply));
        return Ok((title, body));
    }
    let total = jobs + 1;
    let n = material.images.len();
    let run = |i: usize| -> Result<String> {
        let reply = if let Some(text) = texts.get(i) {
            write(
                text_prompt(text, name, Some((i + 1, jobs)), &figures),
                NOTE_TOKENS,
            )?
        } else {
            let j = i - texts.len();
            let batch = looks[j];
            let pages = (j * PAGES_PER_LOOK + 1, j * PAGES_PER_LOOK + batch.len(), n);
            see(
                image_prompt(name, Some((i + 1, jobs)), pages),
                batch,
                NOTE_TOKENS,
            )?
        };
        Ok(chat::clean_reply(&reply))
    };
    let next = AtomicUsize::new(0);
    let results: Vec<Mutex<Option<Result<String>>>> = (0..jobs).map(|_| Mutex::new(None)).collect();
    let mut step = |done: usize| {
        progress(
            &format!("Writing part {} of {jobs}", (done + 1).min(jobs)),
            done,
            total,
        );
    };
    step(0);
    std::thread::scope(|scope| {
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        for _ in 0..budget.at_once.max(1).min(jobs) {
            let tx = tx.clone();
            let (next, results, run) = (&next, &results, &run);
            scope.spawn(move || loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= jobs {
                    return;
                }
                let written = run(i);
                let failed = written.is_err();
                if let Ok(mut slot) = results[i].lock() {
                    *slot = Some(written);
                }
                if failed {
                    next.store(jobs, Ordering::Relaxed);
                    return;
                }
                let _ = tx.send(());
            });
        }
        drop(tx);
        for (done, ()) in rx.iter().enumerate() {
            step(done + 1);
        }
    });
    let mut sections = Vec::with_capacity(jobs);
    for slot in results {
        match slot.into_inner().ok().flatten() {
            Some(Ok(text)) => sections.push(text),
            Some(Err(e)) => return Err(e),
            None => bail!("a part of the note was not written"),
        }
    }
    progress("Naming the note", jobs, total);
    let joined = sections.join("\n\n");
    let digest: String = joined.chars().take(30_000).collect();
    let (title, summary) = match write(chat::build_summary_prompt(&digest), 600) {
        Ok(reply) => chat::split_title_body(&reply),
        Err(_) => (String::new(), String::new()),
    };
    progress("Naming the note", total, total);
    let body = if summary.trim().is_empty() {
        joined
    } else {
        format!("{}\n\n{joined}", summary.trim())
    };
    Ok((title, body))
}

pub fn import(
    uploads: &[Upload],
    progress: &mut dyn FnMut(&str, usize, usize),
) -> Result<Imported> {
    progress("Reading the file", 0, 1);
    let material = gather(uploads)?;
    let name = uploads.first().map(|u| u.name.as_str()).unwrap_or("upload");
    let write = |prompt: Prompt, most: u32| -> Result<String> {
        Ok(crate::ai::chat_outcome(prompt, most)?.value)
    };
    let see = |prompt: Prompt, images: &[Image], most: u32| crate::ai::see(prompt, images, most);
    let budget = crate::ai::writing_budget();
    let (title, body) = write_note(&material, name, &write, &see, progress, budget)?;
    Ok(Imported {
        title,
        body,
        figures: material.figures,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn never(_: Prompt, _: &[Image], _: u32) -> Result<String> {
        panic!("there are no images here")
    }

    #[test]
    fn photos_are_kept_as_pictures_and_document_figures_are_offered_by_place() {
        let photo = Upload {
            name: "board.png".into(),
            mime: "image/png".into(),
            bytes: b"\x89PNG\r\n\x1a\nrest".to_vec(),
        };
        let text = Upload {
            name: "notes.md".into(),
            mime: "text/markdown".into(),
            bytes: b"# Heaps".to_vec(),
        };
        let gathered = gather(&[photo.clone(), text]).unwrap();
        assert_eq!(gathered.images.len(), 1);
        assert_eq!(gathered.figures.len(), 1);
        assert!(gathered.figures[0].photo);
        assert_eq!(gathered.figures[0].place, "board.png");
        let figures = vec![
            gathered.figures[0].clone(),
            Figure {
                mime: "image/png".into(),
                bytes: vec![],
                place: "slide 3".into(),
                photo: false,
            },
        ];
        assert_eq!(offered(&figures), "figure:2 is from slide 3");
        let with = text_prompt("x", "deck.pptx", None, &offered(&figures));
        assert!(with
            .system
            .contains("![what it shows, in a few words](figure:N)"));
        assert!(with
            .user
            .contains("<figures>\nfigure:2 is from slide 3\n</figures>"));
        let without = text_prompt("x", "notes.md", None, "");
        assert!(!without.system.contains("figure:N") && !without.user.contains("<figures>"));
    }

    #[test]
    fn notes_from_uploads_are_written_in_interpretable_language() {
        assert!(text_prompt("x", "a.pdf", None, "")
            .system
            .contains("Use interpretable language"));
        assert!(image_prompt("a.jpg", None, (1, 1, 1))
            .system
            .contains("Use interpretable language"));
    }

    #[test]
    fn a_document_for_felix_keeps_only_its_text_and_is_clipped() {
        let text = Upload {
            name: "notes.md".into(),
            mime: "text/markdown".into(),
            bytes: b"# Heaps\n\nMinimum at the root.".to_vec(),
        };
        let read = read_for_chat(&text, &never, &mut |_| {}).unwrap();
        assert_eq!(read, "# Heaps\n\nMinimum at the root.");
        let long = Upload {
            name: "long.txt".into(),
            mime: "text/plain".into(),
            bytes: "word ".repeat(CHAT_DOC_CHARS / 5 + 100).into_bytes(),
        };
        let clipped = read_for_chat(&long, &never, &mut |_| {}).unwrap();
        assert!(clipped.chars().count() < CHAT_DOC_CHARS + 60);
        assert!(clipped.ends_with("[…the rest of this document is left out]"));
        let blank = Upload {
            name: "blank.txt".into(),
            mime: "text/plain".into(),
            bytes: b"   ".to_vec(),
        };
        assert!(read_for_chat(&blank, &never, &mut |_| {}).is_err());
    }

    #[test]
    fn a_photo_for_felix_is_copied_out_by_the_ai_that_can_see() {
        let looked = Mutex::new(0);
        let see = |prompt: Prompt, images: &[Image], _: u32| -> Result<String> {
            *looked.lock().unwrap() += 1;
            assert_eq!(images.len(), 1);
            assert!(prompt.system.contains("copy out"));
            Ok("Board: Dijkstra uses a heap".into())
        };
        let photo = Upload {
            name: "board.jpg".into(),
            mime: "image/jpeg".into(),
            bytes: vec![0xff, 0xd8, 0xff],
        };
        let mut steps = Vec::new();
        let read = read_for_chat(&photo, &see, &mut |s| steps.push(s.to_string())).unwrap();
        assert_eq!(read, "Board: Dijkstra uses a heap");
        assert_eq!(*looked.lock().unwrap(), 1);
        assert_eq!(steps, ["Reading the image"]);
    }

    fn office(files: &[(&str, &str)]) -> Vec<u8> {
        let mut buffer = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buffer);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for (name, text) in files {
                zip.start_file(*name, options).unwrap();
                zip.write_all(text.as_bytes()).unwrap();
            }
            zip.finish().unwrap();
        }
        buffer.into_inner()
    }

    fn upload(name: &str, mime: &str, bytes: Vec<u8>) -> Upload {
        Upload {
            name: name.into(),
            mime: mime.into(),
            bytes,
        }
    }

    #[test]
    fn word_documents_keep_their_paragraphs_and_symbols() {
        let xml = r#"<w:document><w:body><w:p><w:r><w:t>Heaps &amp; queues</w:t></w:r></w:p><w:p><w:r><w:t xml:space="preserve">Insert is </w:t></w:r><w:r><w:t>O(log n) &lt;fast&gt;</w:t></w:r></w:p><w:p/><w:p><w:r><w:t>Caf&#233;</w:t></w:r></w:p></w:body></w:document>"#;
        let bytes = office(&[("word/document.xml", xml)]);
        let got = extract(&upload("notes.docx", "application/octet-stream", bytes)).unwrap();
        assert_eq!(
            got.text,
            "Heaps & queues\nInsert is O(log n) <fast>\n\nCafé"
        );
        assert!(got.images.is_empty());
    }

    #[test]
    fn slides_come_in_order_with_their_speaker_notes() {
        let slide = |t: &str| format!("<p:sld><a:p><a:r><a:t>{t}</a:t></a:r></a:p></p:sld>");
        let bytes = office(&[
            ("ppt/slides/slide10.xml", &slide("Ten")),
            ("ppt/slides/slide2.xml", &slide("Two")),
            ("ppt/slides/slide1.xml", &slide("One")),
            (
                "ppt/notesSlides/notesSlide2.xml",
                &slide("Say this out loud"),
            ),
            ("ppt/notesSlides/notesSlide1.xml", &slide("1")),
        ]);
        let got = extract(&upload("deck.pptx", "", bytes)).unwrap().text;
        assert_eq!(got, "## Slide 1\nOne\n\n## Slide 2\nTwo\nSpeaker notes: Say this out loud\n\n## Slide 10\nTen");
    }

    #[test]
    fn photos_become_images_and_unknown_files_say_what_works() {
        let photo = extract(&upload("board.jpg", "image/jpeg", vec![1, 2])).unwrap();
        assert_eq!(photo.images.len(), 1);
        assert_eq!(photo.images[0].mime, "image/jpeg");
        let png = extract(&upload("scan.PNG", "", vec![1])).unwrap();
        assert_eq!(png.images[0].mime, "image/png");
        assert!(extract(&upload("x.heic", "", vec![]))
            .unwrap_err()
            .to_string()
            .contains("HEIC"));
        assert!(extract(&upload("old.doc", "", vec![]))
            .unwrap_err()
            .to_string()
            .contains(".docx"));
        assert!(extract(&upload("a.zip", "", vec![]))
            .unwrap_err()
            .to_string()
            .contains("PDF, Word"));
        assert!(extract(&upload("bad.docx", "", vec![1, 2, 3])).is_err());
        assert_eq!(
            extract(&upload("a.md", "text/markdown", b"# Hi".to_vec()))
                .unwrap()
                .text,
            "# Hi"
        );
    }

    fn pdf_with(text: Option<&str>, jpeg: Option<&[u8]>) -> Vec<u8> {
        use lopdf::content::{Content, Operation};
        use lopdf::{dictionary, Document, Object, Stream};
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(
            dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Courier" },
        );
        let mut xobjects = lopdf::Dictionary::new();
        let mut ops = Vec::new();
        if let Some(text) = text {
            ops.extend([
                Operation::new("BT", vec![]),
                Operation::new("Tf", vec!["F1".into(), 12.into()]),
                Operation::new("Td", vec![50.into(), 700.into()]),
                Operation::new("Tj", vec![Object::string_literal(text)]),
                Operation::new("ET", vec![]),
            ]);
        }
        if let Some(jpeg) = jpeg {
            let image = Stream::new(
                dictionary! { "Type" => "XObject", "Subtype" => "Image", "Width" => 400, "Height" => 500, "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8, "Filter" => "DCTDecode" },
                jpeg.to_vec(),
            );
            let image_id = doc.add_object(image);
            xobjects.set("Im1", image_id);
            ops.extend([
                Operation::new("q", vec![]),
                Operation::new(
                    "cm",
                    vec![
                        400.into(),
                        0.into(),
                        0.into(),
                        500.into(),
                        0.into(),
                        0.into(),
                    ],
                ),
                Operation::new("Do", vec!["Im1".into()]),
                Operation::new("Q", vec![]),
            ]);
        }
        let content = Content { operations: ops };
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        let resources =
            dictionary! { "Font" => dictionary! { "F1" => font_id }, "XObject" => xobjects };
        let page_id = doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id, "Contents" => content_id, "Resources" => resources, "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()] });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(
                dictionary! { "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1 },
            ),
        );
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog_id);
        let mut out = Vec::new();
        doc.save_to(&mut out).unwrap();
        out
    }

    #[test]
    fn a_pdf_with_text_is_read_and_a_scanned_one_gives_its_page_images() {
        let typed = pdf_with(
            Some("Dijkstra uses a priority queue to pick the closest vertex next"),
            None,
        );
        let got = extract(&upload("paper.pdf", "application/pdf", typed)).unwrap();
        assert!(got.text.contains("priority queue"), "{}", got.text);
        assert!(got.images.is_empty());
        let fake_jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3, 4];
        let scanned = pdf_with(None, Some(&fake_jpeg));
        let got = extract(&upload("scan.pdf", "application/pdf", scanned)).unwrap();
        assert_eq!(got.images.len(), 1);
        assert_eq!(got.images[0].bytes, fake_jpeg);
        assert!(extract(&upload("empty.pdf", "", pdf_with(None, None))).is_err());
        assert!(extract(&upload("junk.pdf", "", b"not a pdf".to_vec())).is_err());
    }

    #[test]
    fn long_text_is_split_into_parts_without_losing_any() {
        let paragraph = "word ".repeat(1000);
        let text = [paragraph.as_str(); 12].join("\n\n");
        let most = Budget::FREE.chars();
        let parts = text_parts(&text, most);
        assert!(parts.len() >= 3);
        assert!(parts.iter().all(|p| p.chars().count() <= most + 2));
        assert_eq!(parts.concat().matches("word").count(), 12_000);
        let huge = "x".repeat(most * 2 + 5);
        assert_eq!(text_parts(&huge, most).len(), 3);
        assert_eq!(
            text_parts(&text, Budget::AGENT.chars()).len(),
            1,
            "a big model reads it in one go"
        );
    }

    #[test]
    fn a_short_file_is_one_request_and_a_long_one_is_written_in_parts_then_named() {
        let asked = Mutex::new(Vec::new());
        let write = |p: Prompt, _: u32| -> Result<String> {
            asked.lock().unwrap().push(p.system.clone());
            if p.system.contains("name and summarize") {
                Ok("Graphs and queues\n\nA long handout about graphs.".into())
            } else if p.system.contains("This is part") {
                Ok("## A section\n- point".into())
            } else {
                Ok("Heaps\n\nHow heaps work.\n\n## Insert\n- sift up".into())
            }
        };
        let saw = Mutex::new(0);
        let see = |p: Prompt, images: &[Image], _: u32| -> Result<String> {
            assert!(p.system.contains("photos or scans"));
            *saw.lock().unwrap() += images.len();
            Ok("## Whiteboard\n- drawing".into())
        };
        let short = Material {
            text: "Heaps keep the minimum on top.".into(),
            ..Material::default()
        };
        let mut steps = Vec::new();
        let (title, body) = write_note(
            &short,
            "heaps.txt",
            &write,
            &see,
            &mut |s, d, t| steps.push((s.to_string(), d, t)),
            Budget::FREE,
        )
        .unwrap();
        assert_eq!(title, "Heaps");
        assert!(body.starts_with("How heaps work."));
        assert_eq!(asked.lock().unwrap().len(), 1);
        assert_eq!(steps.last().unwrap().1, steps.last().unwrap().2);

        asked.lock().unwrap().clear();
        let long = Material {
            text: vec!["word ".repeat(1000); 10].join("\n\n"),
            images: (0..8)
                .map(|i| Image {
                    mime: "image/jpeg".into(),
                    bytes: vec![i],
                })
                .collect(),
            figures: vec![],
        };
        let (title, body) = write_note(
            &long,
            "handout.pdf",
            &write,
            &see,
            &mut |_, _, _| {},
            Budget::FREE,
        )
        .unwrap();
        assert_eq!(title, "Graphs and queues");
        assert!(body.starts_with("A long handout about graphs."));
        assert!(body.contains("## Whiteboard"));
        assert_eq!(
            *saw.lock().unwrap(),
            8,
            "every page image is looked at once"
        );
        assert!(asked
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .contains("name and summarize"));
    }

    #[test]
    fn parts_are_written_at_the_same_time_and_kept_in_order() {
        use std::sync::atomic::AtomicUsize;
        let now = AtomicUsize::new(0);
        let most = AtomicUsize::new(0);
        let write = |p: Prompt, _: u32| -> Result<String> {
            if p.system.contains("name and summarize") {
                return Ok("Title\n\nSummary.".into());
            }
            let busy = now.fetch_add(1, Ordering::SeqCst) + 1;
            most.fetch_max(busy, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(40));
            now.fetch_sub(1, Ordering::SeqCst);
            let part = p
                .system
                .split("This is part ")
                .nth(1)
                .unwrap()
                .split(' ')
                .next()
                .unwrap()
                .to_string();
            Ok(format!("## Part {part}"))
        };
        let see = |_: Prompt, _: &[Image], _: u32| -> Result<String> { unreachable!() };
        let material = Material {
            text: vec!["word ".repeat(1000); 6].join("\n\n"),
            ..Material::default()
        };
        let budget = Budget {
            words: 1000,
            at_once: 3,
        };
        let mut steps = Vec::new();
        let (_, body) = write_note(
            &material,
            "a.pdf",
            &write,
            &see,
            &mut |_, d, t| steps.push((d, t)),
            budget,
        )
        .unwrap();
        let order: Vec<&str> = body.lines().filter(|l| l.starts_with("## Part")).collect();
        assert_eq!(
            order,
            [
                "## Part 1",
                "## Part 2",
                "## Part 3",
                "## Part 4",
                "## Part 5",
                "## Part 6"
            ]
        );
        assert_eq!(most.load(Ordering::SeqCst), 3);
        assert_eq!(steps.last(), Some(&(7, 7)));
        assert!(
            steps.windows(2).all(|w| w[0].0 <= w[1].0),
            "progress only goes forward"
        );

        let failing = |p: Prompt, _: u32| -> Result<String> {
            if p.system.contains("part 2 of") {
                anyhow::bail!("rate limited")
            }
            Ok("## fine".into())
        };
        let err = write_note(
            &material,
            "a.pdf",
            &failing,
            &see,
            &mut |_, _, _| {},
            budget,
        )
        .unwrap_err();
        assert!(err.to_string().contains("rate limited"));
    }

    #[test]
    fn too_many_pages_or_nothing_to_read_is_said_plainly() {
        let many: Vec<Upload> = (0..41)
            .map(|i| upload(&format!("{i}.jpg"), "image/jpeg", vec![1]))
            .collect();
        assert!(gather(&many)
            .unwrap_err()
            .to_string()
            .contains("smaller groups"));
        assert!(gather(&[upload("blank.txt", "", b"   ".to_vec())]).is_err());
        let both = gather(&[
            upload("a.txt", "", b"alpha".to_vec()),
            upload("b.jpg", "image/jpeg", vec![9]),
        ])
        .unwrap();
        assert!(both.text.starts_with("# a.txt\n\nalpha"));
        assert_eq!(both.images.len(), 1);
    }
}
