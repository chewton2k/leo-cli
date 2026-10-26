use std::io::{Read, Write};

use image::ImageEncoder;
use leo_core::attachments;

use super::png;

pub(crate) const MOST_PIXELS: u64 = 64_000_000;
const KEPT_PIXELS: u64 = 16_000_000;
const MOST_SIDE: u32 = 32_768;
const READING_SIDE: u32 = 4_000;
const DECODE_BYTES: u64 = 192 * 1024 * 1024;

struct Bytes(Vec<u8>);

impl Write for Bytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > attachments::MOST_BYTES.saturating_sub(self.0.len()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "picture exceeds the encoded size limit",
            ));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn reading_copy(bytes: Vec<u8>, width: u32, height: u32) -> Option<Vec<u8>> {
    if u64::from(width) * u64::from(height) <= KEPT_PIXELS {
        return Some(bytes);
    }
    let format = image::guess_format(&bytes).ok()?;
    let dimensions = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format)
        .into_dimensions()
        .ok()?;
    if dimensions != (width, height) {
        return None;
    }
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MOST_SIDE);
    limits.max_image_height = Some(MOST_SIDE);
    limits.max_alloc = Some(DECODE_BYTES);
    reader.limits(limits);
    let decoded = reader.decode().ok()?;
    let resized = decoded.thumbnail(READING_SIDE, READING_SIDE);
    drop(decoded);
    let mut output = Bytes(Vec::new());
    image::codecs::png::PngEncoder::new(&mut output)
        .write_image(
            resized.as_bytes(),
            resized.width(),
            resized.height(),
            resized.color().into(),
        )
        .ok()?;
    Some(output.0)
}

fn plain_rows(content: &[u8], width: u32, height: u32, channels: usize) -> Option<Vec<u8>> {
    let mut decoder = flate2::read::ZlibDecoder::new(content);
    let mut encoder =
        flate2::write::ZlibEncoder::new(Bytes(Vec::new()), flate2::Compression::default());
    let row_bytes = usize::try_from(width).ok()?.checked_mul(channels)?;
    let mut row = vec![0; row_bytes];
    for _ in 0..height {
        decoder.read_exact(&mut row).ok()?;
        encoder.write_all(&[0]).ok()?;
        encoder.write_all(&row).ok()?;
    }
    if decoder.read(&mut [0]).ok()? != 0 {
        return None;
    }
    let idat = encoder.finish().ok()?.0;
    Some(png(width, height, channels as u8, &idat))
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

pub(crate) fn figure(doc: &lopdf::Document, image: &lopdf::xobject::PdfImage) -> Option<Vec<u8>> {
    let filters = image.filters.as_deref().unwrap_or(&[]);
    let (width, height) = (
        u32::try_from(image.width).ok()?,
        u32::try_from(image.height).ok()?,
    );
    if width == 0
        || height == 0
        || u64::from(width) * u64::from(height) > MOST_PIXELS
        || width > MOST_SIDE
        || height > MOST_SIDE
        || image.content.len() > attachments::MOST_BYTES
    {
        return None;
    }
    if filters == ["DCTDecode"] {
        if attachments::dimensions(image.content).is_some_and(|actual| actual != (width, height)) {
            return None;
        }
        let shown = matches!(
            image.color_space.as_deref(),
            None | Some("DeviceRGB" | "DeviceGray" | "CalRGB" | "CalGray")
        ) || channels_of(doc, image.origin_dict).is_some();
        return shown
            .then_some(())
            .and_then(|()| reading_copy(image.content.to_vec(), width, height));
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
        return fits
            .then(|| png(width, height, channels as u8, image.content))
            .and_then(|bytes| reading_copy(bytes, width, height));
    }
    if predictor != 1 {
        return None;
    }
    reading_copy(
        plain_rows(image.content, width, height, channels)?,
        width,
        height,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    #[test]
    fn unsafe_pdf_images_are_refused_before_decoding() {
        let doc = lopdf::Document::with_version("1.5");
        let dict = dictionary! { "ColorSpace" => "DeviceGray" };
        for (width, height) in [
            (0, 400),
            (-1, 400),
            (i64::MAX, i64::MAX),
            (9000, 9000),
            (40000, 300),
        ] {
            let image = lopdf::xobject::PdfImage {
                id: (1, 0),
                width,
                height,
                color_space: Some("DeviceGray".into()),
                filters: Some(vec!["DCTDecode".into()]),
                bits_per_component: Some(8),
                content: &[0xff, 0xd8, 0xff, 1],
                origin_dict: &dict,
            };
            assert!(figure(&doc, &image).is_none());
        }
        let encoded = png(
            2,
            2,
            1,
            &super::super::filtered_rows(&[1; 4], 2, 2, 1).unwrap(),
        );
        assert!(reading_copy(encoded, 5000, 3600).is_none());
    }

    #[test]
    fn raw_pdf_rows_must_match_the_declared_dimensions() {
        for size in [99, 100, 101] {
            let mut encoded =
                flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            encoded.write_all(&vec![120; size]).unwrap();
            let made = plain_rows(&encoded.finish().unwrap(), 10, 10, 1);
            assert_eq!(made.is_some(), size == 100);
        }
    }

    #[test]
    fn encoded_pdf_image_output_has_a_hard_size_limit() {
        let mut output = Bytes(vec![0; attachments::MOST_BYTES - 1]);
        assert!(output.write_all(&[1, 2]).is_err());
        assert_eq!(output.0.len(), attachments::MOST_BYTES - 1);
        assert!(output.write_all(&[1]).is_ok());
        assert!(output.write_all(&[2]).is_err());
        assert_eq!(output.0.len(), attachments::MOST_BYTES);
    }
}
