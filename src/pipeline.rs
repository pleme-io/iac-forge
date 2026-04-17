//! Typed representation pipelines: promotions and mutations with traceable provenance.
//!
//! A **pipeline** is a sequence of stages that transform a value while
//! enforcing invariants at every step. Two stage kinds capture the two
//! shapes of work that happen to software representations:
//!
//! - **Promotion** (`A → B`): cross a representation boundary. The value's
//!   TYPE changes; the meaning is preserved and typically gains proofs.
//!   Examples: `RawToml → IacResource`, `IacResource → Vec<GeneratedArtifact>`,
//!   `IacType → RubyType`, `IacResource → Nix attribute set`.
//!
//! - **Mutation** (`A → A`): stay within a representation; change content
//!   while preserving the type's invariants. Examples: applying a
//!   `ResourceOp` script, running a Nix transform, normalising field
//!   order.
//!
//! Both are morphisms. Promotions change the type parameter; mutations
//! don't. The distinction matters for quality analysis: promotions are
//! where new guarantees appear; mutations are where existing guarantees
//! must be shown to hold after the change.
//!
//! # Quality selection
//!
//! A `Stage` declares what quality it **establishes** (after this stage
//! runs, this property holds) and what qualities it **requires** (for
//! this stage to be valid, these properties must already hold on the
//! input). A pipeline executor can then validate at type-check time (or
//! run-time, via [`Trace`]) that the quality contract holds end-to-end.
//!
//! # Trace output
//!
//! Running a pipeline produces a [`Trace`] — the full audit lineage of
//! what happened at each stage. Each trace entry records:
//! - Stage name
//! - Input content hash
//! - Output content hash
//! - The quality established (if any)
//! - Any invariant violations (empty if clean)
//!
//! The Trace round-trips through sexpr via [`ToSExpr`], so it's
//! attestable, storable, and diffable like any other IR value.

use crate::morphism::{Morphism, ProvenMorphism};
use crate::sexpr::{
    parse_struct, struct_expr, take_field, ContentHash, FromSExpr, SExpr, SExprError,
    ToSExpr,
};

// ── Quality tags ─────────────────────────────────────────────────

/// A named quality a stage can require or establish.
///
/// Qualities are opaque strings by design: the set is extensible per-
/// platform without a central registry. Conventional names:
///
/// - `"validated"` — input has passed schema/spec validation
/// - `"compliance:nist-800-53"` — NIST compliance invariants hold
/// - `"compliance:pci-dss"` — PCI DSS invariants hold
/// - `"content-addressed"` — value has a stable BLAKE3 content hash
/// - `"deterministic-render"` — rendering from this produces byte-
///   identical output on re-run
/// - `"attested"` — value carries a signed provenance chain
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct Quality(pub String);

impl Quality {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Quality {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

// ── StageKind ────────────────────────────────────────────────────

/// Kind of stage: promotion crosses a type boundary, mutation stays within one.
///
/// The distinction is informational — both are morphisms, and both run
/// through `apply` + `check_invariants`. Surfacing the kind makes
/// pipeline diagrams and audit reports clearer about what's happening
/// at each step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageKind {
    /// `A → B`: value's type changes, meaning is preserved.
    Promotion,
    /// `A → A`: content changes, type (and its invariants) preserved.
    Mutation,
}

impl std::fmt::Display for StageKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Promotion => f.write_str("promotion"),
            Self::Mutation => f.write_str("mutation"),
        }
    }
}

// ── Stage ────────────────────────────────────────────────────────

/// A single step in a pipeline.
///
/// Uses a boxed trait object for the morphism so pipelines can hold
/// heterogeneous stages in a single `Vec`.
pub struct Stage<Src, Dst> {
    name: String,
    kind: StageKind,
    requires: Vec<Quality>,
    establishes: Vec<Quality>,
    morphism: Box<dyn ProvenMorphism<Src, Dst>>,
}

impl<Src, Dst> Stage<Src, Dst> {
    /// Construct a new stage with a boxed morphism.
    pub fn new<M: ProvenMorphism<Src, Dst> + 'static>(
        name: impl Into<String>,
        kind: StageKind,
        morphism: M,
    ) -> Self {
        Self {
            name: name.into(),
            kind,
            requires: Vec::new(),
            establishes: Vec::new(),
            morphism: Box::new(morphism),
        }
    }

    /// Add a required-input quality (builder).
    #[must_use]
    pub fn requires(mut self, q: Quality) -> Self {
        self.requires.push(q);
        self
    }

    /// Add an established-output quality (builder).
    #[must_use]
    pub fn establishes(mut self, q: Quality) -> Self {
        self.establishes.push(q);
        self
    }

    /// Read-only access.
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn kind(&self) -> StageKind {
        self.kind
    }
    pub fn requires_qualities(&self) -> &[Quality] {
        &self.requires
    }
    pub fn establishes_qualities(&self) -> &[Quality] {
        &self.establishes
    }
}

impl<Src, Dst> Stage<Src, Dst>
where
    Src: ToSExpr,
    Dst: ToSExpr,
{
    /// Run the stage against an input, emitting a [`TraceStep`]
    /// alongside the output (or recorded violations if the morphism
    /// fails its invariant check).
    pub fn run(
        &self,
        input: &Src,
        held_qualities: &[Quality],
    ) -> Result<(Dst, TraceStep), PipelineError> {
        // Check pre-requisite qualities on input.
        for req in &self.requires {
            if !held_qualities.contains(req) {
                return Err(PipelineError::MissingQuality {
                    stage: self.name.clone(),
                    quality: req.clone(),
                });
            }
        }

        let output = self.morphism.apply(input);
        let input_hash = input.content_hash();
        let output_hash = output.content_hash();
        let violations = self.morphism.check_invariants(input, &output);

        let step = TraceStep {
            stage_name: self.name.clone(),
            morphism_name: self.morphism.name().to_string(),
            kind: self.kind,
            input_hash,
            output_hash,
            established: self.establishes.clone(),
            violations: violations.clone(),
        };

        if !violations.is_empty() {
            return Err(PipelineError::InvariantViolations {
                stage: self.name.clone(),
                violations,
            });
        }

        Ok((output, step))
    }
}

// ── Trace ────────────────────────────────────────────────────────

/// Per-stage record produced during pipeline execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceStep {
    pub stage_name: String,
    pub morphism_name: String,
    pub kind: StageKind,
    pub input_hash: ContentHash,
    pub output_hash: ContentHash,
    pub established: Vec<Quality>,
    pub violations: Vec<String>,
}

impl TraceStep {
    pub fn is_clean(&self) -> bool {
        self.violations.is_empty()
    }
}

impl ToSExpr for TraceStep {
    fn to_sexpr(&self) -> SExpr {
        struct_expr(
            "trace-step",
            vec![
                ("stage", self.stage_name.to_sexpr()),
                ("morphism", self.morphism_name.to_sexpr()),
                ("kind", SExpr::Symbol(format!("{}", self.kind))),
                ("input-hash", SExpr::String(self.input_hash.to_hex())),
                ("output-hash", SExpr::String(self.output_hash.to_hex())),
                (
                    "established",
                    SExpr::List({
                        let mut v = vec![SExpr::Symbol("list".into())];
                        v.extend(
                            self.established
                                .iter()
                                .map(|q| SExpr::String(q.0.clone())),
                        );
                        v
                    }),
                ),
                (
                    "violations",
                    SExpr::List({
                        let mut v = vec![SExpr::Symbol("list".into())];
                        v.extend(self.violations.iter().map(|s| SExpr::String(s.clone())));
                        v
                    }),
                ),
            ],
        )
    }
}

impl FromSExpr for TraceStep {
    fn from_sexpr(s: &SExpr) -> Result<Self, SExprError> {
        let f = parse_struct(s, "trace-step")?;
        let kind_sym = take_field(&f, "kind")?.as_symbol()?;
        let kind = match kind_sym {
            "promotion" => StageKind::Promotion,
            "mutation" => StageKind::Mutation,
            other => {
                return Err(SExprError::UnknownVariant(format!("StageKind::{other}")))
            }
        };
        let input_hex = String::from_sexpr(take_field(&f, "input-hash")?)?;
        let output_hex = String::from_sexpr(take_field(&f, "output-hash")?)?;
        Ok(Self {
            stage_name: String::from_sexpr(take_field(&f, "stage")?)?,
            morphism_name: String::from_sexpr(take_field(&f, "morphism")?)?,
            kind,
            input_hash: hex_to_hash(&input_hex)?,
            output_hash: hex_to_hash(&output_hex)?,
            established: Vec::<String>::from_sexpr(take_field(&f, "established")?)?
                .into_iter()
                .map(Quality::new)
                .collect(),
            violations: Vec::<String>::from_sexpr(take_field(&f, "violations")?)?,
        })
    }
}

fn hex_to_hash(hex: &str) -> Result<ContentHash, SExprError> {
    if hex.len() != 64 {
        return Err(SExprError::Shape(format!(
            "content hash must be 64 hex chars, got {}",
            hex.len()
        )));
    }
    let mut bytes = [0u8; 32];
    for (i, pair) in hex.as_bytes().chunks(2).enumerate() {
        let s = std::str::from_utf8(pair)
            .map_err(|_| SExprError::Parse("non-ascii in hash hex".into()))?;
        bytes[i] = u8::from_str_radix(s, 16)
            .map_err(|e| SExprError::Parse(format!("bad hex byte: {e}")))?;
    }
    Ok(ContentHash(bytes))
}

/// Complete execution trace of a pipeline run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Trace {
    pub steps: Vec<TraceStep>,
}

impl Trace {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, step: TraceStep) {
        self.steps.push(step);
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Are all steps clean (no violations)?
    pub fn is_clean(&self) -> bool {
        self.steps.iter().all(TraceStep::is_clean)
    }

    /// All qualities established end-to-end.
    pub fn established(&self) -> Vec<Quality> {
        let mut out: Vec<Quality> = Vec::new();
        for step in &self.steps {
            for q in &step.established {
                if !out.contains(q) {
                    out.push(q.clone());
                }
            }
        }
        out.sort();
        out
    }
}

impl ToSExpr for Trace {
    fn to_sexpr(&self) -> SExpr {
        struct_expr("trace", vec![("steps", self.steps.to_sexpr())])
    }
}

impl FromSExpr for Trace {
    fn from_sexpr(s: &SExpr) -> Result<Self, SExprError> {
        let f = parse_struct(s, "trace")?;
        Ok(Self {
            steps: Vec::<TraceStep>::from_sexpr(take_field(&f, "steps")?)?,
        })
    }
}

// ── Errors ───────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineError {
    MissingQuality { stage: String, quality: Quality },
    InvariantViolations { stage: String, violations: Vec<String> },
}

impl std::fmt::Display for PipelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingQuality { stage, quality } => write!(
                f,
                "stage '{stage}' requires quality '{quality}' which is not established"
            ),
            Self::InvariantViolations { stage, violations } => write!(
                f,
                "stage '{stage}' produced invariant violations: {violations:?}"
            ),
        }
    }
}

impl std::error::Error for PipelineError {}

// ── Convenience: single-type mutation pipeline ───────────────────

/// Run a chain of same-type stages against an initial value.
///
/// All stages share `T` as both input and output (mutation stages).
/// Each stage's `run` produces a new value and a `TraceStep`; the
/// returned `Trace` records the full lineage. On first error, the
/// pipeline halts with a partial trace.
pub fn run_mutation_chain<T>(
    start: T,
    stages: Vec<Stage<T, T>>,
    initial_qualities: &[Quality],
) -> Result<(T, Trace), (PipelineError, Trace)>
where
    T: ToSExpr + Clone,
{
    let mut current = start;
    let mut trace = Trace::new();
    let mut held: Vec<Quality> = initial_qualities.to_vec();

    for stage in stages {
        match stage.run(&current, &held) {
            Ok((output, step)) => {
                for q in &step.established {
                    if !held.contains(q) {
                        held.push(q.clone());
                    }
                }
                trace.push(step);
                current = output;
            }
            Err(e) => return Err((e, trace)),
        }
    }

    Ok((current, trace))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::morphism::{Morphism, ProvenMorphism};

    // ── Concrete test morphisms ────────────────────────────────

    struct AddOne;
    impl Morphism<i64, i64> for AddOne {
        fn name(&self) -> &'static str {
            "AddOne"
        }
        fn apply(&self, src: &i64) -> i64 {
            src + 1
        }
    }
    impl ProvenMorphism<i64, i64> for AddOne {
        fn check_invariants(&self, src: &i64, dst: &i64) -> Vec<String> {
            if *dst == src + 1 {
                Vec::new()
            } else {
                vec!["AddOne: dst != src + 1".into()]
            }
        }
    }

    struct Double;
    impl Morphism<i64, i64> for Double {
        fn name(&self) -> &'static str {
            "Double"
        }
        fn apply(&self, src: &i64) -> i64 {
            src * 2
        }
    }
    impl ProvenMorphism<i64, i64> for Double {
        fn check_invariants(&self, src: &i64, dst: &i64) -> Vec<String> {
            if *dst == src * 2 {
                Vec::new()
            } else {
                vec!["Double: dst != 2 * src".into()]
            }
        }
    }

    // ── Stage ─────────────────────────────────────────────────

    #[test]
    fn stage_records_hashes_on_clean_run() {
        let stage = Stage::new("add-one", StageKind::Mutation, AddOne);
        let (out, step) = stage.run(&5, &[]).unwrap();
        assert_eq!(out, 6);
        assert_eq!(step.input_hash, 5i64.content_hash());
        assert_eq!(step.output_hash, 6i64.content_hash());
        assert!(step.is_clean());
        assert_eq!(step.kind, StageKind::Mutation);
    }

    #[test]
    fn stage_requires_quality_present_in_held() {
        let stage = Stage::new("x", StageKind::Mutation, AddOne)
            .requires(Quality::new("validated"));
        let err = stage.run(&5, &[]).unwrap_err();
        assert!(matches!(err, PipelineError::MissingQuality { .. }));

        let ok = stage.run(&5, &[Quality::new("validated")]);
        assert!(ok.is_ok());
    }

    #[test]
    fn stage_establishes_quality_recorded_in_step() {
        let stage = Stage::new("x", StageKind::Mutation, AddOne)
            .establishes(Quality::new("normalized"));
        let (_, step) = stage.run(&5, &[]).unwrap();
        assert_eq!(step.established, vec![Quality::new("normalized")]);
    }

    // ── run_mutation_chain ────────────────────────────────────

    #[test]
    fn mutation_chain_runs_in_order() {
        let stages = vec![
            Stage::new("s1", StageKind::Mutation, AddOne),
            Stage::new("s2", StageKind::Mutation, Double),
        ];
        let (out, trace) = run_mutation_chain(5_i64, stages, &[]).unwrap();
        assert_eq!(out, 12); // (5+1)*2
        assert_eq!(trace.len(), 2);
        assert!(trace.is_clean());
    }

    #[test]
    fn mutation_chain_propagates_established_qualities() {
        let stages = vec![
            Stage::new("promote", StageKind::Mutation, AddOne)
                .establishes(Quality::new("normalized")),
            Stage::new("then-requires", StageKind::Mutation, Double)
                .requires(Quality::new("normalized")),
        ];
        let result = run_mutation_chain(1_i64, stages, &[]);
        assert!(result.is_ok(), "qualities should propagate: {result:?}");
    }

    #[test]
    fn mutation_chain_halts_on_missing_quality() {
        let stages = vec![
            Stage::new("needs-validated", StageKind::Mutation, AddOne)
                .requires(Quality::new("validated")),
        ];
        let (err, partial) = run_mutation_chain(1_i64, stages, &[]).unwrap_err();
        assert!(matches!(err, PipelineError::MissingQuality { .. }));
        assert!(partial.is_empty(), "no steps should have run");
    }

    // ── Invariant violation ───────────────────────────────────

    struct BadDouble;
    impl Morphism<i64, i64> for BadDouble {
        fn name(&self) -> &'static str {
            "BadDouble"
        }
        fn apply(&self, src: &i64) -> i64 {
            src * 3 // wrong
        }
    }
    impl ProvenMorphism<i64, i64> for BadDouble {
        fn check_invariants(&self, src: &i64, dst: &i64) -> Vec<String> {
            if *dst == src * 2 {
                Vec::new()
            } else {
                vec!["Double: dst != 2 * src".into()]
            }
        }
    }

    #[test]
    fn mutation_chain_halts_on_invariant_violation() {
        let stages = vec![
            Stage::new("s1", StageKind::Mutation, AddOne),
            Stage::new("bad", StageKind::Mutation, BadDouble),
            Stage::new("s3", StageKind::Mutation, AddOne),
        ];
        let (err, partial) = run_mutation_chain(5_i64, stages, &[]).unwrap_err();
        assert!(matches!(err, PipelineError::InvariantViolations { .. }));
        // Only s1 completed before bad tripped.
        assert_eq!(partial.len(), 1);
        assert_eq!(partial.steps[0].stage_name, "s1");
    }

    // ── Trace sexpr round-trip ────────────────────────────────

    #[test]
    fn trace_step_round_trips_via_sexpr() {
        let stage = Stage::new("x", StageKind::Mutation, AddOne)
            .establishes(Quality::new("q1"));
        let (_, step) = stage.run(&5, &[]).unwrap();
        let round = TraceStep::from_sexpr(&step.to_sexpr()).expect("parse");
        assert_eq!(round, step);
    }

    #[test]
    fn trace_round_trips_via_sexpr() {
        let stages = vec![
            Stage::new("a", StageKind::Mutation, AddOne),
            Stage::new("b", StageKind::Mutation, Double),
        ];
        let (_, trace) = run_mutation_chain(1_i64, stages, &[]).unwrap();
        let round = Trace::from_sexpr(&trace.to_sexpr()).expect("parse");
        assert_eq!(round, trace);
    }

    // ── Content addressing ───────────────────────────────────

    #[test]
    fn trace_content_hash_is_deterministic() {
        let make = || {
            let stages = vec![
                Stage::new("a", StageKind::Mutation, AddOne),
                Stage::new("b", StageKind::Mutation, Double),
            ];
            run_mutation_chain(1_i64, stages, &[]).unwrap().1
        };
        assert_eq!(make().content_hash(), make().content_hash());
    }

    #[test]
    fn trace_hash_differs_for_different_inputs() {
        let stages1 = vec![Stage::new("a", StageKind::Mutation, AddOne)];
        let stages2 = vec![Stage::new("a", StageKind::Mutation, AddOne)];
        let t1 = run_mutation_chain(1_i64, stages1, &[]).unwrap().1;
        let t2 = run_mutation_chain(99_i64, stages2, &[]).unwrap().1;
        assert_ne!(t1.content_hash(), t2.content_hash());
    }

    // ── Established qualities aggregation ────────────────────

    #[test]
    fn trace_aggregates_all_established_qualities() {
        let stages = vec![
            Stage::new("a", StageKind::Mutation, AddOne)
                .establishes(Quality::new("q1")),
            Stage::new("b", StageKind::Mutation, Double)
                .establishes(Quality::new("q2"))
                .establishes(Quality::new("q1")), // duplicate
        ];
        let (_, trace) = run_mutation_chain(1_i64, stages, &[]).unwrap();
        let qs = trace.established();
        assert_eq!(qs.len(), 2);
        assert!(qs.contains(&Quality::new("q1")));
        assert!(qs.contains(&Quality::new("q2")));
    }

    // ── Stage-kind surfacing in trace ────────────────────────

    #[test]
    fn trace_step_preserves_stage_kind() {
        let m = Stage::new("m", StageKind::Mutation, AddOne);
        let p = Stage::new("p", StageKind::Promotion, AddOne);
        let (_, step_m) = m.run(&1, &[]).unwrap();
        let (_, step_p) = p.run(&1, &[]).unwrap();
        assert_eq!(step_m.kind, StageKind::Mutation);
        assert_eq!(step_p.kind, StageKind::Promotion);
    }
}
