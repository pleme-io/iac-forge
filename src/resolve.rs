use std::collections::{BTreeMap, HashSet};

use openapi_forge::{Field, Spec};

use crate::error::IacForgeError;
use crate::ir::{
    AuthInfo, CrudInfo, IacAttribute, IacDataSource, IacProvider, IacResource, IdentityInfo,
};
use crate::naming::to_snake_case;
use crate::spec::{DataSourceSpec, FieldOverride, ProviderDefaults, ProviderSpec, ResourceSpec};
use crate::type_map::{apply_enum_constraint, openapi_to_iac};

/// Build a single `IacAttribute` from an OpenAPI field with optional overrides.
///
/// Returns `None` if the field should be skipped (via override or global skip list).
///
/// Parameters:
/// - `field`: the OpenAPI field definition
/// - `override_cfg`: optional per-field override from the TOML spec
/// - `force_new_fields`: list of field names that force resource replacement
/// - `reverse_mapping`: maps canonical name -> API response read path
/// - `is_resource`: `true` for resources, `false` for data sources
/// - `update_only`: whether this field is update-only (only meaningful for resources)
#[allow(clippy::fn_params_excessive_bools)]
fn build_attribute(
    field: &Field,
    override_cfg: Option<&FieldOverride>,
    force_new_fields: &[String],
    reverse_mapping: &BTreeMap<String, String>,
    is_resource: bool,
    update_only: bool,
) -> Option<IacAttribute> {
    if override_cfg.is_some_and(|o| o.skip) {
        return None;
    }

    let canonical = to_snake_case(&field.name);

    let computed = override_cfg.is_some_and(|o| o.computed);
    let sensitive = override_cfg.is_some_and(|o| o.sensitive);

    let immutable = if is_resource {
        override_cfg.is_some_and(|o| o.force_new)
            || force_new_fields.contains(&field.name)
    } else {
        false
    };

    let type_override = override_cfg.and_then(|o| o.type_override.as_deref());
    let iac_type = openapi_to_iac(&field.type_info, type_override);
    let iac_type = apply_enum_constraint(iac_type, &field.enum_values);

    let (required, computed_final, update_only_final) = if is_resource {
        let req = if computed { false } else { field.required };
        (req, computed, update_only)
    } else {
        // Data sources: all fields are computed unless they're required inputs
        (false, computed || !field.required, false)
    };

    let description = override_cfg
        .and_then(|o| o.description.clone())
        .or_else(|| field.description.clone())
        .unwrap_or_default();

    let read_path = reverse_mapping.get(&canonical).cloned();

    Some(IacAttribute {
        api_name: field.name.clone(),
        canonical_name: canonical,
        description,
        iac_type,
        required,
        optional: !required,
        computed: computed_final,
        sensitive,
        json_encoded: false,
        immutable,
        default_value: field.default.clone(),
        enum_values: field.enum_values.clone(),
        read_path,
        update_only: update_only_final,
    })
}

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
    let reverse_mapping: BTreeMap<String, String> = resource
        .read_mapping
        .iter()
        .map(|(json_path, tf_name)| (to_snake_case(tf_name), json_path.clone()))
        .collect();

    let mut attributes = Vec::new();

    for field in &create_fields {
        if skip_fields.contains(field.name.as_str()) {
            continue;
        }

        let override_cfg = resource.fields.get(&field.name);

        let is_update_required = update_required.contains(&field.name);
        let update_only = is_update_required && !field.required;

        if let Some(attr) = build_attribute(
            field,
            override_cfg,
            &resource.identity.force_new_fields,
            &reverse_mapping,
            true,
            update_only,
        ) {
            attributes.push(attr);
        }
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

    let reverse_mapping: BTreeMap<String, String> = ds
        .read_mapping
        .iter()
        .map(|(json_path, tf_name)| (to_snake_case(tf_name), json_path.clone()))
        .collect();

    let mut attributes = Vec::new();

    for field in &read_fields {
        if skip_fields.contains(field.name.as_str()) {
            continue;
        }

        let override_cfg = ds.fields.get(&field.name);

        if let Some(attr) = build_attribute(
            field,
            override_cfg,
            &[],
            &reverse_mapping,
            false,
            false,
        ) {
            attributes.push(attr);
        }
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

    #[test]
    fn resolve_resource_update_schema_different_required_fields() {
        let toml_str = r#"
[resource]
name = "test_update"
description = "Test update_only"
category = "test"

[crud]
create_endpoint = "/create"
create_schema = "CreateSchema"
update_endpoint = "/update"
update_schema = "UpdateSchema"
read_endpoint = "/read"
read_schema = "ReadSchema"
delete_endpoint = "/delete"
delete_schema = "DeleteSchema"

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
    CreateSchema:
      type: object
      required: [name]
      properties:
        name: { type: string }
        extra_field: { type: string }
    UpdateSchema:
      type: object
      required: [extra_field]
      properties:
        name: { type: string }
        extra_field: { type: string }
    ReadSchema:
      type: object
      properties:
        name: { type: string }
    DeleteSchema:
      type: object
      properties:
        name: { type: string }
"#;
        let api = Spec::from_str(api_str).expect("parse");
        let defaults = ProviderDefaults::default();
        let iac = resolve_resource(&resource, &api, &defaults).expect("resolve");

        // extra_field: required in UpdateSchema but not required in CreateSchema
        // => update_only = true
        let extra = iac
            .attributes
            .iter()
            .find(|a| a.canonical_name == "extra_field")
            .expect("extra_field");
        assert!(extra.update_only, "extra_field should be update_only");

        // name: required in CreateSchema and NOT required in UpdateSchema
        // => update_only = false
        let name = iac
            .attributes
            .iter()
            .find(|a| a.canonical_name == "name")
            .expect("name");
        assert!(!name.update_only, "name should NOT be update_only");
    }

    #[test]
    fn resolve_resource_computed_and_required_makes_required_false() {
        let toml_str = r#"
[resource]
name = "test_computed_req"
description = "Test computed+required"
category = "test"

[crud]
create_endpoint = "/create"
create_schema = "Schema"
read_endpoint = "/read"
read_schema = "ReadSchema"
delete_endpoint = "/delete"
delete_schema = "DeleteSchema"

[identity]
id_field = "id"

[fields]
server_id = { computed = true }
"#;
        let resource: ResourceSpec = toml::from_str(toml_str).expect("parse");

        let api_str = r#"
openapi: "3.0.0"
info: { title: Test, version: "1.0" }
paths: {}
components:
  schemas:
    Schema:
      type: object
      required: [server_id]
      properties:
        server_id: { type: string }
    ReadSchema:
      type: object
      properties:
        server_id: { type: string }
    DeleteSchema:
      type: object
      properties:
        server_id: { type: string }
"#;
        let api = Spec::from_str(api_str).expect("parse");
        let defaults = ProviderDefaults::default();
        let iac = resolve_resource(&resource, &api, &defaults).expect("resolve");

        let field = iac
            .attributes
            .iter()
            .find(|a| a.canonical_name == "server_id")
            .expect("server_id");
        // When computed=true, required is forced to false
        assert!(!field.required, "computed field should have required=false");
        assert!(field.computed, "field should still be computed");
    }

    #[test]
    fn resolve_resource_all_fields_skipped_by_provider_defaults() {
        let toml_str = r#"
[resource]
name = "all_skipped"
description = "All fields skipped"
category = "test"

[crud]
create_endpoint = "/create"
create_schema = "Schema"
read_endpoint = "/read"
read_schema = "ReadSchema"
delete_endpoint = "/delete"
delete_schema = "DeleteSchema"

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
    Schema:
      type: object
      properties:
        token: { type: string }
        uid_token: { type: string }
    ReadSchema:
      type: object
      properties: {}
    DeleteSchema:
      type: object
      properties: {}
"#;
        let api = Spec::from_str(api_str).expect("parse");
        let defaults = ProviderDefaults {
            skip_fields: vec!["token".to_string(), "uid_token".to_string()],
        };
        let iac = resolve_resource(&resource, &api, &defaults).expect("resolve");

        assert!(
            iac.attributes.is_empty(),
            "all fields should be skipped by provider defaults"
        );
    }

    #[test]
    fn resolve_resource_per_field_and_provider_skip_overlap() {
        let toml_str = r#"
[resource]
name = "double_skip"
description = "Both skip mechanisms"
category = "test"

[crud]
create_endpoint = "/create"
create_schema = "Schema"
read_endpoint = "/read"
read_schema = "ReadSchema"
delete_endpoint = "/delete"
delete_schema = "DeleteSchema"

[identity]
id_field = "id"

[fields]
token = { skip = true }
"#;
        let resource: ResourceSpec = toml::from_str(toml_str).expect("parse");

        let api_str = r#"
openapi: "3.0.0"
info: { title: Test, version: "1.0" }
paths: {}
components:
  schemas:
    Schema:
      type: object
      properties:
        token: { type: string }
        name: { type: string }
    ReadSchema:
      type: object
      properties: {}
    DeleteSchema:
      type: object
      properties: {}
"#;
        let api = Spec::from_str(api_str).expect("parse");
        let defaults = ProviderDefaults {
            skip_fields: vec!["token".to_string()],
        };
        let iac = resolve_resource(&resource, &api, &defaults).expect("resolve");

        // token is skipped by both mechanisms; only name remains
        assert_eq!(iac.attributes.len(), 1);
        assert_eq!(iac.attributes[0].canonical_name, "name");
    }

    #[test]
    fn resolve_data_source_all_fields_computed() {
        let toml_str = r#"
[data_source]
name = "all_computed_ds"
description = "All computed"

[read]
endpoint = "/read"
schema = "ReadSchema"
"#;
        let ds: DataSourceSpec = toml::from_str(toml_str).expect("parse");

        let api_str = r#"
openapi: "3.0.0"
info: { title: Test, version: "1.0" }
paths: {}
components:
  schemas:
    ReadSchema:
      type: object
      properties:
        value: { type: string }
        count: { type: integer }
"#;
        let api = Spec::from_str(api_str).expect("parse");
        let defaults = ProviderDefaults::default();
        let iac = resolve_data_source(&ds, &api, &defaults).expect("resolve");

        // No fields are required, so all should be computed
        for attr in &iac.attributes {
            assert!(
                attr.computed,
                "field {} should be computed",
                attr.canonical_name
            );
            assert!(!attr.required, "field {} should not be required", attr.canonical_name);
            assert!(!attr.update_only, "data source field should never be update_only");
        }
    }

    #[test]
    fn resolve_provider_empty_auth() {
        let toml_str = r#"
[provider]
name = "minimal"
description = "Minimal provider"
version = "0.1.0"
"#;
        let provider: ProviderSpec = toml::from_str(toml_str).expect("parse");
        let iac = resolve_provider(&provider);

        assert_eq!(iac.name, "minimal");
        assert!(iac.auth.token_field.is_empty());
        assert!(iac.auth.env_var.is_empty());
        assert!(iac.auth.gateway_url_field.is_empty());
        assert!(iac.auth.gateway_env_var.is_empty());
        assert!(!iac.auth.has_token());
        assert!(!iac.auth.has_gateway());
        assert!(iac.skip_fields.is_empty());
        assert!(iac.platform_config.is_empty());
    }

    #[test]
    fn resolve_provider_with_platform_config() {
        let toml_str = r#"
[provider]
name = "platformed"
description = "With platforms"
version = "1.0.0"

[auth]
token_field = "token"
env_var = "TOKEN"

[defaults]
skip_fields = []

[platforms.terraform]
sdk_import = "github.com/example/sdk"

[platforms.pulumi]
module = "index"
"#;
        let provider: ProviderSpec = toml::from_str(toml_str).expect("parse");
        let iac = resolve_provider(&provider);

        assert_eq!(iac.platform_config.len(), 2);
        assert!(iac.platform_config.contains_key("terraform"));
        assert!(iac.platform_config.contains_key("pulumi"));
    }

    #[test]
    fn resolve_resource_import_field_defaults_to_id_field() {
        let toml_str = r#"
[resource]
name = "no_import_field"
description = "No import_field set"
category = "test"

[crud]
create_endpoint = "/create"
create_schema = "Schema"
read_endpoint = "/read"
read_schema = "ReadSchema"
delete_endpoint = "/delete"
delete_schema = "DeleteSchema"

[identity]
id_field = "my_id"
"#;
        let resource: ResourceSpec = toml::from_str(toml_str).expect("parse");

        let api_str = r#"
openapi: "3.0.0"
info: { title: Test, version: "1.0" }
paths: {}
components:
  schemas:
    Schema:
      type: object
      properties:
        my_id: { type: string }
    ReadSchema:
      type: object
      properties: {}
    DeleteSchema:
      type: object
      properties: {}
"#;
        let api = Spec::from_str(api_str).expect("parse");
        let defaults = ProviderDefaults::default();
        let iac = resolve_resource(&resource, &api, &defaults).expect("resolve");

        assert_eq!(iac.identity.id_field, "my_id");
        assert_eq!(iac.identity.import_field, "my_id");
    }

    #[test]
    fn resolve_resource_explicit_import_field() {
        let toml_str = r#"
[resource]
name = "with_import_field"
description = "Explicit import_field"
category = "test"

[crud]
create_endpoint = "/create"
create_schema = "Schema"
read_endpoint = "/read"
read_schema = "ReadSchema"
delete_endpoint = "/delete"
delete_schema = "DeleteSchema"

[identity]
id_field = "name"
import_field = "path"
"#;
        let resource: ResourceSpec = toml::from_str(toml_str).expect("parse");

        let api_str = r#"
openapi: "3.0.0"
info: { title: Test, version: "1.0" }
paths: {}
components:
  schemas:
    Schema:
      type: object
      properties:
        name: { type: string }
    ReadSchema:
      type: object
      properties: {}
    DeleteSchema:
      type: object
      properties: {}
"#;
        let api = Spec::from_str(api_str).expect("parse");
        let defaults = ProviderDefaults::default();
        let iac = resolve_resource(&resource, &api, &defaults).expect("resolve");

        assert_eq!(iac.identity.id_field, "name");
        assert_eq!(iac.identity.import_field, "path");
    }

    #[test]
    fn resolve_resource_sensitive_field() {
        let toml_str = r#"
[resource]
name = "with_sensitive"
description = "Has sensitive field"
category = "test"

[crud]
create_endpoint = "/create"
create_schema = "Schema"
read_endpoint = "/read"
read_schema = "ReadSchema"
delete_endpoint = "/delete"
delete_schema = "DeleteSchema"

[identity]
id_field = "name"

[fields]
password = { sensitive = true }
"#;
        let resource: ResourceSpec = toml::from_str(toml_str).expect("parse");

        let api_str = r#"
openapi: "3.0.0"
info: { title: Test, version: "1.0" }
paths: {}
components:
  schemas:
    Schema:
      type: object
      properties:
        name: { type: string }
        password: { type: string }
    ReadSchema:
      type: object
      properties: {}
    DeleteSchema:
      type: object
      properties: {}
"#;
        let api = Spec::from_str(api_str).expect("parse");
        let defaults = ProviderDefaults::default();
        let iac = resolve_resource(&resource, &api, &defaults).expect("resolve");

        let pw = iac
            .attributes
            .iter()
            .find(|a| a.canonical_name == "password")
            .expect("password");
        assert!(pw.sensitive);
    }

    #[test]
    fn resolve_resource_force_new_via_field_override() {
        let toml_str = r#"
[resource]
name = "force_new_override"
description = "Force new via field override"
category = "test"

[crud]
create_endpoint = "/create"
create_schema = "Schema"
read_endpoint = "/read"
read_schema = "ReadSchema"
delete_endpoint = "/delete"
delete_schema = "DeleteSchema"

[identity]
id_field = "name"

[fields]
region = { force_new = true }
"#;
        let resource: ResourceSpec = toml::from_str(toml_str).expect("parse");

        let api_str = r#"
openapi: "3.0.0"
info: { title: Test, version: "1.0" }
paths: {}
components:
  schemas:
    Schema:
      type: object
      properties:
        name: { type: string }
        region: { type: string }
    ReadSchema:
      type: object
      properties: {}
    DeleteSchema:
      type: object
      properties: {}
"#;
        let api = Spec::from_str(api_str).expect("parse");
        let defaults = ProviderDefaults::default();
        let iac = resolve_resource(&resource, &api, &defaults).expect("resolve");

        let region = iac
            .attributes
            .iter()
            .find(|a| a.canonical_name == "region")
            .expect("region");
        assert!(region.immutable, "region should be immutable via force_new override");
    }

    #[test]
    fn resolve_resource_description_override() {
        let toml_str = r#"
[resource]
name = "desc_override"
description = "Test description override"
category = "test"

[crud]
create_endpoint = "/create"
create_schema = "Schema"
read_endpoint = "/read"
read_schema = "ReadSchema"
delete_endpoint = "/delete"
delete_schema = "DeleteSchema"

[identity]
id_field = "name"

[fields]
name = { description = "Custom description" }
"#;
        let resource: ResourceSpec = toml::from_str(toml_str).expect("parse");

        let api_str = r#"
openapi: "3.0.0"
info: { title: Test, version: "1.0" }
paths: {}
components:
  schemas:
    Schema:
      type: object
      properties:
        name: { type: string, description: "Original description" }
    ReadSchema:
      type: object
      properties: {}
    DeleteSchema:
      type: object
      properties: {}
"#;
        let api = Spec::from_str(api_str).expect("parse");
        let defaults = ProviderDefaults::default();
        let iac = resolve_resource(&resource, &api, &defaults).expect("resolve");

        let name = iac
            .attributes
            .iter()
            .find(|a| a.canonical_name == "name")
            .expect("name");
        assert_eq!(name.description, "Custom description");
    }

    #[test]
    fn resolve_data_source_immutable_always_false() {
        let toml_str = r#"
[data_source]
name = "ds_no_immutable"
description = "Data sources never have immutable fields"

[read]
endpoint = "/read"
schema = "ReadSchema"

[fields]
name = { force_new = true }
"#;
        let ds: DataSourceSpec = toml::from_str(toml_str).expect("parse");

        let api_str = r#"
openapi: "3.0.0"
info: { title: Test, version: "1.0" }
paths: {}
components:
  schemas:
    ReadSchema:
      type: object
      required: [name]
      properties:
        name: { type: string }
"#;
        let api = Spec::from_str(api_str).expect("parse");
        let defaults = ProviderDefaults::default();
        let iac = resolve_data_source(&ds, &api, &defaults).expect("resolve");

        for attr in &iac.attributes {
            assert!(!attr.immutable, "data source field should never be immutable");
        }
    }

    #[test]
    fn resolve_data_source_required_input_not_computed() {
        let toml_str = r#"
[data_source]
name = "ds_input"
description = "Data source with required input"

[read]
endpoint = "/read"
schema = "ReadSchema"
"#;
        let ds: DataSourceSpec = toml::from_str(toml_str).expect("parse");

        let api_str = r#"
openapi: "3.0.0"
info: { title: Test, version: "1.0" }
paths: {}
components:
  schemas:
    ReadSchema:
      type: object
      required: [name]
      properties:
        name: { type: string, description: "Lookup key" }
        result: { type: string, description: "Computed result" }
"#;
        let api = Spec::from_str(api_str).expect("parse");
        let defaults = ProviderDefaults::default();
        let iac = resolve_data_source(&ds, &api, &defaults).expect("resolve");

        let name = iac.attributes.iter().find(|a| a.canonical_name == "name").expect("name");
        // required field in data source: required=false but computed=false (it's an input)
        assert!(!name.computed, "required input should not be computed");

        let result = iac.attributes.iter().find(|a| a.canonical_name == "result").expect("result");
        assert!(result.computed, "optional non-required field should be computed in data source");
    }
}
