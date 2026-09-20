# 开源范围与构建说明

## 源码来源与许可

本仓库最初以 TK试题题库 v0.1.83 的可公开源码快照建立，现已更新到 v0.1.88。公开仓库保留面向社区的独立提交历史；此前的内部开发历史、验收材料和生产发布记录由维护者单独保管。

原创代码使用 [MIT 许可证](../LICENSE)，允许按许可证条款使用、修改、分发和商用，并要求保留版权与许可声明。第三方组件继续遵守各自许可，见 [THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md)。

## 仓库包含什么

- Windows 桌面客户端的 Vue / TypeScript 界面和 Rust 本地后端。
- SQLite 迁移、Word / Excel 导入导出、编辑、组卷、备份恢复相关实现与测试。
- 产品官网、网页管理端前端、构建脚本和公开开发文档。
- 小程序发布与成员管理的桌面客户端接入代码。

仓库不包含：

- 独立 Rust 云服务端和微信小程序客户端。
- 真实用户题库、生产数据库、部署配置、账号凭据或内部验收材料。
- 官方更新签名私钥、口令、服务器密钥或其他生产秘密。
- 安装包、发布归档、内部 Git bundle 与本机构建缓存。

## 免费桌面功能与在线服务

从 v0.1.85 开始，桌面本地功能免费开放，不再包含设备绑定、试用期、离线许可证或题量许可限制。旧题库云同步功能也已退役，客户端不会在后台整库上传本机题库。

小程序发布属于可选在线功能。用户主动登录并发布时，客户端只上传所选试卷的发布快照及受支持资源；账号、套餐、成员和手机答题由配套服务处理。MIT 源码许可不包含运营中的云服务、账号或套餐权益。

客户端仍保留官方应用标识、更新地址和可选在线服务地址。将修改版作为独立产品分发前，应配置自己的应用标识、品牌、服务端点、更新公钥和网站身份信息，不应把修改版标示为本项目的官方发布。

当前官方应用标识为 `com.zhitiku.desktop`。使用相同标识安装可能会与官方应用及其本地数据关联；开发和安装验证应使用专用 Windows 账户及合成题库。

## 不使用官方私钥构建

```powershell
pnpm install --frozen-lockfile
pnpm build:installer
```

`build:installer` 通过 `src-tauri/tauri.local-build.conf.json` 将 `bundle.createUpdaterArtifacts` 设为 `false`，因此只构建普通安装包，不需要官方更新签名私钥。

该设置不会自动更改应用标识、服务地址或更新公钥。独立分发版本必须使用自己的配置和密钥；官方签名更新的工作原理见 [automatic-updates.md](automatic-updates.md)。

## 验证建议

提交代码前至少运行：

```powershell
pnpm version:check
pnpm check
pnpm build
pnpm website:build
pnpm admin:build

cargo fmt --all --manifest-path src-tauri/Cargo.toml -- --check
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib -- --test-threads=1
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

每次正式发布还应在隔离 Windows 环境中验证安装、升级、打印、文档兼容、备份恢复和卸载行为。公开源码不等于本机安装、在线服务、软件签名或第三方依赖合规已经自动完成新的全面审计。
