use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use scraper::{Html, Selector};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, SqliteConnection, SqlitePool};
use uuid::Uuid;

use crate::{
    db::DatabasePaths,
    docx::{ExtractedImageOccurrence, inspect_raster_image},
};

use super::models::{
    BeginExcelImportResultApi, BeginWordImportResultApi, CommandError, CommandResult,
    DocxFormulaOccurrenceApi, DocxImageOccurrenceApi, DocxTableCellApi, DocxTableOccurrenceApi,
    ManagedImagePayloadApi, QuestionOptionApi, QuestionResourceRefApi,
    ReadWpsClipboardImagesRequestApi, RichContentApi, StoreManagedImagesRequestApi,
    WordImportDraftPayloadApi, WordImportDraftRecordApi, WpsClipboardImagePayloadApi,
};

const WORD_IMPORT_DRAFT_KEY: &str = "word_import:active";
const WORD_IMPORT_DRAFT_KIND: &str = "word_import_preview";
const WORD_IMPORT_PARSER_VERSION: &str = "w1-ooxml-images-formulas-tables-7";
const MAX_RESOURCE_REFS_PER_QUESTION: usize = 512;
const MAX_WPS_CLIPBOARD_IMAGES: usize = 512;
const MAX_WPS_CLIPBOARD_IMAGE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_WPS_CLIPBOARD_TOTAL_BYTES: u64 = 32 * 1024 * 1024;
const MAX_MANAGED_IMAGE_INPUTS: usize = 128;
const MAX_MANAGED_IMAGE_BYTES: usize = 8 * 1024 * 1024;
const MAX_MANAGED_IMAGE_TOTAL_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, FromRow)]
struct ResourceRow {
    id: String,
    sha256: Vec<u8>,
    resource_kind: String,
    mime_type: String,
    storage_rel_path: String,
    byte_size: i64,
    intrinsic_width_px: Option<i64>,
    intrinsic_height_px: Option<i64>,
    availability_status: String,
}

#[derive(Debug)]
struct NewResource {
    id: String,
    sha256: Vec<u8>,
    sha256_hex: String,
    mime_type: String,
    storage_rel_path: String,
    original_filename: Option<String>,
    byte_size: u64,
    width_px: u32,
    height_px: u32,
    bytes: Vec<u8>,
}

#[derive(Debug)]
struct PreparedOccurrence {
    node_id: String,
    resource_id: String,
    paragraph_index: usize,
    text_char_offset: usize,
    original_filename: Option<String>,
    mime_type: String,
    byte_size: u64,
    width_px: u32,
    height_px: u32,
}

struct StagingCleanup {
    path: PathBuf,
    active: bool,
}

impl Drop for StagingCleanup {
    fn drop(&mut self) {
        if self.active {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

pub(crate) async fn begin_word_import(
    pool: &SqlitePool,
    paths: &DatabasePaths,
    prepared: super::docx::PreparedWordImportSource,
) -> CommandResult<BeginWordImportResultApi> {
    let super::docx::PreparedWordImportSource {
        analysis,
        source_filename,
        source_bytes,
        images: extracted,
        formulas: extracted_formulas,
        tables: extracted_tables,
    } = prepared;
    let source_filename = super::drafts::normalized_source_file_name(&source_filename)?;
    if !analysis.is_valid {
        return Err(CommandError::new(
            "DOCX_IMPORT_ANALYSIS_REJECTED",
            "Word 文档未通过安全分析，不能创建导入草稿。",
        ));
    }
    let existing: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM drafts WHERE draft_key = ?")
        .bind(WORD_IMPORT_DRAFT_KEY)
        .fetch_one(pool)
        .await
        .map_err(CommandError::database)?;
    if existing != 0 {
        return Err(CommandError::new(
            "WORD_IMPORT_DRAFT_EXISTS",
            "已有一份未完成的 Word 导入草稿，请先恢复或放弃旧草稿。",
        ));
    }

    let prepared = prepare_resources(pool, paths, extracted).await?;
    let draft_id = Uuid::now_v7().to_string();
    let session_id = Uuid::now_v7().to_string();
    let now = now_millis();
    let source_sha256 = Sha256::digest(&source_bytes).to_vec();
    let source_size = u64::try_from(source_bytes.len()).unwrap_or(u64::MAX);
    let payload = WordImportDraftPayloadApi {
        schema_version: 1,
        source_file_name: source_filename.clone(),
        source_file_size: source_size,
        parser_version: WORD_IMPORT_PARSER_VERSION.to_owned(),
        import_session_id: Some(session_id.clone()),
        source_item_count: None,
        omitted_item_count: 0,
        active_item_id: None,
        items: Vec::new(),
    };
    let payload_json = serde_json::to_string(&payload).map_err(CommandError::database)?;
    let diagnostics_json =
        serde_json::to_string(&analysis.diagnostics).map_err(CommandError::database)?;
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;

    // A second check inside the write transaction prevents two concurrent
    // import starts from replacing one another even if both passed preflight.
    let existing: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM drafts WHERE draft_key = ?")
        .bind(WORD_IMPORT_DRAFT_KEY)
        .fetch_one(&mut *transaction)
        .await
        .map_err(CommandError::database)?;
    if existing != 0 {
        return Err(CommandError::new(
            "WORD_IMPORT_DRAFT_EXISTS",
            "已有一份未完成的 Word 导入草稿，请先恢复或放弃旧草稿。",
        ));
    }

    sqlx::query(
        "INSERT INTO drafts (id, draft_key, draft_kind, target_question_id, \
             base_content_version, payload_schema_version, payload_json, source_title, \
             autosaved_at_ms, created_at_ms, updated_at_ms) \
         VALUES (?, ?, ?, NULL, NULL, 1, ?, ?, ?, ?, ?)",
    )
    .bind(&draft_id)
    .bind(WORD_IMPORT_DRAFT_KEY)
    .bind(WORD_IMPORT_DRAFT_KIND)
    .bind(&payload_json)
    .bind(&source_filename)
    .bind(now)
    .bind(now)
    .bind(now)
    .execute(&mut *transaction)
    .await
    .map_err(CommandError::database)?;

    sqlx::query(
        "INSERT INTO word_import_sessions (id, draft_id, source_filename, source_sha256, \
             source_byte_size, parser_version, status, diagnostics_json, created_at_ms, updated_at_ms) \
         VALUES (?, ?, ?, ?, ?, ?, 'reviewing', ?, ?, ?)",
    )
    .bind(&session_id)
    .bind(&draft_id)
    .bind(&source_filename)
    .bind(source_sha256)
    .bind(i64::try_from(source_size).unwrap_or(i64::MAX))
    .bind(WORD_IMPORT_PARSER_VERSION)
    .bind(diagnostics_json)
    .bind(now)
    .bind(now)
    .execute(&mut *transaction)
    .await
    .map_err(CommandError::database)?;

    for occurrence in &prepared {
        sqlx::query(
            "INSERT INTO draft_resource_refs (id, draft_id, resource_id, node_id, created_at_ms) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(Uuid::now_v7().to_string())
        .bind(&draft_id)
        .bind(&occurrence.resource_id)
        .bind(&occurrence.node_id)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(CommandError::database)?;
    }

    transaction.commit().await.map_err(CommandError::database)?;
    let images = prepared
        .into_iter()
        .map(|occurrence| DocxImageOccurrenceApi {
            node_id: occurrence.node_id,
            resource_id: occurrence.resource_id,
            paragraph_index: u32::try_from(occurrence.paragraph_index).unwrap_or(u32::MAX),
            text_char_offset: u32::try_from(occurrence.text_char_offset).unwrap_or(u32::MAX),
            original_filename: occurrence.original_filename,
            mime_type: occurrence.mime_type,
            byte_size: occurrence.byte_size,
            width_px: occurrence.width_px,
            height_px: occurrence.height_px,
        })
        .collect();
    let formulas = extracted_formulas
        .into_iter()
        .map(|occurrence| DocxFormulaOccurrenceApi {
            node_id: Uuid::now_v7().to_string(),
            paragraph_index: u32::try_from(occurrence.paragraph_index).unwrap_or(u32::MAX),
            text_char_offset: u32::try_from(occurrence.text_char_offset).unwrap_or(u32::MAX),
            latex: occurrence.latex,
            source_kind: occurrence.source_kind,
            product_version: occurrence.product_version,
            product_subversion: occurrence.product_subversion,
        })
        .collect();
    let tables = extracted_tables
        .into_iter()
        .map(|table| DocxTableOccurrenceApi {
            table_index: u32::try_from(table.index).unwrap_or(u32::MAX),
            rows: table
                .rows
                .into_iter()
                .map(|row| {
                    row.into_iter()
                        .map(|cell| DocxTableCellApi {
                            paragraph_indices: cell
                                .paragraph_indices
                                .into_iter()
                                .map(|index| u32::try_from(index).unwrap_or(u32::MAX))
                                .collect(),
                        })
                        .collect()
                })
                .collect(),
        })
        .collect();

    Ok(BeginWordImportResultApi {
        analysis,
        draft: WordImportDraftRecordApi {
            id: draft_id,
            payload,
            autosaved_at: now,
            created_at: now,
            updated_at: now,
        },
        images,
        formulas,
        tables,
    })
}

pub(crate) async fn begin_excel_import(
    pool: &SqlitePool,
    prepared: super::excel_import::PreparedExcelImportSource,
) -> CommandResult<BeginExcelImportResultApi> {
    let super::excel_import::PreparedExcelImportSource {
        analysis,
        source_filename,
        source_bytes,
    } = prepared;
    let source_filename = super::drafts::normalized_source_file_name(&source_filename)?;
    let existing: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM drafts WHERE draft_key = ?")
        .bind(WORD_IMPORT_DRAFT_KEY)
        .fetch_one(pool)
        .await
        .map_err(CommandError::database)?;
    if existing != 0 {
        return Err(CommandError::new(
            "WORD_IMPORT_DRAFT_EXISTS",
            "已有一份未完成的批量导入草稿，请先恢复或放弃旧草稿。",
        ));
    }

    let draft_id = Uuid::now_v7().to_string();
    let session_id = Uuid::now_v7().to_string();
    let now = now_millis();
    let source_sha256 = Sha256::digest(&source_bytes).to_vec();
    let source_size = u64::try_from(source_bytes.len()).unwrap_or(u64::MAX);
    let payload = WordImportDraftPayloadApi {
        schema_version: 1,
        source_file_name: source_filename.clone(),
        source_file_size: source_size,
        parser_version: super::excel_import::EXCEL_IMPORT_PARSER_VERSION.to_owned(),
        import_session_id: Some(session_id.clone()),
        source_item_count: Some(analysis.source_item_count),
        omitted_item_count: analysis.omitted_item_count,
        active_item_id: None,
        items: Vec::new(),
    };
    let payload_json = serde_json::to_string(&payload).map_err(CommandError::database)?;
    let diagnostics_json =
        serde_json::to_string(&analysis.warnings).map_err(CommandError::database)?;
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;
    let existing: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM drafts WHERE draft_key = ?")
        .bind(WORD_IMPORT_DRAFT_KEY)
        .fetch_one(&mut *transaction)
        .await
        .map_err(CommandError::database)?;
    if existing != 0 {
        return Err(CommandError::new(
            "WORD_IMPORT_DRAFT_EXISTS",
            "已有一份未完成的批量导入草稿，请先恢复或放弃旧草稿。",
        ));
    }
    sqlx::query(
        "INSERT INTO drafts (id, draft_key, draft_kind, target_question_id, \
             base_content_version, payload_schema_version, payload_json, source_title, \
             autosaved_at_ms, created_at_ms, updated_at_ms) \
         VALUES (?, ?, ?, NULL, NULL, 1, ?, ?, ?, ?, ?)",
    )
    .bind(&draft_id)
    .bind(WORD_IMPORT_DRAFT_KEY)
    .bind(WORD_IMPORT_DRAFT_KIND)
    .bind(&payload_json)
    .bind(&source_filename)
    .bind(now)
    .bind(now)
    .bind(now)
    .execute(&mut *transaction)
    .await
    .map_err(CommandError::database)?;
    sqlx::query(
        "INSERT INTO word_import_sessions (id, draft_id, source_filename, source_sha256, \
             source_byte_size, parser_version, status, diagnostics_json, created_at_ms, updated_at_ms) \
         VALUES (?, ?, ?, ?, ?, ?, 'reviewing', ?, ?, ?)",
    )
    .bind(&session_id)
    .bind(&draft_id)
    .bind(&source_filename)
    .bind(source_sha256)
    .bind(i64::try_from(source_size).unwrap_or(i64::MAX))
    .bind(super::excel_import::EXCEL_IMPORT_PARSER_VERSION)
    .bind(diagnostics_json)
    .bind(now)
    .bind(now)
    .execute(&mut *transaction)
    .await
    .map_err(CommandError::database)?;
    transaction.commit().await.map_err(CommandError::database)?;

    Ok(BeginExcelImportResultApi {
        analysis,
        draft: WordImportDraftRecordApi {
            id: draft_id,
            payload,
            autosaved_at: now,
            created_at: now,
            updated_at: now,
        },
    })
}

async fn prepare_resources(
    pool: &SqlitePool,
    paths: &DatabasePaths,
    extracted: Vec<ExtractedImageOccurrence>,
) -> CommandResult<Vec<PreparedOccurrence>> {
    let mut by_hash: HashMap<Vec<u8>, NewResource> = HashMap::new();
    let mut occurrence_hashes = Vec::with_capacity(extracted.len());
    for mut occurrence in extracted {
        let sha256 = Sha256::digest(&occurrence.bytes).to_vec();
        let sha256_hex = hex_lower(&sha256);
        let byte_size = u64::try_from(occurrence.bytes.len()).unwrap_or(u64::MAX);
        occurrence.byte_size = byte_size;
        let resource_bytes = std::mem::take(&mut occurrence.bytes);
        by_hash.entry(sha256.clone()).or_insert_with(|| {
            let extension = match occurrence.mime_type.as_str() {
                "image/png" => "png",
                "image/jpeg" => "jpg",
                _ => "bin",
            };
            NewResource {
                id: Uuid::now_v7().to_string(),
                sha256: sha256.clone(),
                sha256_hex: sha256_hex.clone(),
                mime_type: occurrence.mime_type.clone(),
                storage_rel_path: format!(
                    "images/{}/{}.{}",
                    &sha256_hex[..2],
                    sha256_hex,
                    extension
                ),
                original_filename: occurrence.original_filename.clone(),
                byte_size,
                width_px: occurrence.width_px,
                height_px: occurrence.height_px,
                bytes: resource_bytes,
            }
        });
        occurrence_hashes.push((occurrence, sha256));
    }

    let mut ids_by_hash = HashMap::<Vec<u8>, String>::new();
    let mut new_resources = Vec::new();
    for (hash, resource) in by_hash {
        let existing = sqlx::query_as::<_, ResourceRow>(
            "SELECT id, sha256, resource_kind, mime_type, storage_rel_path, byte_size, \
                    intrinsic_width_px, intrinsic_height_px, availability_status \
             FROM resources WHERE sha256 = ?",
        )
        .bind(&hash)
        .fetch_optional(pool)
        .await
        .map_err(CommandError::database)?;
        if let Some(existing) = existing {
            validate_existing_resource(paths, &existing, &resource)?;
            ids_by_hash.insert(hash, existing.id);
        } else {
            ids_by_hash.insert(hash, resource.id.clone());
            new_resources.push(resource);
        }
    }

    commit_new_resource_files(paths, &new_resources)?;
    let now = now_millis();
    let mut transaction = pool.begin().await.map_err(CommandError::database)?;
    for resource in &new_resources {
        sqlx::query(
            "INSERT INTO resources (id, sha256, resource_kind, mime_type, storage_rel_path, \
                 original_filename, byte_size, intrinsic_width_px, intrinsic_height_px, \
                 availability_status, created_at_ms, last_verified_at_ms) \
             VALUES (?, ?, 'image', ?, ?, ?, ?, ?, ?, 'ready', ?, ?)",
        )
        .bind(&resource.id)
        .bind(&resource.sha256)
        .bind(&resource.mime_type)
        .bind(&resource.storage_rel_path)
        .bind(&resource.original_filename)
        .bind(i64::try_from(resource.byte_size).unwrap_or(i64::MAX))
        .bind(i64::from(resource.width_px))
        .bind(i64::from(resource.height_px))
        .bind(now)
        .bind(now)
        .execute(&mut *transaction)
        .await
        .map_err(CommandError::database)?;
    }
    transaction.commit().await.map_err(CommandError::database)?;

    occurrence_hashes
        .into_iter()
        .map(|(occurrence, hash)| {
            let resource_id = ids_by_hash
                .get(&hash)
                .cloned()
                .ok_or_else(|| CommandError::database("导入图片的内容哈希没有对应资源记录。"))?;
            Ok(PreparedOccurrence {
                node_id: Uuid::now_v7().to_string(),
                resource_id,
                paragraph_index: occurrence.paragraph_index,
                text_char_offset: occurrence.text_char_offset,
                original_filename: occurrence.original_filename,
                mime_type: occurrence.mime_type,
                byte_size: occurrence.byte_size,
                width_px: occurrence.width_px,
                height_px: occurrence.height_px,
            })
        })
        .collect()
}

fn validate_existing_resource(
    paths: &DatabasePaths,
    existing: &ResourceRow,
    expected: &NewResource,
) -> CommandResult<()> {
    if existing.sha256 != expected.sha256
        || existing.resource_kind != "image"
        || existing.mime_type != expected.mime_type
        || existing.byte_size != i64::try_from(expected.byte_size).unwrap_or(i64::MAX)
        || existing.intrinsic_width_px != Some(i64::from(expected.width_px))
        || existing.intrinsic_height_px != Some(i64::from(expected.height_px))
        || existing.availability_status != "ready"
    {
        return Err(CommandError::new(
            "RESOURCE_METADATA_CONFLICT",
            "相同图片哈希对应的本地资源记录不一致，已停止导入以避免覆盖。",
        ));
    }
    verify_managed_resource_file(
        paths.resources_dir(),
        &existing.storage_rel_path,
        &existing.sha256,
        expected.byte_size,
    )?;
    Ok(())
}

fn commit_new_resource_files(
    paths: &DatabasePaths,
    resources: &[NewResource],
) -> CommandResult<()> {
    if resources.is_empty() {
        return Ok(());
    }
    let resources_root = canonical_directory(paths.resources_dir(), "资源目录")?;
    let staging_root = canonical_directory(paths.resource_staging_dir(), "资源暂存目录")?;
    if !staging_root.starts_with(&resources_root) {
        return Err(CommandError::new(
            "RESOURCE_STAGING_UNSAFE",
            "资源暂存目录不在受管资源目录内。",
        ));
    }
    let operation_dir = staging_root.join(Uuid::now_v7().to_string());
    fs::create_dir(&operation_dir).map_err(|error| {
        CommandError::new(
            "RESOURCE_STAGING_CREATE_FAILED",
            format!("无法创建图片暂存目录：{error}"),
        )
    })?;
    let mut cleanup = StagingCleanup {
        path: operation_dir.clone(),
        active: true,
    };

    let images_dir = ensure_child_directory(&resources_root, "images")?;
    for resource in resources {
        let prefix_dir = ensure_child_directory(&images_dir, &resource.sha256_hex[..2])?;
        let extension = if resource.mime_type == "image/png" {
            "png"
        } else {
            "jpg"
        };
        let final_path = prefix_dir.join(format!("{}.{}", resource.sha256_hex, extension));
        if path_exists(&final_path)? {
            verify_file(&final_path, &resource.sha256, resource.byte_size)?;
            continue;
        }
        let staged = operation_dir.join(format!("{}.partial", resource.sha256_hex));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged)
            .map_err(|error| {
                CommandError::new(
                    "RESOURCE_STAGING_WRITE_FAILED",
                    format!("无法创建图片暂存文件：{error}"),
                )
            })?;
        file.write_all(&resource.bytes).map_err(|error| {
            CommandError::new(
                "RESOURCE_STAGING_WRITE_FAILED",
                format!("无法写入图片暂存文件：{error}"),
            )
        })?;
        file.sync_all().map_err(|error| {
            CommandError::new(
                "RESOURCE_STAGING_FLUSH_FAILED",
                format!("无法把图片暂存文件完整写入磁盘：{error}"),
            )
        })?;
        drop(file);
        verify_file(&staged, &resource.sha256, resource.byte_size)?;
        fs::rename(&staged, &final_path).map_err(|error| {
            CommandError::new(
                "RESOURCE_COMMIT_FAILED",
                format!("无法把图片提交到受管资源目录：{error}"),
            )
        })?;
        verify_file(&final_path, &resource.sha256, resource.byte_size)?;
    }
    fs::remove_dir(&operation_dir).map_err(|error| {
        CommandError::new(
            "RESOURCE_STAGING_CLEANUP_FAILED",
            format!("图片已保存，但暂存目录无法清理：{error}"),
        )
    })?;
    cleanup.active = false;
    Ok(())
}

pub(crate) async fn get_managed_image(
    pool: &SqlitePool,
    paths: &DatabasePaths,
    resource_id: &str,
) -> CommandResult<ManagedImagePayloadApi> {
    let resource_id = canonical_uuid(resource_id, "图片资源 ID")?;
    let row = sqlx::query_as::<_, ResourceRow>(
        "SELECT id, sha256, resource_kind, mime_type, storage_rel_path, byte_size, \
                intrinsic_width_px, intrinsic_height_px, availability_status \
         FROM resources WHERE id = ?",
    )
    .bind(&resource_id)
    .fetch_optional(pool)
    .await
    .map_err(CommandError::database)?
    .ok_or_else(|| CommandError::new("RESOURCE_NOT_FOUND", "图片资源已经不存在。"))?;
    if row.resource_kind != "image"
        || !matches!(row.mime_type.as_str(), "image/png" | "image/jpeg")
        || row.availability_status != "ready"
        || row.byte_size < 0
    {
        return Err(CommandError::new(
            "RESOURCE_NOT_READY",
            "图片资源当前不可读取。",
        ));
    }
    let bytes = read_managed_resource_file(
        paths.resources_dir(),
        &row.storage_rel_path,
        &row.sha256,
        u64::try_from(row.byte_size).unwrap_or(u64::MAX),
    )?;
    Ok(ManagedImagePayloadApi {
        resource_id,
        mime_type: row.mime_type,
        data_base64: encode_base64(&bytes),
        width_px: row.intrinsic_width_px,
        height_px: row.intrinsic_height_px,
    })
}

pub(crate) async fn store_managed_images(
    pool: &SqlitePool,
    paths: &DatabasePaths,
    request: StoreManagedImagesRequestApi,
) -> CommandResult<Vec<ManagedImagePayloadApi>> {
    if request.images.is_empty() || request.images.len() > MAX_MANAGED_IMAGE_INPUTS {
        return Err(CommandError::validation(format!(
            "一次必须保存 1 至 {MAX_MANAGED_IMAGE_INPUTS} 张图片。"
        )));
    }

    let mut total_bytes = 0usize;
    let mut extracted = Vec::with_capacity(request.images.len());
    for (index, image) in request.images.into_iter().enumerate() {
        let bytes = STANDARD.decode(image.data_base64.trim()).map_err(|_| {
            CommandError::validation(format!("第 {} 张图片的 Base64 数据无效。", index + 1))
        })?;
        if bytes.is_empty() || bytes.len() > MAX_MANAGED_IMAGE_BYTES {
            return Err(CommandError::validation(format!(
                "第 {} 张图片为空或超过 8 MB。",
                index + 1
            )));
        }
        total_bytes = total_bytes.saturating_add(bytes.len());
        if total_bytes > MAX_MANAGED_IMAGE_TOTAL_BYTES {
            return Err(CommandError::validation("本次图片总大小超过 32 MB。"));
        }
        let (mime_type, width_px, height_px) = inspect_raster_image(&bytes).map_err(|error| {
            CommandError::new(
                "MANAGED_IMAGE_INVALID",
                format!("第 {} 张图片不是有效的 PNG/JPEG：{error}", index + 1),
            )
        })?;
        let original_filename = normalized_image_filename(image.original_filename.as_deref())?;
        extracted.push(ExtractedImageOccurrence {
            paragraph_index: index,
            text_char_offset: 0,
            relationship_id: format!("manual-image-{}", index + 1),
            original_filename,
            mime_type: mime_type.to_owned(),
            byte_size: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            width_px,
            height_px,
            bytes,
        });
    }

    let prepared = prepare_resources(pool, paths, extracted).await?;
    let mut payloads = Vec::with_capacity(prepared.len());
    for occurrence in prepared {
        payloads.push(get_managed_image(pool, paths, &occurrence.resource_id).await?);
    }
    Ok(payloads)
}

fn normalized_image_filename(value: Option<&str>) -> CommandResult<Option<String>> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if value.chars().count() > 255
        || value.chars().any(char::is_control)
        || Path::new(value).file_name().and_then(|name| name.to_str()) != Some(value)
        || value.contains('/')
        || value.contains('\\')
    {
        return Err(CommandError::validation("图片原始文件名无效。"));
    }
    Ok(Some(value.to_owned()))
}

pub(crate) fn read_wps_clipboard_images(
    request: ReadWpsClipboardImagesRequestApi,
) -> CommandResult<Vec<WpsClipboardImagePayloadApi>> {
    if request.paths.is_empty() || request.paths.len() > MAX_WPS_CLIPBOARD_IMAGES {
        return Err(CommandError::validation(format!(
            "一次最多读取 {MAX_WPS_CLIPBOARD_IMAGES} 个 WPS 剪贴板图片。"
        )));
    }
    let temp_root = fs::canonicalize(std::env::temp_dir()).map_err(|error| {
        CommandError::new(
            "WPS_CLIPBOARD_TEMP_UNAVAILABLE",
            format!("无法确认系统临时目录：{error}"),
        )
    })?;
    let mut seen = HashSet::new();
    let mut total_bytes = 0u64;
    let mut output = Vec::with_capacity(request.paths.len());
    for source_path in request.paths {
        if !seen.insert(source_path.to_ascii_lowercase()) {
            return Err(CommandError::validation("WPS 剪贴板图片路径不能重复。"));
        }
        let canonical = canonical_wps_clipboard_image(&temp_root, &source_path)?;
        let metadata = fs::metadata(&canonical).map_err(|error| {
            CommandError::new(
                "WPS_CLIPBOARD_IMAGE_UNREADABLE",
                format!("无法读取 WPS 剪贴板图片：{error}"),
            )
        })?;
        if !metadata.is_file() || metadata.len() > MAX_WPS_CLIPBOARD_IMAGE_BYTES {
            return Err(CommandError::validation(
                "WPS 剪贴板图片不是普通文件或大小超过 4 MB。",
            ));
        }
        total_bytes = total_bytes.saturating_add(metadata.len());
        if total_bytes > MAX_WPS_CLIPBOARD_TOTAL_BYTES {
            return Err(CommandError::validation(
                "本次 WPS 剪贴板图片总大小超过 32 MB。",
            ));
        }
        let bytes = read_wps_clipboard_image_bounded(&canonical)?;
        let (mime_type, width_px, height_px) = inspect_raster_image(&bytes).map_err(|error| {
            CommandError::new(
                "WPS_CLIPBOARD_IMAGE_INVALID",
                format!("WPS 剪贴板图片不是有效的 PNG/JPEG：{error}"),
            )
        })?;
        output.push(WpsClipboardImagePayloadApi {
            path: source_path,
            mime_type: mime_type.to_owned(),
            data_base64: encode_base64(&bytes),
            width_px,
            height_px,
        });
    }
    Ok(output)
}

fn canonical_wps_clipboard_image(temp_root: &Path, source_path: &str) -> CommandResult<PathBuf> {
    let requested = PathBuf::from(source_path);
    if !requested.is_absolute() {
        return Err(CommandError::validation(
            "WPS 剪贴板图片路径必须是绝对路径。",
        ));
    }
    let canonical = fs::canonicalize(&requested).map_err(|error| {
        CommandError::new(
            "WPS_CLIPBOARD_IMAGE_MISSING",
            format!("WPS 剪贴板临时图片已经失效：{error}"),
        )
    })?;
    let parent = canonical
        .parent()
        .ok_or_else(|| CommandError::validation("WPS 剪贴板图片缺少父目录。"))?;
    if parent.parent() != Some(temp_root) {
        return Err(CommandError::validation(
            "只允许读取系统临时目录中的 WPS 剪贴板图片。",
        ));
    }
    let directory_name = parent
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let directory_suffix = directory_name
        .strip_prefix("ksohtml")
        .or_else(|| directory_name.strip_prefix("KSOHTML"));
    if !matches!(directory_suffix, Some(value) if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(CommandError::validation(
            "WPS 剪贴板图片不在受支持的 ksohtml 临时目录中。",
        ));
    }
    let file_name = canonical
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let Some((stem, extension)) = file_name.rsplit_once('.') else {
        return Err(CommandError::validation("WPS 剪贴板图片扩展名无效。"));
    };
    let numbered = stem.strip_prefix("wps").unwrap_or_default();
    if numbered.is_empty()
        || !numbered.bytes().all(|byte| byte.is_ascii_digit())
        || !matches!(extension, "png" | "jpg" | "jpeg")
    {
        return Err(CommandError::validation(
            "WPS 剪贴板图片文件名不符合安全规则。",
        ));
    }
    Ok(canonical)
}

fn read_wps_clipboard_image_bounded(path: &Path) -> CommandResult<Vec<u8>> {
    let file = File::open(path).map_err(|error| {
        CommandError::new(
            "WPS_CLIPBOARD_IMAGE_UNREADABLE",
            format!("无法打开 WPS 剪贴板图片：{error}"),
        )
    })?;
    let mut bytes = Vec::new();
    file.take(MAX_WPS_CLIPBOARD_IMAGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| {
            CommandError::new(
                "WPS_CLIPBOARD_IMAGE_UNREADABLE",
                format!("无法读取 WPS 剪贴板图片：{error}"),
            )
        })?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_WPS_CLIPBOARD_IMAGE_BYTES {
        return Err(CommandError::validation(
            "WPS 剪贴板图片读取过程中超过 4 MB。",
        ));
    }
    Ok(bytes)
}

pub(crate) async fn validate_resource_refs(
    connection: &mut SqliteConnection,
    refs: &[QuestionResourceRefApi],
    option_ids: &HashSet<String>,
) -> CommandResult<()> {
    validate_resource_ref_shape(refs, option_ids)?;
    for item in refs {
        let resource_id = canonical_uuid(&item.resource_id, "图片资源 ID")?;
        let status: Option<(String, String)> = sqlx::query_as::<_, (String, String)>(
            "SELECT resource_kind, availability_status FROM resources WHERE id = ?",
        )
        .bind(&resource_id)
        .fetch_optional(&mut *connection)
        .await
        .map_err(CommandError::database)?;
        if !matches!(status.as_ref(), Some((kind, availability)) if kind == "image" && availability == "ready")
        {
            return Err(CommandError::validation(
                "题目引用了不存在或尚未就绪的图片资源。",
            ));
        }
    }
    Ok(())
}

#[derive(Clone, Debug)]
struct ManagedImageNode {
    resource_id: String,
    node_id: Option<String>,
}

fn managed_image_nodes(content: &RichContentApi) -> CommandResult<Vec<ManagedImageNode>> {
    let fragment = Html::parse_fragment(&content.html);
    let selector = Selector::parse("img[data-resource-id]")
        .map_err(|_| CommandError::database("无法建立受管图片校验规则。"))?;
    fragment
        .select(&selector)
        .map(|image| {
            let resource_id = canonical_uuid(
                image
                    .value()
                    .attr("data-resource-id")
                    .unwrap_or_default()
                    .trim(),
                "图片资源 ID",
            )?;
            let node_id = image
                .value()
                .attr("data-node-id")
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| canonical_uuid(value, "图片节点 ID"))
                .transpose()?;
            Ok(ManagedImageNode {
                resource_id,
                node_id,
            })
        })
        .collect()
}

pub(crate) fn managed_image_resource_ids(content: &RichContentApi) -> CommandResult<Vec<String>> {
    managed_image_nodes(content)
        .map(|nodes| nodes.into_iter().map(|node| node.resource_id).collect())
}

pub(crate) fn reconcile_question_resource_refs(
    stem: &RichContentApi,
    options: &[QuestionOptionApi],
    answer: &RichContentApi,
    explanation: &RichContentApi,
    refs: &[QuestionResourceRefApi],
) -> CommandResult<Vec<QuestionResourceRefApi>> {
    let mut reconciled = Vec::new();
    let mut consumed = HashSet::<usize>::new();
    let mut inspect_slot = |content: &RichContentApi,
                            content_slot: &str,
                            option_id: Option<&str>|
     -> CommandResult<()> {
        for image in managed_image_nodes(content)? {
            let matches_location = |reference: &QuestionResourceRefApi| {
                reference.resource_id == image.resource_id
                    && reference.content_slot == content_slot
                    && reference.option_id.as_deref() == option_id
            };
            let exact = image.node_id.as_deref().and_then(|node_id| {
                refs.iter().enumerate().find_map(|(index, reference)| {
                    (!consumed.contains(&index)
                        && reference.node_id == node_id
                        && matches_location(reference))
                    .then_some(index)
                })
            });
            let fallback = exact.or_else(|| {
                refs.iter().enumerate().find_map(|(index, reference)| {
                    (!consumed.contains(&index) && matches_location(reference)).then_some(index)
                })
            });
            if let Some(index) = fallback {
                consumed.insert(index);
                reconciled.push(refs[index].clone());
            } else {
                reconciled.push(QuestionResourceRefApi {
                    node_id: image.node_id.unwrap_or_else(|| Uuid::now_v7().to_string()),
                    resource_id: image.resource_id,
                    content_slot: content_slot.to_owned(),
                    option_id: option_id.map(str::to_owned),
                });
            }
        }
        Ok(())
    };

    inspect_slot(stem, "stem", None)?;
    for option in options {
        inspect_slot(&option.content, "option", Some(&option.id))?;
    }
    inspect_slot(answer, "answer", None)?;
    inspect_slot(explanation, "explanation", None)?;
    Ok(reconciled)
}

pub(crate) fn validate_question_resource_ref_consistency(
    stem: &RichContentApi,
    options: &[QuestionOptionApi],
    answer: &RichContentApi,
    explanation: &RichContentApi,
    refs: &[QuestionResourceRefApi],
) -> CommandResult<()> {
    type ResourceKey = (String, Option<String>, String);
    let mut expected = HashMap::<ResourceKey, usize>::new();
    let mut inspect_slot = |content: &RichContentApi,
                            content_slot: &str,
                            option_id: Option<&str>|
     -> CommandResult<()> {
        for image in managed_image_nodes(content)? {
            let key = (
                content_slot.to_owned(),
                option_id.map(str::to_owned),
                image.resource_id.clone(),
            );
            *expected.entry(key).or_default() += 1;
            if let Some(node_id) = image.node_id
                && !refs.iter().any(|reference| {
                    reference.node_id == node_id
                        && reference.resource_id == image.resource_id
                        && reference.content_slot == content_slot
                        && reference.option_id.as_deref() == option_id
                })
            {
                return Err(CommandError::new(
                    "QUESTION_RESOURCE_RELATIONSHIP_MISMATCH",
                    "题目图片节点与图片资源关联不一致，请重新打开题目后再保存。",
                ));
            }
        }
        Ok(())
    };

    inspect_slot(stem, "stem", None)?;
    for option in options {
        inspect_slot(&option.content, "option", Some(&option.id))?;
    }
    inspect_slot(answer, "answer", None)?;
    inspect_slot(explanation, "explanation", None)?;

    let mut actual = HashMap::<ResourceKey, usize>::new();
    for reference in refs {
        let key = (
            reference.content_slot.clone(),
            reference.option_id.clone(),
            reference.resource_id.clone(),
        );
        *actual.entry(key).or_default() += 1;
    }
    if expected != actual {
        return Err(CommandError::new(
            "QUESTION_RESOURCE_RELATIONSHIP_MISMATCH",
            "题目正文中的图片与保存的图片关联数量不一致，请重新打开题目后再保存。",
        ));
    }
    Ok(())
}

pub(crate) fn validate_resource_ref_shape(
    refs: &[QuestionResourceRefApi],
    option_ids: &HashSet<String>,
) -> CommandResult<()> {
    if refs.len() > MAX_RESOURCE_REFS_PER_QUESTION {
        return Err(CommandError::validation(format!(
            "一道题最多允许 {MAX_RESOURCE_REFS_PER_QUESTION} 个图片资源节点。"
        )));
    }
    let mut node_ids = HashSet::with_capacity(refs.len());
    for item in refs {
        let node_id = canonical_uuid(&item.node_id, "图片节点 ID")?;
        let resource_id = canonical_uuid(&item.resource_id, "图片资源 ID")?;
        if node_id != item.node_id || resource_id != item.resource_id {
            return Err(CommandError::validation(
                "图片节点和资源 ID 必须使用规范 UUID 格式。",
            ));
        }
        if !node_ids.insert(item.node_id.as_str()) {
            return Err(CommandError::validation(
                "同一道题不能包含重复的图片节点 ID。",
            ));
        }
        match item.content_slot.as_str() {
            "stem" | "answer" | "explanation" if item.option_id.is_none() => {}
            "option" => {
                let option_id = item
                    .option_id
                    .as_deref()
                    .ok_or_else(|| CommandError::validation("选项图片必须关联具体选项。"))?;
                let option_id = canonical_uuid(option_id, "图片关联选项 ID")?;
                if item.option_id.as_deref() != Some(option_id.as_str()) {
                    return Err(CommandError::validation(
                        "图片关联选项 ID 必须使用规范 UUID 格式。",
                    ));
                }
                if !option_ids.contains(&option_id) {
                    return Err(CommandError::validation("图片引用的选项不属于当前题目。"));
                }
            }
            _ => {
                return Err(CommandError::validation(
                    "图片内容位置必须是题干、选项、答案或解析，且关联信息要匹配。",
                ));
            }
        }
    }
    Ok(())
}

pub(crate) async fn replace_question_resource_refs(
    connection: &mut SqliteConnection,
    question_id: &str,
    refs: &[QuestionResourceRefApi],
    now: i64,
) -> CommandResult<()> {
    sqlx::query("DELETE FROM question_resource_refs WHERE question_id = ?")
        .bind(question_id)
        .execute(&mut *connection)
        .await
        .map_err(CommandError::database)?;
    for item in refs {
        sqlx::query(
            "INSERT INTO question_resource_refs (id, question_id, option_id, resource_id, \
                 content_slot, node_id, created_at_ms) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(Uuid::now_v7().to_string())
        .bind(question_id)
        .bind(item.option_id.as_deref())
        .bind(&item.resource_id)
        .bind(&item.content_slot)
        .bind(&item.node_id)
        .bind(now)
        .execute(&mut *connection)
        .await
        .map_err(CommandError::database)?;
    }
    Ok(())
}

pub(crate) async fn sync_draft_resource_refs(
    connection: &mut SqliteConnection,
    draft_id: &str,
    refs: &[QuestionResourceRefApi],
    now: i64,
) -> CommandResult<()> {
    sqlx::query("DELETE FROM draft_resource_refs WHERE draft_id = ?")
        .bind(draft_id)
        .execute(&mut *connection)
        .await
        .map_err(CommandError::database)?;
    for item in refs {
        sqlx::query(
            "INSERT INTO draft_resource_refs (id, draft_id, resource_id, node_id, created_at_ms) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(Uuid::now_v7().to_string())
        .bind(draft_id)
        .bind(&item.resource_id)
        .bind(&item.node_id)
        .bind(now)
        .execute(&mut *connection)
        .await
        .map_err(CommandError::database)?;
    }
    Ok(())
}

fn verify_managed_resource_file(
    resources_root: &Path,
    rel_path: &str,
    expected_sha256: &[u8],
    expected_size: u64,
) -> CommandResult<()> {
    read_managed_resource_file(resources_root, rel_path, expected_sha256, expected_size).map(|_| ())
}

pub(crate) fn read_managed_resource_file(
    resources_root: &Path,
    rel_path: &str,
    expected_sha256: &[u8],
    expected_size: u64,
) -> CommandResult<Vec<u8>> {
    validate_relative_resource_path(rel_path)?;
    let root = canonical_directory(resources_root, "资源目录")?;
    let candidate = root.join(rel_path.replace('/', std::path::MAIN_SEPARATOR_STR));
    let canonical = fs::canonicalize(&candidate).map_err(|error| {
        CommandError::new(
            "RESOURCE_FILE_UNAVAILABLE",
            format!("图片资源文件无法访问：{error}"),
        )
    })?;
    if !canonical.starts_with(&root) {
        return Err(CommandError::new(
            "RESOURCE_PATH_UNSAFE",
            "图片资源路径离开了受管资源目录。",
        ));
    }
    let bytes = read_file_bounded(&canonical, expected_size)?;
    if Sha256::digest(&bytes).as_slice() != expected_sha256 {
        return Err(CommandError::new(
            "RESOURCE_FILE_HASH_MISMATCH",
            "图片资源文件与数据库记录不一致。",
        ));
    }
    Ok(bytes)
}

fn verify_file(path: &Path, expected_sha256: &[u8], expected_size: u64) -> CommandResult<()> {
    let bytes = read_file_bounded(path, expected_size)?;
    if Sha256::digest(&bytes).as_slice() != expected_sha256 {
        return Err(CommandError::new(
            "RESOURCE_FILE_HASH_MISMATCH",
            "图片资源文件哈希校验失败。",
        ));
    }
    Ok(())
}

fn read_file_bounded(path: &Path, expected_size: u64) -> CommandResult<Vec<u8>> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        CommandError::new(
            "RESOURCE_FILE_UNAVAILABLE",
            format!("无法读取图片资源信息：{error}"),
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() != expected_size {
        return Err(CommandError::new(
            "RESOURCE_FILE_INVALID",
            "图片资源不是安全的普通文件，或文件大小与数据库不一致。",
        ));
    }
    let mut file = File::open(path).map_err(|error| {
        CommandError::new(
            "RESOURCE_FILE_UNAVAILABLE",
            format!("无法打开图片资源：{error}"),
        )
    })?;
    let capacity = usize::try_from(expected_size).map_err(|_| {
        CommandError::new("RESOURCE_FILE_TOO_LARGE", "图片资源大小超出当前系统限制。")
    })?;
    let mut bytes = Vec::with_capacity(capacity);
    Read::by_ref(&mut file)
        .take(expected_size.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| {
            CommandError::new(
                "RESOURCE_FILE_UNAVAILABLE",
                format!("读取图片资源失败：{error}"),
            )
        })?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) != expected_size {
        return Err(CommandError::new(
            "RESOURCE_FILE_SIZE_MISMATCH",
            "图片资源读取大小与数据库记录不一致。",
        ));
    }
    Ok(bytes)
}

fn validate_relative_resource_path(value: &str) -> CommandResult<()> {
    if value.is_empty()
        || value.contains('\\')
        || value.contains(':')
        || value.starts_with('/')
        || Path::new(value)
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(CommandError::new(
            "RESOURCE_PATH_UNSAFE",
            "数据库中的图片资源路径不安全。",
        ));
    }
    Ok(())
}

fn canonical_directory(path: &Path, label: &str) -> CommandResult<PathBuf> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        CommandError::new(
            "RESOURCE_DIRECTORY_UNAVAILABLE",
            format!("无法检查{label}：{error}"),
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(CommandError::new(
            "RESOURCE_DIRECTORY_UNSAFE",
            format!("{label}不是安全的真实目录。"),
        ));
    }
    fs::canonicalize(path).map_err(|error| {
        CommandError::new(
            "RESOURCE_DIRECTORY_UNAVAILABLE",
            format!("无法解析{label}：{error}"),
        )
    })
}

fn ensure_child_directory(parent: &Path, name: &str) -> CommandResult<PathBuf> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || matches!(name, "." | "..") {
        return Err(CommandError::new(
            "RESOURCE_DIRECTORY_UNSAFE",
            "受管资源子目录名称无效。",
        ));
    }
    let path = parent.join(name);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(CommandError::new(
                "RESOURCE_DIRECTORY_UNSAFE",
                "受管资源子目录不是安全的真实目录。",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(&path).map_err(|error| {
                CommandError::new(
                    "RESOURCE_DIRECTORY_CREATE_FAILED",
                    format!("无法创建受管资源子目录：{error}"),
                )
            })?;
        }
        Err(error) => {
            return Err(CommandError::new(
                "RESOURCE_DIRECTORY_UNAVAILABLE",
                format!("无法检查受管资源子目录：{error}"),
            ));
        }
    }
    let canonical = fs::canonicalize(&path).map_err(|error| {
        CommandError::new(
            "RESOURCE_DIRECTORY_UNAVAILABLE",
            format!("无法解析受管资源子目录：{error}"),
        )
    })?;
    if !canonical.starts_with(parent) {
        return Err(CommandError::new(
            "RESOURCE_DIRECTORY_UNSAFE",
            "受管资源子目录离开了资源根目录。",
        ));
    }
    Ok(canonical)
}

fn path_exists(path: &Path) -> CommandResult<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(CommandError::new(
            "RESOURCE_PATH_CHECK_FAILED",
            format!("无法检查图片资源路径：{error}"),
        )),
    }
}

fn canonical_uuid(value: &str, label: &str) -> CommandResult<String> {
    if value.trim() != value {
        return Err(CommandError::validation(format!(
            "{label}前后不能包含空白字符。"
        )));
    }
    Uuid::parse_str(value)
        .map(|id| id.to_string())
        .map_err(|_| CommandError::validation(format!("{label}无效。")))
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        result.push(char::from(HEX[usize::from(byte >> 4)]));
        result.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    result
}

fn encode_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3).saturating_mul(4));
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        output.push(char::from(TABLE[usize::from(first >> 2)]));
        output.push(char::from(
            TABLE[usize::from(((first & 0x03) << 4) | (second >> 4))],
        ));
        if chunk.len() > 1 {
            output.push(char::from(
                TABLE[usize::from(((second & 0x0f) << 2) | (third >> 6))],
            ));
        } else {
            output.push('=');
        }
        if chunk.len() > 2 {
            output.push(char::from(TABLE[usize::from(third & 0x3f)]));
        } else {
            output.push('=');
        }
    }
    output
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
    use crate::db::Database;

    fn rich(html: &str) -> RichContentApi {
        RichContentApi {
            schema_version: 1,
            editor: None,
            editor_version: None,
            document: None,
            html: html.to_owned(),
            plain_text: String::new(),
            source_ooxml: None,
        }
    }

    fn test_png(width: u32, height: u32) -> Vec<u8> {
        fn chunk(output: &mut Vec<u8>, kind: &[u8; 4], payload: &[u8]) {
            output.extend_from_slice(&(payload.len() as u32).to_be_bytes());
            output.extend_from_slice(kind);
            output.extend_from_slice(payload);
            output.extend_from_slice(&[0; 4]);
        }
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        chunk(
            &mut bytes,
            b"IHDR",
            &[
                width.to_be_bytes().as_slice(),
                height.to_be_bytes().as_slice(),
                &[8, 2, 0, 0, 0],
            ]
            .concat(),
        );
        chunk(&mut bytes, b"IDAT", &[0]);
        chunk(&mut bytes, b"IEND", &[]);
        bytes
    }

    #[test]
    fn base64_encoder_handles_padding() {
        assert_eq!(encode_base64(b""), "");
        assert_eq!(encode_base64(b"f"), "Zg==");
        assert_eq!(encode_base64(b"fo"), "Zm8=");
        assert_eq!(encode_base64(b"foo"), "Zm9v");
        assert_eq!(encode_base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn resource_paths_reject_escape_and_windows_separators() {
        assert!(validate_relative_resource_path("images/ab/hash.png").is_ok());
        assert!(validate_relative_resource_path("../outside.png").is_err());
        assert!(validate_relative_resource_path("images\\outside.png").is_err());
        assert!(validate_relative_resource_path("C:/outside.png").is_err());
    }

    #[test]
    fn resource_consistency_accepts_legacy_html_without_node_id() {
        let node_id = Uuid::now_v7().to_string();
        let resource_id = Uuid::now_v7().to_string();
        let stem = rich(&format!(
            r#"<p><img data-resource-id="{resource_id}" width="458"></p>"#
        ));
        let refs = vec![QuestionResourceRefApi {
            node_id,
            resource_id,
            content_slot: "stem".to_owned(),
            option_id: None,
        }];

        assert!(
            validate_question_resource_ref_consistency(&stem, &[], &rich(""), &rich(""), &refs,)
                .is_ok()
        );
    }

    #[test]
    fn resource_reconciliation_rebuilds_a_missing_legacy_reference() {
        let resource_id = Uuid::now_v7().to_string();
        let stem = rich(&format!(
            r#"<p><img data-resource-id="{resource_id}" width="458"></p>"#
        ));

        let reconciled =
            reconcile_question_resource_refs(&stem, &[], &rich(""), &rich(""), &[]).unwrap();
        assert_eq!(reconciled.len(), 1);
        assert_eq!(reconciled[0].resource_id, resource_id);
        assert_eq!(reconciled[0].content_slot, "stem");
        assert!(Uuid::parse_str(&reconciled[0].node_id).is_ok());
    }

    #[test]
    fn resource_consistency_rejects_an_html_image_without_a_reference() {
        let resource_id = Uuid::now_v7().to_string();
        let stem = rich(&format!(r#"<p><img data-resource-id="{resource_id}"></p>"#));

        let error =
            validate_question_resource_ref_consistency(&stem, &[], &rich(""), &rich(""), &[])
                .expect_err("unrelated HTML and reference lists must be rejected");
        assert_eq!(error.code, "QUESTION_RESOURCE_RELATIONSHIP_MISMATCH");
    }

    #[test]
    fn resource_consistency_rejects_a_reference_in_the_wrong_slot() {
        let node_id = Uuid::now_v7().to_string();
        let resource_id = Uuid::now_v7().to_string();
        let stem = rich(&format!(
            r#"<p><img data-resource-id="{resource_id}" data-node-id="{node_id}"></p>"#
        ));
        let refs = vec![QuestionResourceRefApi {
            node_id,
            resource_id,
            content_slot: "answer".to_owned(),
            option_id: None,
        }];

        let error =
            validate_question_resource_ref_consistency(&stem, &[], &rich(""), &rich(""), &refs)
                .expect_err("the slot must match the HTML location");
        assert_eq!(error.code, "QUESTION_RESOURCE_RELATIONSHIP_MISMATCH");
    }

    #[tokio::test]
    async fn stores_identical_new_images_once_and_reuses_the_resource_id() {
        let root =
            std::env::temp_dir().join(format!("zhitiku-managed-image-{}", Uuid::now_v7().simple()));
        let database = Database::open(&root, "test").await.unwrap();
        let data_base64 = encode_base64(&test_png(32, 16));
        let payloads = store_managed_images(
            database.pool(),
            database.paths(),
            StoreManagedImagesRequestApi {
                images: vec![
                    super::super::models::StoreManagedImageInputApi {
                        data_base64: data_base64.clone(),
                        original_filename: Some("first.png".to_owned()),
                    },
                    super::super::models::StoreManagedImageInputApi {
                        data_base64,
                        original_filename: Some("copy.png".to_owned()),
                    },
                ],
            },
        )
        .await
        .unwrap();

        assert_eq!(payloads.len(), 2);
        assert_eq!(payloads[0].resource_id, payloads[1].resource_id);
        assert_eq!(payloads[0].mime_type, "image/png");
        assert_eq!(
            (payloads[0].width_px, payloads[0].height_px),
            (Some(32), Some(16))
        );
        let resource_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM resources")
            .fetch_one(database.pool())
            .await
            .unwrap();
        assert_eq!(resource_count, 1);

        database.close().await;
        drop(database);
        let retry_delays_ms = [20, 50, 100, 200, 400, 800, 1_000, 1_500];
        for (attempt, delay_ms) in retry_delays_ms.into_iter().enumerate() {
            match fs::remove_dir_all(&root) {
                Ok(()) => return,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
                Err(error)
                    if matches!(error.raw_os_error(), Some(32 | 33))
                        && attempt + 1 < retry_delays_ms.len() =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                }
                Err(error) => panic!("failed to clean managed image test directory: {error}"),
            }
        }
    }

    #[test]
    fn reads_only_valid_numbered_wps_images_from_the_system_temp_directory() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("ksohtml{unique}"));
        fs::create_dir(&directory).unwrap();
        let image_path = directory.join("wps1.png");
        fs::write(&image_path, test_png(30, 16)).unwrap();

        let payloads = read_wps_clipboard_images(ReadWpsClipboardImagesRequestApi {
            paths: vec![image_path.to_string_lossy().into_owned()],
        })
        .unwrap();
        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0].mime_type, "image/png");
        assert_eq!((payloads[0].width_px, payloads[0].height_px), (30, 16));
        assert!(!payloads[0].data_base64.is_empty());

        let outside = std::env::current_dir().unwrap().join("Cargo.toml");
        let error = read_wps_clipboard_images(ReadWpsClipboardImagesRequestApi {
            paths: vec![outside.to_string_lossy().into_owned()],
        })
        .expect_err("non-WPS paths must be rejected");
        assert_eq!(error.code, "VALIDATION_ERROR");

        fs::remove_file(image_path).unwrap();
        fs::remove_dir(directory).unwrap();
    }
}
