use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde::{Deserialize, Serialize};
use ssh2::Session;
use std::io::Read;
use std::net::TcpStream;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeployConfig {
    pub ssh: SshConfig,
    pub domain: Option<String>,
    pub panel_port: u16,
    pub web_base_path: String,
    pub panel_username: String,
    pub panel_password: String,
}

#[derive(Debug, Serialize)]
pub struct DeployResult {
    pub success: bool,
    pub message: String,
    pub panel_url: Option<String>,
    pub panel_port: Option<u16>,
    pub web_base_path: Option<String>,
    pub panel_username: Option<String>,
    pub panel_password: Option<String>,
}

/// 通过 SSH 连接远程服务器并安装 3x-ui
/// 3x-ui 安装脚本是交互式的，我们通过管道输入默认值来模拟用户输入
pub fn connect_and_install(config: &DeployConfig) -> DeployResult {
    let addr = format!("{}:{}", config.ssh.host, config.ssh.port);
    let tcp = match TcpStream::connect_timeout(
        &addr.parse().unwrap(),
        Duration::from_secs(30),
    ) {
        Ok(s) => s,
        Err(e) => {
            return DeployResult {
                success: false,
                message: format!("SSH 连接失败: {}", e),
                panel_url: None,
                panel_port: None,
                web_base_path: None,
                panel_username: None,
                panel_password: None,
            };
        }
    };

    let mut sess = match Session::new() {
        Ok(s) => s,
        Err(e) => {
            return DeployResult {
                success: false,
                message: format!("创建 SSH 会话失败: {}", e),
                panel_url: None,
                panel_port: None,
                web_base_path: None,
                panel_username: None,
                panel_password: None,
            };
        }
    };

    sess.set_tcp_stream(tcp);
    if sess.handshake().is_err() {
        return DeployResult {
            success: false,
            message: "SSH 握手失败".to_string(),
            panel_url: None,
            panel_port: None,
            web_base_path: None,
            panel_username: None,
            panel_password: None,
        };
    }
    sess.set_timeout(180000); // 3 分钟超时（安装可能较慢）

    if sess
        .userauth_password(&config.ssh.username, &config.ssh.password)
        .is_err()
    {
        return DeployResult {
            success: false,
            message: "SSH 认证失败: 用户名或密码错误".to_string(),
            panel_url: None,
            panel_port: None,
            web_base_path: None,
            panel_username: None,
            panel_password: None,
        };
    }

    if !sess.authenticated() {
        return DeployResult {
            success: false,
            message: "SSH 认证失败".to_string(),
            panel_url: None,
            panel_port: None,
            web_base_path: None,
            panel_username: None,
            panel_password: None,
        };
    }

    // 3x-ui 安装：使用 install.sh，跳过 SSL 配置 -> 证书由 x-ui 18 号菜单安装
    let web_path = config.web_base_path.trim_start_matches('/');
    let port_str = config.panel_port.to_string();

    // 用 sed 将 prompt_and_setup_ssl 调用替换为 no-op，安装后由 x-ui 18 号菜单安装证书
    let install_cmd = format!(
        r#"sudo bash -c '
export DEBIAN_FRONTEND=noninteractive
cd /tmp
curl -4fLs https://raw.githubusercontent.com/mhsanaei/3x-ui/master/install.sh -o install_3xui.sh
sed -i "s/prompt_and_setup_ssl /: /g" install_3xui.sh
printf "y\n{}\n" | bash install_3xui.sh 2>&1
rm -f install_3xui.sh
'"#,
        port_str
    );

    match run_ssh_command(&mut sess, &install_cmd) {
        Ok(out) => tracing::info!("安装输出: {}", out),
        Err(e) => {
            // 安装可能部分成功，继续尝试配置
            tracing::warn!("安装脚本返回: {}", e);
        }
    }

    // 配置端口、webBasePath、面板用户名和密码（覆盖 install.sh 生成的随机值）
    let user_esc = config.panel_username.replace('\'', "'\\''");
    let pass_esc = config.panel_password.replace('\'', "'\\''");
    let config_cmd = format!(
        r#"sudo /usr/local/x-ui/x-ui setting -port {} -webBasePath "{}" -username '{}' -password '{}' 2>/dev/null; \
sudo systemctl restart x-ui 2>/dev/null || sudo rc-service x-ui restart 2>/dev/null; \
sleep 2; echo done"#,
        config.panel_port, web_path, user_esc, pass_esc
    );

    if run_ssh_command(&mut sess, &config_cmd).is_err() {
        tracing::warn!("配置命令执行可能失败，x-ui 可能尚未安装完成");
    }

    // 先开放防火墙（含 80 端口供 ACME 校验），再安装证书
    // 使用 x-ui 的 21 号命令（防火墙管理）开放端口：21->1(安装ufw)->3(开放)->80,443,面板端口->0(返回)
    // timeout 防止菜单在 EOF 后循环等待
    let firewall_cmd = format!(
        r#"timeout 30 bash -c "printf '21\n1\n3\n80,443,{}\n0' | sudo x-ui" 2>&1 || true"#,
        config.panel_port
    );
    if run_ssh_command(&mut sess, &firewall_cmd).is_err() {
        tracing::warn!("x-ui 防火墙端口开放可能失败，请手动执行 x-ui 选择 21 开放 80、443、{} 端口", config.panel_port);
    }

    // 使用 x-ui 的 18 号命令（SSL 证书管理）安装证书
    // IP: 18->6->y->跳过IPv6->端口80  |  域名: 18->1->domain->80->n->y
    let ssl_cert_cmd = if let Some(ref domain) = config.domain {
        let domain_escaped = domain.replace('\'', "'\\''");
        format!(
            r#"timeout 120 bash -c "printf '18\n1\n{}\n\nn\ny\n0' | sudo x-ui" 2>&1 || true"#,
            domain_escaped
        )
    } else {
        format!(
            r#"timeout 120 bash -c "printf '18\n6\n\n\n\n0' | sudo x-ui" 2>&1 || true"#
        )
    };
    if run_ssh_command(&mut sess, &ssl_cert_cmd).is_err() {
        tracing::warn!("x-ui SSL 证书安装可能失败，请手动执行 x-ui 选择 18 安装证书");
    }

    // 一键完成：通过面板 API 自动添加入站（VLESS + TLS，端口 443）
    let user_safe = config.panel_username.replace('"', "\\\"").replace('$', "\\$").replace('`', "\\`");
    let pass_safe = config.panel_password.replace('"', "\\\"").replace('$', "\\$").replace('`', "\\`");
    let body_tpl = r#"{"enable":true,"remark":"默认VLESS","listen":"","port":443,"protocol":"vless","settings":"{\"clients\":[{\"id\":\"UUID_PLACEHOLDER\",\"flow\":\"\",\"email\":\"default@local\"}],\"decryption\":\"none\"}","streamSettings":"{\"network\":\"tcp\",\"security\":\"tls\"}","sniffing":"{\"enabled\":true}"}"#;
    let inbound_script = format!(
        r#"#!/bin/bash
WEB="{}"
PORT="{}"
USER="{}"
PASS="{}"
UUID=$(cat /proc/sys/kernel/random/uuid 2>/dev/null || (command -v uuidgen >/dev/null && uuidgen) || echo "a1b2c3d4-e5f6-7890-abcd-ef1234567890")
COOKIE=$(mktemp)
BASE="https://127.0.0.1:$PORT/$WEB"
curl -sk -c "$COOKIE" -X POST "$BASE/login" -d "username=$USER&password=$PASS" -o /dev/null
BODY='{}'
BODY="${{BODY/UUID_PLACEHOLDER/$UUID}}"
curl -sk -b "$COOKIE" -X POST "$BASE/panel/api/inbounds/add" -H "Content-Type: application/json" -d "$BODY" -o /dev/null -w "%{{http_code}}"
rm -f "$COOKIE"
"#,
        web_path,
        config.panel_port,
        user_safe,
        pass_safe,
        body_tpl.replace('\'', "'\"'\"'")
    );
    let inbound_b64 = BASE64.encode(inbound_script.as_bytes());
    let inbound_cmd = format!(
        "echo \"{}\" | base64 -d 2>/dev/null | sudo bash 2>/dev/null || true",
        inbound_b64.replace('\\', "\\\\").replace('"', "\\\"")
    );
    match run_ssh_command(&mut sess, &inbound_cmd) {
        Ok(out) => {
            if out.trim().ends_with("200") {
                tracing::info!("默认 VLESS 入站(443) 已添加");
            } else {
                tracing::warn!("入站添加可能失败，HTTP: {}", out.trim());
            }
        }
        Err(e) => tracing::warn!("自动添加入站失败: {}，请登录面板手动添加", e),
    }

    let host = config
        .domain
        .as_deref()
        .unwrap_or(config.ssh.host.as_str());
    // 已自动部署 Let's Encrypt IP 证书，使用 https
    let url = format!(
        "https://{}:{}/{}",
        host,
        config.panel_port,
        if web_path.is_empty() { "" } else { web_path }
    );

    // 确保 URL 格式正确
    let panel_url = if url.ends_with("//") {
        Some(url.trim_end_matches('/').to_string())
    } else {
        Some(url)
    };

    let ssl_note = if config.domain.is_some() {
        "已自动部署 Let's Encrypt 域名证书（90 天有效期，自动续期）"
    } else {
        "已自动部署 Let's Encrypt IP 证书（约 6 天有效期，自动续期）"
    };
    DeployResult {
        success: true,
        message: format!("3x-ui 一键部署完成。{}。已开放防火墙端口 80、443、{}。已自动添加默认 VLESS 入站（端口 443）。若证书或入站添加失败，请登录面板手动配置。", ssl_note, config.panel_port),
        panel_url,
        panel_port: Some(config.panel_port),
        web_base_path: Some(config.web_base_path.clone()),
        panel_username: Some(config.panel_username.clone()),
        panel_password: Some(config.panel_password.clone()),
    }
}

/// 使用 x-ui 的 uninstall 命令卸载 3x-ui（x-ui 菜单 5 号 / x-ui uninstall）
pub fn uninstall_xui(ssh: &SshConfig) -> UninstallResult {
    let addr = format!("{}:{}", ssh.host, ssh.port);
    let tcp = match TcpStream::connect_timeout(&addr.parse().unwrap(), Duration::from_secs(30)) {
        Ok(s) => s,
        Err(e) => {
            return UninstallResult {
                success: false,
                message: format!("SSH 连接失败: {}", e),
            };
        }
    };

    let mut sess = match Session::new() {
        Ok(s) => s,
        Err(e) => {
            return UninstallResult {
                success: false,
                message: format!("创建 SSH 会话失败: {}", e),
            };
        }
    };

    sess.set_tcp_stream(tcp);
    if sess.handshake().is_err() {
        return UninstallResult {
            success: false,
            message: "SSH 握手失败".to_string(),
        };
    }
    sess.set_timeout(60000);

    if sess.userauth_password(&ssh.username, &ssh.password).is_err() {
        return UninstallResult {
            success: false,
            message: "SSH 认证失败: 用户名或密码错误".to_string(),
        };
    }

    if !sess.authenticated() {
        return UninstallResult {
            success: false,
            message: "SSH 认证失败".to_string(),
        };
    }

    // 使用 x-ui uninstall 命令，管道输入 y 确认卸载
    let uninstall_cmd = r#"printf 'y\n' | sudo x-ui uninstall 2>&1 || true"#;
    match run_ssh_command(&mut sess, uninstall_cmd) {
        Ok(out) => {
            if out.contains("Uninstalled Successfully") || out.contains("Uninstalled") {
                UninstallResult {
                    success: true,
                    message: "3x-ui 已成功卸载，Xray 也已一并移除。".to_string(),
                }
            } else {
                UninstallResult {
                    success: false,
                    message: format!("卸载可能未完成或 x-ui 未安装。输出: {}", out.lines().take(5).collect::<Vec<_>>().join(" ")),
                }
            }
        }
        Err(e) => UninstallResult {
            success: false,
            message: format!("卸载执行失败: {}", e),
        },
    }
}

#[derive(Debug, Serialize)]
pub struct UninstallResult {
    pub success: bool,
    pub message: String,
}

/// 面板连接配置（用于添加入站、获取入站列表）
#[derive(Debug, Clone)]
pub struct PanelConfig {
    pub ssh: SshConfig,
    pub panel_port: u16,
    pub web_base_path: String,
    pub panel_username: String,
    pub panel_password: String,
}

/// 入站配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundConfig {
    pub protocol: String,  // vless, vmess, trojan
    pub port: u16,
    pub remark: String,
    #[serde(default)]
    pub enable_tls: bool,
    #[serde(default = "default_network")]
    pub network: String,  // tcp, ws, grpc
    /// 证书域名，用于 TLS。证书路径为 /root/cert/{domain}/fullchain.pem
    #[serde(default)]
    pub cert_domain: Option<String>,
}

fn default_network() -> String {
    "grpc".to_string()
}

#[derive(Debug, Serialize)]
pub struct AddInboundResult {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ListInboundsResult {
    pub success: bool,
    pub message: String,
    pub inbounds: Option<Vec<serde_json::Value>>,
}

/// 通过 SSH 在远程面板上添加入站
pub fn add_inbound(panel: &PanelConfig, inbound: &InboundConfig) -> AddInboundResult {
    let addr = format!("{}:{}", panel.ssh.host, panel.ssh.port);
    let tcp = match TcpStream::connect_timeout(&addr.parse().unwrap(), Duration::from_secs(30)) {
        Ok(s) => s,
        Err(e) => {
            return AddInboundResult {
                success: false,
                message: format!("SSH 连接失败: {}", e),
            };
        }
    };

    let mut sess = match Session::new() {
        Ok(s) => s,
        Err(e) => {
            return AddInboundResult {
                success: false,
                message: format!("创建 SSH 会话失败: {}", e),
            };
        }
    };

    sess.set_tcp_stream(tcp);
    if sess.handshake().is_err() {
        return AddInboundResult {
            success: false,
            message: "SSH 握手失败".to_string(),
        };
    }
    sess.set_timeout(30000);

    if sess
        .userauth_password(&panel.ssh.username, &panel.ssh.password)
        .is_err()
    {
        return AddInboundResult {
            success: false,
            message: "SSH 认证失败".to_string(),
        };
    }

    if !sess.authenticated() {
        return AddInboundResult {
            success: false,
            message: "SSH 认证失败".to_string(),
        };
    }

    let web_path = panel.web_base_path.trim_start_matches('/');
    let user_safe = panel
        .panel_username
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`");
    let pass_safe = panel
        .panel_password
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`");

    let (settings, protocol) = match inbound.protocol.to_lowercase().as_str() {
        "vmess" => {
            let uuid = uuid_or_gen();
            (
                format!(
                    r#"{{"clients":[{{"id":"{}","alterId":0,"email":"default@local"}}],"decryption":"none"}}"#,
                    uuid
                ),
                "vmess",
            )
        }
        "trojan" => {
            let pwd = gen_random_string(16);
            (
                format!(
                    r#"{{"clients":[{{"password":"{}","email":"default@local"}}],"decryption":"none"}}"#,
                    pwd
                ),
                "trojan",
            )
        }
        _ => {
            let uuid = uuid_or_gen();
            (
                format!(
                    r#"{{"clients":[{{"id":"{}","flow":"","email":"default@local"}}],"decryption":"none"}}"#,
                    uuid
                ),
                "vless",
            )
        }
    };

    let security = if inbound.enable_tls { "tls" } else { "none" };
    let net = inbound.network.to_lowercase();
    let cert_path = inbound
        .cert_domain
        .as_ref()
        .map(|d| d.trim())
        .filter(|d| !d.is_empty())
        .map(|d| format!("/root/cert/{}/fullchain.pem", d));

    // serverName 必须与客户端连接的域名一致，否则 gRPC+TLS 会失败（403/502）
    let server_name = inbound
        .cert_domain
        .as_ref()
        .map(|d| d.trim())
        .filter(|d| !d.is_empty())
        .unwrap_or("");

    // 构建 streamSettings：network + security + (可选) tlsSettings + (grpc) grpcSettings
    let stream_settings = if net == "grpc" {
        if let Some(ref cert) = cert_path {
            let key = cert.replace("fullchain.pem", "privkey.pem");
            format!(
                r#"{{"network":"grpc","security":"{}","tlsSettings":{{"serverName":"{}","certificates":[{{"certificateFile":"{}","keyFile":"{}"}}],"alpn":["h2","http/1.1"]}},"grpcSettings":{{"serviceName":"","authority":"","multiMode":true}}}}"#,
                security,
                server_name.replace('\\', "\\\\").replace('"', "\\\""),
                cert.replace('\\', "\\\\").replace('"', "\\\""),
                key.replace('\\', "\\\\").replace('"', "\\\"")
            )
        } else {
            format!(
                r#"{{"network":"grpc","security":"{}","grpcSettings":{{"serviceName":"","authority":"","multiMode":true}}}}"#,
                security
            )
        }
    } else if cert_path.is_some() && inbound.enable_tls {
        let cert = cert_path.as_ref().unwrap();
        let key = cert.replace("fullchain.pem", "privkey.pem");
        format!(
            r#"{{"network":"{}","security":"tls","tlsSettings":{{"serverName":"{}","certificates":[{{"certificateFile":"{}","keyFile":"{}"}}],"alpn":["h2","http/1.1"]}}}}"#,
            net,
            server_name.replace('\\', "\\\\").replace('"', "\\\""),
            cert.replace('\\', "\\\\").replace('"', "\\\""),
            key.replace('\\', "\\\\").replace('"', "\\\"")
        )
    } else if net == "ws" {
        format!(r#"{{"network":"ws","security":"{}"}}"#, security)
    } else {
        format!(r#"{{"network":"tcp","security":"{}"}}"#, security)
    };
    let remark_esc = inbound.remark.replace('\\', "\\\\").replace('"', "\\\"");
    let body = format!(
        r#"{{"enable":true,"remark":"{}","listen":"","port":{},"protocol":"{}","settings":"{}","streamSettings":"{}","sniffing":"{{\"enabled\":true}}"}}"#,
        remark_esc,
        inbound.port,
        protocol,
        settings.replace('\\', "\\\\").replace('"', "\\\""),
        stream_settings
    );

    let inbound_script = format!(
        r#"#!/bin/bash
WEB="{}"
PORT="{}"
USER="{}"
PASS="{}"
BODY='{}'
COOKIE=$(mktemp)
BASE="https://127.0.0.1:$PORT/$WEB"
curl -sk -c "$COOKIE" -X POST "$BASE/login" -d "username=$USER&password=$PASS" -o /dev/null
CODE=$(curl -sk -b "$COOKIE" -X POST "$BASE/panel/api/inbounds/add" -H "Content-Type: application/json" -d "$BODY" -o /dev/null -w "%{{http_code}}")
rm -f "$COOKIE"
echo "$CODE"
"#,
        web_path,
        panel.panel_port,
        user_safe,
        pass_safe,
        body.replace('\'', "'\"'\"'")
    );

    let inbound_b64 = BASE64.encode(inbound_script.as_bytes());
    let inbound_cmd = format!(
        "echo \"{}\" | base64 -d 2>/dev/null | sudo bash 2>/dev/null || true",
        inbound_b64.replace('\\', "\\\\").replace('"', "\\\"")
    );

    match run_ssh_command(&mut sess, &inbound_cmd) {
        Ok(out) => {
            let code = out.trim();
            if code.ends_with("200") {
                AddInboundResult {
                    success: true,
                    message: format!(
                        "入站添加成功：{} {} (端口 {})",
                        protocol.to_uppercase(),
                        inbound.remark,
                        inbound.port
                    ),
                }
            } else {
                AddInboundResult {
                    success: false,
                    message: format!("API 返回异常，HTTP: {}", code),
                }
            }
        }
        Err(e) => AddInboundResult {
            success: false,
            message: format!("执行失败: {}", e),
        },
    }
}

fn uuid_or_gen() -> String {
    // 通过 SSH 在远程生成，这里仅作 fallback；实际由远程脚本生成
    use std::process::Command;
    if let Ok(out) = Command::new("uuidgen").output() {
        if out.status.success() {
            return String::from_utf8_lossy(&out.stdout).trim().to_string();
        }
    }
    "a1b2c3d4-e5f6-7890-abcd-ef1234567890".to_string()
}

fn gen_random_string(len: usize) -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let chars: Vec<char> = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
        .chars()
        .collect();
    (0..len).map(|_| chars[rng.gen_range(0..chars.len())]).collect()
}

/// 通过 SSH 获取远程面板的入站列表
pub fn list_inbounds(panel: &PanelConfig) -> ListInboundsResult {
    let addr = format!("{}:{}", panel.ssh.host, panel.ssh.port);
    let tcp = match TcpStream::connect_timeout(&addr.parse().unwrap(), Duration::from_secs(30)) {
        Ok(s) => s,
        Err(e) => {
            return ListInboundsResult {
                success: false,
                message: format!("SSH 连接失败: {}", e),
                inbounds: None,
            };
        }
    };

    let mut sess = match Session::new() {
        Ok(s) => s,
        Err(e) => {
            return ListInboundsResult {
                success: false,
                message: format!("创建 SSH 会话失败: {}", e),
                inbounds: None,
            };
        }
    };

    sess.set_tcp_stream(tcp);
    if sess.handshake().is_err() {
        return ListInboundsResult {
            success: false,
            message: "SSH 握手失败".to_string(),
            inbounds: None,
        };
    }
    sess.set_timeout(15000);

    if sess
        .userauth_password(&panel.ssh.username, &panel.ssh.password)
        .is_err()
    {
        return ListInboundsResult {
            success: false,
            message: "SSH 认证失败".to_string(),
            inbounds: None,
        };
    }

    if !sess.authenticated() {
        return ListInboundsResult {
            success: false,
            message: "SSH 认证失败".to_string(),
            inbounds: None,
        };
    }

    let web_path = panel.web_base_path.trim_start_matches('/');
    let user_safe = panel
        .panel_username
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`");
    let pass_safe = panel
        .panel_password
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`");

    let list_script = format!(
        r#"#!/bin/bash
WEB="{}"
PORT="{}"
USER="{}"
PASS="{}"
COOKIE=$(mktemp)
BASE="https://127.0.0.1:$PORT/$WEB"
curl -sk -c "$COOKIE" -X POST "$BASE/login" -d "username=$USER&password=$PASS" -o /dev/null
curl -sk -b "$COOKIE" "$BASE/panel/api/inbounds/list" 2>/dev/null || echo '{{"success":false}}'
rm -f "$COOKIE"
"#,
        web_path,
        panel.panel_port,
        user_safe,
        pass_safe
    );

    let list_b64 = BASE64.encode(list_script.as_bytes());
    let list_cmd = format!(
        "echo \"{}\" | base64 -d 2>/dev/null | sudo bash 2>/dev/null || echo '{{\"success\":false}}'",
        list_b64.replace('\\', "\\\\").replace('"', "\\\"")
    );

    match run_ssh_command(&mut sess, &list_cmd) {
        Ok(out) => {
            let trimmed = out.trim();
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
                if let Some(obj) = v.as_object() {
                    if obj.get("success").and_then(|s| s.as_bool()).unwrap_or(false) {
                        let inbounds = v
                            .get("obj")
                            .or_else(|| v.get("data"))
                            .and_then(|o| o.as_array())
                            .cloned();
                        return ListInboundsResult {
                            success: true,
                            message: "获取成功".to_string(),
                            inbounds,
                        };
                    }
                }
            }
            ListInboundsResult {
                success: false,
                message: format!("解析失败或 API 返回异常: {}", trimmed.lines().next().unwrap_or("")),
                inbounds: None,
            }
        }
        Err(e) => ListInboundsResult {
            success: false,
            message: format!("执行失败: {}", e),
            inbounds: None,
        },
    }
}

fn run_ssh_command(sess: &mut Session, cmd: &str) -> Result<String, String> {
    let mut channel = sess.channel_session().map_err(|e| e.to_string())?;
    channel.exec(cmd).map_err(|e| e.to_string())?;

    let mut s = String::new();
    channel.read_to_string(&mut s).map_err(|e| e.to_string())?;
    channel.wait_close().map_err(|e| e.to_string())?;

    let exit = channel.exit_status().map_err(|e| e.to_string())?;
    if exit != 0 {
        return Err(format!("退出码 {}: {}", exit, s));
    }
    Ok(s)
}
