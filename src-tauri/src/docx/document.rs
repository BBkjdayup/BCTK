use std::io::{Read, Seek, SeekFrom};

use quick_xml::{events::Event, reader::NsReader};
use zip::ZipArchive;

use super::{
    ByteSpan, Diagnostic, DocxError, DocxLimits, DocxResult,
    package::{archive_size_diagnostic, inspect_archive_with_size, read_part_limited},
    xml::{NamespaceKind, classify_namespace, decode_reference, local_name},
};

const DOCUMENT_PART: &str = "word/document.xml";

/// Stable logical-text marker inserted where an OMML expression occurs.
pub const FORMULA_PLACEHOLDER: char = '\u{fffc}';

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MathKind {
    Inline,
    Display,
}

/// Exact OMML bytes plus their location in the source XML and logical text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MathFragment {
    pub kind: MathKind,
    pub source_span: ByteSpan,
    pub raw_xml: Vec<u8>,
    pub paragraph_index: Option<usize>,
    pub text_char_offset: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paragraph {
    pub index: usize,
    pub logical_text: String,
    pub source_span: ByteSpan,
    /// Indices into [`ParsedDocument::math_fragments`].
    pub math_fragment_indices: Vec<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DocumentTableCell {
    pub paragraph_indices: Vec<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DocumentTable {
    pub index: usize,
    pub rows: Vec<Vec<DocumentTableCell>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedDocument {
    pub paragraphs: Vec<Paragraph>,
    pub math_fragments: Vec<MathFragment>,
    pub tables: Vec<DocumentTable>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug)]
struct ParagraphBuilder {
    start: usize,
    logical_text: String,
    math_fragment_indices: Vec<usize>,
}

#[derive(Debug)]
struct MathCapture {
    kind: MathKind,
    start: usize,
    depth: usize,
    paragraph_index: Option<usize>,
    text_char_offset: Option<usize>,
}

#[derive(Debug)]
struct TableBuilder {
    rows: Vec<Vec<DocumentTableCell>>,
}

/// Validates the OPC package, reads `word/document.xml` with a bound, and then
/// extracts its narrow logical representation.
pub fn read_document_from_docx<R: Read + Seek>(
    mut source: R,
    limits: &DocxLimits,
) -> DocxResult<ParsedDocument> {
    let archive_bytes = source.seek(SeekFrom::End(0))?;
    if let Some(diagnostic) = archive_size_diagnostic(archive_bytes, limits) {
        return Err(DocxError::rejected(vec![diagnostic]));
    }
    source.seek(SeekFrom::Start(0))?;
    let mut archive = ZipArchive::new(source)?;
    let inspection = inspect_archive_with_size(&mut archive, Some(archive_bytes), limits)?;
    inspection.ensure_acceptable()?;
    let xml = read_part_limited(&mut archive, DOCUMENT_PART, limits.max_xml_part_bytes)?;
    let mut document = parse_document_xml(&xml, limits)?;
    document.diagnostics.extend(
        inspection
            .diagnostics
            .into_iter()
            .filter(|diagnostic| diagnostic.severity != super::DiagnosticSeverity::Error),
    );
    Ok(document)
}

/// Reads paragraph logical text while preserving each outermost OMML subtree as
/// exact source bytes.  Text split over many `w:r`/`w:t` nodes is concatenated;
/// deleted/moved-from text and field instructions are excluded.
pub fn parse_document_xml(xml: &[u8], limits: &DocxLimits) -> DocxResult<ParsedDocument> {
    if xml.len() as u64 > limits.max_xml_part_bytes {
        return Err(DocxError::LimitExceeded {
            resource: DOCUMENT_PART.to_owned(),
            limit: limits.max_xml_part_bytes,
            actual: xml.len() as u64,
        });
    }

    let mut reader = NsReader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut result = ParsedDocument::default();
    let mut paragraph: Option<ParagraphBuilder> = None;
    let mut paragraph_depth = 0usize;
    let mut math_capture: Option<MathCapture> = None;
    let mut xml_depth = 0usize;
    let mut excluded_depth = 0usize;
    let mut instruction_depth = 0usize;
    let mut text_depth = 0usize;
    let mut text_box_depth = 0usize;
    let mut table_depth = 0usize;
    let mut table: Option<TableBuilder> = None;
    let mut table_row: Option<Vec<DocumentTableCell>> = None;
    let mut table_cell: Option<DocumentTableCell> = None;
    let mut saw_document_root = false;

    loop {
        let event_start = reader.buffer_position() as usize;
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|error| DocxError::xml(DOCUMENT_PART, reader.error_position(), error))?;
        let event_end = reader.buffer_position() as usize;

        match event {
            Event::Start(ref start) => {
                xml_depth += 1;
                ensure_xml_depth(xml_depth, limits)?;
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

                if let Some(capture) = math_capture.as_mut() {
                    capture.depth += 1;
                    buffer.clear();
                    continue;
                }

                if namespace == NamespaceKind::Math && (local == b"oMath" || local == b"oMathPara")
                {
                    let kind = if local == b"oMathPara" {
                        MathKind::Display
                    } else {
                        MathKind::Inline
                    };
                    let paragraph_index = paragraph.as_ref().map(|_| result.paragraphs.len());
                    let text_char_offset = paragraph.as_mut().map(|paragraph| {
                        let offset = paragraph.logical_text.chars().count();
                        paragraph.logical_text.push(FORMULA_PLACEHOLDER);
                        offset
                    });
                    math_capture = Some(MathCapture {
                        kind,
                        start: event_start,
                        depth: 1,
                        paragraph_index,
                        text_char_offset,
                    });
                    if paragraph_index.is_none() {
                        result.diagnostics.push(Diagnostic::warning(
                            "DOCX_MATH_OUTSIDE_PARAGRAPH",
                            Some(DOCUMENT_PART),
                            "an OMML expression occurs outside a Word paragraph",
                        ));
                    }
                    buffer.clear();
                    continue;
                }

                if namespace == NamespaceKind::Word {
                    match local.as_slice() {
                        b"document" => saw_document_root = true,
                        b"tbl" => {
                            table_depth += 1;
                            if table_depth == 1 {
                                table = Some(TableBuilder { rows: Vec::new() });
                            }
                        }
                        b"tr" if table_depth == 1 => {
                            table_row = Some(Vec::new());
                        }
                        b"tc" if table_depth == 1 => {
                            table_cell = Some(DocumentTableCell::default());
                        }
                        b"p" => {
                            if paragraph_depth != 0 || paragraph.is_some() {
                                return Err(DocxError::xml(
                                    DOCUMENT_PART,
                                    event_start as u64,
                                    "nested w:p elements outside text boxes are not supported",
                                ));
                            }
                            paragraph = Some(ParagraphBuilder {
                                start: event_start,
                                logical_text: String::new(),
                                math_fragment_indices: Vec::new(),
                            });
                            paragraph_depth = 1;
                        }
                        b"del" | b"moveFrom" => excluded_depth += 1,
                        b"instrText" => instruction_depth += 1,
                        b"t" => text_depth += 1,
                        b"altChunk" => {
                            return Err(DocxError::xml(
                                DOCUMENT_PART,
                                event_start as u64,
                                "w:altChunk is not allowed",
                            ));
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
                let local = local_name(start);
                if text_box_depth > 0 {
                    buffer.clear();
                    continue;
                }
                if math_capture.is_some() {
                    buffer.clear();
                    continue;
                }
                if namespace == NamespaceKind::Math && (local == b"oMath" || local == b"oMathPara")
                {
                    let kind = if local == b"oMathPara" {
                        MathKind::Display
                    } else {
                        MathKind::Inline
                    };
                    let paragraph_index = paragraph.as_ref().map(|_| result.paragraphs.len());
                    let text_char_offset = paragraph.as_mut().map(|paragraph| {
                        let offset = paragraph.logical_text.chars().count();
                        paragraph.logical_text.push(FORMULA_PLACEHOLDER);
                        offset
                    });
                    let fragment_index = result.math_fragments.len();
                    result.math_fragments.push(MathFragment {
                        kind,
                        source_span: ByteSpan::new(event_start, event_end),
                        raw_xml: xml[event_start..event_end].to_vec(),
                        paragraph_index,
                        text_char_offset,
                    });
                    if let Some(paragraph) = paragraph.as_mut() {
                        paragraph.math_fragment_indices.push(fragment_index);
                    }
                } else if namespace == NamespaceKind::Word {
                    if local == b"altChunk" {
                        return Err(DocxError::xml(
                            DOCUMENT_PART,
                            event_start as u64,
                            "w:altChunk is not allowed",
                        ));
                    }
                    if local == b"p" {
                        if paragraph_depth != 0 || paragraph.is_some() {
                            return Err(DocxError::xml(
                                DOCUMENT_PART,
                                event_start as u64,
                                "nested w:p elements outside text boxes are not supported",
                            ));
                        }
                        let paragraph_index = result.paragraphs.len();
                        result.paragraphs.push(Paragraph {
                            index: paragraph_index,
                            logical_text: String::new(),
                            source_span: ByteSpan::new(event_start, event_end),
                            math_fragment_indices: Vec::new(),
                        });
                        if table_depth > 0
                            && let Some(cell) = table_cell.as_mut()
                        {
                            cell.paragraph_indices.push(paragraph_index);
                        }
                    } else if excluded_depth == 0
                        && instruction_depth == 0
                        && let Some(paragraph) = paragraph.as_mut()
                    {
                        match local.as_slice() {
                            b"tab" => paragraph.logical_text.push('\t'),
                            b"br" | b"cr" => paragraph.logical_text.push('\n'),
                            b"noBreakHyphen" => paragraph.logical_text.push('\u{2011}'),
                            b"softHyphen" => paragraph.logical_text.push('\u{00ad}'),
                            _ => {}
                        }
                    }
                }
            }
            Event::End(ref end) => {
                let namespace = {
                    let (namespace, _) = reader.resolver().resolve_element(end.name());
                    classify_namespace(namespace)
                };
                let local = end.local_name().as_ref().to_vec();

                if text_box_depth > 0 {
                    if namespace == NamespaceKind::Word && local == b"txbxContent" {
                        text_box_depth = text_box_depth.saturating_sub(1);
                    }
                    xml_depth = xml_depth.saturating_sub(1);
                    buffer.clear();
                    continue;
                }

                if let Some(capture) = math_capture.as_mut() {
                    capture.depth = capture.depth.saturating_sub(1);
                    if capture.depth == 0 {
                        let capture = math_capture.take().expect("math capture must exist");
                        let fragment_index = result.math_fragments.len();
                        result.math_fragments.push(MathFragment {
                            kind: capture.kind,
                            source_span: ByteSpan::new(capture.start, event_end),
                            raw_xml: xml[capture.start..event_end].to_vec(),
                            paragraph_index: capture.paragraph_index,
                            text_char_offset: capture.text_char_offset,
                        });
                        if let Some(paragraph) = paragraph.as_mut() {
                            paragraph.math_fragment_indices.push(fragment_index);
                        }
                    }
                    xml_depth = xml_depth.saturating_sub(1);
                    buffer.clear();
                    continue;
                }

                if namespace == NamespaceKind::Word {
                    match local.as_slice() {
                        b"t" => text_depth = text_depth.saturating_sub(1),
                        b"instrText" => instruction_depth = instruction_depth.saturating_sub(1),
                        b"del" | b"moveFrom" => excluded_depth = excluded_depth.saturating_sub(1),
                        b"p" => {
                            if paragraph_depth == 0 {
                                return Err(DocxError::xml(
                                    DOCUMENT_PART,
                                    event_start as u64,
                                    "w:p closing tag has no matching opening tag",
                                ));
                            }
                            paragraph_depth -= 1;
                            let paragraph = paragraph.take().ok_or_else(|| {
                                DocxError::xml(
                                    DOCUMENT_PART,
                                    event_start as u64,
                                    "w:p closing tag has no matching opening tag",
                                )
                            })?;
                            let paragraph_index = result.paragraphs.len();
                            result.paragraphs.push(Paragraph {
                                index: paragraph_index,
                                logical_text: paragraph.logical_text,
                                source_span: ByteSpan::new(paragraph.start, event_end),
                                math_fragment_indices: paragraph.math_fragment_indices,
                            });
                            if table_depth > 0
                                && let Some(cell) = table_cell.as_mut()
                            {
                                cell.paragraph_indices.push(paragraph_index);
                            }
                        }
                        b"tc" if table_depth == 1 => {
                            let cell = table_cell.take().ok_or_else(|| {
                                DocxError::xml(
                                    DOCUMENT_PART,
                                    event_start as u64,
                                    "w:tc closing tag has no matching opening tag",
                                )
                            })?;
                            table_row
                                .as_mut()
                                .ok_or_else(|| {
                                    DocxError::xml(
                                        DOCUMENT_PART,
                                        event_start as u64,
                                        "w:tc occurs outside a table row",
                                    )
                                })?
                                .push(cell);
                        }
                        b"tr" if table_depth == 1 => {
                            let row = table_row.take().ok_or_else(|| {
                                DocxError::xml(
                                    DOCUMENT_PART,
                                    event_start as u64,
                                    "w:tr closing tag has no matching opening tag",
                                )
                            })?;
                            table
                                .as_mut()
                                .ok_or_else(|| {
                                    DocxError::xml(
                                        DOCUMENT_PART,
                                        event_start as u64,
                                        "w:tr occurs outside a table",
                                    )
                                })?
                                .rows
                                .push(row);
                        }
                        b"tbl" => {
                            if table_depth == 0 {
                                return Err(DocxError::xml(
                                    DOCUMENT_PART,
                                    event_start as u64,
                                    "w:tbl closing tag has no matching opening tag",
                                ));
                            }
                            if table_depth == 1 {
                                let table = table.take().ok_or_else(|| {
                                    DocxError::xml(
                                        DOCUMENT_PART,
                                        event_start as u64,
                                        "w:tbl closing tag has no matching opening tag",
                                    )
                                })?;
                                result.tables.push(DocumentTable {
                                    index: result.tables.len(),
                                    rows: table.rows,
                                });
                            }
                            table_depth = table_depth.saturating_sub(1);
                        }
                        _ => {}
                    }
                }
                xml_depth = xml_depth.saturating_sub(1);
            }
            Event::Text(ref text) => {
                if math_capture.is_none()
                    && text_box_depth == 0
                    && text_depth > 0
                    && excluded_depth == 0
                    && instruction_depth == 0
                    && let Some(paragraph) = paragraph.as_mut()
                {
                    let decoded = text.xml10_content().map_err(|error| {
                        DocxError::xml(DOCUMENT_PART, event_start as u64, error)
                    })?;
                    paragraph.logical_text.push_str(&decoded);
                }
            }
            Event::CData(ref text) => {
                if math_capture.is_none()
                    && text_box_depth == 0
                    && text_depth > 0
                    && excluded_depth == 0
                    && instruction_depth == 0
                    && let Some(paragraph) = paragraph.as_mut()
                {
                    let decoded = text.xml10_content().map_err(|error| {
                        DocxError::xml(DOCUMENT_PART, event_start as u64, error)
                    })?;
                    paragraph.logical_text.push_str(&decoded);
                }
            }
            Event::GeneralRef(ref reference) => {
                if math_capture.is_none()
                    && text_box_depth == 0
                    && text_depth > 0
                    && excluded_depth == 0
                    && instruction_depth == 0
                    && let Some(paragraph) = paragraph.as_mut()
                {
                    paragraph.logical_text.push(decode_reference(
                        reference,
                        DOCUMENT_PART,
                        event_start as u64,
                    )?);
                }
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

    if !saw_document_root {
        return Err(DocxError::xml(
            DOCUMENT_PART,
            0,
            "missing WordprocessingML document root",
        ));
    }
    if paragraph.is_some()
        || paragraph_depth != 0
        || text_box_depth != 0
        || math_capture.is_some()
        || table_depth != 0
        || table.is_some()
        || table_row.is_some()
        || table_cell.is_some()
    {
        return Err(DocxError::xml(
            DOCUMENT_PART,
            reader.buffer_position(),
            "document ended inside a paragraph or OMML expression",
        ));
    }
    Ok(result)
}

fn ensure_xml_depth(depth: usize, limits: &DocxLimits) -> DocxResult<()> {
    if depth > limits.max_xml_depth {
        Err(DocxError::LimitExceeded {
            resource: format!("XML depth in {DOCUMENT_PART}"),
            limit: limits.max_xml_depth as u64,
            actual: depth as u64,
        })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
            xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math">
  <w:body>
    <w:p>
      <w:r><w:t>Hello &amp; </w:t></w:r><w:r><w:t>world</w:t><w:tab/><w:br/></w:r>
      <w:del><w:r><w:delText>deleted</w:delText></w:r></w:del>
      <w:r><w:instrText>PAGE</w:instrText><w:t>visible</w:t></w:r>
      <m:oMath><m:r><m:t>x</m:t></m:r></m:oMath>
    </w:p>
  </w:body>
</w:document>"#;

    #[test]
    fn joins_split_runs_and_captures_exact_omml() {
        let parsed = parse_document_xml(XML.as_bytes(), &DocxLimits::default()).unwrap();
        assert_eq!(parsed.paragraphs.len(), 1);
        assert_eq!(
            parsed.paragraphs[0].logical_text,
            format!("Hello & world\t\nvisible{FORMULA_PLACEHOLDER}")
        );
        assert_eq!(parsed.math_fragments.len(), 1);
        assert_eq!(parsed.math_fragments[0].kind, MathKind::Inline);
        assert_eq!(
            parsed.math_fragments[0].raw_xml,
            b"<m:oMath><m:r><m:t>x</m:t></m:r></m:oMath>"
        );
        assert_eq!(parsed.paragraphs[0].math_fragment_indices, vec![0]);
    }

    #[test]
    fn skips_nested_text_box_paragraphs() {
        let xml =
            br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:r><w:t>before</w:t></w:r><w:r><w:drawing><w:txbxContent>
      <w:p><w:r><w:t>inside</w:t></w:r></w:p>
    </w:txbxContent></w:drawing><w:t>after</w:t></w:r></w:p>
    <w:p><w:r><w:t>next</w:t></w:r></w:p>
  </w:body>
</w:document>"#;

        let parsed = parse_document_xml(xml, &DocxLimits::default()).unwrap();
        assert_eq!(parsed.paragraphs.len(), 2);
        assert_eq!(parsed.paragraphs[0].logical_text, "beforeafter");
        assert_eq!(parsed.paragraphs[1].logical_text, "next");
    }

    #[test]
    fn still_rejects_nested_paragraphs_outside_text_boxes() {
        let xml =
            br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body><w:p><w:p><w:r><w:t>invalid</w:t></w:r></w:p></w:p></w:body>
</w:document>"#;

        assert!(parse_document_xml(xml, &DocxLimits::default()).is_err());
    }

    #[test]
    fn rejects_doctype_even_without_entity_expansion() {
        let xml = br#"<!DOCTYPE w:document [<!ENTITY x "boom">]>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body/></w:document>"#;
        assert!(parse_document_xml(xml, &DocxLimits::default()).is_err());
    }

    #[test]
    fn enforces_xml_depth() {
        let xml = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p/></w:body></w:document>"#;
        let limits = DocxLimits {
            max_xml_depth: 1,
            ..DocxLimits::default()
        };
        assert!(matches!(
            parse_document_xml(xml, &limits),
            Err(DocxError::LimitExceeded { .. })
        ));
    }

    #[test]
    fn captures_table_rows_cells_and_their_paragraph_indices() {
        let xml =
            br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:r><w:t>before</w:t></w:r></w:p>
    <w:tbl>
      <w:tr>
        <w:tc><w:p><w:r><w:t>x</w:t></w:r></w:p></w:tc>
        <w:tc><w:p/></w:tc>
        <w:tc><w:p/></w:tc>
      </w:tr>
      <w:tr>
        <w:tc><w:p><w:r><w:t>y</w:t></w:r></w:p></w:tc>
        <w:tc><w:p/></w:tc>
        <w:tc><w:p/></w:tc>
      </w:tr>
    </w:tbl>
    <w:p><w:r><w:t>after</w:t></w:r></w:p>
  </w:body>
</w:document>"#;

        let parsed = parse_document_xml(xml, &DocxLimits::default()).unwrap();
        assert_eq!(parsed.paragraphs.len(), 8);
        assert_eq!(parsed.tables.len(), 1);
        assert_eq!(
            parsed.tables[0].rows,
            vec![
                vec![
                    DocumentTableCell {
                        paragraph_indices: vec![1]
                    },
                    DocumentTableCell {
                        paragraph_indices: vec![2]
                    },
                    DocumentTableCell {
                        paragraph_indices: vec![3]
                    },
                ],
                vec![
                    DocumentTableCell {
                        paragraph_indices: vec![4]
                    },
                    DocumentTableCell {
                        paragraph_indices: vec![5]
                    },
                    DocumentTableCell {
                        paragraph_indices: vec![6]
                    },
                ],
            ]
        );
        assert_eq!(parsed.paragraphs[7].logical_text, "after");
    }
}
