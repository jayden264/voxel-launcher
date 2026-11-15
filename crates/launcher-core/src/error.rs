use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("网络错误: {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON 解析错误: {0}")]
    Json(#[from] serde_json::Error),
    #[error("压缩包错误: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("不安全的路径 `{0}`")]
    UnsafePath(String),
    #[error("校验失败: {0}")]
    Checksum(String),
    #[error("下载失败 {url}: {reason}")]
    Download { url: String, reason: String },
    #[error("已取消")]
    Cancelled,
    #[error("无法连接服务器: {0}")]
    Unreachable(String),
    #[error("服务器拒绝请求: {0}")]
    Permanent(String),
    #[error("找不到版本 `{0}`")]
    VersionNotFound(String),
    #[error("玩家名无效: {0}")]
    InvalidPlayerName(String),
    #[error("Java 不可用: {0}")]
    Java(String),
    #[error("启动失败: {0}")]
    Launch(String),
    #[error("文件不存在: {0}")]
    Missing(PathBuf),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
