use std::{env, fs, path::Path};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use minisign_verify::{PublicKey, Signature};

fn decode_tauri_signer_file(path: &Path) -> Result<String, String> {
    let encoded = fs::read_to_string(path)
        .map_err(|error| format!("无法读取 {}：{error}", path.display()))?;
    let decoded = STANDARD
        .decode(encoded.trim())
        .map_err(|error| format!("{} 不是有效的 Tauri 签名文件：{error}", path.display()))?;
    String::from_utf8(decoded)
        .map_err(|error| format!("{} 解码后不是 UTF-8 文本：{error}", path.display()))
}

fn main() -> Result<(), String> {
    let mut arguments = env::args_os().skip(1);
    let public_key_path = arguments
        .next()
        .map(std::path::PathBuf::from)
        .ok_or("缺少公钥路径")?;
    let artifact_path = arguments
        .next()
        .map(std::path::PathBuf::from)
        .ok_or("缺少待验证安装包路径")?;
    let signature_path = arguments
        .next()
        .map(std::path::PathBuf::from)
        .ok_or("缺少签名路径")?;
    if arguments.next().is_some() {
        return Err("参数过多".to_owned());
    }

    let public_key_text = decode_tauri_signer_file(&public_key_path)?;
    let signature_text = decode_tauri_signer_file(&signature_path)?;
    let public_key = PublicKey::decode(&public_key_text)
        .map_err(|error| format!("无法解析更新公钥：{error}"))?;
    let signature =
        Signature::decode(&signature_text).map_err(|error| format!("无法解析更新签名：{error}"))?;
    let artifact = fs::read(&artifact_path)
        .map_err(|error| format!("无法读取 {}：{error}", artifact_path.display()))?;
    public_key
        .verify(&artifact, &signature, false)
        .map_err(|error| format!("更新签名验证失败：{error}"))?;

    println!("更新签名验证通过：{}", artifact_path.display());
    Ok(())
}
