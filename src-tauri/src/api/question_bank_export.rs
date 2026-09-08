use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use uuid::Uuid;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::db::Database;

use super::{
    models::{CommandError, CommandResult, QuestionApi, QuestionBankExportResultApi},
    paper_export, questions,
};

const MAX_XLSX_BYTES: u64 = 512 * 1024 * 1024;
const MAX_EXCEL_ROWS: usize = 1_048_576;
const MAX_EXCEL_CELL_CHARS: usize = 32_767;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum QuestionBankExportFormat {
    Docx,
    Xlsx,
}

impl QuestionBankExportFormat {
    fn parse(value: &str) -> CommandResult<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "docx" => Ok(Self::Docx),
            "xlsx" => Ok(Self::Xlsx),
            _ => Err(CommandError::validation(
                "题库只支持导出为 Word（.docx）或 Excel（.xlsx）文件。",
            )),
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Docx => "docx",
            Self::Xlsx => "xlsx",
        }
    }
}

pub(crate) async fn export_question_bank(
    database: &Database,
    format: &str,
    output_path: String,
) -> CommandResult<QuestionBankExportResultApi> {
    let format = QuestionBankExportFormat::parse(format)?;
    let output = validate_output_path(&output_path, format)?;
    let output_filename = output
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| CommandError::validation("导出文件名包含无法处理的字符。"))?
        .to_owned();
    let mut transaction = database
        .pool()
        .begin()
        .await
        .map_err(CommandError::database)?;
    let questions = questions::list_active_questions_for_export(&mut transaction).await?;
    if questions.is_empty() {
        return Err(CommandError::validation(
            "当前题库没有可导出的题目；回收站中的题目不会导出。",
        ));
    }
    if questions.len() + 1 > MAX_EXCEL_ROWS && format == QuestionBankExportFormat::Xlsx {
        return Err(CommandError::new(
            "QUESTION_BANK_EXPORT_TOO_MANY_ROWS",
            "题目数量超过单个 Excel 工作表的行数上限。",
        ));
    }

    let question_count = u32::try_from(questions.len()).map_err(|_| {
        CommandError::new(
            "QUESTION_BANK_EXPORT_COUNT_OVERFLOW",
            "题目数量超过导出结果能够准确记录的上限。",
        )
    })?;
    let output_bytes = match format {
        QuestionBankExportFormat::Docx => {
            paper_export::export_question_bank_docx(
                database,
                transaction,
                questions,
                output.to_string_lossy().into_owned(),
            )
            .await?
        }
        QuestionBankExportFormat::Xlsx => {
            transaction.commit().await.map_err(CommandError::database)?;
            let worker_output = output.clone();
            tokio::task::spawn_blocking(move || export_xlsx(&worker_output, &questions))
                .await
                .map_err(|error| {
                    CommandError::new(
                        "QUESTION_BANK_XLSX_TASK_FAILED",
                        format!("Excel 导出任务未能正常完成：{error}"),
                    )
                })??
        }
    };

    Ok(QuestionBankExportResultApi {
        format: format.extension().to_owned(),
        output_path: output.to_string_lossy().into_owned(),
        output_filename,
        output_bytes,
        question_count,
    })
}

fn validate_output_path(raw: &str, format: QuestionBankExportFormat) -> CommandResult<PathBuf> {
    let requested = PathBuf::from(raw.trim());
    if raw.trim().is_empty() || !requested.is_absolute() {
        return Err(CommandError::validation("导出文件必须使用非空的绝对路径。"));
    }
    if !requested
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(format.extension()))
    {
        return Err(CommandError::validation(format!(
            "所选导出文件必须以 .{} 结尾。",
            format.extension()
        )));
    }
    let file_name = requested
        .file_name()
        .ok_or_else(|| CommandError::validation("导出路径缺少文件名。"))?;
    let parent = requested
        .parent()
        .ok_or_else(|| CommandError::validation("导出路径缺少保存目录。"))?;
    let parent = fs::canonicalize(parent).map_err(|error| {
        CommandError::new(
            "QUESTION_BANK_EXPORT_DIRECTORY_UNAVAILABLE",
            format!("导出目录不存在或无法访问：{error}"),
        )
    })?;
    let metadata = fs::symlink_metadata(&parent).map_err(|error| {
        CommandError::new(
            "QUESTION_BANK_EXPORT_DIRECTORY_UNAVAILABLE",
            format!("无法检查导出目录：{error}"),
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(CommandError::new(
            "QUESTION_BANK_EXPORT_DIRECTORY_UNSAFE",
            "所选导出位置不是安全的真实目录。",
        ));
    }
    let normalized = parent.join(file_name);
    if fs::symlink_metadata(&normalized).is_ok() {
        return Err(CommandError::new(
            "QUESTION_BANK_EXPORT_OUTPUT_EXISTS",
            "导出目标已经存在。为避免误覆盖，请换一个文件名。",
        ));
    }
    Ok(normalized)
}

fn export_xlsx(output: &Path, questions: &[QuestionApi]) -> CommandResult<u64> {
    let parent = output
        .parent()
        .ok_or_else(|| CommandError::validation("Excel 导出路径缺少保存目录。"))?;
    let file_name = output
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("question-bank.xlsx");
    let temporary = parent.join(format!(".{file_name}.{}.tmp", Uuid::now_v7()));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| {
            CommandError::new(
                "QUESTION_BANK_XLSX_TEMP_CREATE_FAILED",
                format!("无法在导出目录创建 Excel 临时文件：{error}"),
            )
        })?;
    let output_bytes = match write_xlsx_archive(file, questions, MAX_XLSX_BYTES) {
        Ok(bytes) => bytes,
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
    };

    if let Err(error) = verify_xlsx(&temporary) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    if fs::symlink_metadata(output).is_ok() {
        let _ = fs::remove_file(&temporary);
        return Err(CommandError::new(
            "QUESTION_BANK_EXPORT_OUTPUT_EXISTS",
            "导出期间目标文件已经出现；为避免覆盖，已停止保存。",
        ));
    }
    fs::rename(&temporary, output).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        CommandError::new(
            "QUESTION_BANK_XLSX_COMMIT_FAILED",
            format!("Excel 临时文件无法保存到最终位置：{error}"),
        )
    })?;
    Ok(output_bytes)
}

fn xlsx_too_large() -> CommandError {
    CommandError::new(
        "QUESTION_BANK_XLSX_TOO_LARGE",
        "生成的 Excel 文件超过 512 MB 安全上限。",
    )
}

fn ensure_xlsx_size(writer: &ZipWriter<File>, limit: u64) -> CommandResult<()> {
    let file = writer.get_ref().ok_or_else(|| {
        CommandError::new(
            "QUESTION_BANK_XLSX_BUILD_FAILED",
            "Excel 写入器已经意外关闭。",
        )
    })?;
    let bytes = file
        .metadata()
        .map_err(|error| {
            CommandError::new(
                "QUESTION_BANK_XLSX_BUILD_FAILED",
                format!("无法检查 Excel 临时文件大小：{error}"),
            )
        })?
        .len();
    if bytes > limit {
        return Err(xlsx_too_large());
    }
    Ok(())
}

fn write_xlsx_chunk(writer: &mut ZipWriter<File>, content: &[u8], limit: u64) -> CommandResult<()> {
    writer.write_all(content).map_err(|error| {
        CommandError::new(
            "QUESTION_BANK_XLSX_WRITE_FAILED",
            format!("Excel 内容写入磁盘失败：{error}"),
        )
    })?;
    ensure_xlsx_size(writer, limit)
}

fn write_xlsx_part(
    writer: &mut ZipWriter<File>,
    options: SimpleFileOptions,
    name: &str,
    content: &[u8],
    limit: u64,
) -> CommandResult<()> {
    writer.start_file(name, options).map_err(xlsx_zip_error)?;
    write_xlsx_chunk(writer, content, limit)
}

pub(super) fn write_xlsx_archive(
    file: File,
    questions: &[QuestionApi],
    limit: u64,
) -> CommandResult<u64> {
    let headers = [
        "序号",
        "题型",
        "学科",
        "章节",
        "题干",
        "选项",
        "答案",
        "解析",
        "标签",
        "最近使用",
        "创建时间",
        "更新时间",
        "题目ID",
    ];
    let last_row = questions.len() + 1;

    const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/></Types>"#;
    const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/></Relationships>"#;
    const WORKBOOK: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><bookViews><workbookView/></bookViews><sheets><sheet name="题库" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
    const WORKBOOK_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#;
    const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="2"><font><sz val="11"/><name val="等线"/></font><font><b/><color rgb="FFFFFFFF"/><sz val="11"/><name val="等线"/></font></fonts><fills count="3"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="solid"><fgColor rgb="FF2563EB"/><bgColor indexed="64"/></patternFill></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="3"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="0" fontId="1" fillId="2" borderId="0" xfId="0" applyFont="1" applyFill="1" applyAlignment="1"><alignment vertical="center"/></xf><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0" applyAlignment="1"><alignment vertical="top" wrapText="1"/></xf></cellXfs><cellStyles count="1"><cellStyle name="常规" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#;
    const CORE: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><dc:title>TK试题题库导出</dc:title><dc:creator>TK试题题库</dc:creator></cp:coreProperties>"#;
    const APP: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Application>TK试题题库</Application></Properties>"#;

    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, content) in [
        ("[Content_Types].xml", CONTENT_TYPES.as_bytes()),
        ("_rels/.rels", ROOT_RELS.as_bytes()),
        ("xl/workbook.xml", WORKBOOK.as_bytes()),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS.as_bytes()),
        ("xl/styles.xml", STYLES.as_bytes()),
        ("docProps/core.xml", CORE.as_bytes()),
        ("docProps/app.xml", APP.as_bytes()),
    ] {
        write_xlsx_part(&mut writer, options, name, content, limit)?;
    }

    writer
        .start_file("xl/worksheets/sheet1.xml", options)
        .map_err(xlsx_zip_error)?;
    let prefix = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
         <worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">\n\
         <dimension ref=\"A1:M{last_row}\"/><sheetViews><sheetView workbookViewId=\"0\"><pane ySplit=\"1\" topLeftCell=\"A2\" activePane=\"bottomLeft\" state=\"frozen\"/></sheetView></sheetViews>\n\
         <cols><col min=\"1\" max=\"1\" width=\"8\" customWidth=\"1\"/><col min=\"2\" max=\"4\" width=\"18\" customWidth=\"1\"/><col min=\"5\" max=\"8\" width=\"42\" customWidth=\"1\"/><col min=\"9\" max=\"9\" width=\"22\" customWidth=\"1\"/><col min=\"10\" max=\"12\" width=\"20\" customWidth=\"1\"/><col min=\"13\" max=\"13\" width=\"38\" customWidth=\"1\"/></cols><sheetData>"
    );
    write_xlsx_chunk(&mut writer, prefix.as_bytes(), limit)?;

    let mut row_xml = String::from("<row r=\"1\" ht=\"24\" customHeight=\"1\">");
    for (column, header) in headers.iter().enumerate() {
        push_inline_cell(&mut row_xml, column + 1, 1, header, 1);
    }
    row_xml.push_str("</row>");
    write_xlsx_chunk(&mut writer, row_xml.as_bytes(), limit)?;

    for (index, question) in questions.iter().enumerate() {
        let row = index + 2;
        let option_text = question
            .options
            .iter()
            .enumerate()
            .map(|(option_index, option)| {
                format!(
                    "{}. {}",
                    option_label(option_index),
                    option.content.plain_text.trim()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let tags = question
            .tags
            .iter()
            .map(|tag| tag.name.as_str())
            .collect::<Vec<_>>()
            .join("、");
        let values = [
            (index + 1).to_string(),
            question
                .question_type_name
                .clone()
                .unwrap_or_else(|| question.question_type.clone()),
            question.subject_name.clone(),
            question.chapter_name.clone(),
            question.stem.plain_text.trim().to_owned(),
            option_text,
            question.answer.plain_text.trim().to_owned(),
            question.explanation.plain_text.trim().to_owned(),
            tags,
            question
                .last_used_at
                .map(format_unix_millis)
                .unwrap_or_default(),
            format_unix_millis(question.created_at),
            format_unix_millis(question.updated_at),
            question.id.clone(),
        ];
        row_xml.clear();
        row_xml.push_str(&format!("<row r=\"{row}\">"));
        for (column, value) in values.iter().enumerate() {
            push_inline_cell(&mut row_xml, column + 1, row, value, 2);
        }
        row_xml.push_str("</row>");
        write_xlsx_chunk(&mut writer, row_xml.as_bytes(), limit)?;
    }
    let suffix = format!(
        "</sheetData><autoFilter ref=\"A1:M{last_row}\"/><pageMargins left=\"0.3\" right=\"0.3\" top=\"0.5\" bottom=\"0.5\" header=\"0.2\" footer=\"0.2\"/></worksheet>"
    );
    write_xlsx_chunk(&mut writer, suffix.as_bytes(), limit)?;

    let file = writer.finish().map_err(xlsx_zip_error)?;
    file.sync_all().map_err(|error| {
        CommandError::new(
            "QUESTION_BANK_XLSX_WRITE_FAILED",
            format!("Excel 临时文件无法同步到磁盘：{error}"),
        )
    })?;
    let output_bytes = file
        .metadata()
        .map_err(|error| {
            CommandError::new(
                "QUESTION_BANK_XLSX_WRITE_FAILED",
                format!("无法读取 Excel 临时文件大小：{error}"),
            )
        })?
        .len();
    if output_bytes > limit {
        return Err(xlsx_too_large());
    }
    Ok(output_bytes)
}

fn verify_xlsx(path: &Path) -> CommandResult<()> {
    let file = File::open(path).map_err(|error| {
        CommandError::new(
            "QUESTION_BANK_XLSX_VERIFY_READ_FAILED",
            format!("无法重新打开 Excel 临时文件进行校验：{error}"),
        )
    })?;
    let mut archive = ZipArchive::new(file).map_err(xlsx_zip_error)?;
    for name in [
        "[Content_Types].xml",
        "_rels/.rels",
        "xl/workbook.xml",
        "xl/_rels/workbook.xml.rels",
        "xl/styles.xml",
        "xl/worksheets/sheet1.xml",
    ] {
        let mut part = archive.by_name(name).map_err(|error| {
            CommandError::new(
                "QUESTION_BANK_XLSX_VERIFY_FAILED",
                format!("生成的 Excel 缺少必要部件 {name}：{error}"),
            )
        })?;
        if part.size() == 0 {
            return Err(CommandError::new(
                "QUESTION_BANK_XLSX_VERIFY_FAILED",
                format!("生成的 Excel 部件 {name} 为空。"),
            ));
        }
        std::io::copy(&mut part, &mut std::io::sink()).map_err(|error| {
            CommandError::new(
                "QUESTION_BANK_XLSX_VERIFY_FAILED",
                format!("无法校验 Excel 部件 {name}：{error}"),
            )
        })?;
    }
    Ok(())
}

fn xlsx_zip_error(error: zip::result::ZipError) -> CommandError {
    CommandError::new(
        "QUESTION_BANK_XLSX_BUILD_FAILED",
        format!("Excel 文件包生成失败：{error}"),
    )
}

fn push_inline_cell(output: &mut String, column: usize, row: usize, value: &str, style: usize) {
    let reference = format!("{}{}", column_label(column), row);
    let value = truncate_excel_cell(value);
    output.push_str(&format!(
        "<c r=\"{reference}\" t=\"inlineStr\" s=\"{style}\"><is><t xml:space=\"preserve\">{}</t></is></c>",
        escape_xml(&value)
    ));
}

fn column_label(mut column: usize) -> String {
    let mut label = String::new();
    while column > 0 {
        column -= 1;
        label.push(char::from(b'A' + (column % 26) as u8));
        column /= 26;
    }
    label.chars().rev().collect()
}

fn option_label(index: usize) -> String {
    if index < 26 {
        char::from(b'A' + index as u8).to_string()
    } else {
        (index + 1).to_string()
    }
}

fn truncate_excel_cell(value: &str) -> String {
    let sanitized = sanitize_xml_text(value);
    if sanitized.chars().count() <= MAX_EXCEL_CELL_CHARS {
        return sanitized;
    }
    let suffix = "…（内容已截断）";
    let keep = MAX_EXCEL_CELL_CHARS.saturating_sub(suffix.chars().count());
    let mut result = sanitized.chars().take(keep).collect::<String>();
    result.push_str(suffix);
    result
}

fn sanitize_xml_text(value: &str) -> String {
    value
        .chars()
        .filter(|character| {
            matches!(*character, '\t' | '\n' | '\r')
                || (*character >= '\u{20}' && *character != '\u{fffe}' && *character != '\u{ffff}')
        })
        .collect()
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn format_unix_millis(millis: i64) -> String {
    let seconds = millis.div_euclid(1_000);
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} UTC")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::{thread, time::Duration};

    use super::*;
    use crate::api::models::{QuestionDraftApi, QuestionOptionApi, RichContentApi, TagApi};

    fn rich(value: &str) -> RichContentApi {
        RichContentApi {
            schema_version: 2,
            editor: Some("tiptap".to_owned()),
            editor_version: Some("test".to_owned()),
            document: None,
            html: format!("<p>{value}</p>"),
            plain_text: value.to_owned(),
            source_ooxml: None,
        }
    }

    fn stored_rich(value: &str) -> RichContentApi {
        RichContentApi {
            schema_version: 1,
            editor: None,
            editor_version: None,
            document: None,
            html: format!("<p>{value}</p>"),
            plain_text: value.to_owned(),
            source_ooxml: None,
        }
    }

    fn question() -> QuestionApi {
        QuestionApi {
            id: "00000000-0000-7000-8000-000000000001".to_owned(),
            question_type: "single_choice".to_owned(),
            question_type_name: Some("单选题".to_owned()),
            stem: rich("1 < 2 & 3"),
            options: vec![QuestionOptionApi {
                id: "00000000-0000-7000-8000-000000000002".to_owned(),
                position: 0,
                content: rich("选项A"),
            }],
            answer: rich("A"),
            explanation: rich("解析"),
            subject_id: "00000000-0000-7000-8000-000000000003".to_owned(),
            chapter_id: "00000000-0000-7000-8000-000000000004".to_owned(),
            subject_name: "网络".to_owned(),
            chapter_name: "第一章".to_owned(),
            tags: vec![TagApi {
                id: "00000000-0000-7000-8000-000000000005".to_owned(),
                name: "基础".to_owned(),
                question_count: 1,
                created_at: 0,
                updated_at: 0,
            }],
            resource_refs: Vec::new(),
            last_used_at: None,
            deleted_at: None,
            created_at: 0,
            updated_at: 0,
            content_version: 1,
        }
    }

    fn temporary_xlsx_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "zhitiku-question-bank-{label}-{}.xlsx",
            Uuid::now_v7()
        ))
    }

    #[test]
    fn xlsx_contains_real_question_fields_without_score_column() {
        let path = temporary_xlsx_path("content");
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        write_xlsx_archive(file, &[question()], MAX_XLSX_BYTES).unwrap();
        verify_xlsx(&path).unwrap();
        let mut archive = ZipArchive::new(File::open(&path).unwrap()).unwrap();
        let mut sheet = String::new();
        archive
            .by_name("xl/worksheets/sheet1.xml")
            .unwrap()
            .read_to_string(&mut sheet)
            .unwrap();
        assert!(sheet.contains("题干"));
        assert!(sheet.contains("答案"));
        assert!(sheet.contains("单选题"));
        assert!(sheet.contains("1 &lt; 2 &amp; 3"));
        assert!(!sheet.contains("分值"));
        drop(archive);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn xlsx_size_limit_stops_streaming_without_a_large_memory_buffer() {
        let path = temporary_xlsx_path("limit");
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        let error = write_xlsx_archive(file, &[question()], 128).unwrap_err();
        assert_eq!(error.code, "QUESTION_BANK_XLSX_TOO_LARGE");
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn xlsx_export_uses_a_temporary_file_and_never_overwrites_the_result() {
        let root =
            std::env::temp_dir().join(format!("zhitiku-question-bank-export-{}", Uuid::now_v7()));
        fs::create_dir(&root).unwrap();
        let output = root.join("题库.xlsx");

        let bytes = export_xlsx(&output, &[question()]).unwrap();
        assert!(bytes > 0);
        verify_xlsx(&output).unwrap();
        let error = validate_output_path(output.to_str().unwrap(), QuestionBankExportFormat::Xlsx)
            .unwrap_err();
        assert_eq!(error.code, "QUESTION_BANK_EXPORT_OUTPUT_EXISTS");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);

        fs::remove_file(output).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn output_path_requires_an_absolute_path_and_matching_extension() {
        let relative =
            validate_output_path("题库.xlsx", QuestionBankExportFormat::Xlsx).unwrap_err();
        assert_eq!(relative.code, "VALIDATION_ERROR");

        let root = std::env::temp_dir();
        let wrong_extension = root.join(format!("题库-{}.docx", Uuid::now_v7()));
        let error = validate_output_path(
            wrong_extension.to_str().unwrap(),
            QuestionBankExportFormat::Xlsx,
        )
        .unwrap_err();
        assert_eq!(error.code, "VALIDATION_ERROR");
    }

    #[tokio::test]
    async fn full_database_export_writes_readable_xlsx_and_docx_with_tags() {
        let root = std::env::temp_dir().join(format!(
            "zhitiku-question-bank-full-export-{}",
            Uuid::now_v7()
        ));
        let database = Database::open(&root, "test").await.unwrap();
        let subject_id = Uuid::now_v7().to_string();
        let chapter_id = Uuid::now_v7().to_string();
        let tag_id = Uuid::now_v7().to_string();
        sqlx::query(
            "INSERT INTO subjects (id, name, name_key, sort_order, created_at_ms, updated_at_ms) \
             VALUES (?, '计算机网络', '计算机网络', 1024, 1, 1)",
        )
        .bind(&subject_id)
        .execute(database.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO chapters (id, subject_id, name, name_key, sort_order, created_at_ms, updated_at_ms) \
             VALUES (?, ?, '第一章', '第一章', 1024, 1, 1)",
        )
        .bind(&chapter_id)
        .bind(&subject_id)
        .execute(database.pool())
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO tags (id, name, name_key, created_at_ms, updated_at_ms) \
             VALUES (?, '核心考点', '核心考点', 1, 1)",
        )
        .bind(&tag_id)
        .execute(database.pool())
        .await
        .unwrap();
        questions::save_question(
            database.pool(),
            &QuestionDraftApi {
                id: None,
                question_id: None,
                question_type: "single_choice".to_owned(),
                stem: stored_rich("交换机依据什么转发数据帧？"),
                options: vec![
                    QuestionOptionApi {
                        id: Uuid::now_v7().to_string(),
                        position: 0,
                        content: stored_rich("MAC 地址表"),
                    },
                    QuestionOptionApi {
                        id: Uuid::now_v7().to_string(),
                        position: 1,
                        content: stored_rich("域名表"),
                    },
                ],
                answer: stored_rich("A"),
                explanation: stored_rich("交换机会学习并查询 MAC 地址表。"),
                subject_id,
                chapter_id,
                tag_ids: vec![tag_id],
                resource_refs: Vec::new(),
                base_content_version: None,
            },
            "test",
        )
        .await
        .unwrap();

        let xlsx = root.join("题库.xlsx");
        let xlsx_result =
            export_question_bank(&database, "xlsx", xlsx.to_string_lossy().into_owned())
                .await
                .unwrap();
        assert_eq!(xlsx_result.question_count, 1);
        verify_xlsx(&xlsx).unwrap();

        let docx = root.join("题库.docx");
        let docx_result =
            export_question_bank(&database, "docx", docx.to_string_lossy().into_owned())
                .await
                .unwrap();
        assert_eq!(docx_result.question_count, 1);
        let mut archive = ZipArchive::new(File::open(&docx).unwrap()).unwrap();
        let mut document_xml = String::new();
        archive
            .by_name("word/document.xml")
            .unwrap()
            .read_to_string(&mut document_xml)
            .unwrap();
        assert!(document_xml.contains("交换机依据什么转发数据帧"));
        assert!(document_xml.contains("核心考点"));
        drop(archive);

        database.close().await;
        drop(database);
        for attempt in 1..=20 {
            match fs::remove_dir_all(&root) {
                Ok(()) => break,
                Err(error) if error.raw_os_error() == Some(32) && attempt < 20 => {
                    thread::sleep(Duration::from_millis(25));
                }
                Err(error) => panic!("failed to remove test root {}: {error}", root.display()),
            }
        }
    }

    #[test]
    fn unix_timestamp_format_is_stable() {
        assert_eq!(format_unix_millis(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(format_unix_millis(86_400_000), "1970-01-02 00:00:00 UTC");
    }
}
