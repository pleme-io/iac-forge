use std::collections::HashMap;

/// Platform-independent type representation.
///
/// Richer than any single platform's type system — preserves Object structure
/// and Enum values needed by Pulumi schemas, Crossplane CRDs, etc.
#[derive(Debug, Clone, PartialEq)]
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
#[derive(Debug, Clone, PartialEq)]
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
    /// JSON path in API response for reading this field back.
    pub read_path: Option<String>,
}

/// CRUD endpoint information for a resource.
#[derive(Debug, Clone)]
pub struct CrudInfo {
    pub create_endpoint: String,
    pub create_schema: String,
    pub update_endpoint: Option<String>,
    pub update_schema: Option<String>,
    pub read_endpoint: String,
    pub read_schema: String,
    pub read_response_schema: Option<String>,
    pub delete_endpoint: String,
    pub delete_schema: String,
}

/// Identity and import configuration for a resource.
#[derive(Debug, Clone)]
pub struct IdentityInfo {
    pub id_field: String,
    pub import_field: String,
    pub force_replace_fields: Vec<String>,
}

/// A fully resolved resource in the platform-independent IR.
#[derive(Debug, Clone)]
pub struct IacResource {
    pub name: String,
    pub description: String,
    pub category: String,
    pub crud: CrudInfo,
    pub attributes: Vec<IacAttribute>,
    pub identity: IdentityInfo,
}

/// A fully resolved data source in the platform-independent IR.
#[derive(Debug, Clone)]
pub struct IacDataSource {
    pub name: String,
    pub description: String,
    pub read_endpoint: String,
    pub read_schema: String,
    pub read_response_schema: Option<String>,
    pub attributes: Vec<IacAttribute>,
}

/// Provider-level configuration in the platform-independent IR.
#[derive(Debug, Clone)]
pub struct IacProvider {
    pub name: String,
    pub description: String,
    pub version: String,
    pub auth: AuthInfo,
    pub skip_fields: Vec<String>,
    pub platform_config: HashMap<String, toml::Value>,
}

/// Authentication configuration for a provider.
#[derive(Debug, Clone, Default)]
pub struct AuthInfo {
    pub token_field: String,
    pub env_var: String,
    pub gateway_url_field: String,
    pub gateway_env_var: String,
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
}
