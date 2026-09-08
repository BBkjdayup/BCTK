param(
  [string]$MathDoc = ""
)

$ErrorActionPreference = "Stop"
$projectRoot = Split-Path -Parent $PSScriptRoot

if ([string]::IsNullOrWhiteSpace($MathDoc)) {
  $searchRoots = @(
    (Join-Path $env:USERPROFILE "Desktop"),
    (Join-Path $env:USERPROFILE "Documents\xwechat_files")
  )
  $fixture = foreach ($searchRoot in $searchRoots) {
    if (-not (Test-Path -LiteralPath $searchRoot)) { continue }
    Get-ChildItem -LiteralPath $searchRoot -Recurse -File -Filter "*.docx" -ErrorAction SilentlyContinue |
      Where-Object { $_.Name -like "*2025-2026*" -and $_.Length -eq 320956 } |
      Select-Object -First 1
  }
  $MathDoc = ($fixture | Select-Object -First 1).FullName
}

if ([string]::IsNullOrWhiteSpace($MathDoc) -or -not (Test-Path -LiteralPath $MathDoc -PathType Leaf)) {
  throw "Real math DOCX not found. Run: pnpm test:word:real -- -MathDoc 'C:\full\path\exam.docx'"
}

$preparedJson = Join-Path ([IO.Path]::GetTempPath()) "zhitiku-real-word-prepared.json"
$env:ZHITIKU_MATHTYPE_DOCX = (Resolve-Path -LiteralPath $MathDoc).Path
$env:ZHITIKU_MATHTYPE_PREPARED_OUTPUT = $preparedJson

Push-Location $projectRoot
try {
  cargo test --manifest-path src-tauri/Cargo.toml prepares_supplied_mathtype_exam_as_editable_formulas -- --ignored --nocapture
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

  $env:ZHITIKU_MATHTYPE_PREPARED_JSON = $preparedJson
  pnpm exec vitest run src/utils/wordImportExternal.test.ts --reporter=verbose
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
} finally {
  Pop-Location
  Remove-Item -LiteralPath $preparedJson -Force -ErrorAction SilentlyContinue
}
