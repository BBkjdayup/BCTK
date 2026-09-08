use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read},
};

use quick_xml::{
    Writer,
    events::{BytesStart, Event},
    name::{Namespace, ResolveResult},
    reader::NsReader,
};
use serde::{Deserialize, Serialize};
use zip::ZipArchive;

use super::{
    ByteSpan, Diagnostic, DiagnosticSeverity, DocxError, DocxLimits, DocxResult, PackageKind,
    TemplateAnchor, TemplatePackageAnalysis, analyze_template_package,
    analyze_template_package_with_requirements,
    package::{parse_content_types, read_part_limited, scan_relationships},
    parse_document_xml, raw_copy_with_replacements, raw_copy_with_replacements_and_removals,
    read_mathtype_from_docx,
    xml::{NamespaceKind, WML_STRICT, WML_TRANSITIONAL, attribute_value, classify_namespace},
};

const CONTENT_TYPES_PART: &str = "[Content_Types].xml";
const DOCUMENT_PART: &str = "word/document.xml";
const DOCUMENT_RELS_PART: &str = "word/_rels/document.xml.rels";
const SUPPORTED_ANCHORS: [&str; 4] = ["ZT_TITLE", "ZT_QUESTIONS", "ZT_ANSWERS", "ZT_EXPLANATIONS"];
const WORDPROCESSING_DRAWING_NS: &[u8] =
    b"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const WORDPROCESSING_GROUP_NS: &[u8] =
    b"http://schemas.microsoft.com/office/word/2010/wordprocessingGroup";
const WORDPROCESSING_SHAPE_NS: &[u8] =
    b"http://schemas.microsoft.com/office/word/2010/wordprocessingShape";
const EMU_PER_TWIP: i64 = 635;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TemplateRegionPlacement {
    ReplaceParagraph(usize),
    ReplaceRange {
        start_paragraph: usize,
        end_paragraph: usize,
    },
    AfterParagraph(usize),
    DocumentEnd,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TemplateStyleSelection {
    pub section_heading: Option<usize>,
    pub question: Option<usize>,
    pub option: Option<usize>,
    pub answer: Option<usize>,
    pub explanation: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateRegionConfiguration {
    pub questions: TemplateRegionPlacement,
    pub styles: TemplateStyleSelection,
    /// `None` keeps an existing title anchor and otherwise leaves the template
    /// title untouched.
    pub title: Option<TemplateRegionPlacement>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateSourceRange {
    pub start_paragraph_index: usize,
    pub end_paragraph_index: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParagraphStylePrototype {
    pub source_paragraph_index: usize,
    #[serde(default, with = "hex_bytes")]
    pub paragraph_properties: Vec<u8>,
    #[serde(default, with = "hex_bytes")]
    pub run_properties: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateStyleProfile {
    pub schema_version: u32,
    pub source_range: Option<TemplateSourceRange>,
    pub roles: BTreeMap<String, ParagraphStylePrototype>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateConfigurationParagraph {
    pub index: usize,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateConfigurationPreview {
    pub paragraphs: Vec<TemplateConfigurationParagraph>,
    pub has_questions_anchor: bool,
    pub has_title_anchor: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplatePageSetup {
    pub width_twips: u32,
    pub height_twips: u32,
    pub margin_top_twips: u32,
    pub margin_right_twips: u32,
    pub margin_bottom_twips: u32,
    pub margin_left_twips: u32,
    pub header_twips: u32,
    pub footer_twips: u32,
    pub column_count: u32,
    pub column_gap_twips: u32,
    pub column_separator: bool,
    #[serde(default)]
    pub document_grid_type: Option<String>,
    #[serde(default)]
    pub document_grid_line_pitch_twips: Option<u32>,
}

impl Default for TemplatePageSetup {
    fn default() -> Self {
        Self {
            // ISO A4, expressed in twentieths of a point.
            width_twips: 11_906,
            height_twips: 16_838,
            margin_top_twips: 1_440,
            margin_right_twips: 1_440,
            margin_bottom_twips: 1_440,
            margin_left_twips: 1_440,
            header_twips: 720,
            footer_twips: 720,
            column_count: 1,
            column_gap_twips: 720,
            column_separator: false,
            document_grid_type: None,
            document_grid_line_pitch_twips: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateFontTheme {
    pub major_latin: Option<String>,
    pub major_east_asia: Option<String>,
    pub minor_latin: Option<String>,
    pub minor_east_asia: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateSideSealRun {
    pub text: String,
    pub underline: bool,
    pub size_half_points: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateSideSealLine {
    pub alignment: Option<String>,
    pub runs: Vec<TemplateSideSealRun>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateSideSealLayout {
    pub horizontal_relative_from: Option<String>,
    pub vertical_relative_from: Option<String>,
    pub horizontal_offset_emu: Option<i64>,
    pub vertical_offset_emu: Option<i64>,
    pub page_x_emu: Option<i64>,
    pub page_y_emu: Option<i64>,
    pub width_emu: u64,
    pub height_emu: u64,
    pub text_direction: Option<String>,
    pub line_x_emu: Option<i64>,
    pub line_width_emu: Option<u64>,
    pub line_dash: Option<String>,
    pub lines: Vec<TemplateSideSealLine>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateLayoutBlock {
    pub kind: String,
    pub text: String,
    pub style: Option<ParagraphStylePrototype>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side_seal: Option<TemplateSideSealLayout>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateLayoutPreview {
    pub page: TemplatePageSetup,
    pub blocks: Vec<TemplateLayoutBlock>,
    pub font_theme: TemplateFontTheme,
    pub page_number_format: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfiguredTemplatePackage {
    pub bytes: Vec<u8>,
    pub analysis: TemplatePackageAnalysis,
    pub style_profile: Option<TemplateStyleProfile>,
}

#[derive(Clone, Debug)]
struct XmlEdit {
    span: ByteSpan,
    bytes: Vec<u8>,
    label: &'static str,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct MathTypeObjectRelationships {
    ole_ids: BTreeSet<String>,
    resource_ids: BTreeSet<String>,
}

#[derive(Clone, Debug, Default)]
struct CurrentObjectRelationships {
    depth: usize,
    ole_ids: BTreeSet<String>,
    resource_ids: BTreeSet<String>,
}

struct PlacementEditContext<'a> {
    namespace: &'a str,
    body_insert_offset: usize,
    document_xml: &'a [u8],
    paragraphs: &'a [super::Paragraph],
    anchors: &'a [TemplateAnchor],
    limits: &'a DocxLimits,
}

pub fn preview_template_configuration(
    package_bytes: &[u8],
    limits: &DocxLimits,
) -> DocxResult<TemplateConfigurationPreview> {
    let analysis = acceptable_document_analysis(package_bytes, limits)?;
    let document_xml = analysis
        .document_xml
        .as_deref()
        .ok_or_else(missing_document_error)?;
    let parsed = parse_document_xml(document_xml, limits)?;
    let anchor_spans = analysis
        .anchors
        .iter()
        .map(|anchor| anchor.container_span)
        .collect::<Vec<_>>();
    let paragraphs = parsed
        .paragraphs
        .into_iter()
        .filter(|paragraph| {
            !anchor_spans
                .iter()
                .any(|span| span.contains(paragraph.source_span))
        })
        .map(|paragraph| TemplateConfigurationParagraph {
            index: paragraph.index,
            text: paragraph.logical_text,
        })
        .collect();
    Ok(TemplateConfigurationPreview {
        paragraphs,
        has_questions_anchor: analysis.anchors_named("ZT_QUESTIONS").next().is_some(),
        has_title_anchor: analysis.anchors_named("ZT_TITLE").next().is_some(),
    })
}

pub fn preview_template_layout(
    package_bytes: &[u8],
    limits: &DocxLimits,
) -> DocxResult<TemplateLayoutPreview> {
    let analysis = acceptable_document_analysis(package_bytes, limits)?;
    let document_xml = analysis
        .document_xml
        .as_deref()
        .ok_or_else(missing_document_error)?;
    let parsed = parse_document_xml(document_xml, limits)?;
    let page = read_page_setup(document_xml, limits)?;
    let mut positioned_blocks = Vec::new();

    for paragraph in parsed.paragraphs {
        if analysis
            .anchors
            .iter()
            .any(|anchor| anchor.container_span.contains(paragraph.source_span))
        {
            continue;
        }
        let source = &document_xml[paragraph.source_span.as_range()];
        let (paragraph_properties, run_properties) =
            extract_paragraph_style_properties(source, limits)?;
        let is_side_seal = is_side_seal_paragraph(source);
        let side_seal = if is_side_seal {
            extract_side_seal_layout(source, paragraph.index, &page, limits)?
        } else {
            None
        };
        positioned_blocks.push((
            paragraph.source_span.start,
            TemplateLayoutBlock {
                kind: if is_side_seal {
                    "sideSeal".to_owned()
                } else {
                    "static".to_owned()
                },
                text: if is_side_seal {
                    "班级 姓名 考号 密封线内不准答题".to_owned()
                } else {
                    paragraph.logical_text
                },
                style: Some(ParagraphStylePrototype {
                    source_paragraph_index: paragraph.index,
                    paragraph_properties,
                    run_properties,
                }),
                side_seal,
            },
        ));
    }

    for anchor in &analysis.anchors {
        let kind = match anchor.name.as_str() {
            "ZT_TITLE" => "title",
            // The current Word exporter writes every selected section into
            // ZT_QUESTIONS and clears the two legacy optional anchors.
            "ZT_QUESTIONS" => "questions",
            "ZT_ANSWERS" | "ZT_EXPLANATIONS" => continue,
            _ => continue,
        };
        positioned_blocks.push((
            anchor.container_span.start,
            TemplateLayoutBlock {
                kind: kind.to_owned(),
                text: String::new(),
                style: None,
                side_seal: None,
            },
        ));
    }

    positioned_blocks.sort_by_key(|(position, _)| *position);
    let (font_theme, page_number_format) = read_template_preview_metadata(package_bytes, limits)?;
    Ok(TemplateLayoutPreview {
        page,
        blocks: positioned_blocks
            .into_iter()
            .map(|(_, block)| block)
            .collect(),
        font_theme,
        page_number_format,
    })
}

pub fn configure_template_package(
    package_bytes: &[u8],
    configuration: &TemplateRegionConfiguration,
    limits: &DocxLimits,
) -> DocxResult<ConfiguredTemplatePackage> {
    let analysis = acceptable_document_analysis(package_bytes, limits)?;
    reject_unsupported_anchors(&analysis.anchors)?;
    let document_xml = analysis
        .document_xml
        .as_deref()
        .ok_or_else(missing_document_error)?;
    let source_mathtype_references = collect_mathtype_object_relationships(document_xml, limits)?;
    if !source_mathtype_references.ole_ids.is_empty() {
        if !limits.allow_mathtype_ole {
            return Err(DocxError::rejected(vec![Diagnostic::error(
                "TEMPLATE_MATHTYPE_POLICY_REQUIRED",
                Some(DOCUMENT_PART),
                "模板包含 MathType 公式，但当前配置任务没有启用 MathType 安全检查。",
            )]));
        }
        let formulas = read_mathtype_from_docx(Cursor::new(package_bytes), limits)?;
        let converted_ids = formulas
            .iter()
            .map(|formula| formula.relationship_id.clone())
            .collect::<BTreeSet<_>>();
        if converted_ids != source_mathtype_references.ole_ids {
            return Err(DocxError::rejected(vec![Diagnostic::error(
                "TEMPLATE_MATHTYPE_RELATIONSHIP_MISMATCH",
                Some(DOCUMENT_PART),
                "MathType 公式与正文中的嵌入对象关系不一致，无法安全生成模板。",
            )]));
        }
    }
    let parsed = parse_document_xml(document_xml, limits)?;
    let namespace = document_word_namespace(document_xml, limits)?;
    let body_insert_offset = document_body_insert_offset(document_xml, limits)?;
    let style_profile =
        build_style_profile(document_xml, &parsed.paragraphs, configuration, limits)?;
    let mut edits = Vec::new();
    let placement_context = PlacementEditContext {
        namespace,
        body_insert_offset,
        document_xml,
        paragraphs: &parsed.paragraphs,
        anchors: &analysis.anchors,
        limits,
    };

    for anchor in analysis.anchors_named("ZT_QUESTIONS") {
        edits.push(XmlEdit {
            span: anchor.container_span,
            bytes: Vec::new(),
            label: "remove existing questions region",
        });
    }
    if configuration.title.is_some() {
        for anchor in analysis.anchors_named("ZT_TITLE") {
            edits.push(XmlEdit {
                span: anchor.container_span,
                bytes: Vec::new(),
                label: "remove existing title region",
            });
        }
    }

    edits.push(placement_edit(
        &configuration.questions,
        "ZT_QUESTIONS",
        &placement_context,
    )?);
    if let Some(title) = &configuration.title {
        edits.push(placement_edit(title, "ZT_TITLE", &placement_context)?);
    }
    validate_edits(&mut edits, document_xml.len())?;

    let mut configured_xml = document_xml.to_vec();
    for edit in edits.into_iter().rev() {
        configured_xml.splice(edit.span.as_range(), edit.bytes);
    }
    parse_document_xml(&configured_xml, limits)?;

    let remaining_mathtype_references =
        collect_mathtype_object_relationships(&configured_xml, limits)?;
    if !remaining_mathtype_references.ole_ids.is_empty() {
        return Err(DocxError::rejected(vec![Diagnostic::error(
            "TEMPLATE_CONFIGURATION_MATHTYPE_REMAINS",
            Some(DOCUMENT_PART),
            format!(
                "选择的试题区域不完整：区域外仍有 {} 个 MathType 公式。请扩大试题删除范围后重试。",
                remaining_mathtype_references.ole_ids.len()
            ),
        )]));
    }

    let mut replacements = BTreeMap::from([(DOCUMENT_PART.to_owned(), configured_xml)]);
    let mut removals = BTreeSet::new();
    if !source_mathtype_references.ole_ids.is_empty() {
        let sanitized =
            sanitize_mathtype_package_parts(package_bytes, &source_mathtype_references, limits)?;
        replacements.extend(sanitized.0);
        removals.extend(sanitized.1);
    }
    let exported = if removals.is_empty() {
        raw_copy_with_replacements(
            Cursor::new(package_bytes),
            Cursor::new(Vec::new()),
            &replacements,
            limits,
        )?
    } else {
        raw_copy_with_replacements_and_removals(
            Cursor::new(package_bytes),
            Cursor::new(Vec::new()),
            &replacements,
            &removals,
            limits,
        )?
    };
    let bytes = exported.output.into_inner();
    let mut strict_limits = limits.clone();
    strict_limits.allow_mathtype_ole = false;
    let configured_analysis = analyze_template_package_with_requirements(
        Cursor::new(&bytes),
        &["ZT_QUESTIONS"],
        &strict_limits,
    )?;
    if !configured_analysis.is_acceptable()
        || configured_analysis.package_kind != PackageKind::Document
    {
        return Err(DocxError::rejected(configured_analysis.diagnostics));
    }
    reject_unsupported_anchors(&configured_analysis.anchors)?;
    Ok(ConfiguredTemplatePackage {
        bytes,
        analysis: configured_analysis,
        style_profile,
    })
}

fn collect_mathtype_object_relationships(
    document_xml: &[u8],
    limits: &DocxLimits,
) -> DocxResult<MathTypeObjectRelationships> {
    if document_xml.len() as u64 > limits.max_xml_part_bytes {
        return Err(DocxError::LimitExceeded {
            resource: DOCUMENT_PART.to_owned(),
            limit: limits.max_xml_part_bytes,
            actual: document_xml.len() as u64,
        });
    }
    let mut reader = NsReader::from_reader(document_xml);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut current = None::<CurrentObjectRelationships>;
    let mut result = MathTypeObjectRelationships::default();

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
                let local = start.local_name();
                if current.is_none()
                    && namespace == NamespaceKind::Word
                    && local.as_ref() == b"object"
                {
                    current = Some(CurrentObjectRelationships {
                        depth: 1,
                        ..CurrentObjectRelationships::default()
                    });
                } else if let Some(object) = current.as_mut() {
                    object.depth = object.depth.saturating_add(1);
                }
                collect_object_relationship(
                    &reader,
                    start,
                    namespace,
                    local.as_ref(),
                    current.as_mut(),
                )?;
            }
            Event::Empty(ref start) => {
                let namespace = {
                    let (namespace, _) = reader.resolver().resolve_element(start.name());
                    classify_namespace(namespace)
                };
                collect_object_relationship(
                    &reader,
                    start,
                    namespace,
                    start.local_name().as_ref(),
                    current.as_mut(),
                )?;
            }
            Event::End(_) => {
                if let Some(object) = current.as_mut() {
                    object.depth = object.depth.saturating_sub(1);
                    if object.depth == 0 {
                        let completed = current.take().expect("object exists");
                        if !completed.ole_ids.is_empty() {
                            result.ole_ids.extend(completed.ole_ids);
                            result.resource_ids.extend(completed.resource_ids);
                        }
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    Ok(result)
}

fn collect_object_relationship(
    reader: &NsReader<&[u8]>,
    start: &BytesStart<'_>,
    namespace: NamespaceKind,
    local_name: &[u8],
    current: Option<&mut CurrentObjectRelationships>,
) -> DocxResult<()> {
    let Some(object) = current else {
        return Ok(());
    };
    let relationship_id = attribute_value(
        reader,
        start,
        b"id",
        Some(NamespaceKind::OfficeRelationships),
        DOCUMENT_PART,
    )?;
    let Some(relationship_id) = relationship_id else {
        return Ok(());
    };
    object.resource_ids.insert(relationship_id.clone());
    if namespace == NamespaceKind::Office && local_name == b"OLEObject" {
        object.ole_ids.insert(relationship_id);
    }
    Ok(())
}

type SanitizedPackageParts = (BTreeMap<String, Vec<u8>>, BTreeSet<String>);

fn sanitize_mathtype_package_parts(
    package_bytes: &[u8],
    references: &MathTypeObjectRelationships,
    limits: &DocxLimits,
) -> DocxResult<SanitizedPackageParts> {
    let mut archive = ZipArchive::new(Cursor::new(package_bytes))?;
    let relationships_xml =
        read_part_limited(&mut archive, DOCUMENT_RELS_PART, limits.max_xml_part_bytes)?;
    let sanitized_relationships =
        remove_relationships(&relationships_xml, &references.resource_ids, limits)?;
    let content_types_xml =
        read_part_limited(&mut archive, CONTENT_TYPES_PART, limits.max_xml_part_bytes)?;
    let sanitized_content_types = remove_ole_content_types(&content_types_xml, limits)?;

    let mut removals = BTreeSet::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        if entry
            .name()
            .to_ascii_lowercase()
            .starts_with("word/embeddings/")
        {
            removals.insert(entry.name().to_owned());
        }
    }
    if removals.is_empty() {
        return Err(DocxError::rejected(vec![Diagnostic::error(
            "TEMPLATE_MATHTYPE_PARTS_MISSING",
            Some("word/embeddings"),
            "MathType 公式关系存在，但没有找到对应的嵌入对象部件。",
        )]));
    }

    Ok((
        BTreeMap::from([
            (DOCUMENT_RELS_PART.to_owned(), sanitized_relationships),
            (CONTENT_TYPES_PART.to_owned(), sanitized_content_types),
        ]),
        removals,
    ))
}

fn remove_relationships(
    relationships_xml: &[u8],
    relationship_ids: &BTreeSet<String>,
    limits: &DocxLimits,
) -> DocxResult<Vec<u8>> {
    let mut reader = NsReader::from_reader(relationships_xml);
    reader.config_mut().trim_text(false);
    let mut writer = Writer::new(Vec::with_capacity(relationships_xml.len()));
    let mut buffer = Vec::new();
    let mut removed_ids = BTreeSet::new();
    let mut skipped_depth = 0usize;

    loop {
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|error| DocxError::xml(DOCUMENT_RELS_PART, reader.error_position(), error))?;
        if skipped_depth > 0 {
            match event {
                Event::Start(_) => skipped_depth = skipped_depth.saturating_add(1),
                Event::End(_) => skipped_depth = skipped_depth.saturating_sub(1),
                Event::Eof => break,
                _ => {}
            }
            buffer.clear();
            continue;
        }

        let remove = match &event {
            Event::Start(start) | Event::Empty(start) => {
                let namespace = {
                    let (namespace, _) = reader.resolver().resolve_element(start.name());
                    classify_namespace(namespace)
                };
                if namespace == NamespaceKind::Relationships
                    && start.local_name().as_ref() == b"Relationship"
                {
                    let id = attribute_value(&reader, start, b"Id", None, DOCUMENT_RELS_PART)?
                        .unwrap_or_default();
                    if relationship_ids.contains(&id) {
                        removed_ids.insert(id);
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            _ => false,
        };
        let is_eof = matches!(&event, Event::Eof);
        if remove {
            if matches!(&event, Event::Start(_)) {
                skipped_depth = 1;
            }
        } else {
            writer.write_event(event.into_owned())?;
        }
        if is_eof {
            break;
        }
        buffer.clear();
    }

    if removed_ids != *relationship_ids {
        let missing = relationship_ids
            .difference(&removed_ids)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        return Err(DocxError::rejected(vec![Diagnostic::error(
            "TEMPLATE_MATHTYPE_RELATIONSHIP_MISSING",
            Some(DOCUMENT_RELS_PART),
            format!("无法找到需要清理的公式关系：{missing}"),
        )]));
    }
    let output = writer.into_inner();
    let mut strict_limits = limits.clone();
    strict_limits.allow_mathtype_ole = false;
    let mut diagnostics = Vec::new();
    scan_relationships(
        &output,
        DOCUMENT_RELS_PART,
        &strict_limits,
        &mut diagnostics,
    )?;
    if diagnostics
        .iter()
        .any(|item| item.severity == DiagnosticSeverity::Error)
    {
        return Err(DocxError::rejected(diagnostics));
    }
    Ok(output)
}

fn remove_ole_content_types(content_types_xml: &[u8], limits: &DocxLimits) -> DocxResult<Vec<u8>> {
    let mut reader = NsReader::from_reader(content_types_xml);
    reader.config_mut().trim_text(false);
    let mut writer = Writer::new(Vec::with_capacity(content_types_xml.len()));
    let mut buffer = Vec::new();
    let mut skipped_depth = 0usize;

    loop {
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|error| DocxError::xml(CONTENT_TYPES_PART, reader.error_position(), error))?;
        if skipped_depth > 0 {
            match event {
                Event::Start(_) => skipped_depth = skipped_depth.saturating_add(1),
                Event::End(_) => skipped_depth = skipped_depth.saturating_sub(1),
                Event::Eof => break,
                _ => {}
            }
            buffer.clear();
            continue;
        }

        let remove = match &event {
            Event::Start(start) | Event::Empty(start) => {
                let namespace = {
                    let (namespace, _) = reader.resolver().resolve_element(start.name());
                    classify_namespace(namespace)
                };
                if namespace == NamespaceKind::ContentTypes
                    && (start.local_name().as_ref() == b"Default"
                        || start.local_name().as_ref() == b"Override")
                {
                    let content_type =
                        attribute_value(&reader, start, b"ContentType", None, CONTENT_TYPES_PART)?
                            .unwrap_or_default()
                            .to_ascii_lowercase();
                    let part_name =
                        attribute_value(&reader, start, b"PartName", None, CONTENT_TYPES_PART)?
                            .unwrap_or_default()
                            .to_ascii_lowercase();
                    content_type.contains("oleobject") || part_name.starts_with("/word/embeddings/")
                } else {
                    false
                }
            }
            _ => false,
        };
        let is_eof = matches!(&event, Event::Eof);
        if remove {
            if matches!(&event, Event::Start(_)) {
                skipped_depth = 1;
            }
        } else {
            writer.write_event(event.into_owned())?;
        }
        if is_eof {
            break;
        }
        buffer.clear();
    }

    let output = writer.into_inner();
    let mut strict_limits = limits.clone();
    strict_limits.allow_mathtype_ole = false;
    parse_content_types(&output, &strict_limits)?;
    Ok(output)
}

fn acceptable_document_analysis(
    package_bytes: &[u8],
    limits: &DocxLimits,
) -> DocxResult<TemplatePackageAnalysis> {
    let analysis = analyze_template_package(Cursor::new(package_bytes), limits)?;
    if !analysis.is_acceptable() {
        return Err(DocxError::rejected(analysis.diagnostics));
    }
    if analysis.package_kind != PackageKind::Document {
        return Err(DocxError::rejected(vec![Diagnostic::error(
            "TEMPLATE_CONFIGURATION_PACKAGE_UNSUPPORTED",
            None::<String>,
            "only .docx document templates can be configured",
        )]));
    }
    Ok(analysis)
}

fn placement_edit(
    placement: &TemplateRegionPlacement,
    anchor_name: &'static str,
    context: &PlacementEditContext<'_>,
) -> DocxResult<XmlEdit> {
    let marker = marker_paragraph(context.namespace, anchor_name).into_bytes();
    match placement {
        TemplateRegionPlacement::DocumentEnd => Ok(XmlEdit {
            span: ByteSpan::new(context.body_insert_offset, context.body_insert_offset),
            bytes: marker,
            label: anchor_name,
        }),
        TemplateRegionPlacement::ReplaceRange {
            start_paragraph,
            end_paragraph,
        } => {
            if anchor_name != "ZT_QUESTIONS" {
                return Err(DocxError::rejected(vec![Diagnostic::error(
                    "TEMPLATE_CONFIGURATION_RANGE_KIND_UNSUPPORTED",
                    Some(DOCUMENT_PART),
                    "only the questions region can replace a paragraph range",
                )]));
            }
            let (start, end) =
                selected_paragraph_range(*start_paragraph, *end_paragraph, context.paragraphs)?;
            if context.anchors.iter().any(|anchor| {
                anchor.container_span.start < end.source_span.end
                    && start.source_span.start < anchor.container_span.end
            }) {
                return Err(DocxError::rejected(vec![Diagnostic::error(
                    "TEMPLATE_CONFIGURATION_RANGE_CONTAINS_ANCHOR",
                    Some(DOCUMENT_PART),
                    "the selected questions range contains an existing template region",
                )]));
            }
            Ok(XmlEdit {
                span: ByteSpan::new(start.source_span.start, end.source_span.end),
                bytes: content_control(context.namespace, anchor_name).into_bytes(),
                label: anchor_name,
            })
        }
        TemplateRegionPlacement::ReplaceParagraph(index)
        | TemplateRegionPlacement::AfterParagraph(index) => {
            let paragraph = context
                .paragraphs
                .iter()
                .find(|paragraph| paragraph.index == *index)
                .ok_or_else(|| {
                    DocxError::rejected(vec![Diagnostic::error(
                        "TEMPLATE_CONFIGURATION_PARAGRAPH_NOT_FOUND",
                        Some(DOCUMENT_PART),
                        format!("paragraph {index} no longer exists in the managed template"),
                    )])
                })?;
            if context
                .anchors
                .iter()
                .any(|anchor| anchor.container_span.contains(paragraph.source_span))
            {
                return Err(DocxError::rejected(vec![Diagnostic::error(
                    "TEMPLATE_CONFIGURATION_PARAGRAPH_IS_ANCHOR",
                    Some(DOCUMENT_PART),
                    format!("paragraph {index} is already inside a template replacement region"),
                )]));
            }
            let span = match placement {
                TemplateRegionPlacement::ReplaceParagraph(_) => paragraph.source_span,
                TemplateRegionPlacement::AfterParagraph(_) => {
                    ByteSpan::new(paragraph.source_span.end, paragraph.source_span.end)
                }
                TemplateRegionPlacement::ReplaceRange { .. } => unreachable!(),
                TemplateRegionPlacement::DocumentEnd => unreachable!(),
            };
            let bytes = if matches!(placement, TemplateRegionPlacement::ReplaceParagraph(_))
                && anchor_name == "ZT_TITLE"
            {
                let source = &context.document_xml[paragraph.source_span.as_range()];
                if let Some(mut preserved) =
                    preserve_floating_object_paragraph(source, context.namespace, context.limits)?
                {
                    preserved.extend_from_slice(&marker);
                    preserved
                } else {
                    marker
                }
            } else {
                marker
            };
            Ok(XmlEdit {
                span,
                bytes,
                label: anchor_name,
            })
        }
    }
}

fn marker_paragraph(namespace: &str, anchor_name: &str) -> String {
    format!(r#"<w:p xmlns:w="{namespace}"><w:r><w:t>{{{{{anchor_name}}}}}</w:t></w:r></w:p>"#)
}

fn content_control(namespace: &str, anchor_name: &str) -> String {
    format!(
        r#"<w:sdt xmlns:w="{namespace}"><w:sdtPr><w:tag w:val="{anchor_name}"/></w:sdtPr><w:sdtContent><w:p/></w:sdtContent></w:sdt>"#
    )
}

fn is_floating_object_name(local_name: &[u8]) -> bool {
    matches!(
        local_name,
        b"drawing" | b"pict" | b"object" | b"AlternateContent"
    )
}

fn is_side_seal_paragraph(paragraph_xml: &[u8]) -> bool {
    let text = String::from_utf8_lossy(paragraph_xml);
    text.contains("班级")
        && text.contains("姓名")
        && text.contains("考号")
        && text.contains('密')
        && text.contains('封')
        && text.contains('线')
}

#[derive(Default)]
struct SideSealRunBuilder {
    text: String,
    underline: bool,
    size_half_points: Option<u32>,
}

#[derive(Default)]
struct SideSealLineBuilder {
    alignment: Option<String>,
    runs: Vec<TemplateSideSealRun>,
}

#[derive(Default)]
struct SideSealShapeBuilder {
    preset: Option<String>,
    offset_x: Option<i64>,
    extent_x: Option<i64>,
    extent_y: Option<i64>,
    line_width_emu: Option<u64>,
    line_dash: Option<String>,
    contains_text_box: bool,
    text_direction: Option<String>,
}

fn namespace_matches(namespace: &ResolveResult<'_>, expected: &[u8]) -> bool {
    matches!(namespace, ResolveResult::Bound(Namespace(uri)) if *uri == expected)
}

fn parse_i64_attribute(
    reader: &NsReader<&[u8]>,
    start: &quick_xml::events::BytesStart<'_>,
    local_name: &[u8],
) -> DocxResult<Option<i64>> {
    Ok(
        attribute_value(reader, start, local_name, None, DOCUMENT_PART)?
            .and_then(|value| value.parse::<i64>().ok()),
    )
}

fn parse_u64_attribute(
    reader: &NsReader<&[u8]>,
    start: &quick_xml::events::BytesStart<'_>,
    local_name: &[u8],
) -> DocxResult<Option<u64>> {
    Ok(
        attribute_value(reader, start, local_name, None, DOCUMENT_PART)?
            .and_then(|value| value.parse::<u64>().ok()),
    )
}

fn resolved_side_seal_axis(
    relative_from: Option<&str>,
    offset_emu: Option<i64>,
    margin_twips: u32,
    paragraph_index: usize,
    horizontal: bool,
) -> Option<i64> {
    let offset = offset_emu?;
    match relative_from? {
        "page" => Some(offset),
        "margin" | "column" => Some(i64::from(margin_twips) * EMU_PER_TWIP + offset),
        "leftMargin" if horizontal => Some(offset),
        "topMargin" if !horizontal => Some(offset),
        "paragraph" if !horizontal && paragraph_index == 0 => {
            Some(i64::from(margin_twips) * EMU_PER_TWIP + offset)
        }
        _ => None,
    }
}

fn side_seal_namespace_wrapper(paragraph_xml: &[u8]) -> Vec<u8> {
    let mut wrapped = Vec::with_capacity(paragraph_xml.len() + 512);
    wrapped.extend_from_slice(
        br#"<root xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:wpg="http://schemas.microsoft.com/office/word/2010/wordprocessingGroup" xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape">"#,
    );
    wrapped.extend_from_slice(paragraph_xml);
    wrapped.extend_from_slice(b"</root>");
    wrapped
}

fn extract_side_seal_layout(
    paragraph_xml: &[u8],
    paragraph_index: usize,
    page: &TemplatePageSetup,
    limits: &DocxLimits,
) -> DocxResult<Option<TemplateSideSealLayout>> {
    // parse_document_xml returns a paragraph slice, not the document root, so
    // namespace declarations inherited from w:document are no longer present.
    // Supply the namespaces used by the DrawingML side-seal fragment before
    // asking NsReader to classify its elements.
    let wrapped_xml = side_seal_namespace_wrapper(paragraph_xml);
    let mut reader = NsReader::from_reader(wrapped_xml.as_slice());
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut depth = 0usize;
    let mut anchor_depth = None;
    let mut horizontal_position_depth = None;
    let mut vertical_position_depth = None;
    let mut offset_capture: Option<(usize, bool)> = None;
    let mut horizontal_relative_from = None;
    let mut vertical_relative_from = None;
    let mut horizontal_offset_emu = None;
    let mut vertical_offset_emu = None;
    let mut width_emu = None;
    let mut height_emu = None;
    let mut text_direction = None;
    let mut text_box_depth = None;
    let mut paragraph_depth = None;
    let mut paragraph_properties_depth = None;
    let mut line_builder: Option<SideSealLineBuilder> = None;
    let mut run_depth = None;
    let mut run_builder: Option<SideSealRunBuilder> = None;
    let mut text_depth = None;
    let mut lines = Vec::new();
    let mut group_properties_depth = None;
    let mut group_child_offset_x = None;
    let mut group_child_extent_x = None;
    let mut shape_depth = None;
    let mut shape_builder: Option<SideSealShapeBuilder> = None;
    let mut line_local_x = None;
    let mut line_width_emu = None;
    let mut line_dash = None;

    loop {
        let event_start = reader.buffer_position();
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|error| DocxError::xml(DOCUMENT_PART, reader.error_position(), error))?;
        match event {
            Event::Start(ref start) | Event::Empty(ref start) => {
                let is_empty = matches!(event, Event::Empty(_));
                depth += 1;
                if depth > limits.max_xml_depth {
                    return Err(DocxError::LimitExceeded {
                        resource: "XML depth in side-seal drawing".to_owned(),
                        limit: limits.max_xml_depth as u64,
                        actual: depth as u64,
                    });
                }
                let local = start.local_name();
                let (namespace, _) = reader.resolver().resolve_element(start.name());
                let namespace_kind = classify_namespace(namespace.clone());
                let is_word = namespace_kind == NamespaceKind::Word;
                let is_drawing = namespace_kind == NamespaceKind::Drawing;
                let is_wordprocessing_drawing =
                    namespace_matches(&namespace, WORDPROCESSING_DRAWING_NS);
                let is_wordprocessing_group =
                    namespace_matches(&namespace, WORDPROCESSING_GROUP_NS);
                let is_wordprocessing_shape =
                    namespace_matches(&namespace, WORDPROCESSING_SHAPE_NS);

                if is_wordprocessing_drawing && local.as_ref() == b"anchor" {
                    anchor_depth = Some(depth);
                } else if anchor_depth.is_some()
                    && is_wordprocessing_drawing
                    && local.as_ref() == b"positionH"
                {
                    horizontal_position_depth = Some(depth);
                    horizontal_relative_from =
                        attribute_value(&reader, start, b"relativeFrom", None, DOCUMENT_PART)?;
                } else if anchor_depth.is_some()
                    && is_wordprocessing_drawing
                    && local.as_ref() == b"positionV"
                {
                    vertical_position_depth = Some(depth);
                    vertical_relative_from =
                        attribute_value(&reader, start, b"relativeFrom", None, DOCUMENT_PART)?;
                } else if anchor_depth.is_some()
                    && is_wordprocessing_drawing
                    && local.as_ref() == b"posOffset"
                {
                    if horizontal_position_depth.is_some() {
                        offset_capture = Some((depth, true));
                    } else if vertical_position_depth.is_some() {
                        offset_capture = Some((depth, false));
                    }
                } else if anchor_depth.is_some()
                    && is_wordprocessing_drawing
                    && local.as_ref() == b"extent"
                {
                    width_emu = parse_u64_attribute(&reader, start, b"cx")?;
                    height_emu = parse_u64_attribute(&reader, start, b"cy")?;
                }

                if is_wordprocessing_group && local.as_ref() == b"grpSpPr" {
                    group_properties_depth = Some(depth);
                } else if group_properties_depth.is_some()
                    && shape_depth.is_none()
                    && is_drawing
                    && local.as_ref() == b"chOff"
                {
                    group_child_offset_x = parse_i64_attribute(&reader, start, b"x")?;
                } else if group_properties_depth.is_some()
                    && shape_depth.is_none()
                    && is_drawing
                    && local.as_ref() == b"chExt"
                {
                    group_child_extent_x = parse_i64_attribute(&reader, start, b"cx")?;
                }

                if is_wordprocessing_shape && local.as_ref() == b"wsp" {
                    shape_depth = Some(depth);
                    shape_builder = Some(SideSealShapeBuilder::default());
                } else if let Some(shape) = shape_builder.as_mut() {
                    if is_drawing && local.as_ref() == b"prstGeom" {
                        shape.preset =
                            attribute_value(&reader, start, b"prst", None, DOCUMENT_PART)?;
                    } else if is_drawing && local.as_ref() == b"off" && shape.offset_x.is_none() {
                        shape.offset_x = parse_i64_attribute(&reader, start, b"x")?;
                    } else if is_drawing && local.as_ref() == b"ext" && shape.extent_x.is_none() {
                        shape.extent_x = parse_i64_attribute(&reader, start, b"cx")?;
                        shape.extent_y = parse_i64_attribute(&reader, start, b"cy")?;
                    } else if is_drawing && local.as_ref() == b"ln" {
                        shape.line_width_emu = parse_u64_attribute(&reader, start, b"w")?;
                    } else if is_drawing && local.as_ref() == b"prstDash" {
                        shape.line_dash =
                            attribute_value(&reader, start, b"val", None, DOCUMENT_PART)?;
                    } else if is_wordprocessing_shape && local.as_ref() == b"txbx" {
                        shape.contains_text_box = true;
                    }
                }

                if is_wordprocessing_shape
                    && local.as_ref() == b"bodyPr"
                    && let Some(shape) = shape_builder.as_mut()
                    && let Some(direction) =
                        attribute_value(&reader, start, b"vert", None, DOCUMENT_PART)?
                {
                    shape.text_direction = Some(direction);
                }

                if is_word && local.as_ref() == b"txbxContent" {
                    text_box_depth = Some(depth);
                } else if text_box_depth.is_some()
                    && is_word
                    && local.as_ref() == b"p"
                    && paragraph_depth.is_none()
                {
                    paragraph_depth = Some(depth);
                    line_builder = Some(SideSealLineBuilder::default());
                } else if paragraph_depth.is_some()
                    && is_word
                    && local.as_ref() == b"pPr"
                    && paragraph_properties_depth.is_none()
                {
                    paragraph_properties_depth = Some(depth);
                } else if paragraph_properties_depth.is_some()
                    && run_depth.is_none()
                    && is_word
                    && local.as_ref() == b"jc"
                {
                    if let Some(line) = line_builder.as_mut() {
                        line.alignment =
                            attribute_value(&reader, start, b"val", None, DOCUMENT_PART)?;
                    }
                } else if paragraph_depth.is_some()
                    && paragraph_properties_depth.is_none()
                    && is_word
                    && local.as_ref() == b"r"
                    && run_depth.is_none()
                {
                    run_depth = Some(depth);
                    run_builder = Some(SideSealRunBuilder::default());
                } else if run_depth.is_some() && is_word && local.as_ref() == b"u" {
                    if let Some(run) = run_builder.as_mut() {
                        let value = attribute_value(&reader, start, b"val", None, DOCUMENT_PART)?;
                        run.underline =
                            !matches!(value.as_deref(), Some("0" | "false" | "off" | "none"));
                    }
                } else if run_depth.is_some() && is_word && local.as_ref() == b"sz" {
                    if let Some(run) = run_builder.as_mut() {
                        run.size_half_points =
                            attribute_value(&reader, start, b"val", None, DOCUMENT_PART)?
                                .and_then(|value| value.parse::<u32>().ok());
                    }
                } else if run_depth.is_some() && is_word && local.as_ref() == b"t" {
                    text_depth = Some(depth);
                }

                if is_empty {
                    if text_depth == Some(depth) {
                        text_depth = None;
                    }
                    depth = depth.saturating_sub(1);
                }
            }
            Event::Text(ref text) => {
                if let Some((_, horizontal)) = offset_capture {
                    let decoded = text
                        .xml10_content()
                        .map_err(|error| DocxError::xml(DOCUMENT_PART, event_start, error))?;
                    if let Ok(value) = decoded.trim().parse::<i64>() {
                        if horizontal {
                            horizontal_offset_emu = Some(value);
                        } else {
                            vertical_offset_emu = Some(value);
                        }
                    }
                } else if text_depth.is_some()
                    && let Some(run) = run_builder.as_mut()
                {
                    run.text.push_str(
                        &text
                            .xml10_content()
                            .map_err(|error| DocxError::xml(DOCUMENT_PART, event_start, error))?,
                    );
                }
            }
            Event::CData(ref text) => {
                if text_depth.is_some()
                    && let Some(run) = run_builder.as_mut()
                {
                    run.text.push_str(
                        &text
                            .decode()
                            .map_err(|error| DocxError::xml(DOCUMENT_PART, event_start, error))?,
                    );
                }
            }
            Event::End(ref end) => {
                let local = end.local_name();
                let (namespace, _) = reader.resolver().resolve_element(end.name());
                let is_word = classify_namespace(namespace.clone()) == NamespaceKind::Word;
                let is_wordprocessing_drawing =
                    namespace_matches(&namespace, WORDPROCESSING_DRAWING_NS);
                let is_wordprocessing_group =
                    namespace_matches(&namespace, WORDPROCESSING_GROUP_NS);
                let is_wordprocessing_shape =
                    namespace_matches(&namespace, WORDPROCESSING_SHAPE_NS);

                if text_depth == Some(depth) && is_word && local.as_ref() == b"t" {
                    text_depth = None;
                }
                if run_depth == Some(depth) && is_word && local.as_ref() == b"r" {
                    if let Some(run) = run_builder.take()
                        && !run.text.is_empty()
                        && let Some(line) = line_builder.as_mut()
                    {
                        line.runs.push(TemplateSideSealRun {
                            text: run.text,
                            underline: run.underline,
                            size_half_points: run.size_half_points,
                        });
                    }
                    run_depth = None;
                }
                if paragraph_properties_depth == Some(depth) && is_word && local.as_ref() == b"pPr"
                {
                    paragraph_properties_depth = None;
                }
                if paragraph_depth == Some(depth) && is_word && local.as_ref() == b"p" {
                    if let Some(line) = line_builder.take()
                        && !line.runs.is_empty()
                    {
                        lines.push(TemplateSideSealLine {
                            alignment: line.alignment,
                            runs: line.runs,
                        });
                    }
                    paragraph_depth = None;
                }
                if text_box_depth == Some(depth) && is_word && local.as_ref() == b"txbxContent" {
                    text_box_depth = None;
                }

                if shape_depth == Some(depth) && is_wordprocessing_shape && local.as_ref() == b"wsp"
                {
                    if let Some(shape) = shape_builder.take() {
                        if shape.contains_text_box && shape.text_direction.is_some() {
                            text_direction = shape.text_direction.clone();
                        }
                        if shape.preset.as_deref() == Some("line")
                            && shape.extent_y.unwrap_or_default() > 0
                            && shape.extent_x.unwrap_or_default() == 0
                        {
                            line_local_x = shape.offset_x;
                            line_width_emu = shape.line_width_emu;
                            line_dash = shape.line_dash;
                        }
                    }
                    shape_depth = None;
                }
                if group_properties_depth == Some(depth)
                    && is_wordprocessing_group
                    && local.as_ref() == b"grpSpPr"
                {
                    group_properties_depth = None;
                }
                if horizontal_position_depth == Some(depth)
                    && is_wordprocessing_drawing
                    && local.as_ref() == b"positionH"
                {
                    horizontal_position_depth = None;
                }
                if vertical_position_depth == Some(depth)
                    && is_wordprocessing_drawing
                    && local.as_ref() == b"positionV"
                {
                    vertical_position_depth = None;
                }
                if offset_capture.is_some_and(|(capture_depth, _)| capture_depth == depth)
                    && is_wordprocessing_drawing
                    && local.as_ref() == b"posOffset"
                {
                    offset_capture = None;
                }
                if anchor_depth == Some(depth)
                    && is_wordprocessing_drawing
                    && local.as_ref() == b"anchor"
                {
                    anchor_depth = None;
                }
                depth = depth.saturating_sub(1);
            }
            Event::DocType(_) => {
                return Err(DocxError::xml(
                    DOCUMENT_PART,
                    event_start,
                    "DOCTYPE declarations are not allowed",
                ));
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }

    let (Some(width_emu), Some(height_emu)) = (width_emu, height_emu) else {
        return Ok(None);
    };
    if width_emu == 0 || height_emu == 0 {
        return Ok(None);
    }
    let line_x_emu = match (line_local_x, group_child_offset_x, group_child_extent_x) {
        (Some(line_x), Some(group_x), Some(group_width)) if group_width > 0 => {
            let scaled =
                i128::from(line_x - group_x) * i128::from(width_emu) / i128::from(group_width);
            i64::try_from(scaled).ok()
        }
        _ => None,
    };
    let page_x_emu = resolved_side_seal_axis(
        horizontal_relative_from.as_deref(),
        horizontal_offset_emu,
        page.margin_left_twips,
        paragraph_index,
        true,
    );
    let page_y_emu = resolved_side_seal_axis(
        vertical_relative_from.as_deref(),
        vertical_offset_emu,
        page.margin_top_twips,
        paragraph_index,
        false,
    );
    Ok(Some(TemplateSideSealLayout {
        horizontal_relative_from,
        vertical_relative_from,
        horizontal_offset_emu,
        vertical_offset_emu,
        page_x_emu,
        page_y_emu,
        width_emu,
        height_emu,
        text_direction,
        line_x_emu,
        line_width_emu,
        line_dash,
        lines,
    }))
}

/// Keeps direct children that contain floating DrawingML/VML objects, while
/// removing ordinary visible text runs and collapsing the anchor paragraph to
/// one twip. Word
/// commonly anchors a side seal or school logo in the same paragraph as the
/// exam title; replacing that whole paragraph would otherwise delete the
/// floating object together with the old title.
fn preserve_floating_object_paragraph(
    paragraph_xml: &[u8],
    namespace: &str,
    limits: &DocxLimits,
) -> DocxResult<Option<Vec<u8>>> {
    #[derive(Clone, Copy)]
    struct ChildCapture {
        start: usize,
        depth: usize,
        keep: bool,
    }

    let mut reader = NsReader::from_reader(paragraph_xml);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut depth = 0usize;
    let mut paragraph_open_end = None;
    let mut paragraph_close = None;
    let mut child: Option<ChildCapture> = None;
    let mut kept_children = Vec::<ByteSpan>::new();
    let mut saw_floating_object = false;

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
                        resource: "XML depth in floating template object".to_owned(),
                        limit: limits.max_xml_depth as u64,
                        actual: depth as u64,
                    });
                }
                if depth == 1 && start.local_name().as_ref() == b"p" {
                    paragraph_open_end = Some(event_end);
                } else if depth == 2 {
                    child = Some(ChildCapture {
                        start: event_start,
                        depth,
                        keep: false,
                    });
                }
                if is_floating_object_name(start.local_name().as_ref()) {
                    saw_floating_object = true;
                    if let Some(capture) = child.as_mut() {
                        capture.keep = true;
                    }
                }
            }
            Event::Empty(ref start) => {
                if depth == 1 {
                    let keep = is_floating_object_name(start.local_name().as_ref());
                    if keep {
                        kept_children.push(ByteSpan::new(event_start, event_end));
                    }
                    saw_floating_object |= is_floating_object_name(start.local_name().as_ref());
                } else if is_floating_object_name(start.local_name().as_ref()) {
                    saw_floating_object = true;
                    if let Some(capture) = child.as_mut() {
                        capture.keep = true;
                    }
                }
            }
            Event::End(ref end) => {
                if end.local_name().as_ref() == b"p" && depth == 1 {
                    paragraph_close = Some(ByteSpan::new(event_start, event_end));
                }
                if child.is_some_and(|capture| capture.depth == depth) {
                    let capture = child.take().expect("direct child capture must exist");
                    if capture.keep {
                        kept_children.push(ByteSpan::new(capture.start, event_end));
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

    if !saw_floating_object {
        return Ok(None);
    }
    let open_end = paragraph_open_end.ok_or_else(|| {
        DocxError::xml(
            DOCUMENT_PART,
            0,
            "template title paragraph has no opening w:p element",
        )
    })?;
    let close = paragraph_close.ok_or_else(|| {
        DocxError::xml(
            DOCUMENT_PART,
            paragraph_xml.len() as u64,
            "template title paragraph has no closing w:p element",
        )
    })?;
    let mut preserved = Vec::new();
    preserved.extend_from_slice(&paragraph_xml[..open_end]);
    preserved.extend_from_slice(
        format!(
            r#"<ztw:pPr xmlns:ztw="{namespace}"><ztw:spacing ztw:before="0" ztw:after="0" ztw:line="1" ztw:lineRule="exact"/></ztw:pPr>"#
        )
        .as_bytes(),
    );
    for span in kept_children {
        preserved.extend_from_slice(&paragraph_xml[span.as_range()]);
    }
    preserved.extend_from_slice(&paragraph_xml[close.as_range()]);
    Ok(Some(preserved))
}

fn build_style_profile(
    document_xml: &[u8],
    paragraphs: &[super::Paragraph],
    configuration: &TemplateRegionConfiguration,
    limits: &DocxLimits,
) -> DocxResult<Option<TemplateStyleProfile>> {
    let source_range = match configuration.questions {
        TemplateRegionPlacement::ReplaceRange {
            start_paragraph,
            end_paragraph,
        } => {
            selected_paragraph_range(start_paragraph, end_paragraph, paragraphs)?;
            Some(TemplateSourceRange {
                start_paragraph_index: start_paragraph,
                end_paragraph_index: end_paragraph,
            })
        }
        _ => None,
    };
    let default_question = source_range
        .as_ref()
        .map(|range| range.start_paragraph_index);
    let title_sample = configuration
        .title
        .as_ref()
        .and_then(|placement| match placement {
            TemplateRegionPlacement::ReplaceParagraph(index)
            | TemplateRegionPlacement::AfterParagraph(index) => Some(*index),
            TemplateRegionPlacement::ReplaceRange { .. } | TemplateRegionPlacement::DocumentEnd => {
                None
            }
        });
    let samples = [
        ("title", title_sample, false),
        ("sectionHeading", configuration.styles.section_heading, true),
        (
            "question",
            configuration.styles.question.or(default_question),
            true,
        ),
        ("option", configuration.styles.option, true),
        ("answer", configuration.styles.answer, true),
        ("explanation", configuration.styles.explanation, true),
    ];
    let mut roles = BTreeMap::new();
    for (role, paragraph_index, must_be_in_questions_range) in samples {
        let Some(paragraph_index) = paragraph_index else {
            continue;
        };
        if must_be_in_questions_range
            && source_range.as_ref().is_some_and(|range| {
                paragraph_index < range.start_paragraph_index
                    || paragraph_index > range.end_paragraph_index
            })
        {
            return Err(DocxError::rejected(vec![Diagnostic::error(
                "TEMPLATE_STYLE_SAMPLE_OUTSIDE_QUESTIONS_RANGE",
                Some(DOCUMENT_PART),
                format!(
                    "style sample '{role}' uses paragraph {paragraph_index}, which is outside the selected questions range"
                ),
            )]));
        }
        let paragraph = paragraphs
            .iter()
            .find(|paragraph| paragraph.index == paragraph_index)
            .ok_or_else(|| {
                DocxError::rejected(vec![Diagnostic::error(
                    "TEMPLATE_STYLE_SAMPLE_NOT_FOUND",
                    Some(DOCUMENT_PART),
                    format!("style sample '{role}' refers to missing paragraph {paragraph_index}"),
                )])
            })?;
        let source = &document_xml[paragraph.source_span.as_range()];
        let (paragraph_properties, run_properties) =
            extract_paragraph_style_properties(source, limits)?;
        roles.insert(
            role.to_owned(),
            ParagraphStylePrototype {
                source_paragraph_index: paragraph_index,
                paragraph_properties,
                run_properties,
            },
        );
    }
    if roles.is_empty() {
        Ok(None)
    } else {
        Ok(Some(TemplateStyleProfile {
            schema_version: 1,
            source_range,
            roles,
        }))
    }
}

fn selected_paragraph_range(
    start_paragraph: usize,
    end_paragraph: usize,
    paragraphs: &[super::Paragraph],
) -> DocxResult<(&super::Paragraph, &super::Paragraph)> {
    if start_paragraph > end_paragraph {
        return Err(DocxError::rejected(vec![Diagnostic::error(
            "TEMPLATE_CONFIGURATION_RANGE_REVERSED",
            Some(DOCUMENT_PART),
            "the questions range start must not come after its end",
        )]));
    }
    let start = paragraphs
        .iter()
        .find(|paragraph| paragraph.index == start_paragraph)
        .ok_or_else(|| {
            DocxError::rejected(vec![Diagnostic::error(
                "TEMPLATE_CONFIGURATION_RANGE_START_NOT_FOUND",
                Some(DOCUMENT_PART),
                format!("questions range start paragraph {start_paragraph} does not exist"),
            )])
        })?;
    let end = paragraphs
        .iter()
        .find(|paragraph| paragraph.index == end_paragraph)
        .ok_or_else(|| {
            DocxError::rejected(vec![Diagnostic::error(
                "TEMPLATE_CONFIGURATION_RANGE_END_NOT_FOUND",
                Some(DOCUMENT_PART),
                format!("questions range end paragraph {end_paragraph} does not exist"),
            )])
        })?;
    Ok((start, end))
}

fn extract_paragraph_style_properties(
    paragraph_xml: &[u8],
    limits: &DocxLimits,
) -> DocxResult<(Vec<u8>, Vec<u8>)> {
    // The paragraph slice inherits its namespace declarations from the full
    // document root, so a standalone namespace resolver cannot classify its
    // `w:` prefix. The complete document has already passed the namespace-aware
    // parser; local names are therefore safe for this narrow property capture.
    #[derive(Clone, Copy)]
    struct Capture {
        start: usize,
        depth: usize,
    }

    let mut reader = NsReader::from_reader(paragraph_xml);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut depth = 0usize;
    let mut paragraph_properties = None;
    let mut run_properties = None;
    let mut paragraph_capture: Option<Capture> = None;
    let mut run_capture: Option<Capture> = None;
    let mut paragraph_properties_depth = None;
    let mut active_run_depth = None;
    let mut active_run_properties = None;
    let mut active_run_has_floating_object = false;
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
                        resource: "XML depth in template style sample".to_owned(),
                        limit: limits.max_xml_depth as u64,
                        actual: depth as u64,
                    });
                }
                match start.local_name().as_ref() {
                    b"pPr" if paragraph_properties.is_none() && paragraph_capture.is_none() => {
                        paragraph_capture = Some(Capture {
                            start: event_start,
                            depth,
                        });
                        paragraph_properties_depth = Some(depth);
                    }
                    b"r" if active_run_depth.is_none() => {
                        active_run_depth = Some(depth);
                        active_run_properties = None;
                        active_run_has_floating_object = false;
                    }
                    b"rPr"
                        if run_properties.is_none()
                            && run_capture.is_none()
                            && active_run_depth.is_some()
                            && paragraph_properties_depth.is_none() =>
                    {
                        run_capture = Some(Capture {
                            start: event_start,
                            depth,
                        });
                    }
                    _ => {}
                }
                if is_floating_object_name(start.local_name().as_ref())
                    && active_run_depth.is_some()
                {
                    active_run_has_floating_object = true;
                }
            }
            Event::Empty(ref start) => match start.local_name().as_ref() {
                b"pPr" if paragraph_properties.is_none() => {
                    paragraph_properties = Some(paragraph_xml[event_start..event_end].to_vec());
                }
                b"rPr"
                    if run_properties.is_none()
                        && active_run_depth.is_some()
                        && paragraph_properties_depth.is_none() =>
                {
                    active_run_properties = Some(paragraph_xml[event_start..event_end].to_vec());
                }
                _ => {}
            },
            Event::End(ref end) => {
                match end.local_name().as_ref() {
                    b"pPr" if paragraph_capture.is_some_and(|capture| capture.depth == depth) => {
                        let capture = paragraph_capture
                            .take()
                            .expect("paragraph style capture must exist");
                        paragraph_properties =
                            Some(paragraph_xml[capture.start..event_end].to_vec());
                        paragraph_properties_depth = None;
                    }
                    b"rPr" if run_capture.is_some_and(|capture| capture.depth == depth) => {
                        let capture = run_capture.take().expect("run style capture must exist");
                        active_run_properties =
                            Some(paragraph_xml[capture.start..event_end].to_vec());
                    }
                    b"r" if active_run_depth == Some(depth) => {
                        if run_properties.is_none() && !active_run_has_floating_object {
                            run_properties = active_run_properties.take();
                        }
                        active_run_depth = None;
                        active_run_properties = None;
                        active_run_has_floating_object = false;
                    }
                    _ => {}
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
    Ok((
        paragraph_properties.unwrap_or_default(),
        run_properties.unwrap_or_default(),
    ))
}

fn read_template_preview_metadata(
    package_bytes: &[u8],
    limits: &DocxLimits,
) -> DocxResult<(TemplateFontTheme, Option<String>)> {
    let mut archive = ZipArchive::new(Cursor::new(package_bytes))?;
    let mut theme_part = None;
    let mut header_footer_parts = Vec::new();
    for index in 0..archive.len() {
        let file = archive.by_index_raw(index)?;
        let name = file.name().to_owned();
        drop(file);
        if name.starts_with("word/theme/") && name.ends_with(".xml") && theme_part.is_none() {
            theme_part = Some(name);
        } else if (name.starts_with("word/header") || name.starts_with("word/footer"))
            && name.ends_with(".xml")
        {
            header_footer_parts.push(name);
        }
    }

    let font_theme = if let Some(part_name) = theme_part {
        let xml = read_bounded_archive_part(&mut archive, &part_name, limits)?;
        parse_font_theme(&xml, &part_name, limits)?
    } else {
        TemplateFontTheme::default()
    };

    let mut has_page = false;
    let mut has_page_count = false;
    for part_name in header_footer_parts {
        let xml = read_bounded_archive_part(&mut archive, &part_name, limits)?;
        let (part_has_page, part_has_page_count) =
            detect_page_number_fields(&xml, &part_name, limits)?;
        has_page |= part_has_page;
        has_page_count |= part_has_page_count;
    }
    let page_number_format = if has_page_count {
        Some("第 {pageNo} 页，共 {pageCount} 页".to_owned())
    } else if has_page {
        Some("第 {pageNo} 页".to_owned())
    } else {
        None
    };
    Ok((font_theme, page_number_format))
}

fn read_bounded_archive_part<R: std::io::Read + std::io::Seek>(
    archive: &mut ZipArchive<R>,
    part_name: &str,
    limits: &DocxLimits,
) -> DocxResult<Vec<u8>> {
    let mut file = archive.by_name(part_name)?;
    if file.size() > limits.max_xml_part_bytes {
        return Err(DocxError::LimitExceeded {
            resource: part_name.to_owned(),
            limit: limits.max_xml_part_bytes,
            actual: file.size(),
        });
    }
    let mut bytes = Vec::with_capacity(usize::try_from(file.size()).unwrap_or_default());
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn parse_font_theme(
    xml: &[u8],
    part_name: &str,
    limits: &DocxLimits,
) -> DocxResult<TemplateFontTheme> {
    let mut reader = NsReader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut depth = 0usize;
    let mut font_group: Option<bool> = None;
    let mut result = TemplateFontTheme::default();

    loop {
        let event_start = reader.buffer_position();
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|error| DocxError::xml(part_name, reader.error_position(), error))?;
        match event {
            Event::Start(ref start) => {
                depth += 1;
                if depth > limits.max_xml_depth {
                    return Err(DocxError::LimitExceeded {
                        resource: format!("XML depth in {part_name}"),
                        limit: limits.max_xml_depth as u64,
                        actual: depth as u64,
                    });
                }
                let namespace = {
                    let (namespace, _) = reader.resolver().resolve_element(start.name());
                    classify_namespace(namespace)
                };
                if namespace == NamespaceKind::Drawing {
                    match start.local_name().as_ref() {
                        b"majorFont" => font_group = Some(true),
                        b"minorFont" => font_group = Some(false),
                        _ => {}
                    }
                    update_theme_font(&reader, start, font_group, &mut result, part_name)?;
                }
            }
            Event::Empty(ref start) => {
                let namespace = {
                    let (namespace, _) = reader.resolver().resolve_element(start.name());
                    classify_namespace(namespace)
                };
                if namespace == NamespaceKind::Drawing {
                    update_theme_font(&reader, start, font_group, &mut result, part_name)?;
                }
            }
            Event::End(ref end) => {
                if matches!(end.local_name().as_ref(), b"majorFont" | b"minorFont") {
                    font_group = None;
                }
                depth = depth.saturating_sub(1);
            }
            Event::DocType(_) => {
                return Err(DocxError::xml(
                    part_name,
                    event_start,
                    "DOCTYPE declarations are not allowed",
                ));
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    Ok(result)
}

fn update_theme_font(
    reader: &NsReader<&[u8]>,
    start: &quick_xml::events::BytesStart<'_>,
    font_group: Option<bool>,
    result: &mut TemplateFontTheme,
    part_name: &str,
) -> DocxResult<()> {
    let Some(is_major) = font_group else {
        return Ok(());
    };
    let local = start.local_name();
    let typeface = attribute_value(reader, start, b"typeface", None, part_name)?
        .filter(|value| !value.trim().is_empty());
    match local.as_ref() {
        b"latin" => {
            if is_major {
                result.major_latin = typeface;
            } else {
                result.minor_latin = typeface;
            }
        }
        b"ea" => {
            if is_major {
                result.major_east_asia = typeface;
            } else {
                result.minor_east_asia = typeface;
            }
        }
        b"font"
            if attribute_value(reader, start, b"script", None, part_name)?.as_deref()
                == Some("Hans") =>
        {
            if is_major && result.major_east_asia.is_none() {
                result.major_east_asia = typeface;
            } else if !is_major && result.minor_east_asia.is_none() {
                result.minor_east_asia = typeface;
            }
        }
        _ => {}
    }
    Ok(())
}

fn detect_page_number_fields(
    xml: &[u8],
    part_name: &str,
    limits: &DocxLimits,
) -> DocxResult<(bool, bool)> {
    let mut reader = NsReader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut depth = 0usize;
    let mut instruction_depth = 0usize;
    let mut instructions = String::new();

    loop {
        let event_start = reader.buffer_position();
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|error| DocxError::xml(part_name, reader.error_position(), error))?;
        match event {
            Event::Start(ref start) => {
                depth += 1;
                if depth > limits.max_xml_depth {
                    return Err(DocxError::LimitExceeded {
                        resource: format!("XML depth in {part_name}"),
                        limit: limits.max_xml_depth as u64,
                        actual: depth as u64,
                    });
                }
                let (namespace, _) = reader.resolver().resolve_element(start.name());
                if classify_namespace(namespace) == NamespaceKind::Word
                    && start.local_name().as_ref() == b"instrText"
                {
                    instruction_depth += 1;
                }
            }
            Event::End(ref end) => {
                let (namespace, _) = reader.resolver().resolve_element(end.name());
                if classify_namespace(namespace) == NamespaceKind::Word
                    && end.local_name().as_ref() == b"instrText"
                {
                    instruction_depth = instruction_depth.saturating_sub(1);
                }
                depth = depth.saturating_sub(1);
            }
            Event::Text(ref text) if instruction_depth > 0 => {
                instructions.push_str(
                    &text
                        .xml10_content()
                        .map_err(|error| DocxError::xml(part_name, event_start, error))?,
                );
                instructions.push(' ');
            }
            Event::DocType(_) => {
                return Err(DocxError::xml(
                    part_name,
                    event_start,
                    "DOCTYPE declarations are not allowed",
                ));
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }

    let upper = instructions.to_ascii_uppercase();
    Ok((
        upper.split_whitespace().any(|token| token == "PAGE"),
        upper
            .split_whitespace()
            .any(|token| token == "NUMPAGES" || token == "SECTIONPAGES"),
    ))
}

fn read_page_setup(document_xml: &[u8], limits: &DocxLimits) -> DocxResult<TemplatePageSetup> {
    let mut reader = NsReader::from_reader(document_xml);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut depth = 0usize;
    let mut setup = TemplatePageSetup::default();
    let mut column_gap_from_child = false;
    loop {
        let event_start = reader.buffer_position();
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|error| DocxError::xml(DOCUMENT_PART, reader.error_position(), error))?;
        match event {
            Event::Start(ref start) | Event::Empty(ref start) => {
                depth += 1;
                if depth > limits.max_xml_depth {
                    return Err(DocxError::LimitExceeded {
                        resource: format!("XML depth in {DOCUMENT_PART}"),
                        limit: limits.max_xml_depth as u64,
                        actual: depth as u64,
                    });
                }
                let (namespace, _) = reader.resolver().resolve_element(start.name());
                if classify_namespace(namespace) == NamespaceKind::Word {
                    match start.local_name().as_ref() {
                        b"pgSz" => {
                            if let Some(value) = twips_attribute(&reader, start, b"w")? {
                                setup.width_twips = value;
                            }
                            if let Some(value) = twips_attribute(&reader, start, b"h")? {
                                setup.height_twips = value;
                            }
                        }
                        b"pgMar" => {
                            if let Some(value) = twips_attribute(&reader, start, b"top")? {
                                setup.margin_top_twips = value;
                            }
                            if let Some(value) = twips_attribute(&reader, start, b"right")? {
                                setup.margin_right_twips = value;
                            }
                            if let Some(value) = twips_attribute(&reader, start, b"bottom")? {
                                setup.margin_bottom_twips = value;
                            }
                            if let Some(value) = twips_attribute(&reader, start, b"left")? {
                                setup.margin_left_twips = value;
                            }
                            if let Some(value) = twips_attribute(&reader, start, b"header")? {
                                setup.header_twips = value;
                            }
                            if let Some(value) = twips_attribute(&reader, start, b"footer")? {
                                setup.footer_twips = value;
                            }
                        }
                        b"cols" => {
                            if let Some(value) = twips_attribute(&reader, start, b"num")? {
                                setup.column_count = value.clamp(1, 8);
                            }
                            if let Some(value) = twips_attribute(&reader, start, b"space")? {
                                setup.column_gap_twips = value;
                            }
                            if let Some(value) = attribute_value(
                                &reader,
                                start,
                                b"sep",
                                Some(NamespaceKind::Word),
                                DOCUMENT_PART,
                            )? {
                                setup.column_separator =
                                    !matches!(value.as_str(), "0" | "false" | "off" | "none");
                            }
                        }
                        b"col" if !column_gap_from_child => {
                            if let Some(value) = twips_attribute(&reader, start, b"space")? {
                                setup.column_gap_twips = value;
                                column_gap_from_child = true;
                            }
                        }
                        b"docGrid" => {
                            setup.document_grid_type = attribute_value(
                                &reader,
                                start,
                                b"type",
                                Some(NamespaceKind::Word),
                                DOCUMENT_PART,
                            )?;
                            setup.document_grid_line_pitch_twips =
                                twips_attribute(&reader, start, b"linePitch")?;
                        }
                        _ => {}
                    }
                }
                if matches!(event, Event::Empty(_)) {
                    depth = depth.saturating_sub(1);
                }
            }
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::DocType(_) => {
                return Err(DocxError::xml(
                    DOCUMENT_PART,
                    event_start,
                    "DOCTYPE declarations are not allowed",
                ));
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    Ok(setup)
}

fn twips_attribute(
    reader: &NsReader<&[u8]>,
    start: &quick_xml::events::BytesStart<'_>,
    local_name: &[u8],
) -> DocxResult<Option<u32>> {
    let Some(raw) = attribute_value(
        reader,
        start,
        local_name,
        Some(NamespaceKind::Word),
        DOCUMENT_PART,
    )?
    else {
        return Ok(None);
    };
    let value = raw.parse::<u32>().map_err(|_| {
        DocxError::rejected(vec![Diagnostic::error(
            "TEMPLATE_PAGE_SETUP_VALUE_INVALID",
            Some(DOCUMENT_PART),
            format!(
                "page setup attribute '{}' is not a valid non-negative integer",
                raw
            ),
        )])
    })?;
    // Reject pathological page dimensions and margins before they become
    // browser-canvas allocation sizes.
    if value > 100_000 {
        return Err(DocxError::rejected(vec![Diagnostic::error(
            "TEMPLATE_PAGE_SETUP_VALUE_TOO_LARGE",
            Some(DOCUMENT_PART),
            "page setup value exceeds the supported preview range",
        )]));
    }
    Ok(Some(value))
}

fn validate_edits(edits: &mut [XmlEdit], document_len: usize) -> DocxResult<()> {
    for edit in edits.iter() {
        if edit.span.start > edit.span.end || edit.span.end > document_len {
            return Err(DocxError::rejected(vec![Diagnostic::error(
                "TEMPLATE_CONFIGURATION_SPAN_INVALID",
                Some(DOCUMENT_PART),
                format!("{} produced an invalid XML range", edit.label),
            )]));
        }
    }
    edits.sort_by_key(|edit| (edit.span.start, edit.span.end));
    for pair in edits.windows(2) {
        let left = &pair[0];
        let right = &pair[1];
        let same_insertion =
            left.span.is_empty() && right.span.is_empty() && left.span.start == right.span.start;
        if left.span.end > right.span.start || same_insertion {
            return Err(DocxError::rejected(vec![Diagnostic::new(
                DiagnosticSeverity::Error,
                "TEMPLATE_CONFIGURATION_REGIONS_OVERLAP",
                Some(DOCUMENT_PART),
                format!(
                    "configured regions '{}' and '{}' overlap",
                    left.label, right.label
                ),
                Some("请为试卷标题和题目内容选择不同位置。"),
            )]));
        }
    }
    Ok(())
}

fn reject_unsupported_anchors(anchors: &[TemplateAnchor]) -> DocxResult<()> {
    let unknown = anchors
        .iter()
        .map(|anchor| anchor.name.as_str())
        .filter(|name| name.starts_with("ZT_") && !SUPPORTED_ANCHORS.contains(name))
        .collect::<BTreeSet<_>>();
    if unknown.is_empty() {
        return Ok(());
    }
    Err(DocxError::rejected(vec![Diagnostic::new(
        DiagnosticSeverity::Error,
        "TEMPLATE_CONFIGURATION_UNKNOWN_ANCHOR",
        Some(DOCUMENT_PART),
        format!(
            "template contains unsupported anchor(s): {}",
            unknown.into_iter().collect::<Vec<_>>().join(", ")
        ),
        Some("请删除不受支持的 ZT_* 标记后重新导入模板。"),
    )]))
}

fn document_word_namespace(document_xml: &[u8], limits: &DocxLimits) -> DocxResult<&'static str> {
    let mut reader = NsReader::from_reader(document_xml);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut depth = 0usize;
    loop {
        let position = reader.buffer_position();
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|error| DocxError::xml(DOCUMENT_PART, reader.error_position(), error))?;
        match event {
            Event::Start(ref start) | Event::Empty(ref start) => {
                depth += 1;
                if depth > limits.max_xml_depth {
                    return Err(DocxError::LimitExceeded {
                        resource: format!("XML depth in {DOCUMENT_PART}"),
                        limit: limits.max_xml_depth as u64,
                        actual: depth as u64,
                    });
                }
                if start.local_name().as_ref() == b"document" {
                    let (namespace, _) = reader.resolver().resolve_element(start.name());
                    return match namespace {
                        ResolveResult::Bound(Namespace(uri)) if uri == WML_TRANSITIONAL => {
                            Ok("http://schemas.openxmlformats.org/wordprocessingml/2006/main")
                        }
                        ResolveResult::Bound(Namespace(uri)) if uri == WML_STRICT => {
                            Ok("http://purl.oclc.org/ooxml/wordprocessingml/main")
                        }
                        _ => Err(DocxError::rejected(vec![Diagnostic::error(
                            "TEMPLATE_CONFIGURATION_WORD_NAMESPACE_UNSUPPORTED",
                            Some(DOCUMENT_PART),
                            "document root does not use a supported WordprocessingML namespace",
                        )])),
                    };
                }
            }
            Event::DocType(_) => {
                return Err(DocxError::xml(
                    DOCUMENT_PART,
                    position,
                    "DOCTYPE declarations are not allowed",
                ));
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    Err(missing_document_error())
}

fn document_body_insert_offset(document_xml: &[u8], limits: &DocxLimits) -> DocxResult<usize> {
    let mut reader = NsReader::from_reader(document_xml);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut depth = 0usize;
    let mut body_depth = None;
    loop {
        let event_start = reader.buffer_position() as usize;
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|error| DocxError::xml(DOCUMENT_PART, reader.error_position(), error))?;
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
                let (namespace, _) = reader.resolver().resolve_element(start.name());
                if classify_namespace(namespace) == NamespaceKind::Word {
                    if start.local_name().as_ref() == b"body" {
                        body_depth = Some(depth);
                    } else if start.local_name().as_ref() == b"sectPr"
                        && body_depth.is_some_and(|body| depth == body + 1)
                    {
                        return Ok(event_start);
                    }
                }
            }
            Event::Empty(ref start) => {
                let (namespace, _) = reader.resolver().resolve_element(start.name());
                if classify_namespace(namespace) == NamespaceKind::Word
                    && start.local_name().as_ref() == b"sectPr"
                    && body_depth.is_some_and(|body| depth == body)
                {
                    return Ok(event_start);
                }
            }
            Event::End(ref end) => {
                if body_depth == Some(depth) && end.local_name().as_ref() == b"body" {
                    return Ok(event_start);
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
    Err(DocxError::rejected(vec![Diagnostic::error(
        "TEMPLATE_CONFIGURATION_BODY_MISSING",
        Some(DOCUMENT_PART),
        "word/document.xml has no configurable document body",
    )]))
}

fn missing_document_error() -> DocxError {
    DocxError::rejected(vec![Diagnostic::error(
        "TEMPLATE_CONFIGURATION_DOCUMENT_MISSING",
        Some(DOCUMENT_PART),
        "template package has no readable word/document.xml",
    )])
}

mod hex_bytes {
    use serde::{Deserialize, Deserializer, Serializer, de::Error as _};

    pub fn serialize<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut encoded = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            use std::fmt::Write as _;
            write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
        }
        serializer.serialize_str(&encoded)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;
        if encoded.len() % 2 != 0 {
            return Err(D::Error::custom("hex byte string has an odd length"));
        }
        encoded
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let high = hex_digit(pair[0])
                    .ok_or_else(|| D::Error::custom("hex byte string contains invalid data"))?;
                let low = hex_digit(pair[1])
                    .ok_or_else(|| D::Error::custom("hex byte string contains invalid data"))?;
                Ok((high << 4) | low)
            })
            .collect()
    }

    fn hex_digit(value: u8) -> Option<u8> {
        match value {
            b'0'..=b'9' => Some(value - b'0'),
            b'a'..=b'f' => Some(value - b'a' + 10),
            b'A'..=b'F' => Some(value - b'A' + 10),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        io::{Cursor, Read},
    };

    use zip::ZipArchive;

    use super::*;
    use crate::docx::package::test_support::{
        CONTENT_TYPES, ROOT_RELS, minimal_docx, package_with_entries,
    };

    const DOCUMENT: &str = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
<w:p><w:r><w:t>学校期末考试</w:t></w:r></w:p>
<w:p><w:r><w:t>姓名：________</w:t></w:r></w:p>
<w:sectPr/>
</w:body></w:document>"#;

    const SAMPLE_PAPER: &str = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
<w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:rPr><w:b/><w:sz w:val="36"/></w:rPr><w:t>学校期末考试</w:t></w:r></w:p>
<w:p><w:pPr><w:pStyle w:val="SectionHeading"/></w:pPr><w:r><w:rPr><w:rFonts w:eastAsia="黑体"/><w:sz w:val="28"/></w:rPr><w:t>一、选择题</w:t></w:r></w:p>
<w:p><w:pPr><w:ind w:firstLine="420"/></w:pPr><w:r><w:rPr><w:rFonts w:eastAsia="宋体"/><w:sz w:val="28"/></w:rPr><w:t>1. 原来的题目</w:t></w:r></w:p>
<w:p><w:pPr><w:ind w:left="420"/></w:pPr><w:r><w:rPr><w:rFonts w:eastAsia="宋体"/><w:sz w:val="28"/></w:rPr><w:t>A. 原来的选项</w:t></w:r></w:p>
<w:p><w:r><w:t>命题人：张老师</w:t></w:r></w:p>
<w:sectPr/>
</w:body></w:document>"#;

    const LAYOUT_TEMPLATE: &str = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
<w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:rPr><w:rFonts w:eastAsia="宋体"/><w:sz w:val="21"/></w:rPr><w:t>学校：第一中学</w:t></w:r></w:p>
<w:sdt><w:sdtPr><w:tag w:val="ZT_TITLE"/></w:sdtPr><w:sdtContent><w:p/></w:sdtContent></w:sdt>
<w:sdt><w:sdtPr><w:tag w:val="ZT_QUESTIONS"/></w:sdtPr><w:sdtContent><w:p/></w:sdtContent></w:sdt>
<w:sectPr><w:pgSz w:w="16838" w:h="11906"/><w:pgMar w:top="720" w:right="900" w:bottom="1080" w:left="1260" w:header="560" w:footer="680"/><w:cols w:num="2" w:space="425" w:sep="1"/><w:docGrid w:type="lines" w:linePitch="312"/></w:sectPr>
</w:body></w:document>"#;

    const FLOATING_SEAL_TEMPLATE: &str = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"><w:body>
<w:p><w:pPr><w:jc w:val="center"/><w:rPr><w:sz w:val="28"/></w:rPr></w:pPr><w:r><w:rPr><w:sz w:val="24"/></w:rPr><w:drawing><wp:anchor><wps:txbx><w:txbxContent><w:p><w:r><w:t>班级 姓名 考号 密封线内不准答题</w:t></w:r></w:p></w:txbxContent></wps:txbx></wp:anchor></w:drawing></w:r><w:r><w:rPr><w:rFonts w:eastAsia="宋体"/><w:b/><w:sz w:val="40"/></w:rPr><w:t>原试卷标题</w:t></w:r></w:p>
<w:p><w:r><w:t>考试时间：90分钟</w:t></w:r></w:p>
<w:sectPr/>
</w:body></w:document>"#;

    const FLOATING_SEAL_GEOMETRY_PARAGRAPH: &str = r#"<w:p xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:wpg="http://schemas.microsoft.com/office/word/2010/wordprocessingGroup" xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"><w:r><w:drawing><wp:anchor><wp:positionH relativeFrom="column"><wp:posOffset>-907415</wp:posOffset></wp:positionH><wp:positionV relativeFrom="paragraph"><wp:posOffset>-862965</wp:posOffset></wp:positionV><wp:extent cx="961390" cy="11428095"/><a:graphic><a:graphicData><wpg:wgp><wpg:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="961390" cy="11428095"/><a:chOff x="441" y="42"/><a:chExt cx="1680" cy="16692"/></a:xfrm></wpg:grpSpPr><wps:wsp><wps:spPr><a:xfrm><a:off x="441" y="42"/><a:ext cx="1680" cy="16692"/></a:xfrm></wps:spPr><wps:txbx><w:txbxContent><w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:rPr><w:sz w:val="24"/></w:rPr><w:t>班级</w:t></w:r><w:r><w:rPr><w:sz w:val="24"/><w:u w:val="single"/></w:rPr><w:t xml:space="preserve">              </w:t></w:r><w:r><w:rPr><w:sz w:val="24"/></w:rPr><w:t>姓名 考号</w:t></w:r></w:p><w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:t>密              封              线              内              不              准              答              题</w:t></w:r></w:p></w:txbxContent></wps:txbx><wps:bodyPr vert="vert270"/></wps:wsp><wps:wsp><wps:spPr><a:xfrm><a:off x="1510" y="42"/><a:ext cx="0" cy="16692"/></a:xfrm><a:prstGeom prst="line"/><a:ln w="19050"><a:prstDash val="dash"/></a:ln></wps:spPr></wps:wsp></wpg:wgp></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r></w:p>"#;

    const THEME: &str = r#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:themeElements><a:fontScheme name="Office"><a:majorFont><a:latin typeface="Calibri Light"/><a:ea typeface=""/><a:font script="Hans" typeface="宋体"/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/><a:font script="Hans" typeface="仿宋"/></a:minorFont></a:fontScheme></a:themeElements></a:theme>"#;

    const FOOTER: &str = r#"<w:ftr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>第 </w:t></w:r><w:r><w:instrText> PAGE </w:instrText></w:r><w:r><w:t> 页，共 </w:t></w:r><w:r><w:instrText> NUMPAGES </w:instrText></w:r><w:r><w:t> 页</w:t></w:r></w:p></w:ftr>"#;

    fn document_xml(package: &[u8]) -> String {
        let mut archive = ZipArchive::new(Cursor::new(package)).unwrap();
        let mut xml = String::new();
        archive
            .by_name(DOCUMENT_PART)
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        xml
    }

    fn mathtype_limits() -> DocxLimits {
        DocxLimits {
            allow_mathtype_ole: true,
            ..DocxLimits::default()
        }
    }

    #[test]
    fn previews_non_anchor_paragraphs_and_configures_after_a_selected_paragraph() {
        let source = minimal_docx(DOCUMENT);
        let preview = preview_template_configuration(&source, &DocxLimits::default()).unwrap();
        assert_eq!(preview.paragraphs.len(), 2);
        assert!(!preview.has_questions_anchor);

        let configured = configure_template_package(
            &source,
            &TemplateRegionConfiguration {
                questions: TemplateRegionPlacement::AfterParagraph(1),
                styles: TemplateStyleSelection::default(),
                title: Some(TemplateRegionPlacement::ReplaceParagraph(0)),
            },
            &DocxLimits::default(),
        )
        .unwrap();
        assert!(configured.analysis.is_acceptable());
        let xml = document_xml(&configured.bytes);
        assert!(xml.contains("{{ZT_TITLE}}"));
        assert!(xml.contains("姓名：________"));
        assert!(xml.find("姓名：________").unwrap() < xml.find("{{ZT_QUESTIONS}}").unwrap());
    }

    #[test]
    fn document_end_inserts_before_section_properties() {
        let source = minimal_docx(DOCUMENT);
        let configured = configure_template_package(
            &source,
            &TemplateRegionConfiguration {
                questions: TemplateRegionPlacement::DocumentEnd,
                styles: TemplateStyleSelection::default(),
                title: None,
            },
            &DocxLimits::default(),
        )
        .unwrap();
        let xml = document_xml(&configured.bytes);
        assert!(xml.find("{{ZT_QUESTIONS}}").unwrap() < xml.find("<w:sectPr").unwrap());
    }

    #[test]
    fn layout_preview_keeps_static_body_order_styles_and_page_setup() {
        let preview =
            preview_template_layout(&minimal_docx(LAYOUT_TEMPLATE), &DocxLimits::default())
                .unwrap();

        assert_eq!(preview.page.width_twips, 16_838);
        assert_eq!(preview.page.height_twips, 11_906);
        assert_eq!(preview.page.margin_left_twips, 1_260);
        assert_eq!(preview.page.header_twips, 560);
        assert_eq!(preview.page.footer_twips, 680);
        assert_eq!(preview.page.column_count, 2);
        assert_eq!(preview.page.column_gap_twips, 425);
        assert!(preview.page.column_separator);
        assert_eq!(preview.page.document_grid_type.as_deref(), Some("lines"));
        assert_eq!(preview.page.document_grid_line_pitch_twips, Some(312));
        assert_eq!(
            preview
                .blocks
                .iter()
                .map(|block| block.kind.as_str())
                .collect::<Vec<_>>(),
            vec!["static", "title", "questions"]
        );
        let static_block = &preview.blocks[0];
        assert_eq!(static_block.text, "学校：第一中学");
        let style = static_block
            .style
            .as_ref()
            .expect("static style should be kept");
        assert!(
            String::from_utf8_lossy(&style.run_properties)
                .contains(r#"w:rFonts w:eastAsia="宋体""#)
        );
    }

    #[test]
    fn layout_preview_reads_theme_fonts_and_dynamic_page_numbers() {
        let package = package_with_entries(&[
            ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
            ("_rels/.rels", ROOT_RELS.as_bytes()),
            ("word/document.xml", LAYOUT_TEMPLATE.as_bytes()),
            (
                "word/styles.xml",
                b"<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"/>",
            ),
            ("word/theme/theme1.xml", THEME.as_bytes()),
            ("word/footer1.xml", FOOTER.as_bytes()),
        ]);
        let preview = preview_template_layout(&package, &DocxLimits::default()).unwrap();

        assert_eq!(
            preview.font_theme.major_latin.as_deref(),
            Some("Calibri Light")
        );
        assert_eq!(preview.font_theme.major_east_asia.as_deref(), Some("宋体"));
        assert_eq!(preview.font_theme.minor_east_asia.as_deref(), Some("仿宋"));
        assert_eq!(
            preview.page_number_format.as_deref(),
            Some("第 {pageNo} 页，共 {pageCount} 页")
        );
    }

    #[test]
    fn replacing_title_keeps_floating_side_seal_and_uses_visible_title_style() {
        let configured = configure_template_package(
            &minimal_docx(FLOATING_SEAL_TEMPLATE),
            &TemplateRegionConfiguration {
                questions: TemplateRegionPlacement::DocumentEnd,
                styles: TemplateStyleSelection::default(),
                title: Some(TemplateRegionPlacement::ReplaceParagraph(0)),
            },
            &DocxLimits::default(),
        )
        .unwrap();

        let xml = document_xml(&configured.bytes);
        assert!(xml.contains("<wp:anchor>"));
        assert!(xml.contains("班级 姓名 考号 密封线内不准答题"));
        assert!(!xml.contains("原试卷标题"));
        assert!(xml.contains(r#"ztw:line="1" ztw:lineRule="exact""#));
        assert!(xml.find("<wp:anchor>").unwrap() < xml.find("{{ZT_TITLE}}").unwrap());

        let title = configured
            .style_profile
            .as_ref()
            .and_then(|profile| profile.roles.get("title"))
            .expect("title style should be saved");
        let title_run = String::from_utf8_lossy(&title.run_properties);
        assert!(title_run.contains(r#"w:rFonts w:eastAsia="宋体""#));
        assert!(title_run.contains(r#"w:sz w:val="40""#));
        assert!(!title_run.contains(r#"w:sz w:val="24""#));

        let preview = preview_template_layout(&configured.bytes, &DocxLimits::default()).unwrap();
        assert_eq!(
            preview
                .blocks
                .iter()
                .map(|block| block.kind.as_str())
                .collect::<Vec<_>>(),
            vec!["sideSeal", "title", "static", "questions"]
        );
    }

    #[test]
    fn reads_side_seal_anchor_geometry_direction_text_and_line() {
        let page = TemplatePageSetup {
            margin_left_twips: 1_417,
            margin_top_twips: 1_417,
            ..TemplatePageSetup::default()
        };
        // The real template's separate dashed-line shape also contains a
        // bodyPr without a vert attribute. It must not clear the text box's
        // earlier vert270 direction.
        let paragraph = FLOATING_SEAL_GEOMETRY_PARAGRAPH.replace(
            "</wps:spPr></wps:wsp></wpg:wgp>",
            "</wps:spPr><wps:bodyPr upright=\"1\"/></wps:wsp></wpg:wgp>",
        );
        let layout =
            extract_side_seal_layout(paragraph.as_bytes(), 0, &page, &DocxLimits::default())
                .unwrap()
                .expect("side seal geometry should be available");

        assert_eq!(layout.horizontal_relative_from.as_deref(), Some("column"));
        assert_eq!(layout.vertical_relative_from.as_deref(), Some("paragraph"));
        assert_eq!(layout.page_x_emu, Some(-7_620));
        assert_eq!(layout.page_y_emu, Some(36_830));
        assert_eq!(layout.width_emu, 961_390);
        assert_eq!(layout.height_emu, 11_428_095);
        assert_eq!(layout.text_direction.as_deref(), Some("vert270"));
        assert_eq!(layout.line_x_emu, Some(611_741));
        assert_eq!(layout.line_width_emu, Some(19_050));
        assert_eq!(layout.line_dash.as_deref(), Some("dash"));
        assert_eq!(layout.lines.len(), 2);
        assert_eq!(layout.lines[0].alignment.as_deref(), Some("center"));
        assert_eq!(layout.lines[0].runs[0].text, "班级");
        assert!(layout.lines[0].runs[1].underline);
        assert_eq!(layout.lines[0].runs[1].size_half_points, Some(24));
    }

    #[test]
    fn layout_preview_reads_side_seal_namespaces_inherited_from_document_root() {
        let paragraph = FLOATING_SEAL_GEOMETRY_PARAGRAPH
            .replace(
                r#" xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#,
                "",
            )
            .replace(
                r#" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing""#,
                "",
            )
            .replace(
                r#" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main""#,
                "",
            )
            .replace(
                r#" xmlns:wpg="http://schemas.microsoft.com/office/word/2010/wordprocessingGroup""#,
                "",
            )
            .replace(
                r#" xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape""#,
                "",
            );
        let document = format!(
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:wpg="http://schemas.microsoft.com/office/word/2010/wordprocessingGroup" xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"><w:body>{paragraph}<w:sectPr><w:pgSz w:w="23811" w:h="16838"/><w:pgMar w:top="1417" w:right="1417" w:bottom="1417" w:left="1417"/></w:sectPr></w:body></w:document>"#
        );
        let preview =
            preview_template_layout(&minimal_docx(&document), &DocxLimits::default()).unwrap();
        let seal = preview
            .blocks
            .iter()
            .find(|block| block.kind == "sideSeal")
            .and_then(|block| block.side_seal.as_ref())
            .expect("side-seal geometry should survive paragraph slicing");

        assert_eq!(seal.text_direction.as_deref(), Some("vert270"));
        assert_eq!(seal.line_dash.as_deref(), Some("dash"));
        assert_eq!(seal.lines.len(), 2);
    }

    #[test]
    fn rejects_title_and_questions_at_the_same_insertion_point() {
        let source = minimal_docx(DOCUMENT);
        let error = configure_template_package(
            &source,
            &TemplateRegionConfiguration {
                questions: TemplateRegionPlacement::AfterParagraph(0),
                styles: TemplateStyleSelection::default(),
                title: Some(TemplateRegionPlacement::AfterParagraph(0)),
            },
            &DocxLimits::default(),
        )
        .unwrap_err();
        let DocxError::Rejected { diagnostics } = error else {
            panic!("overlapping positions must be rejected");
        };
        assert!(
            diagnostics
                .iter()
                .any(|item| item.code == "TEMPLATE_CONFIGURATION_REGIONS_OVERLAP")
        );
    }

    #[test]
    fn replaces_a_question_range_and_preserves_role_style_prototypes() {
        let source = minimal_docx(SAMPLE_PAPER);
        let configured = configure_template_package(
            &source,
            &TemplateRegionConfiguration {
                questions: TemplateRegionPlacement::ReplaceRange {
                    start_paragraph: 1,
                    end_paragraph: 3,
                },
                styles: TemplateStyleSelection {
                    section_heading: Some(1),
                    question: Some(2),
                    option: Some(3),
                    answer: None,
                    explanation: None,
                },
                title: Some(TemplateRegionPlacement::ReplaceParagraph(0)),
            },
            &DocxLimits::default(),
        )
        .unwrap();

        let xml = document_xml(&configured.bytes);
        assert!(xml.contains(r#"w:tag w:val="ZT_QUESTIONS""#));
        assert!(xml.contains("<w:sdtContent><w:p/></w:sdtContent>"));
        assert!(!xml.contains("原来的题目"));
        assert!(!xml.contains("原来的选项"));
        assert!(xml.contains("命题人：张老师"));

        let profile = configured.style_profile.expect("range must save styles");
        assert_eq!(
            profile.source_range,
            Some(TemplateSourceRange {
                start_paragraph_index: 1,
                end_paragraph_index: 3,
            })
        );
        let heading = profile.roles.get("sectionHeading").unwrap();
        assert!(
            String::from_utf8_lossy(&heading.run_properties)
                .contains(r#"w:rFonts w:eastAsia="黑体""#)
        );
        let question = profile.roles.get("question").unwrap();
        assert!(
            String::from_utf8_lossy(&question.run_properties)
                .contains(r#"w:rFonts w:eastAsia="宋体""#)
        );
        assert!(String::from_utf8_lossy(&question.run_properties).contains(r#"w:sz w:val="28""#));
        let encoded = serde_json::to_value(&profile).unwrap();
        let decoded: TemplateStyleProfile = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded, profile);
    }

    #[test]
    fn supplied_math_exam_becomes_a_strictly_ole_free_template_when_available() {
        let Ok(path) = env::var("ZHITIKU_MATHTYPE_DOCX") else {
            return;
        };
        let source = fs::read(path).expect("supplied MathType fixture should be readable");
        let configured = configure_template_package(
            &source,
            &TemplateRegionConfiguration {
                questions: TemplateRegionPlacement::ReplaceRange {
                    start_paragraph: 6,
                    end_paragraph: 95,
                },
                styles: TemplateStyleSelection::default(),
                title: None,
            },
            &mathtype_limits(),
        )
        .expect("the complete question region should remove every MathType object");

        let strict =
            analyze_template_package(Cursor::new(&configured.bytes), &DocxLimits::default())
                .expect("configured template should pass strict package inspection");
        assert!(
            strict.is_acceptable(),
            "configured template should pass the zero-OLE template policy"
        );

        let archive = ZipArchive::new(Cursor::new(&configured.bytes)).unwrap();
        assert!(
            archive
                .file_names()
                .all(|name| !name.starts_with("word/embeddings/")),
            "configured template must not retain embedded OLE parts"
        );
        let xml = document_xml(&configured.bytes);
        assert!(!xml.contains("OLEObject"));
        assert!(xml.contains(r#"w:tag w:val="ZT_QUESTIONS""#));
    }

    #[test]
    fn supplied_math_exam_rejects_an_incomplete_question_range_when_available() {
        let Ok(path) = env::var("ZHITIKU_MATHTYPE_DOCX") else {
            return;
        };
        let source = fs::read(path).expect("supplied MathType fixture should be readable");
        let error = configure_template_package(
            &source,
            &TemplateRegionConfiguration {
                questions: TemplateRegionPlacement::ReplaceRange {
                    start_paragraph: 6,
                    end_paragraph: 94,
                },
                styles: TemplateStyleSelection::default(),
                title: None,
            },
            &mathtype_limits(),
        )
        .expect_err("an incomplete range must not leave MathType OLE in a template");
        let DocxError::Rejected { diagnostics } = error else {
            panic!("an incomplete range must be rejected with a diagnostic");
        };
        assert!(
            diagnostics
                .iter()
                .any(|item| item.code == "TEMPLATE_CONFIGURATION_MATHTYPE_REMAINS")
        );
    }
}
