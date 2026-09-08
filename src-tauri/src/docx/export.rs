use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Seek, SeekFrom, Write},
};

use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

use super::{
    Diagnostic, DocxError, DocxLimits, DocxResult,
    document::parse_document_xml,
    package::{
        archive_size_diagnostic, inspect_archive_with_size, parse_content_types, scan_relationships,
    },
    xml::validate_safe_xml,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExportSummary {
    pub input_parts: usize,
    pub raw_copied_parts: usize,
    pub rewritten_parts: Vec<String>,
    pub output_bytes: u64,
}

/// Finished output and its deterministic copy/rewrite summary.
pub struct RawCopyExport<W> {
    pub output: W,
    pub summary: ExportSummary,
}

/// Builds a new OPC package in original entry order.  Entries absent from
/// `replacements` are copied in their already-compressed form; replacement
/// entries reuse the source metadata and compression method.
///
/// The source is fully inspected and CRC-checked before the first output byte
/// is written.  This function is intentionally a part-replacement skeleton:
/// generation of new WordprocessingML is a separate responsibility.
pub fn raw_copy_with_replacements<R, W>(
    mut source: R,
    target: W,
    replacements: &BTreeMap<String, Vec<u8>>,
    limits: &DocxLimits,
) -> DocxResult<RawCopyExport<W>>
where
    R: Read + Seek,
    W: Write + Seek,
{
    raw_copy_with_replacements_and_removals(
        &mut source,
        target,
        replacements,
        &BTreeSet::new(),
        limits,
    )
}

/// Builds a new OPC package while replacing existing parts and adding new
/// bounded parts such as managed images and a missing document relationship
/// part. Additions may never shadow a source package member.
pub fn raw_copy_with_replacements_and_additions<R, W>(
    mut source: R,
    target: W,
    replacements: &BTreeMap<String, Vec<u8>>,
    additions: &BTreeMap<String, Vec<u8>>,
    limits: &DocxLimits,
) -> DocxResult<RawCopyExport<W>>
where
    R: Read + Seek,
    W: Write + Seek,
{
    raw_copy_with_part_edits(
        &mut source,
        target,
        replacements,
        &BTreeSet::new(),
        additions,
        limits,
    )
}

/// Builds a new OPC package while replacing selected parts and omitting
/// explicitly named parts. This is used by the template sanitizer after every
/// embedded MathType object has been proven to belong to the removed question
/// region.
pub fn raw_copy_with_replacements_and_removals<R, W>(
    mut source: R,
    target: W,
    replacements: &BTreeMap<String, Vec<u8>>,
    removals: &BTreeSet<String>,
    limits: &DocxLimits,
) -> DocxResult<RawCopyExport<W>>
where
    R: Read + Seek,
    W: Write + Seek,
{
    raw_copy_with_part_edits(
        &mut source,
        target,
        replacements,
        removals,
        &BTreeMap::new(),
        limits,
    )
}

fn raw_copy_with_part_edits<R, W>(
    mut source: R,
    target: W,
    replacements: &BTreeMap<String, Vec<u8>>,
    removals: &BTreeSet<String>,
    additions: &BTreeMap<String, Vec<u8>>,
    limits: &DocxLimits,
) -> DocxResult<RawCopyExport<W>>
where
    R: Read + Seek,
    W: Write + Seek,
{
    let archive_bytes = source.seek(SeekFrom::End(0))?;
    if let Some(diagnostic) = archive_size_diagnostic(archive_bytes, limits) {
        return Err(DocxError::rejected(vec![diagnostic]));
    }
    source.seek(SeekFrom::Start(0))?;
    let mut archive = ZipArchive::new(source)?;
    let inspection = inspect_archive_with_size(&mut archive, Some(archive_bytes), limits)?;
    inspection.ensure_acceptable()?;

    let source_names = inspection
        .parts
        .iter()
        .map(|part| (part.name.as_str(), part))
        .collect::<BTreeMap<_, _>>();
    let mut replacement_diagnostics = Vec::new();
    for part_name in removals {
        let Some(_) = source_names.get(part_name.as_str()) else {
            replacement_diagnostics.push(Diagnostic::error(
                "DOCX_EXPORT_REMOVAL_PART_NOT_FOUND",
                Some(part_name),
                "removal refers to a part that is absent from the source package",
            ));
            continue;
        };
        if replacements.contains_key(part_name) {
            replacement_diagnostics.push(Diagnostic::error(
                "DOCX_EXPORT_PART_EDIT_CONFLICT",
                Some(part_name),
                "the same package part cannot be both replaced and removed",
            ));
        }
    }
    for (part_name, bytes) in replacements {
        let Some(source_part) = source_names.get(part_name.as_str()) else {
            replacement_diagnostics.push(Diagnostic::error(
                "DOCX_EXPORT_PART_NOT_FOUND",
                Some(part_name),
                "replacement refers to a part that is absent from the source package",
            ));
            continue;
        };
        if source_part.is_directory {
            replacement_diagnostics.push(Diagnostic::error(
                "DOCX_EXPORT_DIRECTORY_REPLACEMENT",
                Some(part_name),
                "a ZIP directory entry cannot be replaced with file bytes",
            ));
            continue;
        }
        let limit = limits.part_limit(part_name);
        if bytes.len() as u64 > limit {
            replacement_diagnostics.push(Diagnostic::error(
                "DOCX_EXPORT_REPLACEMENT_TOO_LARGE",
                Some(part_name),
                format!(
                    "replacement is {} bytes; this part is limited to {limit} bytes",
                    bytes.len()
                ),
            ));
            continue;
        }
        let lower = part_name.to_ascii_lowercase();
        let validation = if part_name == "[Content_Types].xml" {
            parse_content_types(bytes, limits).map(|_| ())
        } else if lower.ends_with(".rels") {
            let mut diagnostics = Vec::new();
            scan_relationships(bytes, part_name, limits, &mut diagnostics).and_then(|_| {
                if diagnostics
                    .iter()
                    .any(|item| item.severity == super::DiagnosticSeverity::Error)
                {
                    Err(DocxError::rejected(diagnostics))
                } else {
                    Ok(())
                }
            })
        } else if part_name == "word/document.xml" {
            parse_document_xml(bytes, limits).map(|_| ())
        } else if lower.ends_with(".xml") {
            validate_safe_xml(bytes, part_name, limits)
        } else {
            Ok(())
        };
        if let Err(error) = validation {
            replacement_diagnostics.push(Diagnostic::error(
                "DOCX_EXPORT_REPLACEMENT_INVALID",
                Some(part_name),
                error.to_string(),
            ));
        }
    }
    if inspection.parts.len().saturating_add(additions.len()) > limits.max_entries {
        replacement_diagnostics.push(Diagnostic::error(
            "DOCX_EXPORT_TOO_MANY_PARTS",
            None::<String>,
            "adding generated package parts would exceed the configured ZIP entry limit",
        ));
    }
    for (part_name, bytes) in additions {
        if source_names.contains_key(part_name.as_str()) {
            replacement_diagnostics.push(Diagnostic::error(
                "DOCX_EXPORT_ADDITION_ALREADY_EXISTS",
                Some(part_name),
                "an added package part must not shadow a source package member",
            ));
            continue;
        }
        if !safe_added_part_name(part_name) {
            replacement_diagnostics.push(Diagnostic::error(
                "DOCX_EXPORT_ADDITION_NAME_INVALID",
                Some(part_name),
                "added package part has an unsafe or invalid name",
            ));
            continue;
        }
        if bytes.len() as u64 > limits.part_limit(part_name) {
            replacement_diagnostics.push(Diagnostic::error(
                "DOCX_EXPORT_ADDITION_TOO_LARGE",
                Some(part_name),
                "added package part exceeds its configured size limit",
            ));
            continue;
        }
        if let Err(error) = validate_generated_part(part_name, bytes, limits) {
            replacement_diagnostics.push(Diagnostic::error(
                "DOCX_EXPORT_ADDITION_INVALID",
                Some(part_name),
                error.to_string(),
            ));
        }
    }
    if !replacement_diagnostics.is_empty() {
        return Err(DocxError::rejected(replacement_diagnostics));
    }

    let edited_part_names = replacements
        .keys()
        .chain(additions.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut writer = ZipWriter::new(target);
    let mut summary = ExportSummary {
        input_parts: inspection.parts.len(),
        ..ExportSummary::default()
    };

    for part in &inspection.parts {
        if removals.contains(&part.name) {
            continue;
        } else if let Some(replacement) = replacements.get(&part.name) {
            let source_file = archive.by_index_raw(part.index)?;
            let options = source_file.options();
            let name = source_file.name().to_owned();
            drop(source_file);
            writer.start_file(name, options)?;
            writer.write_all(replacement)?;
            summary.rewritten_parts.push(part.name.clone());
        } else {
            let source_file = archive.by_index_raw(part.index)?;
            writer.raw_copy_file(source_file)?;
            summary.raw_copied_parts += 1;
        }
    }
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (part_name, bytes) in additions {
        writer.start_file(part_name, options)?;
        writer.write_all(bytes)?;
        summary.rewritten_parts.push(part_name.clone());
    }

    debug_assert_eq!(
        edited_part_names,
        summary.rewritten_parts.iter().cloned().collect()
    );
    let mut output = writer.finish()?;
    summary.output_bytes = output.seek(SeekFrom::End(0))?;
    Ok(RawCopyExport { output, summary })
}

fn validate_generated_part(part_name: &str, bytes: &[u8], limits: &DocxLimits) -> DocxResult<()> {
    let lower = part_name.to_ascii_lowercase();
    if part_name == "[Content_Types].xml" {
        parse_content_types(bytes, limits).map(|_| ())
    } else if lower.ends_with(".rels") {
        let mut diagnostics = Vec::new();
        scan_relationships(bytes, part_name, limits, &mut diagnostics)?;
        if diagnostics
            .iter()
            .any(|item| item.severity == super::DiagnosticSeverity::Error)
        {
            Err(DocxError::rejected(diagnostics))
        } else {
            Ok(())
        }
    } else if lower.ends_with(".xml") {
        validate_safe_xml(bytes, part_name, limits)
    } else {
        Ok(())
    }
}

fn safe_added_part_name(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && !value.contains('\\')
        && !value.contains('\0')
        && !value
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read};

    use super::super::package::test_support::minimal_docx;
    use super::*;

    const ORIGINAL_DOCUMENT: &str = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>old</w:t></w:r></w:p></w:body></w:document>"#;
    const NEW_DOCUMENT: &str = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>new</w:t></w:r></w:p></w:body></w:document>"#;

    #[test]
    fn rewrites_selected_part_and_raw_copies_the_rest() {
        let source = minimal_docx(ORIGINAL_DOCUMENT);
        let mut replacements = BTreeMap::new();
        replacements.insert(
            "word/document.xml".to_owned(),
            NEW_DOCUMENT.as_bytes().to_vec(),
        );

        let exported = raw_copy_with_replacements(
            Cursor::new(source.clone()),
            Cursor::new(Vec::new()),
            &replacements,
            &DocxLimits::default(),
        )
        .unwrap();
        assert_eq!(exported.summary.rewritten_parts, vec!["word/document.xml"]);
        assert_eq!(exported.summary.raw_copied_parts, 3);

        let mut output_archive =
            ZipArchive::new(Cursor::new(exported.output.into_inner())).unwrap();
        let mut document = String::new();
        output_archive
            .by_name("word/document.xml")
            .unwrap()
            .read_to_string(&mut document)
            .unwrap();
        assert_eq!(document, NEW_DOCUMENT);

        let mut source_archive = ZipArchive::new(Cursor::new(source)).unwrap();
        let source_styles = source_archive.by_name("word/styles.xml").unwrap();
        let output_styles = output_archive.by_name("word/styles.xml").unwrap();
        assert_eq!(source_styles.crc32(), output_styles.crc32());
        assert_eq!(
            source_styles.compressed_size(),
            output_styles.compressed_size()
        );
        assert_eq!(source_styles.compression(), output_styles.compression());
    }

    #[test]
    fn validates_all_replacement_names_before_writing() {
        let source = minimal_docx(ORIGINAL_DOCUMENT);
        let mut replacements = BTreeMap::new();
        replacements.insert("word/missing.xml".to_owned(), b"<x/>".to_vec());
        let result = raw_copy_with_replacements(
            Cursor::new(source),
            Cursor::new(Vec::new()),
            &replacements,
            &DocxLimits::default(),
        );
        assert!(matches!(result, Err(DocxError::Rejected { .. })));
    }

    #[test]
    fn adds_bounded_generated_parts_without_shadowing_source_members() {
        let source = minimal_docx(ORIGINAL_DOCUMENT);
        let additions = BTreeMap::from([(
            "word/media/zhitiku-image-1.png".to_owned(),
            vec![0x89, b'P', b'N', b'G'],
        )]);
        let exported = raw_copy_with_replacements_and_additions(
            Cursor::new(source),
            Cursor::new(Vec::new()),
            &BTreeMap::new(),
            &additions,
            &DocxLimits::default(),
        )
        .unwrap();

        assert_eq!(
            exported.summary.rewritten_parts,
            vec!["word/media/zhitiku-image-1.png"]
        );
        let mut output_archive =
            ZipArchive::new(Cursor::new(exported.output.into_inner())).unwrap();
        let mut image = Vec::new();
        output_archive
            .by_name("word/media/zhitiku-image-1.png")
            .unwrap()
            .read_to_end(&mut image)
            .unwrap();
        assert_eq!(image, vec![0x89, b'P', b'N', b'G']);
    }

    #[test]
    fn omits_explicitly_removed_parts() {
        let source = minimal_docx(ORIGINAL_DOCUMENT);
        let removals = BTreeSet::from(["word/styles.xml".to_owned()]);
        let exported = raw_copy_with_replacements_and_removals(
            Cursor::new(source),
            Cursor::new(Vec::new()),
            &BTreeMap::new(),
            &removals,
            &DocxLimits::default(),
        )
        .unwrap();

        let mut output_archive =
            ZipArchive::new(Cursor::new(exported.output.into_inner())).unwrap();
        assert!(output_archive.by_name("word/styles.xml").is_err());
        assert!(output_archive.by_name("word/document.xml").is_ok());
    }

    #[test]
    fn rejects_conflicting_replace_and_remove_requests() {
        let source = minimal_docx(ORIGINAL_DOCUMENT);
        let replacements = BTreeMap::from([(
            "word/document.xml".to_owned(),
            NEW_DOCUMENT.as_bytes().to_vec(),
        )]);
        let removals = BTreeSet::from(["word/document.xml".to_owned()]);
        let result = raw_copy_with_replacements_and_removals(
            Cursor::new(source),
            Cursor::new(Vec::new()),
            &replacements,
            &removals,
            &DocxLimits::default(),
        );
        assert!(matches!(result, Err(DocxError::Rejected { .. })));
    }
}
