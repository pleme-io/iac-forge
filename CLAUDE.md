# iac-forge

Platform-independent IaC code generation core library. Defines the IR types,
Backend trait, resolver, and shared test fixtures that all `*-forge` backends consume.
The IR is the shared language between two input paths and seven output backends.

## Architecture

Two input paths produce the same IR:

```
Path 1: TOML + OpenAPI (iac-forge-cli)
  ResourceSpec (TOML) + openapi_forge::Spec
       │
       ▼  resolve_resource() / resolve_data_source()
       IacResource / IacDataSource (platform-neutral IR)

Path 2: Terraform Schema (terraform-schema-importer)
  terraform providers schema -json
       │
       ▼  convert_resource()
       IacResource (same IR)

Both paths feed into:
  IacResource / IacDataSource
       │
       ▼  Backend::generate_resource() / generate_all()
  GeneratedArtifact { path, content, kind }
```

Path 1 is used for providers with OpenAPI specs (Akeyless, Datadog, Splunk).
Path 2 is used for all Terraform-native providers (AWS, Azure, GCP, Cloudflare,
and 21 others). Both produce identical IacResource IR that backends consume.

## Key Types

- **`IacType`** — `String | Integer | Float | Numeric | Boolean | List | Set | Map | Object | Enum | Any` (Serialize/Deserialize/Eq/Hash)
- **`IacAttribute`** — resolved field with `required`, `optional`, `computed`, `sensitive`, `immutable`, `update_only`, `read_path`, `json_encoded`
- **`IacResource`** — resolved resource with attributes, CRUD info, identity
- **`IacDataSource`** — resolved data source with attributes
- **`IacProvider`** — provider config with auth, skip_fields, platform_config

## Key Traits

- **`Backend`** — `generate_resource()`, `generate_data_source()`, `generate_provider()`, `generate_test()`, `validate_resource()`, `generate_all()`
- **`NamingConvention`** — `resource_type_name()`, `data_source_type_name()`, `file_name()`, `field_name()`
- **`ConfigLoader`** — `load(path)`, `from_toml(str)` for spec types

## Testing

Use `iac_forge::testing` for shared fixtures:
```rust
use iac_forge::testing::{test_provider, test_resource, TestAttributeBuilder};
let provider = test_provider("acme");
let resource = test_resource("secret");
let attr = TestAttributeBuilder::new("key", IacType::String).required().sensitive().build();
```

## ConfigLoader Trait

Eliminates duplicated `load()` methods across spec types. Provides both
file-based TOML loading and string-based parsing (useful for tests).

```rust
use iac_forge::ConfigLoader;

// Load from file
let spec = ResourceSpec::load(Path::new("resources/secret.toml"))?;

// Parse from string (tests)
let spec = ResourceSpec::from_toml(toml_str)?;
```

Implemented for: `ResourceSpec`, `DataSourceSpec`, `ProviderSpec`.

## Testing Module

`iac_forge::testing` provides shared fixtures for backend tests. Use these to
avoid duplicating test data construction across `terraform-forge`, `pulumi-forge`,
`crossplane-forge`, and `ansible-forge`.

### Fixtures

```rust
use iac_forge::testing::{test_provider, test_resource, test_data_source,
                          test_resource_with_type, TestAttributeBuilder};

// Minimal provider with auth config
let provider = test_provider("acme");

// Resource with 3 attrs: name (required+immutable), value (required+sensitive), tags (list)
let resource = test_resource("secret");

// Resource with a single attribute of a specific type
let resource = test_resource_with_type("flag", "enabled", IacType::Boolean);

// Data source with 2 attrs: name (required), value (computed)
let ds = test_data_source("config");
```

### TestAttributeBuilder

Fluent builder for constructing test `IacAttribute` values:

```rust
let attr = TestAttributeBuilder::new("secret-key", IacType::String)
    .required()
    .computed()
    .sensitive()
    .immutable()
    .update_only()
    .read_path("secret_key_resp")
    .description("A secret key")
    .default_value(serde_json::json!("default"))
    .enum_values(vec!["a".into(), "b".into()])
    .build();
```

Hyphenated names are auto-converted to snake_case for `canonical_name`
(e.g., `"my-field"` -> api_name `"my-field"`, canonical_name `"my_field"`).

## Helpers

```rust
resource.input_attributes()        // non-computed or required
resource.output_attributes()       // computed or required
resource.required_attribute_names()
resource.sensitive_attribute_names()
resource.immutable_attribute_names()
```

## IacType::Numeric

`Numeric` represents Terraform's `number` type, which can be integer or float.
Backends map it to their most appropriate numeric type:
- pangea-forge: `T::Coercible::Float`
- terraform-forge: `schema.TypeFloat`
- pulumi-forge: `pulumi.Number`

## json_encoded Annotation

`IacAttribute.json_encoded = true` marks fields that contain serialized JSON
(e.g., IAM policy documents). Backends can use this hint to generate
appropriate helpers (e.g., `jsonencode()` wrappers in Terraform, `to_json`
coercion in Pangea).
