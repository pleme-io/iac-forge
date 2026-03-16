use openapi_forge::TypeInfo;

use crate::ir::IacType;

/// Known type override strings for TOML field specs.
const KNOWN_TYPE_OVERRIDES: &[&str] = &[
    "bool", "boolean", "int", "int64", "integer", "float", "float64", "number", "string", "list",
];

/// Check whether a type override string is a recognized built-in type.
///
/// Unknown overrides are treated as Object type names, which is valid but
/// might indicate a typo. Call this during validation to warn users.
#[must_use]
pub fn is_valid_type_override(s: &str) -> bool {
    KNOWN_TYPE_OVERRIDES.contains(&s)
}

/// Map an OpenAPI type to a platform-independent `IacType`.
///
/// Respects `type_override` from TOML field specs.
#[must_use]
pub fn openapi_to_iac(type_info: &TypeInfo, type_override: Option<&str>) -> IacType {
    if let Some(override_str) = type_override {
        return match override_str {
            "bool" | "boolean" => IacType::Boolean,
            "int" | "int64" | "integer" => IacType::Integer,
            "float" | "float64" | "number" => IacType::Float,
            "string" => IacType::String,
            "list" => IacType::List(Box::new(IacType::String)),
            other => IacType::Object {
                name: other.to_string(),
                fields: vec![],
            },
        };
    }

    match type_info {
        TypeInfo::String => IacType::String,
        TypeInfo::Integer => IacType::Integer,
        TypeInfo::Number => IacType::Float,
        TypeInfo::Boolean => IacType::Boolean,
        TypeInfo::Array(inner) => IacType::List(Box::new(openapi_to_iac(inner, None))),
        TypeInfo::Map(inner) => IacType::Map(Box::new(openapi_to_iac(inner, None))),
        TypeInfo::Object(name) => IacType::Object {
            name: name.clone(),
            fields: vec![],
        },
        TypeInfo::Any => IacType::Any,
    }
}

/// Wrap an `IacType` with enum constraint if enum values are present.
#[must_use]
pub fn apply_enum_constraint(iac_type: IacType, enum_values: &Option<Vec<String>>) -> IacType {
    match enum_values {
        Some(values) if !values.is_empty() => IacType::Enum {
            values: values.clone(),
            underlying: Box::new(iac_type),
        },
        _ => iac_type,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_type_mapping() {
        assert_eq!(openapi_to_iac(&TypeInfo::String, None), IacType::String);
        assert_eq!(openapi_to_iac(&TypeInfo::Integer, None), IacType::Integer);
        assert_eq!(openapi_to_iac(&TypeInfo::Number, None), IacType::Float);
        assert_eq!(openapi_to_iac(&TypeInfo::Boolean, None), IacType::Boolean);
        assert_eq!(openapi_to_iac(&TypeInfo::Any, None), IacType::Any);
    }

    #[test]
    fn array_type_mapping() {
        assert_eq!(
            openapi_to_iac(&TypeInfo::Array(Box::new(TypeInfo::String)), None),
            IacType::List(Box::new(IacType::String))
        );
        assert_eq!(
            openapi_to_iac(&TypeInfo::Array(Box::new(TypeInfo::Integer)), None),
            IacType::List(Box::new(IacType::Integer))
        );
    }

    #[test]
    fn map_type_mapping() {
        assert_eq!(
            openapi_to_iac(&TypeInfo::Map(Box::new(TypeInfo::String)), None),
            IacType::Map(Box::new(IacType::String))
        );
    }

    #[test]
    fn object_type_mapping() {
        assert_eq!(
            openapi_to_iac(&TypeInfo::Object("User".to_string()), None),
            IacType::Object {
                name: "User".to_string(),
                fields: vec![]
            }
        );
    }

    #[test]
    fn type_override_bool() {
        assert_eq!(
            openapi_to_iac(&TypeInfo::String, Some("bool")),
            IacType::Boolean
        );
        assert_eq!(
            openapi_to_iac(&TypeInfo::String, Some("boolean")),
            IacType::Boolean
        );
    }

    #[test]
    fn type_override_int() {
        assert_eq!(
            openapi_to_iac(&TypeInfo::String, Some("int64")),
            IacType::Integer
        );
        assert_eq!(
            openapi_to_iac(&TypeInfo::String, Some("integer")),
            IacType::Integer
        );
    }

    #[test]
    fn type_override_list() {
        assert_eq!(
            openapi_to_iac(&TypeInfo::String, Some("list")),
            IacType::List(Box::new(IacType::String))
        );
    }

    #[test]
    fn enum_constraint() {
        let base = IacType::String;
        let values = Some(vec!["a".to_string(), "b".to_string()]);
        let result = apply_enum_constraint(base, &values);
        assert_eq!(
            result,
            IacType::Enum {
                values: vec!["a".to_string(), "b".to_string()],
                underlying: Box::new(IacType::String)
            }
        );
    }

    #[test]
    fn enum_constraint_empty() {
        let base = IacType::String;
        let result = apply_enum_constraint(base.clone(), &None);
        assert_eq!(result, base);

        let result = apply_enum_constraint(base.clone(), &Some(vec![]));
        assert_eq!(result, base);
    }

    #[test]
    fn valid_type_overrides() {
        assert!(is_valid_type_override("bool"));
        assert!(is_valid_type_override("boolean"));
        assert!(is_valid_type_override("int"));
        assert!(is_valid_type_override("int64"));
        assert!(is_valid_type_override("integer"));
        assert!(is_valid_type_override("float"));
        assert!(is_valid_type_override("float64"));
        assert!(is_valid_type_override("number"));
        assert!(is_valid_type_override("string"));
        assert!(is_valid_type_override("list"));
    }

    #[test]
    fn invalid_type_overrides() {
        assert!(!is_valid_type_override("CustomObject"));
        assert!(!is_valid_type_override(""));
        assert!(!is_valid_type_override("BOOL"));
        assert!(!is_valid_type_override("map"));
    }

    #[test]
    fn nested_array_of_array_of_string() {
        let inner = TypeInfo::Array(Box::new(TypeInfo::String));
        let outer = TypeInfo::Array(Box::new(inner));
        assert_eq!(
            openapi_to_iac(&outer, None),
            IacType::List(Box::new(IacType::List(Box::new(IacType::String))))
        );
    }

    #[test]
    fn map_of_integer() {
        assert_eq!(
            openapi_to_iac(&TypeInfo::Map(Box::new(TypeInfo::Integer)), None),
            IacType::Map(Box::new(IacType::Integer))
        );
    }

    #[test]
    fn map_of_boolean() {
        assert_eq!(
            openapi_to_iac(&TypeInfo::Map(Box::new(TypeInfo::Boolean)), None),
            IacType::Map(Box::new(IacType::Boolean))
        );
    }

    #[test]
    fn object_with_name() {
        assert_eq!(
            openapi_to_iac(&TypeInfo::Object("Configuration".to_string()), None),
            IacType::Object {
                name: "Configuration".to_string(),
                fields: vec![]
            }
        );
    }

    #[test]
    fn type_override_float64() {
        assert_eq!(
            openapi_to_iac(&TypeInfo::String, Some("float64")),
            IacType::Float
        );
    }

    #[test]
    fn type_override_number() {
        assert_eq!(
            openapi_to_iac(&TypeInfo::String, Some("number")),
            IacType::Float
        );
    }

    #[test]
    fn type_override_int_alias() {
        assert_eq!(
            openapi_to_iac(&TypeInfo::String, Some("int")),
            IacType::Integer
        );
    }

    #[test]
    fn type_override_float() {
        assert_eq!(
            openapi_to_iac(&TypeInfo::String, Some("float")),
            IacType::Float
        );
    }

    #[test]
    fn type_override_string() {
        // Even when the base type is Integer, "string" override forces String
        assert_eq!(
            openapi_to_iac(&TypeInfo::Integer, Some("string")),
            IacType::String
        );
    }

    #[test]
    fn type_override_unknown_produces_object() {
        assert_eq!(
            openapi_to_iac(&TypeInfo::String, Some("CustomThing")),
            IacType::Object {
                name: "CustomThing".to_string(),
                fields: vec![]
            }
        );
    }

    #[test]
    fn enum_constraint_on_integer() {
        let base = IacType::Integer;
        let values = Some(vec!["1".to_string(), "2".to_string()]);
        let result = apply_enum_constraint(base, &values);
        assert_eq!(
            result,
            IacType::Enum {
                values: vec!["1".to_string(), "2".to_string()],
                underlying: Box::new(IacType::Integer)
            }
        );
    }

    #[test]
    fn enum_constraint_on_boolean() {
        let base = IacType::Boolean;
        let values = Some(vec!["true".to_string(), "false".to_string()]);
        let result = apply_enum_constraint(base, &values);
        assert_eq!(
            result,
            IacType::Enum {
                values: vec!["true".to_string(), "false".to_string()],
                underlying: Box::new(IacType::Boolean)
            }
        );
    }

    #[test]
    fn enum_constraint_on_float() {
        let base = IacType::Float;
        let values = Some(vec!["1.0".to_string(), "2.5".to_string()]);
        let result = apply_enum_constraint(base, &values);
        assert_eq!(
            result,
            IacType::Enum {
                values: vec!["1.0".to_string(), "2.5".to_string()],
                underlying: Box::new(IacType::Float)
            }
        );
    }

    #[test]
    fn enum_constraint_on_list() {
        let base = IacType::List(Box::new(IacType::String));
        let values = Some(vec!["x".to_string()]);
        let result = apply_enum_constraint(base.clone(), &values);
        assert_eq!(
            result,
            IacType::Enum {
                values: vec!["x".to_string()],
                underlying: Box::new(base)
            }
        );
    }

    #[test]
    fn array_of_integer() {
        assert_eq!(
            openapi_to_iac(&TypeInfo::Array(Box::new(TypeInfo::Integer)), None),
            IacType::List(Box::new(IacType::Integer))
        );
    }

    #[test]
    fn array_of_boolean() {
        assert_eq!(
            openapi_to_iac(&TypeInfo::Array(Box::new(TypeInfo::Boolean)), None),
            IacType::List(Box::new(IacType::Boolean))
        );
    }

    #[test]
    fn array_of_object() {
        assert_eq!(
            openapi_to_iac(
                &TypeInfo::Array(Box::new(TypeInfo::Object("Item".to_string()))),
                None
            ),
            IacType::List(Box::new(IacType::Object {
                name: "Item".to_string(),
                fields: vec![]
            }))
        );
    }

    #[test]
    fn map_of_number() {
        assert_eq!(
            openapi_to_iac(&TypeInfo::Map(Box::new(TypeInfo::Number)), None),
            IacType::Map(Box::new(IacType::Float))
        );
    }

    #[test]
    fn type_override_takes_precedence_over_type_info() {
        // Even with Boolean type_info, "string" override wins
        assert_eq!(
            openapi_to_iac(&TypeInfo::Boolean, Some("string")),
            IacType::String
        );
        // Even with Integer type_info, "bool" override wins
        assert_eq!(
            openapi_to_iac(&TypeInfo::Integer, Some("bool")),
            IacType::Boolean
        );
    }
}
