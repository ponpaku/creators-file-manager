use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("リクエストエラー: {0}")]
    InvalidRequest(String),
    #[error("IOエラー: {0}")]
    Io(String),
    #[error("設定エラー: {0}")]
    Settings(String),
}

impl From<std::io::Error> for AppError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value.to_string())
    }
}
