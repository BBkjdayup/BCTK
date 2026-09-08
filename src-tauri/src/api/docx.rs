use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Cursor, Read},
    path::{Path, PathBuf},
};

#[cfg(test)]
use crate::docx::raw_copy_with_replacements;
use crate::docx::{
    Diagnostic, DiagnosticSeverity, DocumentTable, DocxError, DocxLimits,
    ExtractedEditableFormulaOccurrence, ExtractedImageOccurrence, ExtractedMathTypeOccurrence,
    FORMULA_PLACEHOLDER, PackageInspection, PackageKind, RawCopyExport, inspect_docx,
    raw_copy_with_replacements_and_additions, read_document_from_docx, read_images_from_docx,
    read_mathtype_from_docx, read_omml_from_docx,
};

#[cfg(test)]
use super::models::ExportDocxRequestApi;
use super::models::{
    AnalyzeDocxRequestApi, CommandError, CommandResult, DocxAnalysisApi, DocxDiagnosticApi,
    DocxExportResultApi, DocxParagraphApi,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WordFileKind {
    Document,
    Template,
}

const MAX_ANALYSIS_PARAGRAPHS: usize = 50_000;
const MAX_ANALYSIS_VISIBLE_BYTES: usize = 10 * 1024 * 1024;

struct SourcePath {
    canonical: PathBuf,
    display: String,
    file_kind: WordFileKind,
}

struct OutputPath {
    normalized: PathBuf,
    display: String,
    file_kind: WordFileKind,
}

pub(super) struct PreparedWordImportSource {
    pub analysis: DocxAnalysisApi,
    pub source_filename: String,
    pub source_bytes: Vec<u8>,
    pub images: Vec<ExtractedImageOccurrence>,
    pub formulas: Vec<ExtractedEditableFormulaOccurrence>,
    pub tables: Vec<DocumentTable>,
}

pub(super) fn analyze(request: AnalyzeDocxRequestApi) -> CommandResult<DocxAnalysisApi> {
    let source = validate_source_path(&request.input_path)?;
    let limits = DocxLimits::default();
    let bytes = read_source_bounded(&source.canonical, limits.max_archive_bytes)?;
    analyze_loaded_source(&source, &bytes, &limits)
}

pub(super) fn prepare_word_import(
    request: AnalyzeDocxRequestApi,
) -> CommandResult<PreparedWordImportSource> {
    let source = validate_source_path(&request.input_path)?;
    if source.file_kind != WordFileKind::Document {
        return Err(CommandError::new(
            "WORD_IMPORT_REQUIRES_DOCX",
            "题库导入只接受 .docx 文档；.dotx 模板请到模板管理中导入。",
        ));
    }
    let source_filename = source
        .canonical
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .ok_or_else(|| {
            CommandError::new(
                "DOCX_SOURCE_FILENAME_INVALID",
                "Word 文件名包含无法安全保存的字符，请重命名后重试。",
            )
        })?;
    let limits = DocxLimits {
        allow_mathtype_ole: true,
        ..DocxLimits::default()
    };
    let source_bytes = read_source_bounded(&source.canonical, limits.max_archive_bytes)?;
    let mut analysis = analyze_loaded_source(&source, &source_bytes, &limits)?;
    if !analysis.is_valid {
        let message = analysis
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.severity == "error")
            .map(|diagnostic| diagnostic.message.clone())
            .unwrap_or_else(|| "Word 文档未通过安全与结构检查。".to_owned());
        return Err(CommandError::new("DOCX_IMPORT_ANALYSIS_REJECTED", message));
    }
    let images = read_images_from_docx(Cursor::new(&source_bytes), &limits).map_err(|error| {
        let message = diagnostics_from_error(error)
            .into_iter()
            .next()
            .map(|diagnostic| diagnostic.message)
            .unwrap_or_else(|| "无法安全提取 Word 图片。".to_owned());
        CommandError::new("DOCX_IMAGE_EXTRACTION_FAILED", message)
    })?;
    let tables = read_document_from_docx(Cursor::new(&source_bytes), &limits)
        .map_err(|error| {
            let message = diagnostics_from_error(error)
                .into_iter()
                .next()
                .map(|diagnostic| diagnostic.message)
                .unwrap_or_else(|| "无法安全提取 Word 表格结构。".to_owned());
            CommandError::new("DOCX_TABLE_EXTRACTION_FAILED", message)
        })?
        .tables;
    let native_formulas =
        read_omml_from_docx(Cursor::new(&source_bytes), &limits).map_err(|error| {
            let message = diagnostics_from_error(error)
                .into_iter()
                .next()
                .map(|diagnostic| diagnostic.message)
                .unwrap_or_else(|| "无法安全转换 Word 原生公式。".to_owned());
            CommandError::new("DOCX_OMML_CONVERSION_FAILED", message)
        })?;
    let mathtype_formulas =
        read_mathtype_from_docx(Cursor::new(&source_bytes), &limits).map_err(|error| {
            let message = diagnostics_from_error(error)
                .into_iter()
                .next()
                .map(|diagnostic| diagnostic.message)
                .unwrap_or_else(|| "无法安全转换 Word 中的 MathType 公式。".to_owned());
            CommandError::new("DOCX_MATHTYPE_CONVERSION_FAILED", message)
        })?;
    let final_mathtype_offsets = merge_mathtype_occurrences(&mut analysis, &mathtype_formulas)?;
    let formulas =
        normalize_formula_occurrences(native_formulas, mathtype_formulas, &final_mathtype_offsets);
    Ok(PreparedWordImportSource {
        analysis,
        source_filename,
        source_bytes,
        images,
        formulas,
        tables,
    })
}

fn merge_mathtype_occurrences(
    analysis: &mut DocxAnalysisApi,
    formulas: &[ExtractedMathTypeOccurrence],
) -> CommandResult<Vec<usize>> {
    let mut indices_by_paragraph = BTreeMap::<usize, Vec<usize>>::new();
    let mut final_offsets = vec![0usize; formulas.len()];
    for (index, formula) in formulas.iter().enumerate() {
        indices_by_paragraph
            .entry(formula.paragraph_index)
            .or_default()
            .push(index);
    }
    for (paragraph_index, indices) in indices_by_paragraph {
        let paragraph = analysis
            .paragraphs
            .get_mut(paragraph_index)
            .ok_or_else(|| {
                CommandError::new(
                    "DOCX_MATHTYPE_LOCATION_INVALID",
                    "MathType 公式引用了不存在的 Word 段落，已停止导入。",
                )
            })?;
        let mut indices = indices;
        indices.sort_by_key(|index| formulas[*index].text_char_offset);
        let mut characters = paragraph.text.chars().collect::<Vec<_>>();
        for index in indices {
            let formula = &formulas[index];
            // MathType extraction counts earlier OLE formulas as one logical
            // character, so its offsets already target the final merged text.
            let final_offset = formula.text_char_offset;
            if final_offset > characters.len() {
                return Err(CommandError::new(
                    "DOCX_MATHTYPE_LOCATION_INVALID",
                    "MathType 公式在段落中的位置无效，已停止导入。",
                ));
            }
            characters.insert(final_offset, FORMULA_PLACEHOLDER);
            final_offsets[index] = final_offset;
            paragraph.formula_count = paragraph.formula_count.saturating_add(1);
        }
        paragraph.text = characters.into_iter().collect();
    }
    analysis.formula_count = analysis
        .formula_count
        .saturating_add(u32::try_from(formulas.len()).unwrap_or(u32::MAX));
    analysis.visible_text = analysis
        .paragraphs
        .iter()
        .map(|paragraph| paragraph.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    Ok(final_offsets)
}

fn normalize_formula_occurrences(
    native_formulas: Vec<ExtractedEditableFormulaOccurrence>,
    mathtype_formulas: Vec<ExtractedMathTypeOccurrence>,
    final_mathtype_offsets: &[usize],
) -> Vec<ExtractedEditableFormulaOccurrence> {
    let mut formulas = native_formulas
        .into_iter()
        .map(|mut occurrence| {
            let mut paragraph_mathtype = mathtype_formulas
                .iter()
                .filter(|candidate| candidate.paragraph_index == occurrence.paragraph_index)
                .collect::<Vec<_>>();
            paragraph_mathtype.sort_by_key(|candidate| candidate.text_char_offset);
            let preceding_mathtype = paragraph_mathtype
                .iter()
                .enumerate()
                .filter(|(earlier_count, candidate)| {
                    candidate.text_char_offset.saturating_sub(*earlier_count)
                        < occurrence.text_char_offset
                })
                .count();
            occurrence.text_char_offset = occurrence
                .text_char_offset
                .saturating_add(preceding_mathtype);
            occurrence
        })
        .collect::<Vec<_>>();
    formulas.extend(
        mathtype_formulas
            .into_iter()
            .zip(final_mathtype_offsets.iter().copied())
            .map(
                |(occurrence, text_char_offset)| ExtractedEditableFormulaOccurrence {
                    paragraph_index: occurrence.paragraph_index,
                    text_char_offset,
                    latex: occurrence.latex,
                    source_kind: "mathtype_mtef5".to_owned(),
                    product_version: occurrence.product_version,
                    product_subversion: occurrence.product_subversion,
                },
            ),
    );
    formulas.sort_by_key(|occurrence| (occurrence.paragraph_index, occurrence.text_char_offset));
    formulas
}

fn analyze_loaded_source(
    source: &SourcePath,
    bytes: &[u8],
    limits: &DocxLimits,
) -> CommandResult<DocxAnalysisApi> {
    let archive_bytes = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    let inspection = match inspect_docx(Cursor::new(bytes), limits) {
        Ok(inspection) => inspection,
        Err(error) => {
            return Ok(empty_analysis(
                &source.display,
                archive_bytes,
                "unknown",
                0,
                0,
                diagnostics_from_error(error),
            ));
        }
    };

    if !inspection.is_acceptable() {
        return Ok(analysis_from_rejected_inspection(
            &source.display,
            archive_bytes,
            &inspection,
        ));
    }

    let parsed = match read_document_from_docx(Cursor::new(bytes), limits) {
        Ok(parsed) => parsed,
        Err(error) => {
            return Ok(empty_analysis(
                &source.display,
                archive_bytes,
                package_kind_name(inspection.package_kind),
                saturating_u32(inspection.parts.len()),
                inspection.total_uncompressed_bytes,
                diagnostics_from_error(error),
            ));
        }
    };

    let visible_bytes = parsed.paragraphs.iter().fold(0usize, |total, paragraph| {
        total.saturating_add(paragraph.logical_text.len().saturating_add(1))
    });
    if parsed.paragraphs.len() > MAX_ANALYSIS_PARAGRAPHS
        || visible_bytes > MAX_ANALYSIS_VISIBLE_BYTES
    {
        return Ok(empty_analysis(
            &source.display,
            archive_bytes,
            package_kind_name(inspection.package_kind),
            saturating_u32(inspection.parts.len()),
            inspection.total_uncompressed_bytes,
            vec![synthetic_diagnostic(
                "DOCX_VISIBLE_CONTENT_TOO_LARGE",
                Some("word/document.xml".to_owned()),
                format!(
                    "文档包含 {} 个段落、约 {} MB 可见文字，超过单次导入审查上限，请拆分文档后再导入。",
                    parsed.paragraphs.len(),
                    visible_bytes.div_ceil(1024 * 1024)
                ),
            )],
        ));
    }

    let paragraphs = parsed
        .paragraphs
        .iter()
        .map(|paragraph| DocxParagraphApi {
            index: saturating_u32(paragraph.index),
            text: paragraph.logical_text.clone(),
            formula_count: saturating_u32(paragraph.math_fragment_indices.len()),
        })
        .collect::<Vec<_>>();
    let visible_text = parsed
        .paragraphs
        .iter()
        .map(|paragraph| paragraph.logical_text.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    Ok(DocxAnalysisApi {
        source_path: source.display.clone(),
        archive_bytes,
        package_kind: package_kind_name(inspection.package_kind).to_owned(),
        is_valid: true,
        visible_text,
        paragraph_count: saturating_u32(paragraphs.len()),
        formula_count: saturating_u32(parsed.math_fragments.len()),
        part_count: saturating_u32(inspection.parts.len()),
        total_uncompressed_bytes: inspection.total_uncompressed_bytes,
        paragraphs,
        diagnostics: parsed
            .diagnostics
            .iter()
            .map(DocxDiagnosticApi::from)
            .collect(),
    })
}

/// Legacy path-based exporter retained only as a regression harness. Production
/// exports use hash-verified managed template bytes instead.
#[cfg(test)]
pub(super) fn export(request: ExportDocxRequestApi) -> CommandResult<DocxExportResultApi> {
    let source = validate_source_path(&request.template_path)?;
    let output = validate_output_path(&request.output_path)?;
    if same_windows_path(&source.canonical, &output.normalized) {
        return Err(CommandError::new(
            "DOCX_OUTPUT_EQUALS_SOURCE",
            "导出位置不能与原模板相同，请选择一个新的文件名。",
        ));
    }

    let limits = DocxLimits::default();
    if request
        .document_xml
        .as_ref()
        .is_some_and(|xml| xml.len() as u64 > limits.max_xml_part_bytes)
    {
        return Err(CommandError::new(
            "DOCX_REPLACEMENT_TOO_LARGE",
            format!(
                "document.xml 内容超过 {} 字节的安全上限。",
                limits.max_xml_part_bytes
            ),
        ));
    }

    let template_bytes = read_source_bounded(&source.canonical, limits.max_archive_bytes)?;
    let inspection = match inspect_docx(Cursor::new(&template_bytes), &limits) {
        Ok(inspection) => inspection,
        Err(error) => {
            return Ok(failed_export(
                &source.display,
                &output.display,
                "unknown",
                0,
                diagnostics_from_error(error),
            ));
        }
    };

    if !inspection.is_acceptable() {
        return Ok(failed_export(
            &source.display,
            &output.display,
            package_kind_name(inspection.package_kind),
            saturating_u32(inspection.parts.len()),
            inspection
                .diagnostics
                .iter()
                .map(DocxDiagnosticApi::from)
                .collect(),
        ));
    }
    validate_package_extension(&source, &output, inspection.package_kind)?;

    let mut replacements = BTreeMap::new();
    if let Some(document_xml) = request.document_xml {
        replacements.insert("word/document.xml".to_owned(), document_xml.into_bytes());
    }

    let temporary_path = temporary_output_path(&output.normalized)?;
    let temporary_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary_path)
        .map_err(|error| {
            CommandError::new(
                "DOCX_TEMP_FILE_CREATE_FAILED",
                format!("无法在导出目录创建临时文件：{error}"),
            )
        })?;

    let export_result = raw_copy_with_replacements(
        Cursor::new(&template_bytes),
        temporary_file,
        &replacements,
        &limits,
    );
    let RawCopyExport {
        output: output_file,
        summary,
    } = match export_result {
        Ok(exported) => exported,
        Err(error) => {
            remove_temporary_file(&temporary_path);
            return Ok(failed_export(
                &source.display,
                &output.display,
                package_kind_name(inspection.package_kind),
                saturating_u32(inspection.parts.len()),
                diagnostics_from_error(error),
            ));
        }
    };

    if let Err(error) = output_file.sync_all() {
        drop(output_file);
        remove_temporary_file(&temporary_path);
        return Err(CommandError::new(
            "DOCX_EXPORT_FLUSH_FAILED",
            format!("导出文件写入磁盘失败：{error}"),
        ));
    }
    drop(output_file);

    let verification_file = File::open(&temporary_path).map_err(|error| {
        remove_temporary_file(&temporary_path);
        CommandError::new(
            "DOCX_EXPORT_VERIFY_OPEN_FAILED",
            format!("无法重新打开导出临时文件进行安全校验：{error}"),
        )
    })?;
    let verification = match inspect_docx(verification_file, &limits) {
        Ok(verification) => verification,
        Err(error) => {
            remove_temporary_file(&temporary_path);
            return Ok(failed_export(
                &source.display,
                &output.display,
                package_kind_name(inspection.package_kind),
                saturating_u32(inspection.parts.len()),
                diagnostics_from_error(error),
            ));
        }
    };
    if !verification.is_acceptable() || verification.package_kind != inspection.package_kind {
        let mut diagnostics = verification
            .diagnostics
            .iter()
            .map(DocxDiagnosticApi::from)
            .collect::<Vec<_>>();
        if verification.package_kind != inspection.package_kind {
            diagnostics.push(synthetic_diagnostic(
                "DOCX_EXPORT_PACKAGE_KIND_CHANGED",
                Some("[Content_Types].xml".to_owned()),
                "导出文件的 Word 包类型与模板不一致。",
            ));
        }
        remove_temporary_file(&temporary_path);
        return Ok(failed_export(
            &source.display,
            &output.display,
            package_kind_name(inspection.package_kind),
            saturating_u32(inspection.parts.len()),
            diagnostics,
        ));
    }

    if path_entry_exists(&output.normalized)? {
        remove_temporary_file(&temporary_path);
        return Err(CommandError::new(
            "DOCX_OUTPUT_ALREADY_EXISTS",
            "导出目标已经存在。首期为避免误覆盖，请选择新的文件名。",
        ));
    }
    if let Err(error) = fs::rename(&temporary_path, &output.normalized) {
        remove_temporary_file(&temporary_path);
        return Err(CommandError::new(
            "DOCX_EXPORT_COMMIT_FAILED",
            format!("无法把临时文件保存到目标位置：{error}"),
        ));
    }

    Ok(DocxExportResultApi {
        exported: true,
        template_path: source.display,
        output_path: output.display,
        package_kind: package_kind_name(inspection.package_kind).to_owned(),
        output_bytes: Some(summary.output_bytes),
        input_parts: saturating_u32(summary.input_parts),
        raw_copied_parts: saturating_u32(summary.raw_copied_parts),
        rewritten_parts: summary.rewritten_parts,
        diagnostics: inspection
            .diagnostics
            .iter()
            .map(DocxDiagnosticApi::from)
            .collect(),
    })
}

/// Writes a document from bytes that were already read and hash-verified from
/// the private managed-template directory. Unlike the legacy helper, this API
/// has no template path input and cannot race a second template read.
#[cfg(test)]
pub(super) fn export_managed_template_bytes(
    template_bytes: Vec<u8>,
    output_path: String,
    document_xml: Vec<u8>,
) -> CommandResult<DocxExportResultApi> {
    export_managed_template_bytes_with_parts(
        template_bytes,
        output_path,
        document_xml,
        BTreeMap::new(),
        BTreeMap::new(),
    )
}

pub(super) fn export_managed_template_bytes_with_parts(
    template_bytes: Vec<u8>,
    output_path: String,
    document_xml: Vec<u8>,
    mut replacement_parts: BTreeMap<String, Vec<u8>>,
    addition_parts: BTreeMap<String, Vec<u8>>,
) -> CommandResult<DocxExportResultApi> {
    let output = validate_output_path(&output_path)?;
    if output.file_kind != WordFileKind::Document {
        return Err(CommandError::new(
            "DOCX_OUTPUT_TYPE_UNSUPPORTED",
            "受管试卷模板只能导出为 .docx 文件。",
        ));
    }
    let limits = DocxLimits::default();
    if template_bytes.len() as u64 > limits.max_archive_bytes {
        return Err(CommandError::new(
            "DOCX_ARCHIVE_TOO_LARGE",
            "受管模板超过 DOCX 安全大小上限。",
        ));
    }
    if document_xml.len() as u64 > limits.max_xml_part_bytes {
        return Err(CommandError::new(
            "DOCX_REPLACEMENT_TOO_LARGE",
            format!(
                "document.xml 内容超过 {} 字节的安全上限。",
                limits.max_xml_part_bytes
            ),
        ));
    }

    let inspection = match inspect_docx(Cursor::new(&template_bytes), &limits) {
        Ok(inspection) => inspection,
        Err(error) => {
            return Ok(failed_export(
                "managed-template",
                &output.display,
                "unknown",
                0,
                diagnostics_from_error(error),
            ));
        }
    };
    if !inspection.is_acceptable() {
        return Ok(failed_export(
            "managed-template",
            &output.display,
            package_kind_name(inspection.package_kind),
            saturating_u32(inspection.parts.len()),
            inspection
                .diagnostics
                .iter()
                .map(DocxDiagnosticApi::from)
                .collect(),
        ));
    }
    if inspection.package_kind != PackageKind::Document {
        return Ok(failed_export(
            "managed-template",
            &output.display,
            package_kind_name(inspection.package_kind),
            saturating_u32(inspection.parts.len()),
            vec![synthetic_diagnostic(
                "PAPER_EXPORT_TEMPLATE_KIND_UNSUPPORTED",
                Some("[Content_Types].xml".to_owned()),
                "试卷导出只支持 DOCX 文档模板，不支持 DOTX 或未知包类型。",
            )],
        ));
    }

    replacement_parts.insert("word/document.xml".to_owned(), document_xml);
    let temporary_path = temporary_output_path(&output.normalized)?;
    let temporary_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary_path)
        .map_err(|error| {
            CommandError::new(
                "DOCX_TEMP_FILE_CREATE_FAILED",
                format!("无法在导出目录创建临时文件：{error}"),
            )
        })?;
    let RawCopyExport {
        output: output_file,
        summary,
    } = match raw_copy_with_replacements_and_additions(
        Cursor::new(&template_bytes),
        temporary_file,
        &replacement_parts,
        &addition_parts,
        &limits,
    ) {
        Ok(exported) => exported,
        Err(error) => {
            remove_temporary_file(&temporary_path);
            return Ok(failed_export(
                "managed-template",
                &output.display,
                "document",
                saturating_u32(inspection.parts.len()),
                diagnostics_from_error(error),
            ));
        }
    };

    if let Err(error) = output_file.sync_all() {
        drop(output_file);
        remove_temporary_file(&temporary_path);
        return Err(CommandError::new(
            "DOCX_EXPORT_FLUSH_FAILED",
            format!("导出文件写入磁盘失败：{error}"),
        ));
    }
    drop(output_file);

    let verification_file = File::open(&temporary_path).map_err(|error| {
        remove_temporary_file(&temporary_path);
        CommandError::new(
            "DOCX_EXPORT_VERIFY_OPEN_FAILED",
            format!("无法重新打开导出临时文件进行安全校验：{error}"),
        )
    })?;
    let verification = match inspect_docx(verification_file, &limits) {
        Ok(verification) => verification,
        Err(error) => {
            remove_temporary_file(&temporary_path);
            return Ok(failed_export(
                "managed-template",
                &output.display,
                "document",
                saturating_u32(inspection.parts.len()),
                diagnostics_from_error(error),
            ));
        }
    };
    if !verification.is_acceptable() || verification.package_kind != PackageKind::Document {
        let mut diagnostics = verification
            .diagnostics
            .iter()
            .map(DocxDiagnosticApi::from)
            .collect::<Vec<_>>();
        if verification.package_kind != PackageKind::Document {
            diagnostics.push(synthetic_diagnostic(
                "DOCX_EXPORT_PACKAGE_KIND_CHANGED",
                Some("[Content_Types].xml".to_owned()),
                "导出文件不再是有效的 DOCX 文档包。",
            ));
        }
        remove_temporary_file(&temporary_path);
        return Ok(failed_export(
            "managed-template",
            &output.display,
            "document",
            saturating_u32(inspection.parts.len()),
            diagnostics,
        ));
    }

    if path_entry_exists(&output.normalized)? {
        remove_temporary_file(&temporary_path);
        return Err(CommandError::new(
            "DOCX_OUTPUT_ALREADY_EXISTS",
            "导出目标已经存在。为避免误覆盖，请选择新的文件名。",
        ));
    }
    if let Err(error) = fs::rename(&temporary_path, &output.normalized) {
        remove_temporary_file(&temporary_path);
        return Err(CommandError::new(
            "DOCX_EXPORT_COMMIT_FAILED",
            format!("无法把临时文件保存到目标位置：{error}"),
        ));
    }

    Ok(DocxExportResultApi {
        exported: true,
        template_path: "managed-template".to_owned(),
        output_path: output.display,
        package_kind: "document".to_owned(),
        output_bytes: Some(summary.output_bytes),
        input_parts: saturating_u32(summary.input_parts),
        raw_copied_parts: saturating_u32(summary.raw_copied_parts),
        rewritten_parts: summary.rewritten_parts,
        diagnostics: inspection
            .diagnostics
            .iter()
            .map(DocxDiagnosticApi::from)
            .collect(),
    })
}

impl From<&Diagnostic> for DocxDiagnosticApi {
    fn from(diagnostic: &Diagnostic) -> Self {
        Self {
            severity: severity_name(diagnostic.severity).to_owned(),
            code: diagnostic.code.to_owned(),
            part_name: diagnostic.part_name.clone(),
            message: diagnostic.message.clone(),
            suggested_action: diagnostic.suggested_action.clone(),
        }
    }
}

fn analysis_from_rejected_inspection(
    source_path: &str,
    archive_bytes: u64,
    inspection: &PackageInspection,
) -> DocxAnalysisApi {
    empty_analysis(
        source_path,
        archive_bytes,
        package_kind_name(inspection.package_kind),
        saturating_u32(inspection.parts.len()),
        inspection.total_uncompressed_bytes,
        inspection
            .diagnostics
            .iter()
            .map(DocxDiagnosticApi::from)
            .collect(),
    )
}

fn empty_analysis(
    source_path: &str,
    archive_bytes: u64,
    package_kind: &str,
    part_count: u32,
    total_uncompressed_bytes: u64,
    diagnostics: Vec<DocxDiagnosticApi>,
) -> DocxAnalysisApi {
    DocxAnalysisApi {
        source_path: source_path.to_owned(),
        archive_bytes,
        package_kind: package_kind.to_owned(),
        is_valid: false,
        visible_text: String::new(),
        paragraph_count: 0,
        formula_count: 0,
        part_count,
        total_uncompressed_bytes,
        paragraphs: Vec::new(),
        diagnostics,
    }
}

fn failed_export(
    template_path: &str,
    output_path: &str,
    package_kind: &str,
    input_parts: u32,
    diagnostics: Vec<DocxDiagnosticApi>,
) -> DocxExportResultApi {
    DocxExportResultApi {
        exported: false,
        template_path: template_path.to_owned(),
        output_path: output_path.to_owned(),
        package_kind: package_kind.to_owned(),
        output_bytes: None,
        input_parts,
        raw_copied_parts: 0,
        rewritten_parts: Vec::new(),
        diagnostics,
    }
}

pub(super) fn diagnostics_from_error(error: DocxError) -> Vec<DocxDiagnosticApi> {
    match error {
        DocxError::Rejected { diagnostics } => {
            diagnostics.iter().map(DocxDiagnosticApi::from).collect()
        }
        DocxError::Xml {
            part_name,
            position,
            message,
        } => vec![synthetic_diagnostic(
            "DOCX_XML_INVALID",
            Some(part_name),
            format!("XML 在第 {position} 字节附近无效：{message}"),
        )],
        DocxError::LimitExceeded {
            resource,
            limit,
            actual,
        } => vec![synthetic_diagnostic(
            "DOCX_LIMIT_EXCEEDED",
            None,
            format!("{resource} 超过安全上限：上限 {limit}，实际 {actual}。"),
        )],
        DocxError::Zip(error) => vec![synthetic_diagnostic(
            "DOCX_ZIP_INVALID",
            None,
            format!("文件不是有效的 DOCX ZIP 包：{error}"),
        )],
        DocxError::Io(error) => vec![synthetic_diagnostic(
            "DOCX_PACKAGE_IO_FAILED",
            None,
            format!("读取 DOCX 包时发生错误：{error}"),
        )],
    }
}

fn synthetic_diagnostic(
    code: impl Into<String>,
    part_name: Option<String>,
    message: impl Into<String>,
) -> DocxDiagnosticApi {
    DocxDiagnosticApi {
        severity: "error".to_owned(),
        code: code.into(),
        part_name,
        message: message.into(),
        suggested_action: None,
    }
}

fn validate_source_path(raw_path: &str) -> CommandResult<SourcePath> {
    let requested = absolute_path(raw_path, "DOCX_SOURCE_PATH_INVALID", "源文件")?;
    let file_kind = word_file_kind(&requested).ok_or_else(|| {
        CommandError::new(
            "DOCX_FILE_TYPE_UNSUPPORTED",
            "首期只支持 .docx 和 .dotx 文件。启用宏的 .docm/.dotm 文件不会处理。",
        )
    })?;
    let canonical = fs::canonicalize(&requested).map_err(|error| {
        let code = if error.kind() == std::io::ErrorKind::NotFound {
            "DOCX_FILE_NOT_FOUND"
        } else {
            "DOCX_FILE_UNREADABLE"
        };
        CommandError::new(code, format!("无法访问所选 Word 文件：{error}"))
    })?;
    let metadata = fs::metadata(&canonical).map_err(|error| {
        CommandError::new(
            "DOCX_FILE_UNREADABLE",
            format!("无法读取所选 Word 文件信息：{error}"),
        )
    })?;
    if !metadata.is_file() {
        return Err(CommandError::new(
            "DOCX_SOURCE_NOT_FILE",
            "所选路径不是普通文件。",
        ));
    }
    Ok(SourcePath {
        display: display_path(&canonical),
        canonical,
        file_kind,
    })
}

fn validate_output_path(raw_path: &str) -> CommandResult<OutputPath> {
    let requested = absolute_path(raw_path, "DOCX_OUTPUT_PATH_INVALID", "导出文件")?;
    let file_kind = word_file_kind(&requested).ok_or_else(|| {
        CommandError::new(
            "DOCX_OUTPUT_TYPE_UNSUPPORTED",
            "导出文件名必须以 .docx 或 .dotx 结尾。",
        )
    })?;
    if path_entry_exists(&requested)? {
        return Err(CommandError::new(
            "DOCX_OUTPUT_ALREADY_EXISTS",
            "导出目标已经存在。首期为避免误覆盖，请选择新的文件名。",
        ));
    }
    let file_name = requested
        .file_name()
        .ok_or_else(|| CommandError::new("DOCX_OUTPUT_PATH_INVALID", "导出文件名不能为空。"))?;
    let parent = requested
        .parent()
        .ok_or_else(|| CommandError::new("DOCX_OUTPUT_PATH_INVALID", "导出目录无效。"))?;
    let canonical_parent = fs::canonicalize(parent).map_err(|error| {
        CommandError::new(
            "DOCX_OUTPUT_DIRECTORY_UNAVAILABLE",
            format!("导出目录不存在或无法访问：{error}"),
        )
    })?;
    if !canonical_parent.is_dir() {
        return Err(CommandError::new(
            "DOCX_OUTPUT_DIRECTORY_INVALID",
            "导出文件的上级路径不是文件夹。",
        ));
    }
    let normalized = canonical_parent.join(file_name);
    Ok(OutputPath {
        display: display_path(&normalized),
        normalized,
        file_kind,
    })
}

#[cfg(test)]
fn validate_package_extension(
    source: &SourcePath,
    output: &OutputPath,
    package_kind: PackageKind,
) -> CommandResult<()> {
    let expected = match package_kind {
        PackageKind::Document => WordFileKind::Document,
        PackageKind::Template => WordFileKind::Template,
        PackageKind::Unknown => {
            return Err(CommandError::new(
                "DOCX_PACKAGE_KIND_UNKNOWN",
                "无法确定模板是 DOCX 文档还是 DOTX 模板。",
            ));
        }
    };
    if source.file_kind != expected {
        return Err(CommandError::new(
            "DOCX_SOURCE_EXTENSION_MISMATCH",
            "源文件扩展名与包内 Word 内容类型不一致，为避免生成损坏文件已停止导出。",
        ));
    }
    if output.file_kind != expected {
        let extension = match expected {
            WordFileKind::Document => ".docx",
            WordFileKind::Template => ".dotx",
        };
        return Err(CommandError::new(
            "DOCX_OUTPUT_EXTENSION_MISMATCH",
            format!("当前模板必须导出为 {extension}，首期不会自动转换 DOCX/DOTX 类型。"),
        ));
    }
    Ok(())
}

fn absolute_path(raw_path: &str, code: &str, label: &str) -> CommandResult<PathBuf> {
    if raw_path.trim().is_empty() {
        return Err(CommandError::new(code, format!("{label}路径不能为空。")));
    }
    let path = PathBuf::from(raw_path);
    if !path.is_absolute() {
        return Err(CommandError::new(
            code,
            format!("{label}必须使用完整绝对路径。"),
        ));
    }
    Ok(path)
}

fn word_file_kind(path: &Path) -> Option<WordFileKind> {
    let extension = path.extension()?.to_str()?;
    if extension.eq_ignore_ascii_case("docx") {
        Some(WordFileKind::Document)
    } else if extension.eq_ignore_ascii_case("dotx") {
        Some(WordFileKind::Template)
    } else {
        None
    }
}

fn open_source(path: &Path) -> CommandResult<File> {
    File::open(path).map_err(|error| {
        CommandError::new(
            "DOCX_FILE_UNREADABLE",
            format!("无法打开所选 Word 文件：{error}"),
        )
    })
}

fn read_source_bounded(path: &Path, limit: u64) -> CommandResult<Vec<u8>> {
    let file = open_source(path)?;
    let declared_size = file
        .metadata()
        .map(|metadata| metadata.len())
        .map_err(|error| {
            CommandError::new(
                "DOCX_FILE_UNREADABLE",
                format!("无法读取 Word 文件大小：{error}"),
            )
        })?;
    if declared_size > limit {
        return Err(CommandError::new(
            "DOCX_FILE_TOO_LARGE",
            format!("Word 文件为 {declared_size} 字节，超过 {limit} 字节的安全上限。"),
        ));
    }
    let capacity = usize::try_from(declared_size).unwrap_or(0);
    let mut bytes = Vec::with_capacity(capacity);
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| {
            CommandError::new(
                "DOCX_FILE_READ_FAILED",
                format!("读取 Word 文件失败：{error}"),
            )
        })?;
    if bytes.len() as u64 > limit {
        return Err(CommandError::new(
            "DOCX_FILE_TOO_LARGE",
            format!("Word 文件读取结果超过 {limit} 字节的安全上限。"),
        ));
    }
    Ok(bytes)
}

fn temporary_output_path(output: &Path) -> CommandResult<PathBuf> {
    let parent = output
        .parent()
        .ok_or_else(|| CommandError::new("DOCX_OUTPUT_PATH_INVALID", "导出目录无效。"))?;
    let file_name = output
        .file_name()
        .ok_or_else(|| CommandError::new("DOCX_OUTPUT_PATH_INVALID", "导出文件名不能为空。"))?;
    Ok(parent.join(format!(
        ".{}.zhitiku-{}.tmp",
        file_name.to_string_lossy(),
        uuid::Uuid::now_v7()
    )))
}

fn path_entry_exists(path: &Path) -> CommandResult<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(CommandError::new(
            "DOCX_PATH_CHECK_FAILED",
            format!("无法检查路径是否已经存在：{error}"),
        )),
    }
}

fn remove_temporary_file(path: &Path) {
    let _ = fs::remove_file(path);
}

#[cfg(test)]
fn same_windows_path(left: &Path, right: &Path) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

fn display_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    if let Some(rest) = value.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = value.strip_prefix(r"\\?\") {
        rest.to_owned()
    } else {
        value.into_owned()
    }
}

fn package_kind_name(kind: PackageKind) -> &'static str {
    match kind {
        PackageKind::Document => "document",
        PackageKind::Template => "template",
        PackageKind::Unknown => "unknown",
    }
}

fn severity_name(severity: DiagnosticSeverity) -> &'static str {
    match severity {
        DiagnosticSeverity::Info => "info",
        DiagnosticSeverity::Warning => "warning",
        DiagnosticSeverity::Error => "error",
    }
}

fn saturating_u32(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_only_non_macro_word_extensions() {
        assert_eq!(
            word_file_kind(Path::new("paper.DOCX")),
            Some(WordFileKind::Document)
        );
        assert_eq!(
            word_file_kind(Path::new("template.dotx")),
            Some(WordFileKind::Template)
        );
        assert_eq!(word_file_kind(Path::new("macro.docm")), None);
        assert_eq!(word_file_kind(Path::new("archive.zip")), None);
    }

    #[test]
    fn compares_windows_paths_without_ascii_case_sensitivity() {
        assert!(same_windows_path(
            Path::new(r"C:\Papers\Exam.docx"),
            Path::new(r"c:\papers\EXAM.DOCX")
        ));
    }

    #[test]
    fn legacy_export_harness_rejects_empty_paths_before_writing() {
        let error = export(ExportDocxRequestApi {
            template_path: String::new(),
            output_path: String::new(),
            document_xml: None,
        })
        .unwrap_err();
        assert_eq!(error.code, "DOCX_SOURCE_PATH_INVALID");

        let error =
            export_managed_template_bytes(Vec::new(), String::new(), Vec::new()).unwrap_err();
        assert_eq!(error.code, "DOCX_OUTPUT_PATH_INVALID");
    }

    #[test]
    fn serializes_new_models_with_camel_case_fields() {
        let value = serde_json::to_value(AnalyzeDocxRequestApi {
            input_path: r"C:\paper.docx".to_owned(),
        })
        .unwrap();
        assert_eq!(value["inputPath"], r"C:\paper.docx");
        assert!(value.get("input_path").is_none());
    }

    /// Opt-in harness for locally supplied compatibility documents. Keeping
    /// the source files outside the repository avoids shipping real exams as
    /// test data while still exercising package validation, paragraph parsing
    /// and image extraction against the exact files that exposed a regression.
    #[test]
    #[ignore = "set ZHITIKU_WORD_IMPORT_FIXTURES to a platform path list"]
    fn prepares_external_word_import_regression_fixtures() {
        let fixture_list = std::env::var_os("ZHITIKU_WORD_IMPORT_FIXTURES")
            .expect("ZHITIKU_WORD_IMPORT_FIXTURES must contain DOCX paths");
        let paths = std::env::split_paths(&fixture_list).collect::<Vec<_>>();
        assert!(!paths.is_empty(), "at least one DOCX fixture is required");
        let analysis_output =
            std::env::var_os("ZHITIKU_WORD_IMPORT_ANALYSIS_DIR").map(PathBuf::from);
        if let Some(output) = &analysis_output {
            std::fs::create_dir_all(output)
                .unwrap_or_else(|error| panic!("{}: {error}", output.display()));
        }

        for (index, path) in paths.into_iter().enumerate() {
            let prepared = prepare_word_import(AnalyzeDocxRequestApi {
                input_path: path.to_string_lossy().into_owned(),
            })
            .unwrap_or_else(|error| panic!("{}: {}", path.display(), error.message));
            assert!(
                prepared.analysis.is_valid,
                "{} did not produce a valid analysis",
                path.display()
            );
            if let Some(output) = &analysis_output {
                let json = serde_json::to_vec(&prepared.analysis)
                    .expect("DOCX analysis must serialize for the bridge regression");
                std::fs::write(output.join(format!("analysis-{index}.json")), json)
                    .unwrap_or_else(|error| panic!("{}: {error}", output.display()));
            }
        }
    }

    #[test]
    #[ignore = "set ZHITIKU_MATHTYPE_DOCX to the supplied MathType exam path"]
    fn prepares_supplied_mathtype_exam_as_editable_formulas() {
        let path = std::env::var("ZHITIKU_MATHTYPE_DOCX")
            .expect("ZHITIKU_MATHTYPE_DOCX must contain a DOCX path");
        let prepared = prepare_word_import(AnalyzeDocxRequestApi { input_path: path })
            .unwrap_or_else(|error| panic!("{}", error.message));
        assert!(prepared.analysis.is_valid);
        assert_eq!(prepared.formulas.len(), 138);
        assert_eq!(
            prepared
                .formulas
                .iter()
                .filter(|formula| formula.source_kind == "mathtype_mtef5")
                .count(),
            135
        );
        let native_formulas = prepared
            .formulas
            .iter()
            .filter(|formula| formula.source_kind == "word_omml")
            .collect::<Vec<_>>();
        assert_eq!(native_formulas.len(), 3);
        assert_eq!(
            native_formulas
                .iter()
                .map(|formula| (
                    formula.paragraph_index,
                    formula.text_char_offset,
                    formula.latex.as_str(),
                ))
                .collect::<Vec<_>>(),
            vec![
                (57, 5, r"\triangle ABC"),
                (
                    62,
                    8,
                    r"\sin \alpha \cos \alpha ,\quad \sin \alpha -\cos \alpha",
                ),
                (93, 10, "A(8,-6)"),
            ]
        );
        assert_eq!(prepared.analysis.formula_count, 138);
        let five_point_table = prepared
            .tables
            .iter()
            .find(|table| {
                table.rows.len() == 2
                    && table.rows.iter().all(|row| row.len() == 6)
                    && table.rows[0][0].paragraph_indices == vec![68]
            })
            .expect("question 28 should retain its 2 x 6 five-point table");
        assert_eq!(
            five_point_table
                .rows
                .iter()
                .flat_map(|row| row.iter())
                .flat_map(|cell| cell.paragraph_indices.iter().copied())
                .collect::<Vec<_>>(),
            (68..=79).collect::<Vec<_>>()
        );
        assert_eq!(
            prepared
                .analysis
                .visible_text
                .chars()
                .filter(|character| *character == FORMULA_PLACEHOLDER)
                .count(),
            138
        );
        assert!(
            prepared
                .formulas
                .iter()
                .all(|formula| !formula.latex.is_empty() && !formula.latex.contains("{}"))
        );

        if let Some(output_path) = std::env::var_os("ZHITIKU_MATHTYPE_PREPARED_OUTPUT") {
            let images = prepared
                .images
                .iter()
                .enumerate()
                .map(|(index, image)| {
                    serde_json::json!({
                        "nodeId": format!("10000000-0000-4000-8000-{index:012x}"),
                        "resourceId": format!("20000000-0000-4000-8000-{index:012x}"),
                        "paragraphIndex": image.paragraph_index,
                        "textCharOffset": image.text_char_offset,
                        "originalFilename": image.original_filename,
                        "mimeType": image.mime_type,
                        "byteSize": image.byte_size,
                        "widthPx": image.width_px,
                        "heightPx": image.height_px,
                    })
                })
                .collect::<Vec<_>>();
            let formulas = prepared
                .formulas
                .iter()
                .enumerate()
                .map(|(index, formula)| {
                    serde_json::json!({
                        "nodeId": format!("30000000-0000-4000-8000-{index:012x}"),
                        "paragraphIndex": formula.paragraph_index,
                        "textCharOffset": formula.text_char_offset,
                        "latex": formula.latex,
                        "sourceKind": formula.source_kind,
                        "productVersion": formula.product_version,
                        "productSubversion": formula.product_subversion,
                    })
                })
                .collect::<Vec<_>>();
            let tables = prepared
                .tables
                .iter()
                .map(|table| {
                    serde_json::json!({
                        "tableIndex": table.index,
                        "rows": table.rows.iter().map(|row| {
                            row.iter().map(|cell| serde_json::json!({
                                "paragraphIndices": cell.paragraph_indices,
                            })).collect::<Vec<_>>()
                        }).collect::<Vec<_>>(),
                    })
                })
                .collect::<Vec<_>>();
            let regression_fixture = serde_json::json!({
                "analysis": &prepared.analysis,
                "images": images,
                "formulas": formulas,
                "tables": tables,
            });
            std::fs::write(
                PathBuf::from(output_path),
                serde_json::to_vec(&regression_fixture)
                    .expect("prepared Word fixture should serialize"),
            )
            .expect("prepared Word fixture should be written");
        }
    }
}
