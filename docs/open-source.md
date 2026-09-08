# 开源范围与构建说明

## 源码来源与许可

本仓库基于 TK试题题库 v0.1.83 的源码快照，原始交付提交为 `75353c226e5c440e37d115046e73f359af4bd15e`。公开仓库从整理后的源码开始记录历史；此前的完整开发历史与内部验收记录由维护者单独保管。

原创代码使用 [MIT 许可证](../LICENSE)，允许使用、修改、分发和商用，要求保留版权与许可声明。第三方组件继续遵守各自许可，见 [THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md)。

## 包含的内容

- Windows 桌面客户端的 Vue / TypeScript 界面和 Rust 本地后端。
- SQLite 迁移、Word / Excel 导入导出、编辑、组卷、备份恢复相关实现与测试。
- 客户端云同步实现、官网静态源码与部署示例。
- 桌面授权管理工具源码、校验公钥和更新构建脚本。

独立云服务端、真实用户数据、部署凭据、签名私钥、内部验收报告和历史 Git bundle 不包含在公开仓库中。公开公钥用于验证签名，无法用于签发官方授权或官方更新。

## 现有授权与服务配置

此次整理没有改变产品业务代码。原有免费试用、基础模式、桌面专业版授权和云同步逻辑仍然保留。MIT 许可授权对源码的使用和修改，不代表提供运营中的云账号、云订阅或官方签名许可证。

客户端仍包含 `api.tktiku.cn` 的云服务和更新地址。官网包含原项目的名称、Logo、下载地址及备案信息。将修改版作为独立产品分发前，应配置自己的应用标识、服务地址、更新公钥和网站身份信息；不应把自己的版本标示成原项目的官方发布。

当前应用标识为 `com.zhitiku.desktop`。使用相同标识安装会与现有应用及其数据关联。开发和安装验证应使用专用 Windows 账户及合成题库。

## 不使用官方私钥构建

```powershell
pnpm install --frozen-lockfile
pnpm build:installer
```

该命令通过 `src-tauri/tauri.local-build.conf.json` 将 `bundle.createUpdaterArtifacts` 设为 `false`，只关闭更新签名产物生成。它不修改试用、授权、应用标识或客户端自动更新逻辑，也不生成 Windows Authenticode 签名。

本地构建适合开发验证。独立分发版本需要另行配置自己的更新渠道及密钥；签名构建说明见 [automatic-updates.md](automatic-updates.md)，桌面授权说明见 [desktop-licensing.md](desktop-licensing.md)。

## 验证记录

旧版文档中的完成状态是历史开发说明，不是每个新构建的验收结果。每次发布应按 [CONTRIBUTING.md](../CONTRIBUTING.md) 运行相关检查，并在 GitHub Release 记录实际验证、已知限制和产物校验值。公开源码不等于本机安装、在线更新、云同步或第三方依赖合规已完成新的全面审计。
