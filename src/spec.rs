use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::IacForgeError;

/// Trait for loading specs from TOML.
///
/// Eliminates identical `load()` methods across spec types. Provides both
/// file-based loading and string-based parsing (useful for tests).
pub trait ConfigLoader: Sized + serde::de::DeserializeOwned {
    /// Load from a TOML file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file can't be read or parsed.
    fn load(path: &Path) -> Result<Self, IacForgeError> {
        let content = std::fs::read_to_string(path)?;
        let spec: Self = toml::from_str(&content)?;
        Ok(spec)
    }

    /// Parse from a TOML string (useful for tests).
    ///
    /// # Errors
    ///
    /// Returns an error if the string can't be parsed.
    fn from_toml(content: &str) -> Result<Self, IacForgeError> {
        let spec: Self = toml::from_str(content)?;
        Ok(spec)
    }
}

impl ConfigLoader for ResourceSpec {}
impl ConfigLoader for DataSourceSpec {}
impl ConfigLoader for ProviderSpec {}

/// Top-level resource specification loaded from TOML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceSpec {
    /// Resource metadata (name, description, category).
    pub resource: ResourceMeta,
    /// CRUD operation mappings.
    pub crud: CrudMapping,
    /// Identity and import configuration.
    pub identity: IdentityConfig,
    /// Per-field overrides (skip, computed, sensitive, etc.).
    #[serde(default)]
    pub fields: BTreeMap<String, FieldOverride>,
    /// Mapping from API response JSON paths to field names.
    #[serde(default)]
    pub read_mapping: BTreeMap<String, String>,
}

/// Resource metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceMeta {
    /// Resource identifier (e.g., `akeyless_static_secret`).
    pub name: String,
    /// Human-readable description.
    #[serde(default)]
    pub description: String,
    /// Category grouping (e.g., "secret", "auth").
    #[serde(default)]
    pub category: String,
}

/// Maps CRUD operations to API endpoints and schemas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrudMapping {
    /// API path for the create operation.
    pub create_endpoint: String,
    /// `OpenAPI` schema name for the create request body.
    pub create_schema: String,
    /// API path for the update operation, if separate.
    #[serde(default)]
    pub update_endpoint: Option<String>,
    /// `OpenAPI` schema name for the update request body.
    #[serde(default)]
    pub update_schema: Option<String>,
    /// API path for the read operation.
    pub read_endpoint: String,
    /// `OpenAPI` schema name for the read request body.
    pub read_schema: String,
    /// `OpenAPI` schema name for the read response, if different.
    #[serde(default)]
    pub read_response_schema: Option<String>,
    /// API path for the delete operation.
    pub delete_endpoint: String,
    /// `OpenAPI` schema name for the delete request body.
    pub delete_schema: String,
}

/// Identity and import configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityConfig {
    /// Primary identifier field name.
    pub id_field: String,
    /// Field used for Terraform import (defaults to `id_field`).
    #[serde(default)]
    pub import_field: Option<String>,
    /// Fields whose changes force resource replacement.
    #[serde(default)]
    pub force_new_fields: Vec<String>,
}

/// Per-field overrides in the resource spec.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct FieldOverride {
    /// Whether this field is server-computed.
    #[serde(default)]
    pub computed: bool,
    /// Whether this field contains sensitive data.
    #[serde(default)]
    pub sensitive: bool,
    /// Whether to exclude this field from the generated IR.
    #[serde(default)]
    pub skip: bool,
    /// Override the inferred type (e.g., `"bool"`, `"int64"`).
    #[serde(default)]
    pub type_override: Option<String>,
    /// Override the field description.
    #[serde(default)]
    pub description: Option<String>,
    /// Whether changing this field forces resource replacement.
    #[serde(default)]
    pub force_new: bool,
}

/// Provider-level configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSpec {
    /// Provider metadata (name, version, SDK import).
    pub provider: ProviderMeta,
    /// Authentication configuration.
    #[serde(default)]
    pub auth: AuthConfig,
    /// Default settings (e.g., fields to skip).
    #[serde(default)]
    pub defaults: ProviderDefaults,
    /// Per-platform configuration blobs (e.g., terraform, pulumi).
    #[serde(default)]
    pub platforms: BTreeMap<String, toml::Value>,
}

/// Provider metadata (name, version, SDK import path).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderMeta {
    /// Provider identifier (e.g., "akeyless").
    pub name: String,
    /// Human-readable description.
    #[serde(default)]
    pub description: String,
    /// Semantic version of the provider.
    #[serde(default)]
    pub version: String,
    /// Language-specific SDK import path.
    #[serde(default)]
    pub sdk_import: String,
}

/// Authentication configuration for a provider spec.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthConfig {
    /// API field name for the authentication token.
    #[serde(default)]
    pub token_field: String,
    /// Environment variable supplying the token.
    #[serde(default)]
    pub env_var: String,
    /// API field name for the gateway URL.
    #[serde(default)]
    pub gateway_url_field: String,
    /// Environment variable supplying the gateway URL.
    #[serde(default)]
    pub gateway_env_var: String,
}

/// Provider-level default settings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderDefaults {
    /// Fields to skip across all resources for this provider.
    #[serde(default)]
    pub skip_fields: Vec<String>,
}

impl ResourceSpec {
    /// Load a resource spec from a TOML file.
    ///
    /// Delegates to [`ConfigLoader::load`].
    ///
    /// # Errors
    ///
    /// Returns an error if the file can't be read or parsed.
    pub fn load(path: &Path) -> Result<Self, IacForgeError> {
        <Self as ConfigLoader>::load(path)
    }

    /// Validate the resource spec against an `OpenAPI` spec.
    ///
    /// # Errors
    ///
    /// Returns validation errors if schemas are missing or endpoints don't exist.
    pub fn validate(&self, api: &openapi_forge::Spec) -> Result<(), IacForgeError> {
        api.schema(&self.crud.create_schema)
            .map_err(|_| IacForgeError::SchemaNotFound(self.crud.create_schema.clone()))?;

        api.schema(&self.crud.read_schema)
            .map_err(|_| IacForgeError::SchemaNotFound(self.crud.read_schema.clone()))?;

        api.schema(&self.crud.delete_schema)
            .map_err(|_| IacForgeError::SchemaNotFound(self.crud.delete_schema.clone()))?;

        if let Some(ref update_schema) = self.crud.update_schema {
            api.schema(update_schema)
                .map_err(|_| IacForgeError::SchemaNotFound(update_schema.clone()))?;
        }

        if let Some(ref response_schema) = self.crud.read_response_schema {
            api.schema(response_schema)
                .map_err(|_| IacForgeError::SchemaNotFound(response_schema.clone()))?;
        }

        if api.endpoint_by_path(&self.crud.create_endpoint).is_none() {
            return Err(IacForgeError::MissingEndpoint {
                resource: self.resource.name.clone(),
                endpoint: self.crud.create_endpoint.clone(),
            });
        }

        Ok(())
    }
}

impl ProviderSpec {
    /// Load a provider spec from a TOML file.
    ///
    /// Delegates to [`ConfigLoader::load`].
    ///
    /// # Errors
    ///
    /// Returns an error if the file can't be read or parsed.
    pub fn load(path: &Path) -> Result<Self, IacForgeError> {
        <Self as ConfigLoader>::load(path)
    }
}

/// Top-level data source specification loaded from TOML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSourceSpec {
    /// Data source metadata (name, description).
    pub data_source: DataSourceMeta,
    /// Read operation mapping.
    pub read: ReadMapping,
    /// Per-field overrides (skip, computed, sensitive, etc.).
    #[serde(default)]
    pub fields: BTreeMap<String, FieldOverride>,
    /// Mapping from API response JSON paths to field names.
    #[serde(default)]
    pub read_mapping: BTreeMap<String, String>,
}

/// Data source metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSourceMeta {
    /// Data source identifier.
    pub name: String,
    /// Human-readable description.
    #[serde(default)]
    pub description: String,
}

/// Maps a read operation to an API endpoint and schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadMapping {
    /// API path for the read operation.
    pub endpoint: String,
    /// `OpenAPI` schema name for the read request body.
    pub schema: String,
    /// `OpenAPI` schema name for the read response, if different.
    #[serde(default)]
    pub response_schema: Option<String>,
}

impl DataSourceSpec {
    /// Load a data source spec from a TOML file.
    ///
    /// Delegates to [`ConfigLoader::load`].
    ///
    /// # Errors
    ///
    /// Returns an error if the file can't be read or parsed.
    pub fn load(path: &Path) -> Result<Self, IacForgeError> {
        <Self as ConfigLoader>::load(path)
    }

    /// Validate the data source spec against an `OpenAPI` spec.
    ///
    /// # Errors
    ///
    /// Returns validation errors if schemas are missing or endpoints don't exist.
    pub fn validate(&self, api: &openapi_forge::Spec) -> Result<(), IacForgeError> {
        api.schema(&self.read.schema)
            .map_err(|_| IacForgeError::SchemaNotFound(self.read.schema.clone()))?;

        if let Some(ref response_schema) = self.read.response_schema {
            api.schema(response_schema)
                .map_err(|_| IacForgeError::SchemaNotFound(response_schema.clone()))?;
        }

        if api.endpoint_by_path(&self.read.endpoint).is_none() {
            return Err(IacForgeError::MissingEndpoint {
                resource: self.data_source.name.clone(),
                endpoint: self.read.endpoint.clone(),
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_loader_from_toml_resource() {
        let toml_str = r#"
[resource]
name = "test_res"
description = "Test"
category = "test"

[crud]
create_endpoint = "/create"
create_schema = "Create"
read_endpoint = "/read"
read_schema = "Read"
delete_endpoint = "/delete"
delete_schema = "Delete"

[identity]
id_field = "id"
"#;
        let spec = ResourceSpec::from_toml(toml_str).expect("from_toml");
        assert_eq!(spec.resource.name, "test_res");
    }

    #[test]
    fn config_loader_from_toml_data_source() {
        let toml_str = r#"
[data_source]
name = "test_ds"
description = "Test"

[read]
endpoint = "/read"
schema = "Read"
"#;
        let spec = DataSourceSpec::from_toml(toml_str).expect("from_toml");
        assert_eq!(spec.data_source.name, "test_ds");
    }

    #[test]
    fn config_loader_from_toml_provider() {
        let toml_str = r#"
[provider]
name = "test"
description = "Test"
version = "1.0.0"
"#;
        let spec = ProviderSpec::from_toml(toml_str).expect("from_toml");
        assert_eq!(spec.provider.name, "test");
    }

    #[test]
    fn config_loader_from_toml_error() {
        let result = ResourceSpec::from_toml("not valid toml {{{");
        assert!(result.is_err());
    }

    #[test]
    fn parse_resource_spec() {
        let toml_str = r#"
[resource]
name = "akeyless_static_secret"
description = "Static secret"
category = "secret"

[crud]
create_endpoint = "/create-secret"
create_schema = "createSecret"
update_endpoint = "/update-secret-val"
update_schema = "updateSecretVal"
read_endpoint = "/get-secret-value"
read_schema = "getSecretValue"
read_response_schema = "GetSecretValueOutput"
delete_endpoint = "/delete-item"
delete_schema = "deleteItem"

[identity]
id_field = "name"
import_field = "name"
force_new_fields = ["name"]

[fields]
token = { skip = true }
uid_token = { skip = true }
json = { skip = true }
delete_protection = { type_override = "bool" }
"#;
        let spec: ResourceSpec = toml::from_str(toml_str).expect("parse");
        assert_eq!(spec.resource.name, "akeyless_static_secret");
        assert_eq!(spec.crud.create_endpoint, "/create-secret");
        assert!(spec.fields.get("token").unwrap().skip);
        assert_eq!(spec.identity.force_new_fields, vec!["name"]);
    }

    #[test]
    fn parse_data_source_spec() {
        let toml_str = r#"
[data_source]
name = "akeyless_auth_method"
description = "Read an auth method"

[read]
endpoint = "/get-auth-method"
schema = "GetAuthMethod"
response_schema = "AuthMethod"

[fields]
token = { skip = true }

[read_mapping]
"auth_method_access_id" = "access_id"
"#;
        let spec: DataSourceSpec = toml::from_str(toml_str).expect("parse");
        assert_eq!(spec.data_source.name, "akeyless_auth_method");
        assert_eq!(spec.read.endpoint, "/get-auth-method");
        assert_eq!(spec.read.response_schema, Some("AuthMethod".to_string()));
    }

    #[test]
    fn parse_provider_spec() {
        let toml_str = r#"
[provider]
name = "akeyless"
description = "Akeyless Vault Provider"
version = "1.0.0"
sdk_import = "github.com/akeylesslabs/akeyless-go/v5"

[auth]
token_field = "token"
env_var = "AKEYLESS_ACCESS_TOKEN"
gateway_url_field = "api_gateway_address"
gateway_env_var = "AKEYLESS_GATEWAY"

[defaults]
skip_fields = ["token", "uid-token", "json"]
"#;
        let spec: ProviderSpec = toml::from_str(toml_str).expect("parse");
        assert_eq!(spec.provider.name, "akeyless");
        assert_eq!(spec.auth.token_field, "token");
        assert_eq!(spec.defaults.skip_fields.len(), 3);
    }

    #[test]
    fn parse_provider_with_platforms() {
        let toml_str = r#"
[provider]
name = "akeyless"
description = "Akeyless Vault Provider"
version = "1.0.0"
sdk_import = "github.com/akeylesslabs/akeyless-go/v5"

[auth]
token_field = "token"
env_var = "AKEYLESS_ACCESS_TOKEN"

[defaults]
skip_fields = ["token"]

[platforms.terraform]
sdk_import = "github.com/akeylesslabs/akeyless-go/v5"

[platforms.pulumi]
module = "index"

[platforms.crossplane]
group = "akeyless.crossplane.io"
api_version = "v1alpha1"
"#;
        let spec: ProviderSpec = toml::from_str(toml_str).expect("parse");
        assert!(spec.platforms.contains_key("terraform"));
        assert!(spec.platforms.contains_key("pulumi"));
        assert!(spec.platforms.contains_key("crossplane"));
    }

    #[test]
    fn config_loader_from_toml_invalid_resource() {
        let result = ResourceSpec::from_toml("not valid toml {{{");
        assert!(result.is_err());
    }

    #[test]
    fn config_loader_from_toml_invalid_data_source() {
        let result = DataSourceSpec::from_toml("this is not valid");
        assert!(result.is_err());
    }

    #[test]
    fn config_loader_from_toml_invalid_provider() {
        let result = ProviderSpec::from_toml("[broken");
        assert!(result.is_err());
    }

    #[test]
    fn config_loader_from_toml_missing_required_fields() {
        // ResourceSpec requires [resource], [crud], [identity] sections
        let result = ResourceSpec::from_toml(
            r#"
[resource]
name = "test"
"#,
        );
        assert!(result.is_err(), "missing [crud] section should fail");
    }

    #[test]
    fn config_loader_load_nonexistent_file() {
        let result = ResourceSpec::load(std::path::Path::new("/nonexistent/path/resource.toml"));
        assert!(result.is_err());
    }

    #[test]
    fn config_loader_load_provider_nonexistent_file() {
        let result = ProviderSpec::load(std::path::Path::new("/nonexistent/path/provider.toml"));
        assert!(result.is_err());
    }

    #[test]
    fn config_loader_load_data_source_nonexistent_file() {
        let result =
            DataSourceSpec::load(std::path::Path::new("/nonexistent/path/data_source.toml"));
        assert!(result.is_err());
    }

    #[test]
    fn resource_spec_validate_all_schemas_present() {
        let toml_str = r#"
[resource]
name = "valid_res"
description = "Valid resource"
category = "test"

[crud]
create_endpoint = "/create"
create_schema = "CreateSchema"
read_endpoint = "/read"
read_schema = "ReadSchema"
delete_endpoint = "/delete"
delete_schema = "DeleteSchema"

[identity]
id_field = "id"
"#;
        let spec = ResourceSpec::from_toml(toml_str).expect("parse");

        let api_str = r#"
openapi: "3.0.0"
info: { title: Test, version: "1.0" }
paths:
  /create:
    post:
      operationId: create
      responses:
        "200": { description: ok }
components:
  schemas:
    CreateSchema:
      type: object
      properties:
        id: { type: string }
    ReadSchema:
      type: object
      properties:
        id: { type: string }
    DeleteSchema:
      type: object
      properties:
        id: { type: string }
"#;
        let api = openapi_forge::Spec::from_str(api_str).expect("parse");
        assert!(spec.validate(&api).is_ok());
    }

    #[test]
    fn resource_spec_validate_missing_create_schema() {
        let toml_str = r#"
[resource]
name = "bad_res"
description = "Bad resource"
category = "test"

[crud]
create_endpoint = "/create"
create_schema = "MissingCreate"
read_endpoint = "/read"
read_schema = "ReadSchema"
delete_endpoint = "/delete"
delete_schema = "DeleteSchema"

[identity]
id_field = "id"
"#;
        let spec = ResourceSpec::from_toml(toml_str).expect("parse");

        let api_str = r#"
openapi: "3.0.0"
info: { title: Test, version: "1.0" }
paths:
  /create:
    post:
      operationId: create
      responses:
        "200": { description: ok }
components:
  schemas:
    ReadSchema:
      type: object
      properties: {}
    DeleteSchema:
      type: object
      properties: {}
"#;
        let api = openapi_forge::Spec::from_str(api_str).expect("parse");
        let result = spec.validate(&api);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("MissingCreate"));
    }

    #[test]
    fn resource_spec_validate_missing_update_schema() {
        let toml_str = r#"
[resource]
name = "bad_update"
description = "Missing update schema"
category = "test"

[crud]
create_endpoint = "/create"
create_schema = "CreateSchema"
update_endpoint = "/update"
update_schema = "MissingUpdate"
read_endpoint = "/read"
read_schema = "ReadSchema"
delete_endpoint = "/delete"
delete_schema = "DeleteSchema"

[identity]
id_field = "id"
"#;
        let spec = ResourceSpec::from_toml(toml_str).expect("parse");

        let api_str = r#"
openapi: "3.0.0"
info: { title: Test, version: "1.0" }
paths:
  /create:
    post:
      operationId: create
      responses:
        "200": { description: ok }
components:
  schemas:
    CreateSchema:
      type: object
      properties: {}
    ReadSchema:
      type: object
      properties: {}
    DeleteSchema:
      type: object
      properties: {}
"#;
        let api = openapi_forge::Spec::from_str(api_str).expect("parse");
        let result = spec.validate(&api);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("MissingUpdate"));
    }

    #[test]
    fn resource_spec_validate_missing_endpoint() {
        let toml_str = r#"
[resource]
name = "no_endpoint"
description = "Missing endpoint"
category = "test"

[crud]
create_endpoint = "/nonexistent-create"
create_schema = "CreateSchema"
read_endpoint = "/read"
read_schema = "ReadSchema"
delete_endpoint = "/delete"
delete_schema = "DeleteSchema"

[identity]
id_field = "id"
"#;
        let spec = ResourceSpec::from_toml(toml_str).expect("parse");

        let api_str = r#"
openapi: "3.0.0"
info: { title: Test, version: "1.0" }
paths:
  /existing:
    post:
      operationId: existing
      responses:
        "200": { description: ok }
components:
  schemas:
    CreateSchema:
      type: object
      properties: {}
    ReadSchema:
      type: object
      properties: {}
    DeleteSchema:
      type: object
      properties: {}
"#;
        let api = openapi_forge::Spec::from_str(api_str).expect("parse");
        let result = spec.validate(&api);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("no_endpoint"));
        assert!(err.contains("/nonexistent-create"));
    }

    #[test]
    fn resource_spec_validate_with_no_update_schema() {
        // When update_schema is None, validation should pass without checking it
        let toml_str = r#"
[resource]
name = "no_update"
description = "No update schema"
category = "test"

[crud]
create_endpoint = "/create"
create_schema = "CreateSchema"
read_endpoint = "/read"
read_schema = "ReadSchema"
delete_endpoint = "/delete"
delete_schema = "DeleteSchema"

[identity]
id_field = "id"
"#;
        let spec = ResourceSpec::from_toml(toml_str).expect("parse");

        let api_str = r#"
openapi: "3.0.0"
info: { title: Test, version: "1.0" }
paths:
  /create:
    post:
      operationId: create
      responses:
        "200": { description: ok }
components:
  schemas:
    CreateSchema:
      type: object
      properties: {}
    ReadSchema:
      type: object
      properties: {}
    DeleteSchema:
      type: object
      properties: {}
"#;
        let api = openapi_forge::Spec::from_str(api_str).expect("parse");
        assert!(spec.validate(&api).is_ok());
    }

    #[test]
    fn resource_spec_validate_with_read_response_schema() {
        let toml_str = r#"
[resource]
name = "with_resp"
description = "With response schema"
category = "test"

[crud]
create_endpoint = "/create"
create_schema = "CreateSchema"
read_endpoint = "/read"
read_schema = "ReadSchema"
read_response_schema = "ReadResponse"
delete_endpoint = "/delete"
delete_schema = "DeleteSchema"

[identity]
id_field = "id"
"#;
        let spec = ResourceSpec::from_toml(toml_str).expect("parse");

        let api_str = r#"
openapi: "3.0.0"
info: { title: Test, version: "1.0" }
paths:
  /create:
    post:
      operationId: create
      responses:
        "200": { description: ok }
components:
  schemas:
    CreateSchema:
      type: object
      properties: {}
    ReadSchema:
      type: object
      properties: {}
    ReadResponse:
      type: object
      properties: {}
    DeleteSchema:
      type: object
      properties: {}
"#;
        let api = openapi_forge::Spec::from_str(api_str).expect("parse");
        assert!(spec.validate(&api).is_ok());
    }

    #[test]
    fn resource_spec_validate_missing_read_response_schema() {
        let toml_str = r#"
[resource]
name = "bad_resp"
description = "Missing response schema"
category = "test"

[crud]
create_endpoint = "/create"
create_schema = "CreateSchema"
read_endpoint = "/read"
read_schema = "ReadSchema"
read_response_schema = "MissingResponse"
delete_endpoint = "/delete"
delete_schema = "DeleteSchema"

[identity]
id_field = "id"
"#;
        let spec = ResourceSpec::from_toml(toml_str).expect("parse");

        let api_str = r#"
openapi: "3.0.0"
info: { title: Test, version: "1.0" }
paths:
  /create:
    post:
      operationId: create
      responses:
        "200": { description: ok }
components:
  schemas:
    CreateSchema:
      type: object
      properties: {}
    ReadSchema:
      type: object
      properties: {}
    DeleteSchema:
      type: object
      properties: {}
"#;
        let api = openapi_forge::Spec::from_str(api_str).expect("parse");
        let result = spec.validate(&api);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("MissingResponse"));
    }

    #[test]
    fn data_source_spec_validate_all_present() {
        let toml_str = r#"
[data_source]
name = "valid_ds"
description = "Valid data source"

[read]
endpoint = "/read-ds"
schema = "ReadDsSchema"
"#;
        let spec = DataSourceSpec::from_toml(toml_str).expect("parse");

        let api_str = r#"
openapi: "3.0.0"
info: { title: Test, version: "1.0" }
paths:
  /read-ds:
    post:
      operationId: readDs
      responses:
        "200": { description: ok }
components:
  schemas:
    ReadDsSchema:
      type: object
      properties:
        id: { type: string }
"#;
        let api = openapi_forge::Spec::from_str(api_str).expect("parse");
        assert!(spec.validate(&api).is_ok());
    }

    #[test]
    fn data_source_spec_validate_missing_schema() {
        let toml_str = r#"
[data_source]
name = "bad_ds"
description = "Bad data source"

[read]
endpoint = "/read-ds"
schema = "MissingSchema"
"#;
        let spec = DataSourceSpec::from_toml(toml_str).expect("parse");

        let api_str = r#"
openapi: "3.0.0"
info: { title: Test, version: "1.0" }
paths:
  /read-ds:
    post:
      operationId: readDs
      responses:
        "200": { description: ok }
components:
  schemas: {}
"#;
        let api = openapi_forge::Spec::from_str(api_str).expect("parse");
        let result = spec.validate(&api);
        assert!(result.is_err());
    }

    #[test]
    fn data_source_spec_validate_missing_endpoint() {
        let toml_str = r#"
[data_source]
name = "no_ep_ds"
description = "No endpoint"

[read]
endpoint = "/nonexistent"
schema = "DsSchema"
"#;
        let spec = DataSourceSpec::from_toml(toml_str).expect("parse");

        let api_str = r#"
openapi: "3.0.0"
info: { title: Test, version: "1.0" }
paths:
  /other:
    post:
      operationId: other
      responses:
        "200": { description: ok }
components:
  schemas:
    DsSchema:
      type: object
      properties: {}
"#;
        let api = openapi_forge::Spec::from_str(api_str).expect("parse");
        let result = spec.validate(&api);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("no_ep_ds"));
        assert!(err.contains("/nonexistent"));
    }

    #[test]
    fn data_source_spec_validate_missing_response_schema() {
        let toml_str = r#"
[data_source]
name = "resp_ds"
description = "Missing response schema"

[read]
endpoint = "/read-ds"
schema = "DsSchema"
response_schema = "MissingResp"
"#;
        let spec = DataSourceSpec::from_toml(toml_str).expect("parse");

        let api_str = r#"
openapi: "3.0.0"
info: { title: Test, version: "1.0" }
paths:
  /read-ds:
    post:
      operationId: readDs
      responses:
        "200": { description: ok }
components:
  schemas:
    DsSchema:
      type: object
      properties: {}
"#;
        let api = openapi_forge::Spec::from_str(api_str).expect("parse");
        let result = spec.validate(&api);
        assert!(result.is_err());
    }

    #[test]
    fn resource_spec_with_all_field_overrides() {
        let toml_str = r#"
[resource]
name = "full_overrides"
description = "All field overrides"
category = "test"

[crud]
create_endpoint = "/create"
create_schema = "Create"
read_endpoint = "/read"
read_schema = "Read"
delete_endpoint = "/delete"
delete_schema = "Delete"

[identity]
id_field = "id"

[fields]
skip_me = { skip = true }
computed_field = { computed = true }
sensitive_field = { sensitive = true }
typed_field = { type_override = "bool" }
described_field = { description = "Custom desc" }
force_new_field = { force_new = true }
"#;
        let spec = ResourceSpec::from_toml(toml_str).expect("parse");
        assert!(spec.fields.get("skip_me").unwrap().skip);
        assert!(spec.fields.get("computed_field").unwrap().computed);
        assert!(spec.fields.get("sensitive_field").unwrap().sensitive);
        assert_eq!(
            spec.fields.get("typed_field").unwrap().type_override,
            Some("bool".to_string())
        );
        assert_eq!(
            spec.fields.get("described_field").unwrap().description,
            Some("Custom desc".to_string())
        );
        assert!(spec.fields.get("force_new_field").unwrap().force_new);
    }

    #[test]
    fn field_override_defaults() {
        let fo = FieldOverride::default();
        assert!(!fo.computed);
        assert!(!fo.sensitive);
        assert!(!fo.skip);
        assert!(fo.type_override.is_none());
        assert!(fo.description.is_none());
        assert!(!fo.force_new);
    }

    #[test]
    fn provider_defaults_default() {
        let pd = ProviderDefaults::default();
        assert!(pd.skip_fields.is_empty());
    }

    #[test]
    fn auth_config_default() {
        let ac = AuthConfig::default();
        assert!(ac.token_field.is_empty());
        assert!(ac.env_var.is_empty());
        assert!(ac.gateway_url_field.is_empty());
        assert!(ac.gateway_env_var.is_empty());
    }

    #[test]
    fn resource_spec_empty_read_mapping() {
        let toml_str = r#"
[resource]
name = "no_mapping"
description = "No read mapping"
category = "test"

[crud]
create_endpoint = "/create"
create_schema = "Create"
read_endpoint = "/read"
read_schema = "Read"
delete_endpoint = "/delete"
delete_schema = "Delete"

[identity]
id_field = "id"
"#;
        let spec = ResourceSpec::from_toml(toml_str).expect("parse");
        assert!(spec.read_mapping.is_empty());
    }

    #[test]
    fn data_source_spec_empty_fields_and_mapping() {
        let toml_str = r#"
[data_source]
name = "minimal"
description = "Minimal"

[read]
endpoint = "/read"
schema = "Read"
"#;
        let spec = DataSourceSpec::from_toml(toml_str).expect("parse");
        assert!(spec.fields.is_empty());
        assert!(spec.read_mapping.is_empty());
    }

    #[test]
    fn provider_spec_minimal() {
        // Provider with only name, everything else defaults
        let toml_str = r#"
[provider]
name = "minimal"
"#;
        let spec = ProviderSpec::from_toml(toml_str).expect("parse");
        assert_eq!(spec.provider.name, "minimal");
        assert!(spec.provider.description.is_empty());
        assert!(spec.provider.version.is_empty());
        assert!(spec.provider.sdk_import.is_empty());
        assert!(spec.auth.token_field.is_empty());
        assert!(spec.defaults.skip_fields.is_empty());
        assert!(spec.platforms.is_empty());
    }

    #[test]
    fn resource_spec_validate_missing_read_schema() {
        let toml_str = r#"
[resource]
name = "bad_read"
description = "Missing read schema"
category = "test"

[crud]
create_endpoint = "/create"
create_schema = "CreateSchema"
read_endpoint = "/read"
read_schema = "MissingReadSchema"
delete_endpoint = "/delete"
delete_schema = "DeleteSchema"

[identity]
id_field = "id"
"#;
        let spec = ResourceSpec::from_toml(toml_str).expect("parse");
        let api_str = r#"
openapi: "3.0.0"
info: { title: Test, version: "1.0" }
paths:
  /create:
    post:
      operationId: create
      responses:
        "200": { description: ok }
components:
  schemas:
    CreateSchema:
      type: object
      properties: {}
    DeleteSchema:
      type: object
      properties: {}
"#;
        let api = openapi_forge::Spec::from_str(api_str).expect("parse");
        let result = spec.validate(&api);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("MissingReadSchema"), "got: {err}");
    }

    #[test]
    fn resource_spec_validate_missing_delete_schema() {
        let toml_str = r#"
[resource]
name = "bad_delete"
description = "Missing delete schema"
category = "test"

[crud]
create_endpoint = "/create"
create_schema = "CreateSchema"
read_endpoint = "/read"
read_schema = "ReadSchema"
delete_endpoint = "/delete"
delete_schema = "MissingDeleteSchema"

[identity]
id_field = "id"
"#;
        let spec = ResourceSpec::from_toml(toml_str).expect("parse");
        let api_str = r#"
openapi: "3.0.0"
info: { title: Test, version: "1.0" }
paths:
  /create:
    post:
      operationId: create
      responses:
        "200": { description: ok }
components:
  schemas:
    CreateSchema:
      type: object
      properties: {}
    ReadSchema:
      type: object
      properties: {}
"#;
        let api = openapi_forge::Spec::from_str(api_str).expect("parse");
        let result = spec.validate(&api);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("MissingDeleteSchema"), "got: {err}");
    }

    #[test]
    fn config_loader_load_valid_file() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let path = dir.path().join("resource.toml");
        std::fs::write(
            &path,
            r#"
[resource]
name = "from_file"
description = "Loaded from file"
category = "test"

[crud]
create_endpoint = "/create"
create_schema = "Create"
read_endpoint = "/read"
read_schema = "Read"
delete_endpoint = "/delete"
delete_schema = "Delete"

[identity]
id_field = "id"
"#,
        )
        .expect("write");
        let spec = ResourceSpec::load(&path).expect("load");
        assert_eq!(spec.resource.name, "from_file");
    }

    #[test]
    fn config_loader_load_valid_provider_file() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let path = dir.path().join("provider.toml");
        std::fs::write(
            &path,
            r#"
[provider]
name = "file_provider"
description = "From file"
version = "2.0.0"
"#,
        )
        .expect("write");
        let spec = ProviderSpec::load(&path).expect("load");
        assert_eq!(spec.provider.name, "file_provider");
        assert_eq!(spec.provider.version, "2.0.0");
    }

    #[test]
    fn config_loader_load_valid_data_source_file() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let path = dir.path().join("ds.toml");
        std::fs::write(
            &path,
            r#"
[data_source]
name = "file_ds"
description = "From file"

[read]
endpoint = "/read"
schema = "ReadDs"
"#,
        )
        .expect("write");
        let spec = DataSourceSpec::load(&path).expect("load");
        assert_eq!(spec.data_source.name, "file_ds");
    }

    #[test]
    fn config_loader_load_invalid_toml_content() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let path = dir.path().join("bad.toml");
        std::fs::write(&path, "this is not valid toml {{{").expect("write");
        let result = ResourceSpec::load(&path);
        assert!(result.is_err());
    }

    #[test]
    fn resource_spec_serde_roundtrip() {
        let toml_str = r#"
[resource]
name = "roundtrip_res"
description = "Roundtrip test"
category = "test"

[crud]
create_endpoint = "/create"
create_schema = "Create"
update_endpoint = "/update"
update_schema = "Update"
read_endpoint = "/read"
read_schema = "Read"
read_response_schema = "ReadResp"
delete_endpoint = "/delete"
delete_schema = "Delete"

[identity]
id_field = "name"
import_field = "path"
force_new_fields = ["name"]

[fields]
secret = { sensitive = true, computed = true }

[read_mapping]
"resp_key" = "name"
"#;
        let spec = ResourceSpec::from_toml(toml_str).expect("parse");
        let json = serde_json::to_string(&spec).expect("serialize");
        let rt: ResourceSpec = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(rt.resource.name, "roundtrip_res");
        assert_eq!(rt.crud.update_endpoint, Some("/update".to_string()));
        assert_eq!(rt.identity.import_field, Some("path".to_string()));
        assert!(rt.fields.get("secret").unwrap().sensitive);
        assert!(rt.fields.get("secret").unwrap().computed);
        assert_eq!(rt.read_mapping.get("resp_key"), Some(&"name".to_string()));
    }

    #[test]
    fn data_source_spec_serde_roundtrip() {
        let toml_str = r#"
[data_source]
name = "roundtrip_ds"
description = "Roundtrip test"

[read]
endpoint = "/read"
schema = "ReadSchema"
response_schema = "ReadResp"

[fields]
token = { skip = true }

[read_mapping]
"resp_val" = "value"
"#;
        let spec = DataSourceSpec::from_toml(toml_str).expect("parse");
        let json = serde_json::to_string(&spec).expect("serialize");
        let rt: DataSourceSpec = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(rt.data_source.name, "roundtrip_ds");
        assert_eq!(rt.read.response_schema, Some("ReadResp".to_string()));
        assert!(rt.fields.get("token").unwrap().skip);
    }

    #[test]
    fn data_source_spec_validate_with_response_schema_present() {
        let toml_str = r#"
[data_source]
name = "with_resp"
description = "Has response schema"

[read]
endpoint = "/read-ds"
schema = "DsSchema"
response_schema = "DsResponse"
"#;
        let spec = DataSourceSpec::from_toml(toml_str).expect("parse");
        let api_str = r#"
openapi: "3.0.0"
info: { title: Test, version: "1.0" }
paths:
  /read-ds:
    post:
      operationId: readDs
      responses:
        "200": { description: ok }
components:
  schemas:
    DsSchema:
      type: object
      properties: {}
    DsResponse:
      type: object
      properties: {}
"#;
        let api = openapi_forge::Spec::from_str(api_str).expect("parse");
        assert!(spec.validate(&api).is_ok());
    }

    #[test]
    fn resource_spec_with_multiple_read_mappings() {
        let toml_str = r#"
[resource]
name = "multi_mapping"
description = "Multiple mappings"
category = "test"

[crud]
create_endpoint = "/create"
create_schema = "Create"
read_endpoint = "/read"
read_schema = "Read"
delete_endpoint = "/delete"
delete_schema = "Delete"

[identity]
id_field = "id"

[read_mapping]
"response_name" = "name"
"response_value" = "value"
"response_count" = "count"
"#;
        let spec = ResourceSpec::from_toml(toml_str).expect("parse");
        assert_eq!(spec.read_mapping.len(), 3);
        assert_eq!(spec.read_mapping.get("response_name"), Some(&"name".to_string()));
        assert_eq!(spec.read_mapping.get("response_value"), Some(&"value".to_string()));
        assert_eq!(spec.read_mapping.get("response_count"), Some(&"count".to_string()));
    }

    #[test]
    fn provider_spec_serde_roundtrip() {
        let toml_str = r#"
[provider]
name = "roundtrip"
description = "Roundtrip test"
version = "2.0.0"
sdk_import = "github.com/example/sdk"

[auth]
token_field = "token"
env_var = "TOKEN"
gateway_url_field = "gw"
gateway_env_var = "GW"

[defaults]
skip_fields = ["token", "uid-token"]

[platforms.terraform]
sdk_import = "github.com/example/tf-sdk"
"#;
        let spec = ProviderSpec::from_toml(toml_str).expect("parse");
        let json = serde_json::to_string(&spec).expect("serialize");
        let rt: ProviderSpec = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(rt.provider.name, "roundtrip");
        assert_eq!(rt.provider.version, "2.0.0");
        assert_eq!(rt.auth.token_field, "token");
        assert_eq!(rt.auth.gateway_url_field, "gw");
        assert_eq!(rt.defaults.skip_fields.len(), 2);
        assert!(rt.platforms.contains_key("terraform"));
    }

    #[test]
    fn field_override_serde_roundtrip() {
        let fo = FieldOverride {
            computed: true,
            sensitive: true,
            skip: false,
            type_override: Some("bool".to_string()),
            description: Some("custom".to_string()),
            force_new: true,
        };
        let json = serde_json::to_string(&fo).expect("serialize");
        let rt: FieldOverride = serde_json::from_str(&json).expect("deserialize");
        assert!(rt.computed);
        assert!(rt.sensitive);
        assert!(!rt.skip);
        assert_eq!(rt.type_override, Some("bool".to_string()));
        assert_eq!(rt.description, Some("custom".to_string()));
        assert!(rt.force_new);
    }

    #[test]
    fn identity_config_serde_roundtrip() {
        let ic = IdentityConfig {
            id_field: "name".to_string(),
            import_field: Some("path".to_string()),
            force_new_fields: vec!["name".to_string()],
        };
        let json = serde_json::to_string(&ic).expect("serialize");
        let rt: IdentityConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(rt.id_field, "name");
        assert_eq!(rt.import_field, Some("path".to_string()));
        assert_eq!(rt.force_new_fields, vec!["name"]);
    }

    #[test]
    fn crud_mapping_serde_roundtrip() {
        let cm = CrudMapping {
            create_endpoint: "/create".to_string(),
            create_schema: "Create".to_string(),
            update_endpoint: Some("/update".to_string()),
            update_schema: Some("Update".to_string()),
            read_endpoint: "/read".to_string(),
            read_schema: "Read".to_string(),
            read_response_schema: Some("ReadResp".to_string()),
            delete_endpoint: "/delete".to_string(),
            delete_schema: "Delete".to_string(),
        };
        let json = serde_json::to_string(&cm).expect("serialize");
        let rt: CrudMapping = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(rt.create_endpoint, "/create");
        assert_eq!(rt.update_endpoint, Some("/update".to_string()));
        assert_eq!(rt.read_response_schema, Some("ReadResp".to_string()));
    }

    #[test]
    fn read_mapping_serde_roundtrip() {
        let rm = ReadMapping {
            endpoint: "/read".to_string(),
            schema: "ReadSchema".to_string(),
            response_schema: Some("ReadResp".to_string()),
        };
        let json = serde_json::to_string(&rm).expect("serialize");
        let rt: ReadMapping = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(rt.endpoint, "/read");
        assert_eq!(rt.schema, "ReadSchema");
        assert_eq!(rt.response_schema, Some("ReadResp".to_string()));
    }
}
