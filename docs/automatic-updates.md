# 软件自动更新发布说明

## 客户端行为

- Windows 桌面端启动约 8 秒后自动检查 `https://api.tktiku.cn/updates/latest.json`。
- 同一个新版本在 24 小时内只自动提醒一次；用户可在“系统设置 → 关于软件 → 软件更新”随时手动检查。
- 软件不会静默强制安装。用户确认后才下载，安装前提醒保存编辑内容，并显示下载进度。
- 安装包必须通过 Tauri Updater 的发布签名校验；HTTPS 服务器返回的下载地址或文件即使被篡改，没有私钥签名也不能安装。
- 更新检查或下载失败不会阻止软件启动，也不会影响本地录题、组卷或题库数据。

## 首次启用边界

`0.1.74` 是第一版带自动更新能力的客户端。已经安装 `0.1.73` 或更早版本的用户必须手工安装一次 `0.1.74`；从 `0.1.74` 升级到后续版本时即可使用软件内更新。

## 发布密钥

- 客户端只内置发布公钥，公钥可公开。
- 私钥保存在当前发布电脑的 `%USERPROFILE%\.tauri\tktiku-updater.key`，不在源码仓库或服务器中。
- 私钥口令保存在 `%USERPROFILE%\.tauri\tktiku-updater.password.dpapi`，由 Windows 当前用户凭据加密。
- 丢失私钥或口令后，已经安装的客户端将无法验证任何新密钥签发的更新。正式发布前必须把私钥和口令分别做离线加密备份。
- 更新签名与 Windows Authenticode 代码签名是两套机制。Updater 签名负责阻止伪造更新；如果没有购买代码签名证书，Windows 仍可能显示“未知发布者”。

## 构建发布包

在项目根目录执行：

```powershell
pnpm release:update
```

如需把单独的纯文本文件作为更新说明：

```powershell
pwsh -NoProfile -File scripts/build-signed-update.ps1 -NotesFile docs/update-notes.txt
```

脚本会读取受 Windows 保护的签名口令，运行 Tauri 发布构建，并在 `.codex-tmp/desktop-updates/<版本>/` 生成：

- `latest.json`：静态更新清单。
- `files/tktiku-desktop_<版本>_windows_x86_64-setup.exe`：供更新器下载的安装包。
- 同名 `.sig`：发布签名留档；清单中已经嵌入签名内容。

发布脚本把版本化安装包写入服务器的 `/var/www/tktiku-updates/files/`，最后原子替换 `/var/www/tktiku-updates/latest.json`。该公开静态目录与 `/opt/zhitiku` 下的云服务程序、数据库和密钥隔离。必须先上传安装包，确认公网可下载并校验 SHA-256，最后再发布清单，避免客户端读到尚不存在的文件。

## 回滚原则

- Tauri 默认只接受版本号高于当前安装版本的更新，不通过降低清单版本执行自动降级。
- 出现问题时先撤下 `latest.json` 或恢复上一份清单以停止继续分发，然后修复并发布更高的新版本。
- 已完成更新的客户端继续保留本地 SQLite 数据目录；安装包更新不应删除或覆盖用户题库。
