use serde::{Deserialize, Serialize};

pub const LICENSE_SCHEMA_VERSION: u32 = 1;
pub const ACTIVATION_REQUEST_SCHEMA_VERSION: u32 = 1;
pub const PRODUCT_CODE: &str = "tk-teacher-question-bank";
pub const DESKTOP_PROFESSIONAL_EDITION: &str = "desktop_professional";
pub const PRODUCTION_KEY_ID: &str = "desktop-prod-2026-01";
pub const BASIC_PAPER_QUESTION_LIMIT: u32 = 10;
pub const DEFAULT_TRIAL_DAYS: u16 = 15;
pub const DEFAULT_GRACE_DAYS: u16 = 15;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SignedEnvelope {
    pub schema_version: u32,
    pub key_id: String,
    pub payload_base64: String,
    pub signature_base64: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopLicenseClaims {
    pub schema_version: u32,
    pub license_id: String,
    pub product: String,
    pub edition: String,
    pub device_id: String,
    pub customer_name: String,
    pub issued_at_ms: i64,
    pub not_before_ms: i64,
    pub expires_at_ms: i64,
    pub grace_days: u16,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivationRequestPayload {
    pub schema_version: u32,
    pub product: String,
    pub device_id: String,
    pub device_public_key_base64: String,
    pub app_version: String,
    pub generated_at_ms: i64,
    pub nonce_base64: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActivationRequestEnvelope {
    pub schema_version: u32,
    pub payload_base64: String,
    pub signature_base64: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveCapabilities {
    pub can_edit_single_question: bool,
    pub can_batch_import: bool,
    pub max_questions_per_paper: Option<u32>,
    pub can_export_documents: bool,
    pub can_print: bool,
    pub can_use_cloud_sync: bool,
    pub can_use_web_app: bool,
}

impl EffectiveCapabilities {
    pub fn basic() -> Self {
        Self {
            can_edit_single_question: true,
            can_batch_import: false,
            max_questions_per_paper: Some(BASIC_PAPER_QUESTION_LIMIT),
            can_export_documents: false,
            can_print: false,
            can_use_cloud_sync: false,
            can_use_web_app: false,
        }
    }

    pub fn desktop_professional() -> Self {
        Self {
            can_edit_single_question: true,
            can_batch_import: true,
            max_questions_per_paper: None,
            can_export_documents: true,
            can_print: true,
            can_use_cloud_sync: false,
            can_use_web_app: false,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopLicenseStatus {
    pub state: String,
    pub plan: String,
    pub license_id: Option<String>,
    pub customer_name: Option<String>,
    pub issued_at_ms: Option<i64>,
    pub expires_at_ms: Option<i64>,
    pub grace_ends_at_ms: Option<i64>,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CloudSubscriptionStatus {
    pub state: String,
    pub sync_enabled: bool,
    pub web_app_enabled: bool,
    pub expires_at_ms: Option<i64>,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LicenseOverview {
    pub device_id: String,
    pub desktop: DesktopLicenseStatus,
    pub cloud: CloudSubscriptionStatus,
    pub capabilities: EffectiveCapabilities,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LicenseFileResult {
    pub path: String,
    pub filename: String,
    pub bytes: u64,
}
