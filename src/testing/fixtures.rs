//! Sexpr-based test fixtures for backends and cross-repo tests.
//!
//! The backend crates historically hand-constructed `IacResource` and
//! `IacAttribute` test fixtures as Rust literals. When the IR grew new
//! fields (`json_encoded`, `optional`, `update_only`) those literals
//! went stale, silently in some repos and loudly in others. Fixtures
//! stored as canonical sexpr text avoid that drift: the IR owns the
//! sexpr shape, every consumer loads through the same canonical path,
//! and adding a field only requires editing the fixtures once.
//!
//! # Pattern
//!
//! Write a fixture:
//!
//! ```no_run
//! use iac_forge::testing::fixtures;
//! use iac_forge::testing::test_resource;
//!
//! let resource = test_resource("widget");
//! fixtures::save_resource(&resource, "tests/fixtures/widget.sexpr").unwrap();
//! ```
//!
//! Load it back in tests:
//!
//! ```no_run
//! use iac_forge::testing::fixtures;
//!
//! let resource = fixtures::load_resource("tests/fixtures/widget.sexpr").unwrap();
//! assert_eq!(resource.name, "widget");
//! ```
//!
//! String-based loaders (`_str` variants) are equivalent but skip disk
//! I/O — useful for tests that embed the fixture via `include_str!`.

use std::path::Path;

use crate::ir::{IacDataSource, IacProvider, IacResource};
use crate::sexpr::{FromSExpr, SExpr, SExprError, ToSExpr};

/// Errors from fixture I/O.
#[derive(Debug)]
pub enum FixtureError {
    /// `std::io::Error` from file read/write.
    Io(std::io::Error),
    /// Parse or shape error during fixture decode.
    Sexpr(SExprError),
}

impl std::fmt::Display for FixtureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "fixture io: {e}"),
            Self::Sexpr(e) => write!(f, "fixture sexpr: {e}"),
        }
    }
}

impl std::error::Error for FixtureError {}

impl From<std::io::Error> for FixtureError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<SExprError> for FixtureError {
    fn from(e: SExprError) -> Self {
        Self::Sexpr(e)
    }
}

// ── Generic helpers ────────────────────────────────────────────────

/// Load any `FromSExpr` type from a file path.
///
/// # Errors
/// Returns `FixtureError::Io` on read failure, `FixtureError::Sexpr` on
/// parse or shape mismatch.
pub fn load<T: FromSExpr>(path: impl AsRef<Path>) -> Result<T, FixtureError> {
    let text = std::fs::read_to_string(path)?;
    load_str(&text)
}

/// Load any `FromSExpr` type from a string (for `include_str!` or tests).
///
/// # Errors
/// Returns `FixtureError::Sexpr` on parse or shape mismatch.
pub fn load_str<T: FromSExpr>(text: &str) -> Result<T, FixtureError> {
    let sexpr = SExpr::parse(text)?;
    Ok(T::from_sexpr(&sexpr)?)
}

/// Save any `ToSExpr` type to a file path in canonical emission.
///
/// # Errors
/// Returns `FixtureError::Io` on write failure. Parent directories are
/// created automatically.
pub fn save<T: ToSExpr>(value: &T, path: impl AsRef<Path>) -> Result<(), FixtureError> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut text = value.to_sexpr().emit();
    text.push('\n');
    std::fs::write(path, text)?;
    Ok(())
}

// ── Type-specialised aliases (ergonomic call sites) ────────────────

/// Load an `IacResource` from a fixture file.
///
/// # Errors
/// See [`load`].
pub fn load_resource(path: impl AsRef<Path>) -> Result<IacResource, FixtureError> {
    load::<IacResource>(path)
}

/// Parse an `IacResource` from a fixture string.
///
/// # Errors
/// See [`load_str`].
pub fn load_resource_str(text: &str) -> Result<IacResource, FixtureError> {
    load_str::<IacResource>(text)
}

/// Save an `IacResource` to a fixture file.
///
/// # Errors
/// See [`save`].
pub fn save_resource(
    resource: &IacResource,
    path: impl AsRef<Path>,
) -> Result<(), FixtureError> {
    save(resource, path)
}

/// Load an `IacProvider` from a fixture file.
///
/// # Errors
/// See [`load`].
pub fn load_provider(path: impl AsRef<Path>) -> Result<IacProvider, FixtureError> {
    load::<IacProvider>(path)
}

/// Parse an `IacProvider` from a fixture string.
///
/// # Errors
/// See [`load_str`].
pub fn load_provider_str(text: &str) -> Result<IacProvider, FixtureError> {
    load_str::<IacProvider>(text)
}

/// Save an `IacProvider` to a fixture file.
///
/// # Errors
/// See [`save`].
pub fn save_provider(
    provider: &IacProvider,
    path: impl AsRef<Path>,
) -> Result<(), FixtureError> {
    save(provider, path)
}

/// Load an `IacDataSource` from a fixture file.
///
/// # Errors
/// See [`load`].
pub fn load_data_source(path: impl AsRef<Path>) -> Result<IacDataSource, FixtureError> {
    load::<IacDataSource>(path)
}

/// Save an `IacDataSource` to a fixture file.
///
/// # Errors
/// See [`save`].
pub fn save_data_source(
    ds: &IacDataSource,
    path: impl AsRef<Path>,
) -> Result<(), FixtureError> {
    save(ds, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{test_provider, test_resource};
    use tempfile::TempDir;

    #[test]
    fn round_trip_resource_via_disk() {
        let original = test_resource("widget");
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("fx.sexpr");
        save_resource(&original, &path).unwrap();

        let loaded = load_resource(&path).unwrap();
        assert_eq!(loaded.name, original.name);
        assert_eq!(loaded.attributes, original.attributes);
    }

    #[test]
    fn round_trip_resource_via_string() {
        let original = test_resource("widget");
        let text = original.to_sexpr().emit();
        let loaded = load_resource_str(&text).unwrap();
        assert_eq!(loaded.name, original.name);
        assert_eq!(loaded.attributes, original.attributes);
    }

    #[test]
    fn save_creates_parent_directories() {
        let original = test_resource("widget");
        let dir = TempDir::new().unwrap();
        let nested = dir.path().join("a/b/c/widget.sexpr");
        save_resource(&original, &nested).unwrap();
        assert!(nested.exists());
    }

    #[test]
    fn saved_file_ends_with_newline() {
        let original = test_resource("widget");
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("fx.sexpr");
        save_resource(&original, &path).unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.ends_with('\n'));
    }

    #[test]
    fn load_nonexistent_file_errors() {
        let err = load_resource("/nonexistent/fixture.sexpr").unwrap_err();
        assert!(matches!(err, FixtureError::Io(_)));
    }

    #[test]
    fn load_malformed_sexpr_errors() {
        let err = load_resource_str("(resource").unwrap_err();
        assert!(matches!(err, FixtureError::Sexpr(_)));
    }

    #[test]
    fn load_wrong_top_level_shape_errors() {
        // Valid sexpr, but wrong top-level — should reject as shape error.
        let err = load_resource_str("(unknown-top-level)").unwrap_err();
        assert!(matches!(err, FixtureError::Sexpr(_)));
    }

    #[test]
    fn round_trip_provider_via_disk() {
        let original = test_provider("acme");
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("provider.sexpr");
        save_provider(&original, &path).unwrap();
        let loaded = load_provider(&path).unwrap();
        assert_eq!(loaded.name, original.name);
        assert_eq!(loaded.platform_config, original.platform_config);
    }

    #[test]
    fn round_trip_data_source_via_disk() {
        use crate::testing::test_data_source;
        let original = test_data_source("secret");
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("ds.sexpr");
        save_data_source(&original, &path).unwrap();
        let loaded = load_data_source(&path).unwrap();
        assert_eq!(loaded.name, original.name);
        assert_eq!(loaded.attributes, original.attributes);
    }

    #[test]
    fn generic_helpers_work_for_any_to_sexpr_type() {
        // Proves the generic `load`/`save` work for any FromSExpr type.
        let attr = crate::ir::IacAttribute {
            api_name: "field".into(),
            canonical_name: "field".into(),
            ..Default::default()
        };
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("attr.sexpr");
        save(&attr, &path).unwrap();
        let loaded: crate::ir::IacAttribute = load(&path).unwrap();
        assert_eq!(loaded, attr);
    }

    #[test]
    fn fixture_error_display() {
        let err = FixtureError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "x"));
        assert!(err.to_string().contains("fixture io"));
        let err2: FixtureError = SExprError::Parse("x".into()).into();
        assert!(err2.to_string().contains("fixture sexpr"));
    }
}
