//! Saved servers and the Minecraft "server list ping" that tells whether one is up.

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub const DEFAULT_PORT: u16 = 25565;
const MAX_PACKET: usize = 2 * 1024 * 1024;
const MAX_FAVICON_CHARS: usize = 128 * 1024;
const PING_WAIT: Duration = Duration::from_millis(1000);

/// A server the user saved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerEntry {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServerStatus {
    /// The message of the day, as plain text.
    pub motd: String,
    pub online: u32,
    pub max: u32,
    pub version: String,
    pub protocol: i64,
    pub latency_ms: u32,
    /// `data:image/png;base64,…`
    pub favicon: Option<String>,
}

/// `host`, `host:port`, `[v6]` or `[v6]:port`; the port defaults to 25565.
pub fn parse_address(input: &str) -> Result<(String, u16)> {
    let bad = || Error::Other(format!("无效的服务器地址: {}", input.trim()));
    let s = input.trim();
    let (host, port) = if let Some(rest) = s.strip_prefix('[') {
        let end = rest.find(']').ok_or_else(bad)?;
        let host = format!("[{}]", &rest[..end]);
        if !rest[..end].chars().all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.') || end == 0 {
            return Err(bad());
        }
        let tail = &rest[end + 1..];
        match tail.strip_prefix(':') {
            Some(p) => (host, Some(p)),
            None if tail.is_empty() => (host, None),
            None => return Err(bad()),
        }
    } else {
        match s.matches(':').count() {
            0 => (s.to_string(), None),
            1 => {
                let (h, p) = s.split_once(':').expect("one colon");
                (h.to_string(), Some(p))
            }
            _ => return Err(bad()),
        }
    };
    let host_ok = !host.is_empty()
        && host.len() <= 253
        && !host.starts_with('-')
        && (host.starts_with('[') || host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')));
    if !host_ok {
        return Err(bad());
    }
    let port = match port {
        Some(p) => p.parse::<u16>().ok().filter(|p| *p != 0).ok_or_else(bad)?,
        None => DEFAULT_PORT,
    };
    Ok((host, port))
}

// ---------------------------------------------------------------- protocol

fn varint(mut v: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(5);
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return out;
        }
        out.push(b | 0x80);
    }
}

fn packet(id: u32, body: &[u8]) -> Vec<u8> {
    let mut inner = varint(id);
    inner.extend_from_slice(body);
    let mut out = varint(inner.len() as u32);
    out.extend(inner);
    out
}

async fn read_varint(s: &mut TcpStream) -> Result<u32> {
    let (mut value, mut shift) = (0u32, 0);
    loop {
        let b = s.read_u8().await?;
        value |= ((b & 0x7f) as u32) << shift;
        if b & 0x80 == 0 {
            return Ok(value);
        }
        shift += 7;
        if shift > 28 {
            return Err(Error::Other("服务器返回了无法识别的数据".into()));
        }
    }
}

/// One packet: its id and the bytes after it.
async fn read_packet(s: &mut TcpStream) -> Result<(u32, Vec<u8>)> {
    let len = read_varint(s).await? as usize;
    if len == 0 || len > MAX_PACKET {
        return Err(Error::Other("服务器返回了无法识别的数据".into()));
    }
    let mut buf = vec![0u8; len];
    s.read_exact(&mut buf).await?;
    let id_len = buf.iter().position(|b| b & 0x80 == 0).map(|p| p + 1).ok_or_else(|| Error::Other("服务器返回了无法识别的数据".into()))?;
    let mut id = 0u32;
    for (i, b) in buf[..id_len].iter().enumerate() {
        id |= ((b & 0x7f) as u32) << (7 * i);
    }
    Ok((id, buf[id_len..].to_vec()))
}

fn flatten(v: &serde_json::Value, out: &mut String) {
    match v {
        serde_json::Value::String(s) => out.push_str(s),
        serde_json::Value::Array(a) => a.iter().for_each(|e| flatten(e, out)),
        serde_json::Value::Object(o) => {
            if let Some(t) = o.get("text") {
                flatten(t, out);
            }
            if let Some(e) = o.get("extra") {
                flatten(e, out);
            }
        }
        _ => {}
    }
}

/// Drop `§x` formatting codes.
fn strip_codes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next();
        } else {
            out.push(c);
        }
    }
    out
}

async fn ping_inner(host: &str, port: u16) -> Result<ServerStatus> {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    let mut s = TcpStream::connect((bare, port)).await.map_err(|e| Error::Other(format!("连不上服务器: {e}")))?;
    let started = Instant::now();

    // Handshake: protocol -1 (any), address, port, next state 1 (status); then the status request.
    let mut hs = varint(u32::MAX);
    hs.extend(varint(host.len() as u32));
    hs.extend(host.as_bytes());
    hs.extend(port.to_be_bytes());
    hs.extend(varint(1));
    s.write_all(&packet(0x00, &hs)).await?;
    s.write_all(&packet(0x00, &[])).await?;

    let (id, body) = read_packet(&mut s).await?;
    let status_rtt = started.elapsed();
    if id != 0x00 {
        return Err(Error::Other("服务器返回了无法识别的数据".into()));
    }
    // The JSON is a length-prefixed string.
    let n = body.iter().position(|b| b & 0x80 == 0).map(|p| p + 1).unwrap_or(0);
    let json: serde_json::Value = serde_json::from_slice(body.get(n..).unwrap_or(&[])).map_err(|_| Error::Other("服务器返回了无法识别的数据".into()))?;

    // A real round trip when the server answers a ping; otherwise the status round trip stands in.
    let ping_start = Instant::now();
    let ponged = async {
        s.write_all(&packet(0x01, &0i64.to_be_bytes())).await.ok()?;
        read_packet(&mut s).await.ok().filter(|(id, _)| *id == 0x01)
    };
    let latency = match tokio::time::timeout(PING_WAIT, ponged).await {
        Ok(Some(_)) => ping_start.elapsed(),
        _ => status_rtt,
    };

    let mut motd = String::new();
    if let Some(d) = json.get("description") {
        flatten(d, &mut motd);
    }
    let favicon = json
        .get("favicon")
        .and_then(|f| f.as_str())
        .filter(|f| f.starts_with("data:image/png;base64,") && f.len() <= MAX_FAVICON_CHARS)
        .map(str::to_string);
    Ok(ServerStatus {
        motd: strip_codes(motd.trim()),
        online: json.pointer("/players/online").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        max: json.pointer("/players/max").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        version: json.pointer("/version/name").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        protocol: json.pointer("/version/protocol").and_then(|v| v.as_i64()).unwrap_or(0),
        latency_ms: latency.as_millis().min(u32::MAX as u128) as u32,
        favicon,
    })
}

/// Ask a server who is on it. Fails (never hangs) when it is down or silent.
pub async fn ping(host: &str, port: u16, timeout: Duration) -> Result<ServerStatus> {
    tokio::time::timeout(timeout, ping_inner(host, port)).await.map_err(|_| Error::Other("连接超时，服务器可能已离线".into()))?
}
