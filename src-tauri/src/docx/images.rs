use std::{
    collections::{HashMap, HashSet},
    io::{Cursor, Read, Seek, SeekFrom},
};

use quick_xml::{events::Event, reader::NsReader};
use zip::ZipArchive;

use super::{
    DocxError, DocxLimits, DocxResult,
    package::{inspect_archive_with_size, read_part_limited},
    xml::{
        NamespaceKind, attribute_value, classify_namespace, decode_reference, local_name,
        validate_safe_xml,
    },
};

const DOCUMENT_PART: &str = "word/document.xml";
const DOCUMENT_RELS_PART: &str = "word/_rels/document.xml.rels";
const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TOTAL_OCCURRENCE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_IMAGE_OCCURRENCES: usize = 128;
const MAX_IMAGE_DIMENSION: u32 = 20_000;
const MAX_IMAGE_PIXELS: u64 = 40_000_000;
const MAX_RELATIONSHIP_ID_CHARS: usize = 256;
const MAX_RELATIONSHIP_TARGET_CHARS: usize = 1_024;

/// A safe, decoded occurrence of a raster image in `word/document.xml`.
///
/// `text_char_offset` is the insertion offset among the paragraph's visible
/// text characters (including the single placeholder used for an OMML
/// expression, but excluding image occurrences themselves). Repeated uses of
/// the same relationship remain separate entries in the returned vector.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtractedImageOccurrence {
    pub paragraph_index: usize,
    pub text_char_offset: usize,
    pub relationship_id: String,
    pub original_filename: Option<String>,
    pub mime_type: String,
    pub byte_size: u64,
    pub width_px: u32,
    pub height_px: u32,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ImageMarker {
    paragraph_index: usize,
    text_char_offset: usize,
    relationship_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ImageRelationship {
    target_part: String,
    original_filename: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DecodedImage {
    original_filename: Option<String>,
    mime_type: String,
    width_px: u32,
    height_px: u32,
    bytes: Vec<u8>,
}

/// Reads one bounded source into memory once, validates that exact byte
/// sequence as a DOCX/OPC package, and extracts embedded PNG/JPEG occurrences
/// from the main document part.
pub fn read_images_from_docx<R: Read + Seek>(
    mut source: R,
    limits: &DocxLimits,
) -> DocxResult<Vec<ExtractedImageOccurrence>> {
    let declared_size = source.seek(SeekFrom::End(0))?;
    if declared_size > limits.max_archive_bytes {
        return Err(DocxError::LimitExceeded {
            resource: "DOCX archive".to_owned(),
            limit: limits.max_archive_bytes,
            actual: declared_size,
        });
    }
    source.seek(SeekFrom::Start(0))?;
    let capacity = usize::try_from(declared_size).map_err(|_| DocxError::LimitExceeded {
        resource: "DOCX archive".to_owned(),
        limit: limits.max_archive_bytes,
        actual: declared_size,
    })?;
    let mut package_bytes = Vec::with_capacity(capacity);
    let copied = source
        .take(limits.max_archive_bytes.saturating_add(1))
        .read_to_end(&mut package_bytes)? as u64;
    if copied > limits.max_archive_bytes {
        return Err(DocxError::LimitExceeded {
            resource: "DOCX archive".to_owned(),
            limit: limits.max_archive_bytes,
            actual: copied,
        });
    }
    read_images_from_bounded_bytes(&package_bytes, limits)
}

fn read_images_from_bounded_bytes(
    package_bytes: &[u8],
    limits: &DocxLimits,
) -> DocxResult<Vec<ExtractedImageOccurrence>> {
    let archive_size = u64::try_from(package_bytes.len()).unwrap_or(u64::MAX);
    if archive_size > limits.max_archive_bytes {
        return Err(DocxError::LimitExceeded {
            resource: "DOCX archive".to_owned(),
            limit: limits.max_archive_bytes,
            actual: archive_size,
        });
    }

    let mut archive = ZipArchive::new(Cursor::new(package_bytes))?;
    let inspection = inspect_archive_with_size(&mut archive, Some(archive_size), limits)?;
    inspection.ensure_acceptable()?;

    let document_xml = read_part_limited(&mut archive, DOCUMENT_PART, limits.max_xml_part_bytes)?;
    let markers = parse_image_markers(&document_xml, limits)?;
    let has_document_relationships = inspection
        .parts
        .iter()
        .any(|part| !part.is_directory && part.name == DOCUMENT_RELS_PART);
    if markers.is_empty() && !has_document_relationships {
        return Ok(Vec::new());
    }

    let relationships_xml =
        read_part_limited(&mut archive, DOCUMENT_RELS_PART, limits.max_xml_part_bytes).map_err(
            |_| {
                invalid_relationships(
                    "word/document.xml contains an image, but its relationship part is missing",
                )
            },
        )?;
    let relationships = parse_image_relationships(&relationships_xml, limits)?;
    if markers.is_empty() {
        return Ok(Vec::new());
    }

    let mut decoded_by_part = HashMap::<String, DecodedImage>::new();
    let mut total_occurrence_bytes = 0u64;
    let mut output = Vec::with_capacity(markers.len());
    for marker in markers {
        let relationship = relationships.get(&marker.relationship_id).ok_or_else(|| {
            invalid_relationships(format!(
                "image occurrence references missing relationship {}",
                marker.relationship_id
            ))
        })?;

        if !decoded_by_part.contains_key(&relationship.target_part) {
            let bytes = read_part_limited(&mut archive, &relationship.target_part, MAX_IMAGE_BYTES)
                .map_err(|error| match error {
                    DocxError::LimitExceeded { actual, .. } => DocxError::LimitExceeded {
                        resource: relationship.target_part.clone(),
                        limit: MAX_IMAGE_BYTES,
                        actual,
                    },
                    _ => invalid_relationships(format!(
                        "image relationship {} points to a missing or unreadable part {}",
                        marker.relationship_id, relationship.target_part
                    )),
                })?;
            let (mime_type, width_px, height_px) = inspect_raster_image(&bytes)?;
            decoded_by_part.insert(
                relationship.target_part.clone(),
                DecodedImage {
                    original_filename: relationship.original_filename.clone(),
                    mime_type: mime_type.to_owned(),
                    width_px,
                    height_px,
                    bytes,
                },
            );
        }

        let decoded = decoded_by_part
            .get(&relationship.target_part)
            .expect("decoded image was inserted above");
        let byte_size = u64::try_from(decoded.bytes.len()).unwrap_or(u64::MAX);
        total_occurrence_bytes =
            total_occurrence_bytes
                .checked_add(byte_size)
                .ok_or_else(|| DocxError::LimitExceeded {
                    resource: "DOCX image occurrence bytes".to_owned(),
                    limit: MAX_TOTAL_OCCURRENCE_BYTES,
                    actual: u64::MAX,
                })?;
        if total_occurrence_bytes > MAX_TOTAL_OCCURRENCE_BYTES {
            return Err(DocxError::LimitExceeded {
                resource: "DOCX image occurrence bytes".to_owned(),
                limit: MAX_TOTAL_OCCURRENCE_BYTES,
                actual: total_occurrence_bytes,
            });
        }

        output.push(ExtractedImageOccurrence {
            paragraph_index: marker.paragraph_index,
            text_char_offset: marker.text_char_offset,
            relationship_id: marker.relationship_id,
            original_filename: decoded.original_filename.clone(),
            mime_type: decoded.mime_type.clone(),
            byte_size,
            width_px: decoded.width_px,
            height_px: decoded.height_px,
            bytes: decoded.bytes.clone(),
        });
    }
    Ok(output)
}

fn parse_image_markers(xml: &[u8], limits: &DocxLimits) -> DocxResult<Vec<ImageMarker>> {
    validate_safe_xml(xml, DOCUMENT_PART, limits)?;
    let mut reader = NsReader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut output = Vec::new();
    let mut paragraph_chars: Option<usize> = None;
    let mut paragraph_depth = 0usize;
    let mut next_paragraph_index = 0usize;
    let mut excluded_depth = 0usize;
    let mut instruction_depth = 0usize;
    let mut text_depth = 0usize;
    let mut math_capture_depth = 0usize;
    let mut text_box_depth = 0usize;

    loop {
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|error| DocxError::xml(DOCUMENT_PART, reader.error_position(), error))?;
        match event {
            Event::Start(ref start) => {
                let namespace = {
                    let (namespace, _) = reader.resolver().resolve_element(start.name());
                    classify_namespace(namespace)
                };
                let local = local_name(start);

                if namespace == NamespaceKind::Word && local == b"txbxContent" {
                    text_box_depth += 1;
                    buffer.clear();
                    continue;
                }
                if text_box_depth > 0 {
                    buffer.clear();
                    continue;
                }

                if math_capture_depth > 0 {
                    math_capture_depth += 1;
                } else if namespace == NamespaceKind::Math
                    && (local == b"oMath" || local == b"oMathPara")
                {
                    if let Some(chars) = paragraph_chars.as_mut() {
                        *chars = chars.saturating_add(1);
                    }
                    math_capture_depth = 1;
                } else {
                    if namespace == NamespaceKind::Word {
                        match local.as_slice() {
                            b"p" => {
                                if paragraph_depth != 0 || paragraph_chars.is_some() {
                                    return Err(DocxError::xml(
                                        DOCUMENT_PART,
                                        reader.buffer_position(),
                                        "nested w:p elements outside text boxes are not supported",
                                    ));
                                }
                                paragraph_chars = Some(0);
                                paragraph_depth = 1;
                            }
                            b"del" | b"moveFrom" => excluded_depth += 1,
                            b"instrText" => instruction_depth += 1,
                            b"t" => text_depth += 1,
                            _ => {}
                        }
                    }
                    if namespace == NamespaceKind::Drawing && local == b"blip" {
                        push_image_marker(
                            &reader,
                            start,
                            paragraph_chars,
                            next_paragraph_index,
                            excluded_depth,
                            instruction_depth,
                            &mut output,
                        )?;
                    }
                }
            }
            Event::Empty(ref start) => {
                let namespace = {
                    let (namespace, _) = reader.resolver().resolve_element(start.name());
                    classify_namespace(namespace)
                };
                let local = local_name(start);
                if text_box_depth > 0 {
                    buffer.clear();
                    continue;
                }
                if math_capture_depth == 0
                    && namespace == NamespaceKind::Math
                    && (local == b"oMath" || local == b"oMathPara")
                {
                    if let Some(chars) = paragraph_chars.as_mut() {
                        *chars = chars.saturating_add(1);
                    }
                } else if math_capture_depth == 0 {
                    if namespace == NamespaceKind::Drawing && local == b"blip" {
                        push_image_marker(
                            &reader,
                            start,
                            paragraph_chars,
                            next_paragraph_index,
                            excluded_depth,
                            instruction_depth,
                            &mut output,
                        )?;
                    } else if namespace == NamespaceKind::Word {
                        match local.as_slice() {
                            b"p" => {
                                if paragraph_depth != 0 || paragraph_chars.is_some() {
                                    return Err(DocxError::xml(
                                        DOCUMENT_PART,
                                        reader.buffer_position(),
                                        "nested w:p elements outside text boxes are not supported",
                                    ));
                                }
                                next_paragraph_index += 1;
                            }
                            b"tab" | b"br" | b"cr" | b"noBreakHyphen" | b"softHyphen"
                                if excluded_depth == 0 && instruction_depth == 0 =>
                            {
                                if let Some(chars) = paragraph_chars.as_mut() {
                                    *chars = chars.saturating_add(1);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            Event::End(ref end) => {
                if math_capture_depth > 0 {
                    math_capture_depth = math_capture_depth.saturating_sub(1);
                } else {
                    let namespace = {
                        let (namespace, _) = reader.resolver().resolve_element(end.name());
                        classify_namespace(namespace)
                    };
                    let local = end.local_name().as_ref().to_vec();
                    if text_box_depth > 0 {
                        if namespace == NamespaceKind::Word && local == b"txbxContent" {
                            text_box_depth = text_box_depth.saturating_sub(1);
                        }
                        buffer.clear();
                        continue;
                    }
                    if namespace == NamespaceKind::Word {
                        match local.as_slice() {
                            b"t" => text_depth = text_depth.saturating_sub(1),
                            b"instrText" => instruction_depth = instruction_depth.saturating_sub(1),
                            b"del" | b"moveFrom" => {
                                excluded_depth = excluded_depth.saturating_sub(1)
                            }
                            b"p" => {
                                if paragraph_depth == 0 {
                                    return Err(DocxError::xml(
                                        DOCUMENT_PART,
                                        reader.buffer_position(),
                                        "w:p closing tag has no matching opening tag",
                                    ));
                                }
                                paragraph_depth -= 1;
                                if paragraph_chars.take().is_none() {
                                    return Err(DocxError::xml(
                                        DOCUMENT_PART,
                                        reader.buffer_position(),
                                        "w:p closing tag has no matching opening tag",
                                    ));
                                }
                                next_paragraph_index += 1;
                            }
                            _ => {}
                        }
                    }
                }
            }
            Event::Text(ref text)
                if math_capture_depth == 0
                    && text_box_depth == 0
                    && text_depth > 0
                    && excluded_depth == 0
                    && instruction_depth == 0 =>
            {
                if let Some(chars) = paragraph_chars.as_mut() {
                    let decoded = text.xml10_content().map_err(|error| {
                        DocxError::xml(DOCUMENT_PART, reader.error_position(), error)
                    })?;
                    *chars = chars.saturating_add(decoded.chars().count());
                }
            }
            Event::CData(ref text)
                if math_capture_depth == 0
                    && text_box_depth == 0
                    && text_depth > 0
                    && excluded_depth == 0
                    && instruction_depth == 0 =>
            {
                if let Some(chars) = paragraph_chars.as_mut() {
                    let decoded = text.xml10_content().map_err(|error| {
                        DocxError::xml(DOCUMENT_PART, reader.error_position(), error)
                    })?;
                    *chars = chars.saturating_add(decoded.chars().count());
                }
            }
            Event::GeneralRef(ref reference)
                if math_capture_depth == 0
                    && text_box_depth == 0
                    && text_depth > 0
                    && excluded_depth == 0
                    && instruction_depth == 0 =>
            {
                let _ = decode_reference(reference, DOCUMENT_PART, reader.buffer_position())?;
                if let Some(chars) = paragraph_chars.as_mut() {
                    *chars = chars.saturating_add(1);
                }
            }
            Event::DocType(_) => {
                return Err(DocxError::xml(
                    DOCUMENT_PART,
                    reader.buffer_position(),
                    "DOCTYPE declarations are not allowed",
                ));
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }

    if paragraph_chars.is_some() || paragraph_depth != 0 || text_box_depth != 0 {
        return Err(DocxError::xml(
            DOCUMENT_PART,
            reader.buffer_position(),
            "unterminated w:p element",
        ));
    }
    Ok(output)
}

fn push_image_marker(
    reader: &NsReader<&[u8]>,
    start: &quick_xml::events::BytesStart<'_>,
    paragraph_chars: Option<usize>,
    paragraph_index: usize,
    excluded_depth: usize,
    instruction_depth: usize,
    output: &mut Vec<ImageMarker>,
) -> DocxResult<()> {
    if excluded_depth > 0 || instruction_depth > 0 {
        return Ok(());
    }
    let link = attribute_value(
        reader,
        start,
        b"link",
        Some(NamespaceKind::OfficeRelationships),
        DOCUMENT_PART,
    )?;
    if link.is_some() {
        return Err(invalid_relationships(
            "linked/external DrawingML images are not allowed",
        ));
    }
    let relationship_id = attribute_value(
        reader,
        start,
        b"embed",
        Some(NamespaceKind::OfficeRelationships),
        DOCUMENT_PART,
    )?
    .ok_or_else(|| invalid_relationships("DrawingML a:blip is missing r:embed"))?;
    validate_relationship_id(&relationship_id)?;
    let text_char_offset = paragraph_chars.ok_or_else(|| {
        DocxError::xml(
            DOCUMENT_PART,
            reader.buffer_position(),
            "DrawingML image occurs outside a Word paragraph",
        )
    })?;
    if output.len() == MAX_IMAGE_OCCURRENCES {
        return Err(DocxError::LimitExceeded {
            resource: "DOCX image occurrences".to_owned(),
            limit: MAX_IMAGE_OCCURRENCES as u64,
            actual: (MAX_IMAGE_OCCURRENCES + 1) as u64,
        });
    }
    output.push(ImageMarker {
        paragraph_index,
        text_char_offset,
        relationship_id,
    });
    Ok(())
}

fn parse_image_relationships(
    xml: &[u8],
    limits: &DocxLimits,
) -> DocxResult<HashMap<String, ImageRelationship>> {
    validate_safe_xml(xml, DOCUMENT_RELS_PART, limits)?;
    let mut reader = NsReader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut all_ids = HashSet::new();
    let mut images = HashMap::new();
    loop {
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|error| DocxError::xml(DOCUMENT_RELS_PART, reader.error_position(), error))?;
        if let Event::Start(ref start) | Event::Empty(ref start) = event {
            let namespace = {
                let (namespace, _) = reader.resolver().resolve_element(start.name());
                classify_namespace(namespace)
            };
            if namespace == NamespaceKind::Relationships && local_name(start) == b"Relationship" {
                let id = attribute_value(&reader, start, b"Id", None, DOCUMENT_RELS_PART)?
                    .ok_or_else(|| invalid_relationships("Relationship is missing Id"))?;
                validate_relationship_id(&id)?;
                if !all_ids.insert(id.clone()) {
                    return Err(invalid_relationships(format!(
                        "duplicate relationship Id {id}"
                    )));
                }
                let relationship_type =
                    attribute_value(&reader, start, b"Type", None, DOCUMENT_RELS_PART)?
                        .ok_or_else(|| invalid_relationships("Relationship is missing Type"))?;
                let target = attribute_value(&reader, start, b"Target", None, DOCUMENT_RELS_PART)?
                    .ok_or_else(|| invalid_relationships("Relationship is missing Target"))?;
                let target_mode =
                    attribute_value(&reader, start, b"TargetMode", None, DOCUMENT_RELS_PART)?;
                if target_mode
                    .as_deref()
                    .is_some_and(|mode| !mode.eq_ignore_ascii_case("internal"))
                {
                    return Err(invalid_relationships(format!(
                        "relationship {id} uses a non-internal target mode"
                    )));
                }
                if relationship_type.to_ascii_lowercase().ends_with("/image") {
                    let target_part = resolve_image_target(&target)?;
                    let original_filename = target_part
                        .rsplit('/')
                        .next()
                        .filter(|name| !name.is_empty())
                        .map(ToOwned::to_owned);
                    images.insert(
                        id,
                        ImageRelationship {
                            target_part,
                            original_filename,
                        },
                    );
                }
            }
        }
        if matches!(event, Event::Eof) {
            break;
        }
        buffer.clear();
    }
    Ok(images)
}

fn validate_relationship_id(value: &str) -> DocxResult<()> {
    if value.is_empty()
        || value.len() > MAX_RELATIONSHIP_ID_CHARS.saturating_mul(4)
        || value.chars().count() > MAX_RELATIONSHIP_ID_CHARS
        || value.chars().any(char::is_control)
        || value.trim() != value
    {
        return Err(invalid_relationships("relationship Id is invalid"));
    }
    Ok(())
}

fn resolve_image_target(target: &str) -> DocxResult<String> {
    if target.is_empty()
        || target.len() > MAX_RELATIONSHIP_TARGET_CHARS.saturating_mul(4)
        || target.chars().count() > MAX_RELATIONSHIP_TARGET_CHARS
        || target.trim() != target
        || target.chars().any(char::is_control)
        || target
            .chars()
            .any(|character| matches!(character, '\\' | ':' | '?' | '#' | '%'))
        || target.starts_with('/')
    {
        return Err(invalid_relationships("image relationship target is unsafe"));
    }
    let segments = target.split('/').collect::<Vec<_>>();
    if segments.len() < 2
        || segments.first() != Some(&"media")
        || segments
            .iter()
            .any(|segment| segment.is_empty() || *segment == "." || *segment == "..")
    {
        return Err(invalid_relationships(
            "image relationship must stay under word/media",
        ));
    }
    Ok(format!("word/{target}"))
}

pub(crate) fn inspect_raster_image(bytes: &[u8]) -> DocxResult<(&'static str, u32, u32)> {
    let (mime_type, width, height) = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let (width, height) = inspect_png(bytes)?;
        ("image/png", width, height)
    } else if bytes.starts_with(&[0xff, 0xd8]) {
        let (width, height) = inspect_jpeg(bytes)?;
        ("image/jpeg", width, height)
    } else {
        return Err(DocxError::xml(
            "DOCX image",
            0,
            "only PNG and JPEG image bytes are supported",
        ));
    };
    validate_dimensions(width, height)?;
    Ok((mime_type, width, height))
}

fn inspect_png(bytes: &[u8]) -> DocxResult<(u32, u32)> {
    if bytes.len() < 33
        || &bytes[12..16] != b"IHDR"
        || u32::from_be_bytes(bytes[8..12].try_into().expect("four bytes")) != 13
    {
        return Err(invalid_image("PNG has no valid leading IHDR chunk"));
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().expect("four bytes"));
    let height = u32::from_be_bytes(bytes[20..24].try_into().expect("four bytes"));

    let mut cursor = 8usize;
    let mut saw_ihdr = false;
    let mut saw_idat = false;
    let mut saw_iend = false;
    while cursor < bytes.len() {
        if bytes.len().saturating_sub(cursor) < 12 {
            return Err(invalid_image("PNG chunk is truncated"));
        }
        let length =
            u32::from_be_bytes(bytes[cursor..cursor + 4].try_into().expect("four bytes")) as usize;
        let chunk_end = cursor
            .checked_add(12)
            .and_then(|value| value.checked_add(length))
            .ok_or_else(|| invalid_image("PNG chunk length overflows"))?;
        if chunk_end > bytes.len() {
            return Err(invalid_image("PNG chunk exceeds the image bytes"));
        }
        let kind = &bytes[cursor + 4..cursor + 8];
        match kind {
            b"IHDR" => {
                if saw_ihdr || cursor != 8 || length != 13 {
                    return Err(invalid_image("PNG contains an invalid IHDR chunk"));
                }
                saw_ihdr = true;
            }
            b"IDAT" => saw_idat = true,
            b"IEND" => {
                if length != 0 || chunk_end != bytes.len() {
                    return Err(invalid_image("PNG contains an invalid IEND chunk"));
                }
                saw_iend = true;
            }
            _ => {}
        }
        cursor = chunk_end;
    }
    if !saw_ihdr || !saw_idat || !saw_iend {
        return Err(invalid_image("PNG is missing IHDR, IDAT, or IEND"));
    }
    Ok((width, height))
}

fn inspect_jpeg(bytes: &[u8]) -> DocxResult<(u32, u32)> {
    if bytes.len() < 4 || !bytes.ends_with(&[0xff, 0xd9]) {
        return Err(invalid_image("JPEG is missing SOI or EOI"));
    }
    let mut cursor = 2usize;
    let mut dimensions = None;
    while cursor + 1 < bytes.len() {
        if bytes[cursor] != 0xff {
            return Err(invalid_image("JPEG marker stream is malformed"));
        }
        while cursor < bytes.len() && bytes[cursor] == 0xff {
            cursor += 1;
        }
        if cursor >= bytes.len() {
            return Err(invalid_image("JPEG marker is truncated"));
        }
        let marker = bytes[cursor];
        cursor += 1;
        if marker == 0xd9 {
            break;
        }
        if marker == 0xda {
            break;
        }
        if marker == 0x01 || (0xd0..=0xd8).contains(&marker) {
            continue;
        }
        if cursor + 2 > bytes.len() {
            return Err(invalid_image("JPEG segment length is truncated"));
        }
        let segment_length = u16::from_be_bytes([bytes[cursor], bytes[cursor + 1]]) as usize;
        if segment_length < 2 || cursor + segment_length > bytes.len() {
            return Err(invalid_image("JPEG segment length is invalid"));
        }
        if is_start_of_frame(marker) {
            if segment_length < 8 {
                return Err(invalid_image("JPEG SOF segment is too short"));
            }
            let height = u16::from_be_bytes([bytes[cursor + 3], bytes[cursor + 4]]) as u32;
            let width = u16::from_be_bytes([bytes[cursor + 5], bytes[cursor + 6]]) as u32;
            if dimensions.replace((width, height)).is_some() {
                return Err(invalid_image(
                    "JPEG contains multiple SOF dimension segments",
                ));
            }
        }
        cursor += segment_length;
    }
    dimensions.ok_or_else(|| invalid_image("JPEG has no supported SOF dimension segment"))
}

fn is_start_of_frame(marker: u8) -> bool {
    matches!(
        marker,
        0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf
    )
}

fn validate_dimensions(width: u32, height: u32) -> DocxResult<()> {
    let pixels = u64::from(width).saturating_mul(u64::from(height));
    if width == 0
        || height == 0
        || width > MAX_IMAGE_DIMENSION
        || height > MAX_IMAGE_DIMENSION
        || pixels > MAX_IMAGE_PIXELS
    {
        return Err(DocxError::LimitExceeded {
            resource: "DOCX image dimensions".to_owned(),
            limit: MAX_IMAGE_PIXELS,
            actual: pixels,
        });
    }
    Ok(())
}

fn invalid_relationships(message: impl Into<String>) -> DocxError {
    DocxError::xml(DOCUMENT_RELS_PART, 0, message.into())
}

fn invalid_image(message: impl Into<String>) -> DocxError {
    DocxError::xml("DOCX image", 0, message.into())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::docx::package::test_support::{CONTENT_TYPES, ROOT_RELS, package_with_entries};

    const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rImg" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image1.png"/>
</Relationships>"#;

    fn document(blips: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
 xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
 xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
 <w:body><w:p><w:r><w:t>Hello</w:t></w:r>{blips}<w:r><w:t>world</w:t></w:r></w:p></w:body>
</w:document>"#
        )
    }

    fn blip(id: &str) -> String {
        format!(r#"<w:r><w:drawing><a:blip r:embed="{id}"/></w:drawing></w:r>"#)
    }

    fn png(width: u32, height: u32, padding: usize) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        push_png_chunk(
            &mut bytes,
            b"IHDR",
            &[
                width.to_be_bytes().as_slice(),
                height.to_be_bytes().as_slice(),
                &[8, 2, 0, 0, 0],
            ]
            .concat(),
        );
        push_png_chunk(&mut bytes, b"IDAT", &vec![0; padding.max(1)]);
        push_png_chunk(&mut bytes, b"IEND", &[]);
        bytes
    }

    fn push_png_chunk(output: &mut Vec<u8>, kind: &[u8; 4], payload: &[u8]) {
        output.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        output.extend_from_slice(kind);
        output.extend_from_slice(payload);
        output.extend_from_slice(&[0; 4]);
    }

    fn jpeg(width: u16, height: u16) -> Vec<u8> {
        vec![
            0xff,
            0xd8,
            0xff,
            0xc0,
            0x00,
            0x0b,
            0x08,
            (height >> 8) as u8,
            height as u8,
            (width >> 8) as u8,
            width as u8,
            0x01,
            0x01,
            0x11,
            0x00,
            0xff,
            0xd9,
        ]
    }

    fn package(document: &str, rels: &str, image_name: &str, image: &[u8]) -> Vec<u8> {
        package_with_entries(&[
            ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
            ("_rels/.rels", ROOT_RELS.as_bytes()),
            ("word/document.xml", document.as_bytes()),
            ("word/_rels/document.xml.rels", rels.as_bytes()),
            (image_name, image),
        ])
    }

    #[test]
    fn extracts_repeated_png_occurrences_with_text_offsets() {
        let xml = document(&format!("{}{}", blip("rImg"), blip("rImg")));
        let image = png(3, 2, 1);
        let bytes = package(&xml, RELS, "word/media/image1.png", &image);
        let extracted = read_images_from_docx(Cursor::new(bytes), &DocxLimits::default()).unwrap();
        assert_eq!(extracted.len(), 2);
        assert_eq!(extracted[0].paragraph_index, 0);
        assert_eq!(extracted[0].text_char_offset, 5);
        assert_eq!(extracted[1].text_char_offset, 5);
        assert_eq!(extracted[0].relationship_id, "rImg");
        assert_eq!(
            extracted[0].original_filename.as_deref(),
            Some("image1.png")
        );
        assert_eq!(extracted[0].mime_type, "image/png");
        assert_eq!((extracted[0].width_px, extracted[0].height_px), (3, 2));
        assert_eq!(extracted[0].bytes, image);
        assert_eq!(extracted[0].bytes, extracted[1].bytes);
    }

    #[test]
    fn reports_the_second_paragraph_index() {
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
 xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
 xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
 <w:body><w:p><w:r><w:t>first</w:t></w:r></w:p><w:p><w:r><w:t>x</w:t></w:r>{}</w:p></w:body>
</w:document>"#,
            blip("rImg")
        );
        let bytes = package(&xml, RELS, "word/media/image1.png", &png(1, 1, 1));
        let extracted = read_images_from_docx(Cursor::new(bytes), &DocxLimits::default()).unwrap();
        assert_eq!(extracted[0].paragraph_index, 1);
        assert_eq!(extracted[0].text_char_offset, 1);
    }

    #[test]
    fn skips_nested_text_box_images_and_keeps_following_indices_aligned() {
        let xml = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
 xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
 xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
 <w:body>
  <w:p><w:r><w:t>a</w:t></w:r><w:txbxContent><w:p><w:r><w:t>inside</w:t></w:r>{}</w:p></w:txbxContent></w:p>
  <w:p><w:r><w:t>x</w:t></w:r>{}</w:p>
 </w:body>
</w:document>"#,
            blip("rImg"),
            blip("rImg")
        );
        let bytes = package(&xml, RELS, "word/media/image1.png", &png(1, 1, 1));

        let extracted = read_images_from_docx(Cursor::new(bytes), &DocxLimits::default()).unwrap();

        assert_eq!(extracted.len(), 1);
        assert_eq!(extracted[0].paragraph_index, 1);
        assert_eq!(extracted[0].text_char_offset, 1);
    }

    #[test]
    fn extracts_jpeg_dimensions() {
        let xml = document(&blip("rJpeg"));
        let rels = RELS
            .replace("rImg", "rJpeg")
            .replace("image1.png", "photo.jpeg");
        let image = jpeg(640, 480);
        let bytes = package(&xml, &rels, "word/media/photo.jpeg", &image);
        let extracted = read_images_from_docx(Cursor::new(bytes), &DocxLimits::default()).unwrap();
        assert_eq!(extracted[0].mime_type, "image/jpeg");
        assert_eq!((extracted[0].width_px, extracted[0].height_px), (640, 480));
    }

    #[test]
    fn rejects_missing_and_duplicate_relationships() {
        let xml = document(&blip("missing"));
        let bytes = package(&xml, RELS, "word/media/image1.png", &png(1, 1, 1));
        assert!(read_images_from_docx(Cursor::new(bytes), &DocxLimits::default()).is_err());

        let duplicate = RELS.replace(
            "</Relationships>",
            "<Relationship Id=\"rImg\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/image1.png\"/></Relationships>",
        );
        let xml = document(&blip("rImg"));
        let bytes = package(&xml, &duplicate, "word/media/image1.png", &png(1, 1, 1));
        assert!(read_images_from_docx(Cursor::new(bytes), &DocxLimits::default()).is_err());
    }

    #[test]
    fn rejects_external_and_escaping_relationships() {
        let xml = document(&blip("rImg"));
        let external = RELS
            .replace("media/image1.png", "https://example.test/image.png")
            .replace("Target=", "TargetMode=\"External\" Target=");
        let bytes = package(&xml, &external, "word/media/image1.png", &png(1, 1, 1));
        assert!(read_images_from_docx(Cursor::new(bytes), &DocxLimits::default()).is_err());

        let escaping = RELS.replace("media/image1.png", "../image1.png");
        let bytes = package(&xml, &escaping, "word/media/image1.png", &png(1, 1, 1));
        assert!(read_images_from_docx(Cursor::new(bytes), &DocxLimits::default()).is_err());
    }

    #[test]
    fn rejects_non_raster_and_invalid_dimensions() {
        let xml = document(&blip("rImg"));
        let bytes = package(&xml, RELS, "word/media/image1.png", b"not an image");
        assert!(read_images_from_docx(Cursor::new(bytes), &DocxLimits::default()).is_err());

        let bytes = package(&xml, RELS, "word/media/image1.png", &png(0, 1, 1));
        assert!(matches!(
            read_images_from_docx(Cursor::new(bytes), &DocxLimits::default()),
            Err(DocxError::LimitExceeded { .. })
        ));

        let bytes = package(&xml, RELS, "word/media/image1.png", &png(20_001, 1, 1));
        assert!(matches!(
            read_images_from_docx(Cursor::new(bytes), &DocxLimits::default()),
            Err(DocxError::LimitExceeded { .. })
        ));

        let bytes = package(&xml, RELS, "word/media/image1.png", &png(10_000, 5_000, 1));
        assert!(matches!(
            read_images_from_docx(Cursor::new(bytes), &DocxLimits::default()),
            Err(DocxError::LimitExceeded { .. })
        ));
    }

    #[test]
    fn rejects_more_than_128_occurrences() {
        let blips = (0..129).map(|_| blip("rImg")).collect::<String>();
        let xml = document(&blips);
        let bytes = package(&xml, RELS, "word/media/image1.png", &png(1, 1, 1));
        assert!(matches!(
            read_images_from_docx(Cursor::new(bytes), &DocxLimits::default()),
            Err(DocxError::LimitExceeded { .. })
        ));
    }

    #[test]
    fn enforces_per_image_and_total_occurrence_byte_limits() {
        let oversized = png(1, 1, MAX_IMAGE_BYTES as usize);
        let xml = document(&blip("rImg"));
        let bytes = package(&xml, RELS, "word/media/image1.png", &oversized);
        assert!(matches!(
            read_images_from_docx(Cursor::new(bytes), &DocxLimits::default()),
            Err(DocxError::LimitExceeded { .. })
        ));

        let image = png(1, 1, 4 * 1024 * 1024);
        let blips = (0..9).map(|_| blip("rImg")).collect::<String>();
        let bytes = package(&document(&blips), RELS, "word/media/image1.png", &image);
        assert!(matches!(
            read_images_from_docx(Cursor::new(bytes), &DocxLimits::default()),
            Err(DocxError::LimitExceeded { .. })
        ));
    }
}
