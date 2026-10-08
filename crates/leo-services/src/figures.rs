use std::collections::HashMap;
use std::io::{Read, Write as _};

use leo_core::attachments;

pub const MOST_FIGURES: usize = 24;
const LEAST_WIDE: u32 = 150;
const LEAST_TALL: u32 = 100;
const LEAST_AREA: u64 = 40_000;
const LEAST_BYTES: usize = 3_000;
const MOST_STRETCH: u32 = 6;
const REPEATED_FROM: usize = 3;

#[derive(Debug, Clone, PartialEq)]
pub struct Figure {
    pub mime: String,
    pub bytes: Vec<u8>,
    pub place: String,
    pub photo: bool,
}

impl Figure {
    pub fn extension(&self) -> &'static str {
        attachments::kind_of(&self.bytes).unwrap_or("png")
    }
}

fn worth_showing(bytes: &[u8]) -> bool {
    let Some((w, h)) = attachments::dimensions(bytes) else {
        return false;
    };
    let (long, short) = (w.max(h), w.min(h).max(1));
    w.max(h) >= LEAST_WIDE
        && w.min(h) >= LEAST_TALL
        && u64::from(w) * u64::from(h) >= LEAST_AREA
        && bytes.len() >= LEAST_BYTES
        && long / short <= MOST_STRETCH
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

pub fn keep(found: Vec<Figure>) -> Vec<Figure> {
    let mut uses: HashMap<u64, usize> = HashMap::new();
    for figure in &found {
        *uses.entry(fnv(&figure.bytes)).or_default() += 1;
    }
    let mut seen = std::collections::HashSet::new();
    found
        .into_iter()
        .filter(|f| {
            let hash = fnv(&f.bytes);
            f.photo || (uses[&hash] < REPEATED_FROM && seen.insert(hash) && worth_showing(&f.bytes))
        })
        .take(MOST_FIGURES)
        .collect()
}

fn zip_bytes(archive: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>, name: &str) -> Option<Vec<u8>> {
    let file = archive.by_name(name).ok()?;
    let mut bytes = Vec::new();
    file.take(attachments::MOST_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() <= attachments::MOST_BYTES).then_some(bytes)
}

fn zip_text(archive: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>, name: &str) -> Option<String> {
    String::from_utf8(zip_bytes(archive, name)?).ok()
}

fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let at = tag.find(&format!(" {name}=\""))? + name.len() + 3;
    let rest = &tag[at..];
    Some(&rest[..rest.find('"')?])
}

fn image_links(rels: &str) -> Vec<(String, String)> {
    rels.split("<Relationship ")
        .skip(1)
        .filter_map(|tag| {
            let tag = &format!(" {}", &tag[..tag.find('>')?]);
            let kind = attribute(tag, "Type")?;
            if !kind.ends_with("/image") || attribute(tag, "TargetMode") == Some("External") {
                return None;
            }
            Some((
                attribute(tag, "Id")?.to_string(),
                attribute(tag, "Target")?.to_string(),
            ))
        })
        .collect()
}

fn joined(base: &str, target: &str) -> String {
    if let Some(rooted) = target.strip_prefix('/') {
        return rooted.to_string();
    }
    let mut parts: Vec<&str> = base.split('/').collect();
    for part in target.split('/') {
        match part {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            other => parts.push(other),
        }
    }
    parts.join("/")
}

fn embedded_in(
    archive: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>,
    xml: &str,
    rels: &str,
    folder: &str,
    place: &str,
) -> Vec<Figure> {
    let mut used: Vec<(usize, String)> = image_links(rels)
        .into_iter()
        .filter_map(|(id, target)| {
            let at = xml
                .find(&format!("r:embed=\"{id}\""))
                .or_else(|| xml.find(&format!("r:id=\"{id}\"")))?;
            Some((at, joined(folder, &target)))
        })
        .collect();
    used.sort();
    used.into_iter()
        .filter_map(|(_, path)| {
            let bytes = zip_bytes(archive, &path)?;
            let kind = attachments::kind_of(&bytes)?;
            Some(Figure {
                mime: attachments::mime_of(kind)?.to_string(),
                bytes,
                place: place.to_string(),
                photo: false,
            })
        })
        .collect()
}

pub fn from_pptx(bytes: &[u8]) -> Vec<Figure> {
    let Ok(mut archive) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else {
        return Vec::new();
    };
    let mut slides: Vec<(u32, String)> = archive
        .file_names()
        .filter_map(|n| {
            let number = n
                .strip_prefix("ppt/slides/slide")?
                .strip_suffix(".xml")?
                .parse()
                .ok()?;
            Some((number, n.to_string()))
        })
        .collect();
    slides.sort();
    let mut out = Vec::new();
    for (n, name) in slides {
        let rels_name = format!("ppt/slides/_rels/slide{n}.xml.rels");
        let (Some(xml), Some(rels)) = (
            zip_text(&mut archive, &name),
            zip_text(&mut archive, &rels_name),
        ) else {
            continue;
        };
        out.extend(embedded_in(
            &mut archive,
            &xml,
            &rels,
            "ppt/slides",
            &format!("slide {n}"),
        ));
    }
    keep(out)
}

pub fn from_docx(bytes: &[u8]) -> Vec<Figure> {
    let Ok(mut archive) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else {
        return Vec::new();
    };
    let (Some(xml), Some(rels)) = (
        zip_text(&mut archive, "word/document.xml"),
        zip_text(&mut archive, "word/_rels/document.xml.rels"),
    ) else {
        return Vec::new();
    };
    keep(embedded_in(
        &mut archive,
        &xml,
        &rels,
        "word",
        "the document",
    ))
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut crc = crc32fast::Hasher::new();
    crc.update(kind);
    crc.update(data);
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&crc.finalize().to_be_bytes());
}

pub fn png(width: u32, height: u32, channels: u8, idat: &[u8]) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, if channels == 3 { 2 } else { 0 }, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", idat);
    chunk(&mut out, b"IEND", &[]);
    out
}

pub fn filtered_rows(raw: &[u8], width: usize, height: usize, channels: usize) -> Option<Vec<u8>> {
    let row = width.checked_mul(channels)?;
    if raw.len() < row.checked_mul(height)? {
        return None;
    }
    let mut rows = Vec::with_capacity((row + 1) * height);
    for line in raw.chunks_exact(row).take(height) {
        rows.push(0);
        rows.extend_from_slice(line);
    }
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&rows).ok()?;
    encoder.finish().ok()
}

fn channels_of(doc: &lopdf::Document, dict: &lopdf::Dictionary) -> Option<usize> {
    let space = dict.get(b"ColorSpace").ok()?;
    let space = match space {
        lopdf::Object::Reference(id) => doc.get_object(*id).ok()?,
        other => other,
    };
    match space {
        lopdf::Object::Name(name) => match name.as_slice() {
            b"DeviceRGB" | b"CalRGB" => Some(3),
            b"DeviceGray" | b"CalGray" => Some(1),
            _ => None,
        },
        lopdf::Object::Array(parts) => {
            let kind = parts.first()?.as_name().ok()?;
            match kind {
                b"ICCBased" => {
                    let id = parts.get(1)?.as_reference().ok()?;
                    let n = doc
                        .get_object(id)
                        .ok()?
                        .as_stream()
                        .ok()?
                        .dict
                        .get(b"N")
                        .ok()?
                        .as_i64()
                        .ok()?;
                    matches!(n, 1 | 3).then_some(n as usize)
                }
                b"CalRGB" => Some(3),
                b"CalGray" => Some(1),
                _ => None,
            }
        }
        _ => None,
    }
}

fn number(dict: &lopdf::Dictionary, key: &[u8]) -> Option<i64> {
    dict.get(key).ok()?.as_i64().ok()
}

fn pdf_figure(doc: &lopdf::Document, image: &lopdf::xobject::PdfImage) -> Option<Vec<u8>> {
    let filters = image.filters.as_deref().unwrap_or(&[]);
    let (width, height) = (
        u32::try_from(image.width).ok()?,
        u32::try_from(image.height).ok()?,
    );
    if filters == ["DCTDecode"] {
        let shown = matches!(
            image.color_space.as_deref(),
            None | Some("DeviceRGB" | "DeviceGray" | "CalRGB" | "CalGray")
        ) || channels_of(doc, image.origin_dict).is_some();
        return shown.then(|| image.content.to_vec());
    }
    if filters != ["FlateDecode"] || image.bits_per_component != Some(8) {
        return None;
    }
    let channels = channels_of(doc, image.origin_dict)?;
    let parms = match image.origin_dict.get(b"DecodeParms") {
        Ok(lopdf::Object::Dictionary(d)) => Some(d.clone()),
        Ok(lopdf::Object::Reference(id)) => doc.get_dictionary(*id).ok().cloned(),
        _ => None,
    };
    let predictor = parms
        .as_ref()
        .and_then(|p| number(p, b"Predictor"))
        .unwrap_or(1);
    if predictor >= 10 {
        let p = parms?;
        let fits = number(&p, b"Colors").unwrap_or(1) == channels as i64
            && number(&p, b"BitsPerComponent").unwrap_or(8) == 8
            && number(&p, b"Columns").unwrap_or(1) == i64::from(width);
        return fits.then(|| png(width, height, channels as u8, image.content));
    }
    if predictor != 1 {
        return None;
    }
    let mut raw = Vec::new();
    flate2::read::ZlibDecoder::new(image.content)
        .take(width as u64 * height as u64 * channels as u64 + 1)
        .read_to_end(&mut raw)
        .ok()?;
    let idat = filtered_rows(&raw, width as usize, height as usize, channels)?;
    Some(png(width, height, channels as u8, &idat))
}

pub fn from_pdf(doc: &lopdf::Document) -> Vec<Figure> {
    let mut out = Vec::new();
    for (number, page) in doc.get_pages() {
        for image in doc.get_page_images(page).unwrap_or_default() {
            if image.width < i64::from(LEAST_TALL) || image.height < i64::from(LEAST_TALL) {
                continue;
            }
            let Some(bytes) = pdf_figure(doc, &image) else {
                continue;
            };
            let Some(kind) = attachments::kind_of(&bytes) else {
                continue;
            };
            out.push(Figure {
                mime: attachments::mime_of(kind)
                    .unwrap_or("image/png")
                    .to_string(),
                bytes,
                place: format!("page {number}"),
                photo: false,
            });
        }
        if out.len() > MOST_FIGURES * 4 {
            break;
        }
    }
    keep(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    fn picture(width: u32, height: u32, shade: u8) -> Vec<u8> {
        let mut raw = Vec::new();
        for y in 0..height {
            for x in 0..width {
                raw.extend_from_slice(&[shade, (x % 251) as u8, (y % 241) as u8]);
            }
        }
        let idat = filtered_rows(&raw, width as usize, height as usize, 3).unwrap();
        png(width, height, 3, &idat)
    }

    fn figure(bytes: Vec<u8>, place: &str) -> Figure {
        Figure {
            mime: "image/png".into(),
            bytes,
            place: place.into(),
            photo: false,
        }
    }

    #[test]
    fn a_made_png_is_a_real_png() {
        let made = picture(320, 200, 9);
        assert_eq!(attachments::kind_of(&made), Some("png"));
        assert_eq!(attachments::dimensions(&made), Some((320, 200)));
        let crc_end = made.len() - 4;
        let mut check = crc32fast::Hasher::new();
        check.update(b"IEND");
        assert_eq!(made[crc_end..], check.finalize().to_be_bytes());
        assert!(filtered_rows(&[1, 2], 4, 4, 3).is_none());
    }

    #[test]
    fn logos_icons_strips_and_repeats_are_left_out() {
        let diagram = picture(400, 300, 1);
        let other = picture(500, 260, 2);
        let logo = picture(320, 200, 3);
        let found = vec![
            figure(diagram.clone(), "slide 1"),
            figure(logo.clone(), "slide 1"),
            figure(picture(40, 40, 4), "slide 2"),
            figure(picture(1200, 120, 5), "slide 2"),
            figure(logo.clone(), "slide 2"),
            figure(diagram.clone(), "slide 3"),
            figure(logo, "slide 3"),
            figure(other.clone(), "slide 4"),
        ];
        let kept = keep(found);
        let places: Vec<&str> = kept.iter().map(|f| f.place.as_str()).collect();
        assert_eq!(places, ["slide 1", "slide 4"]);
        assert_eq!(kept[0].bytes, diagram);
        assert_eq!(kept[1].bytes, other);
    }

    #[test]
    fn slide_pictures_are_found_through_the_slide_and_masters_are_skipped() {
        let diagram = picture(400, 300, 7);
        let master = picture(420, 300, 8);
        let mut zipped = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut zipped));
            let opts = zip::write::SimpleFileOptions::default();
            let mut add = |name: &str, bytes: &[u8]| {
                zip.start_file(name, opts).unwrap();
                zip.write_all(bytes).unwrap();
            };
            add(
                "ppt/slides/slide1.xml",
                b"<p:sld><a:t>Heaps</a:t><a:blip r:embed=\"rId2\"/></p:sld>",
            );
            add(
                "ppt/slides/_rels/slide1.xml.rels",
                b"<Relationships><Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"../media/image1.png\"/><Relationship Id=\"rId3\" Type=\"http://x/image\" Target=\"../media/unused.png\"/><Relationship Id=\"rId4\" Type=\"http://x/image\" Target=\"https://example.com/a.png\" TargetMode=\"External\"/></Relationships>",
            );
            add("ppt/media/image1.png", &diagram);
            add("ppt/media/unused.png", &master);
            add("ppt/slideMasters/slideMaster1.xml", b"<p:sldMaster/>");
            zip.finish().unwrap();
        }
        let found = from_pptx(&zipped);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].place, "slide 1");
        assert_eq!(found[0].bytes, diagram);
        assert!(from_pptx(b"not a zip").is_empty());
        assert_eq!(joined("ppt/slides", "../media/a.png"), "ppt/media/a.png");
        assert_eq!(joined("word", "media/a.png"), "word/media/a.png");
    }

    #[test]
    fn a_pdf_figure_comes_out_as_a_picture() {
        let mut doc = lopdf::Document::with_version("1.5");
        let width = 300u32;
        let height = 200u32;
        let mut seed = 7u32;
        let raw: Vec<u8> = (0..width * height * 3)
            .map(|_| {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                (seed >> 16) as u8
            })
            .collect();
        let mut deflated =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        deflated.write_all(&raw).unwrap();
        let mut dict = lopdf::Dictionary::new();
        dict.set("Type", lopdf::Object::Name(b"XObject".to_vec()));
        dict.set("Subtype", lopdf::Object::Name(b"Image".to_vec()));
        dict.set("Width", i64::from(width));
        dict.set("Height", i64::from(height));
        dict.set("ColorSpace", lopdf::Object::Name(b"DeviceRGB".to_vec()));
        dict.set("BitsPerComponent", 8);
        dict.set("Filter", lopdf::Object::Name(b"FlateDecode".to_vec()));
        let image = doc.add_object(lopdf::Stream::new(dict, deflated.finish().unwrap()));
        let pages = doc.new_object_id();
        let mut xobjects = lopdf::Dictionary::new();
        xobjects.set("Im1", image);
        let mut resources = lopdf::Dictionary::new();
        resources.set("XObject", xobjects);
        let page = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages,
            "Resources" => resources,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        });
        doc.objects.insert(
            pages,
            lopdf::Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![page.into()],
                "Count" => 1,
            }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        doc.trailer.set("Root", catalog);
        let found = from_pdf(&doc);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].place, "page 1");
        assert_eq!(
            attachments::dimensions(&found[0].bytes),
            Some((width, height))
        );
        assert_eq!(found[0].mime, "image/png");
    }

    #[test]
    fn a_pdf_figure_stored_with_png_predictors_is_rewrapped_as_is() {
        let doc = lopdf::Document::with_version("1.5");
        let rows = filtered_rows(&[90; 200 * 120], 200, 120, 1).unwrap();
        let parms = dictionary! { "Predictor" => 15, "Colors" => 1, "BitsPerComponent" => 8, "Columns" => 200 };
        let dict = dictionary! { "ColorSpace" => "DeviceGray", "DecodeParms" => parms };
        let image = lopdf::xobject::PdfImage {
            id: (1, 0),
            width: 200,
            height: 120,
            color_space: Some("DeviceGray".into()),
            filters: Some(vec!["FlateDecode".into()]),
            bits_per_component: Some(8),
            content: &rows,
            origin_dict: &dict,
        };
        let made = pdf_figure(&doc, &image).unwrap();
        assert_eq!(made, png(200, 120, 1, &rows));
        let cmyk = dictionary! { "ColorSpace" => "DeviceCMYK" };
        let jpeg = lopdf::xobject::PdfImage {
            color_space: Some("DeviceCMYK".into()),
            filters: Some(vec!["DCTDecode".into()]),
            origin_dict: &cmyk,
            ..image
        };
        assert!(
            pdf_figure(&doc, &jpeg).is_none(),
            "CMYK pictures look wrong in a browser"
        );
    }
}
