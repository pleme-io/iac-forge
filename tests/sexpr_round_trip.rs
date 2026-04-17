//! Property-based round-trip proofs for ToSExpr/FromSExpr on IR types.
//!
//! For every arbitrary value `x` of an IR type, the following must hold:
//!
//!     T::from_sexpr(&x.to_sexpr())? == x
//!     T::from_sexpr(&SExpr::parse(&x.to_sexpr().emit())?)? == x
//!     x.to_sexpr().emit() == x.to_sexpr().emit()   (determinism)
//!
//! These are the exact laws that make the sexpr form usable for
//! attestation, audit, interchange — no hidden state, no reliance on
//! allocator order, no version drift between emit and parse.

use proptest::prelude::*;

use iac_forge::ir::{IacAttribute, IacType};
use iac_forge::sexpr::{FromSExpr, SExpr, ToSExpr};

// ── Strategies ──────────────────────────────────────────────────────

fn arb_iac_type() -> impl Strategy<Value = IacType> {
    let leaf = prop_oneof![
        Just(IacType::String),
        Just(IacType::Integer),
        Just(IacType::Float),
        Just(IacType::Numeric),
        Just(IacType::Boolean),
        Just(IacType::Any),
    ];
    leaf.prop_recursive(3, 24, 5, |inner| {
        prop_oneof![
            inner.clone().prop_map(|t| IacType::List(Box::new(t))),
            inner.clone().prop_map(|t| IacType::Set(Box::new(t))),
            inner.clone().prop_map(|t| IacType::Map(Box::new(t))),
            (
                "[A-Za-z][A-Za-z0-9_]{0,10}",
                prop::collection::vec(arb_iac_attribute_shallow(), 0..3),
            )
                .prop_map(|(name, fields)| IacType::Object { name, fields }),
            (
                prop::collection::vec("[a-z][a-z0-9_-]{0,8}", 0..5),
                inner.clone(),
            )
                .prop_map(|(values, underlying)| IacType::Enum {
                    values,
                    underlying: Box::new(underlying),
                }),
        ]
    })
}

/// Attributes without nested Object types (to keep recursion bounded
/// for the Object variant above).
fn arb_iac_attribute_shallow() -> impl Strategy<Value = IacAttribute> {
    (
        "[a-zA-Z][a-zA-Z0-9_-]{0,10}",
        prop_oneof![
            Just(IacType::String),
            Just(IacType::Integer),
            Just(IacType::Boolean),
        ],
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(
            |(name, ty, required, optional, sensitive, immutable)| IacAttribute {
                api_name: name.clone(),
                canonical_name: name,
                description: String::new(),
                iac_type: ty,
                required,
                optional,
                computed: false,
                sensitive,
                json_encoded: false,
                immutable,
                default_value: None,
                enum_values: None,
                read_path: None,
                update_only: false,
            },
        )
}

fn arb_iac_attribute() -> impl Strategy<Value = IacAttribute> {
    (
        "[a-zA-Z][a-zA-Z0-9_-]{0,10}",
        arb_iac_type(),
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
        prop::option::of("[a-z_]{1,8}"),
        prop::option::of(prop::collection::vec("[a-z_]{1,5}", 1..4)),
        any::<bool>(),
    )
        .prop_map(
            |(
                name,
                ty,
                required,
                optional,
                computed,
                sensitive,
                json_encoded,
                immutable,
                read_path,
                enum_values,
                update_only,
            )| IacAttribute {
                api_name: name.clone(),
                canonical_name: name,
                description: "a field".into(),
                iac_type: ty,
                required,
                optional,
                computed,
                sensitive,
                json_encoded,
                immutable,
                default_value: None,
                enum_values,
                read_path,
                update_only,
            },
        )
}

// ── Properties ──────────────────────────────────────────────────────

proptest! {
    /// Every arbitrary IacType round-trips through to_sexpr/from_sexpr.
    #[test]
    fn iac_type_direct_roundtrip(ty in arb_iac_type()) {
        let s = ty.to_sexpr();
        let parsed = IacType::from_sexpr(&s)
            .unwrap_or_else(|e| panic!("from_sexpr failed: {e:?} — sexpr: {s:?}"));
        prop_assert_eq!(parsed, ty);
    }

    /// Round-trip survives the emit→parse boundary (text form).
    #[test]
    fn iac_type_text_roundtrip(ty in arb_iac_type()) {
        let emitted = ty.to_sexpr().emit();
        let sexpr = SExpr::parse(&emitted)
            .unwrap_or_else(|e| panic!("parse failed: {e:?} — text: {emitted}"));
        let parsed = IacType::from_sexpr(&sexpr)
            .unwrap_or_else(|e| panic!("from_sexpr failed: {e:?}"));
        prop_assert_eq!(parsed, ty);
    }

    /// Emit is deterministic.
    #[test]
    fn iac_type_emit_is_deterministic(ty in arb_iac_type()) {
        prop_assert_eq!(ty.to_sexpr().emit(), ty.to_sexpr().emit());
    }

    /// IacAttribute round-trips directly.
    #[test]
    fn iac_attribute_direct_roundtrip(attr in arb_iac_attribute()) {
        let parsed = IacAttribute::from_sexpr(&attr.to_sexpr())
            .unwrap_or_else(|e| panic!("parse failed: {e:?}"));
        prop_assert_eq!(parsed, attr);
    }

    /// IacAttribute survives the text boundary.
    #[test]
    fn iac_attribute_text_roundtrip(attr in arb_iac_attribute()) {
        let emitted = attr.to_sexpr().emit();
        let sexpr = SExpr::parse(&emitted)
            .unwrap_or_else(|e| panic!("parse failed: {e:?}"));
        let parsed = IacAttribute::from_sexpr(&sexpr).unwrap();
        prop_assert_eq!(parsed, attr);
    }

    /// Emission never produces non-printable-ASCII text outside string literals
    /// (strings may contain escape sequences for \n, \t etc. but the text is
    /// still ASCII).
    #[test]
    fn iac_type_emission_is_printable_ascii(ty in arb_iac_type()) {
        let emitted = ty.to_sexpr().emit();
        for ch in emitted.chars() {
            prop_assert!(
                ch == '\n' || ch == ' ' || (ch as u32 >= 0x21 && ch as u32 <= 0x7E),
                "non-printable char {:?} in emission: {}", ch, emitted,
            );
        }
    }

    /// Content hash is deterministic across a round-trip.
    #[test]
    fn iac_type_content_hash_stable_on_round_trip(ty in arb_iac_type()) {
        let original = ty.content_hash();
        let round = IacType::from_sexpr(&ty.to_sexpr()).unwrap().content_hash();
        prop_assert_eq!(original, round);
    }

    /// Equal values produce equal content hashes; distinct values
    /// (overwhelmingly) produce distinct hashes.
    #[test]
    fn iac_type_hash_distinguishes_scalars(
        (a, b) in (
            prop_oneof![
                Just(IacType::String),
                Just(IacType::Integer),
                Just(IacType::Boolean),
                Just(IacType::Any),
                Just(IacType::Numeric),
                Just(IacType::Float),
            ],
            prop_oneof![
                Just(IacType::String),
                Just(IacType::Integer),
                Just(IacType::Boolean),
                Just(IacType::Any),
                Just(IacType::Numeric),
                Just(IacType::Float),
            ],
        )
    ) {
        if a == b {
            prop_assert_eq!(a.content_hash(), b.content_hash());
        } else {
            prop_assert_ne!(a.content_hash(), b.content_hash());
        }
    }

    /// Content hash survives an emit→parse→hash round trip.
    #[test]
    fn iac_type_content_hash_survives_text_boundary(ty in arb_iac_type()) {
        let hash_a = ty.content_hash();
        let text = ty.to_sexpr().emit();
        let reparsed = SExpr::parse(&text).unwrap();
        let hash_b = reparsed.content_hash();
        prop_assert_eq!(hash_a, hash_b);
    }

    /// Parens are balanced in any emitted IacType.
    #[test]
    fn iac_type_parens_balanced(ty in arb_iac_type()) {
        let emitted = ty.to_sexpr().emit();
        // Count parens outside of string literals.
        let mut depth = 0i64;
        let mut in_string = false;
        let mut escaped = false;
        for ch in emitted.chars() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    in_string = false;
                }
            } else if ch == '"' {
                in_string = true;
            } else if ch == '(' {
                depth += 1;
            } else if ch == ')' {
                depth -= 1;
                prop_assert!(depth >= 0, "unmatched closing paren: {}", emitted);
            }
        }
        prop_assert_eq!(depth, 0, "unbalanced parens: {}", emitted);
    }
}
