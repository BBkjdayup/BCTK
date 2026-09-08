use std::{
    collections::{BTreeMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{Cursor, Write},
    path::{Path, PathBuf},
};

use calamine::{Data, Reader, Xlsx};
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

use super::models::{
    CommandError, CommandResult, ExcelImportAnalysisApi, ExcelImportRequestApi, ExcelImportRowApi,
    ExcelImportTemplateResultApi, ExcelWorkbookInspectionApi,
};

pub(super) const EXCEL_IMPORT_PARSER_VERSION: &str = "xlsx-text-v2";
const MAX_XLSX_SOURCE_BYTES: u64 = 100 * 1024 * 1024;
const MAX_XLSX_UNCOMPRESSED_BYTES: u64 = 512 * 1024 * 1024;
const MAX_XLSX_PARTS: usize = 4_096;
const MAX_XLSX_SHEETS: usize = 20;
const MAX_IMPORT_ITEMS: usize = 5_000;
const MAX_HEADER_SEARCH_ROWS: usize = 20;
const MAX_COLUMNS: usize = 128;
const MAX_CELL_CHARS: usize = 32_767;
const MAX_TEMPLATE_BYTES: u64 = 8 * 1024 * 1024;

pub(super) struct PreparedExcelImportSource {
    pub analysis: ExcelImportAnalysisApi,
    pub source_filename: String,
    pub source_bytes: Vec<u8>,
}

struct LoadedWorkbook {
    display_path: String,
    filename: String,
    bytes: Vec<u8>,
}

#[derive(Default)]
struct HeaderMapping {
    question_type: Option<usize>,
    subject: Option<usize>,
    chapter: Option<usize>,
    stem: Option<usize>,
    combined_options: Option<usize>,
    option_columns: Vec<(char, usize)>,
    answer: Option<usize>,
    explanation: Option<usize>,
    tags: Option<usize>,
}

impl HeaderMapping {
    fn from_row(row: &[Data]) -> Self {
        let mut mapping = Self::default();
        for (column, cell) in row.iter().take(MAX_COLUMNS).enumerate() {
            let raw = cell_text(cell);
            let key = normalized_header(&raw);
            if key.is_empty() {
                continue;
            }
            match key.as_str() {
                "题型" | "类型" | "questiontype" | "type" => {
                    mapping.question_type.get_or_insert(column);
                }
                "学科" | "科目" | "subject" => {
                    mapping.subject.get_or_insert(column);
                }
                "章节" | "章" | "chapter" => {
                    mapping.chapter.get_or_insert(column);
                }
                "题干" | "题目" | "问题" | "题干内容" | "题目内容" | "试题内容" | "stem"
                | "question" | "questioncontent" => {
                    mapping.stem.get_or_insert(column);
                }
                "选项" | "choices" | "options" => {
                    mapping.combined_options.get_or_insert(column);
                }
                "答案" | "正确答案" | "参考答案" | "标准答案" | "answer" | "correctanswer" =>
                {
                    mapping.answer.get_or_insert(column);
                }
                "解析" | "答案解析" | "题目解析" | "解题解析" | "参考解析" | "explanation"
                | "analysis" => {
                    mapping.explanation.get_or_insert(column);
                }
                "标签" | "标签名称" | "tags" | "tag" => {
                    mapping.tags.get_or_insert(column);
                }
                _ => {
                    if let Some(label) = option_column_label(&key) {
                        mapping.option_columns.push((label, column));
                    }
                }
            }
        }
        mapping.option_columns.sort_by_key(|(label, _)| *label);
        mapping.option_columns.dedup_by_key(|(label, _)| *label);
        mapping
    }

    fn is_complete(&self) -> bool {
        self.question_type.is_some() && self.stem.is_some() && self.answer.is_some()
    }
}

pub(super) fn inspect_workbook(input_path: String) -> CommandResult<ExcelWorkbookInspectionApi> {
    let loaded = load_workbook(&input_path)?;
    let workbook = open_xlsx(&loaded.bytes)?;
    let sheet_names = safe_sheet_names(&workbook)?;
    let default_sheet_name = default_sheet_name(&sheet_names).to_owned();
    Ok(ExcelWorkbookInspectionApi {
        source_path: loaded.display_path,
        source_file_name: loaded.filename,
        source_file_size: u64::try_from(loaded.bytes.len()).unwrap_or(u64::MAX),
        sheet_names,
        default_sheet_name,
    })
}

pub(super) fn prepare_import(
    request: ExcelImportRequestApi,
) -> CommandResult<PreparedExcelImportSource> {
    let loaded = load_workbook(&request.input_path)?;
    let mut workbook = open_xlsx(&loaded.bytes)?;
    let sheet_names = safe_sheet_names(&workbook)?;
    let selected_sheet_name = match request.sheet_name.as_deref().map(str::trim) {
        Some(name) if !name.is_empty() => sheet_names
            .iter()
            .find(|candidate| candidate.as_str() == name)
            .cloned()
            .ok_or_else(|| {
                CommandError::validation("所选 Excel 工作表已经不存在，请重新选择文件。")
            })?,
        _ => default_sheet_name(&sheet_names).to_owned(),
    };
    let range = workbook
        .worksheet_range(&selected_sheet_name)
        .map_err(|error| {
            CommandError::new(
                "XLSX_SHEET_READ_FAILED",
                format!("无法读取工作表“{selected_sheet_name}”：{error}"),
            )
        })?;
    if range.width() > MAX_COLUMNS {
        return Err(CommandError::new(
            "XLSX_TOO_MANY_COLUMNS",
            format!(
                "工作表包含 {} 列，超过 {} 列安全上限。",
                range.width(),
                MAX_COLUMNS
            ),
        ));
    }
    let formula_cell_count = workbook
        .worksheet_formula(&selected_sheet_name)
        .map(|formulas| {
            formulas
                .rows()
                .flat_map(|row| row.iter())
                .filter(|formula| !formula.trim().is_empty())
                .count()
        })
        .unwrap_or(0);

    let rows = range.rows().collect::<Vec<_>>();
    let (header_index, headers) = rows
        .iter()
        .take(MAX_HEADER_SEARCH_ROWS)
        .enumerate()
        .find_map(|(index, row)| {
            let mapping = HeaderMapping::from_row(row);
            mapping.is_complete().then_some((index, mapping))
        })
        .ok_or_else(|| {
            CommandError::new(
                "XLSX_HEADER_NOT_FOUND",
                "前 20 行中没有找到完整表头。至少需要“题型、题干（或题干内容）、答案（或正确答案）”三列。",
            )
        })?;
    let start_row = range.start().map(|(row, _)| row as usize).unwrap_or(0);
    let mut parsed_rows = Vec::new();
    let mut source_item_count = 0usize;
    for (offset, row) in rows.iter().enumerate().skip(header_index + 1) {
        let question_type = cell_at(row, headers.question_type);
        let subject_name = cell_at(row, headers.subject);
        let chapter_name = cell_at(row, headers.chapter);
        let stem = cell_at(row, headers.stem);
        let answer = without_empty_placeholder(&cell_at(row, headers.answer));
        let explanation = without_empty_placeholder(&cell_at(row, headers.explanation));
        let combined_options = cell_at(row, headers.combined_options);
        let tag_text = cell_at(row, headers.tags);
        if [
            &question_type,
            &subject_name,
            &chapter_name,
            &stem,
            &answer,
            &explanation,
            &combined_options,
            &tag_text,
        ]
        .iter()
        .all(|value| value.trim().is_empty())
            && headers
                .option_columns
                .iter()
                .all(|(_, column)| cell_at(row, Some(*column)).trim().is_empty())
        {
            continue;
        }
        source_item_count += 1;
        if parsed_rows.len() >= MAX_IMPORT_ITEMS {
            continue;
        }
        let mut options = headers
            .option_columns
            .iter()
            .filter_map(|(_, column)| {
                let value = cell_at(row, Some(*column));
                (!value.trim().is_empty()).then_some(value)
            })
            .collect::<Vec<_>>();
        if options.is_empty() && !combined_options.trim().is_empty() {
            options = parse_combined_options(&combined_options);
        }
        let row_number = start_row.saturating_add(offset).saturating_add(1);
        parsed_rows.push(ExcelImportRowApi {
            row_number: u32::try_from(row_number).unwrap_or(u32::MAX),
            question_type,
            subject_name,
            chapter_name,
            stem,
            options,
            answer,
            explanation,
            tag_names: split_tags(&tag_text),
        });
    }
    if source_item_count == 0 {
        return Err(CommandError::new(
            "XLSX_NO_QUESTION_ROWS",
            "表头下方没有找到题目数据。请填写后重新导入。",
        ));
    }
    let omitted_item_count = source_item_count.saturating_sub(parsed_rows.len());
    let mut warnings = Vec::new();
    if formula_cell_count > 0 {
        warnings.push(format!(
            "所选工作表含 {formula_cell_count} 个公式单元格；软件不会执行公式，只读取文件中已保存的显示值。"
        ));
    }
    if omitted_item_count > 0 {
        warnings.push(format!(
            "工作表共有 {source_item_count} 行题目，本次只载入前 {MAX_IMPORT_ITEMS} 行；请拆分文件后导入剩余内容。"
        ));
    }
    let source_file_size = u64::try_from(loaded.bytes.len()).unwrap_or(u64::MAX);
    Ok(PreparedExcelImportSource {
        analysis: ExcelImportAnalysisApi {
            source_path: loaded.display_path,
            source_file_name: loaded.filename.clone(),
            source_file_size,
            selected_sheet_name,
            sheet_names,
            header_row_number: u32::try_from(start_row + header_index + 1).unwrap_or(u32::MAX),
            source_item_count: u32::try_from(source_item_count).unwrap_or(u32::MAX),
            omitted_item_count: u32::try_from(omitted_item_count).unwrap_or(u32::MAX),
            formula_cell_count: u32::try_from(formula_cell_count).unwrap_or(u32::MAX),
            rows: parsed_rows,
            warnings,
        },
        source_filename: loaded.filename,
        source_bytes: loaded.bytes,
    })
}

pub(super) fn create_template(output_path: String) -> CommandResult<ExcelImportTemplateResultApi> {
    let output = validate_template_output_path(&output_path)?;
    let filename = output
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| CommandError::validation("Excel 模板文件名包含无法处理的字符。"))?
        .to_owned();
    let parent = output
        .parent()
        .ok_or_else(|| CommandError::validation("Excel 模板路径缺少保存目录。"))?;
    let temporary = parent.join(format!(".{filename}.{}.tmp", Uuid::now_v7()));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| {
            CommandError::new(
                "XLSX_TEMPLATE_CREATE_FAILED",
                format!("无法创建 Excel 模板临时文件：{error}"),
            )
        })?;
    let bytes = match write_template_archive(file) {
        Ok(bytes) => bytes,
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
    };
    if let Err(error) = inspect_template(&temporary) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    if fs::symlink_metadata(&output).is_ok() {
        let _ = fs::remove_file(&temporary);
        return Err(CommandError::new(
            "XLSX_TEMPLATE_OUTPUT_EXISTS",
            "保存位置已经出现同名文件。为避免覆盖，已停止保存。",
        ));
    }
    fs::rename(&temporary, &output).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        CommandError::new(
            "XLSX_TEMPLATE_COMMIT_FAILED",
            format!("Excel 模板无法保存到最终位置：{error}"),
        )
    })?;
    Ok(ExcelImportTemplateResultApi {
        output_path: output.to_string_lossy().into_owned(),
        output_filename: filename,
        output_bytes: bytes,
    })
}

fn load_workbook(input_path: &str) -> CommandResult<LoadedWorkbook> {
    let requested = PathBuf::from(input_path.trim());
    if input_path.trim().is_empty() || !requested.is_absolute() {
        return Err(CommandError::validation(
            "Excel 导入文件必须使用非空的绝对路径。",
        ));
    }
    if !requested
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("xlsx"))
    {
        return Err(CommandError::validation(
            "题库 Excel 导入只接受 .xlsx 文件。",
        ));
    }
    let requested_metadata = fs::symlink_metadata(&requested).map_err(|error| {
        CommandError::new(
            "XLSX_SOURCE_UNAVAILABLE",
            format!("Excel 文件不存在或无法访问：{error}"),
        )
    })?;
    if requested_metadata.file_type().is_symlink() || !requested_metadata.is_file() {
        return Err(CommandError::new(
            "XLSX_SOURCE_UNSAFE",
            "所选 Excel 来源不是安全的真实文件。",
        ));
    }
    if requested_metadata.len() == 0 || requested_metadata.len() > MAX_XLSX_SOURCE_BYTES {
        return Err(CommandError::new(
            "XLSX_SOURCE_SIZE_INVALID",
            "Excel 文件必须大于 0 字节且不超过 100 MB。",
        ));
    }
    let canonical = fs::canonicalize(&requested).map_err(|error| {
        CommandError::new(
            "XLSX_SOURCE_UNAVAILABLE",
            format!("无法定位 Excel 文件：{error}"),
        )
    })?;
    let bytes = fs::read(&canonical).map_err(|error| {
        CommandError::new(
            "XLSX_SOURCE_READ_FAILED",
            format!("无法读取 Excel 文件：{error}"),
        )
    })?;
    preflight_archive(&bytes)?;
    let filename = canonical
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .ok_or_else(|| CommandError::validation("Excel 文件名包含无法安全保存的字符。"))?;
    Ok(LoadedWorkbook {
        display_path: canonical.to_string_lossy().into_owned(),
        filename,
        bytes,
    })
}

fn preflight_archive(bytes: &[u8]) -> CommandResult<()> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|error| {
        CommandError::new(
            "XLSX_ARCHIVE_INVALID",
            format!("Excel 文件不是有效的 Open XML 工作簿：{error}"),
        )
    })?;
    if archive.is_empty() || archive.len() > MAX_XLSX_PARTS {
        return Err(CommandError::new(
            "XLSX_PART_COUNT_INVALID",
            format!("Excel 文件部件数量必须在 1 到 {MAX_XLSX_PARTS} 之间。"),
        ));
    }
    let mut total_uncompressed = 0u64;
    let mut names = HashSet::new();
    for index in 0..archive.len() {
        let part = archive.by_index(index).map_err(|error| {
            CommandError::new(
                "XLSX_ARCHIVE_INVALID",
                format!("无法检查 Excel 部件：{error}"),
            )
        })?;
        let enclosed = part.enclosed_name().ok_or_else(|| {
            CommandError::new(
                "XLSX_ARCHIVE_PATH_UNSAFE",
                "Excel 文件包含不安全的内部路径。",
            )
        })?;
        let name = enclosed.to_string_lossy().replace('\\', "/");
        if !names.insert(name) {
            return Err(CommandError::new(
                "XLSX_ARCHIVE_DUPLICATE_PART",
                "Excel 文件包含重复部件，已停止读取。",
            ));
        }
        total_uncompressed = total_uncompressed.saturating_add(part.size());
        if total_uncompressed > MAX_XLSX_UNCOMPRESSED_BYTES {
            return Err(CommandError::new(
                "XLSX_ARCHIVE_TOO_LARGE",
                "Excel 文件解压后的内容超过 512 MB 安全上限。",
            ));
        }
    }
    if !names.contains("[Content_Types].xml") || !names.contains("xl/workbook.xml") {
        return Err(CommandError::new(
            "XLSX_REQUIRED_PART_MISSING",
            "Excel 文件缺少必要的工作簿部件。",
        ));
    }
    Ok(())
}

fn open_xlsx(bytes: &[u8]) -> CommandResult<Xlsx<Cursor<Vec<u8>>>> {
    Xlsx::new(Cursor::new(bytes.to_vec())).map_err(|error| {
        CommandError::new("XLSX_OPEN_FAILED", format!("Excel 工作簿无法打开：{error}"))
    })
}

fn safe_sheet_names<RS: std::io::Read + std::io::Seek>(
    workbook: &Xlsx<RS>,
) -> CommandResult<Vec<String>> {
    let names = workbook.sheet_names();
    if names.is_empty() || names.len() > MAX_XLSX_SHEETS {
        return Err(CommandError::new(
            "XLSX_SHEET_COUNT_INVALID",
            format!("Excel 工作表数量必须在 1 到 {MAX_XLSX_SHEETS} 之间。"),
        ));
    }
    Ok(names)
}

fn default_sheet_name(sheet_names: &[String]) -> &str {
    sheet_names
        .iter()
        .find(|name| normalized_header(name) == "题库")
        .or_else(|| sheet_names.first())
        .map(String::as_str)
        .unwrap_or("题库")
}

fn cell_at(row: &[Data], column: Option<usize>) -> String {
    column
        .and_then(|index| row.get(index))
        .map(cell_text)
        .unwrap_or_default()
}

fn cell_text(cell: &Data) -> String {
    let value = match cell {
        Data::Empty => String::new(),
        Data::String(value) => value.clone(),
        other => other.to_string(),
    };
    truncate_chars(value.trim(), MAX_CELL_CHARS)
}

fn normalized_header(value: &str) -> String {
    value
        .nfkc()
        .flat_map(char::to_lowercase)
        .filter(|character| {
            !character.is_whitespace() && !matches!(character, '_' | '-' | ':' | '：')
        })
        .collect()
}

fn option_column_label(key: &str) -> Option<char> {
    let normalized = key.nfkc().collect::<String>().to_ascii_uppercase();
    for value in [
        normalized.strip_prefix("选项"),
        normalized.strip_suffix("选项"),
        normalized.strip_prefix("OPTION"),
    ]
    .into_iter()
    .flatten()
    {
        let mut chars = value.chars();
        let label = chars.next()?;
        if chars.next().is_none() && label.is_ascii_uppercase() {
            return Some(label);
        }
    }
    None
}

fn parse_combined_options(value: &str) -> Vec<String> {
    let mut options = BTreeMap::<char, String>::new();
    let mut last_label = None;
    for line in value.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let normalized = line.nfkc().collect::<String>();
        let mut chars = normalized.char_indices();
        let Some((_, label)) = chars.next() else {
            continue;
        };
        let Some((marker_index, marker)) = chars.next() else {
            continue;
        };
        let label = label.to_ascii_uppercase();
        if label.is_ascii_uppercase() && matches!(marker, '.' | '、' | ':' | ')' | ' ' | '\t') {
            let content_start = marker_index + marker.len_utf8();
            let content = normalized[content_start..].trim();
            if !content.is_empty() {
                options.insert(label, content.to_owned());
                last_label = Some(label);
                continue;
            }
        }
        if let Some(label) = last_label {
            options.entry(label).and_modify(|content| {
                content.push('\n');
                content.push_str(line);
            });
        }
    }
    options.into_values().collect()
}

fn split_tags(value: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    value
        .split(['、', ',', '，', ';', '；', '\n', '\r'])
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .filter_map(|value| {
            let key = value.nfkc().collect::<String>().to_lowercase();
            seen.insert(key).then(|| truncate_chars(value, 80))
        })
        .collect()
}

fn without_empty_placeholder(value: &str) -> String {
    let normalized = value.nfkc().collect::<String>();
    if matches!(
        normalized.trim(),
        "(未填写)" | "（未填写）" | "(未填写)。" | "（未填写）。"
    ) {
        String::new()
    } else {
        value.to_owned()
    }
}

fn truncate_chars(value: &str, maximum: usize) -> String {
    if value.chars().count() <= maximum {
        value.to_owned()
    } else {
        value.chars().take(maximum).collect()
    }
}

fn validate_template_output_path(raw: &str) -> CommandResult<PathBuf> {
    let requested = PathBuf::from(raw.trim());
    if raw.trim().is_empty() || !requested.is_absolute() {
        return Err(CommandError::validation(
            "Excel 模板必须使用非空的绝对路径。",
        ));
    }
    if !requested
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("xlsx"))
    {
        return Err(CommandError::validation(
            "Excel 模板保存路径必须以 .xlsx 结尾。",
        ));
    }
    if fs::symlink_metadata(&requested).is_ok() {
        return Err(CommandError::new(
            "XLSX_TEMPLATE_OUTPUT_EXISTS",
            "保存位置已经存在同名文件，请换一个文件名。",
        ));
    }
    let file_name = requested
        .file_name()
        .ok_or_else(|| CommandError::validation("Excel 模板路径缺少文件名。"))?;
    let parent = requested
        .parent()
        .ok_or_else(|| CommandError::validation("Excel 模板路径缺少保存目录。"))?;
    let parent = fs::canonicalize(parent).map_err(|error| {
        CommandError::new(
            "XLSX_TEMPLATE_DIRECTORY_UNAVAILABLE",
            format!("模板保存目录不存在或无法访问：{error}"),
        )
    })?;
    let metadata = fs::symlink_metadata(&parent).map_err(|error| {
        CommandError::new(
            "XLSX_TEMPLATE_DIRECTORY_UNAVAILABLE",
            format!("无法检查模板保存目录：{error}"),
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(CommandError::new(
            "XLSX_TEMPLATE_DIRECTORY_UNSAFE",
            "模板保存位置不是安全的真实目录。",
        ));
    }
    Ok(parent.join(file_name))
}

fn write_template_archive(file: File) -> CommandResult<u64> {
    const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/worksheets/sheet2.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/></Types>"#;
    const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
    const WORKBOOK: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="题库" sheetId="1" r:id="rId1"/><sheet name="填写说明" sheetId="2" r:id="rId2"/></sheets></workbook>"#;
    const WORKBOOK_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#;
    const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="2"><font><sz val="11"/><name val="等线"/></font><font><b/><color rgb="FFFFFFFF"/><sz val="11"/><name val="等线"/></font></fonts><fills count="3"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="solid"><fgColor rgb="FF2563EB"/><bgColor indexed="64"/></patternFill></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="3"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="0" fontId="1" fillId="2" borderId="0" xfId="0" applyFont="1" applyFill="1"/><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0" applyAlignment="1"><alignment vertical="top" wrapText="1"/></xf></cellXfs></styleSheet>"#;
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, content) in [
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("xl/workbook.xml", WORKBOOK),
        ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
        ("xl/styles.xml", STYLES),
    ] {
        writer
            .start_file(name, options)
            .map_err(template_zip_error)?;
        writer
            .write_all(content.as_bytes())
            .map_err(template_write_error)?;
    }
    let headers = [
        "题型", "学科", "章节", "题干", "选项A", "选项B", "选项C", "选项D", "选项E", "选项F",
        "选项G", "选项H", "答案", "解析", "标签",
    ];
    let sheet1 = sheet_xml(&[headers.iter().map(|value| (*value).to_owned()).collect()]);
    writer
        .start_file("xl/worksheets/sheet1.xml", options)
        .map_err(template_zip_error)?;
    writer
        .write_all(sheet1.as_bytes())
        .map_err(template_write_error)?;
    let guide_rows = vec![
        vec![
            "字段".to_owned(),
            "是否必填".to_owned(),
            "填写规则".to_owned(),
        ],
        vec![
            "题型".to_owned(),
            "是".to_owned(),
            "单选题、多选题、填空题、判断题、简答题，或软件中已有的自定义题型".to_owned(),
        ],
        vec![
            "题干".to_owned(),
            "是".to_owned(),
            "每行一道题，可在单元格中换行".to_owned(),
        ],
        vec![
            "答案".to_owned(),
            "是".to_owned(),
            "选择题可填 A 或 A、B、C；判断题可填正确/错误、对/错、√/×".to_owned(),
        ],
        vec![
            "选项A-H".to_owned(),
            "选择题必填".to_owned(),
            "至少填写两个非空选项；非选择题留空".to_owned(),
        ],
        vec![
            "学科/章节".to_owned(),
            "可选".to_owned(),
            "名称必须与软件中已有分类一致；留空时使用进入导入页时选中的分类".to_owned(),
        ],
        vec![
            "标签".to_owned(),
            "可选".to_owned(),
            "多个标签可用顿号、逗号、分号或换行分隔".to_owned(),
        ],
        vec![
            "说明".to_owned(),
            "".to_owned(),
            "请把题目填写在“题库”工作表；软件不会执行公式，也不导入浮动图片、复杂表格或公式对象"
                .to_owned(),
        ],
    ];
    let sheet2 = sheet_xml(&guide_rows);
    writer
        .start_file("xl/worksheets/sheet2.xml", options)
        .map_err(template_zip_error)?;
    writer
        .write_all(sheet2.as_bytes())
        .map_err(template_write_error)?;
    let file = writer.finish().map_err(template_zip_error)?;
    file.sync_all().map_err(template_write_error)?;
    let bytes = file.metadata().map_err(template_write_error)?.len();
    if bytes > MAX_TEMPLATE_BYTES {
        return Err(CommandError::new(
            "XLSX_TEMPLATE_TOO_LARGE",
            "生成的 Excel 模板超过 8 MB 安全上限。",
        ));
    }
    Ok(bytes)
}

fn sheet_xml(rows: &[Vec<String>]) -> String {
    let row_count = rows.len().max(1);
    let column_count = rows.iter().map(Vec::len).max().unwrap_or(1);
    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><dimension ref=\"A1:{}{}\"/><sheetViews><sheetView workbookViewId=\"0\"><pane ySplit=\"1\" topLeftCell=\"A2\" activePane=\"bottomLeft\" state=\"frozen\"/></sheetView></sheetViews><cols><col min=\"1\" max=\"3\" width=\"18\" customWidth=\"1\"/><col min=\"4\" max=\"15\" width=\"28\" customWidth=\"1\"/></cols><sheetData>",
        column_label(column_count),
        row_count,
    );
    for (row_index, row) in rows.iter().enumerate() {
        let row_number = row_index + 1;
        xml.push_str(&format!("<row r=\"{row_number}\">"));
        for (column_index, value) in row.iter().enumerate() {
            let reference = format!("{}{}", column_label(column_index + 1), row_number);
            let style = if row_index == 0 { 1 } else { 2 };
            xml.push_str(&format!(
                "<c r=\"{reference}\" t=\"inlineStr\" s=\"{style}\"><is><t xml:space=\"preserve\">{}</t></is></c>",
                escape_xml(value),
            ));
        }
        xml.push_str("</row>");
    }
    xml.push_str("</sheetData></worksheet>");
    xml
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

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn template_zip_error(error: zip::result::ZipError) -> CommandError {
    CommandError::new(
        "XLSX_TEMPLATE_BUILD_FAILED",
        format!("Excel 模板文件包生成失败：{error}"),
    )
}

fn template_write_error(error: std::io::Error) -> CommandError {
    CommandError::new(
        "XLSX_TEMPLATE_WRITE_FAILED",
        format!("Excel 模板写入失败：{error}"),
    )
}

fn inspect_template(path: &Path) -> CommandResult<()> {
    let bytes = fs::read(path).map_err(template_write_error)?;
    preflight_archive(&bytes)?;
    let workbook = open_xlsx(&bytes)?;
    let names = safe_sheet_names(&workbook)?;
    if names != ["题库", "填写说明"] {
        return Err(CommandError::new(
            "XLSX_TEMPLATE_VERIFY_FAILED",
            "生成的 Excel 模板工作表结构不完整。",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{
        models::{QuestionApi, QuestionOptionApi, RichContentApi, TagApi},
        question_bank_export::write_xlsx_archive,
    };

    fn rich(value: &str) -> RichContentApi {
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

    fn exported_question() -> QuestionApi {
        QuestionApi {
            id: Uuid::now_v7().to_string(),
            question_type: "single_choice".to_owned(),
            question_type_name: Some("单选题".to_owned()),
            stem: rich("Excel 导出后能否重新导入？"),
            options: vec![
                QuestionOptionApi {
                    id: Uuid::now_v7().to_string(),
                    position: 0,
                    content: rich("可以"),
                },
                QuestionOptionApi {
                    id: Uuid::now_v7().to_string(),
                    position: 1,
                    content: rich("不可以"),
                },
            ],
            answer: rich("A"),
            explanation: rich("当前导出格式应与导入格式兼容。"),
            subject_id: Uuid::now_v7().to_string(),
            chapter_id: Uuid::now_v7().to_string(),
            subject_name: "网设".to_owned(),
            chapter_name: "1".to_owned(),
            tags: vec![TagApi {
                id: Uuid::now_v7().to_string(),
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

    #[test]
    fn template_is_readable_and_uses_question_sheet_by_default() {
        let root = std::env::temp_dir().join(format!("zhitiku-xlsx-template-{}", Uuid::now_v7()));
        fs::create_dir(&root).unwrap();
        let output = root.join("导入模板.xlsx");
        let result = create_template(output.to_string_lossy().into_owned()).unwrap();
        assert!(result.output_bytes > 0);
        let inspected = inspect_workbook(output.to_string_lossy().into_owned()).unwrap();
        assert_eq!(inspected.sheet_names, ["题库", "填写说明"]);
        assert_eq!(inspected.default_sheet_name, "题库");
        fs::remove_file(output).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn combined_options_and_export_placeholders_are_normalized() {
        assert_eq!(
            parse_combined_options("A. 第一项\nB、第二项\nC：第三项"),
            ["第一项", "第二项", "第三项"],
        );
        assert_eq!(without_empty_placeholder("（未填写）"), "");
        assert_eq!(split_tags("基础、重点;基础"), ["基础", "重点"]);
    }

    #[test]
    fn common_third_party_header_aliases_are_mapped() {
        let row = [
            "题型",
            "题干内容",
            "题目解析",
            "正确答案",
            "选项 A",
            "B选项",
            "标签名称",
        ]
        .into_iter()
        .map(|value| Data::String(value.to_owned()))
        .collect::<Vec<_>>();

        let mapping = HeaderMapping::from_row(&row);
        assert!(mapping.is_complete());
        assert_eq!(mapping.question_type, Some(0));
        assert_eq!(mapping.stem, Some(1));
        assert_eq!(mapping.explanation, Some(2));
        assert_eq!(mapping.answer, Some(3));
        assert_eq!(mapping.option_columns, [('A', 4), ('B', 5)]);
        assert_eq!(mapping.tags, Some(6));
    }

    #[test]
    fn current_excel_export_can_be_read_back_as_one_question() {
        let root = std::env::temp_dir().join(format!("zhitiku-xlsx-roundtrip-{}", Uuid::now_v7()));
        fs::create_dir(&root).unwrap();
        let output = root.join("题库导出.xlsx");
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output)
            .unwrap();
        write_xlsx_archive(file, &[exported_question()], 8 * 1024 * 1024).unwrap();

        let prepared = prepare_import(ExcelImportRequestApi {
            input_path: output.to_string_lossy().into_owned(),
            sheet_name: Some("题库".to_owned()),
        })
        .unwrap();
        assert_eq!(prepared.analysis.rows.len(), 1);
        let row = &prepared.analysis.rows[0];
        assert_eq!(row.question_type, "单选题");
        assert_eq!(row.stem, "Excel 导出后能否重新导入？");
        assert_eq!(row.options, ["可以", "不可以"]);
        assert_eq!(row.answer, "A");
        assert_eq!(row.explanation, "当前导出格式应与导入格式兼容。");
        assert_eq!(row.tag_names, ["基础"]);

        fs::remove_file(output).unwrap();
        fs::remove_dir(root).unwrap();
    }
}
