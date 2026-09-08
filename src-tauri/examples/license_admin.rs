use std::{
    env,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::ExitCode,
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rand_core::OsRng;
use uuid::Uuid;
use zhitiku_desktop_lib::licensing::{
    ACTIVATION_REQUEST_SCHEMA_VERSION, ActivationRequestEnvelope, ActivationRequestPayload,
    DEFAULT_GRACE_DAYS, DESKTOP_PROFESSIONAL_EDITION, DesktopLicenseClaims, LICENSE_SCHEMA_VERSION,
    LicenseService, PRODUCT_CODE, PRODUCTION_KEY_ID, SignedEnvelope, device_id_for_public_key,
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("错误：{message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    match args.first().map(String::as_str) {
        Some("keygen") if args.len() == 2 => keygen(Path::new(&args[1])),
        Some("inspect-key") if args.len() == 2 => inspect_key(Path::new(&args[1])),
        Some("inspect-request") if args.len() == 2 => inspect_request(Path::new(&args[1])),
        Some("create-request") if args.len() == 3 => {
            create_request(PathBuf::from(&args[1]), Path::new(&args[2]))
        }
        Some("verify-license") if args.len() == 3 => {
            verify_license(PathBuf::from(&args[1]), Path::new(&args[2]))
        }
        Some("issue") if args.len() == 6 => issue(
            Path::new(&args[1]),
            Path::new(&args[2]),
            Path::new(&args[3]),
            &args[4],
            args[5]
                .parse::<i64>()
                .map_err(|_| "到期时间必须是 Unix 毫秒时间戳。".to_owned())?,
        ),
        _ => Err(
            "用法：\n  cargo run --example license_admin -- keygen <私钥文件>\n  cargo run --example license_admin -- inspect-key <私钥文件>\n  cargo run --example license_admin -- inspect-request <.tkreq申请>\n  cargo run --example license_admin -- create-request <测试授权目录> <.tkreq输出>\n  cargo run --example license_admin -- verify-license <测试授权目录> <.tklic许可证>\n  cargo run --example license_admin -- issue <私钥文件> <.tkreq申请> <.tklic输出> <客户名称> <到期Unix毫秒>"
                .to_owned(),
        ),
    }
}

fn create_request(root: PathBuf, output: &Path) -> Result<(), String> {
    let service = LicenseService::initialize(root, env!("CARGO_PKG_VERSION").to_owned())
        .map_err(format_license_error)?;
    let result = service
        .export_activation_request(output)
        .map_err(format_license_error)?;
    println!("REQUEST_PATH={}", result.path);
    println!("DEVICE_ID={}", service.overview().device_id);
    Ok(())
}

fn verify_license(root: PathBuf, input: &Path) -> Result<(), String> {
    let service = LicenseService::initialize(root, env!("CARGO_PKG_VERSION").to_owned())
        .map_err(format_license_error)?;
    let overview = service
        .import_desktop_license(input)
        .map_err(format_license_error)?;
    println!("DEVICE_ID={}", overview.device_id);
    println!("LICENSE_STATE={}", overview.desktop.state);
    println!("LICENSE_PLAN={}", overview.desktop.plan);
    println!(
        "CAN_BATCH_IMPORT={}",
        overview.capabilities.can_batch_import
    );
    println!(
        "CAN_EXPORT_DOCUMENTS={}",
        overview.capabilities.can_export_documents
    );
    println!("CAN_PRINT={}", overview.capabilities.can_print);
    Ok(())
}

fn format_license_error(error: zhitiku_desktop_lib::licensing::LicenseError) -> String {
    format!("{} ({})", error.message, error.code)
}

fn inspect_key(path: &Path) -> Result<(), String> {
    let signing_key = read_signing_key(path)?;
    ensure_production_signing_key(&signing_key)?;
    println!("KEY_ID={PRODUCTION_KEY_ID}");
    println!(
        "PUBLIC_KEY_BASE64={}",
        STANDARD.encode(signing_key.verifying_key().to_bytes())
    );
    println!("MATCHES_APPLICATION=true");
    Ok(())
}

fn inspect_request(path: &Path) -> Result<(), String> {
    let request = read_activation_request(path)?;
    println!("DEVICE_ID={}", request.device_id);
    println!("APP_VERSION={}", request.app_version);
    println!("GENERATED_AT_MS={}", request.generated_at_ms);
    println!("PRODUCT={}", request.product);
    Ok(())
}

fn keygen(output: &Path) -> Result<(), String> {
    let signing_key = SigningKey::generate(&mut OsRng);
    let encoded = format!("{}\n", STANDARD.encode(signing_key.to_bytes()));
    write_new(output, encoded.as_bytes(), "签发私钥")?;
    println!("签发私钥已创建：{}", output.display());
    println!("请把下面的公钥写入 licensing_public_key.txt 后重新构建桌面软件：");
    println!(
        "{}",
        STANDARD.encode(signing_key.verifying_key().to_bytes())
    );
    println!("密钥编号：{PRODUCTION_KEY_ID}");
    println!("警告：私钥不能交给客户、不能放入 Git，也不能打进安装包。请离线备份。");
    Ok(())
}

fn issue(
    private_key_path: &Path,
    request_path: &Path,
    output_path: &Path,
    customer_name: &str,
    expires_at_ms: i64,
) -> Result<(), String> {
    let customer_name = customer_name.trim();
    if customer_name.is_empty() || customer_name.chars().count() > 100 {
        return Err("客户名称不能为空且不能超过100个字符。".to_owned());
    }
    let now = now_millis()?;
    if expires_at_ms <= now {
        return Err("许可证到期时间必须晚于当前时间。".to_owned());
    }
    let signing_key = read_signing_key(private_key_path)?;
    ensure_production_signing_key(&signing_key)?;
    let request = read_activation_request(request_path)?;
    let claims = DesktopLicenseClaims {
        schema_version: LICENSE_SCHEMA_VERSION,
        license_id: Uuid::now_v7().to_string(),
        product: PRODUCT_CODE.to_owned(),
        edition: DESKTOP_PROFESSIONAL_EDITION.to_owned(),
        device_id: request.device_id,
        customer_name: customer_name.to_owned(),
        issued_at_ms: now,
        not_before_ms: now.saturating_sub(5 * 60 * 1000),
        expires_at_ms,
        grace_days: DEFAULT_GRACE_DAYS,
    };
    let payload =
        serde_json::to_vec(&claims).map_err(|error| format!("无法序列化许可证：{error}"))?;
    let envelope = SignedEnvelope {
        schema_version: LICENSE_SCHEMA_VERSION,
        key_id: PRODUCTION_KEY_ID.to_owned(),
        payload_base64: STANDARD.encode(&payload),
        signature_base64: STANDARD.encode(signing_key.sign(&payload).to_bytes()),
    };
    let output = serde_json::to_vec_pretty(&envelope)
        .map_err(|error| format!("无法生成许可证文件：{error}"))?;
    write_new(output_path, &output, "许可证")?;
    println!("许可证已签发：{}", output_path.display());
    println!("许可证编号：{}", claims.license_id);
    println!("绑定设备：{}", claims.device_id);
    println!("到期时间戳：{}", claims.expires_at_ms);
    Ok(())
}

fn ensure_production_signing_key(signing_key: &SigningKey) -> Result<(), String> {
    let expected = include_str!("../licensing_public_key.txt").trim();
    let actual = STANDARD.encode(signing_key.verifying_key().to_bytes());
    if actual != expected {
        return Err(format!(
            "所选私钥与当前软件内置公钥不匹配，不能用于密钥编号 {PRODUCTION_KEY_ID}。"
        ));
    }
    Ok(())
}

fn read_activation_request(path: &Path) -> Result<ActivationRequestPayload, String> {
    let bytes = read_small_file(path, 64 * 1024, "授权申请")?;
    let envelope: ActivationRequestEnvelope =
        serde_json::from_slice(&bytes).map_err(|error| format!("授权申请格式无效：{error}"))?;
    if envelope.schema_version != ACTIVATION_REQUEST_SCHEMA_VERSION {
        return Err("授权申请版本不受支持。".to_owned());
    }
    let payload_bytes = STANDARD
        .decode(envelope.payload_base64)
        .map_err(|_| "授权申请载荷编码无效。".to_owned())?;
    let payload: ActivationRequestPayload = serde_json::from_slice(&payload_bytes)
        .map_err(|error| format!("授权申请内容无效：{error}"))?;
    if payload.schema_version != ACTIVATION_REQUEST_SCHEMA_VERSION
        || payload.product != PRODUCT_CODE
    {
        return Err("授权申请不属于当前产品。".to_owned());
    }
    let public_key_bytes = STANDARD
        .decode(payload.device_public_key_base64.as_bytes())
        .map_err(|_| "设备公钥编码无效。".to_owned())?;
    let public_key_bytes: [u8; 32] = public_key_bytes
        .try_into()
        .map_err(|_| "设备公钥长度无效。".to_owned())?;
    let public_key =
        VerifyingKey::from_bytes(&public_key_bytes).map_err(|_| "设备公钥无效。".to_owned())?;
    if device_id_for_public_key(&public_key) != payload.device_id {
        return Err("授权申请中的设备编号与公钥不匹配。".to_owned());
    }
    let signature_bytes = STANDARD
        .decode(envelope.signature_base64)
        .map_err(|_| "授权申请签名编码无效。".to_owned())?;
    let signature = Signature::try_from(signature_bytes.as_slice())
        .map_err(|_| "授权申请签名长度无效。".to_owned())?;
    public_key
        .verify_strict(&payload_bytes, &signature)
        .map_err(|_| "授权申请签名无效，文件可能被修改。".to_owned())?;
    Ok(payload)
}

fn read_signing_key(path: &Path) -> Result<SigningKey, String> {
    let bytes = read_small_file(path, 4096, "签发私钥")?;
    let encoded = std::str::from_utf8(&bytes)
        .map_err(|_| "签发私钥不是UTF-8文本。".to_owned())?
        .trim();
    let secret = STANDARD
        .decode(encoded.as_bytes())
        .map_err(|_| "签发私钥编码无效。".to_owned())?;
    let secret: [u8; 32] = secret
        .try_into()
        .map_err(|_| "签发私钥长度无效。".to_owned())?;
    Ok(SigningKey::from_bytes(&secret))
}

fn read_small_file(path: &Path, max_bytes: u64, label: &str) -> Result<Vec<u8>, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("无法读取{label}：{error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(format!("{label}必须是普通文件。"));
    }
    if metadata.len() == 0 || metadata.len() > max_bytes {
        return Err(format!("{label}大小无效。"));
    }
    fs::read(path).map_err(|error| format!("无法读取{label}：{error}"))
}

fn write_new(path: &Path, bytes: &[u8], label: &str) -> Result<(), String> {
    if !path.parent().is_some_and(Path::is_dir) {
        return Err(format!("{label}输出目录不存在。"));
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("无法创建{label}文件：{error}"))?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("无法写入{label}文件：{error}"))
}

fn now_millis() -> Result<i64, String> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "系统时间早于1970年。".to_owned())?
        .as_millis();
    i64::try_from(millis).map_err(|_| "系统时间超出支持范围。".to_owned())
}
