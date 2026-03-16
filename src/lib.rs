pub mod backend;
pub mod error;
pub mod ir;
pub mod naming;
pub mod resolve;
pub mod spec;
pub mod type_map;

// Re-export key types for convenience.
pub use backend::{ArtifactKind, Backend, GeneratedArtifact, NamingConvention};
pub use error::IacForgeError;
pub use ir::{
    AuthInfo, CrudInfo, IacAttribute, IacDataSource, IacProvider, IacResource, IacType,
    IdentityInfo,
};
pub use naming::{strip_provider_prefix, to_camel_case, to_kebab_case, to_pascal_case, to_snake_case};
pub use resolve::{resolve_data_source, resolve_provider, resolve_resource};
pub use spec::{
    AuthConfig, CrudMapping, DataSourceMeta, DataSourceSpec, FieldOverride, IdentityConfig,
    ProviderDefaults, ProviderMeta, ProviderSpec, ReadMapping, ResourceMeta, ResourceSpec,
};
pub use type_map::{apply_enum_constraint, is_valid_type_override, openapi_to_iac};
