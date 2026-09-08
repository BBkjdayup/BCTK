use quick_xml::{
    events::{BytesRef, BytesStart, Event},
    name::{Namespace, ResolveResult},
    reader::NsReader,
};

use super::{DocxError, DocxLimits, DocxResult};

pub(crate) const WML_TRANSITIONAL: &[u8] =
    b"http://schemas.openxmlformats.org/wordprocessingml/2006/main";
pub(crate) const WML_STRICT: &[u8] = b"http://purl.oclc.org/ooxml/wordprocessingml/main";
pub(crate) const OMML_TRANSITIONAL: &[u8] =
    b"http://schemas.openxmlformats.org/officeDocument/2006/math";
pub(crate) const OMML_STRICT: &[u8] = b"http://purl.oclc.org/ooxml/officeDocument/math";
pub(crate) const DRAWINGML_TRANSITIONAL: &[u8] =
    b"http://schemas.openxmlformats.org/drawingml/2006/main";
pub(crate) const DRAWINGML_STRICT: &[u8] = b"http://purl.oclc.org/ooxml/drawingml/main";
pub(crate) const OFFICE_RELATIONSHIPS_TRANSITIONAL: &[u8] =
    b"http://schemas.openxmlformats.org/officeDocument/2006/relationships";
pub(crate) const OFFICE_RELATIONSHIPS_STRICT: &[u8] =
    b"http://purl.oclc.org/ooxml/officeDocument/relationships";
pub(crate) const OFFICE_VML_NS: &[u8] = b"urn:schemas-microsoft-com:office:office";
pub(crate) const CONTENT_TYPES_NS: &[u8] =
    b"http://schemas.openxmlformats.org/package/2006/content-types";
pub(crate) const RELATIONSHIPS_NS: &[u8] =
    b"http://schemas.openxmlformats.org/package/2006/relationships";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NamespaceKind {
    Word,
    Math,
    Drawing,
    Office,
    OfficeRelationships,
    ContentTypes,
    Relationships,
    Other,
    Unbound,
}

pub(crate) fn classify_namespace(namespace: ResolveResult<'_>) -> NamespaceKind {
    match namespace {
        ResolveResult::Bound(Namespace(uri)) if uri == WML_TRANSITIONAL || uri == WML_STRICT => {
            NamespaceKind::Word
        }
        ResolveResult::Bound(Namespace(uri)) if uri == OMML_TRANSITIONAL || uri == OMML_STRICT => {
            NamespaceKind::Math
        }
        ResolveResult::Bound(Namespace(uri))
            if uri == DRAWINGML_TRANSITIONAL || uri == DRAWINGML_STRICT =>
        {
            NamespaceKind::Drawing
        }
        ResolveResult::Bound(Namespace(uri)) if uri == OFFICE_VML_NS => NamespaceKind::Office,
        ResolveResult::Bound(Namespace(uri))
            if uri == OFFICE_RELATIONSHIPS_TRANSITIONAL || uri == OFFICE_RELATIONSHIPS_STRICT =>
        {
            NamespaceKind::OfficeRelationships
        }
        ResolveResult::Bound(Namespace(uri)) if uri == CONTENT_TYPES_NS => {
            NamespaceKind::ContentTypes
        }
        ResolveResult::Bound(Namespace(uri)) if uri == RELATIONSHIPS_NS => {
            NamespaceKind::Relationships
        }
        ResolveResult::Bound(_) | ResolveResult::Unknown(_) => NamespaceKind::Other,
        ResolveResult::Unbound => NamespaceKind::Unbound,
    }
}

pub(crate) fn local_name(start: &BytesStart<'_>) -> Vec<u8> {
    start.local_name().as_ref().to_vec()
}

pub(crate) fn attribute_value(
    reader: &NsReader<&[u8]>,
    start: &BytesStart<'_>,
    local: &[u8],
    expected_namespace: Option<NamespaceKind>,
    part_name: &str,
) -> DocxResult<Option<String>> {
    let decoder = start.decoder();
    for attribute in start.attributes() {
        let attribute =
            attribute.map_err(|error| DocxError::xml(part_name, reader.error_position(), error))?;
        if attribute.key.local_name().as_ref() != local {
            continue;
        }
        if let Some(expected) = expected_namespace {
            let (namespace, _) = reader.resolver().resolve_attribute(attribute.key);
            if classify_namespace(namespace) != expected {
                continue;
            }
        }
        #[allow(deprecated)]
        let value = attribute
            .decode_and_unescape_value(decoder)
            .map_err(|error| DocxError::xml(part_name, reader.error_position(), error))?;
        return Ok(Some(value.into_owned()));
    }
    Ok(None)
}

pub(crate) fn decode_reference(
    reference: &BytesRef<'_>,
    part_name: &str,
    position: u64,
) -> DocxResult<char> {
    if let Some(value) = reference
        .resolve_char_ref()
        .map_err(|error| DocxError::xml(part_name, position, error))?
    {
        return Ok(value);
    }

    let name = reference
        .decode()
        .map_err(|error| DocxError::xml(part_name, position, error))?;
    match name.as_ref() {
        "lt" => Ok('<'),
        "gt" => Ok('>'),
        "amp" => Ok('&'),
        "apos" => Ok('\''),
        "quot" => Ok('"'),
        _ => Err(DocxError::xml(
            part_name,
            position,
            format!("custom entity '&{name};' is not allowed"),
        )),
    }
}

/// Parses an XML part without building a tree, rejecting DTDs and excessive
/// nesting.  This is used for replacement parts that the narrow readers do not
/// otherwise interpret.
pub(crate) fn validate_safe_xml(
    xml: &[u8],
    part_name: &str,
    limits: &DocxLimits,
) -> DocxResult<()> {
    if xml.len() as u64 > limits.max_xml_part_bytes {
        return Err(DocxError::LimitExceeded {
            resource: part_name.to_owned(),
            limit: limits.max_xml_part_bytes,
            actual: xml.len() as u64,
        });
    }

    let mut reader = NsReader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut depth = 0usize;
    loop {
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|error| DocxError::xml(part_name, reader.error_position(), error))?;
        match event {
            Event::Start(_) => {
                depth += 1;
                if depth > limits.max_xml_depth {
                    return Err(DocxError::LimitExceeded {
                        resource: format!("XML depth in {part_name}"),
                        limit: limits.max_xml_depth as u64,
                        actual: depth as u64,
                    });
                }
            }
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::DocType(_) => {
                return Err(DocxError::xml(
                    part_name,
                    reader.buffer_position(),
                    "DOCTYPE declarations are not allowed",
                ));
            }
            Event::Eof => return Ok(()),
            _ => {}
        }
        buffer.clear();
    }
}
