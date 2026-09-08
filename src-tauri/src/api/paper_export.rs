use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
};

use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, Sqlite, SqlitePool, Transaction};
use uuid::Uuid;
use zip::{ZipWriter, write::SimpleFileOptions};

use crate::{
    db::{Database, DatabasePaths},
    docx::{
        DocxLimits, PackageKind, PaperContentMode, PaperImageRelationship, PaperPageSetupOverride,
        PaperRichDocument, PaperRichItem, TemplateStyleProfile,
        analyze_template_package_with_requirements, override_document_page_setup,
        parse_rich_content, render_rich_paper_document_xml_with_styles,
    },
};

use super::{
    docx,
    models::{
        CommandError, CommandResult, DocxDiagnosticApi, ExportPaperDocxRequestApi,
        PaperDocxExportResultApi, QuestionApi, RichContentApi,
    },
    resources,
    templates_core::{self, is_managed_template_file_name},
};

#[derive(Debug, FromRow)]
struct PaperExportRow {
    id: String,
    title: String,
    paper_status: String,
    export_content_mode: String,
    layout_json: Option<String>,
    row_version: i64,
}

#[derive(Debug, FromRow)]
struct PaperItemExportRow {
    position: i64,
    snapshot_json: String,
}

#[derive(Debug, FromRow)]
struct TemplateExportRow {
    id: String,
    name: String,
    file_rel_path: String,
    file_sha256: Vec<u8>,
    file_byte_size: i64,
    analysis_json: String,
}

#[derive(Clone, Debug, FromRow)]
struct ExportImageRow {
    id: String,
    sha256: Vec<u8>,
    mime_type: String,
    storage_rel_path: String,
    byte_size: i64,
    intrinsic_width_px: Option<i64>,
    intrinsic_height_px: Option<i64>,
    availability_status: String,
}

#[derive(Clone, Debug)]
struct WorkerImage {
    resource_id: String,
    mime_type: String,
    bytes: Vec<u8>,
    width_px: u32,
    height_px: u32,
}

#[derive(Debug)]
struct ExportSnapshot {
    paper: PaperExportRow,
    questions: Vec<QuestionApi>,
    template: TemplateExportRow,
    template_sha256: [u8; 32],
    template_style_profile: Option<TemplateStyleProfile>,
    page_setup_override: Option<PaperPageSetupOverride>,
    output_path: String,
    output_filename: String,
    mode: PaperContentMode,
    images: Vec<WorkerImage>,
    recovered_image_relationships: usize,
}

pub(crate) async fn export_paper_docx(
    database: &Database,
    request: ExportPaperDocxRequestApi,
    app_version: &str,
) -> CommandResult<PaperDocxExportResultApi> {
    validate_uuid(&request.paper_id, "试卷 ID")?;
    validate_uuid(&request.template_id, "模板 ID")?;
    if request.expected_paper_row_version < 1 {
        return Err(CommandError::validation("试卷版本号无效。"));
    }
    let (requested_output_path, output_filename) = validate_requested_output(&request.output_path)?;
    let snapshot = load_snapshot(
        database.pool(),
        database.paths(),
        &request,
        requested_output_path,
        output_filename,
    )
    .await?;
    let export_run_id = create_export_run(database.pool(), &snapshot, app_version).await?;

    let templates_dir = database.paths().templates_dir().to_path_buf();
    let worker_snapshot = snapshot_for_worker(&snapshot);
    let worker_result =
        tokio::task::spawn_blocking(move || perform_export(&templates_dir, &worker_snapshot))
            .await
            .map_err(|error| {
                CommandError::new(
                    "PAPER_EXPORT_TASK_FAILED",
                    format!("试卷 Word 导出任务未能正常完成：{error}"),
                )
            });

    let outcome = match worker_result {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(error)) => {
            mark_export_failed(database.pool(), &export_run_id, &error.code, &error.message)
                .await?;
            return Err(error);
        }
        Err(error) => {
            mark_export_failed(database.pool(), &export_run_id, &error.code, &error.message)
                .await?;
            return Err(error);
        }
    };

    if !outcome.exported {
        let (code, message) = first_error(&outcome.diagnostics);
        mark_export_failed(database.pool(), &export_run_id, code, message).await?;
        return Ok(result_from_outcome(&snapshot, Some(export_run_id), outcome));
    }

    let output_bytes = match outcome.output_bytes {
        Some(bytes) => bytes,
        None => {
            let error = CommandError::new(
                "PAPER_EXPORT_OUTPUT_METADATA_MISSING",
                "导出器报告成功，但没有返回文件大小。",
            );
            remove_committed_output(Path::new(&outcome.output_path));
            mark_export_failed(database.pool(), &export_run_id, &error.code, &error.message)
                .await?;
            return Err(error);
        }
    };
    let output_path = PathBuf::from(&outcome.output_path);
    let (actual_bytes, output_sha256) = match hash_verified_output(&output_path, output_bytes) {
        Ok(metadata) => metadata,
        Err(error) => {
            remove_committed_output(&output_path);
            mark_export_failed(database.pool(), &export_run_id, &error.code, &error.message)
                .await?;
            return Err(error);
        }
    };
    if let Err(error) = mark_export_succeeded(
        database.pool(),
        &export_run_id,
        &outcome.output_path,
        &snapshot.output_filename,
        actual_bytes,
        &output_sha256,
        app_version,
        &snapshot.paper.id,
    )
    .await
    {
        // A file must not be presented as a successfully recorded export if
        // the authoritative run row could not be committed.
        remove_committed_output(&output_path);
        let _ =
            mark_export_failed(database.pool(), &export_run_id, &error.code, &error.message).await;
        return Err(error);
    }

    Ok(result_from_outcome(&snapshot, Some(export_run_id), outcome))
}

pub(crate) async fn export_question_bank_docx(
    database: &Database,
    mut transaction: Transaction<'_, Sqlite>,
    mut questions: Vec<QuestionApi>,
    output_path: String,
) -> CommandResult<u64> {
    let mode = PaperContentMode::PaperAnswersExplanations;
    let (images, _recovered_image_relationships) =
        load_export_images(&mut transaction, database.paths(), &questions, mode).await?;
    transaction.commit().await.map_err(CommandError::database)?;

    for question in &mut questions {
        let type_name = question
            .question_type_name
            .clone()
            .unwrap_or_else(|| question.question_type.clone());
        let section = format!(
            "{} / {} / {}",
            question.subject_name, question.chapter_name, type_name
        );
        question.question_type = section.clone();
        question.question_type_name = Some(section);

        if !question.tags.is_empty() {
            let tags = question
                .tags
                .iter()
                .map(|tag| tag.name.trim())
                .filter(|name| !name.is_empty())
                .collect::<Vec<_>>()
                .join("、");
            if !tags.is_empty() {
                question
                    .stem
                    .html
                    .push_str(&format!("<p>标签：{}</p>", escape_export_html(&tags)));
                if !question.stem.plain_text.is_empty() {
                    question.stem.plain_text.push('\n');
                }
                question.stem.plain_text.push_str("标签：");
                question.stem.plain_text.push_str(&tags);
            }
        }

        clear_source_ooxml(question);
    }

    tokio::task::spawn_blocking(move || {
        let template_bytes = question_bank_template()?;
        let mut rich_paper =
            build_rich_paper("TK试题题库", &questions, mode).map_err(|diagnostics| {
                let (code, message) = first_error(&diagnostics);
                CommandError::new(code.to_owned(), message.to_owned())
            })?;
        rich_paper.omit_empty_field_placeholders = true;
        let package_parts =
            prepare_rich_package_parts(&template_bytes, &images).map_err(|error| {
                CommandError::new(
                    "QUESTION_BANK_DOCX_PACKAGE_PREPARATION_FAILED",
                    format!("Word 题库图片打包失败：{error}"),
                )
            })?;
        let limits = DocxLimits::default();
        let analysis = analyze_template_package_with_requirements(
            Cursor::new(&template_bytes),
            &["ZT_QUESTIONS"],
            &limits,
        )
        .map_err(|error| {
            CommandError::new(
                "QUESTION_BANK_DOCX_TEMPLATE_INVALID",
                format!("内置 Word 题库模板无法读取：{error}"),
            )
        })?;
        let template_document_xml = analysis.document_xml.ok_or_else(|| {
            CommandError::new(
                "QUESTION_BANK_DOCX_TEMPLATE_INVALID",
                "内置 Word 题库模板缺少正文。",
            )
        })?;
        let rendered = render_rich_paper_document_xml_with_styles(
            &template_document_xml,
            &rich_paper,
            &package_parts.image_relationships,
            None,
            &limits,
        )
        .map_err(|error| {
            CommandError::new(
                "QUESTION_BANK_DOCX_RENDER_FAILED",
                format!("Word 题库内容生成失败：{error}"),
            )
        })?;
        let exported = docx::export_managed_template_bytes_with_parts(
            template_bytes,
            output_path,
            rendered.document_xml,
            package_parts.replacements,
            package_parts.additions,
        )?;
        if !exported.exported {
            let (code, message) = first_error(&exported.diagnostics);
            return Err(CommandError::new(code.to_owned(), message.to_owned()));
        }
        exported.output_bytes.ok_or_else(|| {
            CommandError::new(
                "QUESTION_BANK_DOCX_OUTPUT_METADATA_MISSING",
                "Word 题库已生成，但没有返回文件大小。",
            )
        })
    })
    .await
    .map_err(|error| {
        CommandError::new(
            "QUESTION_BANK_DOCX_TASK_FAILED",
            format!("Word 题库导出任务未能正常完成：{error}"),
        )
    })?
}

fn clear_source_ooxml(question: &mut QuestionApi) {
    question.stem.source_ooxml = None;
    question.answer.source_ooxml = None;
    question.explanation.source_ooxml = None;
    for option in &mut question.options {
        option.content.source_ooxml = None;
    }
}

fn escape_export_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn question_bank_template() -> CommandResult<Vec<u8>> {
    const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/></Types>"#;
    const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    const DOCUMENT: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>{{ZT_QUESTIONS}}</w:t></w:r></w:p><w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1134" w:right="1134" w:bottom="1134" w:left="1134" w:header="708" w:footer="708" w:gutter="0"/></w:sectPr></w:body></w:document>"#;
    const DOCUMENT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdStyles" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#;
    const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="DengXian" w:eastAsia="等线" w:hAnsi="DengXian"/><w:sz w:val="22"/><w:szCs w:val="22"/></w:rPr></w:rPrDefault></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style></w:styles>"#;

    let cursor = Cursor::new(Vec::new());
    let mut writer = ZipWriter::new(cursor);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, bytes) in [
        ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
        ("_rels/.rels", ROOT_RELS.as_bytes()),
        ("word/document.xml", DOCUMENT.as_bytes()),
        ("word/_rels/document.xml.rels", DOCUMENT_RELS.as_bytes()),
        ("word/styles.xml", STYLES.as_bytes()),
    ] {
        writer.start_file(name, options).map_err(|error| {
            CommandError::new(
                "QUESTION_BANK_DOCX_TEMPLATE_BUILD_FAILED",
                format!("内置 Word 题库模板生成失败：{error}"),
            )
        })?;
        writer.write_all(bytes).map_err(|error| {
            CommandError::new(
                "QUESTION_BANK_DOCX_TEMPLATE_BUILD_FAILED",
                format!("内置 Word 题库模板写入失败：{error}"),
            )
        })?;
    }
    writer
        .finish()
        .map(|cursor| cursor.into_inner())
        .map_err(|error| {
            CommandError::new(
                "QUESTION_BANK_DOCX_TEMPLATE_BUILD_FAILED",
                format!("内置 Word 题库模板封装失败：{error}"),
            )
        })
}

#[derive(Debug)]
struct WorkerSnapshot {
    title: String,
    mode: PaperContentMode,
    questions: Vec<QuestionApi>,
    managed_template_name: String,
    template_byte_size: u64,
    template_sha256: [u8; 32],
    template_style_profile: Option<TemplateStyleProfile>,
    page_setup_override: Option<PaperPageSetupOverride>,
    output_path: String,
    images: Vec<WorkerImage>,
    recovered_image_relationships: usize,
}

#[derive(Debug)]
struct WorkerOutcome {
    exported: bool,
    output_path: String,
    output_bytes: Option<u64>,
    rewritten_parts: Vec<String>,
    diagnostics: Vec<DocxDiagnosticApi>,
}

fn snapshot_for_worker(snapshot: &ExportSnapshot) -> WorkerSnapshot {
    WorkerSnapshot {
        title: snapshot.paper.title.clone(),
        mode: snapshot.mode,
        questions: snapshot.questions.clone(),
        managed_template_name: snapshot.template.file_rel_path.clone(),
        template_byte_size: snapshot.template.file_byte_size as u64,
        template_sha256: snapshot.template_sha256,
        template_style_profile: snapshot.template_style_profile.clone(),
        page_setup_override: snapshot.page_setup_override,
        output_path: snapshot.output_path.clone(),
        images: snapshot.images.clone(),
        recovered_image_relationships: snapshot.recovered_image_relationships,
    }
}

fn perform_export(templates_dir: &Path, snapshot: &WorkerSnapshot) -> CommandResult<WorkerOutcome> {
    let rich_paper = match build_rich_paper(&snapshot.title, &snapshot.questions, snapshot.mode) {
        Ok(paper) => paper,
        Err(diagnostics) => return Ok(failed_worker(&snapshot.output_path, diagnostics)),
    };

    let limits = DocxLimits::default();
    let template_bytes = match templates_core::read_verified_managed_template(
        templates_dir,
        &snapshot.managed_template_name,
        snapshot.template_byte_size,
        &snapshot.template_sha256,
        &limits,
    ) {
        Ok(bytes) => bytes,
        Err(error) => {
            return Ok(failed_worker(
                &snapshot.output_path,
                vec![diagnostic(
                    error.code(),
                    format!("受管模板文件校验失败：{error}"),
                    Some("请在模板管理中删除该模板并重新导入。"),
                )],
            ));
        }
    };

    let analysis = match analyze_template_package_with_requirements(
        Cursor::new(&template_bytes),
        &["ZT_QUESTIONS"],
        &limits,
    ) {
        Ok(analysis) => analysis,
        Err(error) => {
            return Ok(failed_worker(
                &snapshot.output_path,
                docx::diagnostics_from_error(error),
            ));
        }
    };
    if !analysis.is_acceptable() || analysis.package_kind != PackageKind::Document {
        let mut diagnostics = analysis
            .diagnostics
            .iter()
            .map(DocxDiagnosticApi::from)
            .collect::<Vec<_>>();
        if analysis.package_kind != PackageKind::Document {
            diagnostics.push(diagnostic(
                "PAPER_EXPORT_TEMPLATE_KIND_UNSUPPORTED",
                "试卷导出只支持 DOCX 文档模板。",
                Some("请重新导入 .docx 格式模板。"),
            ));
        }
        return Ok(failed_worker(&snapshot.output_path, diagnostics));
    }
    let template_document_xml = analysis.document_xml.ok_or_else(|| {
        CommandError::new(
            "PAPER_EXPORT_TEMPLATE_DOCUMENT_MISSING",
            "模板安全分析通过，但没有读取到 word/document.xml。",
        )
    })?;

    let package_parts = match prepare_rich_package_parts(&template_bytes, &snapshot.images) {
        Ok(parts) => parts,
        Err(error) => {
            return Ok(failed_worker(
                &snapshot.output_path,
                vec![diagnostic(
                    "PAPER_EXPORT_PACKAGE_PREPARATION_FAILED",
                    error,
                    Some("请检查题目图片是否完整，然后重新导出。"),
                )],
            ));
        }
    };
    let rendered = match render_rich_paper_document_xml_with_styles(
        &template_document_xml,
        &rich_paper,
        &package_parts.image_relationships,
        snapshot.template_style_profile.as_ref(),
        &limits,
    ) {
        Ok(rendered) => rendered,
        Err(error) => {
            return Ok(failed_worker(
                &snapshot.output_path,
                docx::diagnostics_from_error(error),
            ));
        }
    };

    let rendered_document_xml = if let Some(page_setup) = snapshot.page_setup_override {
        match override_document_page_setup(&rendered.document_xml, page_setup, &limits) {
            Ok(document_xml) => document_xml,
            Err(error) => {
                return Ok(failed_worker(
                    &snapshot.output_path,
                    docx::diagnostics_from_error(error),
                ));
            }
        }
    } else {
        rendered.document_xml
    };
    let exported = docx::export_managed_template_bytes_with_parts(
        template_bytes,
        snapshot.output_path.clone(),
        rendered_document_xml,
        package_parts.replacements,
        package_parts.additions,
    )?;
    let mut diagnostics = rendered
        .diagnostics
        .iter()
        .map(DocxDiagnosticApi::from)
        .collect::<Vec<_>>();
    if snapshot.page_setup_override.is_some() {
        diagnostics.push(information_diagnostic(
            "PAPER_EXPORT_PAGE_SETUP_APPLIED",
            "已把排版页面中选择的纸张尺寸同步到 Word 文档。",
        ));
    }
    if snapshot.recovered_image_relationships > 0 {
        diagnostics.push(information_diagnostic(
            "PAPER_EXPORT_IMAGE_RELATIONSHIP_RECOVERED",
            format!(
                "已根据题目正文安全恢复 {} 个历史图片关联。",
                snapshot.recovered_image_relationships
            ),
        ));
    }
    diagnostics.extend(exported.diagnostics);
    Ok(WorkerOutcome {
        exported: exported.exported,
        output_path: exported.output_path,
        output_bytes: exported.output_bytes,
        rewritten_parts: exported.rewritten_parts,
        diagnostics,
    })
}

async fn load_snapshot(
    pool: &SqlitePool,
    paths: &DatabasePaths,
    request: &ExportPaperDocxRequestApi,
    output_path: String,
    output_filename: String,
) -> CommandResult<ExportSnapshot> {
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;
    let paper = sqlx::query_as::<_, PaperExportRow>(
        "SELECT id, title, paper_status, export_content_mode, layout_json, row_version \
         FROM papers WHERE id = ?",
    )
    .bind(&request.paper_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(CommandError::database)?
    .ok_or_else(|| CommandError::new("PAPER_NOT_FOUND", "这份试卷已不存在。"))?;
    if paper.row_version != request.expected_paper_row_version {
        return Err(CommandError::new(
            "PAPER_VERSION_CONFLICT",
            "试卷已在其他窗口中更新，请重新保存后再导出。",
        ));
    }
    if paper.paper_status != "saved" {
        return Err(CommandError::new(
            "PAPER_EXPORT_REQUIRES_SAVED_PAPER",
            "请先把当前试卷保存到历史试卷，再执行 Word 导出。",
        ));
    }
    let mode = PaperContentMode::parse(&paper.export_content_mode).ok_or_else(|| {
        CommandError::new(
            "PAPER_EXPORT_CONTENT_MODE_UNSUPPORTED",
            "试卷保存的导出内容模式不受支持。",
        )
    })?;

    let item_rows = sqlx::query_as::<_, PaperItemExportRow>(
        "SELECT position, snapshot_json FROM paper_items \
         WHERE paper_id = ? ORDER BY position, id",
    )
    .bind(&request.paper_id)
    .fetch_all(&mut *transaction)
    .await
    .map_err(CommandError::database)?;
    if item_rows.is_empty() {
        return Err(CommandError::new(
            "PAPER_EXPORT_EMPTY",
            "试卷没有题目，无法导出 Word 文件。",
        ));
    }
    let question_type_names =
        sqlx::query_as::<_, (String, String)>("SELECT code, name FROM question_types")
            .fetch_all(&mut *transaction)
            .await
            .map_err(CommandError::database)?
            .into_iter()
            .collect::<BTreeMap<_, _>>();
    let mut questions = Vec::with_capacity(item_rows.len());
    for (expected_position, item) in item_rows.into_iter().enumerate() {
        if item.position != expected_position as i64 {
            return Err(CommandError::new(
                "PAPER_ITEM_ORDER_CORRUPTED",
                "试卷题目顺序不连续，已停止导出。",
            ));
        }
        let mut question =
            serde_json::from_str::<QuestionApi>(&item.snapshot_json).map_err(|error| {
                CommandError::new(
                    "PAPER_SNAPSHOT_CORRUPTED",
                    format!("试卷题目快照无法读取：{error}"),
                )
            })?;
        let Some(question_type_name) = question_type_names.get(&question.question_type) else {
            return Err(CommandError::new(
                "PAPER_SNAPSHOT_CORRUPTED",
                "试卷题目快照包含不存在的题型。",
            ));
        };
        question.question_type_name = Some(question_type_name.clone());
        questions.push(question);
    }

    let template = sqlx::query_as::<_, TemplateExportRow>(
        "SELECT id, name, file_rel_path, file_sha256, file_byte_size, analysis_json \
         FROM word_templates WHERE id = ?",
    )
    .bind(&request.template_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(CommandError::database)?
    .ok_or_else(|| CommandError::new("TEMPLATE_NOT_FOUND", "所选模板已不存在。"))?;
    if !is_managed_template_file_name(&template.file_rel_path) {
        return Err(CommandError::new(
            "TEMPLATE_FILE_REFERENCE_UNSAFE",
            "模板内部文件引用不安全，已停止导出。",
        ));
    }
    let template_sha256: [u8; 32] = template.file_sha256.as_slice().try_into().map_err(|_| {
        CommandError::new(
            "TEMPLATE_HASH_CORRUPTED",
            "模板目录记录的 SHA-256 长度无效。",
        )
    })?;
    if template.file_byte_size < 0 {
        return Err(CommandError::new(
            "TEMPLATE_SIZE_CORRUPTED",
            "模板目录记录的文件大小无效。",
        ));
    }
    let template_style_profile = stored_template_style_profile(&template.analysis_json)?;
    let page_setup_override = stored_page_setup_override(paper.layout_json.as_deref())?;
    let (images, recovered_image_relationships) =
        load_export_images(&mut transaction, paths, &questions, mode).await?;
    transaction.commit().await.map_err(CommandError::database)?;

    Ok(ExportSnapshot {
        paper,
        questions,
        template,
        template_sha256,
        template_style_profile,
        page_setup_override,
        output_path,
        output_filename,
        mode,
        images,
        recovered_image_relationships,
    })
}

async fn load_export_images(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    paths: &DatabasePaths,
    questions: &[QuestionApi],
    mode: PaperContentMode,
) -> CommandResult<(Vec<WorkerImage>, usize)> {
    let (resource_ids, recovered_image_relationships) = export_image_resource_ids(questions, mode)?;
    let mut images = Vec::with_capacity(resource_ids.len());
    for resource_id in resource_ids {
        validate_uuid(&resource_id, "图片资源 ID")?;
        let row = sqlx::query_as::<_, ExportImageRow>(
            "SELECT id, sha256, mime_type, storage_rel_path, byte_size, intrinsic_width_px, \
                    intrinsic_height_px, availability_status FROM resources WHERE id = ?",
        )
        .bind(&resource_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(CommandError::database)?
        .ok_or_else(|| CommandError::new("RESOURCE_NOT_FOUND", "试卷引用的图片资源已经不存在。"))?;
        if row.id != resource_id
            || !matches!(row.mime_type.as_str(), "image/png" | "image/jpeg")
            || row.availability_status != "ready"
            || row.byte_size < 0
        {
            return Err(CommandError::new(
                "RESOURCE_NOT_READY",
                "试卷引用的图片资源当前不能安全导出。",
            ));
        }
        let width_px = u32::try_from(row.intrinsic_width_px.unwrap_or_default())
            .ok()
            .filter(|value| *value > 0)
            .ok_or_else(|| {
                CommandError::new("RESOURCE_DIMENSIONS_INVALID", "图片宽度记录无效。")
            })?;
        let height_px = u32::try_from(row.intrinsic_height_px.unwrap_or_default())
            .ok()
            .filter(|value| *value > 0)
            .ok_or_else(|| {
                CommandError::new("RESOURCE_DIMENSIONS_INVALID", "图片高度记录无效。")
            })?;
        let bytes = resources::read_managed_resource_file(
            paths.resources_dir(),
            &row.storage_rel_path,
            &row.sha256,
            u64::try_from(row.byte_size).unwrap_or(u64::MAX),
        )?;
        images.push(WorkerImage {
            resource_id,
            mime_type: row.mime_type,
            bytes,
            width_px,
            height_px,
        });
    }
    Ok((images, recovered_image_relationships))
}

fn export_image_resource_ids(
    questions: &[QuestionApi],
    mode: PaperContentMode,
) -> CommandResult<(BTreeSet<String>, usize)> {
    let referenced_resource_ids = questions
        .iter()
        .flat_map(|question| question.resource_refs.iter())
        .filter(|reference| included_slot(mode, &reference.content_slot))
        .map(|reference| reference.resource_id.clone())
        .collect::<BTreeSet<_>>();
    let mut resource_ids = referenced_resource_ids.clone();
    for question in questions {
        if mode.includes_questions() {
            resource_ids.extend(resources::managed_image_resource_ids(&question.stem)?);
            for option in &question.options {
                resource_ids.extend(resources::managed_image_resource_ids(&option.content)?);
            }
        }
        if mode.includes_answers() {
            resource_ids.extend(resources::managed_image_resource_ids(&question.answer)?);
        }
        if mode.includes_explanations() {
            resource_ids.extend(resources::managed_image_resource_ids(
                &question.explanation,
            )?);
        }
    }
    let recovered_image_relationships = resource_ids.difference(&referenced_resource_ids).count();
    Ok((resource_ids, recovered_image_relationships))
}

fn stored_template_style_profile(
    raw_analysis: &str,
) -> CommandResult<Option<TemplateStyleProfile>> {
    let analysis: Value = serde_json::from_str(raw_analysis).map_err(|error| {
        CommandError::new(
            "TEMPLATE_ANALYSIS_CORRUPTED",
            format!("模板分析记录无法读取：{error}"),
        )
    })?;
    let Some(value) = analysis
        .get("styleProfile")
        .or_else(|| analysis.get("style_profile"))
    else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let profile =
        serde_json::from_value::<TemplateStyleProfile>(value.clone()).map_err(|error| {
            CommandError::new(
                "TEMPLATE_STYLE_PROFILE_CORRUPTED",
                format!("模板格式原型无法读取：{error}"),
            )
        })?;
    if profile.schema_version != 1 {
        return Err(CommandError::new(
            "TEMPLATE_STYLE_PROFILE_VERSION_UNSUPPORTED",
            format!(
                "模板格式原型版本 {} 不受当前程序支持。",
                profile.schema_version
            ),
        ));
    }
    let total_style_bytes = profile
        .roles
        .values()
        .map(|prototype| prototype.paragraph_properties.len() + prototype.run_properties.len())
        .sum::<usize>();
    if total_style_bytes > 512 * 1024 {
        return Err(CommandError::new(
            "TEMPLATE_STYLE_PROFILE_TOO_LARGE",
            "模板格式原型数据异常过大，已停止导出。",
        ));
    }
    Ok(Some(profile))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredPaperLayout {
    #[serde(default)]
    page_setup: Option<StoredPaperPageSetup>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredPaperPageSetup {
    width_mm: f64,
    height_mm: f64,
}

fn stored_page_setup_override(
    raw_layout: Option<&str>,
) -> CommandResult<Option<PaperPageSetupOverride>> {
    let Some(raw_layout) = raw_layout else {
        return Ok(None);
    };
    let layout = serde_json::from_str::<StoredPaperLayout>(raw_layout).map_err(|error| {
        CommandError::new(
            "PAPER_LAYOUT_CORRUPTED",
            format!("试卷排版数据无法读取：{error}"),
        )
    })?;
    let Some(page) = layout.page_setup else {
        return Ok(None);
    };
    if !page.width_mm.is_finite()
        || !page.height_mm.is_finite()
        || !(90.0..=600.0).contains(&page.width_mm)
        || !(90.0..=600.0).contains(&page.height_mm)
    {
        return Err(CommandError::new(
            "PAPER_PAGE_SETUP_INVALID",
            "试卷保存的纸张宽度或高度无效，请在排版页面重新选择纸张大小。",
        ));
    }
    Ok(Some(PaperPageSetupOverride {
        width_twips: (page.width_mm / 25.4 * 1_440.0).round() as u32,
        height_twips: (page.height_mm / 25.4 * 1_440.0).round() as u32,
    }))
}

fn build_rich_paper(
    title: &str,
    questions: &[QuestionApi],
    mode: PaperContentMode,
) -> Result<PaperRichDocument, Vec<DocxDiagnosticApi>> {
    let mut issues = Vec::new();
    let mut items = Vec::with_capacity(questions.len());
    for (question_index, question) in questions.iter().enumerate() {
        let number = question_index + 1;
        let stem = if mode.includes_questions() {
            parse_export_content(&question.stem, &format!("第 {number} 题题干"), &mut issues)
        } else {
            Default::default()
        };
        let options = if mode.includes_questions() {
            question
                .options
                .iter()
                .enumerate()
                .map(|(option_index, option)| {
                    parse_export_content(
                        &option.content,
                        &format!("第 {number} 题选项 {}", option_index + 1),
                        &mut issues,
                    )
                })
                .collect()
        } else {
            Vec::new()
        };
        let answer = if mode.includes_answers() {
            parse_export_content(
                &question.answer,
                &format!("第 {number} 题答案"),
                &mut issues,
            )
        } else {
            Default::default()
        };
        let explanation = if mode.includes_explanations() {
            parse_export_content(
                &question.explanation,
                &format!("第 {number} 题解析"),
                &mut issues,
            )
        } else {
            Default::default()
        };
        items.push(PaperRichItem {
            question_type: question.question_type.clone(),
            question_type_label: question
                .question_type_name
                .clone()
                .unwrap_or_else(|| question.question_type.clone()),
            stem,
            options,
            answer,
            explanation,
        });
    }
    if !issues.is_empty() {
        let total = issues.len();
        let detail = issues.into_iter().take(20).collect::<Vec<_>>().join("；");
        return Err(vec![diagnostic(
            "PAPER_EXPORT_RICH_CONTENT_UNSUPPORTED",
            format!("本次试卷有 {total} 处内容暂时无法安全转换：{detail}"),
            Some("请在提示所列题目中删除不受支持的嵌套表格、外部图片或原始 OOXML 片段后重试。"),
        )]);
    }
    Ok(PaperRichDocument {
        title: title.to_owned(),
        mode,
        items,
        omit_empty_field_placeholders: false,
    })
}

fn parse_export_content(
    content: &RichContentApi,
    label: &str,
    issues: &mut Vec<String>,
) -> crate::docx::PaperRichContent {
    if content
        .source_ooxml
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        issues.push(format!("{label}含尚未结构化的原始 Word OOXML"));
    }
    let parsed = parse_rich_content(&content.html, &content.plain_text);
    issues.extend(
        parsed
            .unsupported
            .iter()
            .map(|message| format!("{label}：{message}")),
    );
    parsed.content
}

fn included_slot(mode: PaperContentMode, slot: &str) -> bool {
    match slot {
        "stem" | "option" => mode.includes_questions(),
        "answer" => mode.includes_answers(),
        "explanation" => mode.includes_explanations(),
        _ => false,
    }
}

#[derive(Debug, Default)]
struct RichPackageParts {
    image_relationships: BTreeMap<String, PaperImageRelationship>,
    replacements: BTreeMap<String, Vec<u8>>,
    additions: BTreeMap<String, Vec<u8>>,
}

fn prepare_rich_package_parts(
    template_bytes: &[u8],
    images: &[WorkerImage],
) -> Result<RichPackageParts, String> {
    if images.is_empty() {
        return Ok(RichPackageParts::default());
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(template_bytes))
        .map_err(|error| format!("无法读取模板包：{error}"))?;
    let source_names = (0..archive.len())
        .filter_map(|index| {
            archive
                .by_index_raw(index)
                .ok()
                .map(|file| file.name().to_owned())
        })
        .collect::<BTreeSet<_>>();
    let rels_name = "word/_rels/document.xml.rels";
    let rels = read_optional_zip_part(&mut archive, rels_name)?;
    let content_types_name = "[Content_Types].xml";
    let content_types = read_optional_zip_part(&mut archive, content_types_name)?
        .ok_or_else(|| "模板缺少 [Content_Types].xml".to_owned())?;
    drop(archive);

    let mut result = RichPackageParts::default();
    let mut relationships_xml = rels.unwrap_or_else(|| {
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"></Relationships>"#.to_vec()
    });
    let mut content_types_xml = content_types;
    let mut used_part_names = source_names;
    let mut used_relationships = String::from_utf8_lossy(&relationships_xml).into_owned();
    let mut sorted_images = images.to_vec();
    sorted_images.sort_by(|left, right| left.resource_id.cmp(&right.resource_id));
    sorted_images.dedup_by(|left, right| left.resource_id == right.resource_id);

    for (index, image) in sorted_images.iter().enumerate() {
        let extension = if image.mime_type == "image/png" {
            "png"
        } else {
            "jpg"
        };
        let mut suffix = index + 1;
        let part_name = loop {
            let candidate = format!("word/media/zhitiku-image-{suffix}.{extension}");
            if used_part_names.insert(candidate.clone()) {
                break candidate;
            }
            suffix += sorted_images.len().max(1);
        };
        let relationship_id = loop {
            let candidate = format!("rIdZhitikuImage{suffix}");
            if !used_relationships.contains(&format!("Id=\"{candidate}\""))
                && !used_relationships.contains(&format!("Id='{candidate}'"))
            {
                break candidate;
            }
            suffix += sorted_images.len().max(1);
        };
        let target = part_name.strip_prefix("word/").unwrap_or(&part_name);
        let relationship = format!(
            "<Relationship Id=\"{relationship_id}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"{target}\"/>"
        );
        insert_before_root_close(&mut relationships_xml, relationship.as_bytes())?;
        used_relationships.push_str(&relationship);
        result.additions.insert(part_name, image.bytes.clone());
        result.image_relationships.insert(
            image.resource_id.clone(),
            PaperImageRelationship {
                relationship_id,
                intrinsic_width_px: image.width_px,
                intrinsic_height_px: image.height_px,
            },
        );
        ensure_content_type_default(&mut content_types_xml, extension, &image.mime_type)?;
    }
    if rels_name_in_source(&used_part_names, rels_name) {
        result
            .replacements
            .insert(rels_name.to_owned(), relationships_xml);
    } else {
        result
            .additions
            .insert(rels_name.to_owned(), relationships_xml);
    }
    result
        .replacements
        .insert(content_types_name.to_owned(), content_types_xml);
    Ok(result)
}

fn read_optional_zip_part<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<Option<Vec<u8>>, String> {
    match archive.by_name(name) {
        Ok(mut file) => {
            let mut bytes = Vec::with_capacity(file.size().min(64 * 1024 * 1024) as usize);
            file.read_to_end(&mut bytes)
                .map_err(|error| format!("无法读取模板部件 {name}：{error}"))?;
            Ok(Some(bytes))
        }
        Err(zip::result::ZipError::FileNotFound) => Ok(None),
        Err(error) => Err(format!("无法读取模板部件 {name}：{error}")),
    }
}

fn rels_name_in_source(source_names: &BTreeSet<String>, name: &str) -> bool {
    source_names.contains(name)
}

fn insert_before_root_close(xml: &mut Vec<u8>, fragment: &[u8]) -> Result<(), String> {
    let text = std::str::from_utf8(xml).map_err(|_| "模板 XML 不是有效 UTF-8".to_owned())?;
    let position = text
        .rfind("</")
        .ok_or_else(|| "模板 XML 缺少根元素结束标签".to_owned())?;
    xml.splice(position..position, fragment.iter().copied());
    Ok(())
}

fn ensure_content_type_default(
    xml: &mut Vec<u8>,
    extension: &str,
    mime_type: &str,
) -> Result<(), String> {
    let lower = String::from_utf8_lossy(xml).to_ascii_lowercase();
    let double = format!("extension=\"{}\"", extension.to_ascii_lowercase());
    let single = format!("extension='{}'", extension.to_ascii_lowercase());
    if lower.contains(&double) || lower.contains(&single) {
        return Ok(());
    }
    insert_before_root_close(
        xml,
        format!("<Default Extension=\"{extension}\" ContentType=\"{mime_type}\"/>").as_bytes(),
    )
}

async fn create_export_run(
    pool: &SqlitePool,
    snapshot: &ExportSnapshot,
    app_version: &str,
) -> CommandResult<String> {
    if app_version.trim().is_empty() {
        return Err(CommandError::validation("应用版本号不能为空。"));
    }
    let id = Uuid::now_v7().to_string();
    sqlx::query(
        "INSERT INTO paper_export_runs \
         (id, paper_id, template_id, template_name_snapshot, template_sha256_snapshot, \
          content_mode, output_path, output_filename, status, started_at_ms) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'running', ?)",
    )
    .bind(&id)
    .bind(&snapshot.paper.id)
    .bind(&snapshot.template.id)
    .bind(&snapshot.template.name)
    .bind(snapshot.template_sha256.to_vec())
    .bind(&snapshot.paper.export_content_mode)
    .bind(&snapshot.output_path)
    .bind(&snapshot.output_filename)
    .bind(now_millis())
    .execute(pool)
    .await
    .map_err(CommandError::database)?;
    Ok(id)
}

async fn mark_export_failed(
    pool: &SqlitePool,
    id: &str,
    code: &str,
    message: &str,
) -> CommandResult<()> {
    let result = sqlx::query(
        "UPDATE paper_export_runs SET status = 'failed', error_code = ?, error_message = ?, \
         finished_at_ms = ? WHERE id = ? AND status = 'running'",
    )
    .bind(code)
    .bind(truncate_chars(message, 2_000))
    .bind(now_millis())
    .bind(id)
    .execute(pool)
    .await
    .map_err(CommandError::database)?;
    if result.rows_affected() != 1 {
        return Err(CommandError::new(
            "PAPER_EXPORT_RUN_STATE_CONFLICT",
            "导出失败，但导出记录状态无法安全更新。",
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn mark_export_succeeded(
    pool: &SqlitePool,
    id: &str,
    output_path: &str,
    output_filename: &str,
    output_byte_size: u64,
    output_sha256: &[u8; 32],
    app_version: &str,
    paper_id: &str,
) -> CommandResult<()> {
    let byte_size = i64::try_from(output_byte_size).map_err(|_| {
        CommandError::new(
            "PAPER_EXPORT_OUTPUT_TOO_LARGE",
            "导出文件大小超出数据库可记录范围。",
        )
    })?;
    let now = now_millis();
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;
    let result = sqlx::query(
        "UPDATE paper_export_runs SET status = 'succeeded', output_path = ?, output_filename = ?, \
         output_sha256 = ?, output_byte_size = ?, error_code = NULL, error_message = NULL, \
         finished_at_ms = ? WHERE id = ? AND status = 'running'",
    )
    .bind(output_path)
    .bind(output_filename)
    .bind(output_sha256.to_vec())
    .bind(byte_size)
    .bind(now)
    .bind(id)
    .execute(&mut *transaction)
    .await
    .map_err(CommandError::database)?;
    if result.rows_affected() != 1 {
        return Err(CommandError::new(
            "PAPER_EXPORT_RUN_STATE_CONFLICT",
            "导出文件已生成，但导出记录不是可提交的运行状态。",
        ));
    }
    sqlx::query(
        "INSERT INTO operation_logs \
         (id, occurred_at_ms, level, action, entity_type, entity_id, outcome, summary, \
          details_json, app_version) \
         VALUES (?, ?, 'info', 'paper.export.docx', 'paper', ?, 'success', ?, ?, ?)",
    )
    .bind(Uuid::now_v7().to_string())
    .bind(now)
    .bind(paper_id)
    .bind("试卷 Word 文件导出完成")
    .bind(
        json!({
            "exportRunId": id,
            "outputFilename": output_filename,
            "outputByteSize": output_byte_size,
        })
        .to_string(),
    )
    .bind(app_version)
    .execute(&mut *transaction)
    .await
    .map_err(CommandError::database)?;
    transaction.commit().await.map_err(CommandError::database)
}

fn hash_verified_output(path: &Path, expected_bytes: u64) -> CommandResult<(u64, [u8; 32])> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        CommandError::new(
            "PAPER_EXPORT_OUTPUT_VERIFY_FAILED",
            format!("无法读取导出文件信息：{error}"),
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(CommandError::new(
            "PAPER_EXPORT_OUTPUT_UNSAFE",
            "导出目标不是普通的非符号链接文件。",
        ));
    }
    if metadata.len() != expected_bytes {
        return Err(CommandError::new(
            "PAPER_EXPORT_OUTPUT_SIZE_MISMATCH",
            "导出文件大小与写入结果不一致。",
        ));
    }
    let mut file = File::open(path).map_err(|error| {
        CommandError::new(
            "PAPER_EXPORT_OUTPUT_VERIFY_FAILED",
            format!("无法重新打开导出文件：{error}"),
        )
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|error| {
            CommandError::new(
                "PAPER_EXPORT_OUTPUT_VERIFY_FAILED",
                format!("无法计算导出文件哈希：{error}"),
            )
        })?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok((metadata.len(), hasher.finalize().into()))
}

fn validate_requested_output(raw: &str) -> CommandResult<(String, String)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(CommandError::validation("导出文件路径不能为空。"));
    }
    let path = PathBuf::from(trimmed);
    if !path.is_absolute() {
        return Err(CommandError::validation("导出文件必须使用完整绝对路径。"));
    }
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("docx"))
    {
        return Err(CommandError::validation(
            "试卷导出文件名必须以 .docx 结尾。",
        ));
    }
    let filename = path
        .file_name()
        .and_then(|filename| filename.to_str())
        .filter(|filename| !filename.trim().is_empty())
        .ok_or_else(|| CommandError::validation("导出文件名必须是有效的 Unicode 文本。"))?
        .to_owned();
    Ok((trimmed.to_owned(), filename))
}

fn result_from_outcome(
    snapshot: &ExportSnapshot,
    export_run_id: Option<String>,
    outcome: WorkerOutcome,
) -> PaperDocxExportResultApi {
    PaperDocxExportResultApi {
        exported: outcome.exported,
        export_run_id,
        paper_id: snapshot.paper.id.clone(),
        paper_row_version: snapshot.paper.row_version,
        template_id: snapshot.template.id.clone(),
        template_name: snapshot.template.name.clone(),
        content_mode: snapshot.paper.export_content_mode.clone(),
        output_path: outcome.output_path,
        output_filename: snapshot.output_filename.clone(),
        output_bytes: outcome.output_bytes,
        rewritten_parts: outcome.rewritten_parts,
        diagnostics: outcome.diagnostics,
    }
}

fn failed_worker(output_path: &str, diagnostics: Vec<DocxDiagnosticApi>) -> WorkerOutcome {
    WorkerOutcome {
        exported: false,
        output_path: output_path.to_owned(),
        output_bytes: None,
        rewritten_parts: Vec::new(),
        diagnostics,
    }
}

fn diagnostic(
    code: &'static str,
    message: impl Into<String>,
    suggested_action: Option<&str>,
) -> DocxDiagnosticApi {
    DocxDiagnosticApi {
        severity: "error".to_owned(),
        code: code.to_owned(),
        part_name: None,
        message: message.into(),
        suggested_action: suggested_action.map(str::to_owned),
    }
}

fn information_diagnostic(code: &'static str, message: impl Into<String>) -> DocxDiagnosticApi {
    DocxDiagnosticApi {
        severity: "info".to_owned(),
        code: code.to_owned(),
        part_name: Some("word/document.xml".to_owned()),
        message: message.into(),
        suggested_action: None,
    }
}

fn first_error(diagnostics: &[DocxDiagnosticApi]) -> (&str, &str) {
    diagnostics
        .iter()
        .find(|diagnostic| diagnostic.severity == "error")
        .or_else(|| diagnostics.first())
        .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_str()))
        .unwrap_or(("PAPER_EXPORT_FAILED", "试卷 Word 导出未完成。"))
}

fn validate_uuid(value: &str, label: &str) -> CommandResult<()> {
    Uuid::parse_str(value)
        .map(|_| ())
        .map_err(|_| CommandError::validation(format!("{label} 无效。")))
}

fn remove_committed_output(path: &Path) {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file()) {
        let _ = fs::remove_file(path);
    }
}

fn truncate_chars(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

fn now_millis() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::{QuestionOptionApi, TagApi};
    use crate::docx::{
        ParagraphStylePrototype, inspect_docx, minimal_docx,
        raw_copy_with_replacements_and_additions,
    };
    use std::collections::BTreeMap;
    use std::io::Cursor;

    fn rich(html: &str, plain_text: &str) -> RichContentApi {
        RichContentApi {
            schema_version: 1,
            editor: None,
            editor_version: None,
            document: None,
            html: html.to_owned(),
            plain_text: plain_text.to_owned(),
            source_ooxml: None,
        }
    }

    fn question() -> QuestionApi {
        QuestionApi {
            id: Uuid::now_v7().to_string(),
            question_type: "single_choice".to_owned(),
            question_type_name: Some("单选题".to_owned()),
            stem: rich("<p>题目</p>", "题目"),
            options: vec![QuestionOptionApi {
                id: Uuid::now_v7().to_string(),
                position: 0,
                content: rich("<p>A</p>", "A"),
            }],
            answer: rich("<p>A</p>", "A"),
            explanation: rich("<p>解析</p>", "解析"),
            subject_id: Uuid::now_v7().to_string(),
            chapter_id: Uuid::now_v7().to_string(),
            subject_name: "数学".to_owned(),
            chapter_name: "第一章".to_owned(),
            tags: Vec::<TagApi>::new(),
            resource_refs: Vec::new(),
            last_used_at: None,
            deleted_at: None,
            created_at: 0,
            updated_at: 0,
            content_version: 1,
        }
    }

    #[test]
    fn rich_content_validation_is_limited_to_slots_in_the_selected_mode() {
        let mut item = question();
        item.answer.source_ooxml = Some("<m:oMath/>".to_owned());
        assert!(build_rich_paper("试卷", &[item.clone()], PaperContentMode::PaperOnly).is_ok());
        assert!(build_rich_paper("试卷", &[item], PaperContentMode::PaperAndAnswers).is_err());
    }

    #[test]
    fn built_in_question_bank_template_is_a_safe_docx_with_question_anchor() {
        let bytes = question_bank_template().unwrap();
        let inspection = inspect_docx(Cursor::new(&bytes), &DocxLimits::default()).unwrap();
        assert!(inspection.is_acceptable());
        assert_eq!(inspection.package_kind, PackageKind::Document);

        let analysis = analyze_template_package_with_requirements(
            Cursor::new(bytes),
            &["ZT_QUESTIONS"],
            &DocxLimits::default(),
        )
        .unwrap();
        assert!(analysis.is_acceptable());
    }

    #[test]
    fn rich_content_validation_accepts_native_tables() {
        let mut item = question();
        item.stem.html = "<table><tr><td>1</td></tr></table>".to_owned();
        let paper = build_rich_paper("试卷", &[item], PaperContentMode::PaperOnly)
            .expect("native table should be supported");
        assert!(matches!(
            paper.items[0].stem.blocks[0],
            crate::docx::PaperBlock::Table(_)
        ));
    }

    #[test]
    fn managed_images_add_relationship_media_and_content_type_parts() {
        let source = minimal_docx(
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>{{ZT_QUESTIONS}}</w:t></w:r></w:p></w:body></w:document>"#,
        );
        let image = WorkerImage {
            resource_id: Uuid::now_v7().to_string(),
            mime_type: "image/png".to_owned(),
            bytes: vec![0x89, b'P', b'N', b'G'],
            width_px: 320,
            height_px: 200,
        };
        let parts = prepare_rich_package_parts(&source, std::slice::from_ref(&image)).unwrap();
        let relationship = parts.image_relationships.get(&image.resource_id).unwrap();
        assert!(relationship.relationship_id.starts_with("rIdZhitikuImage"));
        assert!(parts.additions.keys().any(|name| name.ends_with(".png")));

        let exported = raw_copy_with_replacements_and_additions(
            Cursor::new(source),
            Cursor::new(Vec::new()),
            &parts.replacements,
            &parts.additions,
            &DocxLimits::default(),
        )
        .unwrap();
        let inspection = inspect_docx(
            Cursor::new(exported.output.into_inner()),
            &DocxLimits::default(),
        )
        .unwrap();
        assert!(inspection.is_acceptable());
    }

    #[test]
    fn export_recovers_a_managed_image_id_from_legacy_snapshot_html() {
        let resource_id = Uuid::now_v7().to_string();
        let mut item = question();
        item.stem.html =
            format!(r#"<p>题目<img data-resource-id="{resource_id}" width="458"></p>"#);
        item.resource_refs.clear();

        let (resource_ids, recovered) =
            export_image_resource_ids(&[item], PaperContentMode::PaperOnly).unwrap();
        assert_eq!(resource_ids, BTreeSet::from([resource_id]));
        assert_eq!(recovered, 1);
    }

    #[test]
    fn unmanaged_images_are_reported_with_question_location() {
        let mut item = question();
        item.stem.html = r#"<p><img src="https://example.invalid/a.png"></p>"#.to_owned();
        let diagnostics = build_rich_paper("试卷", &[item], PaperContentMode::PaperOnly)
            .expect_err("external image must be rejected");
        assert!(diagnostics[0].message.contains("第 1 题题干"));
    }

    #[test]
    fn requested_output_requires_an_absolute_docx_path() {
        assert!(validate_requested_output("paper.docx").is_err());
        assert!(validate_requested_output("C:\\exports\\paper.pdf").is_err());
        if cfg!(windows) {
            let (_, filename) = validate_requested_output("C:\\exports\\试卷.docx").unwrap();
            assert_eq!(filename, "试卷.docx");
        }
    }

    #[test]
    fn stored_template_style_profile_round_trips_into_the_export_worker() {
        let profile = TemplateStyleProfile {
            schema_version: 1,
            source_range: None,
            roles: BTreeMap::from([(
                "question".to_owned(),
                ParagraphStylePrototype {
                    source_paragraph_index: 8,
                    paragraph_properties: br#"<w:pPr><w:ind w:firstLine="420"/></w:pPr>"#.to_vec(),
                    run_properties: br#"<w:rPr><w:sz w:val="28"/></w:rPr>"#.to_vec(),
                },
            )]),
        };
        let analysis = json!({ "styleProfile": profile.clone() });
        let decoded = stored_template_style_profile(&analysis.to_string())
            .unwrap()
            .expect("profile should exist");
        assert_eq!(decoded, profile);
    }

    #[test]
    fn stored_template_style_profile_rejects_unknown_versions() {
        let analysis = json!({
            "styleProfile": {
                "schemaVersion": 99,
                "sourceRange": null,
                "roles": {}
            }
        });
        let error = stored_template_style_profile(&analysis.to_string()).unwrap_err();
        assert_eq!(error.code, "TEMPLATE_STYLE_PROFILE_VERSION_UNSUPPORTED");
    }

    #[test]
    fn stored_layout_page_size_converts_millimetres_to_twips() {
        let layout = json!({
            "schemaVersion": 1,
            "pageSetup": {
                "widthMm": 420.0,
                "heightMm": 297.0
            }
        });
        let page = stored_page_setup_override(Some(&layout.to_string()))
            .unwrap()
            .expect("page setup should exist");

        assert_eq!(page.width_twips, 23_811);
        assert_eq!(page.height_twips, 16_838);
    }

    #[test]
    fn stored_layout_without_explicit_page_size_keeps_the_template_size() {
        let layout = json!({ "schemaVersion": 1, "pageSetup": null });
        assert!(
            stored_page_setup_override(Some(&layout.to_string()))
                .unwrap()
                .is_none()
        );
    }
}
