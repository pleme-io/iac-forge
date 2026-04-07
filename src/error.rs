use thiserror::Error;

#[derive(Debug, Error)]
pub enum IacForgeError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("OpenAPI error: {0}")]
    OpenApi(#[from] openapi_forge::ForgeError),

    #[error("missing CRUD endpoint: {resource} needs {endpoint}")]
    MissingEndpoint { resource: String, endpoint: String },

    #[error("schema not found in spec: {0}")]
    SchemaNotFound(String),

    #[error("validation error: {0}")]
    ValidationError(String),

    #[error("backend error: {0}")]
    BackendError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let err = IacForgeError::Io(io_err);
        let msg = err.to_string();
        assert!(msg.contains("IO error"), "got: {msg}");
        assert!(msg.contains("file missing"), "got: {msg}");
    }

    #[test]
    fn from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let err: IacForgeError = io_err.into();
        assert!(matches!(err, IacForgeError::Io(_)));
    }

    #[test]
    fn display_toml_error() {
        let toml_err = toml::from_str::<toml::Value>("{{invalid").unwrap_err();
        let err = IacForgeError::Toml(toml_err);
        let msg = err.to_string();
        assert!(msg.contains("TOML parse error"), "got: {msg}");
    }

    #[test]
    fn from_toml_error() {
        let toml_err = toml::from_str::<toml::Value>("{{").unwrap_err();
        let err: IacForgeError = toml_err.into();
        assert!(matches!(err, IacForgeError::Toml(_)));
    }

    #[test]
    fn display_json_error() {
        let json_err = serde_json::from_str::<serde_json::Value>("{bad}").unwrap_err();
        let err = IacForgeError::Json(json_err);
        let msg = err.to_string();
        assert!(msg.contains("JSON error"), "got: {msg}");
    }

    #[test]
    fn from_json_error() {
        let json_err = serde_json::from_str::<serde_json::Value>("not json").unwrap_err();
        let err: IacForgeError = json_err.into();
        assert!(matches!(err, IacForgeError::Json(_)));
    }

    #[test]
    fn display_missing_endpoint() {
        let err = IacForgeError::MissingEndpoint {
            resource: "my_secret".to_string(),
            endpoint: "/create-secret".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("my_secret"), "got: {msg}");
        assert!(msg.contains("/create-secret"), "got: {msg}");
        assert!(msg.contains("missing CRUD endpoint"), "got: {msg}");
    }

    #[test]
    fn display_schema_not_found() {
        let err = IacForgeError::SchemaNotFound("CreateFoo".to_string());
        let msg = err.to_string();
        assert!(msg.contains("CreateFoo"), "got: {msg}");
        assert!(msg.contains("schema not found"), "got: {msg}");
    }

    #[test]
    fn display_validation_error() {
        let err = IacForgeError::ValidationError("field X is invalid".to_string());
        let msg = err.to_string();
        assert!(msg.contains("validation error"), "got: {msg}");
        assert!(msg.contains("field X is invalid"), "got: {msg}");
    }

    #[test]
    fn display_backend_error() {
        let err = IacForgeError::BackendError("template rendering failed".to_string());
        let msg = err.to_string();
        assert!(msg.contains("backend error"), "got: {msg}");
        assert!(msg.contains("template rendering failed"), "got: {msg}");
    }

    #[test]
    fn error_is_send_and_sync() {
        fn assert_send_sync<T: Send>() {}
        assert_send_sync::<IacForgeError>();
    }

    #[test]
    fn debug_format_includes_variant() {
        let err = IacForgeError::SchemaNotFound("TestSchema".to_string());
        let debug = format!("{err:?}");
        assert!(debug.contains("SchemaNotFound"), "got: {debug}");
    }

    #[test]
    fn display_missing_endpoint_with_empty_strings() {
        let err = IacForgeError::MissingEndpoint {
            resource: String::new(),
            endpoint: String::new(),
        };
        let msg = err.to_string();
        assert!(msg.contains("missing CRUD endpoint"), "got: {msg}");
    }
}
