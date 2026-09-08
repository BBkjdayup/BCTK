use std::{
    collections::BTreeSet,
    io::{self, Read, Seek, SeekFrom},
};

use quick_xml::{events::Event, reader::NsReader};
use zip::ZipArchive;

use super::{
    Diagnostic, DiagnosticSeverity, DocxError, DocxLimits, DocxResult,
    xml::{NamespaceKind, attribute_value, classify_namespace, local_name, validate_safe_xml},
};

const CONTENT_TYPES_PART: &str = "[Content_Types].xml";
const ROOT_RELS_PART: &str = "_rels/.rels";
const DOCUMENT_PART: &str = "word/document.xml";
const ZIP_END_OF_CENTRAL_DIRECTORY_BYTES: usize = 22;
const ZIP_END_OF_CENTRAL_DIRECTORY_SIGNATURE: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];

const DOCUMENT_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
const TEMPLATE_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.template.main+xml";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PackageKind {
    Document,
    Template,
    #[default]
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartInfo {
    pub index: usize,
    pub name: String,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
    pub crc32: u32,
    pub compression_method: String,
    pub is_directory: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PackageInspection {
    pub package_kind: PackageKind,
    pub archive_bytes: Option<u64>,
    pub total_compressed_bytes: u64,
    pub total_uncompressed_bytes: u64,
    pub parts: Vec<PartInfo>,
    pub diagnostics: Vec<Diagnostic>,
}

impl PackageInspection {
    pub fn is_acceptable(&self) -> bool {
        !self
            .diagnostics
            .iter()
            .any(|item| item.severity == DiagnosticSeverity::Error)
    }

    pub fn ensure_acceptable(&self) -> DocxResult<()> {
        if self.is_acceptable() {
            Ok(())
        } else {
            Err(DocxError::rejected(self.diagnostics.clone()))
        }
    }
}

/// Inspects a DOCX reader from byte zero and restores no caller-visible state.
pub fn inspect_docx<R: Read + Seek>(
    mut source: R,
    limits: &DocxLimits,
) -> DocxResult<PackageInspection> {
    let archive_bytes = source.seek(SeekFrom::End(0))?;
    if let Some(diagnostic) = archive_size_diagnostic(archive_bytes, limits) {
        return Ok(PackageInspection {
            archive_bytes: Some(archive_bytes),
            diagnostics: vec![diagnostic],
            ..PackageInspection::default()
        });
    }
    let declared_entries = declared_zip_entry_count(&mut source, archive_bytes)?;
    source.seek(SeekFrom::Start(0))?;
    let mut archive = ZipArchive::new(source)?;
    inspect_archive_with_declared_entries(
        &mut archive,
        Some(archive_bytes),
        declared_entries,
        limits,
    )
}

fn declared_zip_entry_count<R: Read + Seek>(
    source: &mut R,
    archive_bytes: u64,
) -> io::Result<Option<usize>> {
    let search_bytes = archive_bytes
        .min((ZIP_END_OF_CENTRAL_DIRECTORY_BYTES + usize::from(u16::MAX)) as u64)
        as usize;
    if search_bytes < ZIP_END_OF_CENTRAL_DIRECTORY_BYTES {
        return Ok(None);
    }

    source.seek(SeekFrom::Start(archive_bytes - search_bytes as u64))?;
    let mut tail = vec![0; search_bytes];
    source.read_exact(&mut tail)?;

    for offset in (0..=tail.len() - ZIP_END_OF_CENTRAL_DIRECTORY_BYTES).rev() {
        if tail[offset..offset + 4] != ZIP_END_OF_CENTRAL_DIRECTORY_SIGNATURE {
            continue;
        }

        let comment_bytes = usize::from(u16::from_le_bytes([tail[offset + 20], tail[offset + 21]]));
        if offset + ZIP_END_OF_CENTRAL_DIRECTORY_BYTES + comment_bytes != tail.len() {
            continue;
        }

        return Ok(Some(usize::from(u16::from_le_bytes([
            tail[offset + 10],
            tail[offset + 11],
        ]))));
    }

    Ok(None)
}

pub(crate) fn inspect_archive_with_size<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    archive_bytes: Option<u64>,
    limits: &DocxLimits,
) -> DocxResult<PackageInspection> {
    inspect_archive_with_declared_entries(archive, archive_bytes, None, limits)
}

fn inspect_archive_with_declared_entries<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    archive_bytes: Option<u64>,
    declared_entries: Option<usize>,
    limits: &DocxLimits,
) -> DocxResult<PackageInspection> {
    let mut inspection = PackageInspection {
        archive_bytes,
        ..PackageInspection::default()
    };

    if let Some(diagnostic) = archive_bytes.and_then(|size| archive_size_diagnostic(size, limits)) {
        inspection.diagnostics.push(diagnostic);
    }
    let entry_count = declared_entries.unwrap_or_else(|| archive.len());
    if entry_count > limits.max_entries {
        inspection.diagnostics.push(Diagnostic::error(
            "DOCX_TOO_MANY_PARTS",
            None::<String>,
            format!(
                "package contains {} ZIP entries; the configured limit is {}",
                entry_count, limits.max_entries
            ),
        ));
    }
    if entry_count > archive.len() {
        inspection.diagnostics.push(Diagnostic::error(
            "DOCX_DUPLICATE_PART",
            None::<String>,
            format!(
                "the central directory declares {entry_count} entries, but only {} unique part names remain; duplicate part names are not allowed",
                archive.len()
            ),
        ));
    }
    if archive.offset() != 0 {
        inspection.diagnostics.push(Diagnostic::error(
            "DOCX_PREPENDED_DATA",
            None::<String>,
            format!("{} byte(s) occur before the ZIP package", archive.offset()),
        ));
    }
    if archive.has_overlapping_files()? {
        inspection.diagnostics.push(Diagnostic::error(
            "DOCX_OVERLAPPING_ZIP_ENTRIES",
            None::<String>,
            "two or more ZIP entries share compressed data",
        ));
    }

    let mut exact_names = BTreeSet::new();
    let mut folded_names = BTreeSet::new();
    for index in 0..archive.len() {
        let file = archive.by_index_raw(index)?;
        let name = file.name().to_owned();
        let compressed_size = file.compressed_size();
        let uncompressed_size = file.size();
        let is_directory = file.is_dir();

        inspection.total_compressed_bytes = inspection
            .total_compressed_bytes
            .saturating_add(compressed_size);
        inspection.total_uncompressed_bytes = inspection
            .total_uncompressed_bytes
            .saturating_add(uncompressed_size);

        if let Some(reason) = invalid_part_name(&file) {
            inspection.diagnostics.push(Diagnostic::error(
                "DOCX_UNSAFE_PART_NAME",
                Some(name.clone()),
                reason,
            ));
        }
        if file.encrypted() {
            inspection.diagnostics.push(Diagnostic::error(
                "DOCX_ENCRYPTED_PART",
                Some(name.clone()),
                "encrypted ZIP entries are not supported",
            ));
        }
        if file.is_symlink() {
            inspection.diagnostics.push(Diagnostic::error(
                "DOCX_SYMLINK_PART",
                Some(name.clone()),
                "symbolic links are not valid DOCX package parts",
            ));
        }
        let exact_name_is_new = exact_names.insert(name.clone());
        if !exact_name_is_new {
            inspection.diagnostics.push(Diagnostic::error(
                "DOCX_DUPLICATE_PART",
                Some(name.clone()),
                "the package contains the same part name more than once",
            ));
        }
        if exact_name_is_new && !folded_names.insert(name.to_ascii_lowercase()) {
            inspection.diagnostics.push(Diagnostic::error(
                "DOCX_AMBIGUOUS_PART_CASE",
                Some(name.clone()),
                "two part names differ only by ASCII letter case",
            ));
        }

        let part_limit = limits.part_limit(&name);
        if uncompressed_size > part_limit {
            inspection.diagnostics.push(Diagnostic::error(
                "DOCX_PART_TOO_LARGE",
                Some(name.clone()),
                format!(
                    "part expands to {uncompressed_size} bytes; its limit is {part_limit} bytes"
                ),
            ));
        }
        if compressed_size > 0
            && uncompressed_size >= 1024 * 1024
            && uncompressed_size / compressed_size > limits.suspicious_compression_ratio
        {
            inspection.diagnostics.push(Diagnostic::warning(
                "DOCX_SUSPICIOUS_COMPRESSION_RATIO",
                Some(name.clone()),
                format!(
                    "declared compression ratio is approximately {}:1",
                    uncompressed_size / compressed_size
                ),
            ));
        }

        if let Some(reason) = active_part_reason(&name, limits) {
            inspection.diagnostics.push(Diagnostic::error(
                "DOCX_ACTIVE_CONTENT_PART",
                Some(name.clone()),
                reason,
            ));
        }

        inspection.parts.push(PartInfo {
            index,
            name,
            compressed_size,
            uncompressed_size,
            crc32: file.crc32(),
            compression_method: format!("{:?}", file.compression()),
            is_directory,
        });
    }

    if inspection.total_uncompressed_bytes > limits.max_total_uncompressed_bytes {
        inspection.diagnostics.push(Diagnostic::error(
            "DOCX_TOTAL_UNCOMPRESSED_TOO_LARGE",
            None::<String>,
            format!(
                "parts expand to {} bytes in total; the configured limit is {} bytes",
                inspection.total_uncompressed_bytes, limits.max_total_uncompressed_bytes
            ),
        ));
    }

    for required in [CONTENT_TYPES_PART, ROOT_RELS_PART, DOCUMENT_PART] {
        if !exact_names.contains(required) {
            inspection.diagnostics.push(Diagnostic::error(
                "DOCX_REQUIRED_PART_MISSING",
                Some(required),
                "required OPC/DOCX part is missing",
            ));
        }
    }

    // Do not decompress a package whose container metadata has already failed
    // policy checks.  In particular, overlapping entries must never be read.
    if !inspection.is_acceptable() {
        return Ok(inspection);
    }

    // Fully consume every normal member.  zip verifies decompression and CRC at
    // EOF; declared aggregate/per-entry limits above bound this work.
    for part in &inspection.parts {
        if part.is_directory {
            continue;
        }
        let mut file = archive.by_index(part.index)?;
        let copied = io::copy(&mut file, &mut io::sink())?;
        if copied != part.uncompressed_size {
            return Err(DocxError::Zip(zip::result::ZipError::InvalidArchive(
                "uncompressed size does not match the central directory".into(),
            )));
        }
    }

    let content_types = read_part_limited(archive, CONTENT_TYPES_PART, limits.max_xml_part_bytes)?;
    match parse_content_types(&content_types, limits) {
        Ok(kind) => inspection.package_kind = kind,
        Err(error) => inspection.diagnostics.push(Diagnostic::error(
            "DOCX_CONTENT_TYPES_INVALID",
            Some(CONTENT_TYPES_PART),
            error.to_string(),
        )),
    }

    let relationship_parts = inspection
        .parts
        .iter()
        .filter(|part| !part.is_directory && part.name.to_ascii_lowercase().ends_with(".rels"))
        .map(|part| part.name.clone())
        .collect::<Vec<_>>();
    for part_name in relationship_parts {
        let rels = read_part_limited(archive, &part_name, limits.max_xml_part_bytes)?;
        if let Err(error) =
            scan_relationships(&rels, &part_name, limits, &mut inspection.diagnostics)
        {
            inspection.diagnostics.push(Diagnostic::error(
                "DOCX_RELATIONSHIPS_INVALID",
                Some(part_name),
                error.to_string(),
            ));
        }
    }

    let document = read_part_limited(archive, DOCUMENT_PART, limits.max_xml_part_bytes)?;
    if let Err(error) = scan_document_security(&document, limits, &mut inspection.diagnostics) {
        inspection.diagnostics.push(Diagnostic::error(
            "DOCX_DOCUMENT_XML_INVALID",
            Some(DOCUMENT_PART),
            error.to_string(),
        ));
    }

    Ok(inspection)
}

pub(crate) fn read_part_limited<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    part_name: &str,
    limit: u64,
) -> DocxResult<Vec<u8>> {
    let mut file = archive.by_name(part_name)?;
    if file.size() > limit {
        return Err(DocxError::LimitExceeded {
            resource: part_name.to_owned(),
            limit,
            actual: file.size(),
        });
    }
    let capacity = usize::try_from(file.size()).map_err(|_| DocxError::LimitExceeded {
        resource: part_name.to_owned(),
        limit,
        actual: file.size(),
    })?;
    let mut bytes = Vec::with_capacity(capacity);
    let copied = (&mut file)
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)? as u64;
    if copied > limit {
        return Err(DocxError::LimitExceeded {
            resource: part_name.to_owned(),
            limit,
            actual: copied,
        });
    }
    Ok(bytes)
}

pub(crate) fn archive_size_diagnostic(
    archive_bytes: u64,
    limits: &DocxLimits,
) -> Option<Diagnostic> {
    (archive_bytes > limits.max_archive_bytes).then(|| {
        Diagnostic::error(
            "DOCX_ARCHIVE_TOO_LARGE",
            None::<String>,
            format!(
                "package is {archive_bytes} bytes; the configured limit is {} bytes",
                limits.max_archive_bytes
            ),
        )
    })
}

fn invalid_part_name<R: Read + ?Sized>(file: &zip::read::ZipFile<'_, R>) -> Option<String> {
    let name = file.name();
    if file.enclosed_name().is_none() {
        return Some("part name is absolute, contains NUL, or escapes the package root".to_owned());
    }
    if std::str::from_utf8(file.name_raw()).is_err() {
        return Some("part name is not UTF-8".to_owned());
    }
    if name.is_empty() || name.starts_with('/') || name.starts_with('\\') {
        return Some("part name is empty or absolute".to_owned());
    }
    if name.contains('\\') || name.contains(':') || name.contains('?') || name.contains('#') {
        return Some("part name contains a forbidden URI/path character".to_owned());
    }
    let lower = name.to_ascii_lowercase();
    if lower.contains("%00") || lower.contains("%2f") || lower.contains("%5c") {
        return Some("part name contains an encoded path separator or NUL".to_owned());
    }
    let trimmed = name.trim_end_matches('/');
    if trimmed.is_empty()
        || trimmed
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Some("part name contains an empty, '.' or '..' segment".to_owned());
    }
    None
}

fn active_part_reason(name: &str, limits: &DocxLimits) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with("vbaproject.bin") || lower.contains("/vba/") {
        Some("VBA macro projects are not allowed")
    } else if lower.starts_with("word/activex/") || lower.contains("/activex/") {
        Some("ActiveX controls are not allowed")
    } else if (lower.starts_with("word/embeddings/") || lower.contains("/embeddings/"))
        && !limits.allow_mathtype_ole
    {
        Some("embedded OLE/package objects are not allowed")
    } else if lower.starts_with("_xmlsignatures/") || lower.ends_with("origin.sigs") {
        Some("digital-signature package parts are not preserved by this workflow")
    } else {
        None
    }
}

pub(crate) fn parse_content_types(xml: &[u8], limits: &DocxLimits) -> DocxResult<PackageKind> {
    validate_safe_xml(xml, CONTENT_TYPES_PART, limits)?;
    let mut reader = NsReader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut main_type = None;
    loop {
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|error| DocxError::xml(CONTENT_TYPES_PART, reader.error_position(), error))?;
        match event {
            Event::Start(ref start) | Event::Empty(ref start) => {
                let namespace = {
                    let (namespace, _) = reader.resolver().resolve_element(start.name());
                    classify_namespace(namespace)
                };
                let local = local_name(start);
                if namespace == NamespaceKind::ContentTypes
                    && (local == b"Override" || local == b"Default")
                {
                    let content_type =
                        attribute_value(&reader, start, b"ContentType", None, CONTENT_TYPES_PART)?;
                    if content_type
                        .as_deref()
                        .is_some_and(|value| is_active_content_type(value, limits))
                    {
                        return Err(DocxError::xml(
                            CONTENT_TYPES_PART,
                            reader.buffer_position(),
                            format!(
                                "active or macro-enabled content type is not allowed: {}",
                                content_type.unwrap_or_default()
                            ),
                        ));
                    }
                    if local == b"Override" {
                        let part_name =
                            attribute_value(&reader, start, b"PartName", None, CONTENT_TYPES_PART)?;
                        if part_name.as_deref() == Some("/word/document.xml") {
                            main_type = content_type;
                        }
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }

    match main_type.as_deref() {
        Some(DOCUMENT_CONTENT_TYPE) => Ok(PackageKind::Document),
        Some(TEMPLATE_CONTENT_TYPE) => Ok(PackageKind::Template),
        Some(other) => Err(DocxError::xml(
            CONTENT_TYPES_PART,
            0,
            format!("unsupported main document content type: {other}"),
        )),
        None => Err(DocxError::xml(
            CONTENT_TYPES_PART,
            0,
            "missing content type override for /word/document.xml",
        )),
    }
}

fn is_active_content_type(content_type: &str, limits: &DocxLimits) -> bool {
    let lower = content_type.to_ascii_lowercase();
    lower.contains("macroenabled")
        || lower.contains("vbaproject")
        || lower.contains("activex")
        || (lower.contains("oleobject") && !limits.allow_mathtype_ole)
}

pub(crate) fn scan_relationships(
    xml: &[u8],
    part_name: &str,
    limits: &DocxLimits,
    diagnostics: &mut Vec<Diagnostic>,
) -> DocxResult<()> {
    validate_safe_xml(xml, part_name, limits)?;
    let mut reader = NsReader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut root_office_document = false;
    loop {
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|error| DocxError::xml(part_name, reader.error_position(), error))?;
        if let Event::Start(ref start) | Event::Empty(ref start) = event {
            let namespace = {
                let (namespace, _) = reader.resolver().resolve_element(start.name());
                classify_namespace(namespace)
            };
            if namespace == NamespaceKind::Relationships && local_name(start) == b"Relationship" {
                let relationship_type =
                    attribute_value(&reader, start, b"Type", None, part_name)?.unwrap_or_default();
                let target = attribute_value(&reader, start, b"Target", None, part_name)?
                    .unwrap_or_default();
                let target_mode = attribute_value(&reader, start, b"TargetMode", None, part_name)?;
                let lower_type = relationship_type.to_ascii_lowercase();

                if part_name == ROOT_RELS_PART && lower_type.ends_with("/officedocument") {
                    root_office_document = target.trim_start_matches('/') == DOCUMENT_PART
                        && !target_mode
                            .as_deref()
                            .is_some_and(|mode| mode.eq_ignore_ascii_case("external"));
                }

                if target_mode
                    .as_deref()
                    .is_some_and(|mode| mode.eq_ignore_ascii_case("external"))
                {
                    diagnostics.push(Diagnostic::error(
                        "DOCX_EXTERNAL_RELATIONSHIP",
                        Some(part_name),
                        format!("external target is not allowed: {target}"),
                    ));
                }
                let forbidden_relationship = [
                    "/package",
                    "/afchunk",
                    "/attachedtemplate",
                    "/vbaproject",
                    "/control",
                ]
                .iter()
                .any(|suffix| lower_type.ends_with(suffix))
                    || (lower_type.ends_with("/oleobject") && !limits.allow_mathtype_ole);
                if forbidden_relationship {
                    diagnostics.push(Diagnostic::error(
                        "DOCX_ACTIVE_RELATIONSHIP",
                        Some(part_name),
                        format!("active relationship type is not allowed: {relationship_type}"),
                    ));
                }
            }
        }
        if matches!(event, Event::Eof) {
            break;
        }
        buffer.clear();
    }

    if part_name == ROOT_RELS_PART && !root_office_document {
        diagnostics.push(Diagnostic::error(
            "DOCX_OFFICE_DOCUMENT_RELATIONSHIP_MISSING",
            Some(part_name),
            "root relationships do not point to word/document.xml",
        ));
    }
    Ok(())
}

fn scan_document_security(
    xml: &[u8],
    limits: &DocxLimits,
    diagnostics: &mut Vec<Diagnostic>,
) -> DocxResult<()> {
    validate_safe_xml(xml, DOCUMENT_PART, limits)?;
    let mut reader = NsReader::from_reader(xml);
    let mut buffer = Vec::new();
    loop {
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|error| DocxError::xml(DOCUMENT_PART, reader.error_position(), error))?;
        if let Event::Start(ref start) | Event::Empty(ref start) = event {
            let namespace = {
                let (namespace, _) = reader.resolver().resolve_element(start.name());
                classify_namespace(namespace)
            };
            if namespace == NamespaceKind::Word && local_name(start) == b"altChunk" {
                diagnostics.push(Diagnostic::error(
                    "DOCX_ALT_CHUNK",
                    Some(DOCUMENT_PART),
                    "altChunk can import external HTML or document content and is not allowed",
                ));
            }
        }
        if matches!(event, Event::Eof) {
            break;
        }
        buffer.clear();
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::io::{Cursor, Write};

    use zip::{ZipWriter, write::SimpleFileOptions};

    pub(crate) const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#;

    pub(crate) const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

    pub(crate) fn minimal_docx(document_xml: &str) -> Vec<u8> {
        package_with_entries(&[
            ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
            ("_rels/.rels", ROOT_RELS.as_bytes()),
            ("word/document.xml", document_xml.as_bytes()),
            ("word/styles.xml", b"<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"/>"),
        ])
    }

    pub(crate) fn package_with_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut writer = ZipWriter::new(cursor);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in entries {
            writer.start_file(*name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    pub(crate) fn rename_zip_entry(bytes: &mut [u8], from: &str, to: &str) {
        assert_eq!(
            from.len(),
            to.len(),
            "ZIP entry names must have equal lengths for an in-place test mutation"
        );

        let offsets = bytes
            .windows(from.len())
            .enumerate()
            .filter_map(|(offset, window)| (window == from.as_bytes()).then_some(offset))
            .collect::<Vec<_>>();
        assert_eq!(
            offsets.len(),
            2,
            "expected the entry name once in its local header and once in the central directory"
        );

        for offset in offsets {
            bytes[offset..offset + to.len()].copy_from_slice(to.as_bytes());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{test_support::*, *};

    const DOCUMENT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body><w:p><w:r><w:t>Hello</w:t></w:r></w:p></w:body>
</w:document>"#;

    #[test]
    fn accepts_a_minimal_bounded_docx() {
        let bytes = minimal_docx(DOCUMENT);
        let result = inspect_docx(Cursor::new(bytes), &DocxLimits::default()).unwrap();
        assert!(result.is_acceptable(), "{:?}", result.diagnostics);
        assert_eq!(result.package_kind, PackageKind::Document);
        assert_eq!(result.parts.len(), 4);
    }

    #[test]
    fn rejects_traversal_names_before_decompression() {
        let bytes = package_with_entries(&[
            ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
            ("_rels/.rels", ROOT_RELS.as_bytes()),
            ("word/document.xml", DOCUMENT.as_bytes()),
            ("../../outside.bin", b"bad"),
        ]);
        let result = inspect_docx(Cursor::new(bytes), &DocxLimits::default()).unwrap();
        assert!(!result.is_acceptable());
        assert!(
            result
                .diagnostics
                .iter()
                .any(|item| item.code == "DOCX_UNSAFE_PART_NAME")
        );
    }

    #[test]
    fn reports_declared_size_limits() {
        let bytes = minimal_docx(DOCUMENT);
        let limits = DocxLimits {
            max_xml_part_bytes: 8,
            ..DocxLimits::default()
        };
        let result = inspect_docx(Cursor::new(bytes), &limits).unwrap();
        assert!(
            result
                .diagnostics
                .iter()
                .any(|item| item.code == "DOCX_PART_TOO_LARGE")
        );
    }

    #[test]
    fn rejects_duplicate_and_active_content_parts() {
        let mut bytes = package_with_entries(&[
            ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
            ("_rels/.rels", ROOT_RELS.as_bytes()),
            ("word/document.xml", DOCUMENT.as_bytes()),
            ("word/styles-1.xml", b"<styles/>"),
            ("word/styles-2.xml", b"<duplicate/>"),
            ("word/vbaProject.bin", b"macro"),
        ]);
        rename_zip_entry(&mut bytes, "word/styles-2.xml", "word/styles-1.xml");

        let result = inspect_docx(Cursor::new(bytes), &DocxLimits::default()).unwrap();
        assert!(
            result
                .diagnostics
                .iter()
                .any(|item| item.code == "DOCX_DUPLICATE_PART")
        );
        assert!(
            result
                .diagnostics
                .iter()
                .any(|item| item.code == "DOCX_ACTIVE_CONTENT_PART")
        );
    }
}
