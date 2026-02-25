use axum::response::IntoResponse;
use serde::Deserialize;
use tokio::task::spawn_blocking;

use crate::ssh_client::{
    add_inbound, connect_and_install, list_inbounds, uninstall_xui, AddInboundResult, DeployConfig,
    DeployResult, InboundConfig, ListInboundsResult, PanelConfig, SshConfig, UninstallResult,
};

pub async fn index() -> impl IntoResponse {
    let html = include_str!("../static/index.html");
    axum::response::Html(html)
}

#[derive(Debug, Deserialize)]
pub struct DeployRequest {
    pub host: String,
    pub port: Option<u16>,
    pub username: String,
    pub password: String,
    pub domain: Option<String>,
    pub panel_port: Option<u16>,
    pub web_base_path: Option<String>,
    pub panel_username: Option<String>,
    pub panel_password: Option<String>,
}

pub async fn deploy(
    axum::Json(req): axum::Json<DeployRequest>,
) -> impl IntoResponse {
    let port = req.port.unwrap_or(22);
    let panel_port = req.panel_port.unwrap_or(6789);
    let web_base_path = req
        .web_base_path
        .unwrap_or_else(|| "panel".to_string())
        .trim()
        .to_string();
    let web_base_path = if web_base_path.is_empty() {
        "panel".to_string()
    } else {
        web_base_path
    };
    let panel_username = req
        .panel_username
        .unwrap_or_else(|| "admin".to_string())
        .trim()
        .to_string();
    let panel_username = if panel_username.is_empty() {
        "admin".to_string()
    } else {
        panel_username
    };
    let panel_password = req
        .panel_password
        .unwrap_or_else(|| "admin".to_string())
        .trim()
        .to_string();
    let panel_password = if panel_password.is_empty() {
        "admin".to_string()
    } else {
        panel_password
    };

    let ssh_username = req.username.trim();
    let ssh_username = if ssh_username.is_empty() {
        "root".to_string()
    } else {
        ssh_username.to_string()
    };
    let config = DeployConfig {
        ssh: crate::ssh_client::SshConfig {
            host: req.host.trim().to_string(),
            port,
            username: ssh_username,
            password: req.password,
        },
        domain: req.domain.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        panel_port,
        web_base_path,
        panel_username,
        panel_password,
    };

    // SSH 和安装是阻塞操作，放到 blocking 线程池
    let result = spawn_blocking(move || connect_and_install(&config))
        .await
        .unwrap_or_else(|e| DeployResult {
            success: false,
            message: format!("任务执行失败: {}", e),
            panel_url: None,
            panel_port: None,
            web_base_path: None,
            panel_username: None,
            panel_password: None,
        });

    axum::Json(result)
}

#[derive(Debug, Deserialize)]
pub struct UninstallRequest {
    pub host: String,
    pub port: Option<u16>,
    pub username: String,
    pub password: String,
}

pub async fn uninstall(
    axum::Json(req): axum::Json<UninstallRequest>,
) -> impl IntoResponse {
    let port = req.port.unwrap_or(22);
    let ssh_username = req.username.trim();
    let ssh_username = if ssh_username.is_empty() {
        "root".to_string()
    } else {
        ssh_username.to_string()
    };
    let ssh = SshConfig {
        host: req.host.trim().to_string(),
        port,
        username: ssh_username,
        password: req.password,
    };

    let result = spawn_blocking(move || uninstall_xui(&ssh))
        .await
        .unwrap_or_else(|e| UninstallResult {
            success: false,
            message: format!("任务执行失败: {}", e),
        });

    axum::Json(result)
}

#[derive(Debug, Deserialize)]
pub struct AddInboundRequest {
    pub host: String,
    pub port: Option<u16>,
    pub username: String,
    pub password: String,
    pub panel_port: Option<u16>,
    pub web_base_path: Option<String>,
    pub panel_username: Option<String>,
    pub panel_password: Option<String>,
    pub protocol: Option<String>,
    pub inbound_port: Option<u16>,
    pub remark: Option<String>,
    pub enable_tls: Option<bool>,
    pub network: Option<String>,
    /// 证书域名，证书路径 /root/cert/{domain}/，如 www.ettreasure.com
    pub cert_domain: Option<String>,
}

pub async fn add_inbound_handler(
    axum::Json(req): axum::Json<AddInboundRequest>,
) -> impl IntoResponse {
    let port = req.port.unwrap_or(22);
    let panel_port = req.panel_port.unwrap_or(6789);
    let web_base_path = req
        .web_base_path
        .unwrap_or_else(|| "panel".to_string())
        .trim()
        .to_string();
    let web_base_path = if web_base_path.is_empty() {
        "panel".to_string()
    } else {
        web_base_path
    };
    let panel_username = req
        .panel_username
        .unwrap_or_else(|| "admin".to_string())
        .trim()
        .to_string();
    let panel_username = if panel_username.is_empty() {
        "admin".to_string()
    } else {
        panel_username
    };
    let panel_password = req
        .panel_password
        .unwrap_or_else(|| "admin".to_string())
        .trim()
        .to_string();
    let panel_password = if panel_password.is_empty() {
        "admin".to_string()
    } else {
        panel_password
    };
    let ssh_username = req.username.trim();
    let ssh_username = if ssh_username.is_empty() {
        "root".to_string()
    } else {
        ssh_username.to_string()
    };
    let inbound = InboundConfig {
        protocol: req
            .protocol
            .unwrap_or_else(|| "vmess".to_string())
            .trim()
            .to_lowercase(),
        port: req.inbound_port.unwrap_or(443),
        remark: req.remark.unwrap_or_else(|| "et".to_string()),
        enable_tls: req.enable_tls.unwrap_or(true),
        network: req
            .network
            .unwrap_or_else(|| "grpc".to_string())
            .trim()
            .to_lowercase(),
        cert_domain: req
            .cert_domain
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
    };
    let panel = PanelConfig {
        ssh: SshConfig {
            host: req.host.trim().to_string(),
            port,
            username: ssh_username,
            password: req.password,
        },
        panel_port,
        web_base_path,
        panel_username,
        panel_password,
    };

    let result = spawn_blocking(move || add_inbound(&panel, &inbound))
        .await
        .unwrap_or_else(|e| AddInboundResult {
            success: false,
            message: format!("任务执行失败: {}", e),
        });

    axum::Json(result)
}

#[derive(Debug, Deserialize)]
pub struct ListInboundsRequest {
    pub host: String,
    pub port: Option<u16>,
    pub username: String,
    pub password: String,
    pub panel_port: Option<u16>,
    pub web_base_path: Option<String>,
    pub panel_username: Option<String>,
    pub panel_password: Option<String>,
}

pub async fn list_inbounds_handler(
    axum::Json(req): axum::Json<ListInboundsRequest>,
) -> impl IntoResponse {
    let port = req.port.unwrap_or(22);
    let panel_port = req.panel_port.unwrap_or(6789);
    let web_base_path = req
        .web_base_path
        .unwrap_or_else(|| "panel".to_string())
        .trim()
        .to_string();
    let web_base_path = if web_base_path.is_empty() {
        "panel".to_string()
    } else {
        web_base_path
    };
    let panel_username = req
        .panel_username
        .unwrap_or_else(|| "admin".to_string())
        .trim()
        .to_string();
    let panel_username = if panel_username.is_empty() {
        "admin".to_string()
    } else {
        panel_username
    };
    let panel_password = req
        .panel_password
        .unwrap_or_else(|| "admin".to_string())
        .trim()
        .to_string();
    let panel_password = if panel_password.is_empty() {
        "admin".to_string()
    } else {
        panel_password
    };
    let ssh_username = req.username.trim();
    let ssh_username = if ssh_username.is_empty() {
        "root".to_string()
    } else {
        ssh_username.to_string()
    };
    let panel = PanelConfig {
        ssh: SshConfig {
            host: req.host.trim().to_string(),
            port,
            username: ssh_username,
            password: req.password,
        },
        panel_port,
        web_base_path,
        panel_username,
        panel_password,
    };

    let result = spawn_blocking(move || list_inbounds(&panel))
        .await
        .unwrap_or_else(|e| ListInboundsResult {
            success: false,
            message: format!("任务执行失败: {}", e),
            inbounds: None,
        });

    axum::Json(result)
}
