//! User-extensible IR transforms.
//!
//! A `Transform<T>` is an endomorphism — it takes a `T` and returns a
//! modified `T`. This is how a user extends the pipeline without
//! recompiling the core: they declare transforms as data (or script
//! them in a minimal s-expr surface), stack them, and slot them into
//! the generation pipeline before the backend runs.
//!
//! Transforms compose: `ComposeTransforms(a, b).apply(x)` == `b.apply(a.apply(x))`.
//! `Identity` is the unit — useful as a default and as a sentinel.
//!
//! The `ops` submodule provides a minimal, data-level operation enum
//! (`ResourceOp`) covering the most common IR edits: add a tag, rename
//! an attribute, set a description, flip a sensitivity flag. Users who
//! need richer transforms can either add variants or implement their
//! own `Transform<IacResource>` in Rust.
//!
//! The `script` submodule (feature-gated, `script` feature) provides a
//! tiny s-expression reader that compiles a script string into a
//! `Vec<ResourceOp>`. That is the "embedded Lisp" surface of the
//! pipeline — code-as-data parse, interpreted to structured ops.
//! Sufficient for the usual extension cases; full scripting power can
//! later slot in behind the same trait.

use crate::ir::IacResource;

/// A structure-preserving mutation of a value.
pub trait Transform<T> {
    /// Human-readable name for diagnostics.
    fn name(&self) -> &'static str;

    /// Apply the transform, returning a new value. Must be deterministic.
    fn apply(&self, value: T) -> T;
}

/// Identity transform — returns its input unchanged.
#[derive(Debug, Default, Clone, Copy)]
pub struct Identity;

impl<T> Transform<T> for Identity {
    fn name(&self) -> &'static str {
        "Identity"
    }
    fn apply(&self, value: T) -> T {
        value
    }
}

/// Sequential composition: apply `a`, then `b`.
pub struct ComposeTransforms<A, B>(pub A, pub B);

impl<T, A, B> Transform<T> for ComposeTransforms<A, B>
where
    A: Transform<T>,
    B: Transform<T>,
{
    fn name(&self) -> &'static str {
        "ComposeTransforms"
    }
    fn apply(&self, value: T) -> T {
        self.1.apply(self.0.apply(value))
    }
}

/// Data-level IR operations.
///
/// These are the "atoms" of the transform language. Any more complex
/// transform either composes these or drops down to Rust.
pub mod ops {
    use super::{IacResource, Transform};
    use crate::ir::{IacAttribute, IacType};

    /// A single declarative edit to an `IacResource`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    #[non_exhaustive]
    pub enum ResourceOp {
        /// Set the resource's description.
        SetDescription(String),
        /// Set the resource's category.
        SetCategory(String),
        /// Mark an attribute (by canonical name) as sensitive.
        MarkSensitive(String),
        /// Append a new optional String attribute if one with that
        /// `canonical_name` does not already exist. Idempotent.
        AddOptionalString {
            canonical_name: String,
            api_name: String,
            description: String,
        },
        /// Remove an attribute by canonical name. No-op if absent.
        RemoveAttribute(String),
    }

    impl ResourceOp {
        fn apply_to(&self, mut r: IacResource) -> IacResource {
            match self {
                Self::SetDescription(d) => {
                    r.description = d.clone();
                }
                Self::SetCategory(c) => {
                    r.category = c.clone();
                }
                Self::MarkSensitive(name) => {
                    for a in &mut r.attributes {
                        if a.canonical_name == *name {
                            a.sensitive = true;
                        }
                    }
                }
                Self::AddOptionalString {
                    canonical_name,
                    api_name,
                    description,
                } => {
                    let already = r
                        .attributes
                        .iter()
                        .any(|a| a.canonical_name == *canonical_name);
                    if !already {
                        r.attributes.push(IacAttribute {
                            api_name: api_name.clone(),
                            canonical_name: canonical_name.clone(),
                            description: description.clone(),
                            iac_type: IacType::String,
                            optional: true,
                            ..Default::default()
                        });
                    }
                }
                Self::RemoveAttribute(name) => {
                    r.attributes.retain(|a| a.canonical_name != *name);
                }
            }
            r
        }
    }

    /// A sequence of ops runs in order and is itself a `Transform`.
    impl Transform<IacResource> for Vec<ResourceOp> {
        fn name(&self) -> &'static str {
            "ResourceOpSeq"
        }
        fn apply(&self, mut r: IacResource) -> IacResource {
            for op in self {
                r = op.apply_to(r);
            }
            r
        }
    }

    /// A single op is also a `Transform`.
    impl Transform<IacResource> for ResourceOp {
        fn name(&self) -> &'static str {
            match self {
                Self::SetDescription(_) => "SetDescription",
                Self::SetCategory(_) => "SetCategory",
                Self::MarkSensitive(_) => "MarkSensitive",
                Self::AddOptionalString { .. } => "AddOptionalString",
                Self::RemoveAttribute(_) => "RemoveAttribute",
            }
        }
        fn apply(&self, r: IacResource) -> IacResource {
            self.apply_to(r)
        }
    }
}

/// Minimal s-expression reader → `Vec<ResourceOp>`.
///
/// The surface is intentionally tiny: one op per top-level form, all
/// args are strings (single- or double-quoted). Supports the op names
/// `set-description`, `set-category`, `mark-sensitive`,
/// `add-optional-string`, `remove-attribute`. Whitespace and `;`-line
/// comments are tolerated.
///
/// ```text
/// ; add a tag field; mark the secret field sensitive
/// (add-optional-string "tag" "tag" "a user-provided tag")
/// (mark-sensitive "value")
/// ```
///
/// The parser is a single-pass hand-rolled reader — we are explicitly
/// not pulling in `serde_lexpr` or `steel` here. The point is to prove
/// the extension surface works end-to-end; a full Scheme runtime can
/// slot in later behind the same `Transform` trait.
pub mod script {
    use super::ops::ResourceOp;

    /// Parse a script source into a list of ops.
    ///
    /// # Errors
    /// Returns `Err(String)` describing the first parse failure. The
    /// string is human-readable and includes a pointer to the position.
    pub fn parse(source: &str) -> Result<Vec<ResourceOp>, String> {
        let mut chars = source.chars().peekable();
        let mut ops = Vec::new();
        loop {
            skip_ws_and_comments(&mut chars);
            if chars.peek().is_none() {
                break;
            }
            let expr = read_list(&mut chars)?;
            ops.push(expr_to_op(&expr)?);
        }
        Ok(ops)
    }

    // ── Minimal s-expr value ────────────────────────────────────────

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Sexpr {
        Sym(String),
        Str(String),
        List(Vec<Sexpr>),
    }

    fn skip_ws_and_comments(chars: &mut std::iter::Peekable<std::str::Chars>) {
        loop {
            match chars.peek().copied() {
                Some(c) if c.is_whitespace() => {
                    chars.next();
                }
                Some(';') => {
                    for c in chars.by_ref() {
                        if c == '\n' {
                            break;
                        }
                    }
                }
                _ => break,
            }
        }
    }

    fn read_list(
        chars: &mut std::iter::Peekable<std::str::Chars>,
    ) -> Result<Sexpr, String> {
        skip_ws_and_comments(chars);
        match chars.next() {
            Some('(') => {
                let mut items = Vec::new();
                loop {
                    skip_ws_and_comments(chars);
                    match chars.peek().copied() {
                        None => return Err("unterminated list".into()),
                        Some(')') => {
                            chars.next();
                            return Ok(Sexpr::List(items));
                        }
                        Some('(') => items.push(read_list(chars)?),
                        Some('"') => items.push(read_string(chars)?),
                        Some(_) => items.push(read_atom(chars)?),
                    }
                }
            }
            Some(c) => Err(format!("expected '(', got '{c}'")),
            None => Err("expected '(', got EOF".into()),
        }
    }

    fn read_string(
        chars: &mut std::iter::Peekable<std::str::Chars>,
    ) -> Result<Sexpr, String> {
        if chars.next() != Some('"') {
            return Err("expected opening quote".into());
        }
        let mut buf = String::new();
        loop {
            match chars.next() {
                None => return Err("unterminated string".into()),
                Some('"') => return Ok(Sexpr::Str(buf)),
                Some('\\') => match chars.next() {
                    Some('n') => buf.push('\n'),
                    Some('t') => buf.push('\t'),
                    Some('"') => buf.push('"'),
                    Some('\\') => buf.push('\\'),
                    Some(c) => {
                        return Err(format!("unknown escape \\{c}"));
                    }
                    None => return Err("dangling \\ at EOF".into()),
                },
                Some(c) => buf.push(c),
            }
        }
    }

    fn read_atom(
        chars: &mut std::iter::Peekable<std::str::Chars>,
    ) -> Result<Sexpr, String> {
        let mut buf = String::new();
        while let Some(&c) = chars.peek() {
            if c.is_whitespace() || c == '(' || c == ')' || c == ';' {
                break;
            }
            buf.push(c);
            chars.next();
        }
        if buf.is_empty() {
            Err("empty atom".into())
        } else {
            Ok(Sexpr::Sym(buf))
        }
    }

    fn expr_to_op(expr: &Sexpr) -> Result<ResourceOp, String> {
        let Sexpr::List(items) = expr else {
            return Err("top-level form must be a list".into());
        };
        let (head, rest) = items
            .split_first()
            .ok_or_else(|| "empty top-level form".to_string())?;
        let name = match head {
            Sexpr::Sym(s) => s,
            _ => return Err("first element must be an op name symbol".into()),
        };
        let args: Vec<&str> = rest
            .iter()
            .map(|e| match e {
                Sexpr::Str(s) | Sexpr::Sym(s) => Ok(s.as_str()),
                Sexpr::List(_) => Err("nested lists not supported in args".to_string()),
            })
            .collect::<Result<_, _>>()?;

        match name.as_str() {
            "set-description" => {
                if args.len() != 1 {
                    return Err(format!(
                        "set-description: expected 1 arg, got {}",
                        args.len()
                    ));
                }
                Ok(ResourceOp::SetDescription(args[0].to_string()))
            }
            "set-category" => {
                if args.len() != 1 {
                    return Err(format!("set-category: expected 1 arg, got {}", args.len()));
                }
                Ok(ResourceOp::SetCategory(args[0].to_string()))
            }
            "mark-sensitive" => {
                if args.len() != 1 {
                    return Err(format!(
                        "mark-sensitive: expected 1 arg, got {}",
                        args.len()
                    ));
                }
                Ok(ResourceOp::MarkSensitive(args[0].to_string()))
            }
            "add-optional-string" => {
                if args.len() != 3 {
                    return Err(format!(
                        "add-optional-string: expected 3 args, got {}",
                        args.len()
                    ));
                }
                Ok(ResourceOp::AddOptionalString {
                    canonical_name: args[0].to_string(),
                    api_name: args[1].to_string(),
                    description: args[2].to_string(),
                })
            }
            "remove-attribute" => {
                if args.len() != 1 {
                    return Err(format!(
                        "remove-attribute: expected 1 arg, got {}",
                        args.len()
                    ));
                }
                Ok(ResourceOp::RemoveAttribute(args[0].to_string()))
            }
            other => Err(format!("unknown op: {other}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ops::ResourceOp;
    use super::script;
    use super::{ComposeTransforms, Identity, Transform};
    use crate::ir::IacType;
    use crate::testing::{test_resource, TestAttributeBuilder};

    fn res() -> crate::ir::IacResource {
        let mut r = test_resource("widget");
        r.attributes = vec![
            TestAttributeBuilder::new("name", IacType::String).required().build(),
            TestAttributeBuilder::new("value", IacType::String).build(),
        ];
        r.description = "initial".into();
        r
    }

    #[test]
    fn identity_leaves_resource_unchanged() {
        let r = res();
        let r2 = Identity.apply(r.clone());
        assert_eq!(r.name, r2.name);
        assert_eq!(r.description, r2.description);
        assert_eq!(r.attributes.len(), r2.attributes.len());
    }

    #[test]
    fn set_description_op_runs() {
        let r = res();
        let r = ResourceOp::SetDescription("final".into()).apply(r);
        assert_eq!(r.description, "final");
    }

    #[test]
    fn mark_sensitive_flips_the_flag() {
        let r = res();
        let r = ResourceOp::MarkSensitive("value".into()).apply(r);
        assert!(r.attributes.iter().any(|a| a.canonical_name == "value" && a.sensitive));
    }

    #[test]
    fn add_optional_string_appends_new_attribute() {
        let r = res();
        let before = r.attributes.len();
        let r = ResourceOp::AddOptionalString {
            canonical_name: "note".into(),
            api_name: "note".into(),
            description: "a note".into(),
        }
        .apply(r);
        assert_eq!(r.attributes.len(), before + 1);
        assert!(r.attributes.iter().any(|a| a.canonical_name == "note"));
    }

    #[test]
    fn add_optional_string_is_idempotent() {
        let r = res();
        let op = ResourceOp::AddOptionalString {
            canonical_name: "note".into(),
            api_name: "note".into(),
            description: "a note".into(),
        };
        let r1 = op.apply(r);
        let len1 = r1.attributes.len();
        let r2 = op.apply(r1);
        assert_eq!(r2.attributes.len(), len1);
    }

    #[test]
    fn remove_attribute_drops_by_canonical_name() {
        let r = res();
        let r = ResourceOp::RemoveAttribute("value".into()).apply(r);
        assert!(!r.attributes.iter().any(|a| a.canonical_name == "value"));
    }

    #[test]
    fn vec_of_ops_is_a_transform() {
        let r = res();
        let ops: Vec<ResourceOp> = vec![
            ResourceOp::SetDescription("tuned".into()),
            ResourceOp::MarkSensitive("value".into()),
        ];
        let r = ops.apply(r);
        assert_eq!(r.description, "tuned");
        assert!(r.attributes.iter().any(|a| a.canonical_name == "value" && a.sensitive));
    }

    #[test]
    fn compose_transforms_applies_in_order() {
        let r = res();
        let a = ResourceOp::SetDescription("step1".into());
        let b = ResourceOp::SetDescription("step2".into());
        let composed = ComposeTransforms(a, b);
        let r = composed.apply(r);
        assert_eq!(r.description, "step2", "second transform wins");
    }

    #[test]
    fn compose_with_identity_left_and_right() {
        let r = res();
        let original_desc = r.description.clone();
        let with_id_left =
            ComposeTransforms(Identity, ResourceOp::SetDescription("x".into())).apply(r.clone());
        let with_id_right =
            ComposeTransforms(ResourceOp::SetDescription("x".into()), Identity).apply(r);
        assert_eq!(with_id_left.description, "x");
        assert_eq!(with_id_right.description, "x");
        assert_ne!(original_desc, "x");
    }

    // ── Script layer ───────────────────────────────────────────────

    #[test]
    fn script_parses_a_single_op() {
        let ops = script::parse(r#"(set-description "hello")"#).expect("parse");
        assert_eq!(ops, vec![ResourceOp::SetDescription("hello".into())]);
    }

    #[test]
    fn script_parses_multiple_ops() {
        let src = r#"
            ; add a field and mark value sensitive
            (add-optional-string "note" "note" "a free-form note")
            (mark-sensitive "value")
        "#;
        let ops = script::parse(src).expect("parse");
        assert_eq!(ops.len(), 2);
    }

    #[test]
    fn script_tolerates_comments_and_blank_lines() {
        let src = r#"
            ;; this is a comment

            (set-category "tuned")

            ; another comment
            (set-description "v2")
        "#;
        let ops = script::parse(src).expect("parse");
        assert_eq!(ops.len(), 2);
        assert_eq!(ops[0], ResourceOp::SetCategory("tuned".into()));
    }

    #[test]
    fn script_rejects_unknown_op() {
        let err = script::parse(r#"(unknown-thing "x")"#).unwrap_err();
        assert!(err.contains("unknown op"));
    }

    #[test]
    fn script_rejects_arity_mismatch() {
        let err = script::parse(r#"(set-description "a" "b")"#).unwrap_err();
        assert!(err.contains("set-description"));
        assert!(err.contains("expected 1"));
    }

    #[test]
    fn script_end_to_end_over_resource() {
        let src = r#"
            (set-description "v2")
            (add-optional-string "note" "note" "a note")
            (mark-sensitive "value")
        "#;
        let ops = script::parse(src).expect("parse");
        let r = ops.apply(res());
        assert_eq!(r.description, "v2");
        assert!(r.attributes.iter().any(|a| a.canonical_name == "note"));
        assert!(r.attributes.iter().any(|a| a.canonical_name == "value" && a.sensitive));
    }

    #[test]
    fn script_parse_is_deterministic() {
        let src = r#"(set-description "x") (mark-sensitive "y")"#;
        let a = script::parse(src).unwrap();
        let b = script::parse(src).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn script_handles_string_escapes() {
        let ops = script::parse(r#"(set-description "line1\nline2")"#).unwrap();
        assert_eq!(
            ops,
            vec![ResourceOp::SetDescription("line1\nline2".into())]
        );
    }

    #[test]
    fn script_rejects_unterminated_string() {
        let err = script::parse(r#"(set-description "hello"#).unwrap_err();
        assert!(err.contains("unterminated string"));
    }

    #[test]
    fn script_rejects_unterminated_list() {
        let err = script::parse(r#"(set-description "hello""#).unwrap_err();
        assert!(err.contains("unterminated list"));
    }
}
