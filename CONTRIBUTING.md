# 参与贡献

欢迎通过 GitHub Issues 反馈问题、提出建议，通过 Pull Request 提交改进。较大的功能调整，请先说明使用场景和预期行为。

## 本地开发

请在 Windows 上按 [README](README.md) 安装 Node.js、pnpm、Rust、Visual Studio C++ Build Tools 和 WebView2。

```powershell
pnpm install --frozen-lockfile
pnpm dev
# 运行真实桌面应用
pnpm tauri dev
```

浏览器模式使用演示数据。真实文件、数据库、打印、安装和备份恢复需要在桌面模式验证。

## 提交前验证

按改动范围执行相关检查：

```powershell
pnpm version:check
pnpm check
pnpm build
cargo fmt --all --manifest-path src-tauri/Cargo.toml -- --check
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

后端测试使用串行执行，以减少共享测试资源的干扰。已有 SQLite 迁移文件保持原有内容与 LF 换行；数据库变更通过新增迁移实现。

Pull Request 请说明问题、修改后的行为和实际运行的验证。修复缺陷时，优先增加能复现原问题的测试。

请使用合成题目、测试账号和临时数据目录。提交前检查文件清单，避免把真实题库、客户信息、授权申请、许可证、访问令牌或私钥加入仓库。安全问题请参考 [SECURITY.md](SECURITY.md)。

你提交的原创贡献按本项目的 MIT 许可证提供。引入第三方代码或素材时，保留其原有许可和版权声明，并更新第三方说明。
