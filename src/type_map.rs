use openapi_forge::TypeInfo;

use crate::ir::IacType;

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
}
