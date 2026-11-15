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
