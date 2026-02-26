# 3x-ui 代理服务部署工具

基于 Axum 的 Web 应用，通过 SSH 连接远程服务器，一键安装 [3x-ui](https://github.com/MHSanaei/3x-ui) 代理面板。

## 功能

- **一键部署**：网页配置后自动完成全部步骤
- 通过 SSH 连接远程服务器，执行 3x-ui 安装
- 配置面板端口、Web 路径、用户名密码
- 自动安装 Let's Encrypt SSL 证书
- 自动开放防火墙端口（80、443、面板端口）
- **自动添加默认 VLESS 入站**（端口 443，TLS）

## 3x-ui 说明

3x-ui 是 Xray-core 的 Web 管理面板，支持 VLESS、VMess、Trojan、ShadowSocks、WireGuard 等协议。

- **安装命令**：`bash <(curl -Ls https://raw.githubusercontent.com/mhsanaei/3x-ui/master/install.sh)`
- **默认端口**：6789
- **默认账号**：admin / admin（首次登录后请立即修改）
- **管控页面**：`http://IP:端口/路径` 或 `https://域名:端口/路径`

## 运行

```bash
cargo run
```

访问 http://localhost:3000 打开配置页面。

## 配置项

| 字段 | 说明 |
|------|------|
| 服务器 IP/域名 | 目标服务器地址 |
| SSH 端口 | 默认 22 |
| 账户名 | SSH 登录用户名（通常为 root） |
| 密码 | SSH 登录密码 |
| 访问域名 | 可选，用于 HTTPS 访问 |
| 面板端口 | 3x-ui 管控页面端口，默认 6789 |
| Web 路径 | 管控页面路径，默认 panel |

## 自动 SSL 证书部署

通过 **x-ui 的 18 号命令（SSL 证书管理）** 自动安装证书：

- **有域名**：18 → 1（Get SSL Domain）→ 域名 → 80 → n → y
- **仅 IP**：18 → 6（Get SSL for IP）→ y → 跳过 IPv6 → 端口 80

- **前置条件**：服务器 80 端口需可从公网访问；使用域名时需已解析到服务器

### 证书本地缓存

- **申请成功后**：证书会自动保存到本地 `./certs/{域名}/`（fullchain.pem、privkey.pem）
- **下次部署**：若相同域名且本地证书距今未超过 7 天，将直接推送本地证书到服务器，跳过 ACME 申请
- 适用于同一域名多台服务器部署，或频繁重装场景

## 自动开放防火墙端口

部署时通过 **x-ui 的 21 号命令（防火墙管理）** 自动开放端口：

- **80**：Let's Encrypt ACME 校验
- **443**：HTTPS 代理常用端口
- **面板端口**：3x-ui 管控页面（默认 6789）

执行流程：`x-ui` → 21 → 1(安装 ufw) → 3(开放端口) → `80,443,面板端口` → 0(返回)

## 自动配置入站列表

3x-ui 入站列表支持以下自动配置方式：

### 方式一：面板内「通用操作」批量导入

1. 登录面板 → 入站列表 → 点击 **通用操作**
2. 选择 **批量导入** 或 **从剪贴板导入**
3. 粘贴入站 JSON 配置（可包含多个入站及客户端）

### 方式二：调用面板 API

面板提供 REST API，需先登录获取 session 后调用：

- **登录**：`POST /login`（表单：username, password）
- **添加入站**：`POST /panel/api/inbounds/add`（JSON body）
- **批量导入**：`POST /panel/api/inbounds/import`（表单字段 `data` 为 JSON 字符串）

示例（需先登录，session 通过 cookie 保持）：

```bash
# 1. 登录（basePath 为 panel 时，登录地址为 /panel/login）
curl -c cookies.txt -X POST "https://你的域名:6789/panel/login" \
  -d "username=admin&password=admin"

# 2. 添加入站（API 路径为 /panel/panel/api/inbounds/add）
curl -b cookies.txt -X POST "https://你的域名:6789/panel/panel/api/inbounds/add" \
  -H "Content-Type: application/json" \
  -d '{"enable":true,"remark":"默认VLESS","listen":"","port":443,"protocol":"vless","settings":"{\"clients\":[{\"id\":\"生成的UUID\",\"flow\":\"\",\"email\":\"user@example.com\"}],\"decryption\":\"none\"}","streamSettings":"{\"network\":\"tcp\",\"security\":\"tls\"}","sniffing":"{\"enabled\":true}"}'
```

> 入站 JSON 结构较复杂，建议先在面板内手动添加一个入站，再通过「通用操作」→「导出」获取模板后修改批量导入。

### 方式三：部署后手动添加

部署完成后，在入站列表点击 **添加入站**，按需选择协议（VLESS/VMess/Trojan 等）、端口、传输方式后保存。

---

## 注意事项

1. 目标服务器需支持 SSH 密码登录
2. 安装脚本需要 root 权限
3. 3x-ui 官方安装脚本为交互式，本工具通过管道模拟输入，部分复杂交互可能需手动处理
4. 若安装失败，可 SSH 登录服务器后手动执行：`bash <(curl -Ls https://raw.githubusercontent.com/mhsanaei/3x-ui/master/install.sh)`
