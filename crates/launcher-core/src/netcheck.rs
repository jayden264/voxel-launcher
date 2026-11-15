//! A quick "can I reach it, and how fast" for the hosts the launcher depends on.

use serde::Serialize;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize)]
pub struct Probe {
    pub name: String,
    pub url: String,
    /// The host answered with an HTTP response (any status: an error page still proves it is up).
    pub ok: bool,
    pub status: Option<u16>,
    pub ms: Option<u32>,
    pub error: Option<String>,
}

/// One request; never hangs longer than `timeout`.
pub async fn probe(name: &str, url: &str, timeout: Duration) -> Probe {
    let mut out = Probe { name: name.to_string(), url: url.to_string(), ok: false, status: None, ms: None, error: None };
    let client = match reqwest::Client::builder().user_agent("voxel-launcher/0.1").timeout(timeout).connect_timeout(timeout).build() {
        Ok(c) => c,
        Err(e) => {
            out.error = Some(e.to_string());
            return out;
        }
    };
    let started = Instant::now();
    match client.head(url).send().await {
        Ok(resp) => {
            out.ok = true;
            out.status = Some(resp.status().as_u16());
            out.ms = Some(started.elapsed().as_millis().min(u32::MAX as u128) as u32);
        }
        Err(e) if e.is_timeout() => out.error = Some("连接超时".into()),
        Err(e) if e.is_connect() => out.error = Some("连不上（网络不通或被拦截）".into()),
        Err(e) => out.error = Some(e.to_string()),
    }
    out
}

/// The hosts that matter, tried at the same time.
pub async fn check_all(timeout: Duration) -> Vec<Probe> {
    const HOSTS: [(&str, &str); 4] = [
        ("BMCLAPI 镜像", "https://bmclapi2.bangbang93.com/mc/game/version_manifest_v2.json"),
        ("Mojang 官方", "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"),
        ("Modrinth 接口", "https://api.modrinth.com/v2/"),
        ("Modrinth 下载", "https://cdn.modrinth.com/"),
    ];
    futures::future::join_all(HOSTS.iter().map(|(n, u)| probe(n, u, timeout))).await
}
