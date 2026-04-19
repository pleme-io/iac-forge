//! Property-based proofs of remediation-harness laws.
//!
//! Given an arbitrary resource and an arbitrary valid script, the
//! Outcome must satisfy:
//!
//!   L1. changed() ⟺ !edits.is_empty()
//!   L2. changed() ⟺ before_hash ≠ after_hash
//!   L3. before_hash == before.content_hash().to_hex()
//!   L4. after_hash == after.content_hash().to_hex()
//!   L5. outcome_sexpr(o) round-trips through parse
//!   L6. outcome_sexpr is deterministic
//!   L7. op_count in the audit header matches o.ops.len()
//!   L8. applying an idempotent op twice produces the same post-state
//!       hash as applying once
//!   L9. apply_proposal is deterministic (same IR + script → same Outcome)
//!
//! These hold for every valid input, proven by proptest over 256 cases
//! per property.

use proptest::prelude::*;

use iac_forge::ir::{IacAttribute, IacResource, IacType};
use iac_forge::remediation::{Proposal, apply_proposal, outcome_sexpr};
use iac_forge::sexpr::{SExpr, ToSExpr};
use iac_forge::testing::{TestAttributeBuilder, test_resource};

// ── Strategies ──────────────────────────────────────────────────────

/// Generate a resource with 1-5 attributes, each with a simple canonical
/// name. We keep the shape predictable so scripts can reliably reference
/// the attribute names.
fn arb_resource() -> impl Strategy<Value = IacResource> {
    prop::collection::vec(arb_attr_name(), 1..5).prop_map(|names| {
        let mut r = test_resource("widget");
        r.attributes = names
            .into_iter()
            .enumerate()
            .map(|(i, name)| {
                let mut b = TestAttributeBuilder::new(&name, IacType::String);
                if i == 0 {
                    b = b.required();
                }
                b.build()
            })
            .collect();
        r
    })
}

fn arb_attr_name() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{1,8}".prop_map(String::from)
}

fn arb_description() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9 .,_-]{0,24}".prop_map(String::from)
}

/// Build a valid script string from a small op vocabulary targeting one
/// of the resource's attribute names. The `attr_names` slice ensures
/// we only reference names that exist so `MarkSensitive` isn't a no-op
/// by accident.
fn arb_script_for(attr_names: Vec<String>) -> impl Strategy<Value = (String, usize)> {
    // Pick 1-3 ops.
    (
        1..4_usize,
        arb_description(),
        arb_description(),
        prop::sample::select(attr_names.clone()),
        prop::sample::select(attr_names),
    )
        .prop_map(|(n, desc, cat, sens_name, rem_name)| {
            let mut script = String::new();
            let ops = [
                format!(r#"(set-description "{desc}")"#),
                format!(r#"(set-category "{cat}")"#),
                format!(r#"(mark-sensitive "{sens_name}")"#),
                format!(r#"(remove-attribute "{rem_name}")"#),
            ];
            let count = n.min(ops.len());
            for op in ops.iter().take(count) {
                script.push_str(op);
                script.push('\n');
            }
            (script, count)
        })
}

// ── Properties ──────────────────────────────────────────────────────

proptest! {
    /// L3 + L4 + L2 + L1 + L7: every Outcome's audit-trail fields are
    /// internally consistent.
    #[test]
    fn outcome_fields_agree((resource, seed) in (arb_resource(), 0_u32..100_000)) {
        let _ = seed; // seed is just to diversify strategies
        let attr_names: Vec<String> = resource.attributes.iter()
            .map(|a| a.canonical_name.clone())
            .collect();

        let (script, expected_op_count) = proptest::strategy::ValueTree::current(
            &mut arb_script_for(attr_names).new_tree(&mut proptest::test_runner::TestRunner::deterministic()).unwrap(),
        );
        let proposal = Proposal::new("test", script);
        let o = match apply_proposal(&resource, &proposal) {
            Ok(x) => x,
            Err(e) => {
                prop_assert!(false, "valid script should apply cleanly: {e:?}");
                return Ok(());
            }
        };

        // L3: before_hash matches before.content_hash()
        prop_assert_eq!(&o.before_hash, &o.before.content_hash().to_hex());
        // L4: after_hash matches after.content_hash()
        prop_assert_eq!(&o.after_hash, &o.after.content_hash().to_hex());
        // L7: op_count matches
        prop_assert_eq!(o.ops.len(), expected_op_count);
        // L2: changed iff hashes differ
        prop_assert_eq!(o.changed(), o.before_hash != o.after_hash);
        // L1: changed iff edits are non-empty
        prop_assert_eq!(o.changed(), !o.edits.is_empty());
    }
}

// The strategy interaction above is awkward. Simpler: thread the
// script generation through a single strategy that also returns the
// resource + expected-op-count. That sidesteps the manual ValueTree
// plumbing.

fn arb_case() -> impl Strategy<Value = (IacResource, String, usize)> {
    arb_resource().prop_flat_map(|r| {
        let names: Vec<String> = r
            .attributes
            .iter()
            .map(|a| a.canonical_name.clone())
            .collect();
        (Just(r), arb_script_for(names)).prop_map(|(r, (s, n))| (r, s, n))
    })
}

proptest! {
    /// L3 + L4 (hash agreement) alone — cleanest version using arb_case.
    #[test]
    fn hashes_agree_with_content_hash(case in arb_case()) {
        let (resource, script, _) = case;
        let p = Proposal::new("x", script);
        let Ok(o) = apply_proposal(&resource, &p) else {
            prop_assert!(false, "expected clean apply");
            return Ok(());
        };
        prop_assert_eq!(&o.before_hash, &o.before.content_hash().to_hex());
        prop_assert_eq!(&o.after_hash, &o.after.content_hash().to_hex());
    }

    /// L1 + L2: changed() agrees across both representations.
    #[test]
    fn changed_iff_edits_nonempty_and_hashes_differ(case in arb_case()) {
        let (resource, script, _) = case;
        let p = Proposal::new("x", script);
        let Ok(o) = apply_proposal(&resource, &p) else {
            prop_assert!(false);
            return Ok(());
        };
        let hashes_differ = o.before_hash != o.after_hash;
        let edits_nonempty = !o.edits.is_empty();
        prop_assert_eq!(hashes_differ, edits_nonempty);
        prop_assert_eq!(o.changed(), hashes_differ);
    }

    /// L7: parsed-op count matches the script's op count.
    #[test]
    fn op_count_matches_parsed(case in arb_case()) {
        let (resource, script, expected) = case;
        let p = Proposal::new("x", script);
        let Ok(o) = apply_proposal(&resource, &p) else {
            prop_assert!(false);
            return Ok(());
        };
        prop_assert_eq!(o.ops.len(), expected);
    }

    /// L6: outcome_sexpr is deterministic.
    #[test]
    fn outcome_sexpr_is_deterministic(case in arb_case()) {
        let (resource, script, _) = case;
        let p = Proposal::new("x", script);
        let Ok(o) = apply_proposal(&resource, &p) else {
            prop_assert!(false);
            return Ok(());
        };
        prop_assert_eq!(outcome_sexpr(&o).emit(), outcome_sexpr(&o).emit());
    }

    /// L5: outcome_sexpr round-trips through SExpr::parse.
    #[test]
    fn outcome_sexpr_round_trips(case in arb_case()) {
        let (resource, script, _) = case;
        let p = Proposal::new("reason", script);
        let Ok(o) = apply_proposal(&resource, &p) else {
            prop_assert!(false);
            return Ok(());
        };
        let emitted = outcome_sexpr(&o).emit();
        let reparsed = SExpr::parse(&emitted)
            .unwrap_or_else(|e| panic!("parse failed: {e:?}"));
        prop_assert_eq!(outcome_sexpr(&o), reparsed);
    }

    /// L9: apply_proposal is deterministic — same IR + script ⇒ same Outcome
    /// (compared via canonical sexpr emission).
    #[test]
    fn apply_is_deterministic(case in arb_case()) {
        let (resource, script, _) = case;
        let p = Proposal::new("x", script);
        let Ok(a) = apply_proposal(&resource, &p) else {
            prop_assert!(false);
            return Ok(());
        };
        let Ok(b) = apply_proposal(&resource, &p) else {
            prop_assert!(false);
            return Ok(());
        };
        prop_assert_eq!(&a.before_hash, &b.before_hash);
        prop_assert_eq!(&a.after_hash, &b.after_hash);
        prop_assert_eq!(&a.edits, &b.edits);
        prop_assert_eq!(outcome_sexpr(&a), outcome_sexpr(&b));
    }
}

// ── L8: idempotence ────────────────────────────────────────────────

/// Build an idempotent-only script (set-description, set-category,
/// mark-sensitive, add-optional-string — each is idempotent by
/// construction; remove-attribute is also idempotent once the target
/// is gone).
fn arb_idempotent_script(attr_names: Vec<String>) -> impl Strategy<Value = String> {
    (arb_description(), prop::sample::select(attr_names)).prop_map(|(desc, target)| {
        format!(
            r#"(set-description "{desc}")
                   (mark-sensitive "{target}")"#,
        )
    })
}

fn arb_idempotent_case() -> impl Strategy<Value = (IacResource, String)> {
    arb_resource().prop_flat_map(|r| {
        let names: Vec<String> = r
            .attributes
            .iter()
            .map(|a| a.canonical_name.clone())
            .collect();
        (Just(r), arb_idempotent_script(names))
    })
}

proptest! {
    /// L8: applying an idempotent script twice yields the same post-state
    /// hash as applying once.
    #[test]
    fn idempotent_ops_are_idempotent((resource, script) in arb_idempotent_case()) {
        let p = Proposal::new("x", script);
        let Ok(first) = apply_proposal(&resource, &p) else {
            prop_assert!(false);
            return Ok(());
        };
        // Apply again against the first outcome's after-state.
        let Ok(second) = apply_proposal(&first.after, &p) else {
            prop_assert!(false);
            return Ok(());
        };
        prop_assert_eq!(&first.after_hash, &second.after_hash);
        prop_assert!(!second.changed(), "second apply should be a no-op");
    }
}

// ── A few deterministic negative properties as sanity anchors ──────

#[test]
fn cross_check_no_script_no_changes() {
    // An empty script (no ops) is legal — ResourceOpSeq is Transform<_>.
    // Applying it must leave the resource unchanged.
    let mut r = test_resource("widget");
    r.attributes = vec![
        TestAttributeBuilder::new("x", IacType::String)
            .required()
            .build(),
    ];
    let p = Proposal::new("noop", "");
    // Empty input — parse returns Vec::new(), apply is a no-op.
    let o = apply_proposal(&r, &p).expect("empty script parses");
    assert!(!o.changed());
    assert!(o.edits.is_empty());
    assert_eq!(o.before_hash, o.after_hash);
    assert_eq!(o.ops.len(), 0);
}

#[test]
fn _attribute_is_used() {
    // Silence unused-import warning for IacAttribute import
    let _ = std::mem::size_of::<IacAttribute>();
}
