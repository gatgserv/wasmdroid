//! Authenticated inline display data, never an image URL or runtime capability.
//! Validation checks bounded PNG/JPEG framing and declared dimensions. It does
//! not decompress pixels, certify decoder safety or establish screenshot truth.
use crate::{EngineError, Result, store::Digest};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};

pub const MAX_ICON_BYTES: usize = 8 * 1024;
pub const MAX_SCREENSHOT_BYTES: usize = 20 * 1024;
pub const MAX_MEDIA_BYTES: usize = 48 * 1024;
pub const MAX_SCREENSHOTS: usize = 2;
pub const MAX_SCREENSHOT_PIXELS: u64 = 720 * 1280;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogImageFormat {
    Png,
    Jpeg,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogImage {
    pub format: CatalogImageFormat,
    pub size_bytes: u32,
    pub digest: Digest,
    pub width: u32,
    pub height: u32,
    pub data_base64: String,
    pub alt: String,
    pub provenance: CatalogMediaProvenance,
}
impl std::fmt::Debug for CatalogImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CatalogImage(<redacted>)")
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogMediaProvenance {
    pub license: String,
    pub attribution: String,
    /// Inert committed report reference, never opened or fetched by validation.
    pub report: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogMedia {
    pub icon: CatalogImage,
    pub screenshots: Vec<CatalogImage>,
}
/// Missing optional fields retain legacy wire shapes; an explicit null is not
/// an alternate spelling that can smuggle a new field into a legacy schema.
pub(crate) fn optional_media<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<CatalogMedia>, D::Error> {
    CatalogMedia::deserialize(deserializer).map(Some)
}
pub(crate) fn optional_review<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error> {
    String::deserialize(deserializer).map(Some)
}

fn invalid() -> EngineError {
    EngineError::Invalid("invalid bounded inline catalog media".into())
}
fn text(value: &str, maximum: usize) -> bool {
    !value.trim().is_empty() && value.len() <= maximum && !value.chars().any(char::is_control)
}
pub(crate) fn review_reference(value: &str) -> bool {
    text(value, 512)
        && value.starts_with("docs/")
        && value.ends_with(".md")
        && value.split('/').all(|part| {
            !matches!(part, "" | "." | "..")
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        })
}

/// Pure bytes/metadata checks only. The signed whole-catalog digest authenticates
/// these bytes; license and Android acceptance remain actual review gates.
pub fn validate(media: &CatalogMedia) -> Result<()> {
    if !(1..=MAX_SCREENSHOTS).contains(&media.screenshots.len()) {
        return Err(invalid());
    }
    let mut total = image(&media.icon, true)?;
    for screenshot in &media.screenshots {
        total = total
            .checked_add(image(screenshot, false)?)
            .ok_or_else(invalid)?;
    }
    if total > MAX_MEDIA_BYTES {
        return Err(invalid());
    }
    Ok(())
}
fn image(image: &CatalogImage, icon: bool) -> Result<usize> {
    let maximum = if icon {
        MAX_ICON_BYTES
    } else {
        MAX_SCREENSHOT_BYTES
    };
    let side = image.width.max(image.height);
    let short = image.width.min(image.height);
    let dimensions_invalid = if icon {
        side > 256
    } else {
        side > 1280
            || short > 720
            || u64::from(image.width) * u64::from(image.height) > MAX_SCREENSHOT_PIXELS
    };
    if image.width == 0
        || image.height == 0
        || dimensions_invalid
        || image.size_bytes == 0
        || image.size_bytes as usize > maximum
        || image.data_base64.len() > maximum.div_ceil(3) * 4
        || !text(&image.alt, 240)
        || !text(&image.provenance.license, 128)
        || !text(&image.provenance.attribution, 256)
        || !review_reference(&image.provenance.report)
    {
        return Err(invalid());
    }
    Digest::parse(image.digest.as_str())?;
    let bytes = STANDARD.decode(&image.data_base64).map_err(|_| invalid())?;
    if bytes.len() != image.size_bytes as usize
        || STANDARD.encode(&bytes) != image.data_base64
        || image.digest != Digest::of(&bytes)
    {
        return Err(invalid());
    }
    let dimensions = match image.format {
        CatalogImageFormat::Png => png(&bytes),
        CatalogImageFormat::Jpeg => jpeg(&bytes),
    }
    .ok_or_else(invalid)?;
    if dimensions != (image.width, image.height) {
        return Err(invalid());
    }
    Ok(bytes.len())
}
fn be32(bytes: &[u8]) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.try_into().ok()?))
}
fn be16(bytes: &[u8]) -> Option<u16> {
    Some(u16::from_be_bytes(bytes.try_into().ok()?))
}

/// Small static PNG subset: 8-bit, noninterlaced; only pixel-critical chunks and
/// transparency. CRCs cover actual framed chunks, not blind byte-pattern scans.
/// Deflate/filter/pixel validity is deliberately deferred to the platform decoder.
fn png(bytes: &[u8]) -> Option<(u32, u32)> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return None;
    }
    let mut position = 8usize;
    let mut dimensions = None;
    let mut color = 0;
    let mut palette = None;
    let mut transparency = false;
    let mut idat = false;
    let mut compressed_bytes = 0usize;
    while position < bytes.len() {
        let length = be32(bytes.get(position..position.checked_add(4)?)?)? as usize;
        let kind_start = position.checked_add(4)?;
        let data_start = position.checked_add(8)?;
        let data_end = data_start.checked_add(length)?;
        let end = data_end.checked_add(4)?;
        let kind = bytes.get(kind_start..data_start)?;
        let data = bytes.get(data_start..data_end)?;
        let expected = be32(bytes.get(data_end..end)?)?;
        if crc32(bytes.get(kind_start..data_end)?) != expected {
            return None;
        }
        match kind {
            b"IHDR" => {
                if dimensions.is_some()
                    || position != 8
                    || length != 13
                    || data[8] != 8
                    || !matches!(data[9], 0 | 2..=4 | 6)
                    || data[10..] != [0, 0, 0]
                {
                    return None;
                }
                dimensions = Some((be32(&data[..4])?, be32(&data[4..8])?));
                color = data[9];
            }
            b"PLTE" => {
                if dimensions.is_none()
                    || idat
                    || transparency
                    || palette.is_some()
                    || matches!(color, 0 | 4)
                    || length == 0
                    || length > 768
                    || !length.is_multiple_of(3)
                {
                    return None;
                }
                palette = Some(length / 3);
            }
            b"tRNS" => {
                if dimensions.is_none() || idat || transparency {
                    return None;
                }
                match color {
                    0 if length == 2 => {}
                    2 if length == 6 => {}
                    3 if length > 0 && length <= palette? => {}
                    _ => return None,
                }
                transparency = true;
            }
            b"IDAT" => {
                if dimensions.is_none() || color == 3 && palette.is_none() {
                    return None;
                }
                idat = true;
                compressed_bytes = compressed_bytes.checked_add(length)?;
            }
            b"IEND" => {
                return if dimensions.is_some()
                    && idat
                    && compressed_bytes > 0
                    && length == 0
                    && end == bytes.len()
                {
                    dimensions
                } else {
                    None
                };
            }
            // Unknown ancillary chunks, profiles, textual metadata and APNG
            // control/frame chunks are rejected by their parsed chunk type.
            _ => return None,
        }
        position = end;
    }
    None
}
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 == 0 { 0 } else { 0xedb8_8320 };
        }
    }
    !crc
}

/// Baseline 8-bit sequential JPEG marker envelope. Permits only pixel tables,
/// scan/restart data and a thumbnail-free JFIF APP0; EXIF/ICC/comments/other APP
/// payloads are rejected through parsed marker boundaries. No entropy decoding.
fn jpeg(bytes: &[u8]) -> Option<(u32, u32)> {
    if !bytes.starts_with(&[0xff, 0xd8]) {
        return None;
    }
    let mut position = 2usize;
    let mut dimensions = None;
    let mut components = Vec::new();
    let mut scanned = Vec::new();
    let mut quantization = false;
    let mut huffman = false;
    let mut jfif = false;
    while position < bytes.len() {
        let outer_start = position;
        if *bytes.get(position)? != 0xff {
            return None;
        }
        while *bytes.get(position)? == 0xff {
            position += 1;
        }
        let marker = *bytes.get(position)?;
        position += 1;
        if marker == 0xd9 {
            return if position == bytes.len()
                && dimensions.is_some()
                && !components.is_empty()
                && scanned.len() == components.len()
                && quantization
                && huffman
            {
                dimensions
            } else {
                None
            };
        }
        // Every admitted nonterminal outer marker has a bounded length.
        if !matches!(marker, 0xc0 | 0xc4 | 0xda | 0xdb | 0xdd | 0xe0) {
            return None;
        }
        let length = usize::from(be16(bytes.get(position..position.checked_add(2)?)?)?);
        if length < 2 {
            return None;
        }
        let end = position.checked_add(length)?;
        let data = bytes.get(position + 2..end)?;
        position = end;
        match marker {
            0xc0 => {
                if dimensions.is_some()
                    || data.len() < 6
                    || data[0] != 8
                    || !matches!(data[5], 1 | 3)
                    || data.len() != 6 + usize::from(data[5]) * 3
                {
                    return None;
                }
                dimensions = Some((u32::from(be16(&data[3..5])?), u32::from(be16(&data[1..3])?)));
                for component in data[6..].chunks_exact(3) {
                    if components.contains(&component[0])
                        || !(1..=4).contains(&(component[1] >> 4))
                        || !(1..=4).contains(&(component[1] & 15))
                        || component[2] > 3
                    {
                        return None;
                    }
                    components.push(component[0]);
                }
            }
            0xc4 => {
                if data.is_empty() {
                    return None;
                }
                huffman = true;
            }
            0xdb => {
                if data.is_empty() {
                    return None;
                }
                quantization = true;
            }
            0xdd => {
                if data.len() != 2 {
                    return None;
                }
            }
            0xe0 => {
                if outer_start != 2
                    || jfif
                    || data.len() != 14
                    || !data.starts_with(b"JFIF\0")
                    || data[5] != 1
                    || data[6] > 2
                    || data[7] > 2
                    || data[12..] != [0, 0]
                {
                    return None;
                }
                jfif = true;
            }
            0xda => {
                if dimensions.is_none()
                    || data.is_empty()
                    || data[0] == 0
                    || data[0] > 3
                    || data.len() != 4 + usize::from(data[0]) * 2
                    || data[data.len() - 3..] != [0, 63, 0]
                {
                    return None;
                }
                for component in data[1..data.len() - 3].chunks_exact(2) {
                    if !components.contains(&component[0])
                        || scanned.contains(&component[0])
                        || component[1] >> 4 > 3
                        || component[1] & 15 > 3
                    {
                        return None;
                    }
                    scanned.push(component[0]);
                }
                let entropy_start = position;
                loop {
                    let byte = *bytes.get(position)?;
                    if byte != 0xff {
                        position += 1;
                        continue;
                    }
                    let marker_start = position;
                    while *bytes.get(position)? == 0xff {
                        position += 1;
                    }
                    match *bytes.get(position)? {
                        0x00 | 0xd0..=0xd7 => position += 1,
                        _ => {
                            position = marker_start;
                            break;
                        }
                    }
                }
                if position == entropy_start {
                    return None;
                }
            }
            _ => return None,
        }
    }
    None
}
