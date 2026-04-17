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

// ── Backend → ProvenMorphism blanket impl ────────────────────────────
//
// Every `Backend` is, by definition, a morphism from (resource, provider)
// to a list of artifacts. Rather than ask each of the 7 backends to
// impl `ProvenMorphism` by hand, we provide a blanket impl so they all
// get it for free. The invariants checked are what every backend must
// satisfy in any case:
//
// 1. Determinism: calling `generate_resource` twice with the same inputs
//    returns artifacts with identical (path, content, kind) triples.
// 2. No empty artifacts: every produced artifact has a non-empty path
//    and non-empty content.
// 3. No duplicate paths: an artifact list never contains two entries
//    with the same output path.
//
// These are the *same* invariants iac-forge's existing backend tests
// already enforce test-by-test; the blanket impl lifts them into the
// type system so composition (and cross-backend analysis) can rely on
// them structurally.

/// Wrapper identifying the (resource, provider) pair as a source value.
///
/// Needed because `ProvenMorphism<Src, Dst>` requires a single Src type,
/// and `Backend::generate_resource` takes two references.
#[derive(Debug, Clone)]
pub struct ResourceInput<'a> {
    pub resource: &'a crate::ir::IacResource,
    pub provider: &'a crate::ir::IacProvider,
}

impl<B: crate::backend::Backend> Morphism<ResourceInput<'_>, Vec<crate::backend::GeneratedArtifact>>
    for B
{
    fn name(&self) -> &'static str {
        "Backend::generate_resource"
    }

    fn apply(&self, src: &ResourceInput<'_>) -> Vec<crate::backend::GeneratedArtifact> {
        use crate::sexpr::ToSExpr;
        // A `Morphism` is total by contract. Backend errors collapse to
        // an empty artifact list — the invariant check below will flag
        // that as a violation so the caller sees the failure via
        // `check_invariants` rather than a panic here.
        let mut artifacts =
            <B as crate::backend::Backend>::generate_resource(self, src.resource, src.provider)
                .unwrap_or_default();

        // Populate provenance on each artifact: content hash of the
        // source IR + the morphism chain that produced it. Backends
        // that set their own provenance are respected (we only touch
        // artifacts whose source_hash is still empty).
        let source_hash = src.resource.content_hash().to_hex();
        let platform = <B as crate::backend::Backend>::platform(self);
        let chain = vec![
            format!("Backend::{platform}"),
            "generate_resource".to_string(),
        ];
        for a in &mut artifacts {
            if a.source_hash.is_empty() {
                a.source_hash.clone_from(&source_hash);
            }
            if a.morphism_chain.is_empty() {
                a.morphism_chain = chain.clone();
            }
        }
        artifacts
    }
}

impl<B: crate::backend::Backend>
    ProvenMorphism<ResourceInput<'_>, Vec<crate::backend::GeneratedArtifact>> for B
{
    fn check_invariants(
        &self,
        src: &ResourceInput<'_>,
        dst: &Vec<crate::backend::GeneratedArtifact>,
    ) -> Vec<String> {
        let mut violations = Vec::new();

        // Determinism: re-run (through apply, which also populates
        // provenance so the comparison is apples-to-apples) must match.
        let rerun = <B as Morphism<_, _>>::apply(self, src);
        if &rerun != dst {
            violations.push(format!(
                "backend {}: non-deterministic — re-run differs",
                <B as crate::backend::Backend>::platform(self),
            ));
        }

        // Empty outputs are always a violation: every backend must emit
        // something for a well-formed resource.
        if dst.is_empty() {
            violations.push(format!(
                "backend {}: empty artifact list",
                <B as crate::backend::Backend>::platform(self),
            ));
        }

        // No empty paths or contents.
        for a in dst {
            if a.path.is_empty() {
                violations.push(format!(
                    "backend {}: artifact with empty path",
                    <B as crate::backend::Backend>::platform(self),
                ));
            }
            if a.content.is_empty() {
                violations.push(format!(
                    "backend {}: artifact '{}' has empty content",
                    <B as crate::backend::Backend>::platform(self),
                    a.path,
                ));
            }
        }

        // No duplicate paths.
        let mut paths: Vec<&str> = dst.iter().map(|a| a.path.as_str()).collect();
        paths.sort_unstable();
        for pair in paths.windows(2) {
            if pair[0] == pair[1] {
                violations.push(format!(
                    "backend {}: duplicate artifact path '{}'",
                    <B as crate::backend::Backend>::platform(self),
                    pair[0],
                ));
            }
        }

        violations
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

    // ── Blanket Backend → ProvenMorphism tests ────────────────────

    use crate::backend::{ArtifactKind, Backend, GeneratedArtifact, NamingConvention};
    use crate::error::IacForgeError;
    use crate::ir::{IacDataSource, IacProvider, IacResource};

    struct GoodBackend;
    struct GoodNaming;
    impl NamingConvention for GoodNaming {
        fn resource_type_name(&self, r: &str, p: &str) -> String {
            format!("{p}_{r}")
        }
        fn file_name(&self, r: &str, _k: &ArtifactKind) -> String {
            format!("{r}.out")
        }
        fn field_name(&self, n: &str) -> String {
            n.to_string()
        }
    }
    impl Backend for GoodBackend {
        fn platform(&self) -> &str {
            "good"
        }
        fn naming(&self) -> &dyn NamingConvention {
            &GoodNaming
        }
        fn generate_resource(
            &self,
            _r: &IacResource,
            _p: &IacProvider,
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![GeneratedArtifact { 
                path: "main.out".into(),
                content: "body".into(),
                kind: ArtifactKind::Resource,
                source_hash: String::new(),
                morphism_chain: Vec::new(),
            }])
        }
        fn generate_data_source(
            &self,
            _d: &IacDataSource,
            _p: &IacProvider,
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![])
        }
        fn generate_provider(
            &self,
            _p: &IacProvider,
            _r: &[IacResource],
            _d: &[IacDataSource],
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![])
        }
        fn generate_test(
            &self,
            _r: &IacResource,
            _p: &IacProvider,
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![])
        }
    }

    fn sample_input() -> (crate::ir::IacResource, crate::ir::IacProvider) {
        (
            crate::testing::test_resource("widget"),
            crate::testing::test_provider("acme"),
        )
    }

    #[test]
    fn backend_is_a_morphism() {
        let (r, p) = sample_input();
        let input = ResourceInput { resource: &r, provider: &p };
        let out = <GoodBackend as Morphism<_, _>>::apply(&GoodBackend, &input);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].path, "main.out");
    }

    #[test]
    fn backend_morphism_proofs_hold_on_good_backend() {
        let (r, p) = sample_input();
        let input = ResourceInput { resource: &r, provider: &p };
        let out = <GoodBackend as Morphism<_, _>>::apply(&GoodBackend, &input);
        let violations = <GoodBackend as ProvenMorphism<_, _>>::check_invariants(
            &GoodBackend,
            &input,
            &out,
        );
        assert!(violations.is_empty(), "violations: {violations:?}");
    }

    // A backend that returns duplicate paths — must be caught.
    struct DupBackend;
    impl Backend for DupBackend {
        fn platform(&self) -> &str {
            "dup"
        }
        fn naming(&self) -> &dyn NamingConvention {
            &GoodNaming
        }
        fn generate_resource(
            &self,
            _r: &IacResource,
            _p: &IacProvider,
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![
                GeneratedArtifact { 
                    path: "dup.out".into(),
                    content: "a".into(),
                    kind: ArtifactKind::Resource,
                    source_hash: String::new(),
                    morphism_chain: Vec::new(),
                },
                GeneratedArtifact { 
                    path: "dup.out".into(),
                    content: "b".into(),
                    kind: ArtifactKind::Resource,
                    source_hash: String::new(),
                    morphism_chain: Vec::new(),
                },
            ])
        }
        fn generate_data_source(
            &self,
            _d: &IacDataSource,
            _p: &IacProvider,
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![])
        }
        fn generate_provider(
            &self,
            _p: &IacProvider,
            _r: &[IacResource],
            _d: &[IacDataSource],
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![])
        }
        fn generate_test(
            &self,
            _r: &IacResource,
            _p: &IacProvider,
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![])
        }
    }

    #[test]
    fn backend_morphism_catches_duplicate_paths() {
        let (r, p) = sample_input();
        let input = ResourceInput { resource: &r, provider: &p };
        let out = <DupBackend as Morphism<_, _>>::apply(&DupBackend, &input);
        let violations = <DupBackend as ProvenMorphism<_, _>>::check_invariants(
            &DupBackend,
            &input,
            &out,
        );
        assert!(
            violations.iter().any(|v| v.contains("duplicate artifact path")),
            "expected duplicate-path violation: {violations:?}",
        );
    }

    // A backend that returns an empty artifact list — must be caught.
    struct EmptyBackend;
    impl Backend for EmptyBackend {
        fn platform(&self) -> &str {
            "empty"
        }
        fn naming(&self) -> &dyn NamingConvention {
            &GoodNaming
        }
        fn generate_resource(
            &self,
            _r: &IacResource,
            _p: &IacProvider,
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![])
        }
        fn generate_data_source(
            &self,
            _d: &IacDataSource,
            _p: &IacProvider,
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![])
        }
        fn generate_provider(
            &self,
            _p: &IacProvider,
            _r: &[IacResource],
            _d: &[IacDataSource],
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![])
        }
        fn generate_test(
            &self,
            _r: &IacResource,
            _p: &IacProvider,
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![])
        }
    }

    // ── Provenance population ────────────────────────────────

    #[test]
    fn backend_morphism_populates_source_hash() {
        use crate::sexpr::ToSExpr;
        let (r, p) = sample_input();
        let input = ResourceInput { resource: &r, provider: &p };
        let out = <GoodBackend as Morphism<_, _>>::apply(&GoodBackend, &input);
        assert_eq!(out.len(), 1);
        let expected_hash = r.content_hash().to_hex();
        assert_eq!(out[0].source_hash, expected_hash);
        assert!(out[0].has_provenance());
    }

    #[test]
    fn backend_morphism_populates_morphism_chain() {
        let (r, p) = sample_input();
        let input = ResourceInput { resource: &r, provider: &p };
        let out = <GoodBackend as Morphism<_, _>>::apply(&GoodBackend, &input);
        assert_eq!(out[0].morphism_chain.len(), 2);
        assert_eq!(out[0].morphism_chain[0], "Backend::good");
        assert_eq!(out[0].morphism_chain[1], "generate_resource");
    }

    #[test]
    fn backend_morphism_respects_pre_set_provenance() {
        // If a backend proactively sets provenance, the blanket impl
        // must not overwrite it.
        struct PreSetBackend;
        impl Backend for PreSetBackend {
            fn platform(&self) -> &str { "preset" }
            fn naming(&self) -> &dyn NamingConvention { &GoodNaming }
            fn generate_resource(
                &self,
                _r: &IacResource,
                _p: &IacProvider,
            ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
                Ok(vec![GeneratedArtifact {
                    path: "main.out".into(),
                    content: "body".into(),
                    kind: ArtifactKind::Resource,
                    source_hash: "manually-set".into(),
                    morphism_chain: vec!["custom".into()],
                }])
            }
            fn generate_data_source(&self, _d: &IacDataSource, _p: &IacProvider) -> Result<Vec<GeneratedArtifact>, IacForgeError> { Ok(vec![]) }
            fn generate_provider(&self, _p: &IacProvider, _r: &[IacResource], _d: &[IacDataSource]) -> Result<Vec<GeneratedArtifact>, IacForgeError> { Ok(vec![]) }
            fn generate_test(&self, _r: &IacResource, _p: &IacProvider) -> Result<Vec<GeneratedArtifact>, IacForgeError> { Ok(vec![]) }
        }

        let (r, p) = sample_input();
        let input = ResourceInput { resource: &r, provider: &p };
        let out = <PreSetBackend as Morphism<_, _>>::apply(&PreSetBackend, &input);
        assert_eq!(out[0].source_hash, "manually-set");
        assert_eq!(out[0].morphism_chain, vec!["custom".to_string()]);
    }

    #[test]
    fn backend_morphism_source_hash_is_deterministic() {
        // Same IR should always yield the same source hash.
        let (r, p) = sample_input();
        let input = ResourceInput { resource: &r, provider: &p };
        let a = <GoodBackend as Morphism<_, _>>::apply(&GoodBackend, &input);
        let b = <GoodBackend as Morphism<_, _>>::apply(&GoodBackend, &input);
        assert_eq!(a[0].source_hash, b[0].source_hash);
    }

    #[test]
    fn backend_morphism_different_resources_different_hashes() {
        let p = crate::testing::test_provider("acme");
        let r1 = crate::testing::test_resource("widget");
        let r2 = crate::testing::test_resource("gadget");
        let out1 = <GoodBackend as Morphism<_, _>>::apply(
            &GoodBackend,
            &ResourceInput { resource: &r1, provider: &p },
        );
        let out2 = <GoodBackend as Morphism<_, _>>::apply(
            &GoodBackend,
            &ResourceInput { resource: &r2, provider: &p },
        );
        assert_ne!(out1[0].source_hash, out2[0].source_hash);
    }

    #[test]
    fn backend_morphism_catches_empty_artifact_list() {
        let (r, p) = sample_input();
        let input = ResourceInput { resource: &r, provider: &p };
        let out = <EmptyBackend as Morphism<_, _>>::apply(&EmptyBackend, &input);
        let violations = <EmptyBackend as ProvenMorphism<_, _>>::check_invariants(
            &EmptyBackend,
            &input,
            &out,
        );
        assert!(
            violations.iter().any(|v| v.contains("empty artifact list")),
            "expected empty-list violation: {violations:?}",
        );
    }
}
