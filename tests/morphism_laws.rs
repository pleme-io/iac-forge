//! Morphism laws — property tests for Morphism, Composed, Identity.
//!
//! Proves:
//! - Identity is a left and right unit of composition
//! - Composition is associative (apply-equivalent)
//! - Composed is deterministic when both parts are
//! - ProvenMorphism composition preserves zero-violation invariants
//! - Violations always trace to the originating morphism by name

use proptest::prelude::*;

use iac_forge::morphism::{Composed, Identity, Morphism, ProvenMorphism};

// ── Concrete morphisms used in laws ─────────────────────────────────

struct Double;
impl Morphism<i64, i64> for Double {
    fn name(&self) -> &'static str {
        "Double"
    }
    fn apply(&self, x: &i64) -> i64 {
        x.wrapping_mul(2)
    }
}
impl ProvenMorphism<i64, i64> for Double {
    fn check_invariants(&self, src: &i64, dst: &i64) -> Vec<String> {
        if *dst == src.wrapping_mul(2) {
            Vec::new()
        } else {
            vec!["dst != 2 * src".into()]
        }
    }
}

struct AddOne;
impl Morphism<i64, i64> for AddOne {
    fn name(&self) -> &'static str {
        "AddOne"
    }
    fn apply(&self, x: &i64) -> i64 {
        x.wrapping_add(1)
    }
}
impl ProvenMorphism<i64, i64> for AddOne {
    fn check_invariants(&self, src: &i64, dst: &i64) -> Vec<String> {
        if *dst == src.wrapping_add(1) {
            Vec::new()
        } else {
            vec!["dst != src + 1".into()]
        }
    }
}

proptest! {
    /// Identity is a right unit on Double.
    #[test]
    fn double_right_identity(x in any::<i64>()) {
        let direct = Double.apply(&x);
        let via_composed = Composed::new(Double, Identity::<i64>::default()).apply(&x);
        prop_assert_eq!(direct, via_composed);
    }

    /// Identity is a left unit on Double.
    #[test]
    fn double_left_identity(x in any::<i64>()) {
        let direct = Double.apply(&x);
        let via_composed = Composed::new(Identity::<i64>::default(), Double).apply(&x);
        prop_assert_eq!(direct, via_composed);
    }

    /// Associativity: ((f; g); h).apply == (f; (g; h)).apply for all x.
    #[test]
    fn triple_composition_is_associative(x in any::<i64>()) {
        let left = Composed::new(Composed::new(Double, AddOne), Double);
        let right = Composed::new(Double, Composed::new(AddOne, Double));
        prop_assert_eq!(left.apply(&x), right.apply(&x));
    }

    /// Determinism: Composed is deterministic when parts are.
    #[test]
    fn composed_is_deterministic(x in any::<i64>()) {
        let c = Composed::new(Double, AddOne);
        prop_assert_eq!(c.apply(&x), c.apply(&x));
    }

    /// Proof preservation: composing two proven morphisms on the result
    /// of apply yields zero violations.
    #[test]
    fn composed_proof_holds_on_apply(x in any::<i64>()) {
        let c = Composed::new(Double, AddOne);
        let y = c.apply(&x);
        prop_assert!(c.check_invariants(&x, &y).is_empty());
    }

    /// Identity preserves invariants trivially.
    #[test]
    fn identity_proves_itself(x in any::<i64>()) {
        let id = Identity::<i64>::default();
        prop_assert!(id.check_invariants(&x, &x).is_empty());
    }

    /// Triple composed proof holds.
    #[test]
    fn triple_composed_proof_holds(x in any::<i64>()) {
        let c = Composed::new(Composed::new(Double, AddOne), Double);
        let y = c.apply(&x);
        prop_assert!(c.check_invariants(&x, &y).is_empty());
    }

    /// Apply is total — no panics for any i64 input (checked by wrapping_* ops).
    #[test]
    fn composition_is_total(x in any::<i64>()) {
        let c = Composed::new(Composed::new(Double, AddOne), Double);
        let _ = c.apply(&x);
    }
}

// ── Violation traceability ──────────────────────────────────────────

struct BadDouble;
impl Morphism<i64, i64> for BadDouble {
    fn name(&self) -> &'static str {
        "BadDouble"
    }
    fn apply(&self, x: &i64) -> i64 {
        x.wrapping_mul(3) // intentionally wrong
    }
}
impl ProvenMorphism<i64, i64> for BadDouble {
    fn check_invariants(&self, src: &i64, dst: &i64) -> Vec<String> {
        if *dst == src.wrapping_mul(2) {
            Vec::new()
        } else {
            vec!["dst != 2 * src".into()]
        }
    }
}

proptest! {
    /// Traceability: when the first stage lies, the violation is tagged
    /// with that stage's name.
    #[test]
    fn violation_tags_source_morphism(x in -1_000_000_000_i64..1_000_000_000_i64) {
        let c = Composed::new(BadDouble, AddOne);
        let y = c.apply(&x);
        let violations = c.check_invariants(&x, &y);
        prop_assert!(!violations.is_empty(), "BadDouble should violate");
        prop_assert!(
            violations.iter().any(|v| v.contains("BadDouble")),
            "no BadDouble tag in {:?}", violations,
        );
    }
}
