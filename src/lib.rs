//! Platform-independent `IaC` code generation core library.
//!
//! Defines the IR types ([`IacType`], [`IacAttribute`], [`IacResource`]),
//! the [`Backend`] trait, the resolver functions, and shared test fixtures
//! that all `*-forge` backends consume.

/// Backend trait and generated artifact types.
pub mod backend;
/// Error types for the iac-forge pipeline.
pub mod error;
/// Platform-independent intermediate representation (IR).
pub mod ir;
/// Naming convention helpers (snake_case, camelCase, etc.).
pub mod naming;
/// Resolver: spec + `OpenAPI` → IR.
pub mod resolve;
/// TOML spec types for resources, data sources, and providers.
pub mod spec;
/// Shared test fixtures for backend tests.
pub mod testing;
/// Type mapping from `OpenAPI` / takumi types to `IacType`.
pub mod type_map;

// Re-export key types for convenience.
pub use backend::{ArtifactKind, Backend, GeneratedArtifact, NamingConvention};
pub use error::IacForgeError;
pub use ir::{
    AuthInfo, CrudInfo, IacAttribute, IacDataSource, IacProvider, IacResource, IacType,
    IdentityInfo,
};
pub use naming::{
    strip_provider_prefix, to_camel_case, to_kebab_case, to_pascal_case, to_snake_case,
};
pub use resolve::{resolve_data_source, resolve_provider, resolve_resource};
pub use spec::{
    AuthConfig, ConfigLoader, CrudMapping, DataSourceMeta, DataSourceSpec, FieldOverride,
    IdentityConfig, ProviderDefaults, ProviderMeta, ProviderSpec, ReadMapping, ResourceMeta,
    ResourceSpec,
};
pub use testing::{TestAttributeBuilder, test_data_source, test_provider, test_resource, test_resource_with_type};
pub use type_map::{apply_enum_constraint, is_valid_type_override, openapi_to_iac};
