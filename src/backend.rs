use crate::error::IacForgeError;
use crate::ir::{IacDataSource, IacProvider, IacResource};

/// Kind of generated artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactKind {
    Resource,
    DataSource,
    Provider,
    Test,
    Schema,
    Module,
    Metadata,
}

impl std::fmt::Display for ArtifactKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Resource => write!(f, "resource"),
            Self::DataSource => write!(f, "data_source"),
            Self::Provider => write!(f, "provider"),
            Self::Test => write!(f, "test"),
            Self::Schema => write!(f, "schema"),
            Self::Module => write!(f, "module"),
            Self::Metadata => write!(f, "metadata"),
        }
    }
}

/// A single generated file from a backend.
#[derive(Debug, Clone)]
pub struct GeneratedArtifact {
    /// Relative output path for the file.
    pub path: String,
    /// File content.
    pub content: String,
    /// What kind of artifact this is.
    pub kind: ArtifactKind,
}

/// Naming convention for a specific platform.
///
/// Each backend defines how API names are transformed to platform-idiomatic names.
pub trait NamingConvention {
    /// Generate the platform resource type name from a resource name and provider.
    fn resource_type_name(&self, resource_name: &str, provider_name: &str) -> String;

    /// Generate the output file name for an artifact.
    fn file_name(&self, resource_name: &str, kind: &ArtifactKind) -> String;

    /// Transform an API field name to the platform's convention.
    fn field_name(&self, api_name: &str) -> String;
}

/// Backend trait — each IaC platform implements this.
///
/// The trait operates on platform-independent IR types, producing
/// platform-specific code as `GeneratedArtifact` values.
pub trait Backend {
    /// Platform identifier (e.g., "terraform", "pulumi", "crossplane").
    fn platform(&self) -> &str;

    /// Generate artifacts for a single resource.
    ///
    /// # Errors
    ///
    /// Returns an error if code generation fails.
    fn generate_resource(
        &self,
        resource: &IacResource,
        provider: &IacProvider,
    ) -> Result<Vec<GeneratedArtifact>, IacForgeError>;

    /// Generate artifacts for a single data source.
    ///
    /// # Errors
    ///
    /// Returns an error if code generation fails.
    fn generate_data_source(
        &self,
        ds: &IacDataSource,
        provider: &IacProvider,
    ) -> Result<Vec<GeneratedArtifact>, IacForgeError>;

    /// Generate provider-level artifacts (registration, configuration).
    ///
    /// # Errors
    ///
    /// Returns an error if code generation fails.
    fn generate_provider(
        &self,
        provider: &IacProvider,
        resources: &[IacResource],
        data_sources: &[IacDataSource],
    ) -> Result<Vec<GeneratedArtifact>, IacForgeError>;

    /// Generate test artifacts for a resource.
    ///
    /// # Errors
    ///
    /// Returns an error if code generation fails.
    fn generate_test(
        &self,
        resource: &IacResource,
        provider: &IacProvider,
    ) -> Result<Vec<GeneratedArtifact>, IacForgeError>;

    /// Get the naming convention for this platform.
    fn naming(&self) -> &dyn NamingConvention;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artifact_kind_display() {
        assert_eq!(ArtifactKind::Resource.to_string(), "resource");
        assert_eq!(ArtifactKind::Provider.to_string(), "provider");
        assert_eq!(ArtifactKind::Test.to_string(), "test");
        assert_eq!(ArtifactKind::Schema.to_string(), "schema");
    }
}
