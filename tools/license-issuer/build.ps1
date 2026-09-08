param(
    [string]$OutputDirectory = (Join-Path $PSScriptRoot 'dist')
)

$ErrorActionPreference = 'Stop'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$tauriRoot = Join-Path $repoRoot 'src-tauri'
$source = Join-Path $PSScriptRoot 'LicenseIssuer.cs'
$readme = Join-Path $PSScriptRoot '使用说明.txt'
$icon = Join-Path $tauriRoot 'icons\icon.ico'
$backend = Join-Path $tauriRoot 'target\release\examples\license_admin.exe'
$cscCandidates = @(
    'C:\Windows\Microsoft.NET\Framework64\v4.0.30319\csc.exe',
    'C:\Windows\Microsoft.NET\Framework\v4.0.30319\csc.exe'
)
$csc = $cscCandidates | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
if (-not $csc) {
    throw '未找到 Windows .NET Framework C# 编译器，无法构建授权工具界面。'
}

Push-Location $tauriRoot
try {
    & cargo build --locked --release --example license_admin
    if ($LASTEXITCODE -ne 0) { throw 'Rust 许可证签名后端构建失败。' }
} finally {
    Pop-Location
}

New-Item -ItemType Directory -Path $OutputDirectory -Force | Out-Null
$secretFiles = Get-ChildItem -LiteralPath $OutputDirectory -File -Filter '*.key' -ErrorAction Stop
if ($secretFiles) {
    throw '授权工具输出目录中检测到 .key 私钥文件。请先把私钥移到独立的离线保管目录，再重新构建。'
}
$guiOutput = Join-Path $OutputDirectory 'TK试题题库授权工具.exe'
& $csc /nologo /target:winexe /optimize+ /platform:anycpu "/out:$guiOutput" "/win32icon:$icon" /reference:System.dll /reference:System.Core.dll /reference:System.Drawing.dll /reference:System.Windows.Forms.dll $source
if ($LASTEXITCODE -ne 0) { throw 'Windows 授权工具界面编译失败。' }

Copy-Item -LiteralPath $backend -Destination (Join-Path $OutputDirectory 'license_admin.exe') -Force
Copy-Item -LiteralPath $readme -Destination (Join-Path $OutputDirectory '使用说明.txt') -Force

$artifacts = Get-Item -LiteralPath $guiOutput, (Join-Path $OutputDirectory 'license_admin.exe'), (Join-Path $OutputDirectory '使用说明.txt')
$artifacts | Select-Object FullName, Length, LastWriteTime
Get-FileHash -Algorithm SHA256 -LiteralPath $guiOutput, (Join-Path $OutputDirectory 'license_admin.exe') | Select-Object Path, Hash
