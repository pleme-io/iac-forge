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
/// Structure-preserving maps with composable proofs.
pub mod morphism;
/// Canonical s-expression interchange for IR values.
pub mod sexpr;
/// Semantic diff over sexpr trees.
pub mod sexpr_diff;
/// Remediation harness: bounded transform application with invariants.
pub mod remediation;
/// Content-addressed cache over backend rendering.
pub mod render_cache;
/// Fleet: named collection of IacResource values with composite hashing.
pub mod fleet;
/// Policy-as-code over sexpr patterns.
pub mod policy;
/// Sexpr ↔ Nix AST bridge (NixValue + round-trip to SExpr).
pub mod nix;
/// Nix backend: `Backend` impl rendering IR as Nix attribute sets.
pub mod nix_backend;
/// Nix-powered IR transforms (external evaluator: nix-instantiate or sui).
pub mod nix_transform;
/// `ToSExpr` / `FromSExpr` impls for the IR value types.
mod sexpr_ir;
/// Naming convention helpers (snake_case, camelCase, etc.).
pub mod naming;
/// User-extensible IR transforms with a minimal s-expr script surface.
pub mod transform;
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
    AuthInfo, CrudInfo, HasAttributes, IacAttribute, IacDataSource, IacProvider, IacResource,
    IacType, IdentityInfo,
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
