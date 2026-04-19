//! Sexpr ↔ Nix bridge.
//!
//! Every [`SExpr`][crate::sexpr::SExpr] value can be expressed as a
//! [`NixValue`] (which renders to a valid Nix expression literal) and
//! vice versa. This makes Nix a first-class authoring surface for IR
//! values: a flake can write `{ name = "vpc"; cidr = "10.0.0.0/8"; }`,
//! convert it to SExpr, hash or transform it, then convert back.
//!
//! # Value correspondence
//!
//! | SExpr                         | NixValue                          |
//! |-------------------------------|-----------------------------------|
//! | `Integer(i)`                  | `Int(i)`                          |
//! | `Float(f)`                    | `Float(f)`                        |
//! | `Bool(b)`                     | `Bool(b)`                         |
//! | `Nil`                         | `Null`                            |
//! | `String(s)`                   | `Str(s)`                          |
//! | `Symbol(s)` (no colon prefix) | `Ident(s)`                        |
//! | `Symbol(":field")`            | *(only valid in a struct-form)*   |
//! | `List(items)` (struct-form)   | `AttrSet(BTreeMap)`               |
//! | `List(items)` (plain)         | `List(Vec)`                       |
//!
//! The hairy case is telling a **struct-form** (an attribute-set pattern)
//! apart from a **plain list** at the SExpr layer: they're both `SExpr::List`.
//! We use the same heuristic as [`crate::sexpr_diff`]: if the head is a
//! non-colon symbol and every tail item is a 2-element list starting with
//! `:keyword`, it's a struct-form; otherwise it's a plain list.
//!
//! # Round-trip
//!
//! The round-trip law:
//!
//! ```text
//! NixValue::from_sexpr(&NixValue::from_sexpr_round_trip(x).to_sexpr_root())? ≈ x
//! ```
//!
//! Note "≈" not "==": Nix can't distinguish `(foo)` (a tuple-tag list with
//! no args) from `foo` (a bare symbol) cleanly — both round-trip through
//! identifier+null. The lossless layer is sexpr itself; Nix is the
//! authoring convenience on top.

use std::collections::BTreeMap;

use crate::sexpr::SExpr;

/// A minimal Nix value representation — enough to round-trip any SExpr
/// value through a Nix expression literal.
#[derive(Debug, Clone, PartialEq)]
pub enum NixValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Null,
    /// Bare identifier. In Nix expressions this is a variable reference;
    /// we use it to encode sexpr symbols.
    Ident(String),
    List(Vec<NixValue>),
    /// Attribute set. BTreeMap for deterministic iteration order.
    AttrSet(BTreeMap<String, NixValue>),
}

impl NixValue {
    /// Render to a Nix expression literal.
    #[must_use]
    pub fn to_nix_expr(&self) -> String {
        let mut out = String::new();
        self.write_into(&mut out, 0);
        out
    }

    fn write_into(&self, out: &mut String, indent: usize) {
        match self {
            Self::Int(i) => out.push_str(&i.to_string()),
            Self::Float(f) => {
                let s = f.to_string();
                if s.contains('.') || s.contains('e') || s.contains('E') {
                    out.push_str(&s);
                } else {
                    out.push_str(&s);
                    out.push_str(".0");
                }
            }
            Self::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Self::Null => out.push_str("null"),
            Self::Str(s) => {
                out.push('"');
                for c in s.chars() {
                    match c {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\t' => out.push_str("\\t"),
                        '$' => {
                            // Guard against accidental string interpolation.
                            out.push_str("\\$");
                        }
                        c => out.push(c),
                    }
                }
                out.push('"');
            }
            Self::Ident(name) => out.push_str(name),
            Self::List(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(' ');
                    }
                    item.write_into(out, indent);
                }
                out.push(']');
            }
            Self::AttrSet(entries) => {
                if entries.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push_str("{ ");
                for (i, (k, v)) in entries.iter().enumerate() {
                    if i > 0 {
                        out.push(' ');
                    }
                    // Quote keys only when they contain non-identifier chars.
                    if is_simple_ident(k) {
                        out.push_str(k);
                    } else {
                        out.push('"');
                        out.push_str(&k.replace('"', "\\\""));
                        out.push('"');
                    }
                    out.push_str(" = ");
                    v.write_into(out, indent);
                    out.push(';');
                }
                out.push_str(" }");
            }
        }
    }
}

fn is_simple_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

// ── Sexpr → NixValue ────────────────────────────────────────────────

impl NixValue {
    /// Convert a canonical [`SExpr`] value into a [`NixValue`].
    ///
    /// Struct-form sexprs `(head (:field val) ...)` become attribute
    /// sets `{ head = "head"; field = val; ... }` with an explicit
    /// `head` key preserving the tag. Plain lists become Nix lists.
    /// Bare symbols become `Ident(name)`.
    #[must_use]
    pub fn from_sexpr(s: &SExpr) -> Self {
        match s {
            SExpr::Integer(i) => Self::Int(*i),
            SExpr::Float(f) => Self::Float(*f),
            SExpr::Bool(b) => Self::Bool(*b),
            SExpr::Nil => Self::Null,
            SExpr::String(s) => Self::Str(s.clone()),
            SExpr::Symbol(name) => Self::Ident(name.clone()),
            SExpr::List(items) => {
                if let Some(attrset) = struct_form_to_attrset(items) {
                    attrset
                } else {
                    Self::List(items.iter().map(Self::from_sexpr).collect())
                }
            }
        }
    }

    /// Convert back to an [`SExpr`]. The round-trip is lossless for
    /// values originally produced by [`Self::from_sexpr`].
    ///
    /// Disambiguation rules:
    /// - AttrSet with a `head` key → struct-form `(head (:k v) ...)`
    /// - AttrSet without a `head` key → still struct-form with synthetic
    ///   head `attrs`
    /// - List → `(list ...)` if it looks like a `Vec<T>` (first item is
    ///   the symbol `list`), else a tuple-tag list with the first element
    ///   as the head.
    #[must_use]
    pub fn to_sexpr(&self) -> SExpr {
        match self {
            Self::Int(i) => SExpr::Integer(*i),
            Self::Float(f) => SExpr::Float(*f),
            Self::Bool(b) => SExpr::Bool(*b),
            Self::Null => SExpr::Nil,
            Self::Str(s) => SExpr::String(s.clone()),
            Self::Ident(name) => SExpr::Symbol(name.clone()),
            Self::List(items) => SExpr::List(items.iter().map(Self::to_sexpr).collect()),
            Self::AttrSet(entries) => {
                let head = entries
                    .get("head")
                    .and_then(|v| match v {
                        Self::Str(s) | Self::Ident(s) => Some(s.clone()),
                        _ => None,
                    })
                    .unwrap_or_else(|| "attrs".to_string());
                let mut out = vec![SExpr::Symbol(head.clone())];
                for (k, v) in entries {
                    if k == "head" {
                        continue;
                    }
                    out.push(SExpr::List(vec![
                        SExpr::Symbol(format!(":{k}")),
                        v.to_sexpr(),
                    ]));
                }
                SExpr::List(out)
            }
        }
    }
}

/// If `items` looks like a struct-form `(head (:field val) ...)`,
/// convert to an AttrSet preserving the head as a `head` key.
/// Otherwise return None.
fn struct_form_to_attrset(items: &[SExpr]) -> Option<NixValue> {
    let (head, rest) = items.split_first()?;
    let head_name = match head {
        SExpr::Symbol(s) if !s.starts_with(':') => s.clone(),
        _ => return None,
    };
    if rest.is_empty() {
        return None;
    }
    let mut entries: BTreeMap<String, NixValue> = BTreeMap::new();
    for item in rest {
        let pair = match item {
            SExpr::List(pair) => pair,
            _ => return None,
        };
        if pair.len() != 2 {
            return None;
        }
        let key = match &pair[0] {
            SExpr::Symbol(k) if k.starts_with(':') => k[1..].to_string(),
            _ => return None,
        };
        entries.insert(key, NixValue::from_sexpr(&pair[1]));
    }
    entries.insert("head".into(), NixValue::Str(head_name));
    Some(NixValue::AttrSet(entries))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::IacType;
    use crate::sexpr::ToSExpr;

    // ── Scalars ───────────────────────────────────────────────

    #[test]
    fn int_roundtrip() {
        let s = SExpr::Integer(42);
        assert_eq!(NixValue::from_sexpr(&s).to_sexpr(), s);
    }

    #[test]
    fn bool_roundtrip() {
        assert_eq!(
            NixValue::from_sexpr(&SExpr::Bool(true)).to_sexpr(),
            SExpr::Bool(true)
        );
    }

    #[test]
    fn nil_roundtrip() {
        assert_eq!(NixValue::from_sexpr(&SExpr::Nil).to_sexpr(), SExpr::Nil);
    }

    #[test]
    fn string_roundtrip() {
        let s = SExpr::String("hello world".into());
        assert_eq!(NixValue::from_sexpr(&s).to_sexpr(), s);
    }

    #[test]
    fn symbol_roundtrip() {
        let s = SExpr::Symbol("string".into());
        assert_eq!(NixValue::from_sexpr(&s).to_sexpr(), s);
    }

    // ── Struct-form ↔ AttrSet ────────────────────────────────

    #[test]
    fn struct_form_becomes_attrset() {
        let s = SExpr::parse("(object (:name \"X\") (:size 3))").unwrap();
        let nix = NixValue::from_sexpr(&s);
        let matches = matches!(nix, NixValue::AttrSet(_));
        assert!(matches, "struct-form should convert to AttrSet");
    }

    #[test]
    fn attrset_roundtrips_through_struct_form() {
        let s = SExpr::parse("(object (:name \"X\") (:size 3))").unwrap();
        let nix = NixValue::from_sexpr(&s);
        let back = nix.to_sexpr();
        // Round-trip via Nix preserves the canonical text form.
        assert_eq!(back, s);
    }

    #[test]
    fn iac_type_list_roundtrips_through_nix() {
        let ty = IacType::List(Box::new(IacType::String));
        let s = ty.to_sexpr();
        let nix = NixValue::from_sexpr(&s);
        let back = nix.to_sexpr();
        // IacType::List is a tuple-tag, not a struct-form — it
        // survives as a plain list with the tag as first element.
        assert_eq!(back, s);
    }

    #[test]
    fn iac_attribute_roundtrips_through_nix_semantically() {
        // Nix attribute-sets are intrinsically alphabetically-keyed —
        // { a = 1; b = 2; } and { b = 2; a = 1; } are equal values
        // in Nix. The SExpr layer allows declaration-order fields, so
        // the round-trip Sexpr → Nix → Sexpr canonicalizes field order.
        // This is SEMANTIC equivalence, not BYTE equivalence: parsing
        // both sides via FromSExpr must produce identical IacAttribute
        // values even though the canonical text (and thus hash) differs.
        use crate::ir::IacAttribute;
        use crate::sexpr::FromSExpr;
        use crate::testing::TestAttributeBuilder;

        let attr = TestAttributeBuilder::new("x", IacType::String)
            .required()
            .build();
        let s = attr.to_sexpr();
        let nix = NixValue::from_sexpr(&s);
        let back = nix.to_sexpr();

        let parsed = IacAttribute::from_sexpr(&back).expect("parse round-tripped sexpr");
        assert_eq!(parsed, attr, "semantic round-trip must preserve value");
    }

    #[test]
    fn nix_roundtrip_sorts_fields_alphabetically() {
        // Document the canonicalization behavior explicitly: fields
        // emerge from the Nix round-trip in alphabetical order
        // because BTreeMap sorts.
        let s = SExpr::parse("(x (:z 1) (:a 2))").unwrap();
        let back = NixValue::from_sexpr(&s).to_sexpr();
        let back_text = back.emit();
        let a_pos = back_text.find(":a").unwrap();
        let z_pos = back_text.find(":z").unwrap();
        assert!(a_pos < z_pos, "a should come before z: {back_text}");
    }

    // ── Nix expression rendering ─────────────────────────────

    #[test]
    fn int_renders_as_decimal() {
        assert_eq!(NixValue::Int(42).to_nix_expr(), "42");
    }

    #[test]
    fn float_renders_with_decimal_point() {
        assert_eq!(NixValue::Float(1.0).to_nix_expr(), "1.0");
        assert_eq!(NixValue::Float(3.14).to_nix_expr(), "3.14");
    }

    #[test]
    fn bool_renders_as_nix_literal() {
        assert_eq!(NixValue::Bool(true).to_nix_expr(), "true");
        assert_eq!(NixValue::Bool(false).to_nix_expr(), "false");
    }

    #[test]
    fn null_renders() {
        assert_eq!(NixValue::Null.to_nix_expr(), "null");
    }

    #[test]
    fn string_escapes_dollar_sign() {
        // Nix interpolates ${…}; a literal $ must be escaped or it
        // would be (harmlessly, but confusingly) treated as a regular
        // char. We escape defensively.
        assert_eq!(NixValue::Str("$HOME".into()).to_nix_expr(), "\"\\$HOME\"",);
    }

    #[test]
    fn string_escapes_quotes() {
        assert_eq!(NixValue::Str("a\"b".into()).to_nix_expr(), "\"a\\\"b\"",);
    }

    #[test]
    fn empty_list_renders() {
        assert_eq!(NixValue::List(vec![]).to_nix_expr(), "[]");
    }

    #[test]
    fn list_renders_space_separated() {
        assert_eq!(
            NixValue::List(vec![NixValue::Int(1), NixValue::Int(2), NixValue::Int(3),])
                .to_nix_expr(),
            "[1 2 3]",
        );
    }

    #[test]
    fn empty_attrset_renders() {
        assert_eq!(NixValue::AttrSet(BTreeMap::new()).to_nix_expr(), "{}",);
    }

    #[test]
    fn attrset_renders_semicolon_separated() {
        let mut map = BTreeMap::new();
        map.insert("a".to_string(), NixValue::Int(1));
        map.insert("b".to_string(), NixValue::Int(2));
        assert_eq!(NixValue::AttrSet(map).to_nix_expr(), "{ a = 1; b = 2; }",);
    }

    #[test]
    fn attrset_quotes_non_ident_keys() {
        let mut map = BTreeMap::new();
        map.insert("weird key".to_string(), NixValue::Int(1));
        let rendered = NixValue::AttrSet(map).to_nix_expr();
        assert!(rendered.contains("\"weird key\""));
    }

    // ── Determinism ──────────────────────────────────────────

    #[test]
    fn attrset_iteration_is_deterministic() {
        // BTreeMap guarantees sorted order, so rendering is stable.
        let mut map = BTreeMap::new();
        map.insert("z".to_string(), NixValue::Int(1));
        map.insert("a".to_string(), NixValue::Int(2));
        map.insert("m".to_string(), NixValue::Int(3));
        let rendered = NixValue::AttrSet(map).to_nix_expr();
        // Keys must appear in alphabetical order.
        let a_pos = rendered.find("a = ").unwrap();
        let m_pos = rendered.find("m = ").unwrap();
        let z_pos = rendered.find("z = ").unwrap();
        assert!(a_pos < m_pos);
        assert!(m_pos < z_pos);
    }

    #[test]
    fn iac_type_renders_as_nix() {
        let ty = IacType::List(Box::new(IacType::Integer));
        let nix = NixValue::from_sexpr(&ty.to_sexpr());
        // IacType::List is a tuple-tag (list-with-head), so becomes
        // a Nix list of ident + symbol.
        let expr = nix.to_nix_expr();
        assert!(expr.contains("list"));
        assert!(expr.contains("integer"));
    }

    #[test]
    fn iac_attribute_renders_as_attrset() {
        use crate::testing::TestAttributeBuilder;
        let attr = TestAttributeBuilder::new("x", IacType::String)
            .required()
            .build();
        let nix = NixValue::from_sexpr(&attr.to_sexpr());
        let expr = nix.to_nix_expr();
        // Struct-form → AttrSet.
        assert!(expr.starts_with("{ "));
        assert!(expr.ends_with(" }"));
        assert!(expr.contains("head = \"attribute\""));
        assert!(expr.contains("required = true"));
    }
}
