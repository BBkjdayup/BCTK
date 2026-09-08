//! Safe, bounded MathType MTEF 5 decoding.
//!
//! MathType equations embedded in legacy Word OLE objects contain an
//! `Equation Native` stream.  The stream starts with a 28-byte OLE header and
//! then contains MTEF.  We only read that stream; the OLE object is never
//! activated or executed.

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
const EQUATION_NATIVE_HEADER_BYTES: usize = 28;
const MAX_EQUATION_NATIVE_BYTES: usize = 1024 * 1024;
const MAX_TOTAL_MATHTYPE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_MATHTYPE_OCCURRENCES: usize = 512;
const MAX_MTEF_RECORDS: usize = 16_384;
const MAX_MTEF_DEPTH: usize = 128;
const MAX_ZERO_TERMINATED_BYTES: usize = 4_096;
const MAX_RELATIONSHIP_ID_CHARS: usize = 256;
const MAX_RELATIONSHIP_TARGET_CHARS: usize = 1_024;

const OPT_NUDGE: u8 = 0x08;
const OPT_CHAR_EMBELL: u8 = 0x01;
const OPT_CHAR_ENC_CHAR_8: u8 = 0x04;
const OPT_CHAR_ENC_CHAR_16: u8 = 0x10;
const OPT_CHAR_ENC_NO_MTCODE: u8 = 0x20;
const OPT_LINE_NULL: u8 = 0x01;
const OPT_LINE_RULER: u8 = 0x02;
const OPT_LINE_SPACING: u8 = 0x04;
const OPT_COLOR_CMYK: u8 = 0x01;
const OPT_COLOR_NAME: u8 = 0x04;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MathTypeConversion {
    pub latex: String,
    pub product_version: u8,
    pub product_subversion: u8,
    pub unsupported_features: Vec<String>,
}

/// One MathType equation occurrence converted into the editor's LaTeX model.
///
/// The original embedded object is not returned, activated or persisted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtractedMathTypeOccurrence {
    pub paragraph_index: usize,
    pub text_char_offset: usize,
    pub relationship_id: String,
    pub latex: String,
    pub product_version: u8,
    pub product_subversion: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MathTypeMarker {
    paragraph_index: usize,
    text_char_offset: usize,
    relationship_id: String,
}

/// Safely extracts MathType MTEF 5 objects from a DOCX and converts them to
/// editable LaTeX. Callers must opt in with `allow_mathtype_ole`; all other
/// embedded object types remain rejected.
pub fn read_mathtype_from_docx<R: Read + Seek>(
    mut source: R,
    limits: &DocxLimits,
) -> DocxResult<Vec<ExtractedMathTypeOccurrence>> {
    if !limits.allow_mathtype_ole {
        return Err(invalid_mathtype(
            "MathType extraction was requested without the Word-import safety policy",
        ));
    }
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

    let archive_size = u64::try_from(package_bytes.len()).unwrap_or(u64::MAX);
    let mut archive = ZipArchive::new(Cursor::new(&package_bytes))?;
    let inspection = inspect_archive_with_size(&mut archive, Some(archive_size), limits)?;
    inspection.ensure_acceptable()?;

    let document_xml = read_part_limited(&mut archive, DOCUMENT_PART, limits.max_xml_part_bytes)?;
    let markers = parse_mathtype_markers(&document_xml, limits)?;
    let embedding_parts = inspection
        .parts
        .iter()
        .filter(|part| {
            !part.is_directory
                && part
                    .name
                    .to_ascii_lowercase()
                    .starts_with("word/embeddings/")
        })
        .map(|part| part.name.clone())
        .collect::<HashSet<_>>();

    let relationships_xml =
        read_part_limited(&mut archive, DOCUMENT_RELS_PART, limits.max_xml_part_bytes).map_err(
            |_| invalid_mathtype("MathType objects require word/_rels/document.xml.rels"),
        )?;
    let relationships = parse_mathtype_relationships(&relationships_xml, limits)?;
    let marker_ids = markers
        .iter()
        .map(|marker| marker.relationship_id.as_str())
        .collect::<HashSet<_>>();
    let relationship_ids = relationships
        .keys()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    if marker_ids != relationship_ids {
        return Err(invalid_mathtype(
            "every embedded OLE relationship must be a visible MathType equation occurrence",
        ));
    }
    let relationship_parts = relationships.values().cloned().collect::<HashSet<_>>();
    if embedding_parts != relationship_parts {
        return Err(invalid_mathtype(
            "every embedded object part must be referenced by a MathType equation",
        ));
    }
    if markers.is_empty() {
        return Ok(Vec::new());
    }

    let mut converted_by_part = HashMap::<String, MathTypeConversion>::new();
    let mut total_bytes = 0u64;
    let mut output = Vec::with_capacity(markers.len());
    for marker in markers {
        let target_part = relationships
            .get(&marker.relationship_id)
            .ok_or_else(|| invalid_mathtype("MathType relationship disappeared during parsing"))?;
        if !converted_by_part.contains_key(target_part) {
            let ole_bytes =
                read_part_limited(&mut archive, target_part, MAX_EQUATION_NATIVE_BYTES as u64)
                    .map_err(|error| match error {
                        DocxError::LimitExceeded { actual, .. } => DocxError::LimitExceeded {
                            resource: target_part.clone(),
                            limit: MAX_EQUATION_NATIVE_BYTES as u64,
                            actual,
                        },
                        _ => invalid_mathtype(format!(
                            "MathType relationship {} points to a missing or unreadable object",
                            marker.relationship_id
                        )),
                    })?;
            total_bytes = total_bytes
                .checked_add(u64::try_from(ole_bytes.len()).unwrap_or(u64::MAX))
                .ok_or_else(|| DocxError::LimitExceeded {
                    resource: "DOCX MathType object bytes".to_owned(),
                    limit: MAX_TOTAL_MATHTYPE_BYTES,
                    actual: u64::MAX,
                })?;
            if total_bytes > MAX_TOTAL_MATHTYPE_BYTES {
                return Err(DocxError::LimitExceeded {
                    resource: "DOCX MathType object bytes".to_owned(),
                    limit: MAX_TOTAL_MATHTYPE_BYTES,
                    actual: total_bytes,
                });
            }
            let conversion = convert_equation_native_to_latex(&ole_bytes)?;
            if !conversion.unsupported_features.is_empty() {
                return Err(invalid_mathtype(format!(
                    "MathType equation {} contains unsupported constructs: {}",
                    marker.relationship_id,
                    conversion.unsupported_features.join(", ")
                )));
            }
            converted_by_part.insert(target_part.clone(), conversion);
        }
        let conversion = converted_by_part
            .get(target_part)
            .expect("converted MathType object was inserted above");
        output.push(ExtractedMathTypeOccurrence {
            paragraph_index: marker.paragraph_index,
            text_char_offset: marker.text_char_offset,
            relationship_id: marker.relationship_id,
            latex: conversion.latex.clone(),
            product_version: conversion.product_version,
            product_subversion: conversion.product_subversion,
        });
    }
    Ok(output)
}

fn parse_mathtype_markers(xml: &[u8], limits: &DocxLimits) -> DocxResult<Vec<MathTypeMarker>> {
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
    let mut math_depth = 0usize;
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
                } else if text_box_depth > 0 {
                } else if math_depth > 0 {
                    math_depth += 1;
                } else if namespace == NamespaceKind::Math
                    && (local == b"oMath" || local == b"oMathPara")
                {
                    if excluded_depth == 0
                        && instruction_depth == 0
                        && let Some(chars) = paragraph_chars.as_mut()
                    {
                        *chars = chars.saturating_add(1);
                    }
                    math_depth = 1;
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
                    if namespace == NamespaceKind::Office && local == b"OLEObject" {
                        push_mathtype_marker(
                            &reader,
                            start,
                            paragraph_chars.as_mut(),
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
                } else if math_depth == 0
                    && namespace == NamespaceKind::Math
                    && (local == b"oMath" || local == b"oMathPara")
                {
                    if excluded_depth == 0
                        && instruction_depth == 0
                        && let Some(chars) = paragraph_chars.as_mut()
                    {
                        *chars = chars.saturating_add(1);
                    }
                } else if math_depth == 0 {
                    if namespace == NamespaceKind::Office && local == b"OLEObject" {
                        push_mathtype_marker(
                            &reader,
                            start,
                            paragraph_chars.as_mut(),
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
                if math_depth > 0 {
                    math_depth = math_depth.saturating_sub(1);
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
                    } else if namespace == NamespaceKind::Word {
                        match local.as_slice() {
                            b"t" => text_depth = text_depth.saturating_sub(1),
                            b"instrText" => instruction_depth = instruction_depth.saturating_sub(1),
                            b"del" | b"moveFrom" => {
                                excluded_depth = excluded_depth.saturating_sub(1)
                            }
                            b"p" => {
                                if paragraph_depth == 0 || paragraph_chars.take().is_none() {
                                    return Err(DocxError::xml(
                                        DOCUMENT_PART,
                                        reader.buffer_position(),
                                        "w:p closing tag has no matching opening tag",
                                    ));
                                }
                                paragraph_depth -= 1;
                                next_paragraph_index += 1;
                            }
                            _ => {}
                        }
                    }
                }
            }
            Event::Text(ref text)
                if math_depth == 0
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
                if math_depth == 0
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
                if math_depth == 0
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

fn push_mathtype_marker(
    reader: &NsReader<&[u8]>,
    start: &quick_xml::events::BytesStart<'_>,
    paragraph_chars: Option<&mut usize>,
    paragraph_index: usize,
    excluded_depth: usize,
    instruction_depth: usize,
    output: &mut Vec<MathTypeMarker>,
) -> DocxResult<()> {
    if excluded_depth > 0 || instruction_depth > 0 {
        return Ok(());
    }
    let object_type =
        attribute_value(reader, start, b"Type", None, DOCUMENT_PART)?.unwrap_or_default();
    if !object_type.eq_ignore_ascii_case("Embed") {
        return Err(invalid_mathtype(
            "linked or non-embedded OLE objects are not allowed",
        ));
    }
    let prog_id =
        attribute_value(reader, start, b"ProgID", None, DOCUMENT_PART)?.unwrap_or_default();
    if !prog_id.to_ascii_lowercase().starts_with("equation.") {
        return Err(invalid_mathtype(format!(
            "embedded OLE object {prog_id:?} is not a MathType equation"
        )));
    }
    let relationship_id = attribute_value(
        reader,
        start,
        b"id",
        Some(NamespaceKind::OfficeRelationships),
        DOCUMENT_PART,
    )?
    .ok_or_else(|| invalid_mathtype("MathType OLEObject is missing r:id"))?;
    validate_relationship_id(&relationship_id)?;
    let chars = paragraph_chars
        .ok_or_else(|| invalid_mathtype("MathType OLEObject occurs outside a Word paragraph"))?;
    if output.len() == MAX_MATHTYPE_OCCURRENCES {
        return Err(DocxError::LimitExceeded {
            resource: "DOCX MathType occurrences".to_owned(),
            limit: MAX_MATHTYPE_OCCURRENCES as u64,
            actual: (MAX_MATHTYPE_OCCURRENCES + 1) as u64,
        });
    }
    let text_char_offset = *chars;
    *chars = chars.saturating_add(1);
    output.push(MathTypeMarker {
        paragraph_index,
        text_char_offset,
        relationship_id,
    });
    Ok(())
}

fn parse_mathtype_relationships(
    xml: &[u8],
    limits: &DocxLimits,
) -> DocxResult<HashMap<String, String>> {
    validate_safe_xml(xml, DOCUMENT_RELS_PART, limits)?;
    let mut reader = NsReader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut all_ids = HashSet::new();
    let mut equations = HashMap::new();
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
                    .ok_or_else(|| invalid_mathtype("Relationship is missing Id"))?;
                validate_relationship_id(&id)?;
                if !all_ids.insert(id.clone()) {
                    return Err(invalid_mathtype(format!("duplicate relationship Id {id}")));
                }
                let relationship_type =
                    attribute_value(&reader, start, b"Type", None, DOCUMENT_RELS_PART)?
                        .ok_or_else(|| invalid_mathtype("Relationship is missing Type"))?;
                if !relationship_type
                    .to_ascii_lowercase()
                    .ends_with("/oleobject")
                {
                    buffer.clear();
                    continue;
                }
                let target = attribute_value(&reader, start, b"Target", None, DOCUMENT_RELS_PART)?
                    .ok_or_else(|| invalid_mathtype("OLE relationship is missing Target"))?;
                let target_mode =
                    attribute_value(&reader, start, b"TargetMode", None, DOCUMENT_RELS_PART)?;
                if target_mode
                    .as_deref()
                    .is_some_and(|mode| !mode.eq_ignore_ascii_case("internal"))
                {
                    return Err(invalid_mathtype(
                        "external MathType relationship targets are not allowed",
                    ));
                }
                equations.insert(id, resolve_mathtype_target(&target)?);
            }
        }
        if matches!(event, Event::Eof) {
            break;
        }
        buffer.clear();
    }
    Ok(equations)
}

fn validate_relationship_id(value: &str) -> DocxResult<()> {
    if value.is_empty()
        || value.len() > MAX_RELATIONSHIP_ID_CHARS.saturating_mul(4)
        || value.chars().count() > MAX_RELATIONSHIP_ID_CHARS
        || value.chars().any(char::is_control)
        || value.trim() != value
    {
        return Err(invalid_mathtype("MathType relationship Id is invalid"));
    }
    Ok(())
}

fn resolve_mathtype_target(target: &str) -> DocxResult<String> {
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
        return Err(invalid_mathtype("MathType relationship target is unsafe"));
    }
    let segments = target.split('/').collect::<Vec<_>>();
    if segments.len() < 2
        || segments.first() != Some(&"embeddings")
        || segments
            .iter()
            .any(|segment| segment.is_empty() || *segment == "." || *segment == "..")
    {
        return Err(invalid_mathtype(
            "MathType relationship must stay under word/embeddings",
        ));
    }
    Ok(format!("word/{target}"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum MtefNode {
    Line(Vec<MtefNode>),
    Char {
        mt_code: Option<u16>,
        font_position: Option<u16>,
        embellishments: Vec<u8>,
    },
    Template {
        selector: u8,
        variation: u16,
        children: Vec<MtefNode>,
    },
    Pile(Vec<MtefNode>),
    Matrix {
        rows: u8,
        cols: u8,
        cells: Vec<MtefNode>,
    },
}

#[derive(Debug)]
struct ParsedMtef {
    product_version: u8,
    product_subversion: u8,
    nodes: Vec<MtefNode>,
}

pub(crate) fn convert_equation_native_to_latex(ole_bytes: &[u8]) -> DocxResult<MathTypeConversion> {
    if ole_bytes.len() > MAX_EQUATION_NATIVE_BYTES {
        return Err(invalid_mathtype(format!(
            "MathType OLE object is larger than the {} byte safety limit",
            MAX_EQUATION_NATIVE_BYTES
        )));
    }
    // WPS/MathType commonly leaves unused MiniFAT entries in embedded objects.
    // The regular parser tolerates that producer quirk; all bytes and stream
    // reads remain bounded below and no OLE content is activated.
    let mut compound = cfb::CompoundFile::open(Cursor::new(ole_bytes))
        .map_err(|error| invalid_mathtype(format!("invalid OLE compound file: {error}")))?;
    if !compound.is_stream("/Equation Native") {
        return Err(invalid_mathtype(
            "MathType OLE object does not contain an Equation Native stream",
        ));
    }
    let mut equation_native = Vec::new();
    compound
        .open_stream("/Equation Native")
        .and_then(|mut stream| {
            (&mut stream)
                .take((MAX_EQUATION_NATIVE_BYTES + 1) as u64)
                .read_to_end(&mut equation_native)
        })
        .map_err(|error| invalid_mathtype(format!("cannot read Equation Native: {error}")))?;
    if equation_native.len() > MAX_EQUATION_NATIVE_BYTES {
        return Err(invalid_mathtype(format!(
            "Equation Native is larger than the {} byte safety limit",
            MAX_EQUATION_NATIVE_BYTES
        )));
    }
    let mtef = equation_native
        .get(EQUATION_NATIVE_HEADER_BYTES..)
        .ok_or_else(|| invalid_mathtype("Equation Native is shorter than its 28-byte header"))?;
    convert_mtef_to_latex(mtef)
}

fn convert_mtef_to_latex(bytes: &[u8]) -> DocxResult<MathTypeConversion> {
    let parsed = MtefParser::new(bytes).parse()?;
    let mut renderer = LatexRenderer::default();
    let mut latex = renderer.render_nodes(&parsed.nodes);
    latex = latex.trim().to_owned();
    if latex.is_empty() {
        return Err(invalid_mathtype(
            "MathType equation was decoded but did not contain editable mathematical content",
        ));
    }
    Ok(MathTypeConversion {
        latex,
        product_version: parsed.product_version,
        product_subversion: parsed.product_subversion,
        unsupported_features: renderer.unsupported_features,
    })
}

struct MtefParser<'a> {
    bytes: &'a [u8],
    position: usize,
    records: usize,
}

impl<'a> MtefParser<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            position: 0,
            records: 0,
        }
    }

    fn parse(mut self) -> DocxResult<ParsedMtef> {
        let version = self.read_u8()?;
        if version != 5 {
            return Err(invalid_mathtype(format!(
                "unsupported MathType MTEF version {version}; version 5 is required"
            )));
        }
        let _platform = self.read_u8()?;
        let _product = self.read_u8()?;
        let product_version = self.read_u8()?;
        let product_subversion = self.read_u8()?;
        let _application_key = self.read_stringz(MAX_ZERO_TERMINATED_BYTES)?;
        let _equation_options = self.read_u8()?;
        let nodes = self.parse_list(0)?;
        if self.position != self.bytes.len() {
            let trailing = &self.bytes[self.position..];
            if trailing.iter().any(|byte| *byte != 0) {
                return Err(invalid_mathtype(format!(
                    "{} unexpected bytes follow the MTEF equation",
                    trailing.len()
                )));
            }
        }
        Ok(ParsedMtef {
            product_version,
            product_subversion,
            nodes,
        })
    }

    fn parse_list(&mut self, depth: usize) -> DocxResult<Vec<MtefNode>> {
        if depth > MAX_MTEF_DEPTH {
            return Err(invalid_mathtype(format!(
                "MathType structure exceeds the nesting limit of {MAX_MTEF_DEPTH}"
            )));
        }
        let mut nodes = Vec::new();
        loop {
            let record_type = self.read_u8()?;
            self.records += 1;
            if self.records > MAX_MTEF_RECORDS {
                return Err(invalid_mathtype(format!(
                    "MathType structure exceeds the record limit of {MAX_MTEF_RECORDS}"
                )));
            }
            if record_type == 0 {
                break;
            }
            if let Some(node) = self.parse_record(record_type, depth + 1)? {
                nodes.push(node);
            }
        }
        Ok(nodes)
    }

    fn parse_record(&mut self, record_type: u8, depth: usize) -> DocxResult<Option<MtefNode>> {
        match record_type {
            1 => {
                let options = self.read_u8()?;
                self.skip_nudge(options)?;
                if options & OPT_LINE_SPACING != 0 {
                    self.skip(2)?;
                }
                if options & OPT_LINE_RULER != 0 {
                    self.skip_ruler()?;
                }
                let children = if options & OPT_LINE_NULL == 0 {
                    self.parse_list(depth)?
                } else {
                    Vec::new()
                };
                Ok(Some(MtefNode::Line(children)))
            }
            2 => {
                let options = self.read_u8()?;
                self.skip_nudge(options)?;
                let _typeface = self.read_u8()?;
                let mt_code = if options & OPT_CHAR_ENC_NO_MTCODE == 0 {
                    Some(self.read_u16()?)
                } else {
                    None
                };
                let font_position = if options & OPT_CHAR_ENC_CHAR_8 != 0 {
                    Some(u16::from(self.read_u8()?))
                } else if options & OPT_CHAR_ENC_CHAR_16 != 0 {
                    Some(self.read_u16()?)
                } else {
                    None
                };
                let embellishments = if options & OPT_CHAR_EMBELL != 0 {
                    self.parse_embellishment_list(depth)?
                } else {
                    Vec::new()
                };
                Ok(Some(MtefNode::Char {
                    mt_code,
                    font_position,
                    embellishments,
                }))
            }
            3 => {
                let options = self.read_u8()?;
                self.skip_nudge(options)?;
                let selector = self.read_u8()?;
                let first = self.read_u8()?;
                let variation = if first & 0x80 != 0 {
                    u16::from(first & 0x7f) | (u16::from(self.read_u8()?) << 8)
                } else {
                    u16::from(first)
                };
                let _template_options = self.read_u8()?;
                let children = self.parse_list(depth)?;
                Ok(Some(MtefNode::Template {
                    selector,
                    variation,
                    children,
                }))
            }
            4 => {
                let options = self.read_u8()?;
                self.skip_nudge(options)?;
                self.skip(2)?;
                if options & OPT_LINE_RULER != 0 {
                    self.skip_ruler()?;
                }
                Ok(Some(MtefNode::Pile(self.parse_list(depth)?)))
            }
            5 => {
                let options = self.read_u8()?;
                self.skip_nudge(options)?;
                self.skip(3)?;
                let rows = self.read_u8()?;
                let cols = self.read_u8()?;
                if rows == 0 || cols == 0 {
                    return Err(invalid_mathtype("MathType matrix has zero rows or columns"));
                }
                self.skip(partition_bytes(rows))?;
                self.skip(partition_bytes(cols))?;
                let cells = self.parse_list(depth)?;
                Ok(Some(MtefNode::Matrix { rows, cols, cells }))
            }
            6 => {
                let options = self.read_u8()?;
                self.skip_nudge(options)?;
                self.skip(1)?;
                Ok(None)
            }
            7 => {
                self.skip_ruler()?;
                Ok(None)
            }
            8 => {
                self.skip(2)?;
                Ok(None)
            }
            9 => {
                let size_select = self.read_u8()?;
                match size_select {
                    100 => self.skip(3)?,
                    101 => self.skip(2)?,
                    _ => self.skip(1)?,
                }
                Ok(None)
            }
            10..=14 => Ok(None),
            15 => {
                self.skip(1)?;
                Ok(None)
            }
            16 => {
                let options = self.read_u8()?;
                self.skip(if options & OPT_COLOR_CMYK != 0 { 8 } else { 6 })?;
                if options & OPT_COLOR_NAME != 0 {
                    self.read_stringz(MAX_ZERO_TERMINATED_BYTES)?;
                }
                Ok(None)
            }
            17 => {
                self.skip(1)?;
                self.read_stringz(MAX_ZERO_TERMINATED_BYTES)?;
                Ok(None)
            }
            18 => {
                self.skip_equation_preferences()?;
                Ok(None)
            }
            19 => {
                self.read_stringz(MAX_ZERO_TERMINATED_BYTES)?;
                Ok(None)
            }
            100..=u8::MAX => {
                let length = usize::from(self.read_u8()?);
                self.skip(length)?;
                Ok(None)
            }
            _ => Err(invalid_mathtype(format!(
                "unsupported MTEF record type {record_type} at byte {}",
                self.position.saturating_sub(1)
            ))),
        }
    }

    fn parse_embellishment_list(&mut self, depth: usize) -> DocxResult<Vec<u8>> {
        if depth > MAX_MTEF_DEPTH {
            return Err(invalid_mathtype(
                "MathType embellishment nesting is too deep",
            ));
        }
        let mut result = Vec::new();
        loop {
            let record_type = self.read_u8()?;
            self.records += 1;
            if record_type == 0 {
                break;
            }
            if record_type != 6 {
                return Err(invalid_mathtype(format!(
                    "unexpected record {record_type} in MathType embellishment list"
                )));
            }
            let options = self.read_u8()?;
            self.skip_nudge(options)?;
            result.push(self.read_u8()?);
        }
        Ok(result)
    }

    fn skip_equation_preferences(&mut self) -> DocxResult<()> {
        self.skip(1)?;
        let sizes_count = usize::from(self.read_u8()?);
        self.skip_dimension_entries(sizes_count)?;
        let spaces_count = usize::from(self.read_u8()?);
        self.skip_dimension_entries(spaces_count)?;
        let styles_count = usize::from(self.read_u8()?);
        for _ in 0..styles_count {
            let font_def = self.read_u8()?;
            if font_def != 0 {
                self.skip(1)?;
            }
        }
        Ok(())
    }

    fn skip_dimension_entries(&mut self, count: usize) -> DocxResult<()> {
        let mut high_nibble = true;
        for _ in 0..count {
            let _unit = self.read_nibble(&mut high_nibble)?;
            loop {
                if self.read_nibble(&mut high_nibble)? == 0x0f {
                    break;
                }
            }
        }
        if !high_nibble {
            self.position += 1;
        }
        self.ensure_position()
    }

    fn read_nibble(&mut self, high_nibble: &mut bool) -> DocxResult<u8> {
        let byte = *self
            .bytes
            .get(self.position)
            .ok_or_else(|| invalid_mathtype("unexpected end of MTEF dimension array"))?;
        let value = if *high_nibble { byte >> 4 } else { byte & 0x0f };
        if !*high_nibble {
            self.position += 1;
        }
        *high_nibble = !*high_nibble;
        Ok(value)
    }

    fn skip_nudge(&mut self, options: u8) -> DocxResult<()> {
        if options & OPT_NUDGE == 0 {
            return Ok(());
        }
        let dx = self.read_u8()?;
        let dy = self.read_u8()?;
        if dx == 128 && dy == 128 {
            self.skip(4)?;
        }
        Ok(())
    }

    fn skip_ruler(&mut self) -> DocxResult<()> {
        let stops = usize::from(self.read_u8()?);
        self.skip(stops.saturating_mul(3))
    }

    fn read_stringz(&mut self, max_bytes: usize) -> DocxResult<String> {
        let start = self.position;
        let remaining = self.bytes.get(start..).unwrap_or_default();
        let relative_end = remaining
            .iter()
            .take(max_bytes.saturating_add(1))
            .position(|byte| *byte == 0)
            .ok_or_else(|| invalid_mathtype("unterminated or oversized MTEF string"))?;
        self.position = start + relative_end + 1;
        Ok(String::from_utf8_lossy(&self.bytes[start..start + relative_end]).into_owned())
    }

    fn read_u8(&mut self) -> DocxResult<u8> {
        let value = *self
            .bytes
            .get(self.position)
            .ok_or_else(|| invalid_mathtype("unexpected end of MTEF data"))?;
        self.position += 1;
        Ok(value)
    }

    fn read_u16(&mut self) -> DocxResult<u16> {
        let bytes = self
            .bytes
            .get(self.position..self.position.saturating_add(2))
            .ok_or_else(|| invalid_mathtype("unexpected end of MTEF 16-bit value"))?;
        self.position += 2;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn skip(&mut self, length: usize) -> DocxResult<()> {
        self.position = self
            .position
            .checked_add(length)
            .ok_or_else(|| invalid_mathtype("MTEF byte position overflow"))?;
        self.ensure_position()
    }

    fn ensure_position(&self) -> DocxResult<()> {
        if self.position > self.bytes.len() {
            Err(invalid_mathtype("unexpected end of MTEF data"))
        } else {
            Ok(())
        }
    }
}

fn partition_bytes(parts: u8) -> usize {
    (usize::from(parts).saturating_add(1).saturating_mul(2)).div_ceil(8)
}

#[derive(Default)]
struct LatexRenderer {
    unsupported_features: Vec<String>,
}

impl LatexRenderer {
    fn render_nodes(&mut self, nodes: &[MtefNode]) -> String {
        nodes
            .iter()
            .map(|node| self.render_node(node))
            .collect::<Vec<_>>()
            .join("")
    }

    fn render_node(&mut self, node: &MtefNode) -> String {
        match node {
            MtefNode::Line(children) => self.render_nodes(children),
            MtefNode::Pile(lines) => {
                let rows = lines
                    .iter()
                    .map(|line| self.render_node(line))
                    .collect::<Vec<_>>()
                    .join(r" \\ ");
                format!(r"\begin{{aligned}}{rows}\end{{aligned}}")
            }
            MtefNode::Matrix { rows, cols, cells } => {
                let mut output = String::new();
                let cell_count = usize::from(*rows).saturating_mul(usize::from(*cols));
                for index in 0..cell_count {
                    if index > 0 {
                        if index % usize::from(*cols) == 0 {
                            output.push_str(r" \\ ");
                        } else {
                            output.push_str(" & ");
                        }
                    }
                    if let Some(cell) = cells.get(index) {
                        output.push_str(&self.render_node(cell));
                    }
                }
                format!(r"\begin{{matrix}}{output}\end{{matrix}}")
            }
            MtefNode::Char {
                mt_code,
                font_position,
                embellishments,
            } => {
                let mut value = match mt_code.and_then(mt_code_to_latex) {
                    Some(value) => value,
                    None => {
                        self.note_unsupported(format!(
                            "character mt={:?} font={:?}",
                            mt_code, font_position
                        ));
                        font_position
                            .and_then(|position| char::from_u32(u32::from(position)))
                            .map(escape_latex_char)
                            .unwrap_or_else(|| r"\square".to_owned())
                    }
                };
                for embellishment in embellishments {
                    value = apply_embellishment(*embellishment, value, self);
                }
                value
            }
            MtefNode::Template {
                selector,
                variation,
                children,
            } => self.render_template(*selector, *variation, children),
        }
    }

    fn render_template(&mut self, selector: u8, variation: u16, children: &[MtefNode]) -> String {
        let rendered = children
            .iter()
            .map(|child| self.render_node(child))
            .collect::<Vec<_>>();
        let child = |index: usize| rendered.get(index).cloned().unwrap_or_default();
        match selector {
            0..=8 => {
                let (left, right) = fence_pair(selector);
                let left = if variation & 0x0001 != 0 { left } else { "." };
                let right = if variation & 0x0002 != 0 { right } else { "." };
                format!(r"\left{left}{}\right{right}", child(0))
            }
            9 => {
                let left = match variation & 0x000f {
                    1 => ")",
                    2 => "[",
                    3 => "]",
                    _ => "(",
                };
                let right = match variation & 0x00f0 {
                    0x10 => ")",
                    0x20 => "[",
                    0x30 => "]",
                    _ => "(",
                };
                format!(r"\left{left}{}\right{right}", child(0))
            }
            10 => {
                if variation & 0x0001 != 0 {
                    format!(r"\sqrt[{}]{{{}}}", child(1), child(0))
                } else {
                    format!(r"\sqrt{{{}}}", child(0))
                }
            }
            11 => {
                if variation & 0x0002 != 0 {
                    format!(r"{{{}}}/{{{}}}", child(0), child(1))
                } else {
                    format!(r"\frac{{{}}}{{{}}}", child(0), child(1))
                }
            }
            12 => format!(r"\underline{{{}}}", child(0)),
            13 => format!(r"\overline{{{}}}", child(0)),
            14 => {
                let arrow = if variation & 0x0001 != 0 {
                    r"\Longleftrightarrow"
                } else if variation & 0x0010 != 0 {
                    r"\longleftarrow"
                } else {
                    r"\longrightarrow"
                };
                match (variation & 0x0004 != 0, variation & 0x0008 != 0) {
                    (true, true) => {
                        format!(
                            r"\mathop{{{arrow}}}\limits^{{{}}}_{{{}}}",
                            child(0),
                            child(1)
                        )
                    }
                    (true, false) => format!(r"\mathop{{{arrow}}}\limits^{{{}}}", child(0)),
                    (false, true) => format!(r"\mathop{{{arrow}}}\limits_{{{}}}", child(0)),
                    _ => arrow.to_owned(),
                }
            }
            15 => render_large_operator(integral_symbol(variation), variation, &rendered),
            16 => render_large_operator(r"\sum", variation, &rendered),
            17 => render_large_operator(r"\prod", variation, &rendered),
            18 => render_large_operator(r"\coprod", variation, &rendered),
            19 => render_large_operator(r"\bigcup", variation, &rendered),
            20 => render_large_operator(r"\bigcap", variation, &rendered),
            21 | 22 => {
                let operator = rendered
                    .first()
                    .cloned()
                    .unwrap_or_else(|| r"\mathop{}".to_owned());
                render_large_operator(&operator, variation, &rendered[rendered.len().min(1)..])
            }
            23 => {
                let body = child(0);
                let limit = child(1);
                if variation & 0x0001 != 0 {
                    format!(r"\mathop{{{body}}}\limits^{{{limit}}}")
                } else {
                    format!(r"\mathop{{{body}}}\limits_{{{limit}}}")
                }
            }
            24 => {
                if variation & 0x0001 != 0 {
                    format!(r"\overbrace{{{}}}^{{{}}}", child(0), child(1))
                } else {
                    format!(r"\underbrace{{{}}}_{{{}}}", child(0), child(1))
                }
            }
            25 => {
                if variation & 0x0001 != 0 {
                    format!(r"\overline{{{}}}^{{{}}}", child(0), child(1))
                } else {
                    format!(r"\underline{{{}}}_{{{}}}", child(0), child(1))
                }
            }
            26 => {
                self.note_unsupported("long division template".to_owned());
                format!(
                    r"\overline{{{}}}\mathbin{{\backslash}}{}",
                    child(0),
                    child(1)
                )
            }
            27 => render_script(variation, &child(0), "", true),
            // MathType stores superscript content in slot 2; slot 1 is the
            // null subscript placeholder.
            28 => {
                let mut superscript = child(1);
                if let Some(inner) = superscript
                    .strip_prefix("^{")
                    .and_then(|value| value.strip_suffix('}'))
                {
                    superscript = inner.to_owned();
                }
                render_script(variation, "", &superscript, true)
            }
            29 => render_script(variation, &child(0), &child(1), true),
            30 => format!(
                r"\left\langle {}\middle|{}\right\rangle",
                child(0),
                child(1)
            ),
            31 => {
                if variation & 0x0004 != 0 {
                    format!(r"\underset{{\longrightarrow}}{{{}}}", child(0))
                } else if variation & 0x0001 != 0 {
                    format!(r"\overleftarrow{{{}}}", child(0))
                } else {
                    format!(r"\overrightarrow{{{}}}", child(0))
                }
            }
            32 => format!(r"\widetilde{{{}}}", child(0)),
            33 => format!(r"\widehat{{{}}}", child(0)),
            34 => format!(r"\overset{{\frown}}{{{}}}", child(0)),
            35 => {
                self.note_unsupported("joint-status template".to_owned());
                format!(r"\widehat{{{}}}", child(0))
            }
            36 => {
                self.note_unsupported("strike template".to_owned());
                format!(r"\cancel{{{}}}", child(0))
            }
            37 => format!(r"\boxed{{{}}}", child(0)),
            _ => {
                self.note_unsupported(format!("template selector {selector}"));
                rendered.join("")
            }
        }
    }

    fn note_unsupported(&mut self, feature: String) {
        if !self.unsupported_features.contains(&feature) {
            self.unsupported_features.push(feature);
        }
    }
}

fn fence_pair(selector: u8) -> (&'static str, &'static str) {
    match selector {
        0 => (r"\langle", r"\rangle"),
        1 => ("(", ")"),
        2 => (r"\{", r"\}"),
        3 => ("[", "]"),
        4 => ("|", "|"),
        5 => (r"\|", r"\|"),
        6 => (r"\lfloor", r"\rfloor"),
        7 => (r"\lceil", r"\rceil"),
        8 => (r"\llbracket", r"\rrbracket"),
        _ => (".", "."),
    }
}

fn integral_symbol(variation: u16) -> &'static str {
    match variation & 0x000f {
        2 => r"\iint",
        3 => r"\iiint",
        4 => r"\oint",
        8 => r"\varointclockwise",
        12 => r"\ointctrclockwise",
        _ => r"\int",
    }
}

fn render_large_operator(operator: &str, variation: u16, children: &[String]) -> String {
    let mut index = 0usize;
    let lower = if variation & 0x0010 != 0 {
        let value = children.get(index).cloned().unwrap_or_default();
        index += 1;
        Some(value)
    } else {
        None
    };
    let upper = if variation & 0x0020 != 0 {
        let value = children.get(index).cloned().unwrap_or_default();
        index += 1;
        Some(value)
    } else {
        None
    };
    let body = children.get(index).cloned().unwrap_or_default();
    let limits = if variation & 0x0040 != 0 {
        r"\limits"
    } else {
        ""
    };
    let mut output = format!("{operator}{limits}");
    if let Some(lower) = lower {
        output.push_str(&format!("_{{{lower}}}"));
    }
    if let Some(upper) = upper {
        output.push_str(&format!("^{{{upper}}}"));
    }
    output.push_str(&body);
    output
}

fn render_script(variation: u16, subscript: &str, superscript: &str, braces: bool) -> String {
    let mut output = String::new();
    if variation & 0x0001 != 0 {
        output.push_str("{}");
    }
    if !subscript.is_empty() {
        output.push_str(&format!("_{{{subscript}}}"));
    }
    if !superscript.is_empty() {
        output.push_str(&format!("^{{{superscript}}}"));
    }
    if braces && output.is_empty() {
        "{}".to_owned()
    } else {
        output
    }
}

fn apply_embellishment(embellishment: u8, value: String, renderer: &mut LatexRenderer) -> String {
    match embellishment {
        2 => format!(r"\dot{{{value}}}"),
        3 => format!(r"\ddot{{{value}}}"),
        4 => format!(r"\overset{{...}}{{{value}}}"),
        5 => format!("{value}'"),
        6 => format!("{value}''"),
        7 => format!("'{value}"),
        8 => format!(r"\tilde{{{value}}}"),
        9 => format!(r"\hat{{{value}}}"),
        10 => format!(r"\not{{{value}}}"),
        11 | 14 => format!(r"\overrightarrow{{{value}}}"),
        12 | 15 => format!(r"\overleftarrow{{{value}}}"),
        13 => format!(r"\overleftrightarrow{{{value}}}"),
        16 | 17 => format!(r"\bar{{{value}}}"),
        18 => format!("{value}'''"),
        19 => format!(r"\overset{{\frown}}{{{value}}}"),
        20 => format!(r"\overset{{\smile}}{{{value}}}"),
        25 => format!(r"\underset{{.}}{{{value}}}"),
        26 => format!(r"\underset{{..}}{{{value}}}"),
        29 => format!(r"\underline{{{value}}}"),
        30 => format!(r"\underset{{\sim}}{{{value}}}"),
        33 | 36 => format!(r"\underset{{\longrightarrow}}{{{value}}}"),
        34 | 37 => format!(r"\underset{{\longleftarrow}}{{{value}}}"),
        35 => format!(r"\underset{{\longleftrightarrow}}{{{value}}}"),
        _ => {
            renderer.note_unsupported(format!("embellishment {embellishment}"));
            value
        }
    }
}

fn mt_code_to_latex(code: u16) -> Option<String> {
    let value = match code {
        0x0009 => r"\quad ".to_owned(),
        0x0020 => " ".to_owned(),
        0x00a0 => r"\ ".to_owned(),
        0x00b0 => r"^{\circ}".to_owned(),
        0x00b1 => r"\pm ".to_owned(),
        0x00b7 => r"\cdot ".to_owned(),
        0x00d7 => r"\times ".to_owned(),
        0x00f7 => r"\div ".to_owned(),
        0x0192 => r"\mathit{f}".to_owned(),
        0x02c6 => r"\hat{}".to_owned(),
        0x0302 => r"\hat{}".to_owned(),
        0x03b1 => r"\alpha ".to_owned(),
        0x03b2 => r"\beta ".to_owned(),
        0x03b3 => r"\gamma ".to_owned(),
        0x03b4 => r"\delta ".to_owned(),
        0x03b5 => r"\varepsilon ".to_owned(),
        0x03b6 => r"\zeta ".to_owned(),
        0x03b7 => r"\eta ".to_owned(),
        0x03b8 => r"\theta ".to_owned(),
        0x03bb => r"\lambda ".to_owned(),
        0x03bc => r"\mu ".to_owned(),
        0x03c0 => r"\pi ".to_owned(),
        0x03c1 => r"\rho ".to_owned(),
        0x03c3 => r"\sigma ".to_owned(),
        0x03c6 => r"\varphi ".to_owned(),
        0x03c9 => r"\omega ".to_owned(),
        0x0393 => r"\Gamma ".to_owned(),
        0x0394 => r"\Delta ".to_owned(),
        0x0398 => r"\Theta ".to_owned(),
        0x039b => r"\Lambda ".to_owned(),
        0x03a0 => r"\Pi ".to_owned(),
        0x03a3 => r"\Sigma ".to_owned(),
        0x03a6 => r"\Phi ".to_owned(),
        0x03a9 => r"\Omega ".to_owned(),
        0x2013 => "-".to_owned(),
        0x2022 => r"\bullet ".to_owned(),
        0x22c5 => r"\cdot ".to_owned(),
        0x2026 => r"\ldots ".to_owned(),
        0x2032 => "'".to_owned(),
        0x2033 => "''".to_owned(),
        0x2190 => r"\leftarrow ".to_owned(),
        0x2192 => r"\rightarrow ".to_owned(),
        0x2194 => r"\leftrightarrow ".to_owned(),
        0x21d2 => r"\Rightarrow ".to_owned(),
        0x21d4 => r"\Leftrightarrow ".to_owned(),
        0x2200 => r"\forall ".to_owned(),
        0x2202 => r"\partial ".to_owned(),
        0x2203 => r"\exists ".to_owned(),
        0x2205 => r"\varnothing ".to_owned(),
        0x2206 => r"\Delta ".to_owned(),
        0x2207 => r"\nabla ".to_owned(),
        0x2208 => r"\in ".to_owned(),
        0x2209 => r"\notin ".to_owned(),
        0x220b => r"\ni ".to_owned(),
        0x220f => r"\prod ".to_owned(),
        0x2211 => r"\sum ".to_owned(),
        0x2212 => "-".to_owned(),
        0x221a => r"\sqrt{}".to_owned(),
        0x221d => r"\propto ".to_owned(),
        0x221e => r"\infty ".to_owned(),
        0x2220 => r"\angle ".to_owned(),
        0x2223 => r"\mid ".to_owned(),
        0x2225 => r"\parallel ".to_owned(),
        0x222b => r"\int ".to_owned(),
        0x2234 => r"\therefore ".to_owned(),
        0x2235 => r"\because ".to_owned(),
        0x223c => r"\sim ".to_owned(),
        0x2248 => r"\approx ".to_owned(),
        0x2260 => r"\ne ".to_owned(),
        0x2261 => r"\equiv ".to_owned(),
        0x2264 => r"\le ".to_owned(),
        0x2265 => r"\ge ".to_owned(),
        0x2282 => r"\subset ".to_owned(),
        0x2283 => r"\supset ".to_owned(),
        0x2286 => r"\subseteq ".to_owned(),
        0x2287 => r"\supseteq ".to_owned(),
        0x2295 => r"\oplus ".to_owned(),
        0x22a5 => r"\perp ".to_owned(),
        _ => {
            let character = char::from_u32(u32::from(code))?;
            if character.is_control() {
                return None;
            }
            escape_latex_char(character)
        }
    };
    Some(value)
}

fn escape_latex_char(character: char) -> String {
    match character {
        '#' => r"\#".to_owned(),
        '$' => r"\$".to_owned(),
        '%' => r"\%".to_owned(),
        '&' => r"\&".to_owned(),
        '_' => r"\_".to_owned(),
        '{' => r"\{".to_owned(),
        '}' => r"\}".to_owned(),
        '\\' => r"\backslash ".to_owned(),
        '^' => r"\hat{}".to_owned(),
        '~' => r"\sim ".to_owned(),
        other => other.to_string(),
    }
}

fn invalid_mathtype(message: impl Into<String>) -> DocxError {
    DocxError::Xml {
        part_name: "MathType Equation Native".to_owned(),
        position: 0,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{env, fs::File};
    use zip::ZipArchive;

    #[test]
    fn renders_basic_mtef_characters() {
        let bytes = [
            5, 1, 0, 7, 0, b'T', 0, 1, // header
            1, 0, // line
            2, 0, 0x81, b'x', 0, // x
            2, 0, 0x81, b'+', 0, // +
            2, 0, 0x81, b'1', 0, // 1
            0, // line end
            0, // equation end
        ];
        let converted = convert_mtef_to_latex(&bytes).expect("MTEF should parse");
        assert_eq!(converted.latex, "x+1");
        assert!(converted.unsupported_features.is_empty());
    }

    #[test]
    fn rejects_non_mtef_five() {
        let error =
            convert_mtef_to_latex(&[3, 0, 0, 0, 0, 0, 0]).expect_err("old MTEF must be rejected");
        assert!(error.to_string().contains("version 3"));
    }

    #[test]
    fn converts_supplied_math_exam_formulas_when_available() {
        let Ok(path) = env::var("ZHITIKU_MATHTYPE_DOCX") else {
            return;
        };
        let file = File::open(&path).expect("fixture DOCX should open");
        let mut archive = ZipArchive::new(file).expect("fixture should be a ZIP package");
        let mut names = (0..archive.len())
            .filter_map(|index| {
                archive
                    .by_index(index)
                    .ok()
                    .map(|entry| entry.name().to_owned())
            })
            .filter(|name| name.starts_with("word/embeddings/") && name.ends_with(".bin"))
            .collect::<Vec<_>>();
        names.sort();

        let mut converted = Vec::new();
        for name in names {
            let mut bytes = Vec::new();
            archive
                .by_name(&name)
                .expect("embedding should exist")
                .read_to_end(&mut bytes)
                .expect("embedding should be readable");
            let formula = convert_equation_native_to_latex(&bytes)
                .unwrap_or_else(|error| panic!("{name} failed: {error}"));
            converted.push(formula);
        }

        assert_eq!(converted.len(), 135, "fixture formula count changed");
        assert!(
            converted.iter().all(|formula| !formula.latex.is_empty()),
            "every MathType object should yield editable LaTeX"
        );
        assert!(
            converted
                .iter()
                .all(|formula| formula.unsupported_features.is_empty()),
            "fixture contains an unsupported MathType construct"
        );
        assert!(
            converted
                .iter()
                .all(|formula| !formula.latex.contains("{}")),
            "fixture conversion must not contain empty script placeholders"
        );
        assert!(
            converted
                .iter()
                .any(|formula| formula.latex.contains(r"x^{2}+y^{2}=16")),
            "fixture should preserve ordinary superscripts"
        );
        assert!(
            converted
                .iter()
                .any(|formula| formula.latex.contains(r"2022^{\circ}")),
            "fixture should preserve degree superscripts without double nesting"
        );

        let limits = DocxLimits {
            allow_mathtype_ole: true,
            ..DocxLimits::default()
        };
        let occurrences =
            read_mathtype_from_docx(File::open(path).expect("fixture should reopen"), &limits)
                .expect("complete DOCX MathType extraction should succeed");
        assert_eq!(occurrences.len(), 135);
        assert!(
            occurrences
                .iter()
                .all(|formula| formula.product_version >= 6 && !formula.latex.is_empty())
        );
    }
}
