# Third-Party Notices

本项目可以作为商业软件分发，但第三方开源组件仍保留各自版权。当前编辑器方案不收取按份版权费；分发时必须保留本文件及适用的许可证声明。本文件不是法律意见。

## 关键编辑器组件

- Tiptap 3.31.0 (`@tiptap/core`、`@tiptap/vue-3`、StarterKit 与所列扩展)：MIT License，Copyright (c) 2025 Tiptap GmbH。
- ProseMirror（由 `@tiptap/pm` 使用）：MIT License，各上游作者保留版权。
- canvas-editor 0.9.137 (`@hufe921/canvas-editor`)：MIT License，Copyright (c) 2021-present, hufe。

本项目只使用上述项目的开源包，不包含 Tiptap Pro、Tiptap Cloud 或其他需要商业订阅的服务。

## 其他主要组件

- Vue、Vue Router、Pinia、Element Plus、SortableJS：MIT License。
- Tauri：Apache License 2.0 / MIT License。
- Reqwest、Hyper 与 Rustls：Apache License 2.0 / MIT License，用于通过 HTTPS 连接自建云服务，不收取按用户或按请求费用。
- SQLite：Public Domain。
- Calamine 0.36.1：MIT License，用于在本机安全读取 `.xlsx` 题库文件；软件不会执行工作簿公式，也不依赖 Microsoft Office、WPS Office 或云服务。
- Rust 依赖的具体版本与许可证以 `src-tauri/Cargo.lock` 和各 crate 随附许可证为准。

锁文件审计还包含少量 MPL-2.0 组件：前端构建链中的 `lightningcss`，以及 Tauri 依赖树中的 `cssparser`、`cssparser-macros`、`dtoa-short`、`option-ext` 和 `selectors`。本项目未修改这些组件；其对应源代码可按 `pnpm-lock.yaml` 或 `src-tauri/Cargo.lock` 中的精确版本从 npm、crates.io 及各项目主页免费取得。MPL-2.0 不收取商用版权费，但如果以后修改这些 MPL 文件并分发，修改后的对应文件仍需按 MPL-2.0 提供源代码。

## MathType formula conversion

- `cfb` 0.14 is used only to read the `Equation Native` stream inside an OLE Compound File. It is distributed under the MIT License; Copyright (c) 2017 Matthew Steele.
- KaTeX 0.18.1 renders the converted editable LaTeX formulas inside the application. KaTeX is distributed under the MIT License; Copyright (c) 2013-2020 Khan Academy and other contributors.
- The MathType converter in this project is an independent, bounded implementation of the published MTEF 5 file format. It does not bundle or invoke MathType, the MathType SDK, Microsoft Office, WPS Office, or LibreOffice, and it never activates embedded OLE objects.

## Rich DOCX export

- `tex2word-math` 1.0.6 converts the application's editable LaTeX formula nodes to native Office Math Markup Language (OMML). It is distributed under the MIT License.
- `scraper` 0.27.0 and `ego-tree` 0.11.0 parse the application's bounded rich-content HTML into the DOCX export model. Both are distributed under the ISC License.
- These components run locally and do not require Microsoft Office, WPS Office, LibreOffice, a cloud service, or a per-document royalty.

## MIT License text

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

发布前应使用锁文件重新生成完整生产依赖清单，确认没有新增 GPL/AGPL、商业条款或来源不明的包。
