# TK试题题库官网部署

以下为原项目域名和目录结构的部署示例。请在自己的环境中替换域名、证书路径、下载服务地址及备案信息。此文档不包含原服务器的实例编号、IP、凭据或内部上线记录。

## 入口和资源

- 主域名：`https://tktiku.cn/`。
- 兼容入口：`https://www.tktiku.cn/`，永久跳转至主域名并保留路径。
- 服务器：请使用自己的 Linux 主机；实例编号与公网 IP 由部署者配置。
- DNS：`@` 和 `www` 的 A 记录指向自己的服务器公网 IPv4，默认线路，TTL 600 秒。
- 云服务继续使用 `https://api.tktiku.cn/`，官网不更改其程序、数据库、授权和现有更新目录。
- 官网源码：`website/`；静态输出：`website/dist/`。
- 官网目录：`/var/www/tktiku-site/`，当前站点通过 `current` 符号链接指向版本目录里的 `dist/`。
- Nginx：`/etc/nginx/sites-available/tktiku.cn`，通过同名 `sites-enabled` 链接启用。
- 备案号：`鲁ICP备2026051053号-1`，沿用当前服务已有备案信息并链接工信部页面。

## 首次 HTTPS 配置

使用 `website/deploy/nginx-http.conf` 为两个域名开放既有 ACME 校验目录 `/var/www/certbot`。执行 `nginx -t` 成功后平滑重载，再复用服务器已配置的 Certbot 账号申请证书：

```bash
certbot certonly --webroot -w /var/www/certbot \
  --cert-name tktiku.cn -d tktiku.cn -d www.tktiku.cn \
  --non-interactive --keep-until-expiring \
  --deploy-hook 'nginx -t && systemctl reload nginx'
```

证书位于 `/etc/letsencrypt/live/tktiku.cn/`，覆盖两个官网域名。保持 `certbot.timer` 启用，并保持 HTTP ACME 路径可访问，续期后自动检查并重载 Nginx。

## 日常发布

1. 在本机完成 `pnpm website:build`。
2. 用 `tar -czf .codex-tmp/tktiku-site-release.tar.gz -C website dist deploy` 生成静态发布包。
3. 通过阿里云 Workbench 文件管理上传到服务器 `/root/`。使用已有免密连接即可，不需要创建持久 SSH 密钥或开放新端口。
4. 核对服务器与本机发布包 SHA-256。将包内的 `deploy/publish.sh` 提取至管理员目录后执行：

   ```bash
   bash /path/to/deploy/publish.sh /root/tktiku-site-release.tar.gz
   ```

发布脚本校验归档路径及类型，建立新版本目录，保留旧版，将 `current` 原子切换至新站点；保存原 Nginx 配置，语法检查通过后再重载。验证实际 HTTPS 首页与构建文件一致、版本清单可读以及云服务就绪状态。发布错误时恢复上一版链接和 Nginx 配置。

## 下载与缓存

官网同源 `/updates/latest.json` 只读映射现有 `/var/www/tktiku-updates/latest.json`，不新增云端接口。浏览器仅接受版本格式正常、且严格匹配 `https://api.tktiku.cn/updates/files/tktiku-desktop_<版本>_windows_x86_64-setup.exe` 的地址。

首页与版本清单要求重新验证缓存，带内容摘要的 CSS/JS 缓存 7 天。支持 gzip、中文 UTF-8、基础内容安全策略、拒绝目录列表和隐藏路径。Nginx 只公开构建后的静态文件，未设置把任意未知路径返回首页的规则。

## 回退

历史文件保留在 `/var/www/tktiku-site/releases/`，旧 Nginx 配置保存为 `/etc/nginx/sites-available/tktiku.cn.before-<UTC时间>`。如需回退，选择已验证的历史 `dist/`，建立临时符号链接后用 `mv -Tf` 原子替换 `current`，必要时恢复对应配置；运行 `nginx -t` 和 `systemctl reload nginx` 后重新核对 HTTPS 首页与下载入口。

官网回退不改变客户端发布清单，也不对用户题库和云数据库执行迁移。
