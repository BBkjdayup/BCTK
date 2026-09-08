<p align="center">
  <img src="public/tk-logo.png" width="96" alt="TK试题题库 Logo">
</p>

# BCTK · TK试题题库

**把散落在文档里的题目，整理成自己的本地题库。**

[![MIT License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Windows](https://img.shields.io/badge/platform-Windows_10%20%2F%2011-0078D4)](#获取与使用)
[![Version](https://img.shields.io/badge/release-v0.1.83-16a34a)](https://github.com/BBkjdayup/BCTK/releases/tag/v0.1.83)
[![Tauri](https://img.shields.io/badge/Tauri-2-24C8DB)](#技术组成与目录)

[中文](README.md) · [English](README.en.md)

TK试题题库是一款面向教师个人和教育机构的 Windows 桌面软件。从题目录入、分类检索，到选题组卷、分页排版和备份恢复，让日常积累的题目可以反复使用。题库保存在自己的电脑上，日常本地功能可以离线使用。

基于 **Tauri 2 + Vue 3 + TypeScript + Rust + SQLite** 构建，源码采用 **MIT 许可证**。

**[下载 Windows 安装包](https://github.com/BBkjdayup/BCTK/releases/tag/v0.1.83)** · [查看界面](#界面预览) · [从源码运行](#从源码运行) · [反馈问题](https://github.com/BBkjdayup/BCTK/issues)

> **使用前请了解：** 当前 v0.1.83 程序保留 15 天专业版试用、15 天宽限期及之后的基础模式限制；MIT 开源许可不会自动取消程序内的授权判断。[查看具体范围](#开源范围与授权)。

## 界面预览

### 整理题库：分类、检索与题目预览

![题库管理界面：左侧按学科与章节分类，中间列出题目，右侧查看题干、答案和解析](docs/images/question-bank.png)

<details>
<summary><strong>查看选题组卷与试卷排版截图</strong></summary>

**选题组卷**：筛选候选题目，加入试卷，查看已选内容和题型分布。

![选题组卷界面：按分类筛选题目，并将四道示例题加入试卷](docs/images/paper-selection.png)

**试卷排版**：在分页稿中调整正文和版面，设置试卷标题与显示内容。

![试卷排版界面：编辑示例练习的标题、正文、纸张和版式](docs/images/paper-layout.png)

</details>

以上为 v0.1.83 浏览器演示模式的实际界面截图，使用内置示例题。演示模式不读写真实题库；文件导入导出、备份恢复等操作需要使用桌面版。

## 从这里开始

| 你想做什么 | 入口 |
| --- | --- |
| 下载软件，整理自己的题库 | [Windows v0.1.83](https://github.com/BBkjdayup/BCTK/releases/tag/v0.1.83) · [安装与首次使用](docs/Windows安装与首次使用.md) |
| 了解能处理哪些教学资料 | [功能介绍](#功能介绍) · [文件兼容说明](#文件兼容说明) |
| 运行源码或预览界面 | [开发环境与启动步骤](#从源码运行) |
| 反馈教学需求或参与开发 | [提交 Issue](https://github.com/BBkjdayup/BCTK/issues) · [贡献指南](CONTRIBUTING.md) |

如果这个项目对你有用，欢迎点一下右上角的 **Star** 收藏；使用反馈和可复现的示例也能帮助项目改进。

## 功能介绍

| 场景 | 支持的功能 |
| --- | --- |
| 录入与编辑 | 单选、多选、填空、判断、简答及自定义题型；题干、选项、答案和解析；图片、表格、数学公式、上下标与富文本编辑 |
| 整理题库 | 按学科、章节和标签分类；搜索筛选、批量编辑、题目预览、使用状态筛选和重复题检测 |
| 批量导入 | 从 Word `.docx` 和 Excel `.xlsx` 导入；逐题审查、分类调整、重复项处理及导入草稿恢复 |
| 选题组卷 | 手动选题与随机抽题；按学科、章节、标签和使用状态限定候选；支持多学科抽题、题目替换与排序 |
| 排版与打印 | 真实分页编辑、试卷模板、图片与表格、分页符、撤销重做、缩放、排版保存和系统打印 |
| 文档导出 | 将题库或已保存的试卷导出为 Word；将题库导出为 Excel；Word 导出支持受支持的图片、表格及可编辑数学公式 |
| 历史与统计 | 试卷草稿、历史试卷、题目快照、题库数量及分类统计、近期新增和使用情况 |
| 数据管理 | 回收站、`.tqb` 完整备份、备份校验、恢复和数据目录迁移 |

Word 文档由本机 Rust 模块读取和生成，无需调用 Microsoft Office、WPS Office 或 LibreOffice。格式支持范围见下方的[文件兼容说明](#文件兼容说明)。

> 当前源码保留原有的试用、基础模式和专业版授权逻辑。批量导入、文档导出、打印等功能的实际可用状态受现有授权逻辑控制，详见[开源范围与授权](#开源范围与授权)。

### 一份试卷的制作流程

1. **建立分类**：整理学科、章节和标签，形成适合自己教学进度的题库结构。
2. **积累题目**：手动录入，或导入已有 Word / Excel 资料，核对题干、答案和解析。
3. **筛选与组卷**：手动选择题目，或按条件随机抽取，再调整顺序和替换题目。
4. **排版与输出**：选择模板、编辑分页稿，保存试卷并导出 Word 或打印。
5. **保留与备份**：从历史试卷继续编辑或复制，定期创建题库备份。

## 获取与使用

### Windows 用户

当前主要支持 **Windows 10 / 11 x64**。

1. 打开 [v0.1.83 发布页](https://github.com/BBkjdayup/BCTK/releases/tag/v0.1.83)。
2. 在 **Assets** 中下载 `BCTK_0.1.83_windows_x64-setup.exe`，按中文向导安装。
3. 启动“TK试题题库”，建立分类并尝试录入、筛选和组卷。

也可以[直接下载安装包](https://github.com/BBkjdayup/BCTK/releases/download/v0.1.83/BCTK_0.1.83_windows_x64-setup.exe)。发布页提供 SHA-256 校验文件；GitHub 自动生成的 **Source code** 压缩包是源码，不能直接作为桌面程序运行。

本次提供已有 v0.1.83 Windows 安装包，文件摘要与更新签名已核对；它没有 Windows Authenticode 签名，Windows 可能显示“未知发布者”。版本来源、授权状态和校验方法见[发布说明](docs/releases/v0.1.83.md)。

安装程序使用中文向导，默认仅为当前 Windows 用户安装。若电脑缺少 WebView2，安装时会联网下载运行环境；因此，本地功能可离线使用不代表安装包可以在所有电脑上完全离线安装。

详细步骤见 [Windows 安装与首次使用](docs/Windows安装与首次使用.md)。

### 文件兼容说明

| 文件类型 | 当前范围 |
| --- | --- |
| Word `.docx` | 支持题目导入和文档导出；复杂内容需要在导入审查中核对 |
| Excel `.xlsx` | 导入文字和值；公式单元格读取文件中保存的显示值，不执行公式；导出以每题一行的文字字段和元数据为主 |
| Excel `.xls` / `.xlsm` | 需先另存为 `.xlsx` 再导入 |
| `.tqb` | 软件完整备份格式，用于备份校验与恢复 |

Word 导出暂不支持嵌套表格、外链图片、缺少 LaTeX 数据的公式节点及未经结构化转换的原始 OOXML；遇到不支持的内容会给出提示。Excel 导入不包含浮动图片和复杂公式对象，也不会将题目的富内容图片嵌入导出的单元格。

## 从源码运行

### 1. 准备开发环境

| 工具 | 要求与用途 |
| --- | --- |
| Git | 获取源码 |
| Node.js | 推荐使用本次验证的 24.x 系列 |
| pnpm | 11.19.0，已在 `package.json` 中固定 |
| Rust | stable，Windows 使用 MSVC 工具链 |
| Visual Studio 2022 Build Tools | 勾选“使用 C++ 的桌面开发”，用于生成 Windows 程序 |
| Microsoft Edge WebView2 Runtime | 运行桌面界面 |

仅预览前端界面时，需要 Node.js 和 pnpm；运行真实桌面程序还需要 Rust、C++ 构建工具和 WebView2。首次安装依赖通常需要联网。

### 2. 获取源码并安装依赖

在 PowerShell 中执行：

```powershell
git clone https://github.com/BBkjdayup/BCTK.git
cd BCTK
npm install --global pnpm@11.19.0
pnpm install --frozen-lockfile
```

### 3. 启动应用

运行真实桌面程序：

```powershell
pnpm tauri dev
```

仅预览浏览器演示界面：

```powershell
pnpm dev
```

浏览器演示模式使用演示数据，不读写真实题库文件；文档导入导出、备份恢复、目录迁移等桌面操作需要在 Tauri 程序中使用。

### 4. 构建 Windows 安装包

```powershell
pnpm build:installer
```

安装包输出目录：

```text
src-tauri/target/release/bundle/nsis/
```

该命令通过附加配置关闭自动更新签名产物的生成，不需要官方更新签名私钥。它保留现有功能、应用标识、授权逻辑及客户端更新地址，也不会生成 Windows Authenticode 签名。

独立分发修改版前，请配置自己的应用标识、服务地址和更新公钥，避免与现有安装及数据混用。完整说明见 [开源范围与构建说明](docs/open-source.md)；维护者的签名发布流程见 [自动更新说明](docs/automatic-updates.md)。

## 检查与测试

前端版本、代码规范、类型、单元测试与生产构建：

```powershell
pnpm version:check
pnpm check
pnpm build
```

Rust 格式、单元测试与严格检查：

```powershell
cargo fmt --all --manifest-path src-tauri/Cargo.toml -- --check
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

自动化测试不能替代真实 Windows 环境中的安装、打印、文档兼容和备份恢复验收。请使用测试账户与合成题库验证会修改或删除数据的流程。

## 技术组成与目录

| 技术 | 主要用途 |
| --- | --- |
| Vue 3 / TypeScript / Element Plus / Pinia | 界面、交互与状态管理 |
| Tauri 2 / Rust | 桌面集成、本地业务、文件读写及文档处理 |
| SQLite | 本地题库、分类、草稿、试卷和设置 |
| Tiptap 3 / KaTeX | 富文本录题与数学公式显示 |
| canvas-editor | 试卷分页、排版编辑与打印 |
| Rust OOXML | Word 文档读取与生成、受支持内容的结构化转换 |

```text
BCTK/
├── src/                    # Vue 界面、状态管理、业务调用与前端测试
├── src-tauri/
│   ├── src/                # Rust 本地业务与后端测试
│   ├── migrations/         # SQLite 数据库迁移
│   └── tauri.conf.json     # 桌面窗口与打包配置
├── website/                # 官网静态页面与部署示例
├── tools/license-issuer/   # 桌面授权工具源码
├── scripts/                # 版本检查、构建与验证脚本
├── docs/                   # 使用、架构和维护文档
├── CONTRIBUTING.md         # 贡献指南
├── SECURITY.md             # 安全问题报告方式
└── LICENSE                 # MIT 许可证
```

官网可独立预览和构建：

```powershell
pnpm website:dev
pnpm website:build
```

`website/` 是产品介绍与下载入口页面；在线题库应用仍属于后续开发范围。更多说明见 [官网文档](website/README.md)。

## 数据保存与备份

- 题库默认保存在本机，使用 SQLite 数据库，无需单独安装数据库服务器。
- 图片资源、模板、备份和导出文件由本地数据目录统一管理。
- 回收站的保留期限用于提醒，不会在到期时自动清空题目。
- 完整备份使用 `.tqb` 格式，包含备份清单和文件摘要校验。
- 数据目录迁移采用复制、校验、切换和保留原目录的流程。

客户端包含账号登录、云同步和冲突处理实现；使用云同步需要配套服务与相应账号权限。启用同步后，相关数据会按同步流程传输到配置的服务。本仓库不包含独立云服务端，桌面离线授权与云同步授权相互独立。

## 常见问题

**必须注册账号、购买服务器才能使用吗？** 本地题库功能无需云账号，也无需自己部署服务器。云同步是需要配套服务与账号权限的独立功能。

**开源是否意味着现有安装包的所有功能永久免费？** 当前程序保留原试用与授权流程。源码的使用、修改和分发遵循 MIT 许可，程序的默认行为见下一节。

**支持 macOS、Linux 或在线题库吗？** 当前安装包和主要验证环境为 Windows 10 / 11 x64。浏览器模式用于界面演示，不能替代桌面应用的本地文件功能；在线题库仍属于后续开发范围。

**导出的 Word 和预览会完全一样吗？** 两者使用不同的排版引擎，复杂字体、换行与页数可能存在差异。正式使用前请核对导出结果，支持范围见[文件兼容说明](#文件兼容说明)。

## 开源范围与授权

本仓库包含桌面客户端、官网、数据库迁移、测试、开发文档及授权工具源码，原创代码采用 [MIT 许可证](LICENSE)。第三方组件保留各自版权和许可，见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。

**当前源码仍保留原有的产品授权行为：**

- 首次启动提供 15 天桌面专业版试用，之后还有 15 天宽限期。
- 两段期限结束后进入基础模式，保留题库浏览、搜索、手动单题编辑、分类管理、备份恢复和迁移。
- 基础模式每份试卷最多 10 题，批量导入、文档导出和打印等功能关闭。

MIT 许可证允许按其条款使用、修改和分发源码；它不会自动改变上述程序行为，也不包含运营中的云订阅或官方签名许可证。具体规则见 [桌面授权说明](docs/desktop-licensing.md)。

公开仓库不包含用户题库、部署凭据、更新签名私钥、桌面授权签名私钥或独立云服务端。源码中的公钥用于验证签名，可以公开。修改版如需自行签发授权或提供更新，应使用自己的密钥和服务配置，详见 [开源范围与构建说明](docs/open-source.md)。

## 文档导航

| 文档 | 内容 |
| --- | --- |
| [Windows 安装与首次使用](docs/Windows安装与首次使用.md) | 安装、数据目录与卸载说明 |
| [技术方案说明](docs/技术方案说明.md) | 面向非技术人员的技术选型解释 |
| [编辑器与组卷架构](docs/编辑器与组卷架构.md) | 录题、排版、兼容和导出边界 |
| [开源范围与构建说明](docs/open-source.md) | 源码范围、本地构建与修改版分发 |
| [桌面授权说明](docs/desktop-licensing.md) | 试用、许可证与签发工具 |
| [自动更新说明](docs/automatic-updates.md) | 更新签名与发布流程 |
| [云同步说明](docs/cloud-sync-multi-device.md) | 多设备同步的数据与接口约定 |
| [变更日志](CHANGELOG.md) | 各版本对用户可见的变化 |

## 参与贡献

欢迎教师提供真实的教学需求，也欢迎开发者改进功能、修复问题、补充测试和完善文档。

尤其欢迎以下反馈：Word / Excel 导入时的最小示例、数学公式与表格的显示问题、真实教学中的分类和组卷需求，以及能让首次使用更简单的改进建议。

- 使用问题与功能建议：提交 [Issue](https://github.com/BBkjdayup/BCTK/issues)。
- 代码与文档改进：阅读 [贡献指南](CONTRIBUTING.md) 后提交 Pull Request。
- 安全问题：按 [安全报告说明](SECURITY.md) 联系维护者，避免公开真实题库、账号信息或未处理的漏洞细节。

反馈问题时，请附上软件版本、Windows 版本、复现步骤及预期结果。需要示例文件时，尽量提供不含个人信息的最小示例。

## 许可证

[MIT License](LICENSE) · Copyright (c) 2026 BBkjdayup
