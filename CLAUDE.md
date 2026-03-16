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

## Helpers

```rust
resource.input_attributes()        // non-computed or required
resource.output_attributes()       // computed or required
resource.required_attribute_names()
resource.sensitive_attribute_names()
resource.immutable_attribute_names()
```
