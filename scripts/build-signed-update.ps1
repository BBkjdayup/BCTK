param(
    [string]$NotesFile = ""
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$repoRoot = Split-Path -Parent $PSScriptRoot
$packageJsonPath = Join-Path $repoRoot "package.json"
$package = Get-Content -LiteralPath $packageJsonPath -Raw | ConvertFrom-Json
$version = [string]$package.version
if ($version -notmatch '^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$') {
    throw "package.json 中的软件版本不是有效的 SemVer：$version"
}

$privateKeyPath = Join-Path $env:USERPROFILE ".tauri\tktiku-updater.key"
$publicKeyPath = "$privateKeyPath.pub"
$passwordPath = Join-Path $env:USERPROFILE ".tauri\tktiku-updater.password.dpapi"
if (-not (Test-Path -LiteralPath $privateKeyPath -PathType Leaf)) {
    throw "缺少更新签名私钥：$privateKeyPath"
}
if (-not (Test-Path -LiteralPath $passwordPath -PathType Leaf)) {
    throw "缺少由 Windows 当前用户保护的更新签名口令：$passwordPath"
}
if (-not (Test-Path -LiteralPath $publicKeyPath -PathType Leaf)) {
    throw "缺少更新签名公钥：$publicKeyPath"
}

$tauriConfig = Get-Content -LiteralPath (Join-Path $repoRoot "src-tauri\tauri.conf.json") -Raw | ConvertFrom-Json
$configuredPublicKey = [string]$tauriConfig.plugins.updater.pubkey
$releasePublicKey = (Get-Content -LiteralPath $publicKeyPath -Raw).Trim()
if ($configuredPublicKey.Trim() -ne $releasePublicKey) {
    throw "tauri.conf.json 中的更新公钥与当前发布密钥不一致，已拒绝构建。"
}

$protectedPassword = (Get-Content -LiteralPath $passwordPath -Raw).Trim()
$securePassword = $protectedPassword | ConvertTo-SecureString
$credential = [PSCredential]::new("tktiku-updater", $securePassword)
$plainPassword = $credential.GetNetworkCredential().Password

Push-Location $repoRoot
try {
    $env:TAURI_SIGNING_PRIVATE_KEY = $privateKeyPath
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = $plainPassword
    & pnpm tauri build
    if ($LASTEXITCODE -ne 0) {
        throw "Tauri 发布构建失败，退出码：$LASTEXITCODE"
    }
}
finally {
    Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY -ErrorAction SilentlyContinue
    Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD -ErrorAction SilentlyContinue
    $plainPassword = $null
    Pop-Location
}

$bundleDirectory = Join-Path $repoRoot "src-tauri\target\release\bundle\nsis"
$installerName = "TK试题题库_${version}_x64-setup.exe"
$installerPath = Join-Path $bundleDirectory $installerName
$signaturePath = "$installerPath.sig"
if (-not (Test-Path -LiteralPath $installerPath -PathType Leaf)) {
    throw "构建完成但没有找到安装包：$installerPath"
}
if (-not (Test-Path -LiteralPath $signaturePath -PathType Leaf)) {
    throw "构建完成但没有找到更新签名：$signaturePath"
}

& cargo run --quiet --locked --manifest-path (Join-Path $repoRoot "src-tauri\Cargo.toml") --example verify_updater_signature -- $publicKeyPath $installerPath $signaturePath
if ($LASTEXITCODE -ne 0) {
    throw "构建产物的更新签名验证失败，退出码：$LASTEXITCODE"
}

$notes = "TK试题题库 $version 更新"
if ($NotesFile) {
    $resolvedNotes = [IO.Path]::GetFullPath((Join-Path $repoRoot $NotesFile))
    if (-not (Test-Path -LiteralPath $resolvedNotes -PathType Leaf)) {
        throw "更新说明文件不存在：$resolvedNotes"
    }
    $notes = (Get-Content -LiteralPath $resolvedNotes -Raw).Trim()
}

$stagingDirectory = Join-Path $repoRoot ".codex-tmp\desktop-updates\$version"
$filesDirectory = Join-Path $stagingDirectory "files"
New-Item -ItemType Directory -Path $filesDirectory -Force | Out-Null
$publicInstallerName = "tktiku-desktop_${version}_windows_x86_64-setup.exe"
$publicInstallerPath = Join-Path $filesDirectory $publicInstallerName
$publicSignaturePath = "$publicInstallerPath.sig"
Copy-Item -LiteralPath $installerPath -Destination $publicInstallerPath -Force
Copy-Item -LiteralPath $signaturePath -Destination $publicSignaturePath -Force

$signature = (Get-Content -LiteralPath $signaturePath -Raw).Trim()
$manifest = [ordered]@{
    version = $version
    notes = $notes
    pub_date = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
    platforms = [ordered]@{
        "windows-x86_64" = [ordered]@{
            signature = $signature
            url = "https://api.tktiku.cn/updates/files/$publicInstallerName"
        }
    }
}
$manifestPath = Join-Path $stagingDirectory "latest.json"
$manifestJson = $manifest | ConvertTo-Json -Depth 6
[IO.File]::WriteAllText($manifestPath, $manifestJson, [Text.UTF8Encoding]::new($false))

$hash = Get-FileHash -LiteralPath $publicInstallerPath -Algorithm SHA256
[pscustomobject]@{
    Version = $version
    Installer = $publicInstallerPath
    Signature = $publicSignaturePath
    Manifest = $manifestPath
    SizeBytes = (Get-Item -LiteralPath $publicInstallerPath).Length
    SHA256 = $hash.Hash
}
