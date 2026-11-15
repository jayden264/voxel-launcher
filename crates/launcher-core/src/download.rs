//! Concurrent, resumable, verified downloader.
//!
//! * `.part` temp files + atomic rename, Range resume
//! * per-attempt retry with exponential backoff
//! * mirror -> official fallback (via the configurable `resolver`)
//! * sha1 verification; a failed *resumed* download restarts from scratch
//! * global and per-host concurrency limits
//! * per-destination in-flight locks (the same path is never downloaded twice)
//! * throttled (~100ms) progress callbacks

use crate::{mirror, paths, Error, Result};
use futures::{StreamExt, TryStreamExt};
use reqwest::{header, Client, StatusCode};
use serde::{de::DeserializeOwned, Serialize};
use sha1::{Digest, Sha1};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    sync::Semaphore,
};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub struct DownloadTask {
    pub url: String,
    pub dest: PathBuf,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    /// If no sha1 is known, try fetching `<url>.sha1` (maven repositories).
    pub sidecar_sha1: bool,
}

impl DownloadTask {
    pub fn new(url: impl Into<String>, dest: PathBuf) -> Self {
        Self { url: url.into(), dest, sha1: None, size: None, sidecar_sha1: false }
    }

    /// Build a task whose destination comes from untrusted data: the path is
    /// validated with [`paths::safe_join`].
    pub fn rel(base: &Path, rel: &str, url: impl Into<String>) -> Result<Self> {
        Ok(Self::new(url, paths::safe_join(base, rel)?))
    }

    pub fn sha1(mut self, sha1: Option<String>) -> Self {
        self.sha1 = sha1.map(|s| s.to_ascii_lowercase());
        self
    }
    pub fn size(mut self, size: Option<u64>) -> Self {
        self.size = size;
        self
    }
    pub fn sidecar(mut self, yes: bool) -> Self {
        self.sidecar_sha1 = yes;
        self
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Progress {
    pub stage: String,
    pub files_done: u64,
    pub files_total: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

pub type ProgressFn = Arc<dyn Fn(&Progress) + Send + Sync>;
pub type Resolver = Arc<dyn Fn(&str) -> Vec<String> + Send + Sync>;

struct Counters {
    files_done: AtomicU64,
    bytes_done: AtomicU64,
    files_total: u64,
    bytes_total: u64,
}

impl Counters {
    fn snapshot(&self, stage: &str) -> Progress {
        Progress {
            stage: stage.to_string(),
            files_done: self.files_done.load(Ordering::Relaxed),
            files_total: self.files_total,
            bytes_done: self.bytes_done.load(Ordering::Relaxed),
            bytes_total: self.bytes_total,
        }
    }
}

pub struct Downloader {
    client: Client,
    resolver: Resolver,
    global_limit: usize,
    global: Arc<Semaphore>,
    per_host_limit: usize,
    hosts: Mutex<HashMap<String, Arc<Semaphore>>>,
    /// Hosts that recently failed to connect; skipped while another candidate exists.
    dead: Mutex<HashMap<String, std::time::Instant>>,
    inflight: Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>,
    cancel: CancellationToken,
    retries: u32,
    backoff: Duration,
}

/// How long a host that refused/timed out a connection is avoided.
const DEAD_HOST_TTL: Duration = Duration::from_secs(120);

/// `host:port` identity used for per-host limits and the dead-host list.
fn host_of(url: &str) -> String {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|u| {
            let host = u.host_str()?.to_string();
            Some(match u.port_or_known_default() {
                Some(p) => format!("{host}:{p}"),
                None => host,
            })
        })
        .unwrap_or_default()
}

/// A failure to even reach the server (as opposed to an HTTP error status).
fn is_unreachable(e: &reqwest::Error) -> bool {
    e.is_connect() || e.is_timeout()
}

impl Downloader {
    pub fn new(source: mirror::Source) -> Self {
        let client = Client::builder()
            .user_agent(concat!("MinecraftLauncher/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(6))
            .read_timeout(Duration::from_secs(30))
            .build()
            .expect("failed to build HTTP client");
        Self {
            client,
            resolver: Arc::new(move |u| mirror::candidates(u, source)),
            global_limit: 24,
            global: Arc::new(Semaphore::new(24)),
            per_host_limit: 12,
            hosts: Mutex::new(HashMap::new()),
            dead: Mutex::new(HashMap::new()),
            inflight: Mutex::new(HashMap::new()),
            cancel: CancellationToken::new(),
            retries: 3,
            backoff: Duration::from_millis(400),
        }
    }

    pub fn with_resolver(mut self, r: Resolver) -> Self {
        self.resolver = r;
        self
    }
    pub fn with_cancel(mut self, c: CancellationToken) -> Self {
        self.cancel = c;
        self
    }
    pub fn with_limits(mut self, global: usize, per_host: usize) -> Self {
        self.global_limit = global.max(1);
        self.global = Arc::new(Semaphore::new(self.global_limit));
        self.per_host_limit = per_host.max(1);
        self
    }
    pub fn with_retry(mut self, retries: u32, backoff: Duration) -> Self {
        self.retries = retries;
        self.backoff = backoff;
        self
    }
    pub fn cancel_token(&self) -> CancellationToken {
        self.cancel.clone()
    }

    fn mark_dead(&self, url: &str) {
        self.dead.lock().unwrap().insert(host_of(url), std::time::Instant::now());
    }

    /// True if `url`'s host recently failed to connect.
    pub fn is_host_dead(&self, url: &str) -> bool {
        let mut map = self.dead.lock().unwrap();
        let host = host_of(url);
        match map.get(&host) {
            Some(t) if t.elapsed() < DEAD_HOST_TTL => true,
            Some(_) => {
                map.remove(&host);
                false
            }
            None => false,
        }
    }

    /// Candidate URLs for `url`, leaving out hosts known to be unreachable
    /// (unless that would leave nothing to try).
    fn candidates(&self, url: &str) -> Vec<String> {
        let all = (self.resolver)(url);
        let live: Vec<String> = all.iter().filter(|u| !self.is_host_dead(u)).cloned().collect();
        if live.is_empty() {
            all
        } else {
            live
        }
    }

    fn host_semaphore(&self, url: &str) -> Arc<Semaphore> {
        let host = host_of(url);
        let mut map = self.hosts.lock().unwrap();
        map.entry(host)
            .or_insert_with(|| Arc::new(Semaphore::new(self.per_host_limit)))
            .clone()
    }

    fn path_lock(&self, dest: &Path) -> Arc<tokio::sync::Mutex<()>> {
        let mut map = self.inflight.lock().unwrap();
        map.entry(dest.to_path_buf()).or_default().clone()
    }

    async fn backoff_sleep(&self, attempt: u32) -> Result<()> {
        let d = self.backoff * 2u32.saturating_pow(attempt);
        tokio::select! {
            _ = tokio::time::sleep(d) => Ok(()),
            _ = self.cancel.cancelled() => Err(Error::Cancelled),
        }
    }

    // ---------------------------------------------------------------- fetch

    /// GET a small resource (JSON, manifests) with mirror fallback + retry.
    pub async fn fetch(&self, url: &str) -> Result<Vec<u8>> {
        self.fetch_checked(url, None).await
    }

    /// Like [`fetch`], but when `sha1` is given a source whose bytes do not
    /// match is skipped in favour of the next one. (BMCLAPI re-serialises some
    /// JSON documents, e.g. Java runtime manifests, so the official copy is
    /// the one that matches Mojang's published hash.)
    pub async fn fetch_checked(&self, url: &str, sha1: Option<&str>) -> Result<Vec<u8>> {
        let mut last = String::from("no candidates");
        for candidate in self.candidates(url) {
            for attempt in 0..=self.retries {
                if self.cancel.is_cancelled() {
                    return Err(Error::Cancelled);
                }
                let _g = self.global.acquire().await.map_err(|e| Error::Other(e.to_string()))?;
                match self.client.get(&candidate).send().await {
                    Ok(r) if r.status().is_success() => match r.bytes().await {
                        Ok(b) => {
                            let ok = sha1.map_or(true, |want| sha1_bytes(&b).eq_ignore_ascii_case(want));
                            if ok {
                                return Ok(b.to_vec());
                            }
                            last = format!("{candidate}: sha1 与预期不符");
                            break; // this source serves different bytes; try the next one
                        }
                        Err(e) => last = format!("{candidate}: {e}"),
                    },
                    Ok(r) if r.status().is_client_error() => {
                        last = format!("{candidate}: HTTP {}", r.status());
                        break; // permanent for this candidate
                    }
                    Ok(r) => last = format!("{candidate}: HTTP {}", r.status()),
                    Err(e) => {
                        last = format!("{candidate}: {e}");
                        if is_unreachable(&e) {
                            // Retrying an unreachable host only burns time: switch source now.
                            self.mark_dead(&candidate);
                            break;
                        }
                    }
                }
                drop(_g);
                if attempt < self.retries {
                    self.backoff_sleep(attempt).await?;
                }
            }
        }
        Err(Error::Download { url: url.to_string(), reason: last })
    }

    pub async fn fetch_json<T: DeserializeOwned>(&self, url: &str) -> Result<T> {
        let bytes = self.fetch(url).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    // ------------------------------------------------------------- download

    fn counters_for(tasks: &[DownloadTask]) -> Arc<Counters> {
        Arc::new(Counters {
            files_done: AtomicU64::new(0),
            bytes_done: AtomicU64::new(0),
            files_total: tasks.len() as u64,
            bytes_total: tasks.iter().filter_map(|t| t.size).sum(),
        })
    }

    /// Report `counters` every 100 ms until the returned token is cancelled.
    fn start_reporter(
        progress: &Option<ProgressFn>,
        counters: &Arc<Counters>,
        stage: &str,
    ) -> (CancellationToken, Option<tokio::task::JoinHandle<()>>) {
        let stop = CancellationToken::new();
        let handle = progress.clone().map(|cb| {
            let counters = counters.clone();
            let stop = stop.clone();
            let stage = stage.to_string();
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(Duration::from_millis(100));
                loop {
                    tokio::select! {
                        _ = tick.tick() => cb(&counters.snapshot(&stage)),
                        _ = stop.cancelled() => break,
                    }
                }
            })
        });
        (stop, handle)
    }

    async fn finish_reporter(
        (stop, handle): (CancellationToken, Option<tokio::task::JoinHandle<()>>),
        progress: &Option<ProgressFn>,
        counters: &Counters,
        stage: &str,
    ) {
        stop.cancel();
        if let Some(h) = handle {
            let _ = h.await;
        }
        if let Some(cb) = progress {
            cb(&counters.snapshot(stage));
        }
    }

    /// Download every task, skipping files that are already valid. The first
    /// failure stops the rest.
    pub async fn download_all(
        &self,
        tasks: Vec<DownloadTask>,
        stage: &str,
        progress: Option<ProgressFn>,
    ) -> Result<()> {
        let mut seen = HashSet::new();
        let tasks: Vec<DownloadTask> =
            tasks.into_iter().filter(|t| seen.insert(t.dest.clone())).collect();
        let counters = Self::counters_for(&tasks);
        let reporter = Self::start_reporter(&progress, &counters, stage);

        let c = &counters;
        let result = futures::stream::iter(tasks.into_iter().map(Ok::<_, Error>))
            .try_for_each_concurrent(Some(self.global_limit), |task| async move {
                self.ensure_file(&task, c).await
            })
            .await;

        Self::finish_reporter(reporter, &progress, &counters, stage).await;
        result
    }

    /// Like [`download_all`](Self::download_all) but a failure does not stop the
    /// other files: every task runs to its own end and the results come back in
    /// input order (duplicate destinations share one result).
    pub async fn download_all_settled(
        &self,
        tasks: Vec<DownloadTask>,
        stage: &str,
        progress: Option<ProgressFn>,
    ) -> Vec<Result<()>> {
        let unique: Vec<DownloadTask> = {
            let mut seen = HashSet::new();
            tasks.iter().filter(|t| seen.insert(t.dest.clone())).cloned().collect()
        };
        let counters = Self::counters_for(&unique);
        let reporter = Self::start_reporter(&progress, &counters, stage);

        let c = &counters;
        // Owned items: a closure taking `&DownloadTask` is not `Send`-general.
        let done: Vec<(PathBuf, Result<()>)> = futures::stream::iter(unique)
            .map(|task| async move {
                let result = self.ensure_file(&task, c).await;
                (task.dest, result)
            })
            .buffer_unordered(self.global_limit)
            .collect()
            .await;

        Self::finish_reporter(reporter, &progress, &counters, stage).await;
        tasks
            .iter()
            .map(|t| match done.iter().find(|(d, _)| d == &t.dest) {
                Some((_, Ok(()))) => Ok(()),
                Some((_, Err(Error::Cancelled))) => Err(Error::Cancelled),
                Some((_, Err(e))) => Err(Error::Other(e.to_string())),
                None => Err(Error::Other("内部错误: 任务结果丢失".into())),
            })
            .collect()
    }

    /// Download a single file (convenience wrapper).
    pub async fn download_one(&self, task: DownloadTask) -> Result<()> {
        self.download_all(vec![task], "download", None).await
    }

    async fn ensure_file(&self, task: &DownloadTask, counters: &Counters) -> Result<()> {
        if self.cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        // Same destination => serialise; the second caller finds the file valid.
        let lock = self.path_lock(&task.dest);
        let _held = lock.lock().await;

        if is_valid_existing(task).await {
            counters.files_done.fetch_add(1, Ordering::Relaxed);
            counters.bytes_done.fetch_add(task.size.unwrap_or(0), Ordering::Relaxed);
            return Ok(());
        }

        let mut last = String::from("no candidates");
        for url in self.candidates(&task.url) {
            match self.try_url(&url, task, counters).await {
                Ok(()) => {
                    counters.files_done.fetch_add(1, Ordering::Relaxed);
                    return Ok(());
                }
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                Err(e) => last = format!("{url}: {e}"),
            }
        }
        Err(Error::Download { url: task.url.clone(), reason: last })
    }

    async fn try_url(&self, url: &str, task: &DownloadTask, counters: &Counters) -> Result<()> {
        let part = part_path(&task.dest);
        if let Some(dir) = task.dest.parent() {
            tokio::fs::create_dir_all(dir).await?;
        }
        let host_sem = self.host_semaphore(url);
        let mut last = String::from("unknown");

        for attempt in 0..=self.retries {
            if self.cancel.is_cancelled() {
                return Err(Error::Cancelled);
            }
            let _g = self.global.acquire().await.map_err(|e| Error::Other(e.to_string()))?;
            let _h = host_sem.acquire().await.map_err(|e| Error::Other(e.to_string()))?;

            let mut existing = tokio::fs::metadata(&part).await.map(|m| m.len()).unwrap_or(0);
            if let Some(size) = task.size {
                if existing > size {
                    let _ = tokio::fs::remove_file(&part).await;
                    existing = 0;
                }
            }
            let resumed = existing > 0;
            let mut attempt_bytes = 0u64;

            let outcome: Result<()> = async {
                // Fully present .part (e.g. crash right before rename) needs no request.
                let complete = task.size == Some(existing) && existing > 0;
                if !complete {
                    let mut req = self.client.get(url);
                    if resumed {
                        req = req.header(header::RANGE, format!("bytes={existing}-"));
                    }
                    let resp = tokio::select! {
                        r = req.send() => r,
                        _ = self.cancel.cancelled() => return Err(Error::Cancelled),
                    };
                    let resp = match resp {
                        Ok(r) => r,
                        Err(e) => {
                            if is_unreachable(&e) {
                                self.mark_dead(url);
                                return Err(Error::Unreachable(e.to_string()));
                            }
                            return Err(e.into());
                        }
                    };
                    let status = resp.status();
                    let append = match status {
                        StatusCode::PARTIAL_CONTENT => true,
                        s if s.is_success() => false,
                        StatusCode::RANGE_NOT_SATISFIABLE => {
                            let _ = tokio::fs::remove_file(&part).await;
                            return Err(Error::Other("range not satisfiable".into()));
                        }
                        // 4xx (other than timeout / rate limit) will not change on retry:
                        // e.g. the mirror simply does not have this file. Switch source.
                        s if s.is_client_error()
                            && s != StatusCode::REQUEST_TIMEOUT
                            && s != StatusCode::TOO_MANY_REQUESTS =>
                        {
                            return Err(Error::Permanent(format!("HTTP {s}")));
                        }
                        s => return Err(Error::Other(format!("HTTP {s}"))),
                    };
                    let mut file = if append {
                        let mut f = tokio::fs::OpenOptions::new().write(true).open(&part).await?;
                        f.seek(std::io::SeekFrom::End(0)).await?;
                        f
                    } else {
                        tokio::fs::File::create(&part).await?
                    };
                    let mut stream = resp.bytes_stream();
                    loop {
                        let chunk = tokio::select! {
                            c = stream.next() => c,
                            _ = self.cancel.cancelled() => return Err(Error::Cancelled),
                        };
                        match chunk {
                            Some(Ok(bytes)) => {
                                file.write_all(&bytes).await?;
                                attempt_bytes += bytes.len() as u64;
                                counters.bytes_done.fetch_add(bytes.len() as u64, Ordering::Relaxed);
                            }
                            Some(Err(e)) => return Err(e.into()),
                            None => break,
                        }
                    }
                    file.flush().await?;
                }
                self.verify_and_commit(task, &part).await
            }
            .await;

            match outcome {
                Ok(()) => return Ok(()),
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                // Host unreachable: no point retrying it, let the caller try the next source.
                Err(e @ (Error::Unreachable(_) | Error::Permanent(_))) => return Err(e),
                Err(e) => {
                    counters.bytes_done.fetch_sub(
                        attempt_bytes.min(counters.bytes_done.load(Ordering::Relaxed)),
                        Ordering::Relaxed,
                    );
                    last = e.to_string();
                    let corrupt = matches!(e, Error::Checksum(_));
                    if corrupt {
                        // Never retry the same corrupt bytes.
                        let _ = tokio::fs::remove_file(&part).await;
                        if !resumed {
                            // A clean full download was corrupt: this source is bad.
                            return Err(e);
                        }
                        // A resumed download mismatched: restart from scratch at once.
                        continue;
                    }
                }
            }
            drop(_h);
            drop(_g);
            if attempt < self.retries {
                self.backoff_sleep(attempt).await?;
            }
        }
        let _ = tokio::fs::remove_file(&part).await;
        Err(Error::Other(last))
    }

    async fn verify_and_commit(&self, task: &DownloadTask, part: &Path) -> Result<()> {
        let len = tokio::fs::metadata(part).await?.len();
        if let Some(size) = task.size {
            if len != size {
                return Err(Error::Checksum(format!("size {len} != expected {size}")));
            }
        } else if len == 0 {
            return Err(Error::Checksum("empty file".into()));
        }
        let mut expected = task.sha1.clone();
        if expected.is_none() && task.sidecar_sha1 {
            expected = self.fetch_sidecar(&task.url).await;
        }
        if let Some(want) = expected {
            let got = sha1_file(part).await?;
            if !got.eq_ignore_ascii_case(&want) {
                return Err(Error::Checksum(format!("sha1 {got} != expected {want}")));
            }
        }
        tokio::fs::rename(part, &task.dest).await?;
        Ok(())
    }

    async fn fetch_sidecar(&self, url: &str) -> Option<String> {
        let bytes = self.fetch(&format!("{url}.sha1")).await.ok()?;
        let text = String::from_utf8(bytes).ok()?;
        let hex: String = text.trim().chars().take(40).collect();
        (hex.len() == 40 && hex.chars().all(|c| c.is_ascii_hexdigit())).then_some(hex)
    }
}

/// An existing destination is trusted when its size matches (or, with no
/// known size, it is non-empty). Full hashing is reserved for explicit
/// integrity checks to keep startup fast with thousands of assets.
async fn is_valid_existing(task: &DownloadTask) -> bool {
    match tokio::fs::metadata(&task.dest).await {
        Ok(m) if m.is_file() => match task.size {
            Some(s) => m.len() == s,
            None => m.len() > 0,
        },
        _ => false,
    }
}

fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".part");
    dest.with_file_name(name)
}

pub async fn sha1_file(path: &Path) -> Result<String> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha1::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub fn sha1_bytes(data: &[u8]) -> String {
    let mut h = Sha1::new();
    h.update(data);
    hex::encode(h.finalize())
}
