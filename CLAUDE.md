# iac-forge

Platform-independent IaC code generation core library. Defines the IR types,
Backend trait, resolver, and shared test fixtures that all `*-forge` backends consume.

## Architecture

```
ResourceSpec (TOML) + openapi_forge::Spec
     │
     ▼  resolve_resource() / resolve_data_source()
IacResource / IacDataSource (platform-neutral IR)
     │
     ▼  Backend::generate_resource() / generate_all()
GeneratedArtifact { path, content, kind }
```

## Key Types

- **`IacType`** — `String | Integer | Float | Boolean | List | Set | Map | Object | Enum | Any` (Serialize/Deserialize/Eq/Hash)
- **`IacAttribute`** — resolved field with `required`, `computed`, `sensitive`, `immutable`, `update_only`, `read_path`
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
