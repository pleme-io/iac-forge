use std::collections::HashMap;

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
pub struct IacAttribute {
    /// Original API field name (e.g., "bound-aws-account-id").
    pub api_name: String,
    /// Normalized name with underscores (e.g., "bound_aws_account_id").
    pub canonical_name: String,
    /// Human-readable description.
    pub description: String,
    /// Platform-independent type.
    pub iac_type: IacType,
    /// Whether the field is required on create.
    pub required: bool,
    /// Whether the field is computed (server-side generated).
    pub computed: bool,
    /// Whether the field contains sensitive data.
    pub sensitive: bool,
    /// Whether changing this field forces resource replacement.
    pub immutable: bool,
    /// Default value, if any.
    pub default_value: Option<serde_json::Value>,
    /// Enum constraint values, if any.
    pub enum_values: Option<Vec<String>>,
    /// JSON path in API response for reading this field back
    /// (e.g., "item_name" maps to the API response key).
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
/// Each field maps to an API endpoint path and its corresponding OpenAPI schema
/// name used for request/response serialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrudInfo {
    /// API path for the create operation (e.g., "/create-secret").
    pub create_endpoint: String,
    /// OpenAPI schema name for the create request body.
    pub create_schema: String,
    /// API path for the update operation, if separate from create.
    pub update_endpoint: Option<String>,
    /// OpenAPI schema name for the update request body.
    pub update_schema: Option<String>,
    /// API path for the read operation.
    pub read_endpoint: String,
    /// OpenAPI schema name for the read request body.
    pub read_schema: String,
    /// OpenAPI schema name for the read response, if different from the request.
    pub read_response_schema: Option<String>,
    /// API path for the delete operation.
    pub delete_endpoint: String,
    /// OpenAPI schema name for the delete request body.
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

/// Provider-level configuration in the platform-independent IR.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IacProvider {
    pub name: String,
    pub description: String,
    pub version: String,
    pub auth: AuthInfo,
    pub skip_fields: Vec<String>,
    pub platform_config: HashMap<String, toml::Value>,
}

/// Authentication configuration for a provider.
///
/// Empty strings mean "not configured". Use the `has_token()` and
/// `has_gateway()` helpers to check.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthInfo {
    /// API field name for the authentication token (e.g., "token").
    pub token_field: String,
    /// Environment variable that supplies the token (e.g., "AKEYLESS_ACCESS_TOKEN").
    pub env_var: String,
    /// API field name for the gateway URL (e.g., "api_gateway_address").
    pub gateway_url_field: String,
    /// Environment variable that supplies the gateway URL (e.g., "AKEYLESS_GATEWAY").
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
            computed: false,
            sensitive: false,
            immutable: false,
            default_value: None,
            enum_values: None,
            read_path: None,
            update_only: false,
        };
        assert_eq!(attr.to_string(), "name: string (required)");

        let optional = IacAttribute {
            required: false,
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
                computed: false,
                sensitive: false,
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
}
