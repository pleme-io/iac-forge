//! Frozen test vectors: canonical emission → expected BLAKE3 hex.
//!
//! These hashes were verified independently against the standard
//! `b3sum` command-line tool (from nixpkgs):
//!
//! ```text
//! $ printf '(list integer)' | b3sum --no-names
//! df476bed92dcb5156e97d74bbea85131a4d4ab4905d9ddde4886d1b12e4c599a
//! ```
//!
//! Any reimplementation (the Ruby reference in
//! `tests/cross_lang/sexpr_ref.rb`, a Python port, a Go port, etc.)
//! MUST produce these same hashes for the same canonical emissions.
//! This test is the portable contract — if it ever fails, either the
//! emission changed (shouldn't happen without bumping schema) or the
//! hash function changed (ditto).

use iac_forge::ir::IacType;
use iac_forge::sexpr::{SExpr, ToSExpr};

/// (canonical emission, expected BLAKE3 hex lowercase)
///
/// Verified out-of-band against `b3sum 1.8.4` — see module docs.
const VECTORS: &[(&str, &str)] = &[
    // Bare symbols (IacType unit variants)
    (
        "integer",
        "bf0b731c90564bc8c1a8b8078964f3fb4e20636f1beb54ff1cfecb06a7ca2ac8",
    ),
    (
        "string",
        "1c47174c0ccd034618e1a604adce0002103e088e87329f0c2fab4324e8a06c60",
    ),
    // Primitives
    (
        "42",
        "da136474d7575c325f702bb7aa75f1123864033cc488bf7d9c074eadaf9bd0d3",
    ),
    (
        "true",
        "acc8a7699a2bf4cbd05f69678eac4fc236572041c28dfd0ab558e5fcf2ab6540",
    ),
    (
        "nil",
        "f2ebafc4960f3bcffbd79b2478942adcbc0846fd66f3700ecb552754ddd883c5",
    ),
    // Lists
    (
        "()",
        "622be36e2f51ac6ced2c7c6f12649a71d41650d1e47b9a83f58d7dbc9983f0fa",
    ),
    (
        "(list)",
        "e4d01eff7e2457426da0f6112ba2665e83dc554276221bee97584828cb548333",
    ),
    (
        "(list 1 2 3)",
        "218f16f9feb222135d920d7924f961f561487d6aab171f9241c204731b1e3606",
    ),
    (
        "(list string)",
        "9825d0823450a484ac52a704179ced80b3fc8866efcc9ddbf49812202ee787fb",
    ),
    // Tuple-tag IacType composite
    (
        "(list integer)",
        "df476bed92dcb5156e97d74bbea85131a4d4ab4905d9ddde4886d1b12e4c599a",
    ),
    // String literal
    (
        "\"hello\"",
        "c5919eb25e32df3ac400757942250b6a9776c7b1ac1e8e465ec6ca0de8e4cb3f",
    ),
];

#[test]
fn frozen_vectors_agree_with_independent_b3sum() {
    for (text, expected) in VECTORS {
        let parsed = SExpr::parse(text).expect("vector parses");
        // Two independent paths to the hash must agree:
        // (a) hash of the re-emitted canonical text
        // (b) the stored expected value (computed by b3sum)
        assert_eq!(
            parsed.content_hash().to_hex(),
            *expected,
            "disagreement on text: {text:?}",
        );
    }
}

#[test]
fn iac_type_list_integer_matches_frozen_hash() {
    let ty = IacType::List(Box::new(IacType::Integer));
    let emitted = ty.to_sexpr().emit();
    assert_eq!(emitted, "(list integer)");
    assert_eq!(
        ty.content_hash().to_hex(),
        "df476bed92dcb5156e97d74bbea85131a4d4ab4905d9ddde4886d1b12e4c599a",
    );
}
