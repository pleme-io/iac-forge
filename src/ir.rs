use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Platform-independent type representation.
///
/// Richer than any single platform's type system -- preserves Object structure
/// and Enum values needed by Pulumi schemas, Crossplane CRDs, etc.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum IacType {
    String,
    Integer,
    Float,
    /// Platform-independent numeric type.
    ///
    /// Terraform's `number` type accepts both integers and floats. This variant
    /// preserves that ambiguity rather than forcing a choice between `Integer`
    /// and `Float` at schema-import time.
    Numeric,
    Boolean,
    List(Box<IacType>),
    Set(Box<IacType>),
    Map(Box<IacType>),
    Object {
        name: String,
        fields: Vec<IacAttribute>,
    },
    /// Enum constraint wrapping an underlying type.
    ///
    /// `values` lists the allowed enum variants. `underlying` is the base type
    /// (typically `IacType::String`).
    Enum {
        values: Vec<String>,
        underlying: Box<IacType>,
    },
    Any,
}

impl std::fmt::Display for IacType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::String => write!(f, "string"),
            Self::Integer => write!(f, "integer"),
            Self::Float => write!(f, "float"),
            Self::Numeric => write!(f, "numeric"),
            Self::Boolean => write!(f, "boolean"),
            Self::List(inner) => write!(f, "list<{inner}>"),
            Self::Set(inner) => write!(f, "set<{inner}>"),
            Self::Map(inner) => write!(f, "map<string, {inner}>"),
            Self::Object { name, .. } => write!(f, "object<{name}>"),
            Self::Enum { underlying, .. } => write!(f, "enum<{underlying}>"),
            Self::Any => write!(f, "any"),
        }
    }
}

/// A resolved attribute in the platform-independent IR.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct IacAttribute {
    /// Original API field name (e.g., "bound-aws-account-id").
    pub api_name: String,
    /// Normalized name with underscores (e.g., `bound_aws_account_id`).
    pub canonical_name: String,
    /// Human-readable description.
    pub description: String,
    /// Platform-independent type.
    pub iac_type: IacType,
    /// Whether the field is required on create.
    pub required: bool,
    /// Whether the field is optional (user can set it, but it is not required).
    ///
    /// In Terraform schema terms:
    /// - `optional: true, computed: false` — user can set, no server default
    /// - `optional: true, computed: true`  — user can set, server has default
    /// - `optional: false, computed: true` — purely server-generated
    #[serde(default)]
    pub optional: bool,
    /// Whether the field is computed (server-side generated).
    pub computed: bool,
    /// Whether the field contains sensitive data.
    pub sensitive: bool,
    /// Whether the field value is a JSON-encoded string.
    ///
    /// When `true`, the underlying Terraform type is still `String`, but the
    /// value carries structured JSON (e.g., IAM policy documents). Code
    /// generators can use this to accept both `String` and `Hash` inputs.
    #[serde(default)]
    pub json_encoded: bool,
    /// Whether changing this field forces resource replacement.
    pub immutable: bool,
    /// Default value, if any.
    pub default_value: Option<serde_json::Value>,
    /// Enum constraint values, if any.
    pub enum_values: Option<Vec<String>>,
    /// JSON path in API response for reading this field back
    /// (e.g., `item_name` maps to the API response key).
    pub read_path: Option<String>,
    /// Whether this field exists only in the update schema (not in create).
    pub update_only: bool,
}

impl std::fmt::Display for IacAttribute {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {} ({})",
            self.canonical_name,
            self.iac_type,
            if self.required { "required" } else { "optional" }
        )
    }
}

/// CRUD endpoint information for a resource.
///
/// Each field maps to an API endpoint path and its corresponding `OpenAPI` schema
/// name used for request/response serialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrudInfo {
    /// API path for the create operation (e.g., "/create-secret").
    pub create_endpoint: String,
    /// `OpenAPI` schema name for the create request body.
    pub create_schema: String,
    /// API path for the update operation, if separate from create.
    pub update_endpoint: Option<String>,
    /// `OpenAPI` schema name for the update request body.
    pub update_schema: Option<String>,
    /// API path for the read operation.
    pub read_endpoint: String,
    /// `OpenAPI` schema name for the read request body.
    pub read_schema: String,
    /// `OpenAPI` schema name for the read response, if different from the request.
    pub read_response_schema: Option<String>,
    /// API path for the delete operation.
    pub delete_endpoint: String,
    /// `OpenAPI` schema name for the delete request body.
    pub delete_schema: String,
}

/// Identity and import configuration for a resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityInfo {
    /// Primary identifier field name (e.g., "name" or "id").
    pub id_field: String,
    /// Field used for Terraform import (defaults to `id_field`).
    pub import_field: String,
    /// Fields that force resource replacement when changed.
    pub force_replace_fields: Vec<String>,
}

/// A fully resolved resource in the platform-independent IR.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IacResource {
    pub name: String,
    pub description: String,
    pub category: String,
    pub crud: CrudInfo,
    pub attributes: Vec<IacAttribute>,
    pub identity: IdentityInfo,
}

impl IacResource {
    /// Attributes that are user-provided inputs (not purely computed).
    ///
    /// Includes required, optional, and optional+computed attributes.
    /// Excludes purely computed attributes (computed=true, optional=false, required=false).
    #[must_use]
    pub fn input_attributes(&self) -> Vec<&IacAttribute> {
        self.attributes
            .iter()
            .filter(|a| a.required || a.optional || !a.computed)
            .collect()
    }

    /// Attributes that appear in the output/state (computed or required).
    #[must_use]
    pub fn output_attributes(&self) -> Vec<&IacAttribute> {
        self.attributes
            .iter()
            .filter(|a| a.computed || a.required)
            .collect()
    }

    /// Required attribute canonical names.
    #[must_use]
    pub fn required_attribute_names(&self) -> Vec<&str> {
        self.attributes
            .iter()
            .filter(|a| a.required)
            .map(|a| a.canonical_name.as_str())
            .collect()
    }

    /// Sensitive attribute canonical names.
    #[must_use]
    pub fn sensitive_attribute_names(&self) -> Vec<&str> {
        self.attributes
            .iter()
            .filter(|a| a.sensitive)
            .map(|a| a.canonical_name.as_str())
            .collect()
    }

    /// Immutable attribute canonical names.
    #[must_use]
    pub fn immutable_attribute_names(&self) -> Vec<&str> {
        self.attributes
            .iter()
            .filter(|a| a.immutable)
            .map(|a| a.canonical_name.as_str())
            .collect()
    }
}

/// A fully resolved data source in the platform-independent IR.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IacDataSource {
    pub name: String,
    pub description: String,
    pub read_endpoint: String,
    pub read_schema: String,
    pub read_response_schema: Option<String>,
    pub attributes: Vec<IacAttribute>,
}

impl IacDataSource {
    /// Attributes that are user-provided inputs (not purely computed).
    ///
    /// Includes required, optional, and optional+computed attributes.
    /// Excludes purely computed attributes (computed=true, optional=false, required=false).
    #[must_use]
    pub fn input_attributes(&self) -> Vec<&IacAttribute> {
        self.attributes
            .iter()
            .filter(|a| a.required || a.optional || !a.computed)
            .collect()
    }

    /// Attributes that appear in the output/state (computed or required).
    #[must_use]
    pub fn output_attributes(&self) -> Vec<&IacAttribute> {
        self.attributes
            .iter()
            .filter(|a| a.computed || a.required)
            .collect()
    }

    /// Required attribute names.
    #[must_use]
    pub fn required_attribute_names(&self) -> Vec<&str> {
        self.attributes
            .iter()
            .filter(|a| a.required)
            .map(|a| a.canonical_name.as_str())
            .collect()
    }

    /// Sensitive attribute names.
    #[must_use]
    pub fn sensitive_attribute_names(&self) -> Vec<&str> {
        self.attributes
            .iter()
            .filter(|a| a.sensitive)
            .map(|a| a.canonical_name.as_str())
            .collect()
    }

    /// Computed attribute names.
    #[must_use]
    pub fn computed_attribute_names(&self) -> Vec<&str> {
        self.attributes
            .iter()
            .filter(|a| a.computed)
            .map(|a| a.canonical_name.as_str())
            .collect()
    }
}

/// Provider-level configuration in the platform-independent IR.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IacProvider {
    pub name: String,
    pub description: String,
    pub version: String,
    pub auth: AuthInfo,
    pub skip_fields: Vec<String>,
    pub platform_config: BTreeMap<String, toml::Value>,
}

/// Authentication configuration for a provider.
///
/// Empty strings mean "not configured". Use the `has_token()` and
/// `has_gateway()` helpers to check.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthInfo {
    /// API field name for the authentication token (e.g., "token").
    pub token_field: String,
    /// Environment variable that supplies the token (e.g., `AKEYLESS_ACCESS_TOKEN`).
    pub env_var: String,
    /// API field name for the gateway URL (e.g., `api_gateway_address`).
    pub gateway_url_field: String,
    /// Environment variable that supplies the gateway URL (e.g., `AKEYLESS_GATEWAY`).
    pub gateway_env_var: String,
}

impl AuthInfo {
    /// Returns `true` if a token field is configured.
    #[must_use]
    pub fn has_token(&self) -> bool {
        !self.token_field.is_empty()
    }

    /// Returns `true` if a gateway URL field is configured.
    #[must_use]
    pub fn has_gateway(&self) -> bool {
        !self.gateway_url_field.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iac_type_display() {
        assert_eq!(IacType::String.to_string(), "string");
        assert_eq!(IacType::Integer.to_string(), "integer");
        assert_eq!(IacType::Float.to_string(), "float");
        assert_eq!(IacType::Numeric.to_string(), "numeric");
        assert_eq!(IacType::Boolean.to_string(), "boolean");
        assert_eq!(
            IacType::List(Box::new(IacType::String)).to_string(),
            "list<string>"
        );
        assert_eq!(
            IacType::Set(Box::new(IacType::Integer)).to_string(),
            "set<integer>"
        );
        assert_eq!(
            IacType::Map(Box::new(IacType::Boolean)).to_string(),
            "map<string, boolean>"
        );
        assert_eq!(
            IacType::Object {
                name: "User".to_string(),
                fields: vec![]
            }
            .to_string(),
            "object<User>"
        );
        assert_eq!(
            IacType::Enum {
                values: vec!["a".into()],
                underlying: Box::new(IacType::String)
            }
            .to_string(),
            "enum<string>"
        );
        assert_eq!(IacType::Any.to_string(), "any");
    }

    #[test]
    fn iac_type_equality() {
        assert_eq!(IacType::String, IacType::String);
        assert_ne!(IacType::String, IacType::Integer);
        assert_eq!(
            IacType::List(Box::new(IacType::String)),
            IacType::List(Box::new(IacType::String))
        );
        assert_ne!(
            IacType::List(Box::new(IacType::String)),
            IacType::List(Box::new(IacType::Integer))
        );
    }

    #[test]
    fn iac_type_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(IacType::String);
        set.insert(IacType::Integer);
        set.insert(IacType::String);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn iac_attribute_display() {
        let attr = IacAttribute {
            api_name: "name".to_string(),
            canonical_name: "name".to_string(),
            description: String::new(),
            iac_type: IacType::String,
            required: true,
            optional: false,
            computed: false,
            sensitive: false,
            json_encoded: false,
            immutable: false,
            default_value: None,
            enum_values: None,
            read_path: None,
            update_only: false,
        };
        assert_eq!(attr.to_string(), "name: string (required)");

        let optional = IacAttribute {
            required: false,
            optional: true,
            ..attr
        };
        assert_eq!(optional.to_string(), "name: string (optional)");
    }

    #[test]
    fn auth_info_helpers() {
        let empty = AuthInfo::default();
        assert!(!empty.has_token());
        assert!(!empty.has_gateway());

        let configured = AuthInfo {
            token_field: "token".to_string(),
            env_var: "TOKEN".to_string(),
            gateway_url_field: "gateway".to_string(),
            gateway_env_var: "GW".to_string(),
        };
        assert!(configured.has_token());
        assert!(configured.has_gateway());
    }

    #[test]
    fn iac_resource_serialize_roundtrip() {
        let resource = IacResource {
            name: "test_resource".to_string(),
            description: "A test".to_string(),
            category: "test".to_string(),
            crud: CrudInfo {
                create_endpoint: "/create".to_string(),
                create_schema: "Create".to_string(),
                update_endpoint: None,
                update_schema: None,
                read_endpoint: "/read".to_string(),
                read_schema: "Read".to_string(),
                read_response_schema: None,
                delete_endpoint: "/delete".to_string(),
                delete_schema: "Delete".to_string(),
            },
            attributes: vec![IacAttribute {
                api_name: "name".to_string(),
                canonical_name: "name".to_string(),
                description: "The name".to_string(),
                iac_type: IacType::String,
                required: true,
                optional: false,
                computed: false,
                sensitive: false,
                json_encoded: false,
                immutable: true,
                default_value: None,
                enum_values: None,
                read_path: None,
                update_only: false,
            }],
            identity: IdentityInfo {
                id_field: "name".to_string(),
                import_field: "name".to_string(),
                force_replace_fields: vec!["name".to_string()],
            },
        };

        let json = serde_json::to_string(&resource).expect("serialize");
        let deserialized: IacResource = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deserialized.name, resource.name);
        assert_eq!(deserialized.attributes.len(), 1);
        assert_eq!(deserialized.attributes[0].iac_type, IacType::String);
    }

    /// Helper to build a resource with mixed attribute flags for filter tests.
    fn resource_with_mixed_attrs() -> IacResource {
        use crate::testing::TestAttributeBuilder;
        IacResource {
            name: "mixed".to_string(),
            description: String::new(),
            category: "test".to_string(),
            crud: CrudInfo {
                create_endpoint: "/create".to_string(),
                create_schema: "Create".to_string(),
                update_endpoint: None,
                update_schema: None,
                read_endpoint: "/read".to_string(),
                read_schema: "Read".to_string(),
                read_response_schema: None,
                delete_endpoint: "/delete".to_string(),
                delete_schema: "Delete".to_string(),
            },
            attributes: vec![
                TestAttributeBuilder::new("name", IacType::String)
                    .required()
                    .immutable()
                    .build(),
                TestAttributeBuilder::new("secret", IacType::String)
                    .required()
                    .sensitive()
                    .build(),
                TestAttributeBuilder::new("computed_id", IacType::String)
                    .computed()
                    .build(),
                TestAttributeBuilder::new("tags", IacType::List(Box::new(IacType::String)))
                    .build(),
            ],
            identity: IdentityInfo {
                id_field: "name".to_string(),
                import_field: "name".to_string(),
                force_replace_fields: vec!["name".to_string()],
            },
        }
    }

    #[test]
    fn input_attributes_excludes_purely_computed() {
        let r = resource_with_mixed_attrs();
        let inputs = r.input_attributes();
        // "name" (required), "secret" (required), "tags" (not computed) = 3
        // "computed_id" is computed and not required, so excluded
        assert_eq!(inputs.len(), 3);
        assert!(inputs.iter().all(|a| a.canonical_name != "computed_id"));
    }

    #[test]
    fn output_attributes_includes_computed_and_required() {
        let r = resource_with_mixed_attrs();
        let outputs = r.output_attributes();
        // "name" (required), "secret" (required), "computed_id" (computed) = 3
        // "tags" is neither computed nor required
        assert_eq!(outputs.len(), 3);
        assert!(outputs.iter().all(|a| a.canonical_name != "tags"));
    }

    #[test]
    fn required_attribute_names_returns_correct_set() {
        let r = resource_with_mixed_attrs();
        let names = r.required_attribute_names();
        assert_eq!(names, vec!["name", "secret"]);
    }

    #[test]
    fn sensitive_attribute_names_returns_correct_set() {
        let r = resource_with_mixed_attrs();
        let names = r.sensitive_attribute_names();
        assert_eq!(names, vec!["secret"]);
    }

    #[test]
    fn immutable_attribute_names_returns_correct_set() {
        let r = resource_with_mixed_attrs();
        let names = r.immutable_attribute_names();
        assert_eq!(names, vec!["name"]);
    }

    #[test]
    fn data_source_input_and_output_attributes() {
        let ds = crate::testing::test_data_source("cfg");
        // "name" is required (not computed) -> input
        // "value" is computed -> output
        let inputs = ds.input_attributes();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].canonical_name, "name");

        let outputs = ds.output_attributes();
        assert_eq!(outputs.len(), 2);

        let computed = ds.computed_attribute_names();
        assert_eq!(computed, vec!["value"]);
    }

    #[test]
    fn data_source_required_and_sensitive() {
        use crate::testing::TestAttributeBuilder;
        let ds = IacDataSource {
            name: "test".to_string(),
            description: String::new(),
            read_endpoint: "/read".to_string(),
            read_schema: "Read".to_string(),
            read_response_schema: None,
            attributes: vec![
                TestAttributeBuilder::new("id", IacType::String)
                    .required()
                    .build(),
                TestAttributeBuilder::new("password", IacType::String)
                    .sensitive()
                    .computed()
                    .build(),
            ],
        };
        assert_eq!(ds.required_attribute_names(), vec!["id"]);
        assert_eq!(ds.sensitive_attribute_names(), vec!["password"]);
    }

    #[test]
    fn resource_input_attributes_mix() {
        use crate::testing::TestAttributeBuilder;
        let r = IacResource {
            name: "mix".to_string(),
            description: String::new(),
            category: "test".to_string(),
            crud: CrudInfo {
                create_endpoint: "/create".to_string(),
                create_schema: "Create".to_string(),
                update_endpoint: None,
                update_schema: None,
                read_endpoint: "/read".to_string(),
                read_schema: "Read".to_string(),
                read_response_schema: None,
                delete_endpoint: "/delete".to_string(),
                delete_schema: "Delete".to_string(),
            },
            attributes: vec![
                TestAttributeBuilder::new("req_only", IacType::String)
                    .required()
                    .build(),
                TestAttributeBuilder::new("comp_only", IacType::String)
                    .computed()
                    .build(),
                TestAttributeBuilder::new("comp_and_req", IacType::String)
                    .computed()
                    .required()
                    .build(),
                TestAttributeBuilder::new("optional", IacType::String).build(),
            ],
            identity: IdentityInfo {
                id_field: "req_only".to_string(),
                import_field: "req_only".to_string(),
                force_replace_fields: vec![],
            },
        };

        let inputs = r.input_attributes();
        let input_names: Vec<&str> = inputs.iter().map(|a| a.canonical_name.as_str()).collect();
        // Input: not computed OR required
        // req_only: not computed -> yes
        // comp_only: computed and not required -> no
        // comp_and_req: computed but required -> yes
        // optional: not computed -> yes
        assert_eq!(input_names, vec!["req_only", "comp_and_req", "optional"]);

        let outputs = r.output_attributes();
        let output_names: Vec<&str> = outputs.iter().map(|a| a.canonical_name.as_str()).collect();
        // Output: computed OR required
        // req_only: required -> yes
        // comp_only: computed -> yes
        // comp_and_req: both -> yes
        // optional: neither -> no
        assert_eq!(output_names, vec!["req_only", "comp_only", "comp_and_req"]);
    }

    #[test]
    fn data_source_input_and_output_full() {
        use crate::testing::TestAttributeBuilder;
        let ds = IacDataSource {
            name: "full_ds".to_string(),
            description: String::new(),
            read_endpoint: "/read".to_string(),
            read_schema: "Read".to_string(),
            read_response_schema: None,
            attributes: vec![
                TestAttributeBuilder::new("input_key", IacType::String)
                    .required()
                    .build(),
                TestAttributeBuilder::new("output_val", IacType::String)
                    .computed()
                    .build(),
                TestAttributeBuilder::new("both", IacType::String)
                    .required()
                    .computed()
                    .build(),
                TestAttributeBuilder::new("neither", IacType::String).build(),
            ],
        };

        let inputs = ds.input_attributes();
        let input_names: Vec<&str> = inputs.iter().map(|a| a.canonical_name.as_str()).collect();
        // Input: not computed OR required
        assert_eq!(input_names, vec!["input_key", "both", "neither"]);

        let outputs = ds.output_attributes();
        let output_names: Vec<&str> = outputs.iter().map(|a| a.canonical_name.as_str()).collect();
        // Output: computed OR required
        assert_eq!(output_names, vec!["input_key", "output_val", "both"]);

        let computed = ds.computed_attribute_names();
        assert_eq!(computed, vec!["output_val", "both"]);
    }

    #[test]
    fn auth_info_has_token_only() {
        let auth = AuthInfo {
            token_field: "token".to_string(),
            env_var: "TOKEN".to_string(),
            gateway_url_field: String::new(),
            gateway_env_var: String::new(),
        };
        assert!(auth.has_token());
        assert!(!auth.has_gateway());
    }

    #[test]
    fn auth_info_has_gateway_only() {
        let auth = AuthInfo {
            token_field: String::new(),
            env_var: String::new(),
            gateway_url_field: "gateway".to_string(),
            gateway_env_var: "GW".to_string(),
        };
        assert!(!auth.has_token());
        assert!(auth.has_gateway());
    }

    #[test]
    fn resource_no_attributes() {
        let r = IacResource {
            name: "empty".to_string(),
            description: String::new(),
            category: "test".to_string(),
            crud: CrudInfo {
                create_endpoint: "/create".to_string(),
                create_schema: "Create".to_string(),
                update_endpoint: None,
                update_schema: None,
                read_endpoint: "/read".to_string(),
                read_schema: "Read".to_string(),
                read_response_schema: None,
                delete_endpoint: "/delete".to_string(),
                delete_schema: "Delete".to_string(),
            },
            attributes: vec![],
            identity: IdentityInfo {
                id_field: "id".to_string(),
                import_field: "id".to_string(),
                force_replace_fields: vec![],
            },
        };

        assert!(r.input_attributes().is_empty());
        assert!(r.output_attributes().is_empty());
        assert!(r.required_attribute_names().is_empty());
        assert!(r.sensitive_attribute_names().is_empty());
        assert!(r.immutable_attribute_names().is_empty());
    }

    #[test]
    fn data_source_no_attributes() {
        let ds = IacDataSource {
            name: "empty".to_string(),
            description: String::new(),
            read_endpoint: "/read".to_string(),
            read_schema: "Read".to_string(),
            read_response_schema: None,
            attributes: vec![],
        };

        assert!(ds.input_attributes().is_empty());
        assert!(ds.output_attributes().is_empty());
        assert!(ds.required_attribute_names().is_empty());
        assert!(ds.sensitive_attribute_names().is_empty());
        assert!(ds.computed_attribute_names().is_empty());
    }

    #[test]
    fn iac_type_display_nested() {
        // Nested types display correctly
        assert_eq!(
            IacType::List(Box::new(IacType::List(Box::new(IacType::String)))).to_string(),
            "list<list<string>>"
        );
        assert_eq!(
            IacType::Map(Box::new(IacType::List(Box::new(IacType::Integer)))).to_string(),
            "map<string, list<integer>>"
        );
    }

    #[test]
    fn iac_attribute_display_optional() {
        let attr = IacAttribute {
            api_name: "tags".to_string(),
            canonical_name: "tags".to_string(),
            description: String::new(),
            iac_type: IacType::List(Box::new(IacType::String)),
            required: false,
            optional: true,
            computed: false,
            sensitive: false,
            json_encoded: false,
            immutable: false,
            default_value: None,
            enum_values: None,
            read_path: None,
            update_only: false,
        };
        assert_eq!(attr.to_string(), "tags: list<string> (optional)");
    }

    #[test]
    fn iac_type_serde_roundtrip_all_variants() {
        let types = vec![
            IacType::String,
            IacType::Integer,
            IacType::Float,
            IacType::Numeric,
            IacType::Boolean,
            IacType::Any,
            IacType::List(Box::new(IacType::Integer)),
            IacType::Set(Box::new(IacType::Boolean)),
            IacType::Map(Box::new(IacType::Float)),
            IacType::Object {
                name: "Nested".to_string(),
                fields: vec![],
            },
            IacType::Enum {
                values: vec!["x".into(), "y".into()],
                underlying: Box::new(IacType::String),
            },
        ];
        for t in &types {
            let json = serde_json::to_string(t).expect("serialize");
            let roundtripped: IacType = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(*t, roundtripped, "roundtrip failed for {t}");
        }
    }

    #[test]
    fn iac_type_serde_roundtrip_deeply_nested() {
        let deep = IacType::List(Box::new(IacType::Map(Box::new(IacType::Set(Box::new(
            IacType::Object {
                name: "Inner".to_string(),
                fields: vec![
                    crate::testing::TestAttributeBuilder::new("f", IacType::String)
                        .required()
                        .build(),
                ],
            },
        ))))));
        let json = serde_json::to_string(&deep).expect("serialize");
        let roundtripped: IacType = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deep, roundtripped);
    }

    #[test]
    fn iac_attribute_serde_with_all_fields_set() {
        let attr = IacAttribute {
            api_name: "my-key".to_string(),
            canonical_name: "my_key".to_string(),
            description: "A key".to_string(),
            iac_type: IacType::String,
            required: true,
            optional: false,
            computed: true,
            sensitive: true,
            json_encoded: true,
            immutable: true,
            default_value: Some(serde_json::json!("default_val")),
            enum_values: Some(vec!["a".into(), "b".into()]),
            read_path: Some("response_key".to_string()),
            update_only: true,
        };
        let json = serde_json::to_string(&attr).expect("serialize");
        let roundtripped: IacAttribute = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(attr, roundtripped);
    }

    #[test]
    fn iac_attribute_json_encoded_default_false() {
        let json_str = r#"{
            "api_name":"f","canonical_name":"f","description":"","iac_type":"String",
            "required":false,"optional":false,"computed":false,"sensitive":false,
            "immutable":false,"default_value":null,"enum_values":null,
            "read_path":null,"update_only":false
        }"#;
        let attr: IacAttribute = serde_json::from_str(json_str).expect("deserialize");
        assert!(!attr.json_encoded, "json_encoded should default to false");
    }

    #[test]
    fn iac_attribute_optional_default_false() {
        let json_str = r#"{
            "api_name":"f","canonical_name":"f","description":"","iac_type":"String",
            "required":false,"computed":false,"sensitive":false,
            "immutable":false,"default_value":null,"enum_values":null,
            "read_path":null,"update_only":false
        }"#;
        let attr: IacAttribute = serde_json::from_str(json_str).expect("deserialize");
        assert!(!attr.optional, "optional should default to false");
    }

    #[test]
    fn resource_optional_computed_is_input() {
        use crate::testing::TestAttributeBuilder;
        let r = IacResource {
            name: "opt_comp".to_string(),
            description: String::new(),
            category: "test".to_string(),
            crud: CrudInfo {
                create_endpoint: "/create".to_string(),
                create_schema: "Create".to_string(),
                update_endpoint: None,
                update_schema: None,
                read_endpoint: "/read".to_string(),
                read_schema: "Read".to_string(),
                read_response_schema: None,
                delete_endpoint: "/delete".to_string(),
                delete_schema: "Delete".to_string(),
            },
            attributes: vec![
                TestAttributeBuilder::new("opt_comp_field", IacType::String)
                    .optional()
                    .computed()
                    .build(),
            ],
            identity: IdentityInfo {
                id_field: "id".to_string(),
                import_field: "id".to_string(),
                force_replace_fields: vec![],
            },
        };
        let inputs = r.input_attributes();
        assert_eq!(inputs.len(), 1, "optional+computed should be an input");
        assert_eq!(inputs[0].canonical_name, "opt_comp_field");
    }

    #[test]
    fn data_source_optional_computed_is_input() {
        use crate::testing::TestAttributeBuilder;
        let ds = IacDataSource {
            name: "opt_comp_ds".to_string(),
            description: String::new(),
            read_endpoint: "/read".to_string(),
            read_schema: "Read".to_string(),
            read_response_schema: None,
            attributes: vec![
                TestAttributeBuilder::new("opt_comp_field", IacType::String)
                    .optional()
                    .computed()
                    .build(),
            ],
        };
        let inputs = ds.input_attributes();
        assert_eq!(inputs.len(), 1, "optional+computed should be an input for data source");
    }

    #[test]
    fn iac_type_clone_equality() {
        let original = IacType::Object {
            name: "Config".to_string(),
            fields: vec![
                crate::testing::TestAttributeBuilder::new("k", IacType::String).build(),
            ],
        };
        let cloned = original.clone();
        assert_eq!(original, cloned);
    }

    #[test]
    fn crud_info_serde_roundtrip_with_optionals() {
        let crud = CrudInfo {
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
        let json = serde_json::to_string(&crud).expect("serialize");
        let rt: CrudInfo = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(rt.update_endpoint, Some("/update".to_string()));
        assert_eq!(rt.update_schema, Some("Update".to_string()));
        assert_eq!(rt.read_response_schema, Some("ReadResp".to_string()));
    }

    #[test]
    fn identity_info_serde_roundtrip() {
        let id = IdentityInfo {
            id_field: "name".to_string(),
            import_field: "path".to_string(),
            force_replace_fields: vec!["name".to_string(), "region".to_string()],
        };
        let json = serde_json::to_string(&id).expect("serialize");
        let rt: IdentityInfo = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(rt.id_field, "name");
        assert_eq!(rt.import_field, "path");
        assert_eq!(rt.force_replace_fields.len(), 2);
    }

    #[test]
    fn iac_provider_serialize_roundtrip() {
        let provider = IacProvider {
            name: "test".to_string(),
            description: "Test provider".to_string(),
            version: "1.0.0".to_string(),
            auth: AuthInfo {
                token_field: "token".to_string(),
                env_var: "TOKEN".to_string(),
                gateway_url_field: "gw".to_string(),
                gateway_env_var: "GW".to_string(),
            },
            skip_fields: vec!["token".to_string()],
            platform_config: {
                let mut m = BTreeMap::new();
                m.insert("terraform".to_string(), toml::Value::String("sdk".to_string()));
                m
            },
        };
        let json = serde_json::to_string(&provider).expect("serialize");
        let deserialized: IacProvider = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deserialized.name, "test");
        assert_eq!(deserialized.auth.token_field, "token");
        assert_eq!(deserialized.skip_fields, vec!["token"]);
    }

    #[test]
    fn iac_data_source_serialize_roundtrip() {
        use crate::testing::TestAttributeBuilder;
        let ds = IacDataSource {
            name: "ds".to_string(),
            description: "A data source".to_string(),
            read_endpoint: "/read".to_string(),
            read_schema: "Read".to_string(),
            read_response_schema: Some("ReadOutput".to_string()),
            attributes: vec![
                TestAttributeBuilder::new("key", IacType::String)
                    .required()
                    .build(),
            ],
        };
        let json = serde_json::to_string(&ds).expect("serialize");
        let deserialized: IacDataSource = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deserialized.name, "ds");
        assert_eq!(
            deserialized.read_response_schema,
            Some("ReadOutput".to_string())
        );
        assert_eq!(deserialized.attributes.len(), 1);
    }
}
