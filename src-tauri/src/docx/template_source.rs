//! Detach inherited Word templates from an import copy without opening their targets.

use std::{
    collections::BTreeSet,
    io::{Cursor, Write},
};

use quick_xml::{
    events::{BytesStart, Event},
    reader::NsReader,
};
use zip::{ZipArchive, ZipWriter};

use super::{
    DiagnosticSeverity, DocxError, DocxLimits, DocxResult, inspect_docx,
    package::{read_part_limited, scan_relationships},
    xml::{NamespaceKind, attribute_value, classify_namespace, validate_safe_xml},
};

const SETTINGS: &str = "word/settings.xml";
const SETTINGS_RELS: &str = "word/_rels/settings.xml.rels";
const ATTACHED_TEMPLATE_TYPES: [&str; 2] = [
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/attachedTemplate",
    "http://purl.oclc.org/ooxml/officeDocument/relationships/attachedTemplate",
];

/// Return the validated bytes and whether an external template was detached.
/// No general inspection/export policy is relaxed: only settings relationships
/// are eligible for repair, and the remainder and final package must pass the
/// original checks before any managed file may be created.
pub(crate) fn prepare_template_source(
    bytes: Vec<u8>,
    limits: &DocxLimits,
) -> DocxResult<(Vec<u8>, bool)> {
    let inspection = inspect_docx(Cursor::new(&bytes), limits)?;
    if inspection.is_acceptable() {
        return Ok((bytes, false));
    }
    let repairable_only = inspection.diagnostics.iter().all(|diagnostic| {
        diagnostic.severity != DiagnosticSeverity::Error
            || (diagnostic.part_name.as_deref() == Some(SETTINGS_RELS)
                && matches!(
                    diagnostic.code,
                    "DOCX_EXTERNAL_RELATIONSHIP" | "DOCX_ACTIVE_RELATIONSHIP"
                ))
    });
    if !repairable_only || !inspection.parts.iter().any(|part| part.name == SETTINGS) {
        return Err(DocxError::rejected(inspection.diagnostics));
    }

    // Reaching here means container limits, member CRCs and all other parts
    // have passed. Still use bounded reads for both XML parts being changed.
    let mut archive = ZipArchive::new(Cursor::new(&bytes))?;
    let relationships = read_part_limited(&mut archive, SETTINGS_RELS, limits.max_xml_part_bytes)?;
    let mut ids = BTreeSet::new();
    let mut detached_ids = BTreeSet::new();
    let relationships = remove_children(
        &relationships,
        SETTINGS_RELS,
        (NamespaceKind::Relationships, b"Relationships"),
        limits,
        |reader, start| {
            if !element_is(reader, start, NamespaceKind::Relationships, b"Relationship") {
                return Ok(false);
            }
            let id = attribute_value(
                reader,
                start,
                b"Id",
                Some(NamespaceKind::Unbound),
                SETTINGS_RELS,
            )?
            .filter(|id| !id.is_empty())
            .ok_or_else(|| invalid_settings(SETTINGS_RELS, "模板关系缺少标识。"))?;
            if !ids.insert(id.clone()) {
                return Err(invalid_settings(
                    SETTINGS_RELS,
                    "模板关系标识重复，无法安全解除关联。",
                ));
            }
            let kind = attribute_value(
                reader,
                start,
                b"Type",
                Some(NamespaceKind::Unbound),
                SETTINGS_RELS,
            )?;
            let mode = attribute_value(
                reader,
                start,
                b"TargetMode",
                Some(NamespaceKind::Unbound),
                SETTINGS_RELS,
            )?;
            let detach = kind
                .as_deref()
                .is_some_and(|kind| ATTACHED_TEMPLATE_TYPES.contains(&kind))
                && mode
                    .as_deref()
                    .is_some_and(|mode| mode.eq_ignore_ascii_case("external"));
            if detach {
                detached_ids.insert(id);
            }
            Ok(detach)
        },
    )?;
    if detached_ids.is_empty() {
        return Err(DocxError::rejected(inspection.diagnostics));
    }

    // An attached template must not mask another external/active relationship
    // in this same part. Such a document is still rejected.
    let mut remaining_diagnostics = Vec::new();
    scan_relationships(
        &relationships,
        SETTINGS_RELS,
        limits,
        &mut remaining_diagnostics,
    )?;
    if remaining_diagnostics
        .iter()
        .any(|item| item.severity == DiagnosticSeverity::Error)
    {
        return Err(DocxError::rejected(remaining_diagnostics));
    }

    let settings = read_part_limited(&mut archive, SETTINGS, limits.max_xml_part_bytes)?;
    let settings = remove_children(
        &settings,
        SETTINGS,
        (NamespaceKind::Word, b"settings"),
        limits,
        |reader, start| {
            if element_is(reader, start, NamespaceKind::Word, b"attachedTemplate") {
                let id = attribute_value(
                    reader,
                    start,
                    b"id",
                    Some(NamespaceKind::OfficeRelationships),
                    SETTINGS,
                )?;
                if !id.as_ref().is_some_and(|id| detached_ids.contains(id)) {
                    return Err(invalid_settings(
                        SETTINGS,
                        "附加模板引用与关系记录不匹配，无法安全解除关联。",
                    ));
                }
                return Ok(true);
            }
            // Do not keep automatic style updates from a now-detached template.
            Ok(element_is(
                reader,
                start,
                NamespaceKind::Word,
                b"linkStyles",
            ))
        },
    )?;

    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for part in &inspection.parts {
        let replacement = match part.name.as_str() {
            SETTINGS => Some(&settings),
            SETTINGS_RELS => Some(&relationships),
            _ => None,
        };
        let file = archive.by_index_raw(part.index)?;
        if let Some(replacement) = replacement {
            writer.start_file(&part.name, file.options())?;
            writer.write_all(replacement)?;
        } else {
            writer.raw_copy_file(file)?;
        }
    }
    let output = writer.finish()?.into_inner();
    inspect_docx(Cursor::new(&output), limits)?.ensure_acceptable()?;
    Ok((output, true))
}

fn invalid_settings(part: &str, message: &str) -> DocxError {
    DocxError::xml(part, 0, message)
}

fn element_is(
    reader: &NsReader<&[u8]>,
    start: &BytesStart<'_>,
    namespace: NamespaceKind,
    name: &[u8],
) -> bool {
    let (resolved, _) = reader.resolver().resolve_element(start.name());
    classify_namespace(resolved) == namespace && start.local_name().as_ref() == name
}

/// Remove selected direct children by byte span, preserving every other byte
/// (including namespace declarations, comments and compatibility extensions).
fn remove_children(
    xml: &[u8],
    part: &str,
    root: (NamespaceKind, &[u8]),
    limits: &DocxLimits,
    mut remove: impl FnMut(&NsReader<&[u8]>, &BytesStart<'_>) -> DocxResult<bool>,
) -> DocxResult<Vec<u8>> {
    validate_safe_xml(xml, part, limits)?;
    let mut reader = NsReader::from_reader(xml);
    let mut buffer = Vec::new();
    let mut depth = 0;
    let mut seen_root = false;
    let mut removal_start = None;
    let mut ranges = Vec::new();
    loop {
        let start_offset = reader.buffer_position() as usize;
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|error| DocxError::xml(part, reader.error_position(), error))?;
        match event {
            Event::Start(ref start) | Event::Empty(ref start) => {
                if removal_start.is_some() {
                    return Err(invalid_settings(
                        part,
                        "历史模板关联节点包含不支持的子节点，无法安全解除关联。",
                    ));
                }
                if depth == 0 {
                    if seen_root || !element_is(&reader, start, root.0, root.1) {
                        return Err(invalid_settings(part, "模板设置 XML 根节点无效。"));
                    }
                    seen_root = true;
                } else if depth == 1 && remove(&reader, start)? {
                    if matches!(event, Event::Empty(_)) {
                        ranges.push(start_offset..reader.buffer_position() as usize);
                    } else {
                        removal_start = Some(start_offset);
                    }
                }
                if matches!(event, Event::Start(_)) {
                    depth += 1;
                }
            }
            Event::End(_) => {
                depth -= 1;
                if depth == 1
                    && let Some(start) = removal_start.take()
                {
                    ranges.push(start..reader.buffer_position() as usize);
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if !seen_root || depth != 0 {
        return Err(invalid_settings(part, "模板设置 XML 不完整。"));
    }
    let mut output = Vec::with_capacity(xml.len());
    let mut position = 0;
    for range in ranges {
        output.extend_from_slice(&xml[position..range.start]);
        position = range.end;
    }
    output.extend_from_slice(&xml[position..]);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docx::package::test_support::{
        CONTENT_TYPES, ROOT_RELS, minimal_docx, package_with_entries, rename_zip_entry,
    };

    const DOCUMENT: &str = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>试题</w:t></w:r></w:p></w:body></w:document>"#;
    const SETTINGS_XML: &str = r#"<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><!--保留--><w:attachedTemplate r:id="old"/><w:linkStyles/><w:zoom w:percent="125"/></w:settings>"#;
    const ATTACHED: &str = r#"<Relationship Id="old" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/attachedTemplate" Target="file:///D:/old.dot" TargetMode="External"/>"#;
    const HEADER: &[u8] = br#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p/></w:hdr>"#;

    fn fixture(settings: &str, relationships: &str, extra: &[(&str, &[u8])]) -> Vec<u8> {
        let rels = format!(
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{relationships}</Relationships>"#
        );
        let mut parts = vec![
            ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
            ("_rels/.rels", ROOT_RELS.as_bytes()),
            ("word/document.xml", DOCUMENT.as_bytes()),
            (SETTINGS, settings.as_bytes()),
            (SETTINGS_RELS, rels.as_bytes()),
            ("word/header1.xml", HEADER),
        ];
        parts.extend_from_slice(extra);
        package_with_entries(&parts)
    }

    fn read_part(bytes: &[u8], name: &str) -> Vec<u8> {
        read_part_limited(
            &mut ZipArchive::new(Cursor::new(bytes)).unwrap(),
            name,
            1_000_000,
        )
        .unwrap()
    }

    fn error_codes(result: DocxResult<(Vec<u8>, bool)>) -> Vec<&'static str> {
        let DocxError::Rejected { diagnostics } = result.unwrap_err() else {
            panic!("expected package diagnostics");
        };
        diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.code)
            .collect()
    }

    #[test]
    fn detaches_only_the_external_template_and_preserves_every_other_part() {
        // Prefixes, Strict OOXML, nonempty tags and remote targets must work
        // without opening the referenced file or URL.
        for strict in [false, true] {
            for expanded in [false, true] {
                let mut settings = SETTINGS_XML
                    .replace("w:", "x:")
                    .replace("xmlns:w=", "xmlns:x=");
                let mut relation =
                    ATTACHED.replace("file:///D:/old.dot", "https://invalid.example/old.dot");
                if expanded {
                    settings = settings.replace(
                        r#"<x:attachedTemplate r:id="old"/>"#,
                        r#"<x:attachedTemplate r:id="old"></x:attachedTemplate>"#,
                    );
                    relation = relation.replace("/>", "></Relationship>");
                }
                if strict {
                    settings = settings
                        .replace(
                            "http://schemas.openxmlformats.org/wordprocessingml/2006/main",
                            "http://purl.oclc.org/ooxml/wordprocessingml/main",
                        )
                        .replace(
                            "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
                            "http://purl.oclc.org/ooxml/officeDocument/relationships",
                        );
                    relation =
                        relation.replace(ATTACHED_TEMPLATE_TYPES[0], ATTACHED_TEMPLATE_TYPES[1]);
                }
                let source = fixture(&settings, &relation, &[]);
                let (prepared, detached) =
                    prepare_template_source(source.clone(), &DocxLimits::default()).unwrap();
                assert!(detached);
                assert!(
                    inspect_docx(Cursor::new(&prepared), &DocxLimits::default())
                        .unwrap()
                        .is_acceptable()
                );
                for name in [
                    "[Content_Types].xml",
                    "_rels/.rels",
                    "word/document.xml",
                    "word/header1.xml",
                ] {
                    assert_eq!(
                        read_part(&source, name),
                        read_part(&prepared, name),
                        "{name}"
                    );
                }
                let expected_settings = settings
                    .replace(r#"<x:attachedTemplate r:id="old"/>"#, "")
                    .replace(
                        r#"<x:attachedTemplate r:id="old"></x:attachedTemplate>"#,
                        "",
                    )
                    .replace("<x:linkStyles/>", "");
                assert_eq!(read_part(&prepared, SETTINGS), expected_settings.as_bytes());
                assert!(
                    !String::from_utf8(read_part(&prepared, SETTINGS_RELS))
                        .unwrap()
                        .contains("attachedTemplate")
                );
                let (again, detached_again) =
                    prepare_template_source(prepared.clone(), &DocxLimits::default()).unwrap();
                assert!(!detached_again);
                assert_eq!(again, prepared);
            }
        }
    }

    #[test]
    fn ordinary_document_is_not_repacked() {
        let source = minimal_docx(DOCUMENT);
        let (prepared, detached) =
            prepare_template_source(source.clone(), &DocxLimits::default()).unwrap();
        assert!(!detached);
        assert_eq!(source, prepared);
    }

    #[test]
    fn an_attached_template_cannot_hide_other_external_or_active_relationships() {
        for kind in ["image", "hyperlink", "package", "vbaProject"] {
            let extra = format!(
                r#"<Relationship Id="other" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/{kind}" Target="https://invalid.example/resource" TargetMode="External"/>"#
            );
            let source = fixture(SETTINGS_XML, &format!("{ATTACHED}{extra}"), &[]);
            let codes = error_codes(prepare_template_source(source, &DocxLimits::default()));
            assert!(codes.contains(&"DOCX_EXTERNAL_RELATIONSHIP"), "{kind}");
        }
        let document_rels = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="img" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="https://invalid.example/image" TargetMode="External"/></Relationships>"#;
        let source = fixture(
            SETTINGS_XML,
            ATTACHED,
            &[("word/_rels/document.xml.rels", document_rels)],
        );
        assert!(
            error_codes(prepare_template_source(source, &DocxLimits::default()))
                .contains(&"DOCX_EXTERNAL_RELATIONSHIP")
        );
    }

    #[test]
    fn internal_or_spoofed_template_relationships_remain_rejected() {
        for relation in [
            ATTACHED.replace("External", "Internal"),
            ATTACHED.replace(
                ATTACHED_TEMPLATE_TYPES[0],
                "https://invalid.example/attachedTemplate",
            ),
            ATTACHED.replace("attachedTemplate", "image"),
        ] {
            assert!(
                prepare_template_source(
                    fixture(SETTINGS_XML, &relation, &[]),
                    &DocxLimits::default()
                )
                .is_err()
            );
        }
    }

    #[test]
    fn ambiguous_or_malformed_settings_are_not_repaired() {
        for settings in [
            SETTINGS_XML.replace(r#"r:id="old""#, r#"r:id="missing""#),
            SETTINGS_XML.replace("xmlns:r=", "xmlns:wrong="),
            SETTINGS_XML.replace("</w:settings>", ""),
            SETTINGS_XML.replace(
                r#"<w:attachedTemplate r:id="old"/>"#,
                r#"<w:attachedTemplate r:id="old"><w:unexpected/></w:attachedTemplate>"#,
            ),
            format!("<!DOCTYPE settings>{SETTINGS_XML}"),
        ] {
            assert!(
                prepare_template_source(fixture(&settings, ATTACHED, &[]), &DocxLimits::default())
                    .is_err()
            );
        }
        assert!(
            prepare_template_source(
                fixture(SETTINGS_XML, &format!("{ATTACHED}{ATTACHED}"), &[]),
                &DocxLimits::default()
            )
            .is_err()
        );
        let nested = ATTACHED.replace(
            "/>",
            r#"><Relationship Id="hidden" Target="https://invalid.example" TargetMode="External"/></Relationship>"#,
        );
        assert!(
            prepare_template_source(fixture(SETTINGS_XML, &nested, &[]), &DocxLimits::default())
                .is_err()
        );
    }

    #[test]
    fn unsafe_container_and_macro_checks_still_run_before_repair() {
        let source = fixture(SETTINGS_XML, ATTACHED, &[]);
        let limits = DocxLimits {
            max_archive_bytes: source.len() as u64 - 1,
            ..Default::default()
        };
        assert!(
            error_codes(prepare_template_source(source.clone(), &limits))
                .contains(&"DOCX_ARCHIVE_TOO_LARGE")
        );
        let limits = DocxLimits {
            max_xml_part_bytes: 16,
            ..Default::default()
        };
        assert!(
            error_codes(prepare_template_source(source.clone(), &limits))
                .contains(&"DOCX_PART_TOO_LARGE")
        );
        let mut unsafe_name = source;
        rename_zip_entry(&mut unsafe_name, "word/header1.xml", "../x/header1.xml");
        assert!(
            error_codes(prepare_template_source(unsafe_name, &DocxLimits::default()))
                .contains(&"DOCX_UNSAFE_PART_NAME")
        );
        let macro_source = fixture(SETTINGS_XML, ATTACHED, &[("word/vbaProject.bin", b"macro")]);
        assert!(
            error_codes(prepare_template_source(
                macro_source,
                &DocxLimits::default()
            ))
            .contains(&"DOCX_ACTIVE_CONTENT_PART")
        );
    }
}
