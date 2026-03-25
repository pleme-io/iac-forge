//! Shared test fixtures for iac-forge backends.
//!
//! Use these helpers in backend tests to avoid duplicating test data construction.

use std::collections::BTreeMap;

use crate::ir::{
    AuthInfo, CrudInfo, IacAttribute, IacDataSource, IacProvider, IacResource, IacType,
    IdentityInfo,
};

/// Create a minimal test provider.
#[must_use]
pub fn test_provider(name: &str) -> IacProvider {
    IacProvider {
        name: name.to_string(),
        description: format!("{name} test provider"),
        version: "1.0.0".to_string(),
        auth: AuthInfo {
            token_field: "token".to_string(),
            env_var: format!("{}_TOKEN", name.to_uppercase()),
            gateway_url_field: "api_url".to_string(),
            gateway_env_var: format!("{}_API_URL", name.to_uppercase()),
        },
        skip_fields: vec!["token".to_string()],
        platform_config: BTreeMap::new(),
    }
}

/// Create a test resource with typical fields (string required+immutable, string sensitive, list optional).
///
/// Creates three attributes:
/// - `"name"`: String, required, immutable, `read_path = "item_name"`
/// - `"value"`: String, required, sensitive
/// - `"tags"`: `List(String)`, optional, `read_path = "item_tags"`
#[must_use]
pub fn test_resource(name: &str) -> IacResource {
    IacResource {
        name: name.to_string(),
        description: format!("{name} test resource"),
        category: "test".to_string(),
        crud: CrudInfo {
            create_endpoint: format!("/create-{name}"),
            create_schema: format!("Create{name}"),
            update_endpoint: None,
            update_schema: None,
            read_endpoint: format!("/get-{name}"),
            read_schema: format!("Get{name}"),
            read_response_schema: None,
            delete_endpoint: format!("/delete-{name}"),
            delete_schema: format!("Delete{name}"),
        },
        attributes: vec![
            TestAttributeBuilder::new("name", IacType::String)
                .required()
                .immutable()
                .read_path("item_name")
                .description("The resource name")
                .build(),
            TestAttributeBuilder::new("value", IacType::String)
                .required()
                .sensitive()
                .description("The resource value")
                .build(),
            TestAttributeBuilder::new("tags", IacType::List(Box::new(IacType::String)))
                .read_path("item_tags")
                .description("Resource tags")
                .build(),
        ],
        identity: IdentityInfo {
            id_field: "name".to_string(),
            import_field: "name".to_string(),
            force_replace_fields: vec!["name".to_string()],
        },
    }
}

/// Create a test resource with a single attribute of a given type.
#[must_use]
pub fn test_resource_with_type(name: &str, attr_name: &str, iac_type: IacType) -> IacResource {
    IacResource {
        name: name.to_string(),
        description: format!("{name} test resource"),
        category: "test".to_string(),
        crud: CrudInfo {
            create_endpoint: format!("/create-{name}"),
            create_schema: format!("Create{name}"),
            update_endpoint: None,
            update_schema: None,
            read_endpoint: format!("/get-{name}"),
            read_schema: format!("Get{name}"),
            read_response_schema: None,
            delete_endpoint: format!("/delete-{name}"),
            delete_schema: format!("Delete{name}"),
        },
        attributes: vec![TestAttributeBuilder::new(attr_name, iac_type)
            .required()
            .build()],
        identity: IdentityInfo {
            id_field: attr_name.to_string(),
            import_field: attr_name.to_string(),
            force_replace_fields: vec![],
        },
    }
}

/// Create a test data source with typical fields.
///
/// Creates two attributes:
/// - `"name"`: String, required (not computed)
/// - `"value"`: String, computed
#[must_use]
pub fn test_data_source(name: &str) -> IacDataSource {
    IacDataSource {
        name: name.to_string(),
        description: format!("{name} test data source"),
        read_endpoint: format!("/get-{name}"),
        read_schema: format!("Get{name}"),
        read_response_schema: None,
        attributes: vec![
            TestAttributeBuilder::new("name", IacType::String)
                .required()
                .description("The data source name")
                .build(),
            TestAttributeBuilder::new("value", IacType::String)
                .computed()
                .description("The data source value")
                .build(),
        ],
    }
}

/// Builder for constructing test `IacAttribute` values with sensible defaults.
pub struct TestAttributeBuilder {
    api_name: String,
    canonical_name: String,
    description: String,
    iac_type: IacType,
    required: bool,
    computed: bool,
    sensitive: bool,
    json_encoded: bool,
    immutable: bool,
    default_value: Option<serde_json::Value>,
    enum_values: Option<Vec<String>>,
    read_path: Option<String>,
    update_only: bool,
}

impl TestAttributeBuilder {
    /// Create a new builder with the given name and type. All flags default to `false`.
    #[must_use]
    pub fn new(name: &str, iac_type: IacType) -> Self {
        Self {
            api_name: name.to_string(),
            canonical_name: name.replace('-', "_"),
            description: String::new(),
            iac_type,
            required: false,
            computed: false,
            sensitive: false,
            json_encoded: false,
            immutable: false,
            default_value: None,
            enum_values: None,
            read_path: None,
            update_only: false,
        }
    }

    /// Mark the attribute as required.
    #[must_use]
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Mark the attribute as computed.
    #[must_use]
    pub fn computed(mut self) -> Self {
        self.computed = true;
        self
    }

    /// Mark the attribute as sensitive.
    #[must_use]
    pub fn sensitive(mut self) -> Self {
        self.sensitive = true;
        self
    }

    /// Mark the attribute as carrying JSON-encoded string content.
    #[must_use]
    pub fn json_encoded(mut self) -> Self {
        self.json_encoded = true;
        self
    }

    /// Mark the attribute as immutable (force-new on change).
    #[must_use]
    pub fn immutable(mut self) -> Self {
        self.immutable = true;
        self
    }

    /// Set the read path for mapping from API response.
    #[must_use]
    pub fn read_path(mut self, path: &str) -> Self {
        self.read_path = Some(path.to_string());
        self
    }

    /// Set the attribute description.
    #[must_use]
    pub fn description(mut self, desc: &str) -> Self {
        self.description = desc.to_string();
        self
    }

    /// Set a default value.
    #[must_use]
    pub fn default_value(mut self, val: serde_json::Value) -> Self {
        self.default_value = Some(val);
        self
    }

    /// Set enum constraint values.
    #[must_use]
    pub fn enum_values(mut self, values: Vec<String>) -> Self {
        self.enum_values = Some(values);
        self
    }

    /// Mark the attribute as update-only.
    #[must_use]
    pub fn update_only(mut self) -> Self {
        self.update_only = true;
        self
    }

    /// Build the `IacAttribute`.
    #[must_use]
    pub fn build(self) -> IacAttribute {
        IacAttribute {
            api_name: self.api_name,
            canonical_name: self.canonical_name,
            description: self.description,
            iac_type: self.iac_type,
            required: self.required,
            computed: self.computed,
            sensitive: self.sensitive,
            json_encoded: self.json_encoded,
            immutable: self.immutable,
            default_value: self.default_value,
            enum_values: self.enum_values,
            read_path: self.read_path,
            update_only: self.update_only,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_creates_valid_provider() {
        let p = test_provider("acme");
        assert_eq!(p.name, "acme");
        assert_eq!(p.description, "acme test provider");
        assert_eq!(p.version, "1.0.0");
        assert_eq!(p.auth.token_field, "token");
        assert_eq!(p.auth.env_var, "ACME_TOKEN");
        assert_eq!(p.auth.gateway_url_field, "api_url");
        assert_eq!(p.auth.gateway_env_var, "ACME_API_URL");
        assert_eq!(p.skip_fields, vec!["token"]);
        assert!(p.platform_config.is_empty());
    }

    #[test]
    fn test_resource_creates_three_attributes() {
        let r = test_resource("secret");
        assert_eq!(r.name, "secret");
        assert_eq!(r.attributes.len(), 3);

        let name_attr = &r.attributes[0];
        assert_eq!(name_attr.canonical_name, "name");
        assert!(name_attr.required);
        assert!(name_attr.immutable);
        assert_eq!(name_attr.read_path, Some("item_name".to_string()));

        let value_attr = &r.attributes[1];
        assert_eq!(value_attr.canonical_name, "value");
        assert!(value_attr.required);
        assert!(value_attr.sensitive);

        let tags_attr = &r.attributes[2];
        assert_eq!(tags_attr.canonical_name, "tags");
        assert!(!tags_attr.required);
        assert_eq!(tags_attr.read_path, Some("item_tags".to_string()));
        assert_eq!(
            tags_attr.iac_type,
            IacType::List(Box::new(IacType::String))
        );
    }

    #[test]
    fn test_resource_with_type_single_attribute() {
        let r = test_resource_with_type("flag", "enabled", IacType::Boolean);
        assert_eq!(r.attributes.len(), 1);
        assert_eq!(r.attributes[0].canonical_name, "enabled");
        assert_eq!(r.attributes[0].iac_type, IacType::Boolean);
        assert!(r.attributes[0].required);
    }

    #[test]
    fn test_data_source_creates_two_attributes() {
        let ds = test_data_source("config");
        assert_eq!(ds.name, "config");
        assert_eq!(ds.attributes.len(), 2);

        let name_attr = &ds.attributes[0];
        assert_eq!(name_attr.canonical_name, "name");
        assert!(name_attr.required);

        let value_attr = &ds.attributes[1];
        assert_eq!(value_attr.canonical_name, "value");
        assert!(value_attr.computed);
    }

    #[test]
    fn test_attribute_builder_defaults() {
        let attr = TestAttributeBuilder::new("field", IacType::String).build();
        assert_eq!(attr.api_name, "field");
        assert_eq!(attr.canonical_name, "field");
        assert_eq!(attr.iac_type, IacType::String);
        assert!(!attr.required);
        assert!(!attr.computed);
        assert!(!attr.sensitive);
        assert!(!attr.json_encoded);
        assert!(!attr.immutable);
        assert!(!attr.update_only);
        assert!(attr.description.is_empty());
        assert!(attr.default_value.is_none());
        assert!(attr.enum_values.is_none());
        assert!(attr.read_path.is_none());
    }

    #[test]
    fn test_attribute_builder_all_flags() {
        let attr = TestAttributeBuilder::new("secret-key", IacType::String)
            .required()
            .computed()
            .sensitive()
            .json_encoded()
            .immutable()
            .update_only()
            .read_path("secret_key_resp")
            .description("A secret key")
            .default_value(serde_json::json!("default"))
            .enum_values(vec!["a".into(), "b".into()])
            .build();
        assert_eq!(attr.api_name, "secret-key");
        assert_eq!(attr.canonical_name, "secret_key");
        assert!(attr.required);
        assert!(attr.computed);
        assert!(attr.sensitive);
        assert!(attr.json_encoded);
        assert!(attr.immutable);
        assert!(attr.update_only);
        assert_eq!(attr.read_path, Some("secret_key_resp".to_string()));
        assert_eq!(attr.description, "A secret key");
        assert_eq!(attr.default_value, Some(serde_json::json!("default")));
        assert_eq!(
            attr.enum_values,
            Some(vec!["a".to_string(), "b".to_string()])
        );
    }

    #[test]
    fn test_resource_with_type_integer() {
        let r = test_resource_with_type("counter", "count", IacType::Integer);
        assert_eq!(r.attributes.len(), 1);
        assert_eq!(r.attributes[0].iac_type, IacType::Integer);
        assert!(r.attributes[0].required);
    }

    #[test]
    fn test_resource_with_type_float() {
        let r = test_resource_with_type("metric", "value", IacType::Float);
        assert_eq!(r.attributes[0].iac_type, IacType::Float);
    }

    #[test]
    fn test_resource_with_type_list() {
        let r = test_resource_with_type(
            "tagged",
            "tags",
            IacType::List(Box::new(IacType::String)),
        );
        assert_eq!(
            r.attributes[0].iac_type,
            IacType::List(Box::new(IacType::String))
        );
    }

    #[test]
    fn test_resource_with_type_map() {
        let r = test_resource_with_type(
            "config",
            "settings",
            IacType::Map(Box::new(IacType::String)),
        );
        assert_eq!(
            r.attributes[0].iac_type,
            IacType::Map(Box::new(IacType::String))
        );
    }

    #[test]
    fn test_resource_with_type_set() {
        let r = test_resource_with_type(
            "unique",
            "items",
            IacType::Set(Box::new(IacType::Integer)),
        );
        assert_eq!(
            r.attributes[0].iac_type,
            IacType::Set(Box::new(IacType::Integer))
        );
    }

    #[test]
    fn test_resource_with_type_object() {
        let r = test_resource_with_type(
            "complex",
            "config",
            IacType::Object {
                name: "Config".to_string(),
                fields: vec![],
            },
        );
        assert_eq!(
            r.attributes[0].iac_type,
            IacType::Object {
                name: "Config".to_string(),
                fields: vec![]
            }
        );
    }

    #[test]
    fn test_resource_with_type_enum() {
        let r = test_resource_with_type(
            "status",
            "state",
            IacType::Enum {
                values: vec!["active".to_string(), "inactive".to_string()],
                underlying: Box::new(IacType::String),
            },
        );
        assert_eq!(
            r.attributes[0].iac_type,
            IacType::Enum {
                values: vec!["active".to_string(), "inactive".to_string()],
                underlying: Box::new(IacType::String)
            }
        );
    }

    #[test]
    fn test_resource_with_type_any() {
        let r = test_resource_with_type("dynamic", "data", IacType::Any);
        assert_eq!(r.attributes[0].iac_type, IacType::Any);
    }

    #[test]
    fn test_attribute_builder_hyphenated_name() {
        let attr = TestAttributeBuilder::new("my-field-name", IacType::String).build();
        assert_eq!(attr.api_name, "my-field-name");
        assert_eq!(attr.canonical_name, "my_field_name");
    }

    #[test]
    fn test_attribute_builder_already_snake_case() {
        let attr = TestAttributeBuilder::new("already_snake", IacType::String).build();
        assert_eq!(attr.api_name, "already_snake");
        assert_eq!(attr.canonical_name, "already_snake");
    }

    #[test]
    fn test_resource_crud_endpoints() {
        let r = test_resource("widget");
        assert_eq!(r.crud.create_endpoint, "/create-widget");
        assert_eq!(r.crud.read_endpoint, "/get-widget");
        assert_eq!(r.crud.delete_endpoint, "/delete-widget");
        assert!(r.crud.update_endpoint.is_none());
        assert!(r.crud.update_schema.is_none());
    }

    #[test]
    fn test_data_source_read_info() {
        let ds = test_data_source("info");
        assert_eq!(ds.read_endpoint, "/get-info");
        assert_eq!(ds.read_schema, "Getinfo");
        assert!(ds.read_response_schema.is_none());
    }

    #[test]
    fn test_resource_with_type_identity() {
        let r = test_resource_with_type("item", "my_id", IacType::String);
        assert_eq!(r.identity.id_field, "my_id");
        assert_eq!(r.identity.import_field, "my_id");
        assert!(r.identity.force_replace_fields.is_empty());
    }
}
