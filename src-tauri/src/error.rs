use serde::Serialize;

#[derive(Debug, thiserror::Error, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "snake_case")]
pub enum LmtError {
    #[error("io: {0}")]
    Io(String),
    #[error("yaml: {0}")]
    Yaml(String),
    #[error("core: {0}")]
    Core(String),
    #[error("db: {0}")]
    Db(String),
    #[error("not_found: {0}")]
    NotFound(String),
    #[error("invalid_input: {0}")]
    InvalidInput(String),
    #[error("{0}")]
    Other(String),
}

pub type LmtResult<T> = Result<T, LmtError>;

impl From<std::io::Error> for LmtError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

impl From<serde_yaml::Error> for LmtError {
    fn from(e: serde_yaml::Error) -> Self {
        Self::Yaml(e.to_string())
    }
}

impl From<serde_json::Error> for LmtError {
    fn from(e: serde_json::Error) -> Self {
        Self::Yaml(format!("json: {e}"))
    }
}

impl From<rusqlite::Error> for LmtError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Db(e.to_string())
    }
}

impl From<lmt_core::CoreError> for LmtError {
    fn from(e: lmt_core::CoreError) -> Self {
        Self::Core(e.to_string())
    }
}

impl From<lmt_adapter_total_station::AdapterError> for LmtError {
    fn from(e: lmt_adapter_total_station::AdapterError) -> Self {
        Self::Other(format!("{e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_with_kind_and_message() {
        let err = LmtError::NotFound("foo".into());
        let s = serde_json::to_string(&err).unwrap();
        assert_eq!(s, r#"{"kind":"not_found","message":"foo"}"#);
    }

    #[test]
    fn io_error_converts() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "x");
        let lmt: LmtError = io.into();
        assert!(matches!(lmt, LmtError::Io(_)));
    }

    #[test]
    fn adapter_error_converts_to_lmt_error() {
        use lmt_adapter_total_station::AdapterError;
        let adapter_err = AdapterError::InvalidInput("bad csv row".into());
        let lmt_err: LmtError = adapter_err.into();
        let s = format!("{lmt_err}");
        assert!(s.contains("bad csv row"), "got: {s}");
        assert!(matches!(lmt_err, LmtError::Other(_)));
    }
}
