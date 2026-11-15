//! Behavioural tests for the downloader against a local mock HTTP server.

use launcher_core::download::{sha1_bytes, DownloadTask, Downloader};
use launcher_core::mirror::Source;
use launcher_core::Error;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use wiremock::matchers::{header_exists, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn downloader() -> Downloader {
    Downloader::new(Source::Official).with_retry(2, Duration::from_millis(10))
}

fn payload() -> Vec<u8> {
    (0..4000u32).map(|i| (i % 251) as u8).collect()
}

#[tokio::test]
async fn retries_after_server_errors() {
    let server = MockServer::start().await;
    let data = payload();
    Mock::given(method("GET"))
        .and(path("/f"))
        .respond_with(ResponseTemplate::new(500))
        .up_to_n_times(2)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/f"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(data.clone()))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("a/b/f.bin");
    let task = DownloadTask::new(format!("{}/f", server.uri()), dest.clone())
        .sha1(Some(sha1_bytes(&data)))
        .size(Some(data.len() as u64));
    downloader().download_one(task).await.unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), data);
    assert!(!dir.path().join("a/b/f.bin.part").exists());
    assert_eq!(server.received_requests().await.unwrap().len(), 3);
}

#[tokio::test]
async fn resumes_from_part_file_with_range() {
    let server = MockServer::start().await;
    let data = payload();
    let half = 1500;
    Mock::given(method("GET"))
        .and(path("/f"))
        .and(header_exists("range"))
        .respond_with(ResponseTemplate::new(206).set_body_bytes(data[half..].to_vec()))
        .expect(1)
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("f.bin");
    std::fs::write(dir.path().join("f.bin.part"), &data[..half]).unwrap();
    let task = DownloadTask::new(format!("{}/f", server.uri()), dest.clone())
        .sha1(Some(sha1_bytes(&data)))
        .size(Some(data.len() as u64));
    downloader().download_one(task).await.unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), data);

    let reqs = server.received_requests().await.unwrap();
    let range = reqs[0].headers.get("range").unwrap().to_str().unwrap().to_string();
    assert_eq!(range, format!("bytes={half}-"));
}

#[tokio::test]
async fn corrupt_resume_deletes_part_and_restarts_from_scratch() {
    let server = MockServer::start().await;
    let data = payload();
    // A stale .part whose bytes do not belong to this file: the ranged
    // response completes it into a corrupt file.
    Mock::given(method("GET"))
        .and(path("/f"))
        .and(header_exists("range"))
        .respond_with(ResponseTemplate::new(206).set_body_bytes(data[5..].to_vec()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/f"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(data.clone()))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("f.bin");
    std::fs::write(dir.path().join("f.bin.part"), b"XXXXX").unwrap();
    let task = DownloadTask::new(format!("{}/f", server.uri()), dest.clone())
        .sha1(Some(sha1_bytes(&data)))
        .size(Some(data.len() as u64));
    downloader().download_one(task).await.unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), data);

    let reqs = server.received_requests().await.unwrap();
    assert_eq!(reqs.len(), 2, "one ranged attempt, then exactly one clean restart");
    assert!(reqs[0].headers.get("range").is_some());
    assert!(reqs[1].headers.get("range").is_none(), "restart must not use Range");
}

#[tokio::test]
async fn falls_back_to_official_when_mirror_fails_or_serves_bad_bytes() {
    let server = MockServer::start().await;
    let data = payload();
    // "mirror" serves wrong bytes with a 200; "official" is correct.
    Mock::given(method("GET"))
        .and(path("/mirror/f"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![7u8; data.len()]))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/mirror/missing"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/official/f"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(data.clone()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/official/missing"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(data.clone()))
        .mount(&server)
        .await;

    let base = server.uri();
    let dl = Downloader::new(Source::Official)
        .with_retry(1, Duration::from_millis(5))
        .with_resolver(Arc::new(move |u: &str| {
            let file = u.rsplit('/').next().unwrap();
            vec![format!("{base}/mirror/{file}"), format!("{base}/official/{file}")]
        }));

    let dir = tempfile::tempdir().unwrap();
    for name in ["f", "missing"] {
        let task = DownloadTask::new(format!("https://example.invalid/{name}"), dir.path().join(name))
            .sha1(Some(sha1_bytes(&data)))
            .size(Some(data.len() as u64));
        dl.download_one(task).await.unwrap();
        assert_eq!(std::fs::read(dir.path().join(name)).unwrap(), data, "{name}");
    }
}

#[tokio::test]
async fn fetch_checked_skips_a_source_that_serves_reformatted_bytes() {
    let server = MockServer::start().await;
    let official = br#"{"files":{"bin":{"type":"directory"}}}"#.to_vec();
    // The mirror re-serialises the JSON (same meaning, different bytes).
    Mock::given(method("GET"))
        .and(path("/mirror/manifest.json"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(br#"{"files": {"bin": {"type": "directory"}}}"#.to_vec()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/official/manifest.json"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(official.clone()))
        .mount(&server)
        .await;
    let base = server.uri();
    let dl = Downloader::new(Source::Official)
        .with_retry(1, Duration::from_millis(5))
        .with_resolver(Arc::new(move |_| {
            vec![format!("{base}/mirror/manifest.json"), format!("{base}/official/manifest.json")]
        }));
    let got = dl.fetch_checked("https://example.invalid/m", Some(&sha1_bytes(&official))).await.unwrap();
    assert_eq!(got, official);
    // without a checksum the first source wins
    let first = dl.fetch("https://example.invalid/m").await.unwrap();
    assert_ne!(first, official);
}

#[tokio::test]
async fn a_404_from_the_mirror_switches_source_without_retrying() {
    let server = MockServer::start().await;
    let data = payload();
    Mock::given(method("GET"))
        .and(path("/mirror/f"))
        .respond_with(ResponseTemplate::new(404))
        .expect(1) // exactly one attempt, not 1 + retries
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/official/f"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(data.clone()))
        .mount(&server)
        .await;
    let base = server.uri();
    let dl = Downloader::new(Source::Official)
        .with_retry(3, Duration::from_millis(400))
        .with_resolver(Arc::new(move |_| vec![format!("{base}/mirror/f"), format!("{base}/official/f")]));
    let dir = tempfile::tempdir().unwrap();
    let started = std::time::Instant::now();
    dl.download_one(DownloadTask::new("https://example.invalid/f", dir.path().join("f")).size(Some(data.len() as u64)))
        .await
        .unwrap();
    assert!(started.elapsed() < Duration::from_millis(1500), "took {:?}", started.elapsed());
    assert_eq!(std::fs::read(dir.path().join("f")).unwrap(), data);
}

#[tokio::test]
async fn unreachable_mirror_fails_over_immediately_and_is_remembered() {
    let official = MockServer::start().await;
    let data = payload();
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(data.clone()))
        .mount(&official)
        .await;
    // A port nobody listens on: connections are refused. (wiremock pools its
    // servers, so a dropped MockServer is *not* reliably closed.)
    let dead_base = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        format!("http://{}", l.local_addr().unwrap())
    };

    let ok_base = official.uri();
    let dead_for_resolver = dead_base.clone();
    let dl = Downloader::new(Source::Official)
        .with_retry(3, Duration::from_millis(400)) // would take seconds if it retried the dead host
        .with_resolver(Arc::new(move |u: &str| {
            let file = u.rsplit('/').next().unwrap();
            vec![format!("{dead_for_resolver}/{file}"), format!("{ok_base}/{file}")]
        }));
    assert!(!dl.is_host_dead(&dead_base));
    let dir = tempfile::tempdir().unwrap();
    let started = std::time::Instant::now();
    for i in 0..5 {
        let t = DownloadTask::new(format!("https://example.invalid/f{i}"), dir.path().join(format!("f{i}")))
            .size(Some(data.len() as u64));
        dl.download_one(t).await.unwrap();
    }
    assert!(started.elapsed() < Duration::from_secs(3), "took {:?}", started.elapsed());
    assert!(dl.is_host_dead(&dead_base), "the refused host must be remembered as dead");
    assert!(!dl.is_host_dead(&official.uri()));
    assert_eq!(official.received_requests().await.unwrap().len(), 5);
}

#[tokio::test]
async fn cancellation_stops_downloads() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(payload()).set_delay(Duration::from_secs(5)))
        .mount(&server)
        .await;
    let token = CancellationToken::new();
    let dl = downloader().with_cancel(token.clone());
    let dir = tempfile::tempdir().unwrap();
    let task = DownloadTask::new(format!("{}/slow", server.uri()), dir.path().join("slow.bin"));

    let canceller = {
        let t = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            t.cancel();
        })
    };
    let started = std::time::Instant::now();
    let err = dl.download_one(task).await.unwrap_err();
    canceller.await.unwrap();
    assert!(matches!(err, Error::Cancelled), "{err:?}");
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(!dir.path().join("slow.bin").exists());
}

#[tokio::test]
async fn same_destination_is_downloaded_once_even_when_requested_concurrently() {
    let server = MockServer::start().await;
    let data = payload();
    Mock::given(method("GET"))
        .and(path("/shared"))
        .respond_with(
            ResponseTemplate::new(200).set_body_bytes(data.clone()).set_delay(Duration::from_millis(150)),
        )
        .mount(&server)
        .await;

    let dl = Arc::new(downloader());
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("shared.bin");
    let mk = || {
        DownloadTask::new(format!("{}/shared", server.uri()), dest.clone())
            .sha1(Some(sha1_bytes(&data)))
            .size(Some(data.len() as u64))
    };
    let (a, b) = tokio::join!(dl.download_one(mk()), dl.download_one(mk()));
    a.unwrap();
    b.unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), data);
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn existing_valid_file_is_not_downloaded_again() {
    let server = MockServer::start().await;
    let data = payload();
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("have.bin");
    std::fs::write(&dest, &data).unwrap();
    let task = DownloadTask::new(format!("{}/have", server.uri()), dest)
        .size(Some(data.len() as u64));
    downloader().download_one(task).await.unwrap();
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn maven_sidecar_sha1_is_used_when_no_checksum_is_known() {
    let server = MockServer::start().await;
    let data = payload();
    Mock::given(method("GET"))
        .and(path("/lib.jar"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(data.clone()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/lib.jar.sha1"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!("{}  lib.jar\n", sha1_bytes(&data))))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("lib.jar");
    let task = DownloadTask::new(format!("{}/lib.jar", server.uri()), dest.clone()).sidecar(true);
    downloader().download_one(task).await.unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), data);
}

#[tokio::test]
async fn wrong_checksum_never_leaves_a_file_behind() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(payload()))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("bad.bin");
    let task = DownloadTask::new(format!("{}/bad", server.uri()), dest.clone())
        .sha1(Some("0".repeat(40)));
    let err = downloader().download_one(task).await.unwrap_err();
    assert!(matches!(err, Error::Download { .. }), "{err:?}");
    assert!(!dest.exists());
    assert!(!dir.path().join("bad.bin.part").exists());
}

#[tokio::test]
async fn untrusted_relative_paths_are_rejected_before_any_request() {
    let base = tempfile::tempdir().unwrap();
    for bad in ["../x", "a/../../x", "C:/x", "/abs", "con.txt", "a:b"] {
        assert!(
            DownloadTask::rel(base.path(), bad, "http://localhost/x").is_err(),
            "{bad} should be rejected"
        );
    }
    assert!(DownloadTask::rel(base.path(), "ok/file.jar", "http://localhost/x").is_ok());
}

#[tokio::test]
async fn download_all_reports_progress_and_completes_every_file() {
    let server = MockServer::start().await;
    let data = payload();
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(data.clone()))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let tasks: Vec<_> = (0..30)
        .map(|i| {
            DownloadTask::new(format!("{}/f{i}", server.uri()), dir.path().join(format!("f{i}.bin")))
                .size(Some(data.len() as u64))
        })
        .collect();
    let last = Arc::new(std::sync::Mutex::new(None));
    let sink = last.clone();
    let cb: launcher_core::download::ProgressFn = Arc::new(move |p| {
        *sink.lock().unwrap() = Some(p.clone());
    });
    downloader().with_limits(8, 4).download_all(tasks, "test", Some(cb)).await.unwrap();
    let p = last.lock().unwrap().clone().unwrap();
    assert_eq!((p.files_done, p.files_total), (30, 30));
    assert_eq!(p.bytes_done, 30 * data.len() as u64);
}

#[tokio::test]
async fn settled_download_finishes_the_other_files_when_one_fails() {
    let server = MockServer::start().await;
    let data = payload();
    Mock::given(method("GET"))
        .and(path("/missing"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(data.clone()))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let mk = |name: &str, url_path: &str| {
        DownloadTask::new(format!("{}/{url_path}", server.uri()), dir.path().join(name))
            .sha1(Some(sha1_bytes(&data)))
            .size(Some(data.len() as u64))
    };
    let tasks = vec![mk("a.bin", "a"), mk("bad.bin", "missing"), mk("c.bin", "c")];
    let results = downloader().with_limits(8, 8).download_all_settled(tasks, "test", None).await;
    assert_eq!(results.len(), 3);
    assert!(results[0].is_ok() && results[1].is_err() && results[2].is_ok());
    assert_eq!(std::fs::read(dir.path().join("a.bin")).unwrap(), data);
    assert_eq!(std::fs::read(dir.path().join("c.bin")).unwrap(), data);
    assert!(!dir.path().join("bad.bin").exists());
    assert!(!dir.path().join("bad.bin.part").exists());
}

/// Tauri commands must be `Send`; a borrowing closure inside the stream once
/// made `download_all_settled`'s future fail that bound.
#[tokio::test]
async fn settled_download_future_is_send() {
    fn assert_send<T: Send>(_: &T) {}
    let d = downloader();
    let fut = d.download_all_settled(vec![], "t", None);
    assert_send(&fut);
    assert!(fut.await.is_empty());
}
