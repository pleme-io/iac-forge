//! Remediation harness — bounded, auditable transform application.
//!
//! A remediation takes a proposal (a transform script, typically from
//! a human reviewer or an LLM), applies it to a source IR, and returns
//! a full [`Outcome`]: the post-state IR, the edit list that describes
//! what changed, the before/after content hashes, and any invariant
//! violations the transform introduced.
//!
//! The *whole point* of the harness is that the LLM-or-human author
//! of the proposal doesn't have to be trusted — the script is bounded
//! by the existing [`ResourceOp`] vocabulary, parse failures are
//! rejected before anything runs, and the Outcome carries a complete
//! audit trail so tameshi / sekiban / kensa can gate on it.
//!
//! # Flow
//!
//! ```
//! # use iac_forge::remediation::{Proposal, apply_proposal};
//! # use iac_forge::testing::test_resource;
//! let resource = test_resource("widget");
//! let proposal = Proposal::new(
//!     "flag database-url sensitive",
//!     r#"(mark-sensitive "database_url")"#,
//! );
//! let outcome = apply_proposal(&resource, &proposal).expect("apply");
//! assert_eq!(outcome.proposal_reason, "flag database-url sensitive");
//! assert_ne!(outcome.before_hash, outcome.after_hash); // something changed
//! # // (or stayed the same if no attribute was named "database_url")
//! ```

use crate::ir::IacResource;
use crate::sexpr::{SExpr, SExprError, ToSExpr};
use crate::sexpr_diff::{Edit, diff};
use crate::transform::ops::ResourceOp;
use crate::transform::{Transform, script};

/// A remediation proposal — script text plus the human-readable reason
/// a caller wants to apply it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    /// Free-form description of why this remediation exists (e.g.,
    /// "PCI-DSS §3.4 — flag cardholder data fields sensitive").
    pub reason: String,
    /// s-expression script in the [`crate::transform::script`] surface.
    pub script: String,
}

impl Proposal {
    /// Construct a proposal.
    pub fn new(reason: impl Into<String>, script: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            script: script.into(),
        }
    }
}

/// Errors that can prevent a remediation from applying.
#[derive(Debug)]
pub enum RemediationError {
    /// The script couldn't be parsed (malformed, unknown op, bad arity).
    /// The underlying string is the same one [`script::parse`] returned.
    ScriptParse(String),
    /// A post-state invariant check failed. Carries the human-readable
    /// violations so callers can report or gate.
    InvariantViolations(Vec<String>),
    /// Sexpr emission or parsing hiccup during provenance hashing.
    /// Shouldn't occur for valid IR values but we surface it rather
    /// than panicking.
    Sexpr(SExprError),
}

impl std::fmt::Display for RemediationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ScriptParse(s) => write!(f, "remediation script parse: {s}"),
            Self::InvariantViolations(v) => {
                write!(f, "remediation invariant violations: {v:?}")
            }
            Self::Sexpr(e) => write!(f, "remediation sexpr: {e}"),
        }
    }
}

impl std::error::Error for RemediationError {}

impl From<SExprError> for RemediationError {
    fn from(e: SExprError) -> Self {
        Self::Sexpr(e)
    }
}

/// The full outcome of applying a proposal: transformed IR, edit list,
/// before/after hashes, parsed ops, and the proposal reason.
///
/// Every field is intended to be attested — tameshi can Merkle-hash
/// the Outcome's sexpr form and sekiban can gate admission on a
/// signature over it.
#[derive(Debug, Clone)]
pub struct Outcome {
    /// The original resource, unchanged.
    pub before: IacResource,
    /// The transformed resource.
    pub after: IacResource,
    /// Content hash of `before` (hex).
    pub before_hash: String,
    /// Content hash of `after` (hex).
    pub after_hash: String,
    /// Parsed ops from the script.
    pub ops: Vec<ResourceOp>,
    /// Structural edit list from `before` to `after` (derived from
    /// sexpr_diff).
    pub edits: Vec<Edit>,
    /// The proposal reason, preserved for audit.
    pub proposal_reason: String,
}

impl Outcome {
    /// Whether the remediation actually changed anything.
    #[must_use]
    pub fn changed(&self) -> bool {
        self.before_hash != self.after_hash
    }

    /// Count of structural edits produced.
    #[must_use]
    pub fn edit_count(&self) -> usize {
        self.edits.len()
    }
}

/// Post-state invariant — anything a caller wants to assert about the
/// transformed IR. Returns violations (empty = all hold).
pub type Invariant = fn(&IacResource) -> Vec<String>;

/// Apply a proposal and return its Outcome. Runs no invariant checks
/// beyond parse-time.
///
/// # Errors
/// `ScriptParse` if the script fails to parse (malformed, unknown op,
/// arity mismatch).
pub fn apply_proposal(
    resource: &IacResource,
    proposal: &Proposal,
) -> Result<Outcome, RemediationError> {
    apply_proposal_with_invariants(resource, proposal, &[])
}

/// Apply a proposal and check a list of post-state invariants.
///
/// # Errors
/// - `ScriptParse` if the script fails to parse.
/// - `InvariantViolations` if any invariant returns non-empty violations.
pub fn apply_proposal_with_invariants(
    resource: &IacResource,
    proposal: &Proposal,
    invariants: &[Invariant],
) -> Result<Outcome, RemediationError> {
    let ops = script::parse(&proposal.script).map_err(RemediationError::ScriptParse)?;

    let before = resource.clone();
    let before_sexpr = before.to_sexpr();
    let before_hash = before_sexpr.content_hash().to_hex();

    let after = ops.apply(before.clone());
    let after_sexpr = after.to_sexpr();
    let after_hash = after_sexpr.content_hash().to_hex();

    // Compute edit list (structural diff between before/after sexprs).
    let edits = diff(&before_sexpr, &after_sexpr);

    // Run invariants — any violation blocks the outcome.
    let mut all_violations = Vec::new();
    for inv in invariants {
        all_violations.extend(inv(&after));
    }
    if !all_violations.is_empty() {
        return Err(RemediationError::InvariantViolations(all_violations));
    }

    Ok(Outcome {
        before,
        after,
        before_hash,
        after_hash,
        ops,
        edits,
        proposal_reason: proposal.reason.clone(),
    })
}

/// Emit an Outcome as a canonical sexpr for attestation.
///
/// Shape: `(remediation-outcome (:reason "…") (:before-hash "…")
/// (:after-hash "…") (:edit-count N) (:op-count N))`.
///
/// Does not include the full before/after IR — callers that want that
/// serialize `.before` and `.after` via their own `to_sexpr` calls. The
/// canonical form here is the *audit header*: enough to identify which
/// remediation happened to which IR without embedding the whole IR.
#[must_use]
pub fn outcome_sexpr(o: &Outcome) -> SExpr {
    use crate::sexpr::struct_expr;
    struct_expr(
        "remediation-outcome",
        vec![
            ("reason", SExpr::String(o.proposal_reason.clone())),
            ("before-hash", SExpr::String(o.before_hash.clone())),
            ("after-hash", SExpr::String(o.after_hash.clone())),
            ("edit-count", SExpr::Integer(o.edit_count() as i64)),
            (
                "op-count",
                SExpr::Integer(i64::try_from(o.ops.len()).unwrap_or(i64::MAX)),
            ),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::IacType;
    use crate::testing::{TestAttributeBuilder, test_resource};

    fn sample() -> IacResource {
        let mut r = test_resource("widget");
        r.attributes = vec![
            TestAttributeBuilder::new("name", IacType::String)
                .required()
                .build(),
            TestAttributeBuilder::new("database_url", IacType::String).build(),
        ];
        r
    }

    #[test]
    fn apply_proposal_with_valid_script() {
        let r = sample();
        let p = Proposal::new(
            "mark db url sensitive",
            r#"(mark-sensitive "database_url")"#,
        );
        let o = apply_proposal(&r, &p).unwrap();
        assert!(o.changed());
        assert_ne!(o.before_hash, o.after_hash);
        assert_eq!(o.ops.len(), 1);
        assert!(!o.edits.is_empty());
        assert_eq!(o.proposal_reason, "mark db url sensitive");
    }

    #[test]
    fn apply_proposal_no_op_produces_unchanged_outcome() {
        let r = sample();
        let p = Proposal::new(
            "mark nonexistent sensitive",
            r#"(mark-sensitive "does_not_exist")"#,
        );
        let o = apply_proposal(&r, &p).unwrap();
        // The op runs but matches no attribute → no real change.
        assert!(!o.changed());
        assert_eq!(o.before_hash, o.after_hash);
        assert!(o.edits.is_empty());
    }

    #[test]
    fn apply_proposal_rejects_malformed_script() {
        let r = sample();
        let p = Proposal::new("nope", "(mark-sensitive");
        let err = apply_proposal(&r, &p).unwrap_err();
        assert!(matches!(err, RemediationError::ScriptParse(_)));
    }

    #[test]
    fn apply_proposal_rejects_unknown_op() {
        let r = sample();
        let p = Proposal::new("nope", r#"(nuke-everything "x")"#);
        let err = apply_proposal(&r, &p).unwrap_err();
        assert!(matches!(err, RemediationError::ScriptParse(s) if s.contains("unknown op")));
    }

    #[test]
    fn apply_proposal_with_multiple_ops() {
        let r = sample();
        let p = Proposal::new(
            "set description + sensitive",
            r#"(set-description "v2") (mark-sensitive "database_url")"#,
        );
        let o = apply_proposal(&r, &p).unwrap();
        assert_eq!(o.ops.len(), 2);
        assert!(o.changed());
    }

    #[test]
    fn content_hashes_are_stable_and_correct() {
        use crate::sexpr::ToSExpr;
        let r = sample();
        let p = Proposal::new("noop", r#"(mark-sensitive "missing")"#);
        let o = apply_proposal(&r, &p).unwrap();
        assert_eq!(o.before_hash, r.content_hash().to_hex());
        assert_eq!(o.after_hash, o.after.content_hash().to_hex());
    }

    #[test]
    fn edit_list_reflects_sensitive_flip() {
        let r = sample();
        let p = Proposal::new("s", r#"(mark-sensitive "database_url")"#);
        let o = apply_proposal(&r, &p).unwrap();
        // The sensitive bit changed on one attribute; diff should
        // show at least one edit whose path ends in "sensitive".
        assert!(
            o.edits.iter().any(|e| e.path().ends_with("sensitive")),
            "expected a 'sensitive' edit, got {:?}",
            o.edits,
        );
    }

    // ── Invariant checking ─────────────────────────────────────

    fn no_unsensitive_dburl(r: &IacResource) -> Vec<String> {
        let mut v = Vec::new();
        for a in &r.attributes {
            if a.canonical_name == "database_url" && !a.sensitive {
                v.push("database_url must be sensitive".to_string());
            }
        }
        v
    }

    #[test]
    fn invariants_pass_when_remediation_fixes_them() {
        let r = sample();
        let p = Proposal::new("fix dburl", r#"(mark-sensitive "database_url")"#);
        let o = apply_proposal_with_invariants(&r, &p, &[no_unsensitive_dburl]).unwrap();
        assert!(o.changed());
    }

    #[test]
    fn invariants_block_outcome_when_not_met() {
        let r = sample();
        // A proposal that does nothing (wrong field name) leaves
        // database_url unflagged — the invariant should fire.
        let p = Proposal::new("wrong", r#"(mark-sensitive "other")"#);
        let err = apply_proposal_with_invariants(&r, &p, &[no_unsensitive_dburl]).unwrap_err();
        match err {
            RemediationError::InvariantViolations(v) => {
                assert_eq!(v.len(), 1);
                assert!(v[0].contains("database_url"));
            }
            other => panic!("expected InvariantViolations, got {other:?}"),
        }
    }

    // ── Outcome sexpr (audit header) ───────────────────────────

    #[test]
    fn outcome_sexpr_has_audit_fields() {
        let r = sample();
        let p = Proposal::new("reason", r#"(mark-sensitive "database_url")"#);
        let o = apply_proposal(&r, &p).unwrap();
        let s = outcome_sexpr(&o).emit();
        assert!(s.starts_with("(remediation-outcome"));
        assert!(s.contains("(:reason \"reason\")"));
        assert!(s.contains("(:before-hash"));
        assert!(s.contains("(:after-hash"));
        assert!(s.contains("(:edit-count"));
        assert!(s.contains("(:op-count"));
    }

    #[test]
    fn outcome_sexpr_is_deterministic() {
        let r = sample();
        let p = Proposal::new("x", r#"(mark-sensitive "database_url")"#);
        let o = apply_proposal(&r, &p).unwrap();
        assert_eq!(outcome_sexpr(&o).emit(), outcome_sexpr(&o).emit());
    }

    // ── Error display ──────────────────────────────────────────

    #[test]
    fn error_displays_script_parse() {
        let err = RemediationError::ScriptParse("bad".into());
        assert!(err.to_string().contains("script parse"));
    }

    #[test]
    fn error_displays_invariant_violations() {
        let err = RemediationError::InvariantViolations(vec!["x".into()]);
        assert!(err.to_string().contains("invariant violations"));
    }

    // ── Determinism ────────────────────────────────────────────

    #[test]
    fn apply_proposal_is_deterministic() {
        let r = sample();
        let p = Proposal::new(
            "x",
            r#"(set-description "new") (mark-sensitive "database_url")"#,
        );
        let o1 = apply_proposal(&r, &p).unwrap();
        let o2 = apply_proposal(&r, &p).unwrap();
        assert_eq!(o1.before_hash, o2.before_hash);
        assert_eq!(o1.after_hash, o2.after_hash);
        assert_eq!(o1.edits, o2.edits);
    }
}
