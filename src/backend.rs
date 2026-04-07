use serde::{Deserialize, Serialize};

use crate::error::IacForgeError;
use crate::ir::{IacDataSource, IacProvider, IacResource};

/// Kind of generated artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
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

impl std::str::FromStr for ArtifactKind {
    type Err = IacForgeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "resource" => Ok(Self::Resource),
            "data_source" => Ok(Self::DataSource),
            "provider" => Ok(Self::Provider),
            "test" => Ok(Self::Test),
            "schema" => Ok(Self::Schema),
            "module" => Ok(Self::Module),
            "metadata" => Ok(Self::Metadata),
            _ => Err(IacForgeError::ValidationError(format!(
                "unknown artifact kind: {s}"
            ))),
        }
    }
}

/// A single generated file from a backend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GeneratedArtifact {
    /// Relative output path for the file.
    pub path: String,
    /// File content.
    pub content: String,
    /// What kind of artifact this is.
    pub kind: ArtifactKind,
}

impl std::fmt::Display for GeneratedArtifact {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.kind, self.path)
    }
}

/// Naming convention for a specific platform.
///
/// Each backend defines how API names are transformed to platform-idiomatic names.
/// Default implementations delegate to `resource_type_name` where possible,
/// so backends only need to override methods where their convention differs.
pub trait NamingConvention {
    /// Generate the platform resource type name from a resource name and provider.
    fn resource_type_name(&self, resource_name: &str, provider_name: &str) -> String;

    /// Generate the platform data source type name.
    ///
    /// Defaults to the same as `resource_type_name`. Override for platforms
    /// where data sources have different naming (e.g., Pulumi uses `get` prefix).
    fn data_source_type_name(&self, ds_name: &str, provider_name: &str) -> String {
        self.resource_type_name(ds_name, provider_name)
    }

    /// Generate the output file name for an artifact.
    fn file_name(&self, resource_name: &str, kind: &ArtifactKind) -> String;

    /// Transform an API field name to the platform's convention.
    fn field_name(&self, api_name: &str) -> String;
}

/// Backend trait -- each `IaC` platform implements this.
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

    /// Validate an `IacResource` before generation.
    ///
    /// Returns a list of human-readable validation messages. An empty list
    /// means the resource is valid for this backend.
    fn validate_resource(
        &self,
        _resource: &IacResource,
        _provider: &IacProvider,
    ) -> Vec<String> {
        vec![]
    }

    /// Generate all artifacts for a complete provider in one batch.
    ///
    /// Default implementation loops over resources and data sources, then
    /// generates provider-level artifacts. Backends can override for
    /// optimized batch generation (e.g., Pulumi generates a single schema.json).
    ///
    /// # Errors
    ///
    /// Returns an error if any generation step fails.
    fn generate_all(
        &self,
        provider: &IacProvider,
        resources: &[IacResource],
        data_sources: &[IacDataSource],
    ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
        let mut artifacts = Vec::new();
        for r in resources {
            artifacts.extend(self.generate_resource(r, provider)?);
        }
        for ds in data_sources {
            artifacts.extend(self.generate_data_source(ds, provider)?);
        }
        artifacts.extend(self.generate_provider(provider, resources, data_sources)?);
        for r in resources {
            artifacts.extend(self.generate_test(r, provider)?);
        }
        Ok(artifacts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{AuthInfo, CrudInfo, IdentityInfo};
    use std::collections::BTreeMap;

    #[test]
    fn artifact_kind_display() {
        assert_eq!(ArtifactKind::Resource.to_string(), "resource");
        assert_eq!(ArtifactKind::Provider.to_string(), "provider");
        assert_eq!(ArtifactKind::Test.to_string(), "test");
        assert_eq!(ArtifactKind::Schema.to_string(), "schema");
    }

    /// Minimal backend for testing default method implementations.
    struct TestBackend;

    struct TestNaming;

    impl NamingConvention for TestNaming {
        fn resource_type_name(&self, resource_name: &str, provider_name: &str) -> String {
            format!("{provider_name}_{resource_name}")
        }
        fn file_name(&self, resource_name: &str, _kind: &ArtifactKind) -> String {
            format!("{resource_name}.go")
        }
        fn field_name(&self, api_name: &str) -> String {
            api_name.to_string()
        }
    }

    impl Backend for TestBackend {
        fn platform(&self) -> &str {
            "test"
        }
        fn generate_resource(
            &self,
            resource: &IacResource,
            _provider: &IacProvider,
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![GeneratedArtifact {
                path: format!("resource_{}.go", resource.name),
                content: String::new(),
                kind: ArtifactKind::Resource,
            }])
        }
        fn generate_data_source(
            &self,
            ds: &IacDataSource,
            _provider: &IacProvider,
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![GeneratedArtifact {
                path: format!("data_source_{}.go", ds.name),
                content: String::new(),
                kind: ArtifactKind::DataSource,
            }])
        }
        fn generate_provider(
            &self,
            _provider: &IacProvider,
            _resources: &[IacResource],
            _data_sources: &[IacDataSource],
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![GeneratedArtifact {
                path: "provider.go".to_string(),
                content: String::new(),
                kind: ArtifactKind::Provider,
            }])
        }
        fn generate_test(
            &self,
            resource: &IacResource,
            _provider: &IacProvider,
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![GeneratedArtifact {
                path: format!("resource_{}_test.go", resource.name),
                content: String::new(),
                kind: ArtifactKind::Test,
            }])
        }
        fn naming(&self) -> &dyn NamingConvention {
            &TestNaming
        }
    }

    fn make_test_provider() -> IacProvider {
        IacProvider {
            name: "test".to_string(),
            description: "Test provider".to_string(),
            version: "1.0.0".to_string(),
            auth: AuthInfo::default(),
            skip_fields: vec![],
            platform_config: BTreeMap::new(),
        }
    }

    fn make_test_resource(name: &str) -> IacResource {
        IacResource {
            name: name.to_string(),
            description: String::new(),
            category: "test".to_string(),
            crud: CrudInfo {
                create_endpoint: "/create".to_string(),
                create_schema: "Create".to_string(),
                update_endpoint: None,
                update_schema: None,
                read_endpoint: "/read".to_string(),
                read_schema: "Read".to_string(),
                read_response_schema: None,
                delete_endpoint: "/delete".to_string(),
                delete_schema: "Delete".to_string(),
            },
            attributes: vec![],
            identity: IdentityInfo {
                id_field: "id".to_string(),
                import_field: "id".to_string(),
                force_replace_fields: vec![],
            },
        }
    }

    fn make_test_data_source(name: &str) -> IacDataSource {
        IacDataSource {
            name: name.to_string(),
            description: String::new(),
            read_endpoint: "/read".to_string(),
            read_schema: "Read".to_string(),
            read_response_schema: None,
            attributes: vec![],
        }
    }

    #[test]
    fn validate_resource_default_returns_empty() {
        let backend = TestBackend;
        let provider = make_test_provider();
        let resource = make_test_resource("foo");
        let errors = backend.validate_resource(&resource, &provider);
        assert!(errors.is_empty());
    }

    #[test]
    fn generate_all_default_delegates() {
        let backend = TestBackend;
        let provider = make_test_provider();
        let resources = vec![make_test_resource("r1"), make_test_resource("r2")];
        let data_sources = vec![make_test_data_source("ds1")];

        let artifacts = backend
            .generate_all(&provider, &resources, &data_sources)
            .expect("generate_all");

        // 2 resources + 1 data source + 1 provider + 2 tests = 6
        assert_eq!(artifacts.len(), 6);
        assert_eq!(
            artifacts
                .iter()
                .filter(|a| a.kind == ArtifactKind::Resource)
                .count(),
            2
        );
        assert_eq!(
            artifacts
                .iter()
                .filter(|a| a.kind == ArtifactKind::DataSource)
                .count(),
            1
        );
        assert_eq!(
            artifacts
                .iter()
                .filter(|a| a.kind == ArtifactKind::Provider)
                .count(),
            1
        );
        assert_eq!(
            artifacts
                .iter()
                .filter(|a| a.kind == ArtifactKind::Test)
                .count(),
            2
        );
    }

    #[test]
    fn data_source_type_name_default() {
        let naming = TestNaming;
        assert_eq!(
            naming.data_source_type_name("auth_method", "akeyless"),
            naming.resource_type_name("auth_method", "akeyless")
        );
    }

    #[test]
    fn generated_artifact_display() {
        let artifact = GeneratedArtifact {
            path: "resource_secret.go".to_string(),
            content: "package main".to_string(),
            kind: ArtifactKind::Resource,
        };
        assert_eq!(artifact.to_string(), "[resource] resource_secret.go");
    }

    #[test]
    fn generated_artifact_serialize_roundtrip() {
        let artifact = GeneratedArtifact {
            path: "provider.go".to_string(),
            content: "code here".to_string(),
            kind: ArtifactKind::Provider,
        };
        let json = serde_json::to_string(&artifact).expect("serialize");
        let deserialized: GeneratedArtifact = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(artifact, deserialized);
    }

    #[test]
    fn artifact_kind_serialize_roundtrip() {
        let kinds = vec![
            ArtifactKind::Resource,
            ArtifactKind::DataSource,
            ArtifactKind::Provider,
            ArtifactKind::Test,
            ArtifactKind::Schema,
            ArtifactKind::Module,
            ArtifactKind::Metadata,
        ];
        for kind in kinds {
            let json = serde_json::to_string(&kind).expect("serialize");
            let deserialized: ArtifactKind = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(kind, deserialized);
        }
    }

    #[test]
    fn artifact_kind_display_all_variants() {
        assert_eq!(ArtifactKind::DataSource.to_string(), "data_source");
        assert_eq!(ArtifactKind::Module.to_string(), "module");
        assert_eq!(ArtifactKind::Metadata.to_string(), "metadata");
    }

    /// Backend that returns errors for resource generation.
    struct FailingBackend;

    struct FailingNaming;

    impl NamingConvention for FailingNaming {
        fn resource_type_name(&self, resource_name: &str, provider_name: &str) -> String {
            format!("{provider_name}_{resource_name}")
        }
        fn file_name(&self, resource_name: &str, _kind: &ArtifactKind) -> String {
            format!("{resource_name}.go")
        }
        fn field_name(&self, api_name: &str) -> String {
            api_name.to_string()
        }
    }

    impl Backend for FailingBackend {
        fn platform(&self) -> &str {
            "failing"
        }
        fn generate_resource(
            &self,
            _resource: &IacResource,
            _provider: &IacProvider,
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Err(IacForgeError::BackendError(
                "resource generation failed".to_string(),
            ))
        }
        fn generate_data_source(
            &self,
            _ds: &IacDataSource,
            _provider: &IacProvider,
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![])
        }
        fn generate_provider(
            &self,
            _provider: &IacProvider,
            _resources: &[IacResource],
            _data_sources: &[IacDataSource],
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![])
        }
        fn generate_test(
            &self,
            _resource: &IacResource,
            _provider: &IacProvider,
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![])
        }
        fn naming(&self) -> &dyn NamingConvention {
            &FailingNaming
        }
    }

    #[test]
    fn generate_all_propagates_resource_error() {
        let backend = FailingBackend;
        let provider = make_test_provider();
        let resources = vec![make_test_resource("r1")];
        let data_sources = vec![];

        let result = backend.generate_all(&provider, &resources, &data_sources);
        assert!(result.is_err(), "should propagate resource generation error");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("resource generation failed"),
            "error message should contain failure reason"
        );
    }

    /// Backend that fails on data source generation.
    struct FailingDsBackend;

    impl Backend for FailingDsBackend {
        fn platform(&self) -> &str {
            "failing_ds"
        }
        fn generate_resource(
            &self,
            _resource: &IacResource,
            _provider: &IacProvider,
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![])
        }
        fn generate_data_source(
            &self,
            _ds: &IacDataSource,
            _provider: &IacProvider,
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Err(IacForgeError::BackendError(
                "data source generation failed".to_string(),
            ))
        }
        fn generate_provider(
            &self,
            _provider: &IacProvider,
            _resources: &[IacResource],
            _data_sources: &[IacDataSource],
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![])
        }
        fn generate_test(
            &self,
            _resource: &IacResource,
            _provider: &IacProvider,
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![])
        }
        fn naming(&self) -> &dyn NamingConvention {
            &FailingNaming
        }
    }

    #[test]
    fn generate_all_propagates_data_source_error() {
        let backend = FailingDsBackend;
        let provider = make_test_provider();
        let resources = vec![];
        let data_sources = vec![make_test_data_source("ds1")];

        let result = backend.generate_all(&provider, &resources, &data_sources);
        assert!(
            result.is_err(),
            "should propagate data source generation error"
        );
    }

    /// Backend that overrides validate_resource.
    struct ValidatingBackend;

    impl Backend for ValidatingBackend {
        fn platform(&self) -> &str {
            "validating"
        }
        fn generate_resource(
            &self,
            _resource: &IacResource,
            _provider: &IacProvider,
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![])
        }
        fn generate_data_source(
            &self,
            _ds: &IacDataSource,
            _provider: &IacProvider,
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![])
        }
        fn generate_provider(
            &self,
            _provider: &IacProvider,
            _resources: &[IacResource],
            _data_sources: &[IacDataSource],
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![])
        }
        fn generate_test(
            &self,
            _resource: &IacResource,
            _provider: &IacProvider,
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![])
        }
        fn naming(&self) -> &dyn NamingConvention {
            &TestNaming
        }
        fn validate_resource(
            &self,
            resource: &IacResource,
            _provider: &IacProvider,
        ) -> Vec<String> {
            let mut errors = Vec::new();
            if resource.attributes.is_empty() {
                errors.push("resource has no attributes".to_string());
            }
            errors
        }
    }

    #[test]
    fn validate_resource_override_returns_errors() {
        let backend = ValidatingBackend;
        let provider = make_test_provider();
        let resource = make_test_resource("empty");
        // resource has no attributes, so validation should report an error
        let errors = backend.validate_resource(&resource, &provider);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0], "resource has no attributes");
    }

    #[test]
    fn validate_resource_override_returns_empty_for_valid() {
        let backend = ValidatingBackend;
        let provider = make_test_provider();
        let mut resource = make_test_resource("valid");
        resource.attributes.push(crate::ir::IacAttribute {
            api_name: "name".to_string(),
            canonical_name: "name".to_string(),
            description: String::new(),
            iac_type: crate::ir::IacType::String,
            required: true,
            optional: false,
            computed: false,
            sensitive: false,
            json_encoded: false,
            immutable: false,
            default_value: None,
            enum_values: None,
            read_path: None,
            update_only: false,
        });
        let errors = backend.validate_resource(&resource, &provider);
        assert!(errors.is_empty());
    }

    #[test]
    fn generate_all_empty_inputs() {
        let backend = TestBackend;
        let provider = make_test_provider();
        let artifacts = backend
            .generate_all(&provider, &[], &[])
            .expect("generate_all");
        // Only provider artifact
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].kind, ArtifactKind::Provider);
    }

    /// Backend that fails on provider generation.
    struct FailingProviderBackend;

    impl Backend for FailingProviderBackend {
        fn platform(&self) -> &str {
            "failing_provider"
        }
        fn generate_resource(
            &self,
            _resource: &IacResource,
            _provider: &IacProvider,
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![])
        }
        fn generate_data_source(
            &self,
            _ds: &IacDataSource,
            _provider: &IacProvider,
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![])
        }
        fn generate_provider(
            &self,
            _provider: &IacProvider,
            _resources: &[IacResource],
            _data_sources: &[IacDataSource],
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Err(IacForgeError::BackendError(
                "provider generation failed".to_string(),
            ))
        }
        fn generate_test(
            &self,
            _resource: &IacResource,
            _provider: &IacProvider,
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![])
        }
        fn naming(&self) -> &dyn NamingConvention {
            &FailingNaming
        }
    }

    #[test]
    fn generate_all_propagates_provider_error() {
        let backend = FailingProviderBackend;
        let provider = make_test_provider();
        let result = backend.generate_all(&provider, &[], &[]);
        assert!(result.is_err(), "should propagate provider generation error");
    }

    /// Backend that fails on test generation.
    struct FailingTestBackend;

    impl Backend for FailingTestBackend {
        fn platform(&self) -> &str {
            "failing_test"
        }
        fn generate_resource(
            &self,
            _resource: &IacResource,
            _provider: &IacProvider,
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![])
        }
        fn generate_data_source(
            &self,
            _ds: &IacDataSource,
            _provider: &IacProvider,
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![])
        }
        fn generate_provider(
            &self,
            _provider: &IacProvider,
            _resources: &[IacResource],
            _data_sources: &[IacDataSource],
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![])
        }
        fn generate_test(
            &self,
            _resource: &IacResource,
            _provider: &IacProvider,
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Err(IacForgeError::BackendError(
                "test generation failed".to_string(),
            ))
        }
        fn naming(&self) -> &dyn NamingConvention {
            &FailingNaming
        }
    }

    #[test]
    fn generate_all_propagates_test_error() {
        let backend = FailingTestBackend;
        let provider = make_test_provider();
        let resources = vec![make_test_resource("r1")];
        let result = backend.generate_all(&provider, &resources, &[]);
        assert!(result.is_err(), "should propagate test generation error");
    }

    #[test]
    fn generate_all_ordering_resources_before_datasources_before_provider() {
        let backend = TestBackend;
        let provider = make_test_provider();
        let resources = vec![make_test_resource("r1")];
        let data_sources = vec![make_test_data_source("ds1")];
        let artifacts = backend
            .generate_all(&provider, &resources, &data_sources)
            .expect("generate_all");

        let resource_idx = artifacts
            .iter()
            .position(|a| a.kind == ArtifactKind::Resource)
            .expect("resource");
        let ds_idx = artifacts
            .iter()
            .position(|a| a.kind == ArtifactKind::DataSource)
            .expect("data_source");
        let provider_idx = artifacts
            .iter()
            .position(|a| a.kind == ArtifactKind::Provider)
            .expect("provider");
        let test_idx = artifacts
            .iter()
            .position(|a| a.kind == ArtifactKind::Test)
            .expect("test");

        assert!(
            resource_idx < ds_idx,
            "resources should come before data sources"
        );
        assert!(
            ds_idx < provider_idx,
            "data sources should come before provider"
        );
        assert!(
            provider_idx < test_idx,
            "provider should come before tests"
        );
    }

    #[test]
    fn generate_all_multiple_resources_produces_correct_count() {
        let backend = TestBackend;
        let provider = make_test_provider();
        let resources = vec![
            make_test_resource("r1"),
            make_test_resource("r2"),
            make_test_resource("r3"),
        ];
        let data_sources = vec![
            make_test_data_source("ds1"),
            make_test_data_source("ds2"),
        ];
        let artifacts = backend
            .generate_all(&provider, &resources, &data_sources)
            .expect("generate_all");
        // 3 resources + 2 data sources + 1 provider + 3 tests = 9
        assert_eq!(artifacts.len(), 9);
    }

    #[test]
    fn naming_convention_resource_type_name() {
        let naming = TestNaming;
        assert_eq!(naming.resource_type_name("secret", "akeyless"), "akeyless_secret");
    }

    #[test]
    fn naming_convention_file_name() {
        let naming = TestNaming;
        assert_eq!(naming.file_name("secret", &ArtifactKind::Resource), "secret.go");
        assert_eq!(naming.file_name("secret", &ArtifactKind::Test), "secret.go");
    }

    #[test]
    fn naming_convention_field_name() {
        let naming = TestNaming;
        assert_eq!(naming.field_name("my_field"), "my_field");
    }

    #[test]
    fn generated_artifact_display_data_source() {
        let artifact = GeneratedArtifact {
            path: "data_source_config.go".to_string(),
            content: String::new(),
            kind: ArtifactKind::DataSource,
        };
        assert_eq!(artifact.to_string(), "[data_source] data_source_config.go");
    }

    #[test]
    fn generated_artifact_display_test() {
        let artifact = GeneratedArtifact {
            path: "resource_secret_test.go".to_string(),
            content: "test code".to_string(),
            kind: ArtifactKind::Test,
        };
        assert_eq!(artifact.to_string(), "[test] resource_secret_test.go");
    }

    #[test]
    fn generated_artifact_equality() {
        let a = GeneratedArtifact {
            path: "a.go".to_string(),
            content: "content".to_string(),
            kind: ArtifactKind::Resource,
        };
        let b = GeneratedArtifact {
            path: "a.go".to_string(),
            content: "content".to_string(),
            kind: ArtifactKind::Resource,
        };
        let c = GeneratedArtifact {
            path: "a.go".to_string(),
            content: "different".to_string(),
            kind: ArtifactKind::Resource,
        };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn generated_artifact_display_metadata() {
        let artifact = GeneratedArtifact {
            path: "metadata.json".to_string(),
            content: "{}".to_string(),
            kind: ArtifactKind::Metadata,
        };
        assert_eq!(artifact.to_string(), "[metadata] metadata.json");
    }

    #[test]
    fn generated_artifact_display_schema() {
        let artifact = GeneratedArtifact {
            path: "schema.json".to_string(),
            content: "{}".to_string(),
            kind: ArtifactKind::Schema,
        };
        assert_eq!(artifact.to_string(), "[schema] schema.json");
    }

    #[test]
    fn generated_artifact_display_module() {
        let artifact = GeneratedArtifact {
            path: "index.ts".to_string(),
            content: String::new(),
            kind: ArtifactKind::Module,
        };
        assert_eq!(artifact.to_string(), "[module] index.ts");
    }

    #[test]
    fn backend_platform_name() {
        let backend = TestBackend;
        assert_eq!(backend.platform(), "test");
    }

    #[test]
    fn artifact_kind_from_str_roundtrip() {
        let kinds = vec![
            ArtifactKind::Resource,
            ArtifactKind::DataSource,
            ArtifactKind::Provider,
            ArtifactKind::Test,
            ArtifactKind::Schema,
            ArtifactKind::Module,
            ArtifactKind::Metadata,
        ];
        for kind in kinds {
            let s = kind.to_string();
            let parsed: ArtifactKind = s.parse().expect("parse");
            assert_eq!(kind, parsed);
        }
    }

    #[test]
    fn artifact_kind_from_str_invalid() {
        let result: Result<ArtifactKind, _> = "unknown".parse();
        assert!(result.is_err());
    }
}
