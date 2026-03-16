use std::collections::HashSet;

use openapi_forge::Spec;

use crate::error::IacForgeError;
use crate::ir::{
    AuthInfo, CrudInfo, IacAttribute, IacDataSource, IacProvider, IacResource, IdentityInfo,
};
use crate::naming::to_snake_case;
use crate::spec::{DataSourceSpec, ProviderDefaults, ProviderSpec, ResourceSpec};
use crate::type_map::{apply_enum_constraint, openapi_to_iac};

/// Resolve a resource spec + OpenAPI spec into a platform-independent `IacResource`.
///
/// This is the core resolution step: it reads OpenAPI schema fields, applies
/// TOML overrides (skip, computed, sensitive, type_override, force_new), and
/// produces a fully resolved IR ready for any backend.
///
/// # Errors
///
/// Returns an error if referenced schemas are missing from the OpenAPI spec.
pub fn resolve_resource(
    resource: &ResourceSpec,
    api: &Spec,
    defaults: &ProviderDefaults,
) -> Result<IacResource, IacForgeError> {
    let create_fields = api.fields(&resource.crud.create_schema)?;

    let update_required: HashSet<String> = if let Some(ref update_schema) =
        resource.crud.update_schema
    {
        api.fields(update_schema)
            .unwrap_or_default()
            .iter()
            .filter(|f| f.required)
            .map(|f| f.name.clone())
            .collect()
    } else {
        HashSet::new()
    };

    let skip_fields: HashSet<&str> = defaults.skip_fields.iter().map(String::as_str).collect();

    // Build reverse read_mapping: tf_name -> json_path
    let reverse_mapping: std::collections::HashMap<String, String> = resource
        .read_mapping
        .iter()
        .map(|(json_path, tf_name)| (to_snake_case(tf_name), json_path.clone()))
        .collect();

    let mut attributes = Vec::new();

    for field in &create_fields {
        let canonical = to_snake_case(&field.name);

        if skip_fields.contains(field.name.as_str()) {
            continue;
        }

        let override_cfg = resource.fields.get(&field.name);
        if override_cfg.is_some_and(|o| o.skip) {
            continue;
        }

        let computed = override_cfg.is_some_and(|o| o.computed);
        let sensitive = override_cfg.is_some_and(|o| o.sensitive);
        let immutable = override_cfg.is_some_and(|o| o.force_new)
            || resource.identity.force_new_fields.contains(&field.name);

        let type_override = override_cfg.and_then(|o| o.type_override.as_deref());
        let iac_type = openapi_to_iac(&field.type_info, type_override);
        let iac_type = apply_enum_constraint(iac_type, &field.enum_values);

        let is_create_required = field.required;
        let is_update_required = update_required.contains(&field.name);
        let required = if computed { false } else { is_create_required };

        // A field is update_only if it appears in the update schema as required
        // but is not required in the create schema.
        let update_only = is_update_required && !is_create_required;

        let description = override_cfg
            .and_then(|o| o.description.clone())
            .or_else(|| field.description.clone())
            .unwrap_or_default();

        let read_path = reverse_mapping.get(&canonical).cloned();

        attributes.push(IacAttribute {
            api_name: field.name.clone(),
            canonical_name: canonical,
            description,
            iac_type,
            required,
            computed,
            sensitive,
            immutable,
            default_value: field.default.clone(),
            enum_values: field.enum_values.clone(),
            read_path,
            update_only,
        });
    }

    Ok(IacResource {
        name: resource.resource.name.clone(),
        description: resource.resource.description.clone(),
        category: resource.resource.category.clone(),
        crud: CrudInfo {
            create_endpoint: resource.crud.create_endpoint.clone(),
            create_schema: resource.crud.create_schema.clone(),
            update_endpoint: resource.crud.update_endpoint.clone(),
            update_schema: resource.crud.update_schema.clone(),
            read_endpoint: resource.crud.read_endpoint.clone(),
            read_schema: resource.crud.read_schema.clone(),
            read_response_schema: resource.crud.read_response_schema.clone(),
            delete_endpoint: resource.crud.delete_endpoint.clone(),
            delete_schema: resource.crud.delete_schema.clone(),
        },
        attributes,
        identity: IdentityInfo {
            id_field: resource.identity.id_field.clone(),
            import_field: resource
                .identity
                .import_field
                .clone()
                .unwrap_or_else(|| resource.identity.id_field.clone()),
            force_replace_fields: resource.identity.force_new_fields.clone(),
        },
    })
}

/// Resolve a data source spec + OpenAPI spec into a platform-independent `IacDataSource`.
///
/// # Errors
///
/// Returns an error if referenced schemas are missing from the OpenAPI spec.
pub fn resolve_data_source(
    ds: &DataSourceSpec,
    api: &Spec,
    defaults: &ProviderDefaults,
) -> Result<IacDataSource, IacForgeError> {
    let read_fields = api.fields(&ds.read.schema)?;

    let skip_fields: HashSet<&str> = defaults.skip_fields.iter().map(String::as_str).collect();

    let reverse_mapping: std::collections::HashMap<String, String> = ds
        .read_mapping
        .iter()
        .map(|(json_path, tf_name)| (to_snake_case(tf_name), json_path.clone()))
        .collect();

    let mut attributes = Vec::new();

    for field in &read_fields {
        let canonical = to_snake_case(&field.name);

        if skip_fields.contains(field.name.as_str()) {
            continue;
        }

        let override_cfg = ds.fields.get(&field.name);
        if override_cfg.is_some_and(|o| o.skip) {
            continue;
        }

        let computed = override_cfg.is_some_and(|o| o.computed);
        let sensitive = override_cfg.is_some_and(|o| o.sensitive);

        let type_override = override_cfg.and_then(|o| o.type_override.as_deref());
        let iac_type = openapi_to_iac(&field.type_info, type_override);
        let iac_type = apply_enum_constraint(iac_type, &field.enum_values);

        let description = override_cfg
            .and_then(|o| o.description.clone())
            .or_else(|| field.description.clone())
            .unwrap_or_default();

        let read_path = reverse_mapping.get(&canonical).cloned();

        attributes.push(IacAttribute {
            api_name: field.name.clone(),
            canonical_name: canonical,
            description,
            iac_type,
            required: false,
            computed: computed || !field.required,
            sensitive,
            immutable: false,
            default_value: field.default.clone(),
            enum_values: field.enum_values.clone(),
            read_path,
            update_only: false,
        });
    }

    Ok(IacDataSource {
        name: ds.data_source.name.clone(),
        description: ds.data_source.description.clone(),
        read_endpoint: ds.read.endpoint.clone(),
        read_schema: ds.read.schema.clone(),
        read_response_schema: ds.read.response_schema.clone(),
        attributes,
    })
}

/// Resolve a provider spec into a platform-independent `IacProvider`.
#[must_use]
pub fn resolve_provider(provider: &ProviderSpec) -> IacProvider {
    IacProvider {
        name: provider.provider.name.clone(),
        description: provider.provider.description.clone(),
        version: provider.provider.version.clone(),
        auth: AuthInfo {
            token_field: provider.auth.token_field.clone(),
            env_var: provider.auth.env_var.clone(),
            gateway_url_field: provider.auth.gateway_url_field.clone(),
            gateway_env_var: provider.auth.gateway_env_var.clone(),
        },
        skip_fields: provider.defaults.skip_fields.clone(),
        platform_config: provider.platforms.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_spec() -> (ResourceSpec, Spec) {
        let toml_str = r#"
[resource]
name = "akeyless_static_secret"
description = "Static secret"
category = "secret"

[crud]
create_endpoint = "/create-secret"
create_schema = "CreateSecret"
read_endpoint = "/get-secret-value"
read_schema = "GetSecretValue"
delete_endpoint = "/delete-item"
delete_schema = "DeleteItem"

[identity]
id_field = "name"
force_new_fields = ["name"]

[fields]
token = { skip = true }
delete_protection = { type_override = "bool" }

[read_mapping]
"item_name" = "name"
"item_tags" = "tags"
"#;
        let resource: ResourceSpec = toml::from_str(toml_str).expect("parse resource");

        let api_str = r#"
openapi: "3.0.0"
info:
  title: Test
  version: "1.0"
paths:
  /create-secret:
    post:
      operationId: createSecret
      requestBody:
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/CreateSecret'
      responses:
        "200":
          description: ok
  /get-secret-value:
    post:
      operationId: getSecretValue
      responses:
        "200":
          description: ok
  /delete-item:
    post:
      operationId: deleteItem
      responses:
        "200":
          description: ok
components:
  schemas:
    CreateSecret:
      type: object
      required:
        - name
        - value
      properties:
        name:
          type: string
          description: Secret name
        value:
          type: string
          description: Secret value
        tags:
          type: array
          items:
            type: string
        token:
          type: string
        delete_protection:
          type: string
    GetSecretValue:
      type: object
      properties:
        names:
          type: array
          items:
            type: string
    DeleteItem:
      type: object
      properties:
        name:
          type: string
"#;
        let api = Spec::from_str(api_str).expect("parse api");
        (resource, api)
    }

    #[test]
    fn resolve_resource_basic() {
        let (resource, api) = make_test_spec();
        let defaults = ProviderDefaults::default();
        let iac = resolve_resource(&resource, &api, &defaults).expect("resolve");

        assert_eq!(iac.name, "akeyless_static_secret");
        assert_eq!(iac.category, "secret");
        assert_eq!(iac.identity.id_field, "name");
        assert_eq!(iac.identity.force_replace_fields, vec!["name"]);
    }

    #[test]
    fn resolve_resource_skips_token() {
        let (resource, api) = make_test_spec();
        let defaults = ProviderDefaults::default();
        let iac = resolve_resource(&resource, &api, &defaults).expect("resolve");

        assert!(iac.attributes.iter().all(|a| a.api_name != "token"));
    }

    #[test]
    fn resolve_resource_type_override() {
        let (resource, api) = make_test_spec();
        let defaults = ProviderDefaults::default();
        let iac = resolve_resource(&resource, &api, &defaults).expect("resolve");

        let dp = iac
            .attributes
            .iter()
            .find(|a| a.canonical_name == "delete_protection")
            .expect("dp");
        assert_eq!(dp.iac_type, crate::ir::IacType::Boolean);
    }

    #[test]
    fn resolve_resource_required_fields() {
        let (resource, api) = make_test_spec();
        let defaults = ProviderDefaults::default();
        let iac = resolve_resource(&resource, &api, &defaults).expect("resolve");

        let name = iac
            .attributes
            .iter()
            .find(|a| a.canonical_name == "name")
            .expect("name");
        assert!(name.required);
        assert!(name.immutable);
    }

    #[test]
    fn resolve_resource_read_mapping() {
        let (resource, api) = make_test_spec();
        let defaults = ProviderDefaults::default();
        let iac = resolve_resource(&resource, &api, &defaults).expect("resolve");

        let name = iac
            .attributes
            .iter()
            .find(|a| a.canonical_name == "name")
            .expect("name");
        assert_eq!(name.read_path, Some("item_name".to_string()));

        let tags = iac
            .attributes
            .iter()
            .find(|a| a.canonical_name == "tags")
            .expect("tags");
        assert_eq!(tags.read_path, Some("item_tags".to_string()));
    }

    #[test]
    fn resolve_resource_list_type() {
        let (resource, api) = make_test_spec();
        let defaults = ProviderDefaults::default();
        let iac = resolve_resource(&resource, &api, &defaults).expect("resolve");

        let tags = iac
            .attributes
            .iter()
            .find(|a| a.canonical_name == "tags")
            .expect("tags");
        assert_eq!(
            tags.iac_type,
            crate::ir::IacType::List(Box::new(crate::ir::IacType::String))
        );
    }

    #[test]
    fn resolve_resource_global_skip() {
        let (resource, api) = make_test_spec();
        let defaults = ProviderDefaults {
            skip_fields: vec!["value".to_string()],
        };
        let iac = resolve_resource(&resource, &api, &defaults).expect("resolve");

        assert!(iac.attributes.iter().all(|a| a.api_name != "value"));
    }

    #[test]
    fn resolve_resource_update_only() {
        let (resource, api) = make_test_spec();
        let defaults = ProviderDefaults::default();
        let iac = resolve_resource(&resource, &api, &defaults).expect("resolve");

        // Without an update schema, all fields should have update_only = false
        for attr in &iac.attributes {
            assert!(!attr.update_only, "field {} should not be update_only", attr.canonical_name);
        }
    }

    #[test]
    fn resolve_provider_basic() {
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
skip_fields = ["token", "uid-token"]
"#;
        let provider: ProviderSpec = toml::from_str(toml_str).expect("parse");
        let iac = resolve_provider(&provider);

        assert_eq!(iac.name, "akeyless");
        assert_eq!(iac.auth.env_var, "AKEYLESS_ACCESS_TOKEN");
        assert_eq!(iac.skip_fields.len(), 2);
    }

    #[test]
    fn resolve_data_source_basic() {
        let toml_str = r#"
[data_source]
name = "akeyless_auth_method"
description = "Read an auth method"

[read]
endpoint = "/get-auth-method"
schema = "GetAuthMethod"

[fields]
token = { skip = true }

[read_mapping]
"auth_method_access_id" = "access_id"
"#;
        let ds: DataSourceSpec = toml::from_str(toml_str).expect("parse");

        let api_str = r#"
openapi: "3.0.0"
info: { title: Test, version: "1.0" }
paths:
  /get-auth-method:
    post:
      operationId: getAuthMethod
      requestBody:
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/GetAuthMethod'
      responses:
        "200": { description: ok }
components:
  schemas:
    GetAuthMethod:
      type: object
      required: [name]
      properties:
        name: { type: string, description: "Auth method name" }
        access_id: { type: string, description: "Access ID" }
        token: { type: string }
"#;
        let api = Spec::from_str(api_str).expect("parse");
        let defaults = ProviderDefaults::default();
        let iac = resolve_data_source(&ds, &api, &defaults).expect("resolve");

        assert_eq!(iac.name, "akeyless_auth_method");
        assert!(iac.attributes.iter().all(|a| a.api_name != "token"));

        let access = iac
            .attributes
            .iter()
            .find(|a| a.canonical_name == "access_id")
            .expect("access_id");
        assert!(access.computed);
        assert_eq!(
            access.read_path,
            Some("auth_method_access_id".to_string())
        );
    }

    #[test]
    fn resolve_data_source_update_only_always_false() {
        let toml_str = r#"
[data_source]
name = "test_ds"
description = "Test"

[read]
endpoint = "/read"
schema = "ReadSchema"
"#;
        let ds: DataSourceSpec = toml::from_str(toml_str).expect("parse");

        let api_str = r#"
openapi: "3.0.0"
info: { title: Test, version: "1.0" }
paths:
  /read:
    post:
      operationId: read
      responses:
        "200": { description: ok }
components:
  schemas:
    ReadSchema:
      type: object
      properties:
        name: { type: string }
"#;
        let api = Spec::from_str(api_str).expect("parse");
        let defaults = ProviderDefaults::default();
        let iac = resolve_data_source(&ds, &api, &defaults).expect("resolve");

        for attr in &iac.attributes {
            assert!(!attr.update_only);
        }
    }

    #[test]
    fn resolve_resource_missing_schema() {
        let toml_str = r#"
[resource]
name = "test"
description = "Test"
category = "test"

[crud]
create_endpoint = "/create"
create_schema = "NonExistentSchema"
read_endpoint = "/read"
read_schema = "Read"
delete_endpoint = "/delete"
delete_schema = "Delete"

[identity]
id_field = "id"
"#;
        let resource: ResourceSpec = toml::from_str(toml_str).expect("parse");

        let api_str = r#"
openapi: "3.0.0"
info: { title: Test, version: "1.0" }
paths: {}
components:
  schemas:
    Read:
      type: object
      properties:
        id: { type: string }
    Delete:
      type: object
      properties:
        id: { type: string }
"#;
        let api = Spec::from_str(api_str).expect("parse");
        let defaults = ProviderDefaults::default();
        let result = resolve_resource(&resource, &api, &defaults);

        assert!(result.is_err(), "should fail when create_schema is missing");
    }

    #[test]
    fn resolve_resource_empty_fields() {
        let toml_str = r#"
[resource]
name = "empty_resource"
description = "Has no fields"
category = "test"

[crud]
create_endpoint = "/create"
create_schema = "EmptySchema"
read_endpoint = "/read"
read_schema = "Read"
delete_endpoint = "/delete"
delete_schema = "Delete"

[identity]
id_field = "id"
"#;
        let resource: ResourceSpec = toml::from_str(toml_str).expect("parse");

        let api_str = r#"
openapi: "3.0.0"
info: { title: Test, version: "1.0" }
paths: {}
components:
  schemas:
    EmptySchema:
      type: object
      properties: {}
    Read:
      type: object
      properties: {}
    Delete:
      type: object
      properties: {}
"#;
        let api = Spec::from_str(api_str).expect("parse");
        let defaults = ProviderDefaults::default();
        let iac = resolve_resource(&resource, &api, &defaults).expect("resolve");

        assert!(iac.attributes.is_empty(), "empty schema should produce zero attributes");
    }

    #[test]
    fn resolve_data_source_missing_schema() {
        let toml_str = r#"
[data_source]
name = "test_ds"
description = "Test"

[read]
endpoint = "/read"
schema = "NonExistentSchema"
"#;
        let ds: DataSourceSpec = toml::from_str(toml_str).expect("parse");

        let api_str = r#"
openapi: "3.0.0"
info: { title: Test, version: "1.0" }
paths: {}
components:
  schemas: {}
"#;
        let api = Spec::from_str(api_str).expect("parse");
        let defaults = ProviderDefaults::default();
        let result = resolve_data_source(&ds, &api, &defaults);

        assert!(result.is_err(), "should fail when read schema is missing");
    }
}
