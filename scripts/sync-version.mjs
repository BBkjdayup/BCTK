import { readFileSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const checkOnly = process.argv.includes('--check')

function read(relativePath) {
  return readFileSync(resolve(root, relativePath), 'utf8')
}

function write(relativePath, content) {
  writeFileSync(resolve(root, relativePath), content, 'utf8')
}

const packageInfo = JSON.parse(read('package.json'))
const version = packageInfo.version
if (typeof version !== 'string' || !/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version)) {
  throw new Error(`package.json 中的版本号无效：${String(version)}`)
}

const tauriConfig = JSON.parse(read('src-tauri/tauri.conf.json'))
const cargoToml = read('src-tauri/Cargo.toml')
const cargoLock = read('src-tauri/Cargo.lock')
const cargoVersionMatch = cargoToml.match(/\[package\][\s\S]*?\nversion\s*=\s*"([^"]+)"/)
const lockVersionMatch = cargoLock.match(/\[\[package\]\]\r?\nname = "zhitiku-desktop"\r?\nversion = "([^"]+)"/)

const actualVersions = {
  'src-tauri/tauri.conf.json': tauriConfig.version,
  'src-tauri/Cargo.toml': cargoVersionMatch?.[1],
  'src-tauri/Cargo.lock': lockVersionMatch?.[1],
}
const mismatches = Object.entries(actualVersions)
  .filter(([, actual]) => actual !== version)
  .map(([path, actual]) => `${path}: ${actual ?? '未找到'}（应为 ${version}）`)

if (checkOnly) {
  if (mismatches.length) {
    throw new Error(`版本号不一致，请先运行 pnpm version:sync：\n${mismatches.join('\n')}`)
  }
  console.log(`版本号检查通过：${version}`)
  process.exit(0)
}

tauriConfig.version = version
write('src-tauri/tauri.conf.json', `${JSON.stringify(tauriConfig, null, 2)}\n`)
write(
  'src-tauri/Cargo.toml',
  cargoToml.replace(
    /(\[package\][\s\S]*?\nversion\s*=\s*")[^"]+(")/,
    `$1${version}$2`,
  ),
)
write(
  'src-tauri/Cargo.lock',
  cargoLock.replace(
    /(\[\[package\]\]\r?\nname = "zhitiku-desktop"\r?\nversion = ")[^"]+(")/,
    `$1${version}$2`,
  ),
)
console.log(`已将应用版本同步为 ${version}`)
