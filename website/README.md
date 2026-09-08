# TK试题题库官网

官网地址：<https://tktiku.cn/>。`www.tktiku.cn` 跳转至主域名，沿用现有阿里云 ECS。

## 本地开发

在桌面项目根目录运行：

```powershell
pnpm website:dev
pnpm website:build
```

开发地址为 `http://127.0.0.1:4178/`，发布文件位于 `website/dist/`。官网复用项目已经安装的 Vite，不加载桌面程序、不调用 Tauri，也不要求新增运行时服务。

- `index.html`：产品介绍、操作示意、功能、使用流程、下载及 FAQ。
- `styles.css`：响应式样式，包含桌面、平板、手机及减少动画偏好。
- `main.js`：从同源 `/updates/latest.json` 更新下载入口与版本信息；失败时保留已验证的版本化下载链接。
- `public/`：项目现有品牌图片、favicon、robots.txt、sitemap.xml。
- `deploy/`：独立 Nginx 配置和保留历史版本的发布脚本。

## 维护内容

官网使用项目既有 Logo，不包含外部字体、分析脚本、注册表单或虚构的用户数量、客户评价、价格。操作示意使用公开示例题，并标明“功能示意 · 示例数据”。

更新功能说明时，以客户端当前实现和最新发布记录为准。桌面专业版与云同步授权相互独立；不要把 Windows 客户端首页称为网页版题库。

发布新客户端后，官网会读取现有更新清单，校验版本格式和官方安装包 URL 后更新下载按钮。修改官网时也应更新 HTML 中的静态下载链接、版本和日期，使禁用 JavaScript 或服务不可用时仍能下载已验证的安装包。

## 部署

部署步骤与配置示例见 [官网部署说明](../docs/website-deployment.md)。只打包 `dist` 和 `deploy`：

```powershell
pnpm website:build
tar -czf .codex-tmp/tktiku-site-release.tar.gz -C website dist deploy
```

不要上传整个桌面工程，或把数据目录、安装签名私钥、服务器密钥放入公开目录。
