# Windows 安装与首次使用

## 安装哪个文件

普通用户请前往 [TK试题题库官网](https://tktiku.cn/#download)，下载当前 Windows 10 / 11 x64 安装包。

GitHub 自动生成的 **Source code** 是源码压缩包，不是安装程序。开发者自行构建的安装包位于 `src-tauri/target/release/bundle/nsis/`。

安装包使用简体中文向导。电脑已有 WebView2 时不会重复下载；若缺少 WebView2，安装程序会联网获取运行环境。安装完成后，题库管理、录题、组卷、导入、导出、打印和备份等本地功能可以离线使用。

## 安装步骤

1. 关闭正在运行的旧版 TK试题题库。
2. 从官网下载安装包，按中文向导完成安装。
3. 安装方式为“仅当前用户”，不会自动给电脑上的其他账户安装。
4. 从开始菜单或桌面快捷方式打开“TK试题题库”。

覆盖安装或软件内更新不会删除正常的本地题库目录。首次在重要数据上升级前，仍建议先创建 `.tqb` 完整备份。

## Windows 显示“未知发布者”是什么意思

当前安装包没有 Windows Authenticode 代码签名，因此 Windows 可能显示“未知发布者”或 SmartScreen 提示。这表示 Windows 无法通过商业代码签名证书确认发布者身份，不等于文件必然损坏。

请从官方页面下载。软件内自动更新还会使用独立的 Tauri Updater 签名验证安装包，签名不匹配的更新不会被安装。

## 数据保存在哪里

首次启动默认在当前用户“文档”目录创建 `教师题库` 文件夹，其中保存数据库、图片资源、模板、备份和导出文件。

正常卸载时：

- 不勾选删除选项：保留题库及本机设置，重新安装后可继续使用。
- 勾选“删除本机题库和设置”：只删除软件登记并通过安全校验的受管内容；数据根目录中不属于软件的其他文件会保留。
- 安全校验失败时，卸载器会保留数据，不会猜测路径后强制删除。

v0.1.85 起，桌面本地功能没有试用期、设备绑定或离线许可证。旧版本遗留的授权状态不再参与功能判断。

卸载前仍应创建 `.tqb` 完整备份。首次安装、覆盖安装和卸载数据测试应放在 Windows Sandbox、虚拟机或一次性测试账户中，不要拿唯一一份真实题库做实验。

## 第一次使用建议

1. 先创建一个测试学科和章节。
2. 手动录入一道不含个人信息的示例题。
3. 尝试把题目加入试卷并打开排版页面。
4. 创建一份 `.tqb` 备份并完成校验。
5. 确认流程符合预期后，再导入正式教学资料。

## 如何核对源码版本

开发者可在项目根目录运行：

```powershell
pnpm install --frozen-lockfile
pnpm version:check
pnpm check
pnpm build

cargo fmt --all --manifest-path src-tauri/Cargo.toml -- --check
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

v0.1.88 的源码变化与验证范围见 [发布说明](releases/v0.1.88.md)。未购买商业代码签名证书时，自行构建的安装包同样会显示为未签名。
