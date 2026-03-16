use std::collections::HashMap;
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
    pub resource: ResourceMeta,
    pub crud: CrudMapping,
    pub identity: IdentityConfig,
    #[serde(default)]
    pub fields: HashMap<String, FieldOverride>,
    #[serde(default)]
    pub read_mapping: HashMap<String, String>,
}

/// Resource metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceMeta {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub category: String,
}

/// Maps CRUD operations to API endpoints and schemas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrudMapping {
    pub create_endpoint: String,
    pub create_schema: String,
    #[serde(default)]
    pub update_endpoint: Option<String>,
    #[serde(default)]
    pub update_schema: Option<String>,
    pub read_endpoint: String,
    pub read_schema: String,
    #[serde(default)]
    pub read_response_schema: Option<String>,
    pub delete_endpoint: String,
    pub delete_schema: String,
}

/// Identity and import configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityConfig {
    pub id_field: String,
    #[serde(default)]
    pub import_field: Option<String>,
    #[serde(default)]
    pub force_new_fields: Vec<String>,
}

/// Per-field overrides in the resource spec.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FieldOverride {
    #[serde(default)]
    pub computed: bool,
    #[serde(default)]
    pub sensitive: bool,
    #[serde(default)]
    pub skip: bool,
    #[serde(default)]
    pub type_override: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub force_new: bool,
}

/// Provider-level configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSpec {
    pub provider: ProviderMeta,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub defaults: ProviderDefaults,
    #[serde(default)]
    pub platforms: HashMap<String, toml::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderMeta {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub sdk_import: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthConfig {
    #[serde(default)]
    pub token_field: String,
    #[serde(default)]
    pub env_var: String,
    #[serde(default)]
    pub gateway_url_field: String,
    #[serde(default)]
    pub gateway_env_var: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderDefaults {
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

    /// Validate the resource spec against an OpenAPI spec.
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
    pub data_source: DataSourceMeta,
    pub read: ReadMapping,
    #[serde(default)]
    pub fields: HashMap<String, FieldOverride>,
    #[serde(default)]
    pub read_mapping: HashMap<String, String>,
}

/// Data source metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSourceMeta {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

/// Maps a read operation to an API endpoint and schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadMapping {
    pub endpoint: String,
    pub schema: String,
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

    /// Validate the data source spec against an OpenAPI spec.
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
}
