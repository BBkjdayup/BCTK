mod device;
mod model;

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, VerifyingKey};
use rand_core::{OsRng, RngCore};
use uuid::Uuid;

pub use device::device_id_for_public_key;
pub(crate) use device::{protect_for_current_user, unprotect_for_current_user};
pub use model::{
    ACTIVATION_REQUEST_SCHEMA_VERSION, ActivationRequestEnvelope, ActivationRequestPayload,
    BASIC_PAPER_QUESTION_LIMIT, DEFAULT_GRACE_DAYS, DEFAULT_TRIAL_DAYS,
    DESKTOP_PROFESSIONAL_EDITION, DesktopLicenseClaims, EffectiveCapabilities,
    LICENSE_SCHEMA_VERSION, LicenseFileResult, LicenseOverview, PRODUCT_CODE, PRODUCTION_KEY_ID,
    SignedEnvelope,
};

use device::{DEVICE_KEY_FILENAME, DeviceIdentity};
use model::{CloudSubscriptionStatus, DesktopLicenseStatus};

const LICENSE_FILENAME: &str = "desktop-license.tklic";
const CLOCK_FILENAME: &str = "trusted-clock.dpapi";
const TRIAL_FILENAME: &str = "trial-record.dpapi";
const SECURITY_STATE_MARKER_FILENAME: &str = "security-state-v1";
const SECURITY_STATE_MARKER_CONTENT: &[u8] = b"com.zhitiku.desktop:security-state:v1";
const SECURITY_STATE_FILENAMES: [&str; 3] = [DEVICE_KEY_FILENAME, CLOCK_FILENAME, TRIAL_FILENAME];
const MAX_LICENSE_BYTES: u64 = 64 * 1024;
const MAX_TRIAL_RECORD_BYTES: u64 = 16 * 1024;
const MAX_SECURITY_STATE_BYTES: u64 = 64 * 1024;
const PUBLIC_KEY_BASE64: &str = include_str!("../../licensing_public_key.txt");
const MILLIS_PER_DAY: i64 = 24 * 60 * 60 * 1000;
const CLOCK_TOLERANCE_MS: i64 = 24 * 60 * 60 * 1000;
const CLOCK_PERSIST_INTERVAL_MS: i64 = 60 * 1000;

#[derive(Clone, Debug)]
pub struct LicenseError {
    pub code: &'static str,
    pub message: String,
}

impl LicenseError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    fn storage(message: impl Into<String>) -> Self {
        Self::new("LICENSE_STORAGE_FAILED", message)
    }
}

#[derive(Clone)]
pub struct LicenseService {
    /// Device identity, trial start and trusted clock. This directory is kept
    /// across a normal uninstall so reinstalling cannot start a fresh trial.
    root: PathBuf,
    /// User-removable offline license file. This remains under the ordinary
    /// application data directory and is deleted with the user's app data.
    license_root: PathBuf,
    app_version: String,
    device: Arc<DeviceIdentity>,
    overview: Arc<RwLock<LicenseOverview>>,
    clock: Arc<RwLock<ClockState>>,
}

#[derive(Clone, Copy, Default)]
struct ClockState {
    highest_seen_ms: i64,
    last_persisted_ms: i64,
    rollback_detected: bool,
}

#[derive(serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TrialRecord {
    schema_version: u32,
    device_id: String,
    started_at_ms: i64,
}

impl LicenseService {
    pub fn initialize(root: PathBuf, app_version: String) -> Result<Self, LicenseError> {
        Self::initialize_with_roots(root.clone(), root, None, app_version)
    }

    pub fn initialize_with_roots(
        security_root: PathBuf,
        license_root: PathBuf,
        legacy_security_root: Option<PathBuf>,
        app_version: String,
    ) -> Result<Self, LicenseError> {
        if let Some(legacy_root) = legacy_security_root.as_deref() {
            migrate_legacy_security_state(legacy_root, &security_root)?;
        }
        validate_security_state_before_initialize(&security_root)?;
        let device = Arc::new(DeviceIdentity::load_or_create(&security_root)?);
        let persisted_clock_ms = load_trusted_clock(&security_root)?;
        let service = Self {
            root: security_root,
            license_root,
            app_version,
            device,
            overview: Arc::new(RwLock::new(basic_overview(
                "尚未导入桌面专业授权。",
                "basic",
            ))),
            clock: Arc::new(RwLock::new(ClockState {
                highest_seen_ms: persisted_clock_ms,
                last_persisted_ms: persisted_clock_ms,
                rollback_detected: false,
            })),
        };
        let trial_now = trusted_now(&service.clock, &service.root)?;
        // Create the trial marker even when a paid license is already present so
        // removing that license later cannot start a fresh trial.
        load_or_create_trial_started_at(&service.root, service.device.device_id(), trial_now)?;
        service.refresh()?;
        persist_security_state_marker(&service.root)?;
        Ok(service)
    }

    pub fn fallback(root: PathBuf, app_version: String, message: String) -> Self {
        Self::fallback_with_roots(root.clone(), root, app_version, message)
    }

    pub fn fallback_with_roots(
        security_root: PathBuf,
        license_root: PathBuf,
        app_version: String,
        message: String,
    ) -> Self {
        let device = DeviceIdentity::ephemeral();
        let mut overview = basic_overview(&message, "invalid");
        overview.device_id = device.device_id().to_owned();
        Self {
            root: security_root,
            license_root,
            app_version,
            device: Arc::new(device),
            overview: Arc::new(RwLock::new(overview)),
            clock: Arc::new(RwLock::new(ClockState::default())),
        }
    }

    pub fn overview(&self) -> LicenseOverview {
        if let Err(error) = self.refresh() {
            let mut overview = basic_overview(&error.message, "invalid");
            overview.device_id = self.device.device_id().to_owned();
            *self.overview.write().expect("license overview lock") = overview;
        }
        self.overview.read().expect("license overview lock").clone()
    }

    pub fn capabilities(&self) -> EffectiveCapabilities {
        self.overview().capabilities
    }

    pub fn require_batch_import(&self) -> Result<(), LicenseError> {
        if self.capabilities().can_batch_import {
            Ok(())
        } else {
            Err(LicenseError::new(
                "DESKTOP_LICENSE_BATCH_IMPORT_REQUIRED",
                "当前为基础桌面模式，只支持手动单题录入；Word 导入、文档式录入和批量导入需要有效的桌面专业授权。",
            ))
        }
    }

    pub fn require_document_export(&self) -> Result<(), LicenseError> {
        if self.capabilities().can_export_documents {
            Ok(())
        } else {
            Err(LicenseError::new(
                "DESKTOP_LICENSE_EXPORT_REQUIRED",
                "当前为基础桌面模式，不能导出题库或试卷；导入有效的桌面专业授权后可恢复导出。",
            ))
        }
    }

    pub fn require_print(&self) -> Result<(), LicenseError> {
        if self.capabilities().can_print {
            Ok(())
        } else {
            Err(LicenseError::new(
                "DESKTOP_LICENSE_PRINT_REQUIRED",
                "当前为基础桌面模式，不能打印试卷；导入有效的桌面专业授权后可恢复打印。",
            ))
        }
    }

    pub fn validate_paper_question_count(&self, count: usize) -> Result<(), LicenseError> {
        let Some(limit) = self.capabilities().max_questions_per_paper else {
            return Ok(());
        };
        if count <= limit as usize {
            return Ok(());
        }
        Err(LicenseError::new(
            "PAPER_QUESTION_LIMIT_EXCEEDED",
            format!(
                "基础桌面模式下每份试卷最多包含 {limit} 道题。已有超限试卷不会被删除，但请先减少到 {limit} 道题以内再保存。"
            ),
        ))
    }

    pub fn export_activation_request(
        &self,
        output_path: &Path,
    ) -> Result<LicenseFileResult, LicenseError> {
        let now = now_millis()?;
        let mut nonce = [0_u8; 16];
        OsRng.fill_bytes(&mut nonce);
        let payload = ActivationRequestPayload {
            schema_version: ACTIVATION_REQUEST_SCHEMA_VERSION,
            product: PRODUCT_CODE.to_owned(),
            device_id: self.device.device_id().to_owned(),
            device_public_key_base64: self.device.public_key_base64(),
            app_version: self.app_version.clone(),
            generated_at_ms: now,
            nonce_base64: STANDARD.encode(nonce),
        };
        let payload_bytes = serde_json::to_vec(&payload)
            .map_err(|error| LicenseError::storage(format!("无法生成授权申请：{error}")))?;
        let envelope = ActivationRequestEnvelope {
            schema_version: ACTIVATION_REQUEST_SCHEMA_VERSION,
            payload_base64: STANDARD.encode(&payload_bytes),
            signature_base64: self.device.sign(&payload_bytes),
        };
        let bytes = serde_json::to_vec_pretty(&envelope)
            .map_err(|error| LicenseError::storage(format!("无法生成授权申请文件：{error}")))?;
        write_new_file(output_path, &bytes, "授权申请")
    }

    pub fn import_desktop_license(
        &self,
        input_path: &Path,
    ) -> Result<LicenseOverview, LicenseError> {
        let bytes = read_guarded_file(input_path, MAX_LICENSE_BYTES, "离线许可证")?;
        let now = trusted_now(&self.clock, &self.root)?;
        let claims = verify_license_bytes(&bytes, self.device.device_id(), now)?;
        let stored = self.license_root.join(LICENSE_FILENAME);
        replace_file(&stored, &bytes, "离线许可证")?;
        self.apply_claims(claims, trusted_now(&self.clock, &self.root)?);
        Ok(self.overview())
    }

    pub fn remove_desktop_license(&self) -> Result<LicenseOverview, LicenseError> {
        let path = self.license_root.join(LICENSE_FILENAME);
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(LicenseError::storage(format!(
                    "无法移除本机许可证：{error}"
                )));
            }
        }
        self.refresh()?;
        Ok(self.overview.read().expect("license overview lock").clone())
    }

    fn refresh(&self) -> Result<(), LicenseError> {
        let license_path = self.license_root.join(LICENSE_FILENAME);
        let now = trusted_now(&self.clock, &self.root)?;
        if !license_path.exists() {
            match load_or_create_trial_started_at(&self.root, self.device.device_id(), now) {
                Ok(started_at_ms) => self.apply_trial(started_at_ms, now),
                Err(error) => {
                    let mut overview = basic_overview(
                        &format!(
                            "本机免费试用记录无效，当前使用基础桌面模式；可导入有效许可证继续使用。{}",
                            error.message
                        ),
                        "invalid",
                    );
                    overview.device_id = self.device.device_id().to_owned();
                    *self.overview.write().expect("license overview lock") = overview;
                }
            }
            return Ok(());
        }
        match read_guarded_file(&license_path, MAX_LICENSE_BYTES, "离线许可证")
            .and_then(|bytes| verify_license_bytes(&bytes, self.device.device_id(), now))
        {
            Ok(claims) => self.apply_claims(claims, now),
            Err(error) => {
                let mut overview = basic_overview(&error.message, "invalid");
                overview.device_id = self.device.device_id().to_owned();
                *self.overview.write().expect("license overview lock") = overview;
            }
        }
        Ok(())
    }

    fn apply_trial(&self, started_at_ms: i64, now: i64) {
        let expires_at_ms = started_at_ms
            .saturating_add(i64::from(DEFAULT_TRIAL_DAYS).saturating_mul(MILLIS_PER_DAY));
        let grace_ends_at_ms = expires_at_ms
            .saturating_add(i64::from(DEFAULT_GRACE_DAYS).saturating_mul(MILLIS_PER_DAY));
        let (state, plan, message, professional) = if now <= expires_at_ms {
            (
                "active",
                "trial",
                format!("正在免费试用桌面专业版，试用期为 {DEFAULT_TRIAL_DAYS} 天。"),
                true,
            )
        } else if now <= grace_ends_at_ms {
            (
                "grace",
                "trial",
                format!("免费试用已到期，当前处于 {DEFAULT_GRACE_DAYS} 天宽限期；请及时购买授权。"),
                true,
            )
        } else {
            (
                "basic",
                "basic",
                "免费试用及宽限期已结束，当前使用基础桌面模式。".to_owned(),
                false,
            )
        };
        let rollback_detected = self
            .clock
            .read()
            .expect("license clock lock")
            .rollback_detected;
        let message = if rollback_detected {
            format!("{message} 检测到系统时间曾向过去调整，试用期限按本机已记录的较晚时间计算。")
        } else {
            message
        };
        let overview = LicenseOverview {
            device_id: self.device.device_id().to_owned(),
            desktop: DesktopLicenseStatus {
                state: state.to_owned(),
                plan: plan.to_owned(),
                license_id: None,
                customer_name: Some("免费试用".to_owned()),
                issued_at_ms: Some(started_at_ms),
                expires_at_ms: Some(expires_at_ms),
                grace_ends_at_ms: Some(grace_ends_at_ms),
                message,
            },
            cloud: cloud_unavailable(),
            capabilities: if professional {
                EffectiveCapabilities::desktop_professional()
            } else {
                EffectiveCapabilities::basic()
            },
        };
        *self.overview.write().expect("license overview lock") = overview;
    }

    fn apply_claims(&self, claims: DesktopLicenseClaims, now: i64) {
        let grace_ends_at_ms = claims
            .expires_at_ms
            .saturating_add(i64::from(claims.grace_days).saturating_mul(MILLIS_PER_DAY));
        let (state, message, professional) = if now < claims.not_before_ms {
            (
                "invalid",
                "桌面许可证尚未到生效时间，请检查系统日期或联系授权方。".to_owned(),
                false,
            )
        } else if now <= claims.expires_at_ms {
            ("active", "桌面专业授权有效。".to_owned(), true)
        } else if now <= grace_ends_at_ms {
            (
                "grace",
                format!(
                    "桌面专业授权已到期，当前处于 {} 天宽限期；请及时续费。",
                    claims.grace_days
                ),
                true,
            )
        } else {
            (
                "basic",
                "桌面专业授权及宽限期已结束，当前使用基础桌面模式。".to_owned(),
                false,
            )
        };
        let rollback_detected = self
            .clock
            .read()
            .expect("license clock lock")
            .rollback_detected;
        let message = if rollback_detected {
            format!("{message} 检测到系统时间曾向过去调整，授权期限按本机已记录的较晚时间计算。")
        } else {
            message
        };
        let overview = LicenseOverview {
            device_id: self.device.device_id().to_owned(),
            desktop: DesktopLicenseStatus {
                state: state.to_owned(),
                plan: if professional {
                    "professional"
                } else {
                    "basic"
                }
                .to_owned(),
                license_id: Some(claims.license_id),
                customer_name: Some(claims.customer_name),
                issued_at_ms: Some(claims.issued_at_ms),
                expires_at_ms: Some(claims.expires_at_ms),
                grace_ends_at_ms: Some(grace_ends_at_ms),
                message,
            },
            cloud: cloud_unavailable(),
            capabilities: if professional {
                EffectiveCapabilities::desktop_professional()
            } else {
                EffectiveCapabilities::basic()
            },
        };
        *self.overview.write().expect("license overview lock") = overview;
    }
}

fn migrate_legacy_security_state(
    legacy_root: &Path,
    security_root: &Path,
) -> Result<(), LicenseError> {
    if legacy_root == security_root || !legacy_root.exists() {
        return Ok(());
    }
    require_real_directory(legacy_root, "旧授权状态目录")?;

    let present_sources = SECURITY_STATE_FILENAMES
        .iter()
        .filter(|name| legacy_root.join(name).exists())
        .count();
    if present_sources == 0 {
        return Ok(());
    }
    if present_sources != SECURITY_STATE_FILENAMES.len() {
        return Err(LicenseError::storage(
            "旧授权状态不完整，已拒绝生成新的试用记录。请保留当前文件并联系技术支持。",
        ));
    }

    if security_root.exists() {
        require_real_directory(security_root, "持久授权状态目录")?;
        let has_any_target = fs::read_dir(security_root)
            .map_err(|error| LicenseError::storage(format!("无法检查持久授权状态目录：{error}")))?
            .next()
            .transpose()
            .map_err(|error| LicenseError::storage(format!("无法读取持久授权状态目录：{error}")))?
            .is_some();
        let has_complete_target = SECURITY_STATE_FILENAMES
            .iter()
            .all(|name| security_root.join(name).exists());
        if has_complete_target {
            return Ok(());
        }
        if has_any_target {
            return Err(LicenseError::storage(
                "持久授权状态迁移目标不完整，已拒绝生成新的试用记录。请保留当前文件并联系技术支持。",
            ));
        }
        fs::remove_dir(security_root).map_err(|error| {
            LicenseError::storage(format!("无法准备持久授权状态迁移目录：{error}"))
        })?;
    }

    let parent = security_root
        .parent()
        .ok_or_else(|| LicenseError::storage("持久授权状态目录缺少安全的父目录。"))?;
    fs::create_dir_all(parent)
        .map_err(|error| LicenseError::storage(format!("无法创建授权状态父目录：{error}")))?;
    require_real_directory(parent, "持久授权状态父目录")?;
    let staging = parent.join(format!(
        ".entitlement-migration-{}",
        Uuid::now_v7().simple()
    ));
    fs::create_dir(&staging)
        .map_err(|error| LicenseError::storage(format!("无法创建授权状态迁移目录：{error}")))?;

    let migration_result = (|| {
        for filename in SECURITY_STATE_FILENAMES {
            let source = legacy_root.join(filename);
            let bytes = read_guarded_file(&source, MAX_SECURITY_STATE_BYTES, "旧授权状态")?;
            write_new_file(&staging.join(filename), &bytes, "授权状态迁移")?;
        }
        fs::rename(&staging, security_root)
            .map_err(|error| LicenseError::storage(format!("无法启用持久授权状态目录：{error}")))?;
        Ok(())
    })();
    if migration_result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    migration_result
}

fn validate_security_state_before_initialize(root: &Path) -> Result<(), LicenseError> {
    if !root.exists() {
        return Ok(());
    }
    require_real_directory(root, "持久授权状态目录")?;
    let marker = root.join(SECURITY_STATE_MARKER_FILENAME);
    if !marker.exists() {
        return Ok(());
    }
    let marker_bytes = read_guarded_file(&marker, MAX_SECURITY_STATE_BYTES, "授权状态标记")?;
    if marker_bytes != SECURITY_STATE_MARKER_CONTENT {
        return Err(LicenseError::storage(
            "持久授权状态标记无效，已拒绝生成新的试用记录。",
        ));
    }
    for filename in SECURITY_STATE_FILENAMES {
        if !root.join(filename).exists() {
            return Err(LicenseError::storage(
                "持久授权状态文件不完整，已拒绝重新开始试用。请联系技术支持。",
            ));
        }
    }
    Ok(())
}

fn persist_security_state_marker(root: &Path) -> Result<(), LicenseError> {
    let marker = root.join(SECURITY_STATE_MARKER_FILENAME);
    if marker.exists() {
        let bytes = read_guarded_file(&marker, MAX_SECURITY_STATE_BYTES, "授权状态标记")?;
        if bytes == SECURITY_STATE_MARKER_CONTENT {
            return Ok(());
        }
        return Err(LicenseError::storage("持久授权状态标记无效。"));
    }
    write_new_file(&marker, SECURITY_STATE_MARKER_CONTENT, "授权状态标记")?;
    Ok(())
}

fn require_real_directory(path: &Path, label: &str) -> Result<(), LicenseError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| LicenseError::storage(format!("无法检查{label}：{error}")))?;
    if link_or_reparse(&metadata) || !metadata.is_dir() {
        return Err(LicenseError::new(
            "LICENSE_PATH_INVALID",
            format!("{label}必须是普通目录，不能是符号链接、重解析点或文件。"),
        ));
    }
    Ok(())
}

pub(crate) fn link_or_reparse(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    false
}

fn basic_overview(message: &str, state: &str) -> LicenseOverview {
    LicenseOverview {
        device_id: String::new(),
        desktop: DesktopLicenseStatus {
            state: state.to_owned(),
            plan: "basic".to_owned(),
            license_id: None,
            customer_name: None,
            issued_at_ms: None,
            expires_at_ms: None,
            grace_ends_at_ms: None,
            message: message.to_owned(),
        },
        cloud: cloud_unavailable(),
        capabilities: EffectiveCapabilities::basic(),
    }
}

fn cloud_unavailable() -> CloudSubscriptionStatus {
    CloudSubscriptionStatus {
        state: "notConfigured".to_owned(),
        sync_enabled: false,
        web_app_enabled: false,
        expires_at_ms: None,
        message: "云同步和网页版尚未接入；其授权与桌面许可证相互独立。".to_owned(),
    }
}

fn trusted_now(clock: &RwLock<ClockState>, root: &Path) -> Result<i64, LicenseError> {
    let system_now = now_millis()?;
    let mut state = clock.write().expect("license clock lock");
    if state.highest_seen_ms > 0
        && system_now.saturating_add(CLOCK_TOLERANCE_MS) < state.highest_seen_ms
    {
        state.rollback_detected = true;
    }
    state.highest_seen_ms = state.highest_seen_ms.max(system_now);
    if state.last_persisted_ms == 0
        || state
            .highest_seen_ms
            .saturating_sub(state.last_persisted_ms)
            >= CLOCK_PERSIST_INTERVAL_MS
    {
        persist_trusted_clock(root, state.highest_seen_ms)?;
        state.last_persisted_ms = state.highest_seen_ms;
    }
    Ok(state.highest_seen_ms)
}

fn load_trusted_clock(root: &Path) -> Result<i64, LicenseError> {
    let path = root.join(CLOCK_FILENAME);
    if !path.exists() {
        return Ok(0);
    }
    let protected = read_guarded_file(&path, 16 * 1024, "授权时间记录")?;
    let bytes = unprotect_for_current_user(&protected)?;
    let raw: [u8; 8] = bytes
        .try_into()
        .map_err(|_| LicenseError::storage("本机授权时间记录无效，无法安全判断许可证期限。"))?;
    let value = i64::from_le_bytes(raw);
    if value <= 0 {
        return Err(LicenseError::storage("本机授权时间记录无效。"));
    }
    Ok(value)
}

fn persist_trusted_clock(root: &Path, value: i64) -> Result<(), LicenseError> {
    let protected = protect_for_current_user(&value.to_le_bytes())?;
    replace_file(&root.join(CLOCK_FILENAME), &protected, "授权时间记录")
}

fn load_or_create_trial_started_at(
    root: &Path,
    expected_device_id: &str,
    now: i64,
) -> Result<i64, LicenseError> {
    let path = root.join(TRIAL_FILENAME);
    if !path.exists() {
        let record = TrialRecord {
            schema_version: 1,
            device_id: expected_device_id.to_owned(),
            started_at_ms: now,
        };
        let bytes = serde_json::to_vec(&record)
            .map_err(|error| LicenseError::storage(format!("无法生成免费试用记录：{error}")))?;
        let protected = protect_for_current_user(&bytes)?;
        write_new_file(&path, &protected, "免费试用记录")?;
        return Ok(now);
    }

    let protected = read_guarded_file(&path, MAX_TRIAL_RECORD_BYTES, "免费试用记录")?;
    let bytes = unprotect_for_current_user(&protected)?;
    let record: TrialRecord = serde_json::from_slice(&bytes)
        .map_err(|_| LicenseError::storage("本机免费试用记录内容无效。"))?;
    if record.schema_version != 1
        || record.device_id != expected_device_id
        || record.started_at_ms <= 0
        || record.started_at_ms > now.saturating_add(CLOCK_TOLERANCE_MS)
    {
        return Err(LicenseError::storage(
            "本机免费试用记录与当前设备或授权时间不匹配。",
        ));
    }
    Ok(record.started_at_ms)
}

fn now_millis() -> Result<i64, LicenseError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| {
            LicenseError::new(
                "SYSTEM_TIME_INVALID",
                "系统时间早于 1970 年，无法判断授权状态。",
            )
        })?
        .as_millis();
    i64::try_from(millis)
        .map_err(|_| LicenseError::new("SYSTEM_TIME_INVALID", "系统时间超出授权系统支持范围。"))
}

fn verify_license_bytes(
    bytes: &[u8],
    expected_device_id: &str,
    now: i64,
) -> Result<DesktopLicenseClaims, LicenseError> {
    let envelope: SignedEnvelope = serde_json::from_slice(bytes).map_err(|error| {
        LicenseError::new(
            "LICENSE_FILE_INVALID",
            format!("离线许可证格式无效：{error}"),
        )
    })?;
    if envelope.schema_version != LICENSE_SCHEMA_VERSION {
        return Err(LicenseError::new(
            "LICENSE_SCHEMA_UNSUPPORTED",
            "离线许可证版本不受当前软件支持。",
        ));
    }
    if envelope.key_id != PRODUCTION_KEY_ID {
        return Err(LicenseError::new(
            "LICENSE_KEY_UNKNOWN",
            "离线许可证使用了未知的签发密钥。",
        ));
    }
    let payload = STANDARD
        .decode(envelope.payload_base64.as_bytes())
        .map_err(|_| LicenseError::new("LICENSE_FILE_INVALID", "离线许可证载荷编码无效。"))?;
    let signature_bytes = STANDARD
        .decode(envelope.signature_base64.as_bytes())
        .map_err(|_| LicenseError::new("LICENSE_FILE_INVALID", "离线许可证签名编码无效。"))?;
    let signature = Signature::try_from(signature_bytes.as_slice())
        .map_err(|_| LicenseError::new("LICENSE_FILE_INVALID", "离线许可证签名长度无效。"))?;
    production_verifying_key()?
        .verify_strict(&payload, &signature)
        .map_err(|_| {
            LicenseError::new(
                "LICENSE_SIGNATURE_INVALID",
                "离线许可证签名无效，文件可能被修改。",
            )
        })?;
    let claims: DesktopLicenseClaims = serde_json::from_slice(&payload).map_err(|error| {
        LicenseError::new(
            "LICENSE_FILE_INVALID",
            format!("离线许可证内容无效：{error}"),
        )
    })?;
    if claims.schema_version != LICENSE_SCHEMA_VERSION
        || claims.product != PRODUCT_CODE
        || claims.edition != DESKTOP_PROFESSIONAL_EDITION
    {
        return Err(LicenseError::new(
            "LICENSE_PRODUCT_MISMATCH",
            "该许可证不属于当前桌面专业版产品。",
        ));
    }
    if claims.device_id != expected_device_id {
        return Err(LicenseError::new(
            "LICENSE_DEVICE_MISMATCH",
            "该许可证绑定的是另一台电脑，请在本机重新生成授权申请。",
        ));
    }
    if claims.issued_at_ms <= 0
        || claims.not_before_ms <= 0
        || claims.expires_at_ms <= claims.not_before_ms
        || claims.issued_at_ms > claims.expires_at_ms
        || claims.grace_days > 30
    {
        return Err(LicenseError::new(
            "LICENSE_CLAIMS_INVALID",
            "离线许可证中的时间或宽限期设置无效。",
        ));
    }
    if claims.not_before_ms > now.saturating_add(30 * MILLIS_PER_DAY) {
        return Err(LicenseError::new(
            "LICENSE_NOT_YET_VALID",
            "许可证生效时间明显晚于当前系统时间，请检查日期。",
        ));
    }
    Ok(claims)
}

fn production_verifying_key() -> Result<VerifyingKey, LicenseError> {
    let bytes = STANDARD.decode(PUBLIC_KEY_BASE64.trim()).map_err(|_| {
        LicenseError::new(
            "LICENSE_CONFIGURATION_INVALID",
            "软件内置授权公钥格式无效。",
        )
    })?;
    let bytes: [u8; 32] = bytes.try_into().map_err(|_| {
        LicenseError::new(
            "LICENSE_CONFIGURATION_INVALID",
            "软件内置授权公钥长度无效。",
        )
    })?;
    VerifyingKey::from_bytes(&bytes).map_err(|_| {
        LicenseError::new(
            "LICENSE_CONFIGURATION_INVALID",
            "软件内置授权公钥不是有效的 Ed25519 公钥。",
        )
    })
}

fn read_guarded_file(path: &Path, max_bytes: u64, label: &str) -> Result<Vec<u8>, LicenseError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| LicenseError::storage(format!("无法读取{label}：{error}")))?;
    if link_or_reparse(&metadata) || !metadata.is_file() {
        return Err(LicenseError::new(
            "LICENSE_PATH_INVALID",
            format!("{label}必须是普通文件，不能是符号链接或目录。"),
        ));
    }
    if metadata.len() == 0 || metadata.len() > max_bytes {
        return Err(LicenseError::new(
            "LICENSE_FILE_INVALID",
            format!("{label}大小无效。"),
        ));
    }
    fs::read(path).map_err(|error| LicenseError::storage(format!("无法读取{label}：{error}")))
}

fn write_new_file(
    path: &Path,
    bytes: &[u8],
    label: &str,
) -> Result<LicenseFileResult, LicenseError> {
    let parent = path.parent().ok_or_else(|| {
        LicenseError::new("LICENSE_PATH_INVALID", format!("{label}保存路径无效。"))
    })?;
    if !parent.is_dir() {
        return Err(LicenseError::new(
            "LICENSE_PATH_INVALID",
            format!("{label}保存目录不存在。"),
        ));
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| LicenseError::storage(format!("无法创建{label}文件：{error}")))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| LicenseError::storage(format!("无法写入{label}文件：{error}")))?;
    Ok(LicenseFileResult {
        path: path.to_string_lossy().into_owned(),
        filename: path
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_default(),
        bytes: bytes.len() as u64,
    })
}

fn replace_file(path: &Path, bytes: &[u8], label: &str) -> Result<(), LicenseError> {
    fs::create_dir_all(path.parent().ok_or_else(|| {
        LicenseError::new("LICENSE_PATH_INVALID", format!("{label}保存路径无效。"))
    })?)
    .map_err(|error| LicenseError::storage(format!("无法创建{label}目录：{error}")))?;
    let temporary = path.with_extension(format!("tmp-{}", Uuid::now_v7()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| LicenseError::storage(format!("无法创建{label}临时文件：{error}")))?;
    if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(&temporary);
        return Err(LicenseError::storage(format!("无法写入{label}：{error}")));
    }
    let backup = path.with_extension(format!("backup-{}", Uuid::now_v7()));
    let had_existing = path.exists();
    if had_existing {
        fs::rename(path, &backup)
            .map_err(|error| LicenseError::storage(format!("无法暂存旧{label}：{error}")))?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        if had_existing {
            let _ = fs::rename(&backup, path);
        }
        return Err(LicenseError::storage(format!("无法启用新{label}：{error}")));
    }
    if had_existing {
        let _ = fs::remove_file(&backup);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn signed_license(signing_key: &SigningKey, device_id: &str, now: i64) -> Vec<u8> {
        let claims = DesktopLicenseClaims {
            schema_version: LICENSE_SCHEMA_VERSION,
            license_id: Uuid::now_v7().to_string(),
            product: PRODUCT_CODE.to_owned(),
            edition: DESKTOP_PROFESSIONAL_EDITION.to_owned(),
            device_id: device_id.to_owned(),
            customer_name: "测试用户".to_owned(),
            issued_at_ms: now,
            not_before_ms: now,
            expires_at_ms: now + MILLIS_PER_DAY,
            grace_days: DEFAULT_GRACE_DAYS,
        };
        let payload = serde_json::to_vec(&claims).unwrap();
        let envelope = SignedEnvelope {
            schema_version: LICENSE_SCHEMA_VERSION,
            key_id: PRODUCTION_KEY_ID.to_owned(),
            payload_base64: STANDARD.encode(&payload),
            signature_base64: STANDARD.encode(signing_key.sign(&payload).to_bytes()),
        };
        serde_json::to_vec(&envelope).unwrap()
    }

    #[test]
    fn basic_capabilities_keep_single_entry_and_limit_papers() {
        let capabilities = EffectiveCapabilities::basic();
        assert!(capabilities.can_edit_single_question);
        assert!(!capabilities.can_batch_import);
        assert_eq!(capabilities.max_questions_per_paper, Some(10));
        assert!(!capabilities.can_export_documents);
        assert!(!capabilities.can_print);
    }

    #[test]
    fn default_trial_and_grace_periods_are_both_fifteen_days() {
        assert_eq!(DEFAULT_TRIAL_DAYS, 15);
        assert_eq!(DEFAULT_GRACE_DAYS, 15);
    }

    #[test]
    fn trial_lifecycle_keeps_professional_features_through_grace() {
        let device = Arc::new(DeviceIdentity::ephemeral());
        let root = std::env::temp_dir().join(format!("tk-trial-state-{}", Uuid::now_v7()));
        let service = LicenseService {
            root: root.clone(),
            license_root: root,
            app_version: "0.1.54".to_owned(),
            device: Arc::clone(&device),
            overview: Arc::new(RwLock::new(basic_overview("test", "basic"))),
            clock: Arc::new(RwLock::new(ClockState::default())),
        };
        let started_at_ms = 1_800_000_000_000;
        let expires_at_ms = started_at_ms + i64::from(DEFAULT_TRIAL_DAYS) * MILLIS_PER_DAY;
        let grace_ends_at_ms = expires_at_ms + i64::from(DEFAULT_GRACE_DAYS) * MILLIS_PER_DAY;

        service.apply_trial(started_at_ms, expires_at_ms);
        let active = service.overview.read().unwrap().clone();
        assert_eq!(active.desktop.state, "active");
        assert_eq!(active.desktop.plan, "trial");
        assert!(active.capabilities.can_export_documents);
        assert_eq!(active.capabilities.max_questions_per_paper, None);

        service.apply_trial(started_at_ms, expires_at_ms + 1);
        let grace = service.overview.read().unwrap().clone();
        assert_eq!(grace.desktop.state, "grace");
        assert_eq!(grace.desktop.grace_ends_at_ms, Some(grace_ends_at_ms));
        assert!(grace.capabilities.can_batch_import);

        service.apply_trial(started_at_ms, grace_ends_at_ms + 1);
        let expired = service.overview.read().unwrap().clone();
        assert_eq!(expired.desktop.state, "basic");
        assert!(expired.capabilities.can_edit_single_question);
        assert!(!expired.capabilities.can_export_documents);
        assert_eq!(expired.capabilities.max_questions_per_paper, Some(10));
    }

    #[test]
    fn trial_start_is_persisted_across_service_reinitialization() {
        let root = std::env::temp_dir().join(format!("tk-trial-persist-{}", Uuid::now_v7()));
        let first = LicenseService::initialize(root.clone(), "0.1.54".to_owned()).unwrap();
        let first_overview = first.overview();
        let first_started_at_ms = first_overview.desktop.issued_at_ms.unwrap();
        assert_eq!(first_overview.desktop.state, "active");
        assert_eq!(first_overview.desktop.plan, "trial");
        drop(first);

        let second = LicenseService::initialize(root.clone(), "0.1.54".to_owned()).unwrap();
        let second_overview = second.overview();
        assert_eq!(
            second_overview.desktop.issued_at_ms,
            Some(first_started_at_ms)
        );
        assert_eq!(
            second_overview.desktop.expires_at_ms,
            Some(first_started_at_ms + i64::from(DEFAULT_TRIAL_DAYS) * MILLIS_PER_DAY)
        );
        drop(second);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn trial_state_survives_removal_of_ordinary_app_data() {
        let root = std::env::temp_dir().join(format!("tk-trial-uninstall-{}", Uuid::now_v7()));
        let security_root = root.join("persistent-entitlement");
        let first_app_data = root.join("app-data-first").join("licensing");
        fs::create_dir_all(&first_app_data).unwrap();
        let first = LicenseService::initialize_with_roots(
            security_root.clone(),
            first_app_data.clone(),
            None,
            "0.1.75".to_owned(),
        )
        .unwrap();
        let first_overview = first.overview();
        let first_started_at_ms = first_overview.desktop.issued_at_ms;
        let first_device_id = first_overview.device_id;
        drop(first);

        fs::remove_dir_all(first_app_data.parent().unwrap()).unwrap();
        let second = LicenseService::initialize_with_roots(
            security_root,
            root.join("app-data-second").join("licensing"),
            None,
            "0.1.75".to_owned(),
        )
        .unwrap();
        let second_overview = second.overview();
        assert_eq!(second_overview.desktop.issued_at_ms, first_started_at_ms);
        assert_eq!(second_overview.device_id, first_device_id);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_trial_state_is_migrated_without_changing_its_start() {
        let root = std::env::temp_dir().join(format!("tk-trial-migration-{}", Uuid::now_v7()));
        let legacy_root = root.join("legacy-app-data").join("licensing");
        let legacy = LicenseService::initialize(legacy_root.clone(), "0.1.74".to_owned()).unwrap();
        let legacy_overview = legacy.overview();
        drop(legacy);

        let security_root = root.join("persistent-entitlement");
        let migrated = LicenseService::initialize_with_roots(
            security_root.clone(),
            legacy_root.clone(),
            Some(legacy_root),
            "0.1.75".to_owned(),
        )
        .unwrap();
        let migrated_overview = migrated.overview();
        assert_eq!(migrated_overview.device_id, legacy_overview.device_id);
        assert_eq!(
            migrated_overview.desktop.issued_at_ms,
            legacy_overview.desktop.issued_at_ms
        );
        for filename in SECURITY_STATE_FILENAMES {
            assert!(security_root.join(filename).is_file());
        }
        assert!(security_root.join(SECURITY_STATE_MARKER_FILENAME).is_file());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn incomplete_persistent_target_never_overrides_valid_legacy_trial_state() {
        let root = std::env::temp_dir().join(format!("tk-trial-partial-{}", Uuid::now_v7()));
        let legacy_root = root.join("legacy-app-data").join("licensing");
        let legacy = LicenseService::initialize(legacy_root.clone(), "0.1.74".to_owned()).unwrap();
        drop(legacy);

        let security_root = root.join("persistent-entitlement");
        fs::create_dir_all(&security_root).unwrap();
        fs::write(
            security_root.join("unexpected-partial-state"),
            b"incomplete",
        )
        .unwrap();
        let result = LicenseService::initialize_with_roots(
            security_root,
            legacy_root.clone(),
            Some(legacy_root),
            "0.1.75".to_owned(),
        );
        let error = match result {
            Ok(_) => panic!("不完整的迁移目标不应覆盖旧试用状态"),
            Err(error) => error,
        };
        assert!(error.message.contains("迁移目标不完整"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn initialized_security_state_never_recreates_a_deleted_trial_record() {
        let root = std::env::temp_dir().join(format!("tk-trial-tamper-{}", Uuid::now_v7()));
        let service = LicenseService::initialize(root.clone(), "0.1.75".to_owned()).unwrap();
        drop(service);
        fs::remove_file(root.join(TRIAL_FILENAME)).unwrap();

        let error = match LicenseService::initialize(root.clone(), "0.1.75".to_owned()) {
            Ok(_) => panic!("缺少试用记录时不应重新初始化授权状态"),
            Err(error) => error,
        };
        assert!(error.message.contains("拒绝重新开始试用"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn license_lifecycle_keeps_professional_features_only_through_grace() {
        let device = Arc::new(DeviceIdentity::ephemeral());
        let root = std::env::temp_dir().join(format!("tk-license-state-{}", Uuid::now_v7()));
        let service = LicenseService {
            root: root.clone(),
            license_root: root,
            app_version: "0.1.54".to_owned(),
            device: Arc::clone(&device),
            overview: Arc::new(RwLock::new(basic_overview("test", "basic"))),
            clock: Arc::new(RwLock::new(ClockState::default())),
        };
        let expires_at_ms = 1_800_000_000_000;
        let claims = DesktopLicenseClaims {
            schema_version: LICENSE_SCHEMA_VERSION,
            license_id: "license-test".to_owned(),
            product: PRODUCT_CODE.to_owned(),
            edition: DESKTOP_PROFESSIONAL_EDITION.to_owned(),
            device_id: device.device_id().to_owned(),
            customer_name: "测试用户".to_owned(),
            issued_at_ms: expires_at_ms - MILLIS_PER_DAY,
            not_before_ms: expires_at_ms - MILLIS_PER_DAY,
            expires_at_ms,
            grace_days: DEFAULT_GRACE_DAYS,
        };

        service.apply_claims(claims.clone(), expires_at_ms);
        let active = service.overview.read().unwrap().clone();
        assert_eq!(active.desktop.state, "active");
        assert!(active.capabilities.can_export_documents);
        assert_eq!(active.capabilities.max_questions_per_paper, None);

        service.apply_claims(claims.clone(), expires_at_ms + MILLIS_PER_DAY);
        let grace = service.overview.read().unwrap().clone();
        assert_eq!(grace.desktop.state, "grace");
        assert!(grace.capabilities.can_batch_import);

        service.apply_claims(
            claims,
            expires_at_ms + (i64::from(DEFAULT_GRACE_DAYS) + 1) * MILLIS_PER_DAY,
        );
        let expired = service.overview.read().unwrap().clone();
        assert_eq!(expired.desktop.state, "basic");
        assert!(expired.capabilities.can_edit_single_question);
        assert!(!expired.capabilities.can_export_documents);
        assert_eq!(expired.capabilities.max_questions_per_paper, Some(10));
    }

    #[test]
    fn tampered_license_signature_is_rejected() {
        let signing_key = SigningKey::from_bytes(&[7_u8; 32]);
        let now = 1_800_000_000_000;
        let mut bytes = signed_license(&signing_key, "device-a", now);
        let last = bytes.len() - 2;
        bytes[last] ^= 1;
        let result =
            verify_license_bytes_with_key(&bytes, "device-a", now, signing_key.verifying_key());
        assert!(result.is_err());
    }

    fn verify_license_bytes_with_key(
        bytes: &[u8],
        expected_device_id: &str,
        now: i64,
        key: VerifyingKey,
    ) -> Result<DesktopLicenseClaims, LicenseError> {
        let envelope: SignedEnvelope = serde_json::from_slice(bytes)
            .map_err(|error| LicenseError::new("TEST", error.to_string()))?;
        let payload = STANDARD.decode(envelope.payload_base64).unwrap();
        let signature_bytes = STANDARD.decode(envelope.signature_base64).unwrap();
        let signature = Signature::try_from(signature_bytes.as_slice()).unwrap();
        key.verify_strict(&payload, &signature)
            .map_err(|_| LicenseError::new("TEST", "signature"))?;
        let claims: DesktopLicenseClaims = serde_json::from_slice(&payload).unwrap();
        if claims.device_id != expected_device_id || claims.not_before_ms > now {
            return Err(LicenseError::new("TEST", "claims"));
        }
        Ok(claims)
    }
}
