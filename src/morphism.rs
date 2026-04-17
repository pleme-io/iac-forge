//! Structure-preserving maps between types — morphisms in the categorical sense.
//!
//! This module lifts what `Backend` already does (take an `IacResource`, produce
//! artifacts) into a composable, proof-bearing primitive. Every
//! synthesizer in the pipeline is a morphism from one typed domain to
//! another — `IacType → RubyType`, `RubyType → String`, `IacResource →
//! GeneratedArtifact` — and when two morphisms compose, their proofs
//! compose with them.
//!
//! # Why this exists
//!
//! The platform's 2,739-test proof engine attaches invariants to types
//! (`IacType` is injective to `RubyType`, render is deterministic, etc.).
//! But the *glue* between types — the actual transformation functions —
//! has been plain Rust fns. A plain fn carries no first-class proof;
//! proofs live in adjacent test files.
//!
//! `Morphism` makes the transformation itself a value you can compose,
//! name, and attach invariants to. `ProvenMorphism` adds a
//! `check_invariants` hook: the morphism declares what must hold about
//! every (src, dst) pair it produces. Composition naturally inherits
//! both participants' invariants.
//!
//! # Minimal API
//!
//! ```rust
//! use iac_forge::morphism::{Morphism, Composed, ProvenMorphism};
//!
//! struct Double;
//! impl Morphism<i64, i64> for Double {
//!     fn name(&self) -> &'static str { "Double" }
//!     fn apply(&self, x: &i64) -> i64 { x * 2 }
//! }
//!
//! struct AddOne;
//! impl Morphism<i64, i64> for AddOne {
//!     fn name(&self) -> &'static str { "AddOne" }
//!     fn apply(&self, x: &i64) -> i64 { x + 1 }
//! }
//!
//! let composed = Composed::new(Double, AddOne);
//! assert_eq!(composed.apply(&3), 7); // (3 * 2) + 1
//! ```

use std::marker::PhantomData;

/// A structure-preserving map from `Src` to `Dst`.
///
/// Implementations should be **total** (every `Src` maps to exactly one
/// `Dst`) and **deterministic** (same input → same output, always). These
/// are the invariants composition depends on; `ProvenMorphism` lets you
/// declare them explicitly.
pub trait Morphism<Src, Dst> {
    /// Human-readable name for diagnostics, traceability, attestation.
    fn name(&self) -> &'static str;

    /// Apply the morphism. Must be total and deterministic.
    fn apply(&self, src: &Src) -> Dst;
}

/// A morphism that can verify its own invariants for a given (src, dst) pair.
///
/// The empty return vector means "all invariants hold." Non-empty is a
/// list of human-readable violations, intended for test-time or debug-time
/// assertion. Production code paths should not need to run checks at
/// runtime — the point is that the morphism *would* satisfy them.
///
/// Composition: when you compose two `ProvenMorphism`s, the resulting
/// morphism's invariants are the union of both, checked at the
/// intermediate and final values.
pub trait ProvenMorphism<Src, Dst>: Morphism<Src, Dst> {
    /// Return violations (empty = all invariants hold).
    fn check_invariants(&self, src: &Src, dst: &Dst) -> Vec<String>;
}

/// Sequential composition: `A -> B` followed by `B -> C` gives `A -> C`.
///
/// Requires `Mid: Clone` because the intermediate value must be
/// available both to the second morphism and (optionally) to the
/// composed invariant check.
pub struct Composed<A, B, C, M1, M2> {
    first: M1,
    second: M2,
    _src: PhantomData<fn(&A) -> A>,
    _mid: PhantomData<fn(&B) -> B>,
    _dst: PhantomData<fn(&C) -> C>,
}

impl<A, B, C, M1, M2> Composed<A, B, C, M1, M2>
where
    M1: Morphism<A, B>,
    M2: Morphism<B, C>,
{
    /// Construct a composed morphism `first; second`.
    pub fn new(first: M1, second: M2) -> Self {
        Self {
            first,
            second,
            _src: PhantomData,
            _mid: PhantomData,
            _dst: PhantomData,
        }
    }
}

impl<A, B, C, M1, M2> Morphism<A, C> for Composed<A, B, C, M1, M2>
where
    M1: Morphism<A, B>,
    M2: Morphism<B, C>,
{
    fn name(&self) -> &'static str {
        // A slight pragmatic compromise — we cannot format! a &'static str
        // here. The concrete name isn't printable through the trait; use
        // `named_composition` for labelled composition.
        "Composed"
    }

    fn apply(&self, src: &A) -> C {
        let mid = self.first.apply(src);
        self.second.apply(&mid)
    }
}

impl<A, B, C, M1, M2> ProvenMorphism<A, C> for Composed<A, B, C, M1, M2>
where
    M1: ProvenMorphism<A, B>,
    M2: ProvenMorphism<B, C>,
{
    fn check_invariants(&self, src: &A, dst: &C) -> Vec<String> {
        // Recompute the intermediate; both participants check their own
        // invariants against their respective (src, dst) pair. Violations
        // are prefixed with the originating morphism's name for
        // traceability.
        let mid = self.first.apply(src);
        let mut out = Vec::new();
        for v in self.first.check_invariants(src, &mid) {
            out.push(format!("[{}] {}", self.first.name(), v));
        }
        for v in self.second.check_invariants(&mid, dst) {
            out.push(format!("[{}] {}", self.second.name(), v));
        }
        out
    }
}

/// Identity morphism — useful as a unit in composition chains and as
/// the default transform in pipelines.
pub struct Identity<T>(PhantomData<fn(&T) -> T>);

impl<T> Default for Identity<T> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<T: Clone> Morphism<T, T> for Identity<T> {
    fn name(&self) -> &'static str {
        "Identity"
    }
    fn apply(&self, src: &T) -> T {
        src.clone()
    }
}

impl<T: Clone + PartialEq> ProvenMorphism<T, T> for Identity<T> {
    fn check_invariants(&self, src: &T, dst: &T) -> Vec<String> {
        if src == dst {
            Vec::new()
        } else {
            vec!["identity: src != dst".to_string()]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Double;
    impl Morphism<i64, i64> for Double {
        fn name(&self) -> &'static str {
            "Double"
        }
        fn apply(&self, x: &i64) -> i64 {
            x * 2
        }
    }
    impl ProvenMorphism<i64, i64> for Double {
        fn check_invariants(&self, src: &i64, dst: &i64) -> Vec<String> {
            if *dst == src * 2 {
                Vec::new()
            } else {
                vec!["double: dst != 2 * src".into()]
            }
        }
    }

    struct AddOne;
    impl Morphism<i64, i64> for AddOne {
        fn name(&self) -> &'static str {
            "AddOne"
        }
        fn apply(&self, x: &i64) -> i64 {
            x + 1
        }
    }
    impl ProvenMorphism<i64, i64> for AddOne {
        fn check_invariants(&self, src: &i64, dst: &i64) -> Vec<String> {
            if *dst == src + 1 {
                Vec::new()
            } else {
                vec!["addone: dst != src + 1".into()]
            }
        }
    }

    #[test]
    fn apply_composes_left_to_right() {
        let c = Composed::new(Double, AddOne);
        assert_eq!(c.apply(&3), 7); // (3 * 2) + 1
    }

    #[test]
    fn proof_composes() {
        let c = Composed::new(Double, AddOne);
        let src = 5_i64;
        let dst = c.apply(&src);
        let violations = c.check_invariants(&src, &dst);
        assert!(
            violations.is_empty(),
            "valid composition should yield zero violations: {violations:?}",
        );
    }

    #[test]
    fn proof_composes_identifies_source_of_violation() {
        struct BadDouble;
        impl Morphism<i64, i64> for BadDouble {
            fn name(&self) -> &'static str {
                "BadDouble"
            }
            fn apply(&self, x: &i64) -> i64 {
                x * 3 // wrong
            }
        }
        impl ProvenMorphism<i64, i64> for BadDouble {
            fn check_invariants(&self, src: &i64, dst: &i64) -> Vec<String> {
                if *dst == src * 2 {
                    Vec::new()
                } else {
                    vec!["double: dst != 2 * src".into()]
                }
            }
        }

        let c = Composed::new(BadDouble, AddOne);
        let src = 5_i64;
        let dst = c.apply(&src);
        let violations = c.check_invariants(&src, &dst);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].starts_with("[BadDouble]"));
    }

    #[test]
    fn identity_name_and_apply() {
        let id = Identity::<i64>::default();
        assert_eq!(id.name(), "Identity");
        assert_eq!(id.apply(&42), 42);
    }

    #[test]
    fn identity_proves_itself() {
        let id = Identity::<i64>::default();
        assert!(id.check_invariants(&42, &42).is_empty());
    }

    #[test]
    fn identity_is_right_unit_of_composition() {
        let c = Composed::new(Double, Identity::<i64>::default());
        assert_eq!(c.apply(&5), 10);
        assert!(c.check_invariants(&5, &10).is_empty());
    }

    #[test]
    fn identity_is_left_unit_of_composition() {
        let c = Composed::new(Identity::<i64>::default(), Double);
        assert_eq!(c.apply(&5), 10);
        assert!(c.check_invariants(&5, &10).is_empty());
    }

    #[test]
    fn triple_composition_proof_chain() {
        let c = Composed::new(Composed::new(Double, AddOne), Double);
        let src = 3_i64;
        let dst = c.apply(&src);
        assert_eq!(dst, 14); // ((3 * 2) + 1) * 2
        assert!(c.check_invariants(&src, &dst).is_empty());
    }

    #[test]
    fn composition_apply_is_deterministic() {
        let c = Composed::new(Double, AddOne);
        let a = c.apply(&7);
        let b = c.apply(&7);
        assert_eq!(a, b);
    }
}
