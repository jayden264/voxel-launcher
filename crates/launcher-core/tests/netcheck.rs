//! "Can I reach it, and how fast": the network test in the settings.

use launcher_core::netcheck::probe;
use std::time::Duration;
use wiremock::matchers::any;
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn a_server_that_answers_is_reachable_with_its_status_and_latency() {
    let server = MockServer::start().await;
    Mock::given(any()).respond_with(ResponseTemplate::new(200)).mount(&server).await;
    let p = probe("测试", &server.uri(), Duration::from_secs(3)).await;
    assert!(p.ok);
    assert_eq!((p.name.as_str(), p.status), ("测试", Some(200)));
    assert!(p.ms.is_some() && p.error.is_none());
}

#[tokio::test]
async fn any_http_answer_counts_as_reachable_even_an_error_page() {
    let server = MockServer::start().await;
    Mock::given(any()).respond_with(ResponseTemplate::new(404)).mount(&server).await;
    let p = probe("cdn", &server.uri(), Duration::from_secs(3)).await;
    assert!(p.ok, "a 404 still proves the host is up");
    assert_eq!(p.status, Some(404));
}

#[tokio::test]
async fn a_refused_connection_is_a_failure_with_a_reason() {
    let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let p = probe("down", &format!("http://127.0.0.1:{free}/"), Duration::from_secs(2)).await;
    assert!(!p.ok);
    assert!(p.status.is_none() && p.ms.is_none());
    assert!(p.error.is_some());
}

#[tokio::test]
async fn a_server_that_never_answers_times_out_instead_of_hanging() {
    let server = MockServer::start().await;
    Mock::given(any()).respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(5))).mount(&server).await;
    let started = std::time::Instant::now();
    let p = probe("slow", &server.uri(), Duration::from_millis(400)).await;
    assert!(!p.ok);
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(p.error.unwrap().contains("超时"));
}
