//! Canonical s-expression interchange for the platform-independent IR.
//!
//! Typed ASTs remain the construction surface — you can never build an
//! invalid `IacResource`, an unbalanced `RubyNode`, an unterminated class.
//! But every typed value can also emit a canonical, human-readable
//! s-expression form and round-trip through it.
//!
//! The interchange unlocks:
//! - **Attestation**: tameshi/sekiban can hash a canonical form that is
//!   the same across machines, languages, and revisions.
//! - **Scripting**: the same reader pattern used by
//!   [`crate::transform::script`] operates over typed IR values.
//! - **Cross-process**: emit as sexpr, send over a socket, re-parse on
//!   the other side — same shape guaranteed.
//! - **Audit queries**: shinryu can store an IR snapshot as text and
//!   SQL over it without losing structure.
//!
//! Non-goals:
//! - **Wire efficiency** — this is a human-readable, debuggable format.
//!   For bytes-on-the-wire use serde_json / protobuf.
//! - **Schema evolution** — every round-trip must be lossless against
//!   the *current* type definitions. Adding a field means updating impls.
//!
//! # Format
//!
//! - **Unit enum variants**: the variant tag as a kebab-case symbol.
//!   `IacType::String` → `string`.
//! - **Tuple enum variants**: `(tag arg1 arg2 …)`.
//!   `IacType::List(inner)` → `(list <inner>)`.
//! - **Struct enum variants**: `(tag (:field val) …)`.
//!   `IacType::Enum { values, underlying }` →
//!   `(enum (:values (list "tcp" "udp")) (:underlying string))`.
//! - **Structs**: `(struct-name (:field val) …)`.
//!   `IacAttribute { api_name: "x", … }` →
//!   `(attribute (:api-name "x") …)`.
//! - **Vec**: `(list item1 item2 …)`. Empty vec: `(list)`.
//! - **Option**: `nil` for None, the value otherwise.
//! - **String**: double-quoted with `\n`, `\t`, `\"`, `\\` escapes.
//! - **Bool**: `true` / `false` symbols.
//! - **Integer / Float**: numeric literals. Integers have no decimal point.
//!
//! # Examples
//!
//! ```
//! use iac_forge::sexpr::{FromSExpr, SExpr, ToSExpr};
//! use iac_forge::ir::IacType;
//!
//! let ty = IacType::List(Box::new(IacType::Integer));
//! let s = ty.to_sexpr();
//! assert_eq!(s.emit(), "(list integer)");
//!
//! let roundtrip = IacType::from_sexpr(&SExpr::parse("(list integer)").unwrap()).unwrap();
//! assert_eq!(ty, roundtrip);
//! ```

use std::fmt;

/// BLAKE3 content hash over a canonical emission.
///
/// Displayed as lowercase hex (64 chars). Constructed only via
/// [`SExpr::content_hash`] to guarantee the hash always corresponds to
/// canonical emission bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContentHash(pub [u8; 32]);

impl ContentHash {
    /// Lowercase hex string (64 chars).
    #[must_use]
    pub fn to_hex(&self) -> String {
        let mut out = String::with_capacity(64);
        for b in &self.0 {
            out.push(hex_nibble(b >> 4));
            out.push(hex_nibble(b & 0xF));
        }
        out
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

fn hex_nibble(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        10..=15 => (b'a' + (n - 10)) as char,
        _ => unreachable!("nibble must be 0..=15"),
    }
}

/// Canonical s-expression value.
#[derive(Debug, Clone, PartialEq)]
pub enum SExpr {
    /// Identifier / variant tag / keyword label (with `:` prefix).
    Symbol(String),
    /// Double-quoted string literal.
    String(String),
    /// Integer literal (no decimal point).
    Integer(i64),
    /// Float literal (has decimal point or exponent).
    Float(f64),
    /// `true` or `false`.
    Bool(bool),
    /// `nil` — serves as both "empty" and "None".
    Nil,
    /// `(...)` form.
    List(Vec<SExpr>),
}

/// Conversion errors during `FromSExpr` parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SExprError {
    /// Top-level parse failure (bad tokens, unbalanced parens, etc.).
    Parse(String),
    /// Value shape didn't match what the destination type expected.
    Shape(String),
    /// A named field required by the destination was missing.
    MissingField(String),
    /// Unknown / unsupported variant tag.
    UnknownVariant(String),
}

impl fmt::Display for SExprError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(s) => write!(f, "sexpr parse: {s}"),
            Self::Shape(s) => write!(f, "sexpr shape: {s}"),
            Self::MissingField(s) => write!(f, "sexpr missing field: {s}"),
            Self::UnknownVariant(s) => write!(f, "sexpr unknown variant: {s}"),
        }
    }
}

impl std::error::Error for SExprError {}

impl SExpr {
    /// Emit this value as canonical source text.
    #[must_use]
    pub fn emit(&self) -> String {
        let mut out = String::new();
        self.emit_into(&mut out);
        out
    }

    /// Compute a BLAKE3 content hash over the canonical emission.
    ///
    /// This is the content address of the value: structurally-equal
    /// values produce byte-equal emissions (proven by the round-trip
    /// proptests) and therefore equal hashes. Structurally-different
    /// values produce different emissions and (overwhelmingly) different
    /// hashes.
    ///
    /// The hash is suitable for:
    /// - **Cache keys** — skip regeneration when IR hasn't changed
    /// - **Attestation** — pair with tameshi Merkle trees at leaf level
    /// - **Audit** — shinryu rows carry the hash for historical
    ///   correlation without storing the full sexpr
    #[must_use]
    pub fn content_hash(&self) -> ContentHash {
        let emitted = self.emit();
        let hash = blake3::hash(emitted.as_bytes());
        ContentHash(*hash.as_bytes())
    }

    fn emit_into(&self, out: &mut String) {
        match self {
            Self::Symbol(s) => out.push_str(s),
            Self::String(s) => {
                out.push('"');
                for c in s.chars() {
                    match c {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\t' => out.push_str("\\t"),
                        c => out.push(c),
                    }
                }
                out.push('"');
            }
            Self::Integer(i) => out.push_str(&i.to_string()),
            Self::Float(fl) => {
                // Always include a decimal to distinguish from Integer.
                let s = fl.to_string();
                if s.contains('.')
                    || s.contains('e')
                    || s.contains('E')
                    || s == "inf"
                    || s == "-inf"
                    || s == "NaN"
                {
                    out.push_str(&s);
                } else {
                    out.push_str(&s);
                    out.push_str(".0");
                }
            }
            Self::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Self::Nil => out.push_str("nil"),
            Self::List(items) => {
                out.push('(');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(' ');
                    }
                    item.emit_into(out);
                }
                out.push(')');
            }
        }
    }

    /// Parse canonical source text into an SExpr.
    ///
    /// # Errors
    /// Returns `SExprError::Parse` with a human-readable message on
    /// unterminated strings, unbalanced parens, bad number tokens, etc.
    pub fn parse(source: &str) -> Result<Self, SExprError> {
        let mut chars = source.chars().peekable();
        skip_ws_and_comments(&mut chars);
        let val = read_one(&mut chars)?;
        skip_ws_and_comments(&mut chars);
        if chars.peek().is_some() {
            return Err(SExprError::Parse("trailing content after value".into()));
        }
        Ok(val)
    }

    /// Interpret as a list reference or return a shape error.
    ///
    /// # Errors
    /// Returns `SExprError::Shape` if this is not a `List`.
    pub fn as_list(&self) -> Result<&[SExpr], SExprError> {
        match self {
            Self::List(items) => Ok(items),
            other => Err(SExprError::Shape(format!("expected list, got {other:?}"))),
        }
    }

    /// Interpret as a symbol and return a shape error otherwise.
    ///
    /// # Errors
    /// Returns `SExprError::Shape` if this is not a `Symbol`.
    pub fn as_symbol(&self) -> Result<&str, SExprError> {
        match self {
            Self::Symbol(s) => Ok(s),
            other => Err(SExprError::Shape(format!("expected symbol, got {other:?}"))),
        }
    }

    /// Interpret as a string and return a shape error otherwise.
    ///
    /// # Errors
    /// Returns `SExprError::Shape` if this is not a `String`.
    pub fn as_str(&self) -> Result<&str, SExprError> {
        match self {
            Self::String(s) => Ok(s),
            other => Err(SExprError::Shape(format!("expected string, got {other:?}"))),
        }
    }
}

// ── Reader ──────────────────────────────────────────────────────────

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

fn read_one(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<SExpr, SExprError> {
    skip_ws_and_comments(chars);
    match chars.peek().copied() {
        None => Err(SExprError::Parse("unexpected EOF".into())),
        Some('(') => read_list(chars),
        Some('"') => read_string(chars),
        Some(_) => read_atom(chars),
    }
}

fn read_list(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<SExpr, SExprError> {
    debug_assert_eq!(chars.next(), Some('('));
    let mut items = Vec::new();
    loop {
        skip_ws_and_comments(chars);
        match chars.peek().copied() {
            None => return Err(SExprError::Parse("unterminated list".into())),
            Some(')') => {
                chars.next();
                return Ok(SExpr::List(items));
            }
            Some(_) => items.push(read_one(chars)?),
        }
    }
}

fn read_string(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<SExpr, SExprError> {
    debug_assert_eq!(chars.next(), Some('"'));
    let mut buf = String::new();
    loop {
        match chars.next() {
            None => return Err(SExprError::Parse("unterminated string".into())),
            Some('"') => return Ok(SExpr::String(buf)),
            Some('\\') => match chars.next() {
                Some('n') => buf.push('\n'),
                Some('t') => buf.push('\t'),
                Some('"') => buf.push('"'),
                Some('\\') => buf.push('\\'),
                Some(c) => {
                    return Err(SExprError::Parse(format!("unknown escape: \\{c}")));
                }
                None => return Err(SExprError::Parse("dangling \\ at EOF".into())),
            },
            Some(c) => buf.push(c),
        }
    }
}

fn read_atom(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<SExpr, SExprError> {
    let mut buf = String::new();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() || c == '(' || c == ')' || c == ';' || c == '"' {
            break;
        }
        buf.push(c);
        chars.next();
    }
    if buf.is_empty() {
        return Err(SExprError::Parse("empty atom".into()));
    }
    // Order matters: try bool, nil, integer, float, symbol.
    match buf.as_str() {
        "true" => return Ok(SExpr::Bool(true)),
        "false" => return Ok(SExpr::Bool(false)),
        "nil" => return Ok(SExpr::Nil),
        _ => {}
    }
    // Integer: only if no decimal point or exponent.
    if !buf.contains('.') && !buf.contains('e') && !buf.contains('E') {
        if let Ok(i) = buf.parse::<i64>() {
            return Ok(SExpr::Integer(i));
        }
    }
    if let Ok(f) = buf.parse::<f64>() {
        return Ok(SExpr::Float(f));
    }
    Ok(SExpr::Symbol(buf))
}

// ── Traits ──────────────────────────────────────────────────────────

/// Produce a canonical SExpr for this value.
///
/// Must be the inverse of `FromSExpr::from_sexpr` — every implementation
/// is expected to satisfy the round-trip law
/// `T::from_sexpr(&x.to_sexpr())? == x`.
pub trait ToSExpr {
    fn to_sexpr(&self) -> SExpr;

    /// Convenience: content hash of this value's canonical emission.
    ///
    /// Equivalent to `self.to_sexpr().content_hash()`. Every type that
    /// implements `ToSExpr` gets a stable content address for free.
    #[must_use]
    fn content_hash(&self) -> ContentHash {
        self.to_sexpr().content_hash()
    }
}

/// Parse an SExpr back into this value.
pub trait FromSExpr: Sized {
    /// # Errors
    /// Returns `SExprError` if the structure or content doesn't match.
    fn from_sexpr(s: &SExpr) -> Result<Self, SExprError>;
}

// ── Primitive impls ─────────────────────────────────────────────────

impl ToSExpr for String {
    fn to_sexpr(&self) -> SExpr {
        SExpr::String(self.clone())
    }
}

impl FromSExpr for String {
    fn from_sexpr(s: &SExpr) -> Result<Self, SExprError> {
        Ok(s.as_str()?.to_string())
    }
}

impl ToSExpr for bool {
    fn to_sexpr(&self) -> SExpr {
        SExpr::Bool(*self)
    }
}

impl ToSExpr for i64 {
    fn to_sexpr(&self) -> SExpr {
        SExpr::Integer(*self)
    }
}

impl FromSExpr for i64 {
    fn from_sexpr(s: &SExpr) -> Result<Self, SExprError> {
        match s {
            SExpr::Integer(i) => Ok(*i),
            other => Err(SExprError::Shape(format!(
                "expected integer, got {other:?}"
            ))),
        }
    }
}

impl ToSExpr for f64 {
    fn to_sexpr(&self) -> SExpr {
        SExpr::Float(*self)
    }
}

impl FromSExpr for f64 {
    fn from_sexpr(s: &SExpr) -> Result<Self, SExprError> {
        match s {
            SExpr::Float(f) => Ok(*f),
            SExpr::Integer(i) => Ok(*i as f64), // widen int to float
            other => Err(SExprError::Shape(format!("expected float, got {other:?}"))),
        }
    }
}

impl FromSExpr for bool {
    fn from_sexpr(s: &SExpr) -> Result<Self, SExprError> {
        match s {
            SExpr::Bool(b) => Ok(*b),
            other => Err(SExprError::Shape(format!("expected bool, got {other:?}"))),
        }
    }
}

impl<T: ToSExpr> ToSExpr for Option<T> {
    fn to_sexpr(&self) -> SExpr {
        match self {
            Some(v) => v.to_sexpr(),
            None => SExpr::Nil,
        }
    }
}

impl<T: FromSExpr> FromSExpr for Option<T> {
    fn from_sexpr(s: &SExpr) -> Result<Self, SExprError> {
        match s {
            SExpr::Nil => Ok(None),
            other => Ok(Some(T::from_sexpr(other)?)),
        }
    }
}

impl<T: ToSExpr> ToSExpr for Vec<T> {
    fn to_sexpr(&self) -> SExpr {
        let mut items = Vec::with_capacity(self.len() + 1);
        items.push(SExpr::Symbol("list".into()));
        for v in self {
            items.push(v.to_sexpr());
        }
        SExpr::List(items)
    }
}

impl<T: FromSExpr> FromSExpr for Vec<T> {
    fn from_sexpr(s: &SExpr) -> Result<Self, SExprError> {
        let items = s.as_list()?;
        let Some((head, rest)) = items.split_first() else {
            return Err(SExprError::Shape("expected (list ...) form, got ()".into()));
        };
        let tag = head.as_symbol()?;
        if tag != "list" {
            return Err(SExprError::Shape(format!(
                "expected list head 'list', got '{tag}'"
            )));
        }
        rest.iter().map(T::from_sexpr).collect()
    }
}

// ── Helpers for struct emission / parsing ───────────────────────────

/// Build a struct-form SExpr: `(name (:field val) …)`.
#[must_use]
pub fn struct_expr(name: &str, fields: Vec<(&str, SExpr)>) -> SExpr {
    let mut items = Vec::with_capacity(fields.len() + 1);
    items.push(SExpr::Symbol(name.to_string()));
    for (k, v) in fields {
        items.push(SExpr::List(vec![SExpr::Symbol(format!(":{k}")), v]));
    }
    SExpr::List(items)
}

/// Pull a struct-form list by name, returning its field map.
///
/// # Errors
/// Returns `SExprError::Shape` if the head symbol doesn't match `expected_name`
/// or the form isn't a list of keyword pairs.
pub fn parse_struct<'a>(
    s: &'a SExpr,
    expected_name: &str,
) -> Result<std::collections::BTreeMap<String, &'a SExpr>, SExprError> {
    let items = s.as_list()?;
    let (head, rest) = items
        .split_first()
        .ok_or_else(|| SExprError::Shape(format!("expected ({expected_name} …), got ()")))?;
    let name = head.as_symbol()?;
    if name != expected_name {
        return Err(SExprError::Shape(format!(
            "expected '{expected_name}', got '{name}'"
        )));
    }
    let mut out = std::collections::BTreeMap::new();
    for field in rest {
        let pair = field.as_list()?;
        if pair.len() != 2 {
            return Err(SExprError::Shape(format!(
                "expected (:field value) pair, got {pair:?}"
            )));
        }
        let key = pair[0].as_symbol()?;
        if !key.starts_with(':') {
            return Err(SExprError::Shape(format!(
                "expected keyword (:name), got '{key}'"
            )));
        }
        out.insert(key[1..].to_string(), &pair[1]);
    }
    Ok(out)
}

/// Extract a required field from a struct-form map.
///
/// # Errors
/// Returns `SExprError::MissingField` if the field is absent.
pub fn take_field<'a>(
    fields: &std::collections::BTreeMap<String, &'a SExpr>,
    name: &str,
) -> Result<&'a SExpr, SExprError> {
    fields
        .get(name)
        .copied()
        .ok_or_else(|| SExprError::MissingField(name.to_string()))
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emit_symbol() {
        assert_eq!(SExpr::Symbol("hello".into()).emit(), "hello");
    }

    #[test]
    fn emit_string_with_escapes() {
        assert_eq!(SExpr::String("a\"b\nc".into()).emit(), "\"a\\\"b\\nc\"",);
    }

    #[test]
    fn emit_integer() {
        assert_eq!(SExpr::Integer(42).emit(), "42");
        assert_eq!(SExpr::Integer(-7).emit(), "-7");
    }

    #[test]
    fn emit_float_adds_decimal_if_missing() {
        assert_eq!(SExpr::Float(1.0).emit(), "1.0");
        assert_eq!(SExpr::Float(3.14).emit(), "3.14");
    }

    #[test]
    fn emit_bool() {
        assert_eq!(SExpr::Bool(true).emit(), "true");
        assert_eq!(SExpr::Bool(false).emit(), "false");
    }

    #[test]
    fn emit_nil() {
        assert_eq!(SExpr::Nil.emit(), "nil");
    }

    #[test]
    fn emit_empty_list() {
        assert_eq!(SExpr::List(vec![]).emit(), "()");
    }

    #[test]
    fn emit_nested_list() {
        let s = SExpr::List(vec![
            SExpr::Symbol("foo".into()),
            SExpr::Integer(1),
            SExpr::List(vec![SExpr::Symbol("bar".into()), SExpr::String("x".into())]),
        ]);
        assert_eq!(s.emit(), "(foo 1 (bar \"x\"))");
    }

    #[test]
    fn parse_primitives() {
        assert_eq!(SExpr::parse("42").unwrap(), SExpr::Integer(42));
        assert_eq!(SExpr::parse("3.14").unwrap(), SExpr::Float(3.14));
        assert_eq!(SExpr::parse("true").unwrap(), SExpr::Bool(true));
        assert_eq!(SExpr::parse("false").unwrap(), SExpr::Bool(false));
        assert_eq!(SExpr::parse("nil").unwrap(), SExpr::Nil);
        assert_eq!(
            SExpr::parse("hello").unwrap(),
            SExpr::Symbol("hello".into())
        );
        assert_eq!(SExpr::parse("\"hi\"").unwrap(), SExpr::String("hi".into()));
    }

    #[test]
    fn parse_string_escapes() {
        assert_eq!(
            SExpr::parse("\"a\\nb\"").unwrap(),
            SExpr::String("a\nb".into())
        );
        assert_eq!(
            SExpr::parse("\"a\\\"b\"").unwrap(),
            SExpr::String("a\"b".into())
        );
    }

    #[test]
    fn parse_nested_list() {
        let s = SExpr::parse("(foo 1 (bar \"x\"))").unwrap();
        assert_eq!(
            s,
            SExpr::List(vec![
                SExpr::Symbol("foo".into()),
                SExpr::Integer(1),
                SExpr::List(vec![SExpr::Symbol("bar".into()), SExpr::String("x".into()),]),
            ])
        );
    }

    #[test]
    fn parse_rejects_unterminated_list() {
        assert!(matches!(SExpr::parse("(foo"), Err(SExprError::Parse(_))));
    }

    #[test]
    fn parse_rejects_trailing_content() {
        assert!(matches!(SExpr::parse("1 2"), Err(SExprError::Parse(_))));
    }

    #[test]
    fn parse_tolerates_comments() {
        let s = SExpr::parse("; leading\n(a ; mid\n b)\n; trailing").unwrap();
        assert_eq!(
            s,
            SExpr::List(vec![SExpr::Symbol("a".into()), SExpr::Symbol("b".into())])
        );
    }

    #[test]
    fn round_trip_string() {
        let s = "hello \"world\"\n".to_string();
        let round = String::from_sexpr(&s.to_sexpr()).unwrap();
        assert_eq!(s, round);
    }

    #[test]
    fn round_trip_bool() {
        assert_eq!(bool::from_sexpr(&true.to_sexpr()).unwrap(), true);
        assert_eq!(bool::from_sexpr(&false.to_sexpr()).unwrap(), false);
    }

    #[test]
    fn round_trip_option_some_and_none() {
        let some: Option<String> = Some("hi".into());
        let none: Option<String> = None;
        assert_eq!(
            Option::<String>::from_sexpr(&some.to_sexpr()).unwrap(),
            some
        );
        assert_eq!(
            Option::<String>::from_sexpr(&none.to_sexpr()).unwrap(),
            none
        );
    }

    #[test]
    fn round_trip_vec() {
        let v: Vec<String> = vec!["a".into(), "b".into()];
        let s = v.to_sexpr();
        assert_eq!(s.emit(), "(list \"a\" \"b\")");
        assert_eq!(Vec::<String>::from_sexpr(&s).unwrap(), v);
    }

    #[test]
    fn round_trip_empty_vec() {
        let v: Vec<String> = vec![];
        let s = v.to_sexpr();
        assert_eq!(s.emit(), "(list)");
        assert_eq!(Vec::<String>::from_sexpr(&s).unwrap(), v);
    }

    #[test]
    fn struct_expr_and_parse_struct_round_trip() {
        let s = struct_expr(
            "attribute",
            vec![
                ("api-name", "foo".to_string().to_sexpr()),
                ("required", true.to_sexpr()),
            ],
        );
        assert_eq!(s.emit(), "(attribute (:api-name \"foo\") (:required true))");

        let parsed = parse_struct(&s, "attribute").unwrap();
        assert_eq!(parsed.len(), 2);
        assert!(parsed.contains_key("api-name"));
        assert!(parsed.contains_key("required"));
    }

    #[test]
    fn parse_struct_errors_on_wrong_name() {
        let s = struct_expr("attribute", vec![]);
        let err = parse_struct(&s, "resource").unwrap_err();
        assert!(matches!(err, SExprError::Shape(_)));
    }

    #[test]
    fn take_field_missing_errors_cleanly() {
        let s = struct_expr("x", vec![]);
        let map = parse_struct(&s, "x").unwrap();
        let err = take_field(&map, "foo").unwrap_err();
        assert!(matches!(err, SExprError::MissingField(_)));
    }

    #[test]
    fn float_and_integer_distinguished_on_parse() {
        assert_eq!(SExpr::parse("1").unwrap(), SExpr::Integer(1));
        assert_eq!(SExpr::parse("1.0").unwrap(), SExpr::Float(1.0));
    }

    // ── Content addressing ────────────────────────────────────

    #[test]
    fn content_hash_is_64_hex_chars() {
        let h = SExpr::Integer(42).content_hash();
        assert_eq!(h.to_hex().len(), 64);
        assert!(h.to_hex().chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(h.to_string(), h.to_hex());
    }

    #[test]
    fn content_hash_is_deterministic() {
        let s = SExpr::List(vec![
            SExpr::Symbol("foo".into()),
            SExpr::Integer(1),
            SExpr::String("bar".into()),
        ]);
        assert_eq!(s.content_hash(), s.content_hash());
    }

    #[test]
    fn content_hash_differs_for_different_values() {
        let a = SExpr::Integer(1).content_hash();
        let b = SExpr::Integer(2).content_hash();
        assert_ne!(a, b);
    }

    #[test]
    fn content_hash_matches_emission_hash() {
        // The hash is defined as BLAKE3 over canonical emission, so
        // computing it independently via the same path must agree.
        let s = SExpr::String("hello world".into());
        let expected = {
            let emitted = s.emit();
            let h = blake3::hash(emitted.as_bytes());
            ContentHash(*h.as_bytes())
        };
        assert_eq!(s.content_hash(), expected);
    }

    #[test]
    fn to_sexpr_blanket_content_hash() {
        // String implements ToSExpr; the blanket content_hash must
        // equal the raw sexpr form's content_hash.
        let s = "hello".to_string();
        assert_eq!(s.content_hash(), s.to_sexpr().content_hash());
    }

    #[test]
    fn content_hash_stable_across_clones() {
        let s = SExpr::List(vec![SExpr::Symbol("x".into()), SExpr::Integer(42)]);
        let clone = s.clone();
        assert_eq!(s.content_hash(), clone.content_hash());
    }
}
