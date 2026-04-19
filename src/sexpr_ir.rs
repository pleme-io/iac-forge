//! `ToSExpr` / `FromSExpr` implementations for the IR value types.
//!
//! Separate file from [`crate::ir`] to keep the IR itself free of
//! serialization concerns and from [`crate::sexpr`] to keep the core
//! value + trait layer free of domain types. All impls here satisfy
//! the round-trip law `T::from_sexpr(&x.to_sexpr())? == x`, verified by
//! both concrete unit tests and proptest runs in
//! [`tests/sexpr_ir_round_trip.rs`].

use crate::ir::{
    AuthInfo, CrudInfo, IacAttribute, IacDataSource, IacProvider, IacResource, IacType,
    IdentityInfo,
};
use crate::sexpr::{FromSExpr, SExpr, SExprError, ToSExpr, parse_struct, struct_expr, take_field};

// ── IacType ─────────────────────────────────────────────────────────

impl ToSExpr for IacType {
    fn to_sexpr(&self) -> SExpr {
        match self {
            Self::String => SExpr::Symbol("string".into()),
            Self::Integer => SExpr::Symbol("integer".into()),
            Self::Float => SExpr::Symbol("float".into()),
            Self::Numeric => SExpr::Symbol("numeric".into()),
            Self::Boolean => SExpr::Symbol("boolean".into()),
            Self::Any => SExpr::Symbol("any".into()),
            Self::List(inner) => SExpr::List(vec![SExpr::Symbol("list".into()), inner.to_sexpr()]),
            Self::Set(inner) => SExpr::List(vec![SExpr::Symbol("set".into()), inner.to_sexpr()]),
            Self::Map(inner) => SExpr::List(vec![SExpr::Symbol("map".into()), inner.to_sexpr()]),
            Self::Object { name, fields } => struct_expr(
                "object",
                vec![("name", name.to_sexpr()), ("fields", fields.to_sexpr())],
            ),
            Self::Enum { values, underlying } => struct_expr(
                "enum",
                vec![
                    ("values", values.to_sexpr()),
                    ("underlying", underlying.to_sexpr()),
                ],
            ),
            other => panic!(
                "unsupported IacType variant in ToSExpr: {other:?} — add an explicit mapping"
            ),
        }
    }
}

impl FromSExpr for IacType {
    fn from_sexpr(s: &SExpr) -> Result<Self, SExprError> {
        // Unit variants: bare symbol.
        if let SExpr::Symbol(tag) = s {
            return match tag.as_str() {
                "string" => Ok(Self::String),
                "integer" => Ok(Self::Integer),
                "float" => Ok(Self::Float),
                "numeric" => Ok(Self::Numeric),
                "boolean" => Ok(Self::Boolean),
                "any" => Ok(Self::Any),
                other => Err(SExprError::UnknownVariant(format!("IacType::{other}"))),
            };
        }

        // Parameterized variants: list form.
        let items = s.as_list()?;
        let (head, rest) = items
            .split_first()
            .ok_or_else(|| SExprError::Shape("empty IacType list".into()))?;
        let tag = head.as_symbol()?;
        match tag {
            "list" | "set" | "map" => {
                if rest.len() != 1 {
                    return Err(SExprError::Shape(format!(
                        "IacType::{tag} expects 1 arg, got {}",
                        rest.len()
                    )));
                }
                let inner = Box::new(IacType::from_sexpr(&rest[0])?);
                Ok(match tag {
                    "list" => Self::List(inner),
                    "set" => Self::Set(inner),
                    "map" => Self::Map(inner),
                    _ => unreachable!(),
                })
            }
            "object" => {
                let fields = parse_struct(s, "object")?;
                Ok(Self::Object {
                    name: String::from_sexpr(take_field(&fields, "name")?)?,
                    fields: Vec::<IacAttribute>::from_sexpr(take_field(&fields, "fields")?)?,
                })
            }
            "enum" => {
                let fields = parse_struct(s, "enum")?;
                Ok(Self::Enum {
                    values: Vec::<String>::from_sexpr(take_field(&fields, "values")?)?,
                    underlying: Box::new(IacType::from_sexpr(take_field(&fields, "underlying")?)?),
                })
            }
            other => Err(SExprError::UnknownVariant(format!("IacType::{other}"))),
        }
    }
}

// ── serde_json::Value → SExpr bridge ────────────────────────────────
//
// IacAttribute carries an optional JSON default value. We embed it as a
// canonical JSON string wrapped in a `(json-value <str>)` tag so the
// surface stays human-readable and round-trips through serde_json.

fn json_to_sexpr(v: &serde_json::Value) -> SExpr {
    let encoded = serde_json::to_string(v).expect("serde_json::Value is serializable");
    SExpr::List(vec![
        SExpr::Symbol("json-value".into()),
        SExpr::String(encoded),
    ])
}

fn json_from_sexpr(s: &SExpr) -> Result<serde_json::Value, SExprError> {
    let items = s.as_list()?;
    let (head, rest) = items
        .split_first()
        .ok_or_else(|| SExprError::Shape("expected (json-value ...) form".into()))?;
    let tag = head.as_symbol()?;
    if tag != "json-value" {
        return Err(SExprError::Shape(format!(
            "expected 'json-value' tag, got '{tag}'"
        )));
    }
    if rest.len() != 1 {
        return Err(SExprError::Shape(format!(
            "json-value expects 1 arg, got {}",
            rest.len()
        )));
    }
    let encoded = rest[0].as_str()?;
    serde_json::from_str(encoded).map_err(|e| SExprError::Parse(format!("json: {e}")))
}

// ── IacAttribute ────────────────────────────────────────────────────

impl ToSExpr for IacAttribute {
    fn to_sexpr(&self) -> SExpr {
        struct_expr(
            "attribute",
            vec![
                ("api-name", self.api_name.to_sexpr()),
                ("canonical-name", self.canonical_name.to_sexpr()),
                ("description", self.description.to_sexpr()),
                ("iac-type", self.iac_type.to_sexpr()),
                ("required", self.required.to_sexpr()),
                ("optional", self.optional.to_sexpr()),
                ("computed", self.computed.to_sexpr()),
                ("sensitive", self.sensitive.to_sexpr()),
                ("json-encoded", self.json_encoded.to_sexpr()),
                ("immutable", self.immutable.to_sexpr()),
                (
                    "default-value",
                    self.default_value
                        .as_ref()
                        .map_or(SExpr::Nil, json_to_sexpr),
                ),
                ("enum-values", self.enum_values.to_sexpr()),
                ("read-path", self.read_path.to_sexpr()),
                ("update-only", self.update_only.to_sexpr()),
            ],
        )
    }
}

impl FromSExpr for IacAttribute {
    fn from_sexpr(s: &SExpr) -> Result<Self, SExprError> {
        let fields = parse_struct(s, "attribute")?;
        Ok(Self {
            api_name: String::from_sexpr(take_field(&fields, "api-name")?)?,
            canonical_name: String::from_sexpr(take_field(&fields, "canonical-name")?)?,
            description: String::from_sexpr(take_field(&fields, "description")?)?,
            iac_type: IacType::from_sexpr(take_field(&fields, "iac-type")?)?,
            required: bool::from_sexpr(take_field(&fields, "required")?)?,
            optional: bool::from_sexpr(take_field(&fields, "optional")?)?,
            computed: bool::from_sexpr(take_field(&fields, "computed")?)?,
            sensitive: bool::from_sexpr(take_field(&fields, "sensitive")?)?,
            json_encoded: bool::from_sexpr(take_field(&fields, "json-encoded")?)?,
            immutable: bool::from_sexpr(take_field(&fields, "immutable")?)?,
            default_value: match take_field(&fields, "default-value")? {
                SExpr::Nil => None,
                other => Some(json_from_sexpr(other)?),
            },
            enum_values: Option::<Vec<String>>::from_sexpr(take_field(&fields, "enum-values")?)?,
            read_path: Option::<String>::from_sexpr(take_field(&fields, "read-path")?)?,
            update_only: bool::from_sexpr(take_field(&fields, "update-only")?)?,
        })
    }
}

// ── CrudInfo ────────────────────────────────────────────────────────

impl ToSExpr for CrudInfo {
    fn to_sexpr(&self) -> SExpr {
        struct_expr(
            "crud",
            vec![
                ("create-endpoint", self.create_endpoint.to_sexpr()),
                ("create-schema", self.create_schema.to_sexpr()),
                ("update-endpoint", self.update_endpoint.to_sexpr()),
                ("update-schema", self.update_schema.to_sexpr()),
                ("read-endpoint", self.read_endpoint.to_sexpr()),
                ("read-schema", self.read_schema.to_sexpr()),
                ("read-response-schema", self.read_response_schema.to_sexpr()),
                ("delete-endpoint", self.delete_endpoint.to_sexpr()),
                ("delete-schema", self.delete_schema.to_sexpr()),
            ],
        )
    }
}

impl FromSExpr for CrudInfo {
    fn from_sexpr(s: &SExpr) -> Result<Self, SExprError> {
        let f = parse_struct(s, "crud")?;
        Ok(Self {
            create_endpoint: String::from_sexpr(take_field(&f, "create-endpoint")?)?,
            create_schema: String::from_sexpr(take_field(&f, "create-schema")?)?,
            update_endpoint: Option::<String>::from_sexpr(take_field(&f, "update-endpoint")?)?,
            update_schema: Option::<String>::from_sexpr(take_field(&f, "update-schema")?)?,
            read_endpoint: String::from_sexpr(take_field(&f, "read-endpoint")?)?,
            read_schema: String::from_sexpr(take_field(&f, "read-schema")?)?,
            read_response_schema: Option::<String>::from_sexpr(take_field(
                &f,
                "read-response-schema",
            )?)?,
            delete_endpoint: String::from_sexpr(take_field(&f, "delete-endpoint")?)?,
            delete_schema: String::from_sexpr(take_field(&f, "delete-schema")?)?,
        })
    }
}

// ── IdentityInfo ────────────────────────────────────────────────────

impl ToSExpr for IdentityInfo {
    fn to_sexpr(&self) -> SExpr {
        struct_expr(
            "identity",
            vec![
                ("id-field", self.id_field.to_sexpr()),
                ("import-field", self.import_field.to_sexpr()),
                ("force-replace-fields", self.force_replace_fields.to_sexpr()),
            ],
        )
    }
}

impl FromSExpr for IdentityInfo {
    fn from_sexpr(s: &SExpr) -> Result<Self, SExprError> {
        let f = parse_struct(s, "identity")?;
        Ok(Self {
            id_field: String::from_sexpr(take_field(&f, "id-field")?)?,
            import_field: String::from_sexpr(take_field(&f, "import-field")?)?,
            force_replace_fields: Vec::<String>::from_sexpr(take_field(
                &f,
                "force-replace-fields",
            )?)?,
        })
    }
}

// ── AuthInfo ────────────────────────────────────────────────────────

impl ToSExpr for AuthInfo {
    fn to_sexpr(&self) -> SExpr {
        struct_expr(
            "auth",
            vec![
                ("token-field", self.token_field.to_sexpr()),
                ("env-var", self.env_var.to_sexpr()),
                ("gateway-url-field", self.gateway_url_field.to_sexpr()),
                ("gateway-env-var", self.gateway_env_var.to_sexpr()),
            ],
        )
    }
}

impl FromSExpr for AuthInfo {
    fn from_sexpr(s: &SExpr) -> Result<Self, SExprError> {
        let f = parse_struct(s, "auth")?;
        Ok(Self {
            token_field: String::from_sexpr(take_field(&f, "token-field")?)?,
            env_var: String::from_sexpr(take_field(&f, "env-var")?)?,
            gateway_url_field: String::from_sexpr(take_field(&f, "gateway-url-field")?)?,
            gateway_env_var: String::from_sexpr(take_field(&f, "gateway-env-var")?)?,
        })
    }
}

// ── IacResource ─────────────────────────────────────────────────────

impl ToSExpr for IacResource {
    fn to_sexpr(&self) -> SExpr {
        struct_expr(
            "resource",
            vec![
                ("name", self.name.to_sexpr()),
                ("description", self.description.to_sexpr()),
                ("category", self.category.to_sexpr()),
                ("crud", self.crud.to_sexpr()),
                ("attributes", self.attributes.to_sexpr()),
                ("identity", self.identity.to_sexpr()),
            ],
        )
    }
}

impl FromSExpr for IacResource {
    fn from_sexpr(s: &SExpr) -> Result<Self, SExprError> {
        let f = parse_struct(s, "resource")?;
        Ok(Self {
            name: String::from_sexpr(take_field(&f, "name")?)?,
            description: String::from_sexpr(take_field(&f, "description")?)?,
            category: String::from_sexpr(take_field(&f, "category")?)?,
            crud: CrudInfo::from_sexpr(take_field(&f, "crud")?)?,
            attributes: Vec::<IacAttribute>::from_sexpr(take_field(&f, "attributes")?)?,
            identity: IdentityInfo::from_sexpr(take_field(&f, "identity")?)?,
        })
    }
}

// ── IacDataSource ───────────────────────────────────────────────────

impl ToSExpr for IacDataSource {
    fn to_sexpr(&self) -> SExpr {
        struct_expr(
            "data-source",
            vec![
                ("name", self.name.to_sexpr()),
                ("description", self.description.to_sexpr()),
                ("read-endpoint", self.read_endpoint.to_sexpr()),
                ("read-schema", self.read_schema.to_sexpr()),
                ("read-response-schema", self.read_response_schema.to_sexpr()),
                ("attributes", self.attributes.to_sexpr()),
            ],
        )
    }
}

impl FromSExpr for IacDataSource {
    fn from_sexpr(s: &SExpr) -> Result<Self, SExprError> {
        let f = parse_struct(s, "data-source")?;
        Ok(Self {
            name: String::from_sexpr(take_field(&f, "name")?)?,
            description: String::from_sexpr(take_field(&f, "description")?)?,
            read_endpoint: String::from_sexpr(take_field(&f, "read-endpoint")?)?,
            read_schema: String::from_sexpr(take_field(&f, "read-schema")?)?,
            read_response_schema: Option::<String>::from_sexpr(take_field(
                &f,
                "read-response-schema",
            )?)?,
            attributes: Vec::<IacAttribute>::from_sexpr(take_field(&f, "attributes")?)?,
        })
    }
}

// ── IacProvider ─────────────────────────────────────────────────────
//
// `platform_config` is a BTreeMap<String, toml::Value>. We encode it as
// a canonical TOML string wrapped in `(toml-map <str>)` to preserve the
// nested structure without losing type info. TOML is already canonical
// so round-trip is deterministic.

fn toml_map_to_sexpr(map: &std::collections::BTreeMap<String, toml::Value>) -> SExpr {
    let as_table: toml::Table = map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    let encoded = toml::to_string(&as_table).expect("toml::Table is serializable");
    SExpr::List(vec![
        SExpr::Symbol("toml-map".into()),
        SExpr::String(encoded),
    ])
}

fn toml_map_from_sexpr(
    s: &SExpr,
) -> Result<std::collections::BTreeMap<String, toml::Value>, SExprError> {
    let items = s.as_list()?;
    let (head, rest) = items
        .split_first()
        .ok_or_else(|| SExprError::Shape("expected (toml-map ...) form".into()))?;
    let tag = head.as_symbol()?;
    if tag != "toml-map" {
        return Err(SExprError::Shape(format!(
            "expected 'toml-map' tag, got '{tag}'"
        )));
    }
    if rest.len() != 1 {
        return Err(SExprError::Shape(format!(
            "toml-map expects 1 arg, got {}",
            rest.len()
        )));
    }
    let encoded = rest[0].as_str()?;
    let table: toml::Table =
        toml::from_str(encoded).map_err(|e| SExprError::Parse(format!("toml: {e}")))?;
    Ok(table.into_iter().collect())
}

impl ToSExpr for IacProvider {
    fn to_sexpr(&self) -> SExpr {
        struct_expr(
            "provider",
            vec![
                ("name", self.name.to_sexpr()),
                ("description", self.description.to_sexpr()),
                ("version", self.version.to_sexpr()),
                ("auth", self.auth.to_sexpr()),
                ("skip-fields", self.skip_fields.to_sexpr()),
                ("platform-config", toml_map_to_sexpr(&self.platform_config)),
            ],
        )
    }
}

impl FromSExpr for IacProvider {
    fn from_sexpr(s: &SExpr) -> Result<Self, SExprError> {
        let f = parse_struct(s, "provider")?;
        Ok(Self {
            name: String::from_sexpr(take_field(&f, "name")?)?,
            description: String::from_sexpr(take_field(&f, "description")?)?,
            version: String::from_sexpr(take_field(&f, "version")?)?,
            auth: AuthInfo::from_sexpr(take_field(&f, "auth")?)?,
            skip_fields: Vec::<String>::from_sexpr(take_field(&f, "skip-fields")?)?,
            platform_config: toml_map_from_sexpr(take_field(&f, "platform-config")?)?,
        })
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{TestAttributeBuilder, test_provider, test_resource};

    // ── IacType ─────────────────────────────────────────────────

    fn roundtrip_type(ty: &IacType) {
        let s = ty.to_sexpr();
        let parsed = IacType::from_sexpr(&s).expect("parse");
        assert_eq!(&parsed, ty, "round-trip failed for {ty:?}");
        // Also: emit → parse → roundtrip.
        let emitted = s.emit();
        let reparsed_sexpr = SExpr::parse(&emitted).expect("parse emitted");
        let reparsed = IacType::from_sexpr(&reparsed_sexpr).expect("from emitted");
        assert_eq!(&reparsed, ty, "emit/parse round-trip failed for {ty:?}");
    }

    #[test]
    fn round_trip_scalar_types() {
        for ty in [
            IacType::String,
            IacType::Integer,
            IacType::Float,
            IacType::Numeric,
            IacType::Boolean,
            IacType::Any,
        ] {
            roundtrip_type(&ty);
        }
    }

    #[test]
    fn round_trip_list_of_string() {
        roundtrip_type(&IacType::List(Box::new(IacType::String)));
    }

    #[test]
    fn round_trip_set_of_integer() {
        roundtrip_type(&IacType::Set(Box::new(IacType::Integer)));
    }

    #[test]
    fn round_trip_map_of_bool() {
        roundtrip_type(&IacType::Map(Box::new(IacType::Boolean)));
    }

    #[test]
    fn round_trip_nested_list() {
        roundtrip_type(&IacType::List(Box::new(IacType::List(Box::new(
            IacType::List(Box::new(IacType::Integer)),
        )))));
    }

    #[test]
    fn round_trip_enum_with_values() {
        roundtrip_type(&IacType::Enum {
            values: vec!["tcp".into(), "udp".into(), "icmp".into()],
            underlying: Box::new(IacType::String),
        });
    }

    #[test]
    fn round_trip_empty_enum() {
        roundtrip_type(&IacType::Enum {
            values: vec![],
            underlying: Box::new(IacType::String),
        });
    }

    #[test]
    fn round_trip_object_with_fields() {
        let attr = TestAttributeBuilder::new("inner", IacType::String)
            .required()
            .build();
        roundtrip_type(&IacType::Object {
            name: "Inner".into(),
            fields: vec![attr],
        });
    }

    #[test]
    fn from_sexpr_rejects_unknown_variant() {
        let err = IacType::from_sexpr(&SExpr::Symbol("nope".into())).unwrap_err();
        assert!(matches!(err, SExprError::UnknownVariant(_)));
    }

    #[test]
    fn from_sexpr_rejects_wrong_arity_list() {
        // (list) with no inner
        let s = SExpr::List(vec![SExpr::Symbol("list".into())]);
        let err = IacType::from_sexpr(&s).unwrap_err();
        assert!(matches!(err, SExprError::Shape(_)));
    }

    // ── IacAttribute ───────────────────────────────────────────

    fn sample_attr() -> IacAttribute {
        TestAttributeBuilder::new("bound-secret", IacType::String)
            .required()
            .immutable()
            .sensitive()
            .description("a secret")
            .build()
    }

    #[test]
    fn round_trip_attribute() {
        let a = sample_attr();
        let s = a.to_sexpr();
        let parsed = IacAttribute::from_sexpr(&s).expect("parse");
        assert_eq!(parsed, a);
    }

    #[test]
    fn round_trip_attribute_through_emit() {
        let a = sample_attr();
        let emitted = a.to_sexpr().emit();
        let parsed = IacAttribute::from_sexpr(&SExpr::parse(&emitted).unwrap()).unwrap();
        assert_eq!(parsed, a);
    }

    #[test]
    fn round_trip_attribute_with_default_value() {
        let attr = TestAttributeBuilder::new("note", IacType::String)
            .default_value(serde_json::json!("hello"))
            .build();
        let parsed = IacAttribute::from_sexpr(&attr.to_sexpr()).expect("parse");
        assert_eq!(parsed, attr);
    }

    #[test]
    fn round_trip_attribute_with_enum_values_and_read_path() {
        let attr = TestAttributeBuilder::new("protocol", IacType::String)
            .enum_values(vec!["tcp".into(), "udp".into()])
            .read_path("resp_path")
            .build();
        let parsed = IacAttribute::from_sexpr(&attr.to_sexpr()).expect("parse");
        assert_eq!(parsed, attr);
    }

    // ── IacResource ────────────────────────────────────────────

    #[test]
    fn round_trip_resource() {
        let r = test_resource("widget");
        let parsed = IacResource::from_sexpr(&r.to_sexpr()).expect("parse");
        assert_eq!(parsed.name, r.name);
        assert_eq!(parsed.description, r.description);
        assert_eq!(parsed.attributes, r.attributes);
        assert_eq!(parsed.identity.id_field, r.identity.id_field);
        assert_eq!(parsed.crud.create_endpoint, r.crud.create_endpoint);
    }

    #[test]
    fn round_trip_resource_through_emit() {
        let r = test_resource("widget");
        let emitted = r.to_sexpr().emit();
        let parsed = IacResource::from_sexpr(&SExpr::parse(&emitted).unwrap()).expect("parse");
        assert_eq!(parsed.name, r.name);
        assert_eq!(parsed.attributes, r.attributes);
    }

    // ── IacProvider ────────────────────────────────────────────

    #[test]
    fn round_trip_provider() {
        let p = test_provider("acme");
        let parsed = IacProvider::from_sexpr(&p.to_sexpr()).expect("parse");
        assert_eq!(parsed.name, p.name);
        assert_eq!(parsed.auth.token_field, p.auth.token_field);
        assert_eq!(parsed.platform_config, p.platform_config);
    }

    #[test]
    fn round_trip_provider_through_emit() {
        let p = test_provider("acme");
        let emitted = p.to_sexpr().emit();
        let parsed = IacProvider::from_sexpr(&SExpr::parse(&emitted).unwrap()).expect("parse");
        assert_eq!(parsed.name, p.name);
        assert_eq!(parsed.auth.token_field, p.auth.token_field);
    }

    // ── Canonical form stability ───────────────────────────────

    #[test]
    fn emitted_form_is_deterministic() {
        let r = test_resource("widget");
        let a = r.to_sexpr().emit();
        let b = r.to_sexpr().emit();
        assert_eq!(a, b);
    }

    #[test]
    fn parse_is_inverse_of_emit_on_all_types() {
        let types = vec![
            IacType::String,
            IacType::Integer,
            IacType::List(Box::new(IacType::Boolean)),
            IacType::Enum {
                values: vec!["a".into(), "b".into()],
                underlying: Box::new(IacType::String),
            },
            IacType::Map(Box::new(IacType::List(Box::new(IacType::String)))),
        ];
        for ty in types {
            let emitted = ty.to_sexpr().emit();
            let parsed = IacType::from_sexpr(&SExpr::parse(&emitted).unwrap()).unwrap();
            assert_eq!(parsed, ty);
        }
    }
}
