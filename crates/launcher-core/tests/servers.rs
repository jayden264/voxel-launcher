//! Saved servers: address parsing and the Minecraft status ping.

use launcher_core::servers::{parse_address, ping};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn varint(mut v: u32) -> Vec<u8> {
    let mut out = Vec::new();
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
    inner.extend(body);
    let mut out = varint(inner.len() as u32);
    out.extend(inner);
    out
}

async fn read_varint(s: &mut tokio::net::TcpStream) -> u32 {
    let (mut v, mut shift) = (0u32, 0);
    loop {
        let b = s.read_u8().await.unwrap();
        v |= ((b & 0x7f) as u32) << shift;
        if b & 0x80 == 0 {
            return v;
        }
        shift += 7;
    }
}

/// A one-shot fake server answering the status handshake with `json`; returns its port.
async fn fake_server(json: String, answer_ping: bool) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (mut s, _) = listener.accept().await.unwrap();
        // handshake
        let len = read_varint(&mut s).await as usize;
        let mut hs = vec![0u8; len];
        s.read_exact(&mut hs).await.unwrap();
        assert_eq!(hs[0], 0x00, "handshake packet id");
        assert_eq!(*hs.last().unwrap(), 1, "next state = status");
        // status request
        let len = read_varint(&mut s).await as usize;
        let mut req = vec![0u8; len];
        s.read_exact(&mut req).await.unwrap();
        assert_eq!(req, [0x00]);
        let mut body = varint(json.len() as u32);
        body.extend(json.as_bytes());
        s.write_all(&packet(0x00, &body)).await.unwrap();
        if answer_ping {
            let len = read_varint(&mut s).await as usize;
            let mut p = vec![0u8; len];
            s.read_exact(&mut p).await.unwrap();
            s.write_all(&packet(0x01, &p[1..])).await.unwrap();
        }
    });
    port
}

#[test]
fn addresses_default_to_the_standard_port_and_reject_junk() {
    assert_eq!(parse_address("play.example.com").unwrap(), ("play.example.com".into(), 25565));
    assert_eq!(parse_address("  play.example.com:25566 ").unwrap(), ("play.example.com".into(), 25566));
    assert_eq!(parse_address("[::1]:2556").unwrap(), ("[::1]".into(), 2556));
    assert_eq!(parse_address("192.168.1.5").unwrap(), ("192.168.1.5".into(), 25565));
    for bad in ["", "   ", ":25565", "a b", "host:0", "host:99999", "host:abc", "-bad.com", "a;b.com", "a/b"] {
        assert!(parse_address(bad).is_err(), "{bad:?}");
    }
}

#[tokio::test]
async fn pings_a_server_and_reads_players_version_and_motd() {
    let json = r#"{"version":{"name":"Paper 1.21.1","protocol":767},"players":{"max":100,"online":7},
                   "description":{"text":"Hello ","extra":[{"text":"World","color":"gold"}]},
                   "favicon":"data:image/png;base64,AAAA"}"#;
    let port = fake_server(json.to_string(), true).await;

    let s = ping("127.0.0.1", port, Duration::from_secs(3)).await.unwrap();

    assert_eq!((s.online, s.max), (7, 100));
    assert_eq!((s.version.as_str(), s.protocol), ("Paper 1.21.1", 767));
    assert_eq!(s.motd, "Hello World");
    assert_eq!(s.favicon.as_deref(), Some("data:image/png;base64,AAAA"));
    assert!(s.latency_ms < 3000);
}

#[tokio::test]
async fn a_plain_string_motd_with_legacy_colour_codes_is_cleaned() {
    let json = r#"{"version":{"name":"1.8","protocol":47},"players":{"max":20,"online":0},"description":"§aGreen §lBold§r plain"}"#;
    let port = fake_server(json.to_string(), true).await;
    assert_eq!(ping("127.0.0.1", port, Duration::from_secs(3)).await.unwrap().motd, "Green Bold plain");
}

#[tokio::test]
async fn a_server_that_ignores_the_ping_still_reports_its_status() {
    let json = r#"{"version":{"name":"x","protocol":1},"players":{"max":1,"online":1},"description":"hi"}"#;
    let port = fake_server(json.to_string(), false).await;
    let s = ping("127.0.0.1", port, Duration::from_millis(800)).await.unwrap();
    assert_eq!(s.motd, "hi");
}

#[tokio::test]
async fn an_oversized_favicon_is_dropped_and_odd_json_does_not_panic() {
    let big = "A".repeat(200_000);
    let json = format!(r#"{{"version":{{"name":"x","protocol":1}},"players":{{"max":1,"online":1}},"description":"d","favicon":"data:image/png;base64,{big}"}}"#);
    let port = fake_server(json, true).await;
    assert_eq!(ping("127.0.0.1", port, Duration::from_secs(3)).await.unwrap().favicon, None);

    let port = fake_server("{}".to_string(), true).await;
    let s = ping("127.0.0.1", port, Duration::from_secs(3)).await.unwrap();
    assert_eq!((s.online, s.max, s.motd.as_str()), (0, 0, ""));
}

#[tokio::test]
async fn unreachable_and_silent_servers_are_errors_not_hangs() {
    // Nothing listens on this port.
    let free = TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap().port();
    assert!(ping("127.0.0.1", free, Duration::from_millis(800)).await.is_err());

    // Accepts but never answers.
    let silent = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = silent.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (_s, _) = silent.accept().await.unwrap();
        tokio::time::sleep(Duration::from_secs(5)).await;
    });
    let started = std::time::Instant::now();
    assert!(ping("127.0.0.1", port, Duration::from_millis(500)).await.is_err());
    assert!(started.elapsed() < Duration::from_secs(3));
}

// -------------------------------------------------------- login check

use launcher_core::servers::{check_login, login_start_body, LoginCheck};

fn string(s: &str) -> Vec<u8> {
    let mut v = varint(s.len() as u32);
    v.extend(s.as_bytes());
    v
}

fn disconnect(text: &str) -> Vec<u8> {
    packet(0x00, &string(&format!(r#"{{"text":"{text}"}}"#)))
}

/// A fake server that reads the handshake and login start, remembers the login-start bytes,
/// and answers with whatever `reply` is.
async fn login_server(reply: Vec<u8>) -> (u16, tokio::sync::oneshot::Receiver<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let (mut s, _) = listener.accept().await.unwrap();
        let len = read_varint(&mut s).await as usize;
        let mut hs = vec![0u8; len];
        s.read_exact(&mut hs).await.unwrap();
        assert_eq!(*hs.last().unwrap(), 2, "next state = login");
        let len = read_varint(&mut s).await as usize;
        let mut start = vec![0u8; len];
        s.read_exact(&mut start).await.unwrap();
        let _ = tx.send(start);
        s.write_all(&reply).await.unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
    });
    (port, rx)
}

async fn check(reply: Vec<u8>) -> LoginCheck {
    let (port, _rx) = login_server(reply).await;
    check_login("127.0.0.1", port, 767, Duration::from_secs(3)).await.unwrap()
}

#[tokio::test]
async fn a_server_that_lets_us_in_is_open_to_offline_accounts() {
    let mut success = vec![0u8; 16]; // uuid
    success.extend(string("VoxelCheck"));
    success.extend(varint(0)); // no properties
    assert_eq!(check(packet(0x02, &success)).await, LoginCheck::Open);
}

#[tokio::test]
async fn compression_before_login_success_still_means_open() {
    let mut reply = packet(0x03, &varint(256));
    assert_eq!(check(std::mem::take(&mut reply)).await, LoginCheck::Open);
}

#[tokio::test]
async fn an_encryption_request_means_the_server_wants_a_real_account() {
    let mut body = string(""); // server id
    body.extend(varint(3));
    body.extend([1, 2, 3]); // public key
    body.extend(varint(4));
    body.extend([9, 9, 9, 9]); // verify token
    assert_eq!(check(packet(0x01, &body)).await, LoginCheck::OnlineOnly);
}

#[tokio::test]
async fn kicks_are_sorted_by_what_the_server_says() {
    let cases = [
        ("You are not white-listed on this server!", "whitelist"),
        ("你不在白名单中", "whitelist"),
        ("Outdated client! Please use 1.20.4", "version"),
        ("Incompatible client! Please use 1.8.9", "version"),
        ("You are banned from this server", "banned"),
        ("Server is restarting", "refused"),
    ];
    for (text, want) in cases {
        let got = check(disconnect(text)).await;
        let kind = match &got {
            LoginCheck::Whitelist => "whitelist",
            LoginCheck::WrongVersion { .. } => "version",
            LoginCheck::Banned => "banned",
            LoginCheck::Refused { .. } => "refused",
            other => panic!("{text}: unexpected {other:?}"),
        };
        assert_eq!(kind, want, "{text}");
    }
    match check(disconnect("Server is restarting")).await {
        LoginCheck::Refused { reason } => assert_eq!(reason, "Server is restarting"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_server_that_never_answers_the_login_is_an_error_not_a_hang() {
    let silent = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = silent.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (_s, _) = silent.accept().await.unwrap();
        tokio::time::sleep(Duration::from_secs(5)).await;
    });
    let t = std::time::Instant::now();
    assert!(check_login("127.0.0.1", port, 767, Duration::from_millis(500)).await.is_err());
    assert!(t.elapsed() < Duration::from_secs(3));
}

#[test]
fn login_start_follows_each_protocol_generation() {
    let uuid = [0xAB; 16];
    let name = "VoxelCheck";
    let n = string(name);
    // 1.7–1.18: just the name.
    assert_eq!(login_start_body(47, name, uuid), n);
    assert_eq!(login_start_body(758, name, uuid), n);
    // 1.19: name, "no signature data".
    assert_eq!(login_start_body(759, name, uuid), [n.clone(), vec![0]].concat());
    // 1.19.1–1.19.2: name, no signature data, no uuid.
    assert_eq!(login_start_body(760, name, uuid), [n.clone(), vec![0, 0]].concat());
    // 1.19.3–1.20.1: name, then an optional uuid (sent).
    assert_eq!(login_start_body(763, name, uuid), [n.clone(), vec![1], uuid.to_vec()].concat());
    // 1.20.2 and later: name and a required uuid.
    assert_eq!(login_start_body(764, name, uuid), [n.clone(), uuid.to_vec()].concat());
    assert_eq!(login_start_body(773, name, uuid), [n, uuid.to_vec()].concat());
}

#[tokio::test]
async fn the_probe_sends_the_servers_own_protocol_and_identifies_itself() {
    let (port, rx) = login_server(disconnect("bye")).await;
    let _ = check_login("127.0.0.1", port, 765, Duration::from_secs(3)).await;
    let start = rx.await.unwrap();
    assert_eq!(start[0], 0x00, "login start packet id");
    assert!(String::from_utf8_lossy(&start).contains("VoxelCheck"), "admins can tell what this was");
    assert_eq!(start.len(), 1 + 1 + "VoxelCheck".len() + 16, "name + 16 byte uuid for protocol 765");
}
