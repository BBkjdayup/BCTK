use std::collections::{BTreeMap, BTreeSet};

use quick_xml::{
    events::Event,
    name::{Namespace, ResolveResult},
    reader::NsReader,
    writer::Writer,
};

use super::{
    AnchorKind, Diagnostic, DiagnosticSeverity, DocxError, DocxLimits, DocxResult, TemplateAnchor,
    check_template_anchors,
    rich_content::{PaperBlock, PaperInline, PaperRichContent, PaperTable},
    template_config::{ParagraphStylePrototype, TemplateStyleProfile},
    xml::{NamespaceKind, WML_STRICT, WML_TRANSITIONAL, classify_namespace},
};

const DOCUMENT_PART: &str = "word/document.xml";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaperContentMode {
    PaperOnly,
    AnswersOnly,
    PaperAndAnswers,
    PaperAnswersExplanations,
}

impl PaperContentMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "paper_only" => Some(Self::PaperOnly),
            "answers_only" => Some(Self::AnswersOnly),
            "paper_and_answers" => Some(Self::PaperAndAnswers),
            "paper_answers_explanations" => Some(Self::PaperAnswersExplanations),
            _ => None,
        }
    }

    pub fn required_anchor_names(self) -> &'static [&'static str] {
        &["ZT_QUESTIONS"]
    }

    pub fn includes_questions(self) -> bool {
        !matches!(self, Self::AnswersOnly)
    }

    pub fn includes_answers(self) -> bool {
        !matches!(self, Self::PaperOnly)
    }

    pub fn includes_explanations(self) -> bool {
        matches!(self, Self::PaperAnswersExplanations)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg(test)]
pub struct PaperTextItem {
    pub question_type: String,
    pub question_type_label: String,
    pub stem: String,
    pub options: Vec<String>,
    pub answer: String,
    pub explanation: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg(test)]
pub struct PaperTextDocument {
    pub title: String,
    pub mode: PaperContentMode,
    pub items: Vec<PaperTextItem>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaperRichItem {
    pub question_type: String,
    pub question_type_label: String,
    pub stem: PaperRichContent,
    pub options: Vec<PaperRichContent>,
    pub answer: PaperRichContent,
    pub explanation: PaperRichContent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaperRichDocument {
    pub title: String,
    pub mode: PaperContentMode,
    pub items: Vec<PaperRichItem>,
    /// Question-bank interchange exports keep numbered empty fields blank so
    /// importing the generated DOCX restores an empty value instead of the
    /// display-only "（未填写）" text.
    pub omit_empty_field_placeholders: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaperImageRelationship {
    pub relationship_id: String,
    pub intrinsic_width_px: u32,
    pub intrinsic_height_px: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderedPaperDocument {
    pub document_xml: Vec<u8>,
    pub rewritten_anchors: Vec<String>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug)]
struct SpanReplacement {
    anchor: TemplateAnchor,
    bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaperPageSetupOverride {
    pub width_twips: u32,
    pub height_twips: u32,
}

#[derive(Debug)]
struct XmlTagReplacement {
    start: usize,
    end: usize,
    bytes: Vec<u8>,
}

pub fn override_document_page_setup(
    document_xml: &[u8],
    page: PaperPageSetupOverride,
    limits: &DocxLimits,
) -> DocxResult<Vec<u8>> {
    if page.width_twips < 1_000
        || page.height_twips < 1_000
        || page.width_twips > 100_000
        || page.height_twips > 100_000
    {
        return Err(DocxError::rejected(vec![Diagnostic::error(
            "PAPER_PAGE_SETUP_INVALID",
            Some(DOCUMENT_PART),
            "requested paper dimensions are outside the supported range",
        )]));
    }

    let mut reader = NsReader::from_reader(document_xml);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut depth = 0usize;
    let mut replacements = Vec::new();
    loop {
        let event_start = reader.buffer_position() as usize;
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|error| DocxError::xml(DOCUMENT_PART, reader.error_position(), error))?;
        match event {
            Event::Start(ref start) | Event::Empty(ref start) => {
                let is_empty = matches!(event, Event::Empty(_));
                depth += 1;
                if depth > limits.max_xml_depth {
                    return Err(DocxError::LimitExceeded {
                        resource: format!("XML depth in {DOCUMENT_PART}"),
                        limit: limits.max_xml_depth as u64,
                        actual: depth as u64,
                    });
                }
                let (namespace, _) = reader.resolver().resolve_element(start.name());
                if classify_namespace(namespace) == NamespaceKind::Word
                    && start.local_name().as_ref() == b"pgSz"
                {
                    replacements.push(XmlTagReplacement {
                        start: event_start,
                        end: reader.buffer_position() as usize,
                        bytes: rewritten_page_size_tag(start, is_empty, page, event_start as u64)?,
                    });
                }
                if is_empty {
                    depth = depth.saturating_sub(1);
                }
            }
            Event::End(_) => depth = depth.saturating_sub(1),
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

    if replacements.is_empty() {
        return Err(DocxError::rejected(vec![Diagnostic::error(
            "PAPER_PAGE_SETUP_ELEMENT_MISSING",
            Some(DOCUMENT_PART),
            "template does not contain a Word page-size element",
        )]));
    }
    let mut output = document_xml.to_vec();
    for replacement in replacements.into_iter().rev() {
        output.splice(replacement.start..replacement.end, replacement.bytes);
        if output.len() as u64 > limits.max_xml_part_bytes {
            return Err(DocxError::LimitExceeded {
                resource: "generated word/document.xml".to_owned(),
                limit: limits.max_xml_part_bytes,
                actual: output.len() as u64,
            });
        }
    }
    Ok(output)
}

fn rewritten_page_size_tag(
    start: &quick_xml::events::BytesStart<'_>,
    is_empty: bool,
    page: PaperPageSetupOverride,
    position: u64,
) -> DocxResult<Vec<u8>> {
    let mut retained = Vec::<(Vec<u8>, Vec<u8>)>::new();
    for attribute in start.attributes().with_checks(false) {
        let attribute =
            attribute.map_err(|error| DocxError::xml(DOCUMENT_PART, position, error))?;
        if matches!(attribute.key.local_name().as_ref(), b"w" | b"h" | b"orient") {
            continue;
        }
        retained.push((
            attribute.key.as_ref().to_vec(),
            attribute.value.as_ref().to_vec(),
        ));
    }

    let element_name = start.name();
    let raw_name = element_name.as_ref();
    let prefix = raw_name
        .iter()
        .position(|byte| *byte == b':')
        .map(|index| &raw_name[..index])
        .unwrap_or(b"w");
    let width_name = [prefix, b":w"].concat();
    let height_name = [prefix, b":h"].concat();
    let orientation_name = [prefix, b":orient"].concat();
    let width = page.width_twips.to_string();
    let height = page.height_twips.to_string();
    let mut rewritten = start.to_owned();
    rewritten.clear_attributes();
    for (key, value) in &retained {
        rewritten.push_attribute((key.as_slice(), value.as_slice()));
    }
    rewritten.push_attribute((width_name.as_slice(), width.as_bytes()));
    rewritten.push_attribute((height_name.as_slice(), height.as_bytes()));
    if page.width_twips > page.height_twips {
        rewritten.push_attribute((orientation_name.as_slice(), b"landscape".as_slice()));
    }

    let mut writer = Writer::new(Vec::new());
    writer
        .write_event(if is_empty {
            Event::Empty(rewritten)
        } else {
            Event::Start(rewritten)
        })
        .map_err(|error| DocxError::xml(DOCUMENT_PART, position, error))?;
    Ok(writer.into_inner())
}

/// Replaces only freshly rediscovered, unique template anchors. The inserted
/// WordprocessingML is deliberately plain text and namespace self-contained;
/// callers must reject content that cannot be represented by this first
/// exporter (managed images, tables, or source OOXML) before calling it.
#[cfg(test)]
pub fn render_paper_document_xml(
    template_document_xml: &[u8],
    paper: &PaperTextDocument,
    limits: &DocxLimits,
) -> DocxResult<RenderedPaperDocument> {
    render_paper_document_xml_with_styles(template_document_xml, paper, None, limits)
}

#[cfg(test)]
pub fn render_paper_document_xml_with_styles(
    template_document_xml: &[u8],
    paper: &PaperTextDocument,
    style_profile: Option<&TemplateStyleProfile>,
    limits: &DocxLimits,
) -> DocxResult<RenderedPaperDocument> {
    let namespace = document_word_namespace(template_document_xml, limits)?;
    let mut invalid_character_count = 0usize;
    let title_xml = paragraphs_for_text(
        namespace,
        "",
        &paper.title,
        prototype_for_role(style_profile, StyleRole::Title),
        &mut invalid_character_count,
    );
    let body_xml = render_export_body(
        namespace,
        paper,
        style_profile,
        &mut invalid_character_count,
    );
    replace_paper_anchors(
        template_document_xml,
        paper.mode,
        title_xml,
        body_xml,
        style_profile,
        invalid_character_count,
        true,
        limits,
    )
}

pub fn render_rich_paper_document_xml_with_styles(
    template_document_xml: &[u8],
    paper: &PaperRichDocument,
    image_relationships: &BTreeMap<String, PaperImageRelationship>,
    style_profile: Option<&TemplateStyleProfile>,
    limits: &DocxLimits,
) -> DocxResult<RenderedPaperDocument> {
    let namespace = document_word_namespace(template_document_xml, limits)?;
    let mut invalid_character_count = 0usize;
    let mut drawing_id = 1u32;
    let title_xml = paragraphs_for_text(
        namespace,
        "",
        &paper.title,
        prototype_for_role(style_profile, StyleRole::Title),
        &mut invalid_character_count,
    );
    let body_xml = render_rich_export_body(
        namespace,
        paper,
        image_relationships,
        style_profile,
        &mut invalid_character_count,
        &mut drawing_id,
    )?;
    replace_paper_anchors(
        template_document_xml,
        paper.mode,
        title_xml,
        body_xml,
        style_profile,
        invalid_character_count,
        false,
        limits,
    )
}

#[allow(clippy::too_many_arguments)]
fn replace_paper_anchors(
    template_document_xml: &[u8],
    mode: PaperContentMode,
    title_xml: Vec<u8>,
    body_xml: Vec<u8>,
    style_profile: Option<&TemplateStyleProfile>,
    mut invalid_character_count: usize,
    plain_text_only: bool,
    limits: &DocxLimits,
) -> DocxResult<RenderedPaperDocument> {
    let namespace = document_word_namespace(template_document_xml, limits)?;
    let required_names = mode.required_anchor_names();
    let report = check_template_anchors(template_document_xml, required_names, limits)?;
    if report
        .diagnostics
        .iter()
        .any(|item| item.severity == DiagnosticSeverity::Error)
    {
        return Err(DocxError::rejected(report.diagnostics));
    }

    reject_unregistered_visible_markers(template_document_xml, &report.anchors, limits)?;

    let unknown_anchors = report
        .anchors
        .iter()
        .map(|anchor| anchor.name.as_str())
        .filter(|name| {
            !matches!(
                *name,
                "ZT_TITLE" | "ZT_QUESTIONS" | "ZT_ANSWERS" | "ZT_EXPLANATIONS"
            )
        })
        .collect::<BTreeSet<_>>();
    if !unknown_anchors.is_empty() {
        return Err(DocxError::rejected(vec![Diagnostic::new(
            DiagnosticSeverity::Error,
            "PAPER_EXPORT_UNKNOWN_TEMPLATE_ANCHOR",
            Some(DOCUMENT_PART),
            format!(
                "template contains unsupported anchor(s): {}",
                unknown_anchors.into_iter().collect::<Vec<_>>().join(", ")
            ),
            Some(
                "请删除未受支持的 ZT_* 锚点；当前仅支持 ZT_TITLE、ZT_QUESTIONS、ZT_ANSWERS、ZT_EXPLANATIONS。"
                    .to_owned(),
            ),
        )]));
    }

    let mut diagnostics = report.diagnostics;
    if plain_text_only {
        diagnostics.push(Diagnostic::new(
            DiagnosticSeverity::Warning,
            "PAPER_EXPORT_PLAIN_TEXT_ONLY",
            Some(DOCUMENT_PART),
            "题库富文本会按纯文字写入 Word；模板字体、字号、缩进和段距可以继承，但题目内部单独设置的颜色、加粗等行内格式暂不导出。",
            Some("请在导出后检查含特殊行内格式的题目。".to_owned()),
        ));
    } else {
        diagnostics.push(Diagnostic::new(
            DiagnosticSeverity::Info,
            "PAPER_EXPORT_RICH_CONTENT_WRITTEN",
            Some(DOCUMENT_PART),
            "题目公式已写为 Word 原生可编辑公式，受管图片与表格已作为 DOCX 原生内容写入。",
            None::<String>,
        ));
    }
    if style_profile.is_some_and(|profile| !profile.roles.is_empty()) {
        diagnostics.push(Diagnostic::new(
            DiagnosticSeverity::Info,
            "PAPER_EXPORT_TEMPLATE_STYLES_APPLIED",
            Some(DOCUMENT_PART),
            "已按模板保存的大题标题、题干、选项、答案和解析格式原型生成试卷。",
            None::<String>,
        ));
    } else {
        diagnostics.push(Diagnostic::new(
            DiagnosticSeverity::Warning,
            "PAPER_EXPORT_STYLE_PROFILE_MISSING",
            Some(DOCUMENT_PART),
            "模板没有保存试题格式原型，新题目将使用 Word 文档的默认段落格式。",
            Some("请在模板管理中使用“从样卷制作模板”重新配置试题范围和格式样本。".to_owned()),
        ));
    }

    if invalid_character_count > 0 {
        diagnostics.push(Diagnostic::new(
            DiagnosticSeverity::Warning,
            "PAPER_EXPORT_INVALID_XML_CHAR_REPLACED",
            Some(DOCUMENT_PART),
            format!(
                "题目文字中有 {invalid_character_count} 个 XML 1.0 不允许的控制字符，已替换为可见占位符。"
            ),
            Some("如文字显示异常，请回到题库删除不可见控制字符后重新导出。".to_owned()),
        ));
    }

    let anchors_by_name = report
        .anchors
        .into_iter()
        .map(|anchor| (anchor.name.clone(), anchor))
        .collect::<BTreeMap<_, _>>();
    let mut replacements = Vec::new();
    if let Some(anchor) = anchors_by_name.get("ZT_TITLE") {
        replacements.push(SpanReplacement {
            anchor: anchor.clone(),
            bytes: title_xml,
        });
    } else {
        diagnostics.push(Diagnostic::new(
            DiagnosticSeverity::Warning,
            "PAPER_EXPORT_TITLE_ANCHOR_MISSING",
            Some(DOCUMENT_PART),
            "模板没有 ZT_TITLE 区域，因此试卷名称未写入模板。",
            Some("如需自动写入试卷名称，请在模板中添加 ZT_TITLE 内容控件或整段标记。".to_owned()),
        ));
    }
    let anchor = anchors_by_name.get("ZT_QUESTIONS").ok_or_else(|| {
        DocxError::rejected(vec![Diagnostic::error(
            "DOCX_TEMPLATE_ANCHOR_MISSING",
            Some(DOCUMENT_PART),
            "required template anchor 'ZT_QUESTIONS' is missing",
        )])
    })?;
    replacements.push(SpanReplacement {
        anchor: anchor.clone(),
        bytes: body_xml,
    });
    // The first exporter writes every selected section into ZT_QUESTIONS.
    // Clear legacy optional section markers so a valid template can never
    // expose raw {{ZT_*}} placeholder text in the finished document.
    for name in ["ZT_ANSWERS", "ZT_EXPLANATIONS"] {
        if let Some(anchor) = anchors_by_name.get(name) {
            replacements.push(SpanReplacement {
                anchor: anchor.clone(),
                bytes: paragraphs_for_text(namespace, "", "", None, &mut invalid_character_count),
            });
        }
    }

    validate_block_level_replacements(template_document_xml, &replacements, limits)?;
    validate_non_overlapping_replacements(&replacements)?;
    replacements.sort_by_key(|replacement| replacement.anchor.replacement_span.start);
    let rewritten_anchors = replacements
        .iter()
        .map(|replacement| replacement.anchor.name.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let mut output = template_document_xml.to_vec();
    for replacement in replacements.into_iter().rev() {
        output.splice(
            replacement.anchor.replacement_span.as_range(),
            replacement.bytes,
        );
        if output.len() as u64 > limits.max_xml_part_bytes {
            return Err(DocxError::LimitExceeded {
                resource: "generated word/document.xml".to_owned(),
                limit: limits.max_xml_part_bytes,
                actual: output.len() as u64,
            });
        }
    }

    // Parse the final bytes again before the package writer sees them. This
    // catches malformed span handling and keeps output validation local.
    super::parse_document_xml(&output, limits)?;
    Ok(RenderedPaperDocument {
        document_xml: output,
        rewritten_anchors,
        diagnostics,
    })
}

fn reject_unregistered_visible_markers(
    document_xml: &[u8],
    anchors: &[TemplateAnchor],
    limits: &DocxLimits,
) -> DocxResult<()> {
    let parsed = super::parse_document_xml(document_xml, limits)?;
    let diagnostics = parsed
        .paragraphs
        .iter()
        .filter(|paragraph| visible_zt_marker_start(&paragraph.logical_text).is_some())
        .filter(|paragraph| {
            !anchors.iter().any(|anchor| match anchor.kind {
                AnchorKind::ParagraphMarker => anchor.replacement_span == paragraph.source_span,
                AnchorKind::ContentControl => {
                    anchor.replacement_span.contains(paragraph.source_span)
                }
            })
        })
        .map(|paragraph| {
            Diagnostic::new(
                DiagnosticSeverity::Error,
                "PAPER_EXPORT_UNREGISTERED_VISIBLE_MARKER",
                Some(DOCUMENT_PART),
                format!(
                    "template paragraph {} contains a visible ZT placeholder that is not a registered replaceable anchor",
                    paragraph.index + 1
                ),
                Some(
                    "请把占位符改成独占整段的 {{ZT_NAME}}，或改用包住完整段落的 Word 内容控件；不要把占位符和其他可见文字写在同一段。"
                        .to_owned(),
                ),
            )
        })
        .collect::<Vec<_>>();
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(DocxError::rejected(diagnostics))
    }
}

fn visible_zt_marker_start(text: &str) -> Option<usize> {
    text.match_indices("{{").find_map(|(offset, _)| {
        let remainder =
            text[offset + 2..].trim_start_matches(|character: char| character.is_whitespace());
        remainder.starts_with("ZT_").then_some(offset)
    })
}

fn validate_non_overlapping_replacements(replacements: &[SpanReplacement]) -> DocxResult<()> {
    let mut spans = replacements
        .iter()
        .map(|replacement| {
            (
                replacement.anchor.replacement_span,
                replacement.anchor.name.as_str(),
            )
        })
        .collect::<Vec<_>>();
    spans.sort_by_key(|(span, _)| (span.start, span.end));
    for pair in spans.windows(2) {
        if pair[0].0.end > pair[1].0.start {
            return Err(DocxError::rejected(vec![Diagnostic::error(
                "DOCX_TEMPLATE_ANCHOR_OVERLAP",
                Some(DOCUMENT_PART),
                format!(
                    "template anchors '{}' and '{}' overlap and cannot be replaced safely",
                    pair[0].1, pair[1].1
                ),
            )]));
        }
    }
    Ok(())
}

fn validate_block_level_replacements(
    document_xml: &[u8],
    replacements: &[SpanReplacement],
    limits: &DocxLimits,
) -> DocxResult<()> {
    let parsed = super::parse_document_xml(document_xml, limits)?;
    let inline = replacements.iter().find(|replacement| {
        replacement.anchor.kind == AnchorKind::ContentControl
            && parsed.paragraphs.iter().any(|paragraph| {
                paragraph
                    .source_span
                    .contains(replacement.anchor.container_span)
            })
    });
    if let Some(replacement) = inline {
        return Err(DocxError::rejected(vec![Diagnostic::new(
            DiagnosticSeverity::Error,
            "PAPER_EXPORT_INLINE_ANCHOR_UNSUPPORTED",
            Some(DOCUMENT_PART),
            format!(
                "template anchor '{}' is an inline content control and cannot safely contain generated paragraphs",
                replacement.anchor.name
            ),
            Some(
                "请把该锚点改成独占整段的 {{ZT_NAME}} 标记，或使用包住完整段落的块级内容控件。"
                    .to_owned(),
            ),
        )]));
    }
    Ok(())
}

fn document_word_namespace(document_xml: &[u8], limits: &DocxLimits) -> DocxResult<&'static str> {
    let mut reader = NsReader::from_reader(document_xml);
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut depth = 0usize;
    loop {
        let start_position = reader.buffer_position();
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
                            "PAPER_EXPORT_WORD_NAMESPACE_UNSUPPORTED",
                            Some(DOCUMENT_PART),
                            "document root does not use a supported Transitional or Strict WordprocessingML namespace",
                        )])),
                    };
                }
            }
            Event::DocType(_) => {
                return Err(DocxError::xml(
                    DOCUMENT_PART,
                    start_position,
                    "DOCTYPE declarations are not allowed",
                ));
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    Err(DocxError::rejected(vec![Diagnostic::error(
        "PAPER_EXPORT_DOCUMENT_ROOT_MISSING",
        Some(DOCUMENT_PART),
        "word/document.xml has no WordprocessingML document root",
    )]))
}

#[derive(Clone, Copy)]
enum StyleRole {
    Title,
    SectionHeading,
    Question,
    Option,
    Answer,
    Explanation,
}

impl StyleRole {
    fn fallback_keys(self) -> &'static [&'static str] {
        match self {
            Self::Title => &["title", "sectionHeading", "question"],
            Self::SectionHeading => &["sectionHeading", "question"],
            Self::Question => &["question", "option", "sectionHeading"],
            Self::Option => &["option", "question"],
            Self::Answer => &["answer", "question"],
            Self::Explanation => &["explanation", "answer", "question"],
        }
    }
}

fn prototype_for_role(
    profile: Option<&TemplateStyleProfile>,
    role: StyleRole,
) -> Option<&ParagraphStylePrototype> {
    let profile = profile?;
    role.fallback_keys()
        .iter()
        .find_map(|key| profile.roles.get(*key))
}

#[cfg(test)]
fn render_questions(
    namespace: &str,
    items: &[PaperTextItem],
    style_profile: Option<&TemplateStyleProfile>,
    invalid: &mut usize,
) -> Vec<u8> {
    let mut xml = String::new();
    let mut previous_type: Option<&str> = None;
    let mut section_index = 0usize;
    for (index, item) in items.iter().enumerate() {
        if previous_type != Some(item.question_type.as_str()) {
            section_index += 1;
            let question_count = items[index..]
                .iter()
                .take_while(|candidate| candidate.question_type == item.question_type)
                .count();
            xml.push_str(&paragraphs_for_text_string(
                namespace,
                "",
                &question_section_heading(section_index, &item.question_type_label, question_count),
                prototype_for_role(style_profile, StyleRole::SectionHeading),
                invalid,
            ));
            previous_type = Some(&item.question_type);
        }
        xml.push_str(&paragraphs_for_text_string(
            namespace,
            &format!("{}. ", index + 1),
            &item.stem,
            prototype_for_role(style_profile, StyleRole::Question),
            invalid,
        ));
        for (option_index, option) in item.options.iter().enumerate() {
            xml.push_str(&paragraphs_for_text_string(
                namespace,
                &format!("{}. ", option_label(option_index)),
                option,
                prototype_for_role(style_profile, StyleRole::Option),
                invalid,
            ));
        }
    }
    xml.into_bytes()
}

fn question_section_heading(
    section_index: usize,
    question_type_label: &str,
    question_count: usize,
) -> String {
    format!(
        "{}、{}：本题共{}个小题，每小题      分。共      分。",
        chinese_section_number(section_index),
        question_type_label,
        question_count,
    )
}

fn chinese_section_number(index: usize) -> String {
    const DIGITS: [&str; 10] = ["零", "一", "二", "三", "四", "五", "六", "七", "八", "九"];
    match index {
        0..=9 => DIGITS[index].to_owned(),
        10 => "十".to_owned(),
        11..=19 => format!("十{}", DIGITS[index % 10]),
        20..=99 if index.is_multiple_of(10) => format!("{}十", DIGITS[index / 10]),
        20..=99 => format!("{}十{}", DIGITS[index / 10], DIGITS[index % 10]),
        _ => index.to_string(),
    }
}

#[cfg(test)]
fn render_export_body(
    namespace: &str,
    paper: &PaperTextDocument,
    style_profile: Option<&TemplateStyleProfile>,
    invalid: &mut usize,
) -> Vec<u8> {
    let mut body = Vec::new();
    if paper.mode.includes_questions() {
        body.extend(render_questions(
            namespace,
            &paper.items,
            style_profile,
            invalid,
        ));
    }
    if paper.mode.includes_answers() {
        if paper.mode.includes_questions() {
            body.extend(paragraphs_for_text(
                namespace,
                "",
                "参考答案",
                prototype_for_role(style_profile, StyleRole::SectionHeading),
                invalid,
            ));
        }
        body.extend(render_answers(
            namespace,
            &paper.items,
            style_profile,
            invalid,
        ));
    }
    if paper.mode.includes_explanations() {
        body.extend(paragraphs_for_text(
            namespace,
            "",
            "题目解析",
            prototype_for_role(style_profile, StyleRole::SectionHeading),
            invalid,
        ));
        body.extend(render_explanations(
            namespace,
            &paper.items,
            style_profile,
            invalid,
        ));
    }
    body
}

#[cfg(test)]
fn render_answers(
    namespace: &str,
    items: &[PaperTextItem],
    style_profile: Option<&TemplateStyleProfile>,
    invalid: &mut usize,
) -> Vec<u8> {
    let mut xml = String::new();
    for (index, item) in items.iter().enumerate() {
        let answer = if item.answer.trim().is_empty() {
            "（未填写）"
        } else {
            &item.answer
        };
        xml.push_str(&paragraphs_for_text_string(
            namespace,
            &format!("{}. ", index + 1),
            answer,
            prototype_for_role(style_profile, StyleRole::Answer),
            invalid,
        ));
    }
    xml.into_bytes()
}

#[cfg(test)]
fn render_explanations(
    namespace: &str,
    items: &[PaperTextItem],
    style_profile: Option<&TemplateStyleProfile>,
    invalid: &mut usize,
) -> Vec<u8> {
    let mut xml = String::new();
    for (index, item) in items.iter().enumerate() {
        let explanation = if item.explanation.trim().is_empty() {
            "（未填写）"
        } else {
            &item.explanation
        };
        xml.push_str(&paragraphs_for_text_string(
            namespace,
            &format!("{}. ", index + 1),
            explanation,
            prototype_for_role(style_profile, StyleRole::Explanation),
            invalid,
        ));
    }
    xml.into_bytes()
}

fn render_rich_export_body(
    namespace: &str,
    paper: &PaperRichDocument,
    image_relationships: &BTreeMap<String, PaperImageRelationship>,
    style_profile: Option<&TemplateStyleProfile>,
    invalid: &mut usize,
    drawing_id: &mut u32,
) -> DocxResult<Vec<u8>> {
    let mut body = Vec::new();
    if paper.mode.includes_questions() {
        let mut previous_type: Option<&str> = None;
        let mut section_index = 0usize;
        for (index, item) in paper.items.iter().enumerate() {
            if previous_type != Some(item.question_type.as_str()) {
                section_index += 1;
                let question_count = paper.items[index..]
                    .iter()
                    .take_while(|candidate| candidate.question_type == item.question_type)
                    .count();
                body.extend(paragraphs_for_text(
                    namespace,
                    "",
                    &question_section_heading(
                        section_index,
                        &item.question_type_label,
                        question_count,
                    ),
                    prototype_for_role(style_profile, StyleRole::SectionHeading),
                    invalid,
                ));
                previous_type = Some(&item.question_type);
            }
            body.extend(render_rich_content(
                namespace,
                &format!("{}. ", index + 1),
                &item.stem,
                prototype_for_role(style_profile, StyleRole::Question),
                image_relationships,
                invalid,
                drawing_id,
            )?);
            for (option_index, option) in item.options.iter().enumerate() {
                body.extend(render_rich_content(
                    namespace,
                    &format!("{}. ", option_label(option_index)),
                    option,
                    prototype_for_role(style_profile, StyleRole::Option),
                    image_relationships,
                    invalid,
                    drawing_id,
                )?);
            }
        }
    }
    if paper.mode.includes_answers() {
        if paper.mode.includes_questions() {
            body.extend(paragraphs_for_text(
                namespace,
                "",
                "参考答案",
                prototype_for_role(style_profile, StyleRole::SectionHeading),
                invalid,
            ));
        }
        for (index, item) in paper.items.iter().enumerate() {
            if rich_content_is_empty(&item.answer) {
                body.extend(paragraphs_for_text(
                    namespace,
                    &format!("{}. ", index + 1),
                    if paper.omit_empty_field_placeholders {
                        ""
                    } else {
                        "（未填写）"
                    },
                    prototype_for_role(style_profile, StyleRole::Answer),
                    invalid,
                ));
            } else {
                body.extend(render_rich_content(
                    namespace,
                    &format!("{}. ", index + 1),
                    &item.answer,
                    prototype_for_role(style_profile, StyleRole::Answer),
                    image_relationships,
                    invalid,
                    drawing_id,
                )?);
            }
        }
    }
    if paper.mode.includes_explanations() {
        body.extend(paragraphs_for_text(
            namespace,
            "",
            "题目解析",
            prototype_for_role(style_profile, StyleRole::SectionHeading),
            invalid,
        ));
        for (index, item) in paper.items.iter().enumerate() {
            if rich_content_is_empty(&item.explanation) {
                body.extend(paragraphs_for_text(
                    namespace,
                    &format!("{}. ", index + 1),
                    if paper.omit_empty_field_placeholders {
                        ""
                    } else {
                        "（未填写）"
                    },
                    prototype_for_role(style_profile, StyleRole::Explanation),
                    invalid,
                ));
            } else {
                body.extend(render_rich_content(
                    namespace,
                    &format!("{}. ", index + 1),
                    &item.explanation,
                    prototype_for_role(style_profile, StyleRole::Explanation),
                    image_relationships,
                    invalid,
                    drawing_id,
                )?);
            }
        }
    }
    Ok(body)
}

fn rich_content_is_empty(content: &PaperRichContent) -> bool {
    content.blocks.iter().all(|block| match block {
        PaperBlock::Paragraph(paragraph) => paragraph.inlines.iter().all(|inline| match inline {
            PaperInline::Text(run) => run.text.trim().is_empty(),
            PaperInline::LineBreak => true,
            PaperInline::Formula { .. } | PaperInline::Image(_) => false,
        }),
        PaperBlock::Table(table) => table.rows.is_empty(),
    })
}

#[allow(clippy::too_many_arguments)]
fn render_rich_content(
    namespace: &str,
    prefix: &str,
    content: &PaperRichContent,
    prototype: Option<&ParagraphStylePrototype>,
    image_relationships: &BTreeMap<String, PaperImageRelationship>,
    invalid: &mut usize,
    drawing_id: &mut u32,
) -> DocxResult<Vec<u8>> {
    let mut output = String::new();
    let mut prefix_pending = true;
    if content.blocks.is_empty() {
        output.push_str(&rich_paragraph_xml(
            namespace,
            prefix,
            &[],
            prototype,
            image_relationships,
            invalid,
            drawing_id,
        )?);
    }
    for block in &content.blocks {
        match block {
            PaperBlock::Paragraph(paragraph) => {
                output.push_str(&rich_paragraph_xml(
                    namespace,
                    if prefix_pending { prefix } else { "" },
                    &paragraph.inlines,
                    prototype,
                    image_relationships,
                    invalid,
                    drawing_id,
                )?);
                prefix_pending = false;
            }
            PaperBlock::Table(table) => {
                if prefix_pending && !prefix.is_empty() {
                    output.push_str(&rich_paragraph_xml(
                        namespace,
                        prefix,
                        &[],
                        prototype,
                        image_relationships,
                        invalid,
                        drawing_id,
                    )?);
                }
                prefix_pending = false;
                output.push_str(&rich_table_xml(
                    namespace,
                    table,
                    prototype,
                    image_relationships,
                    invalid,
                    drawing_id,
                )?);
            }
        }
    }
    Ok(output.into_bytes())
}

#[allow(clippy::too_many_arguments)]
fn rich_paragraph_xml(
    namespace: &str,
    prefix: &str,
    inlines: &[PaperInline],
    prototype: Option<&ParagraphStylePrototype>,
    image_relationships: &BTreeMap<String, PaperImageRelationship>,
    invalid: &mut usize,
    drawing_id: &mut u32,
) -> DocxResult<String> {
    let mut xml = String::new();
    xml.push_str("<ztw:p xmlns:ztw=\"");
    xml.push_str(namespace);
    xml.push_str("\">");
    if let Some(prototype) = prototype {
        xml.push_str(&String::from_utf8_lossy(&prototype.paragraph_properties));
    }
    if !prefix.is_empty() {
        xml.push_str(&text_run_xml(prefix, None, prototype, invalid));
    }
    for inline in inlines {
        match inline {
            PaperInline::Text(run) => {
                xml.push_str(&text_run_xml(&run.text, Some(run), prototype, invalid));
            }
            PaperInline::Formula { latex } => {
                let formula = tex2word_math::to_omath(latex);
                xml.push_str(&formula.replacen(
                    "<m:oMath>",
                    "<m:oMath xmlns:m=\"http://schemas.openxmlformats.org/officeDocument/2006/math\">",
                    1,
                ));
            }
            PaperInline::Image(image) => {
                let relationship =
                    image_relationships.get(&image.resource_id).ok_or_else(|| {
                        DocxError::rejected(vec![Diagnostic::error(
                            "PAPER_EXPORT_IMAGE_RELATIONSHIP_MISSING",
                            Some(DOCUMENT_PART),
                            format!(
                                "managed image {} has no generated DOCX relationship",
                                image.resource_id
                            ),
                        )])
                    })?;
                xml.push_str(&image_run_xml(image, relationship, *drawing_id, invalid));
                *drawing_id = drawing_id.saturating_add(1);
            }
            PaperInline::LineBreak => xml.push_str("<ztw:r><ztw:br/></ztw:r>"),
        }
    }
    xml.push_str("</ztw:p>");
    Ok(xml)
}

fn text_run_xml(
    text: &str,
    run: Option<&super::rich_content::PaperTextRun>,
    prototype: Option<&ParagraphStylePrototype>,
    invalid: &mut usize,
) -> String {
    let mut xml = String::from("<ztw:r>");
    let marks = run.map(run_mark_properties).unwrap_or_default();
    xml.push_str(&merged_run_properties(prototype, &marks));
    xml.push_str("<ztw:t xml:space=\"preserve\">");
    xml.push_str(&escape_xml_text(text, invalid));
    xml.push_str("</ztw:t></ztw:r>");
    xml
}

fn run_mark_properties(run: &super::rich_content::PaperTextRun) -> String {
    let mut properties = String::new();
    if run.bold {
        properties.push_str("<ztw:b/>");
    }
    if run.italic {
        properties.push_str("<ztw:i/>");
    }
    if run.underline {
        properties.push_str("<ztw:u ztw:val=\"single\"/>");
    }
    if run.strike {
        properties.push_str("<ztw:strike/>");
    }
    if run.superscript {
        properties.push_str("<ztw:vertAlign ztw:val=\"superscript\"/>");
    }
    if run.subscript {
        properties.push_str("<ztw:vertAlign ztw:val=\"subscript\"/>");
    }
    properties
}

fn merged_run_properties(prototype: Option<&ParagraphStylePrototype>, marks: &str) -> String {
    let source = prototype
        .map(|prototype| String::from_utf8_lossy(&prototype.run_properties).into_owned())
        .unwrap_or_default();
    if source.is_empty() {
        return if marks.is_empty() {
            String::new()
        } else {
            format!("<ztw:rPr>{marks}</ztw:rPr>")
        };
    }
    if marks.is_empty() {
        return source;
    }
    if let Some(position) = source.rfind("</") {
        let mut merged = source;
        merged.insert_str(position, marks);
        merged
    } else {
        format!("{source}<ztw:rPr>{marks}</ztw:rPr>")
    }
}

fn image_run_xml(
    image: &super::rich_content::PaperImage,
    relationship: &PaperImageRelationship,
    drawing_id: u32,
    invalid: &mut usize,
) -> String {
    let intrinsic_width = relationship.intrinsic_width_px.max(1);
    let intrinsic_height = relationship.intrinsic_height_px.max(1);
    let requested_width = image.width_px.unwrap_or(intrinsic_width).clamp(1, 2_000);
    let requested_height = image.height_px.unwrap_or_else(|| {
        ((u64::from(intrinsic_height) * u64::from(requested_width)) / u64::from(intrinsic_width))
            .clamp(1, 2_000) as u32
    });
    let width_emu = u64::from(requested_width) * 9_525;
    let height_emu = u64::from(requested_height.clamp(1, 2_000)) * 9_525;
    let name = escape_xml_attribute(&image.alt, invalid);
    let rel = escape_xml_attribute(&relationship.relationship_id, invalid);
    format!(
        "<ztw:r><ztw:drawing><wp:inline xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"{width_emu}\" cy=\"{height_emu}\"/><wp:docPr id=\"{drawing_id}\" name=\"{name}\"/><a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:pic xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:nvPicPr><pic:cNvPr id=\"{drawing_id}\" name=\"{name}\"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" r:embed=\"{rel}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{width_emu}\" cy=\"{height_emu}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></ztw:drawing></ztw:r>"
    )
}

#[allow(clippy::too_many_arguments)]
fn rich_table_xml(
    namespace: &str,
    table: &PaperTable,
    prototype: Option<&ParagraphStylePrototype>,
    image_relationships: &BTreeMap<String, PaperImageRelationship>,
    invalid: &mut usize,
    drawing_id: &mut u32,
) -> DocxResult<String> {
    let column_count = table
        .rows
        .iter()
        .map(|row| {
            row.cells
                .iter()
                .map(|cell| cell.column_span as usize)
                .sum::<usize>()
        })
        .max()
        .unwrap_or(1)
        .max(1);
    let mut xml = String::new();
    xml.push_str("<ztw:tbl xmlns:ztw=\"");
    xml.push_str(namespace);
    xml.push_str("\"><ztw:tblPr><ztw:tblW ztw:w=\"0\" ztw:type=\"auto\"/><ztw:tblBorders><ztw:top ztw:val=\"single\" ztw:sz=\"4\" ztw:color=\"000000\"/><ztw:left ztw:val=\"single\" ztw:sz=\"4\" ztw:color=\"000000\"/><ztw:bottom ztw:val=\"single\" ztw:sz=\"4\" ztw:color=\"000000\"/><ztw:right ztw:val=\"single\" ztw:sz=\"4\" ztw:color=\"000000\"/><ztw:insideH ztw:val=\"single\" ztw:sz=\"4\" ztw:color=\"000000\"/><ztw:insideV ztw:val=\"single\" ztw:sz=\"4\" ztw:color=\"000000\"/></ztw:tblBorders></ztw:tblPr><ztw:tblGrid>");
    for _ in 0..column_count {
        xml.push_str("<ztw:gridCol ztw:w=\"1440\"/>");
    }
    xml.push_str("</ztw:tblGrid>");
    for row in &table.rows {
        xml.push_str("<ztw:tr>");
        for cell in &row.cells {
            xml.push_str("<ztw:tc><ztw:tcPr><ztw:tcW ztw:w=\"0\" ztw:type=\"auto\"/>");
            if cell.column_span > 1 {
                xml.push_str(&format!("<ztw:gridSpan ztw:val=\"{}\"/>", cell.column_span));
            }
            xml.push_str("</ztw:tcPr>");
            let cell_content = PaperRichContent {
                blocks: cell.blocks.clone(),
            };
            xml.push_str(&String::from_utf8_lossy(&render_rich_content(
                namespace,
                "",
                &cell_content,
                prototype,
                image_relationships,
                invalid,
                drawing_id,
            )?));
            xml.push_str("</ztw:tc>");
        }
        xml.push_str("</ztw:tr>");
    }
    xml.push_str("</ztw:tbl>");
    Ok(xml)
}

fn escape_xml_attribute(value: &str, invalid: &mut usize) -> String {
    escape_xml_text(value, invalid)
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn paragraphs_for_text(
    namespace: &str,
    prefix: &str,
    text: &str,
    prototype: Option<&ParagraphStylePrototype>,
    invalid: &mut usize,
) -> Vec<u8> {
    paragraphs_for_text_string(namespace, prefix, text, prototype, invalid).into_bytes()
}

fn paragraphs_for_text_string(
    namespace: &str,
    prefix: &str,
    text: &str,
    prototype: Option<&ParagraphStylePrototype>,
    invalid: &mut usize,
) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut result = String::new();
    let mut lines = normalized.split('\n').peekable();
    if lines.peek().is_none() {
        lines = "".split('\n').peekable();
    }
    for (line_index, line) in lines.enumerate() {
        let line_prefix = if line_index == 0 { prefix } else { "" };
        let escaped = escape_xml_text(&format!("{line_prefix}{line}"), invalid);
        result.push_str("<ztw:p xmlns:ztw=\"");
        result.push_str(namespace);
        result.push_str("\">");
        if let Some(prototype) = prototype {
            result.push_str(&String::from_utf8_lossy(&prototype.paragraph_properties));
        }
        result.push_str("<ztw:r>");
        if let Some(prototype) = prototype {
            result.push_str(&String::from_utf8_lossy(&prototype.run_properties));
        }
        result.push_str("<ztw:t xml:space=\"preserve\">");
        result.push_str(&escaped);
        result.push_str("</ztw:t></ztw:r></ztw:p>");
    }
    result
}

fn escape_xml_text(value: &str, invalid: &mut usize) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        if !is_xml_1_0_character(character) {
            *invalid += 1;
            output.push('\u{fffd}');
            continue;
        }
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            _ => output.push(character),
        }
    }
    output
}

fn is_xml_1_0_character(character: char) -> bool {
    matches!(character, '\u{9}' | '\u{a}' | '\u{d}')
        || ('\u{20}'..='\u{d7ff}').contains(&character)
        || ('\u{e000}'..='\u{fffd}').contains(&character)
        || ('\u{10000}'..='\u{10ffff}').contains(&character)
}

fn option_label(mut index: usize) -> String {
    let mut label = String::new();
    loop {
        let remainder = index % 26;
        label.insert(0, (b'A' + remainder as u8) as char);
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    label
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paper(mode: PaperContentMode) -> PaperTextDocument {
        PaperTextDocument {
            title: "数学 & <期中> 😀\u{1}".to_owned(),
            mode,
            items: vec![PaperTextItem {
                question_type: "single_choice".to_owned(),
                question_type_label: "选择题".to_owned(),
                stem: "第一行\n第二行 < 3 & 4".to_owned(),
                options: vec!["选项 A".to_owned(), "选项 B".to_owned()],
                answer: "A".to_owned(),
                explanation: "因为……".to_owned(),
            }],
        }
    }

    fn template(namespace: &str) -> String {
        format!(
            r#"<x:document xmlns:x="{namespace}"><x:body>
<x:p><x:r><x:t>{{{{ZT_TITLE}}}}</x:t></x:r></x:p>
<x:p><x:r><x:t>{{{{ZT_QUESTIONS}}}}</x:t></x:r></x:p>
<x:p><x:r><x:t>{{{{ZT_ANSWERS}}}}</x:t></x:r></x:p>
<x:p><x:r><x:t>{{{{ZT_EXPLANATIONS}}}}</x:t></x:r></x:p>
<x:sectPr/></x:body></x:document>"#
        )
    }

    #[test]
    fn renders_all_sections_escapes_text_and_replaces_invalid_controls() {
        let rendered = render_paper_document_xml(
            template("http://schemas.openxmlformats.org/wordprocessingml/2006/main").as_bytes(),
            &paper(PaperContentMode::PaperAnswersExplanations),
            &DocxLimits::default(),
        )
        .unwrap();
        let xml = String::from_utf8(rendered.document_xml).unwrap();

        assert!(xml.contains("数学 &amp; &lt;期中&gt; 😀�"));
        assert!(xml.contains("1. 第一行"));
        assert!(xml.contains("一、选择题：本题共1个小题，每小题      分。共      分。"));
        assert!(xml.contains("第二行 &lt; 3 &amp; 4"));
        assert!(xml.contains("A. 选项 A"));
        assert!(xml.contains("因为……"));
        assert!(xml.contains("<x:sectPr/>"));
        assert!(!xml.contains("{{ZT_"));
        assert!(xml.contains("参考答案"));
        assert!(xml.contains("题目解析"));
        assert!(
            rendered
                .diagnostics
                .iter()
                .any(|item| item.code == "PAPER_EXPORT_INVALID_XML_CHAR_REPLACED")
        );
        assert!(
            rendered
                .diagnostics
                .iter()
                .any(|item| item.code == "PAPER_EXPORT_PLAIN_TEXT_ONLY")
        );
    }

    #[test]
    fn applies_separate_template_styles_to_headings_questions_and_options() {
        let profile = TemplateStyleProfile {
            schema_version: 1,
            source_range: None,
            roles: BTreeMap::from([
                (
                    "sectionHeading".to_owned(),
                    ParagraphStylePrototype {
                        source_paragraph_index: 1,
                        paragraph_properties:
                            br#"<x:pPr><x:pStyle x:val="SectionHeading"/></x:pPr>"#.to_vec(),
                        run_properties:
                            r#"<x:rPr><x:rFonts x:eastAsia="黑体"/><x:sz x:val="28"/></x:rPr>"#
                                .as_bytes()
                                .to_vec(),
                    },
                ),
                (
                    "question".to_owned(),
                    ParagraphStylePrototype {
                        source_paragraph_index: 2,
                        paragraph_properties: br#"<x:pPr><x:ind x:firstLine="420"/></x:pPr>"#
                            .to_vec(),
                        run_properties:
                            r#"<x:rPr><x:rFonts x:eastAsia="宋体"/><x:sz x:val="28"/></x:rPr>"#
                                .as_bytes()
                                .to_vec(),
                    },
                ),
                (
                    "option".to_owned(),
                    ParagraphStylePrototype {
                        source_paragraph_index: 3,
                        paragraph_properties: br#"<x:pPr><x:ind x:left="420"/></x:pPr>"#.to_vec(),
                        run_properties:
                            r#"<x:rPr><x:rFonts x:eastAsia="楷体"/><x:sz x:val="24"/></x:rPr>"#
                                .as_bytes()
                                .to_vec(),
                    },
                ),
            ]),
        };
        let rendered = render_paper_document_xml_with_styles(
            template("http://schemas.openxmlformats.org/wordprocessingml/2006/main").as_bytes(),
            &paper(PaperContentMode::PaperOnly),
            Some(&profile),
            &DocxLimits::default(),
        )
        .unwrap();
        let xml = String::from_utf8(rendered.document_xml).unwrap();

        assert!(xml.contains(r#"<x:pStyle x:val="SectionHeading"/>"#));
        assert!(xml.contains(r#"<x:rFonts x:eastAsia="宋体"/><x:sz x:val="28"/>"#));
        assert!(xml.contains(r#"<x:rFonts x:eastAsia="楷体"/><x:sz x:val="24"/>"#));
        assert!(
            rendered
                .diagnostics
                .iter()
                .any(|item| item.code == "PAPER_EXPORT_TEMPLATE_STYLES_APPLIED")
        );
        super::super::parse_document_xml(xml.as_bytes(), &DocxLimits::default()).unwrap();
    }

    #[test]
    fn renders_editable_omml_managed_images_and_native_tables() {
        let stem = super::super::parse_rich_content(
            r#"<p>计算<span class="math-node" data-latex="\frac{1}{2}">1/2</span><img data-resource-id="image-1" width="160" alt="函数图像"></p><table><tbody><tr><td><p>x</p></td><td><p>0</p></td></tr><tr><td><p>y</p></td><td><p>1</p></td></tr></tbody></table>"#,
            "计算 1/2 x 0 y 1",
        );
        assert!(stem.unsupported.is_empty());
        let rich_paper = PaperRichDocument {
            title: "富文本试卷".to_owned(),
            mode: PaperContentMode::PaperOnly,
            items: vec![PaperRichItem {
                question_type: "short_answer".to_owned(),
                question_type_label: "简答题".to_owned(),
                stem: stem.content,
                options: Vec::new(),
                answer: PaperRichContent::default(),
                explanation: PaperRichContent::default(),
            }],
            omit_empty_field_placeholders: false,
        };
        let relationships = BTreeMap::from([(
            "image-1".to_owned(),
            PaperImageRelationship {
                relationship_id: "rIdZhitikuImage1".to_owned(),
                intrinsic_width_px: 640,
                intrinsic_height_px: 480,
            },
        )]);

        let rendered = render_rich_paper_document_xml_with_styles(
            template("http://schemas.openxmlformats.org/wordprocessingml/2006/main").as_bytes(),
            &rich_paper,
            &relationships,
            None,
            &DocxLimits::default(),
        )
        .unwrap();
        let xml = String::from_utf8(rendered.document_xml).unwrap();

        assert!(xml.contains("<m:oMath"));
        assert!(xml.contains("<m:f>"));
        assert!(!xml.contains(r"\frac{1}{2}"));
        assert!(xml.contains("r:embed=\"rIdZhitikuImage1\""));
        assert!(xml.contains("<ztw:tbl"));
        assert!(xml.contains("<ztw:tc>"));
        assert!(
            rendered
                .diagnostics
                .iter()
                .any(|item| item.code == "PAPER_EXPORT_RICH_CONTENT_WRITTEN")
        );
        super::super::parse_document_xml(xml.as_bytes(), &DocxLimits::default()).unwrap();
    }

    #[test]
    fn question_bank_interchange_keeps_empty_fields_blank() {
        let rich_paper = PaperRichDocument {
            title: "题库导出".to_owned(),
            mode: PaperContentMode::PaperAnswersExplanations,
            items: vec![PaperRichItem {
                question_type: "short_answer".to_owned(),
                question_type_label: "简答题".to_owned(),
                stem: PaperRichContent::default(),
                options: Vec::new(),
                answer: PaperRichContent::default(),
                explanation: PaperRichContent::default(),
            }],
            omit_empty_field_placeholders: true,
        };

        let rendered = render_rich_paper_document_xml_with_styles(
            template("http://schemas.openxmlformats.org/wordprocessingml/2006/main").as_bytes(),
            &rich_paper,
            &BTreeMap::new(),
            None,
            &DocxLimits::default(),
        )
        .unwrap();
        let xml = String::from_utf8(rendered.document_xml).unwrap();

        assert!(!xml.contains("未填写"));
        assert!(xml.contains("参考答案"));
        assert!(xml.contains("题目解析"));
    }

    #[test]
    fn uses_the_strict_namespace_when_the_template_is_strict() {
        let rendered = render_paper_document_xml(
            template("http://purl.oclc.org/ooxml/wordprocessingml/main").as_bytes(),
            &paper(PaperContentMode::PaperOnly),
            &DocxLimits::default(),
        )
        .unwrap();
        let xml = String::from_utf8(rendered.document_xml).unwrap();
        assert!(xml.contains("xmlns:ztw=\"http://purl.oclc.org/ooxml/wordprocessingml/main\""));
        assert!(!xml.contains("{{ZT_"));
    }

    #[test]
    fn overrides_page_size_without_changing_other_section_properties() {
        let source = br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p/><w:sectPr><w:pgSz w:w="11906" w:h="16838" w:code="9"/><w:pgMar w:top="720" w:right="900" w:bottom="720" w:left="900"/></w:sectPr></w:body></w:document>"#;
        let output = override_document_page_setup(
            source,
            PaperPageSetupOverride {
                width_twips: 23_811,
                height_twips: 16_838,
            },
            &DocxLimits::default(),
        )
        .unwrap();
        let xml = String::from_utf8(output).unwrap();

        assert!(xml.contains(r#"w:pgSz w:code="9" w:w="23811" w:h="16838" w:orient="landscape""#));
        assert!(
            xml.contains(r#"<w:pgMar w:top="720" w:right="900" w:bottom="720" w:left="900"/>"#)
        );
    }

    #[test]
    fn rejects_missing_and_duplicate_required_anchors() {
        let missing = template("http://schemas.openxmlformats.org/wordprocessingml/2006/main")
            .replace("{{ZT_QUESTIONS}}", "questions");
        let result = render_paper_document_xml(
            missing.as_bytes(),
            &paper(PaperContentMode::PaperOnly),
            &DocxLimits::default(),
        );
        assert!(matches!(result, Err(DocxError::Rejected { .. })));

        let duplicate = template("http://schemas.openxmlformats.org/wordprocessingml/2006/main")
            .replace(
                "{{ZT_QUESTIONS}}",
                "{{ZT_QUESTIONS}}</x:t></x:r></x:p><x:p><x:r><x:t>{{ZT_QUESTIONS}}",
            );
        let result = render_paper_document_xml(
            duplicate.as_bytes(),
            &paper(PaperContentMode::PaperOnly),
            &DocxLimits::default(),
        );
        assert!(matches!(result, Err(DocxError::Rejected { .. })));
    }

    #[test]
    fn content_modes_require_only_their_own_regions() {
        assert_eq!(
            PaperContentMode::PaperOnly.required_anchor_names(),
            &["ZT_QUESTIONS"]
        );
        assert_eq!(
            PaperContentMode::AnswersOnly.required_anchor_names(),
            &["ZT_QUESTIONS"]
        );
        assert_eq!(
            PaperContentMode::PaperAndAnswers.required_anchor_names(),
            &["ZT_QUESTIONS"]
        );
        assert_eq!(option_label(25), "Z");
        assert_eq!(option_label(26), "AA");
    }

    #[test]
    fn question_type_headings_follow_saved_item_order() {
        let mut value = paper(PaperContentMode::PaperOnly);
        let mut second = value.items[0].clone();
        second.question_type = "short_answer".to_owned();
        second.question_type_label = "简答题".to_owned();
        second.options.clear();
        value.items.push(second);
        let rendered = render_paper_document_xml(
            template("http://schemas.openxmlformats.org/wordprocessingml/2006/main").as_bytes(),
            &value,
            &DocxLimits::default(),
        )
        .unwrap();
        let xml = String::from_utf8(rendered.document_xml).unwrap();
        assert!(
            xml.find("一、选择题：本题共1个小题，每小题      分。共      分。")
                .unwrap()
                < xml.find("1. 第一行").unwrap()
        );
        assert!(
            xml.find("二、简答题：本题共1个小题，每小题      分。共      分。")
                .unwrap()
                < xml.find("2. 第一行").unwrap()
        );
    }

    #[test]
    fn rejects_inline_content_control_anchors() {
        let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
<w:p><w:r><w:t>前缀</w:t></w:r><w:sdt><w:sdtPr><w:tag w:val="ZT_QUESTIONS"/></w:sdtPr><w:sdtContent><w:r><w:t>题目</w:t></w:r></w:sdtContent></w:sdt></w:p>
</w:body></w:document>"#;
        let result = render_paper_document_xml(
            xml.as_bytes(),
            &paper(PaperContentMode::PaperOnly),
            &DocxLimits::default(),
        );
        let Err(DocxError::Rejected { diagnostics }) = result else {
            panic!("inline anchor must be rejected");
        };
        assert!(
            diagnostics
                .iter()
                .any(|item| item.code == "PAPER_EXPORT_INLINE_ANCHOR_UNSUPPORTED")
        );
    }

    #[test]
    fn rejects_unknown_zt_anchors_instead_of_leaking_placeholders() {
        let xml = template("http://schemas.openxmlformats.org/wordprocessingml/2006/main").replace(
            "<x:sectPr/>",
            "<x:p><x:r><x:t>{{ZT_SCHOOL}}</x:t></x:r></x:p><x:sectPr/>",
        );
        let result = render_paper_document_xml(
            xml.as_bytes(),
            &paper(PaperContentMode::PaperOnly),
            &DocxLimits::default(),
        );
        let Err(DocxError::Rejected { diagnostics }) = result else {
            panic!("unknown anchor must be rejected");
        };
        assert!(
            diagnostics
                .iter()
                .any(|item| item.code == "PAPER_EXPORT_UNKNOWN_TEMPLATE_ANCHOR")
        );
    }

    #[test]
    fn rejects_unregistered_visible_zt_markers() {
        for visible_text in [
            "学校：{{ZT_SCHOOL}}",
            "页眉中的支持名 {{ZT_TITLE}}",
            "未闭合 {{ZT_SCHOOL",
            "带空白的内联标记 {{   ZT_SCHOOL   }}",
        ] {
            let paragraph = format!("<x:p><x:r><x:t>{visible_text}</x:t></x:r></x:p>");
            let xml = template("http://schemas.openxmlformats.org/wordprocessingml/2006/main")
                .replace("<x:sectPr/>", &format!("{paragraph}<x:sectPr/>"));
            let result = render_paper_document_xml(
                xml.as_bytes(),
                &paper(PaperContentMode::PaperOnly),
                &DocxLimits::default(),
            );
            let Err(DocxError::Rejected { diagnostics }) = result else {
                panic!("visible marker must be rejected: {visible_text}");
            };
            assert!(
                diagnostics
                    .iter()
                    .any(|item| { item.code == "PAPER_EXPORT_UNREGISTERED_VISIBLE_MARKER" }),
                "unexpected diagnostics for {visible_text}: {diagnostics:?}"
            );
        }
    }

    #[test]
    fn allows_registered_markers_with_inner_whitespace() {
        let xml = template("http://schemas.openxmlformats.org/wordprocessingml/2006/main")
            .replace("{{ZT_QUESTIONS}}", "{{   ZT_QUESTIONS   }}");
        let rendered = render_paper_document_xml(
            xml.as_bytes(),
            &paper(PaperContentMode::PaperOnly),
            &DocxLimits::default(),
        )
        .unwrap();
        let output = String::from_utf8(rendered.document_xml).unwrap();
        assert!(!output.contains("ZT_QUESTIONS"));
    }

    #[test]
    fn ignores_zt_text_that_is_not_part_of_visible_paragraph_text() {
        let xml = template("http://schemas.openxmlformats.org/wordprocessingml/2006/main").replace(
            "<x:sectPr/>",
            "<x:p><x:r><x:instrText>{{ZT_FIELD_CODE}}</x:instrText></x:r></x:p><x:sectPr/>",
        );
        let rendered = render_paper_document_xml(
            xml.as_bytes(),
            &paper(PaperContentMode::PaperOnly),
            &DocxLimits::default(),
        )
        .unwrap();
        assert!(
            String::from_utf8(rendered.document_xml)
                .unwrap()
                .contains("{{ZT_FIELD_CODE}}")
        );
    }
}
