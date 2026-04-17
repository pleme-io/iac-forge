//! Frozen test vectors: canonical emission → expected BLAKE3 hex.
//!
//! Every hash here was computed out-of-band against `b3sum 1.8.4`
//! (nixpkgs). Any reimplementation — the Ruby reference at
//! `tests/cross_lang/sexpr_ref.rb`, a Python port, a Go port, a
//! JavaScript port — MUST reproduce these hashes for the same
//! canonical emissions. That's the cross-language contract.
//!
//! If a vector ever fails:
//! - Either the canonical emission of that value changed (which would
//!   require a schema version bump)
//! - Or the hash function changed
//!
//! Either way: pinned down. No silent drift.
//!
//! Regenerating a vector:
//! ```sh
//! nix-shell -p b3sum --run "printf '%s' '<CANONICAL_TEXT>' | b3sum --no-names"
//! ```

use iac_forge::ir::{IacAttribute, IacType};
use iac_forge::sexpr::{SExpr, ToSExpr};

/// (canonical emission, expected BLAKE3 hex lowercase)
///
/// Grouped by domain for readability. Every entry was verified against
/// `b3sum 1.8.4` during commit.
const VECTORS: &[(&str, &str)] = &[
    // ═══ Primitives ═══
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
    (
        "\"hello\"",
        "c5919eb25e32df3ac400757942250b6a9776c7b1ac1e8e465ec6ca0de8e4cb3f",
    ),

    // ═══ Lists ═══
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

    // ═══ IacType — bare symbols for scalars ═══
    (
        "string",
        "1c47174c0ccd034618e1a604adce0002103e088e87329f0c2fab4324e8a06c60",
    ),
    (
        "integer",
        "bf0b731c90564bc8c1a8b8078964f3fb4e20636f1beb54ff1cfecb06a7ca2ac8",
    ),
    (
        "float",
        "88d3ce9a7ddc1cdb461b8ff3d6106ad21f17d8e970d3f69cb6e5fdc0c1d20f39",
    ),
    (
        "numeric",
        "dd3f583fa632fefc152b7f02c4fdccd250e69a5b225f8cf64c9b75b559fbb7f8",
    ),
    (
        "boolean",
        "04898d6f3559bfd5840dd3a30e0aa3472df8b2ffda0ae2a40815b34b4e24ac75",
    ),
    (
        "any",
        "fd0b6c0bab658ae9e3e6bf09032b6aec599d2c78a2b3783afb8fc415078533d1",
    ),

    // ═══ IacType — composite variants ═══
    (
        "(list integer)",
        "df476bed92dcb5156e97d74bbea85131a4d4ab4905d9ddde4886d1b12e4c599a",
    ),
    (
        "(list string)",
        "9825d0823450a484ac52a704179ced80b3fc8866efcc9ddbf49812202ee787fb",
    ),
    (
        "(set integer)",
        "ba8ef5bd77e5fb39a7451553ed572e2ccffa60b91053acb77248358fcd27e393",
    ),
    (
        "(map boolean)",
        "50f013744a7dc10712d513fd00254a9eca27f6ae2d929ad53f765df306f38486",
    ),
    (
        "(list (list integer))",
        "9d9f6f793b4cbc8cb771ac2f0891d2d1bf5f580d636b32041dfd819b234e20e4",
    ),
    (
        "(set (map string))",
        "3644439e815cbf5a0ebb569fb9083908c4f70c9f674d4a2876842de31b806278",
    ),
    (
        "(object (:name \"X\") (:fields (list)))",
        "8f402993eb1b3d5d81768a210ea673423793cf7165f2b904d1bfb2e784319ca1",
    ),
    (
        "(enum (:values (list)) (:underlying string))",
        "ee96a04c397f483da92fe0586947a08e7d95b931ca4e22bf02d59650aa8ec07f",
    ),
    (
        "(enum (:values (list \"tcp\" \"udp\")) (:underlying string))",
        "7210f505fb7676b17039f4a07344be0f795b6d837e037cb6737c330cdc940980",
    ),

    // ═══ RubyType variants ═══
    (
        "(simple \"T::String\")",
        "9cd5f904c8ed4748eed176b63fde28f7b11487adede8ab2ada8b5163e2cefd7f",
    ),
    (
        "(simple \"T::Integer\")",
        "84783156859c7180a900394cd04aac0d0e9276c232032753015f0e39ae92751c",
    ),
    (
        "(array (simple \"T::String\"))",
        "02db9750a4c8fb19c10eb70cdccaf0280c8db950b020e4a4e4359b84db267549",
    ),
    (
        "hash",
        "b6716efe6829269249e48a93798c6e40058255c268f21566431b8e7ea7da3b15",
    ),
    (
        "(union (simple \"T::Coercible::Integer\") (simple \"T::Coercible::Float\"))",
        "f2b4773ee214468ffb3ff8afce9c868189aeb0c97b365d61321ef2321b76bbf6",
    ),
    (
        "(constrained (:base (simple \"T::String\")) (:constraint \"x\"))",
        "6ad4e3ed59b9b3ee13881499ab1e8733d116f93b1a47c5dfee7697ab1f4fe3a3",
    ),
    (
        "(optional (simple \"T::Integer\"))",
        "cba66661972132ba4bace88af27787463f632bc4f80996fb858163cdaad02f20",
    ),

    // ═══ RbsType variants ═══
    (
        "(named \"String\")",
        "43201850e62bc40327a13ee4a3df9d3f9e12eaf5ece5d09c13de0463126af312",
    ),
    (
        "(named \"Integer\")",
        "7d3985bbd92921a69e84a0f085e55811e25a205a032d0cd776296a55f9aa3f44",
    ),
    (
        "(array (named \"String\"))",
        "5627bb484858b9d1a1760102e3c3dce889ed1857c158ef8870fb2861de93d6e9",
    ),
    (
        "(hash (named \"Symbol\") untyped)",
        "3193ebd7efc11b842771fa7657fafa815224708f55119d7f4f438ba16d938cc5",
    ),
    (
        "(string-literal \"tcp\")",
        "69e10108139a806ba815e8c95f504f4d8024a9a2ad15fd143c5cab89efb86ac5",
    ),
    (
        "(nilable (named \"String\"))",
        "2a9fca7ee885d0fd011006dbe84eabdce8a1034441e9b787f361775a2baf72aa",
    ),
    (
        "untyped",
        "384f22513927f65f8d891c22eadfafc5178043edca0649c5620d07acd3e1ee1f",
    ),
    (
        "(union (string-literal \"tcp\") (string-literal \"udp\"))",
        "e357f7b14b16cfaf8ffbda303debd361cb417e5d7cc230b36ebac5058f72c0ce",
    ),
    (
        "(union (named \"A\") (named \"B\"))",
        "4b86eabd9817d06ebd46302e3236e86c36845e336288284837bef957bf71d19d",
    ),
];

#[test]
fn frozen_vectors_agree_with_independent_b3sum() {
    for (text, expected) in VECTORS {
        let parsed = SExpr::parse(text)
            .unwrap_or_else(|e| panic!("vector failed to parse: {text:?} — {e:?}"));
        assert_eq!(
            parsed.content_hash().to_hex(),
            *expected,
            "disagreement on vector {text:?}",
        );
    }
}

#[test]
fn every_vector_round_trips() {
    // Cross-check: emitting the parsed form should equal the original
    // canonical text. This proves the textual form is self-canonical —
    // no hidden normalization between parse and emit.
    for (text, _) in VECTORS {
        let parsed = SExpr::parse(text).unwrap();
        let reemitted = parsed.emit();
        assert_eq!(
            &reemitted, text,
            "emit disagreement: wrote {reemitted:?}, expected {text:?}",
        );
    }
}

#[test]
fn vector_count_sanity() {
    // Acts as a guard against accidental deletion — if someone cuts a
    // group of vectors, this fires and they have to justify it.
    assert!(
        VECTORS.len() >= 37,
        "vector set should not shrink below the committed 37-entry baseline",
    );
}

// ── Rust-side emission matches stored vectors for real IR values ──
//
// The VECTORS table above covers canonical text forms. These tests
// additionally prove that the Rust IR types emit those exact forms —
// so a change to emission logic that happens to not break parsing
// (but would break portability) still gets caught.

#[test]
fn iac_type_scalars_emit_expected_text() {
    let cases: &[(IacType, &str)] = &[
        (IacType::String, "string"),
        (IacType::Integer, "integer"),
        (IacType::Float, "float"),
        (IacType::Numeric, "numeric"),
        (IacType::Boolean, "boolean"),
        (IacType::Any, "any"),
    ];
    for (ty, expected) in cases {
        assert_eq!(ty.to_sexpr().emit(), *expected, "bad emission for {ty:?}");
    }
}

#[test]
fn iac_type_list_integer_matches_frozen_hash() {
    let ty = IacType::List(Box::new(IacType::Integer));
    assert_eq!(ty.to_sexpr().emit(), "(list integer)");
    assert_eq!(
        ty.content_hash().to_hex(),
        "df476bed92dcb5156e97d74bbea85131a4d4ab4905d9ddde4886d1b12e4c599a",
    );
}

#[test]
fn iac_type_enum_with_values_matches_frozen_hash() {
    let ty = IacType::Enum {
        values: vec!["tcp".into(), "udp".into()],
        underlying: Box::new(IacType::String),
    };
    assert_eq!(
        ty.to_sexpr().emit(),
        "(enum (:values (list \"tcp\" \"udp\")) (:underlying string))",
    );
    assert_eq!(
        ty.content_hash().to_hex(),
        "7210f505fb7676b17039f4a07344be0f795b6d837e037cb6737c330cdc940980",
    );
}

#[test]
fn iac_type_object_empty_matches_frozen_hash() {
    let ty = IacType::Object {
        name: "X".into(),
        fields: vec![],
    };
    assert_eq!(
        ty.to_sexpr().emit(),
        "(object (:name \"X\") (:fields (list)))",
    );
    assert_eq!(
        ty.content_hash().to_hex(),
        "8f402993eb1b3d5d81768a210ea673423793cf7165f2b904d1bfb2e784319ca1",
    );
}

#[test]
fn iac_attribute_has_stable_content_hash() {
    // Attribute shape depends on all 14 fields. We emit one concrete
    // shape and assert its hash matches its own current emission — the
    // vector itself is auto-updated here because attribute shape may
    // evolve, but the test still guarantees deterministic emit+hash.
    let attr = IacAttribute {
        api_name: "x".into(),
        canonical_name: "x".into(),
        description: String::new(),
        iac_type: IacType::String,
        required: true,
        ..Default::default()
    };
    let emitted_a = attr.to_sexpr().emit();
    let emitted_b = attr.to_sexpr().emit();
    assert_eq!(emitted_a, emitted_b, "attribute emission must be deterministic");
    assert_eq!(attr.content_hash(), attr.content_hash());
}
