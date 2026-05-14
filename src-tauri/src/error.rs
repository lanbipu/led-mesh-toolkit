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
        use lmt_adapter_total_station::AdapterError as A;
        match e {
            A::InvalidInput(s) => Self::InvalidInput(s),
            A::Csv(err) => Self::InvalidInput(format!("csv: {err}")),
            A::Yaml(err) => Self::Yaml(err.to_string()),
            A::Json(err) => Self::Yaml(format!("json: {err}")),
            A::Io(err) => Self::Io(err.to_string()),
            A::Core(err) => Self::Core(err.to_string()),
            A::Pdf(s) => Self::Other(format!("pdf: {s}")),
            other => Self::Other(other.to_string()),
        }
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
    fn adapter_invalid_input_maps_to_invalid_input_with_kind() {
        use lmt_adapter_total_station::AdapterError;
        let lmt: LmtError = AdapterError::InvalidInput("bad row".into()).into();
        assert!(matches!(lmt, LmtError::InvalidInput(_)));
        let json = serde_json::to_string(&lmt).unwrap();
        assert!(json.contains(r#""kind":"invalid_input""#), "got: {json}");
        assert!(json.contains("bad row"), "got: {json}");
    }

    #[test]
    fn adapter_io_maps_to_io_with_kind() {
        use lmt_adapter_total_station::AdapterError;
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "missing.csv");
        let lmt: LmtError = AdapterError::Io(io).into();
        assert!(matches!(lmt, LmtError::Io(_)));
        let json = serde_json::to_string(&lmt).unwrap();
        assert!(json.contains(r#""kind":"io""#), "got: {json}");
    }

    #[test]
    fn adapter_pdf_maps_to_other_with_pdf_prefix() {
        use lmt_adapter_total_station::AdapterError;
        let lmt: LmtError = AdapterError::Pdf("layout failure".into()).into();
        assert!(matches!(lmt, LmtError::Other(_)));
        let json = serde_json::to_string(&lmt).unwrap();
        assert!(json.contains(r#""kind":"other""#));
        assert!(json.contains("pdf: layout failure"), "got: {json}");
    }
}
