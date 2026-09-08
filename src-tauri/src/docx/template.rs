use std::collections::BTreeMap;

use quick_xml::{events::Event, reader::NsReader};

use super::{
    ByteSpan, Diagnostic, DiagnosticSeverity, DocxError, DocxLimits, DocxResult,
    document::parse_document_xml,
    xml::{NamespaceKind, attribute_value, classify_namespace, local_name},
};

const DOCUMENT_PART: &str = "word/document.xml";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnchorKind {
    /// A tagged Word content control (`w:sdt`).
    ContentControl,
    /// A whole paragraph whose visible text is `{{ZT_NAME}}`.
    ParagraphMarker,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateAnchor {
    pub name: String,
    pub kind: AnchorKind,
    pub part_name: String,
    /// Bytes to replace while keeping the content-control wrapper when present.
    pub replacement_span: ByteSpan,
    /// Complete `w:sdt` or `w:p` source range.
    pub container_span: ByteSpan,
    pub paragraph_index: Option<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TemplateAnchorReport {
    pub anchors: Vec<TemplateAnchor>,
    pub diagnostics: Vec<Diagnostic>,
}

impl TemplateAnchorReport {
    #[cfg(test)]
    pub fn is_valid(&self) -> bool {
        !self
            .diagnostics
            .iter()
            .any(|item| item.severity == DiagnosticSeverity::Error)
    }
}

#[derive(Debug)]
struct SdtBuilder {
    start: usize,
    tag: Option<String>,
    properties_depth: usize,
    content_start: Option<usize>,
    content_end: Option<usize>,
}

/// Discovers anchors and diagnoses every required missing name.  Duplicate
/// names are always errors because export replacement would otherwise be
/// ambiguous.
pub fn check_template_anchors(
    document_xml: &[u8],
    required_names: &[&str],
    limits: &DocxLimits,
) -> DocxResult<TemplateAnchorReport> {
    discover_and_check(document_xml, required_names, limits)
}

fn discover_and_check(
    document_xml: &[u8],
    required_names: &[&str],
    limits: &DocxLimits,
) -> DocxResult<TemplateAnchorReport> {
    // Reuse the document reader so marker matching follows exactly the same
    // visible-text rules as question import.
    let parsed = parse_document_xml(document_xml, limits)?;
    let mut report = TemplateAnchorReport {
        diagnostics: parsed.diagnostics,
        ..TemplateAnchorReport::default()
    };
    scan_content_controls(document_xml, limits, &mut report)?;

    for paragraph in &parsed.paragraphs {
        let Some(name) = marker_name(&paragraph.logical_text) else {
            continue;
        };
        let nested_equivalent = report.anchors.iter().any(|anchor| {
            anchor.kind == AnchorKind::ContentControl
                && anchor.name == name
                && anchor.replacement_span.contains(paragraph.source_span)
        });
        if !nested_equivalent {
            report.anchors.push(TemplateAnchor {
                name,
                kind: AnchorKind::ParagraphMarker,
                part_name: DOCUMENT_PART.to_owned(),
                replacement_span: paragraph.source_span,
                container_span: paragraph.source_span,
                paragraph_index: Some(paragraph.index),
            });
        }
    }

    report
        .anchors
        .sort_by_key(|anchor| (anchor.container_span.start, anchor.container_span.end));

    let mut counts = BTreeMap::<String, usize>::new();
    for anchor in &report.anchors {
        *counts.entry(anchor.name.clone()).or_default() += 1;
    }
    for (name, count) in &counts {
        if *count > 1 {
            report.diagnostics.push(Diagnostic::error(
                "DOCX_TEMPLATE_ANCHOR_DUPLICATE",
                Some(DOCUMENT_PART),
                format!("template anchor '{name}' occurs {count} times"),
            ));
        }
    }
    for required in required_names {
        let normalized = normalize_anchor_name(required).unwrap_or_else(|| (*required).to_owned());
        if counts.get(&normalized).copied().unwrap_or_default() == 0 {
            report.diagnostics.push(Diagnostic::new(
                DiagnosticSeverity::Error,
                "DOCX_TEMPLATE_ANCHOR_MISSING",
                Some(DOCUMENT_PART),
                format!("required template anchor '{normalized}' is missing"),
                Some(format!(
                    "Add a content control tagged '{normalized}' or a whole paragraph containing '{{{{{normalized}}}}}'."
                )),
            ));
        }
    }

    Ok(report)
}

fn scan_content_controls(
    document_xml: &[u8],
    limits: &DocxLimits,
    report: &mut TemplateAnchorReport,
) -> DocxResult<()> {
    let mut reader = NsReader::from_reader(document_xml);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut depth = 0usize;
    let mut controls = Vec::<SdtBuilder>::new();

    loop {
        let event_start = reader.buffer_position() as usize;
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|error| DocxError::xml(DOCUMENT_PART, reader.error_position(), error))?;
        let event_end = reader.buffer_position() as usize;
        match event {
            Event::Start(ref start) => {
                depth += 1;
                if depth > limits.max_xml_depth {
                    return Err(DocxError::LimitExceeded {
                        resource: format!("XML depth in {DOCUMENT_PART}"),
                        limit: limits.max_xml_depth as u64,
                        actual: depth as u64,
                    });
                }
                let namespace = {
                    let (namespace, _) = reader.resolver().resolve_element(start.name());
                    classify_namespace(namespace)
                };
                let local = local_name(start);
                if namespace == NamespaceKind::Word {
                    match local.as_slice() {
                        b"sdt" => controls.push(SdtBuilder {
                            start: event_start,
                            tag: None,
                            properties_depth: 0,
                            content_start: None,
                            content_end: None,
                        }),
                        b"sdtPr" => {
                            if let Some(control) = controls.last_mut() {
                                control.properties_depth += 1;
                            }
                        }
                        b"tag" => set_control_tag(&reader, start, &mut controls)?,
                        b"sdtContent" => {
                            if let Some(control) = controls.last_mut() {
                                control.content_start.get_or_insert(event_end);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Event::Empty(ref start) => {
                let namespace = {
                    let (namespace, _) = reader.resolver().resolve_element(start.name());
                    classify_namespace(namespace)
                };
                if namespace == NamespaceKind::Word {
                    match local_name(start).as_slice() {
                        b"tag" => set_control_tag(&reader, start, &mut controls)?,
                        b"sdtContent" => {
                            if let Some(control) = controls.last_mut() {
                                control.content_start = Some(event_end);
                                control.content_end = Some(event_end);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Event::End(ref end) => {
                let namespace = {
                    let (namespace, _) = reader.resolver().resolve_element(end.name());
                    classify_namespace(namespace)
                };
                let local = end.local_name().as_ref().to_vec();
                if namespace == NamespaceKind::Word {
                    match local.as_slice() {
                        b"sdtPr" => {
                            if let Some(control) = controls.last_mut() {
                                control.properties_depth =
                                    control.properties_depth.saturating_sub(1);
                            }
                        }
                        b"sdtContent" => {
                            if let Some(control) = controls.last_mut() {
                                control.content_end = Some(event_start);
                            }
                        }
                        b"sdt" => {
                            let control = controls.pop().ok_or_else(|| {
                                DocxError::xml(
                                    DOCUMENT_PART,
                                    event_start as u64,
                                    "w:sdt closing tag has no matching opening tag",
                                )
                            })?;
                            finish_control(control, event_end, report);
                        }
                        _ => {}
                    }
                }
                depth = depth.saturating_sub(1);
            }
            Event::DocType(_) => {
                return Err(DocxError::xml(
                    DOCUMENT_PART,
                    event_start as u64,
                    "DOCTYPE declarations are not allowed",
                ));
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }

    if !controls.is_empty() {
        return Err(DocxError::xml(
            DOCUMENT_PART,
            reader.buffer_position(),
            "document ended inside a content control",
        ));
    }
    Ok(())
}

fn set_control_tag(
    reader: &NsReader<&[u8]>,
    start: &quick_xml::events::BytesStart<'_>,
    controls: &mut [SdtBuilder],
) -> DocxResult<()> {
    let Some(control) = controls.last_mut() else {
        return Ok(());
    };
    if control.properties_depth == 0 || control.tag.is_some() {
        return Ok(());
    }
    control.tag = attribute_value(
        reader,
        start,
        b"val",
        Some(NamespaceKind::Word),
        DOCUMENT_PART,
    )?;
    Ok(())
}

fn finish_control(control: SdtBuilder, end: usize, report: &mut TemplateAnchorReport) {
    let Some(raw_tag) = control.tag else {
        return;
    };
    let Some(name) = normalize_anchor_name(&raw_tag) else {
        return;
    };
    match (control.content_start, control.content_end) {
        (Some(content_start), Some(content_end)) if content_start <= content_end => {
            report.anchors.push(TemplateAnchor {
                name,
                kind: AnchorKind::ContentControl,
                part_name: DOCUMENT_PART.to_owned(),
                replacement_span: ByteSpan::new(content_start, content_end),
                container_span: ByteSpan::new(control.start, end),
                paragraph_index: None,
            });
        }
        _ => report.diagnostics.push(Diagnostic::error(
            "DOCX_TEMPLATE_ANCHOR_MALFORMED",
            Some(DOCUMENT_PART),
            format!("content control tagged '{name}' has no usable w:sdtContent"),
        )),
    }
}

fn marker_name(text: &str) -> Option<String> {
    let trimmed = text.trim();
    let inner = trimmed.strip_prefix("{{")?.strip_suffix("}}")?;
    normalize_anchor_name(inner)
}

fn normalize_anchor_name(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let unwrapped = trimmed
        .strip_prefix("{{")
        .and_then(|value| value.strip_suffix("}}"))
        .unwrap_or(trimmed)
        .trim();
    if !unwrapped.starts_with("ZT_")
        || unwrapped.len() <= 3
        || !unwrapped
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return None;
    }
    Some(unwrapped.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEMPLATE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:sdt>
      <w:sdtPr><w:tag w:val="ZT_TITLE"/></w:sdtPr>
      <w:sdtContent><w:p><w:r><w:t>Title here</w:t></w:r></w:p></w:sdtContent>
    </w:sdt>
    <w:p><w:r><w:t>{{ZT_</w:t></w:r><w:r><w:t>QUESTIONS}}</w:t></w:r></w:p>
  </w:body>
</w:document>"#;

    #[test]
    fn finds_content_control_and_split_run_marker() {
        let report = check_template_anchors(
            TEMPLATE.as_bytes(),
            &["ZT_TITLE", "ZT_QUESTIONS"],
            &DocxLimits::default(),
        )
        .unwrap();
        assert!(report.is_valid(), "{:?}", report.diagnostics);
        assert_eq!(report.anchors.len(), 2);
        assert_eq!(report.anchors[0].kind, AnchorKind::ContentControl);
        assert_eq!(report.anchors[0].name, "ZT_TITLE");
        assert_eq!(report.anchors[1].kind, AnchorKind::ParagraphMarker);
        assert_eq!(report.anchors[1].name, "ZT_QUESTIONS");
    }

    #[test]
    fn diagnoses_missing_and_duplicate_anchors() {
        let xml = TEMPLATE.replace("ZT_TITLE", "ZT_QUESTIONS");
        let report = check_template_anchors(
            xml.as_bytes(),
            &["ZT_TITLE", "ZT_QUESTIONS"],
            &DocxLimits::default(),
        )
        .unwrap();
        assert!(
            report
                .diagnostics
                .iter()
                .any(|item| item.code == "DOCX_TEMPLATE_ANCHOR_MISSING")
        );
        assert!(
            report
                .diagnostics
                .iter()
                .any(|item| item.code == "DOCX_TEMPLATE_ANCHOR_DUPLICATE")
        );
    }
}
