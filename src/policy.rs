//! Policy-as-code over sexpr patterns.
//!
//! Compliance controls (NIST SC-7, CIS 5.x, FedRAMP, PCI-DSS §3.4, …)
//! have historically been hand-coded Rust invariants scattered across
//! repos. That makes them hard to review, hard to diff, and hard to
//! audit. This module moves them into *data*: each control is a
//! `Policy` composed of a [`Pattern`] that declares what to match and
//! a [`Rule`] that declares what must hold where it matches.
//!
//! A minimal policy engine walks an IR's sexpr form, finds every
//! subtree matching the pattern, and asserts the rule on each. The
//! result is a `PolicyReport` — structured pass/fail findings with
//! paths into the IR, ready for attestation.
//!
//! # Pattern language
//!
//! - `Pattern::Any` — matches anything
//! - `Pattern::Symbol(s)` — matches only that symbol
//! - `Pattern::String(s)` — matches only that string literal
//! - `Pattern::Integer(i)` / `Bool(b)` / `Nil` — scalar equality
//! - `Pattern::AnyString` / `AnySymbol` / `AnyInteger` — value classes
//! - `Pattern::OneOf(vec)` — any of the literal string/symbol values
//! - `Pattern::Struct { head, fields }` — matches a struct-form with
//!   the given head symbol and the given `(field, pattern)` pairs.
//!   Fields not named in the pattern are ignored (partial match).
//! - `Pattern::ListHead { head, tail }` — matches a `(head …rest)` list
//!   form where `tail` is applied element-wise (with `Pattern::Any`
//!   tolerated as wildcard).
//!
//! # Rules
//!
//! - `Rule::Deny(reason)` — any match is a violation
//! - `Rule::RequireField { field, pattern }` — the matched subtree must
//!   contain a struct-form field whose value matches `pattern`
//! - `Rule::ForbidField { field }` — the matched subtree must NOT
//!   contain this field (or, if present, the field value must be
//!   `Nil`)
//!
//! # Example
//!
//! ```
//! use iac_forge::policy::{Pattern, Policy, Rule, evaluate};
//! use iac_forge::sexpr::ToSExpr;
//! use iac_forge::testing::{test_resource, TestAttributeBuilder};
//! use iac_forge::ir::IacType;
//!
//! // Control: "every sensitive attribute must also be marked immutable"
//! let policy = Policy {
//!     id: "custom-1".into(),
//!     description: "sensitive fields must be immutable".into(),
//!     pattern: Pattern::Struct {
//!         head: "attribute".into(),
//!         fields: vec![("sensitive".into(), Pattern::Bool(true))],
//!     },
//!     rule: Rule::RequireField {
//!         field: "immutable".into(),
//!         pattern: Pattern::Bool(true),
//!     },
//! };
//!
//! let mut r = test_resource("x");
//! r.attributes = vec![
//!     TestAttributeBuilder::new("password", IacType::String)
//!         .required().sensitive().build(),
//! ];
//! let report = evaluate(&[policy], &r.to_sexpr());
//! assert!(report.has_violations());
//! ```

use crate::sexpr::SExpr;

// ── Patterns ────────────────────────────────────────────────────────

/// A declarative match-template over SExpr trees.
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    /// Matches any subtree.
    Any,
    /// Matches the specific symbol.
    Symbol(String),
    /// Matches the specific string literal.
    String(String),
    /// Matches the specific integer.
    Integer(i64),
    /// Matches the specific bool.
    Bool(bool),
    /// Matches `nil`.
    Nil,
    /// Matches any symbol.
    AnySymbol,
    /// Matches any string literal.
    AnyString,
    /// Matches any integer.
    AnyInteger,
    /// Matches any one of the given string literals OR symbols.
    OneOf(Vec<String>),
    /// Matches a struct-form `(head (:field val) …)` where `head` must
    /// equal `head` and every declared field must match (extra fields
    /// on the target are ignored — partial match).
    Struct {
        head: String,
        fields: Vec<(String, Pattern)>,
    },
    /// Matches a list-form `(head tail0 tail1 …)` where `head` equals
    /// the expected head symbol and each tail pattern matches positionally.
    ListHead { head: String, tail: Vec<Pattern> },
}

impl Pattern {
    /// True iff this pattern matches the target.
    #[must_use]
    pub fn matches(&self, target: &SExpr) -> bool {
        match (self, target) {
            (Self::Any, _) => true,
            (Self::Symbol(s), SExpr::Symbol(t)) => s == t,
            (Self::String(s), SExpr::String(t)) => s == t,
            (Self::Integer(i), SExpr::Integer(t)) => i == t,
            (Self::Bool(b), SExpr::Bool(t)) => b == t,
            (Self::Nil, SExpr::Nil) => true,
            (Self::AnySymbol, SExpr::Symbol(_)) => true,
            (Self::AnyString, SExpr::String(_)) => true,
            (Self::AnyInteger, SExpr::Integer(_)) => true,
            (Self::OneOf(opts), SExpr::Symbol(s) | SExpr::String(s)) => {
                opts.iter().any(|o| o == s)
            }
            (Self::Struct { head, fields }, SExpr::List(items)) => {
                let Some((target_head, tail)) = items.split_first() else {
                    return false;
                };
                let Ok(name) = head_symbol(target_head) else {
                    return false;
                };
                if name != head {
                    return false;
                }
                let target_fields = parse_keyword_fields(tail);
                fields
                    .iter()
                    .all(|(k, p)| match target_fields.get(k.as_str()) {
                        Some(v) => p.matches(v),
                        None => matches!(p, Pattern::Nil),
                    })
            }
            (Self::ListHead { head, tail }, SExpr::List(items)) => {
                let Some((target_head, rest)) = items.split_first() else {
                    return false;
                };
                let Ok(name) = head_symbol(target_head) else {
                    return false;
                };
                if name != head {
                    return false;
                }
                if rest.len() != tail.len() {
                    return false;
                }
                tail.iter()
                    .zip(rest)
                    .all(|(pat, target)| pat.matches(target))
            }
            _ => false,
        }
    }
}

fn head_symbol(s: &SExpr) -> Result<&str, ()> {
    match s {
        SExpr::Symbol(s) => Ok(s),
        _ => Err(()),
    }
}

fn parse_keyword_fields(items: &[SExpr]) -> std::collections::BTreeMap<&str, &SExpr> {
    let mut out = std::collections::BTreeMap::new();
    for item in items {
        let SExpr::List(pair) = item else { continue };
        if pair.len() != 2 {
            continue;
        }
        let SExpr::Symbol(key) = &pair[0] else { continue };
        if let Some(rest) = key.strip_prefix(':') {
            out.insert(rest, &pair[1]);
        }
    }
    out
}

// ── Rules ───────────────────────────────────────────────────────────

/// What must hold about a subtree once the pattern matches.
#[derive(Debug, Clone, PartialEq)]
pub enum Rule {
    /// Any match is a violation. The string is the reason/diagnostic.
    Deny(String),
    /// The matched subtree must be a struct-form containing `field`
    /// with a value matching `pattern`.
    RequireField { field: String, pattern: Pattern },
    /// The matched subtree must NOT contain `field`, or if it does, the
    /// value must be `Nil`.
    ForbidField { field: String },
}

// ── Policy + evaluation ────────────────────────────────────────────

/// A single policy: pattern + rule + metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct Policy {
    /// Short machine identifier (e.g., "pci-3.4-sensitive").
    pub id: String,
    /// Human-readable description.
    pub description: String,
    /// What subtrees this policy applies to.
    pub pattern: Pattern,
    /// What must hold where the pattern matches.
    pub rule: Rule,
}

/// A single policy finding — pass OR fail, with its path and reason.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub policy_id: String,
    pub path: String,
    /// `true` = policy was satisfied at this site. `false` = violated.
    pub satisfied: bool,
    pub reason: String,
}

/// The full report from evaluating a set of policies.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PolicyReport {
    pub findings: Vec<Finding>,
}

impl PolicyReport {
    #[must_use]
    pub fn has_violations(&self) -> bool {
        self.findings.iter().any(|f| !f.satisfied)
    }

    #[must_use]
    pub fn violations(&self) -> Vec<&Finding> {
        self.findings.iter().filter(|f| !f.satisfied).collect()
    }

    #[must_use]
    pub fn passes(&self) -> Vec<&Finding> {
        self.findings.iter().filter(|f| f.satisfied).collect()
    }

    #[must_use]
    pub fn total(&self) -> usize {
        self.findings.len()
    }
}

/// Evaluate a slice of policies against a target sexpr tree.
#[must_use]
pub fn evaluate(policies: &[Policy], target: &SExpr) -> PolicyReport {
    let mut findings = Vec::new();
    for policy in policies {
        walk_and_eval(policy, target, "", &mut findings);
    }
    PolicyReport { findings }
}

fn walk_and_eval(
    policy: &Policy,
    target: &SExpr,
    path: &str,
    out: &mut Vec<Finding>,
) {
    if policy.pattern.matches(target) {
        eval_rule(policy, target, path, out);
    }

    // Recurse into children regardless — deep matches are expected.
    if let SExpr::List(items) = target {
        // Try to name children by keyword when this is a struct-form;
        // otherwise by index.
        let struct_fields = if let Some((head, tail)) = items.split_first() {
            if matches!(head, SExpr::Symbol(s) if !s.starts_with(':')) {
                // Could be a struct-form — attempt keyword labelling.
                let head_name = match head {
                    SExpr::Symbol(s) => s.clone(),
                    _ => String::new(),
                };
                let labelled: Vec<(String, &SExpr)> = tail
                    .iter()
                    .enumerate()
                    .map(|(i, child)| {
                        if let SExpr::List(pair) = child {
                            if pair.len() == 2 {
                                if let SExpr::Symbol(k) = &pair[0] {
                                    if let Some(name) = k.strip_prefix(':') {
                                        return (name.to_string(), &pair[1]);
                                    }
                                }
                            }
                        }
                        (format!("[{i}]"), child)
                    })
                    .collect();
                Some((head_name, labelled))
            } else {
                None
            }
        } else {
            None
        };

        if let Some((_, labelled)) = struct_fields {
            for (name, child) in labelled {
                let child_path = if path.is_empty() {
                    name
                } else {
                    format!("{path}.{name}")
                };
                walk_and_eval(policy, child, &child_path, out);
            }
        } else {
            for (i, child) in items.iter().enumerate() {
                let child_path = if path.is_empty() {
                    format!("[{i}]")
                } else {
                    format!("{path}[{i}]")
                };
                walk_and_eval(policy, child, &child_path, out);
            }
        }
    }
}

fn eval_rule(policy: &Policy, target: &SExpr, path: &str, out: &mut Vec<Finding>) {
    match &policy.rule {
        Rule::Deny(reason) => out.push(Finding {
            policy_id: policy.id.clone(),
            path: path.to_string(),
            satisfied: false,
            reason: reason.clone(),
        }),
        Rule::RequireField { field, pattern } => {
            let fields = match target {
                SExpr::List(items) => {
                    let tail = items.split_first().map_or(&items[..], |(_, t)| t);
                    parse_keyword_fields(tail)
                }
                _ => std::collections::BTreeMap::new(),
            };
            let satisfied = match fields.get(field.as_str()) {
                Some(v) => pattern.matches(v),
                None => matches!(pattern, Pattern::Nil),
            };
            out.push(Finding {
                policy_id: policy.id.clone(),
                path: path.to_string(),
                satisfied,
                reason: if satisfied {
                    format!("required field '{field}' present and matches")
                } else {
                    format!("required field '{field}' missing or did not match")
                },
            });
        }
        Rule::ForbidField { field } => {
            let fields = match target {
                SExpr::List(items) => {
                    let tail = items.split_first().map_or(&items[..], |(_, t)| t);
                    parse_keyword_fields(tail)
                }
                _ => std::collections::BTreeMap::new(),
            };
            let satisfied = match fields.get(field.as_str()) {
                Some(SExpr::Nil) | None => true,
                _ => false,
            };
            out.push(Finding {
                policy_id: policy.id.clone(),
                path: path.to_string(),
                satisfied,
                reason: if satisfied {
                    format!("forbidden field '{field}' is absent or nil")
                } else {
                    format!("forbidden field '{field}' is present with non-nil value")
                },
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::IacType;
    use crate::sexpr::ToSExpr;
    use crate::testing::{test_resource, TestAttributeBuilder};

    // ── Pattern primitives ──────────────────────────────────

    #[test]
    fn any_matches_everything() {
        assert!(Pattern::Any.matches(&SExpr::Integer(42)));
        assert!(Pattern::Any.matches(&SExpr::String("x".into())));
        assert!(Pattern::Any.matches(&SExpr::Nil));
    }

    #[test]
    fn symbol_exact_match() {
        assert!(Pattern::Symbol("foo".into()).matches(&SExpr::Symbol("foo".into())));
        assert!(!Pattern::Symbol("foo".into()).matches(&SExpr::Symbol("bar".into())));
    }

    #[test]
    fn any_string_ignores_content() {
        assert!(Pattern::AnyString.matches(&SExpr::String("x".into())));
        assert!(!Pattern::AnyString.matches(&SExpr::Symbol("x".into())));
    }

    #[test]
    fn one_of_matches_strings_and_symbols() {
        let p = Pattern::OneOf(vec!["a".into(), "b".into()]);
        assert!(p.matches(&SExpr::String("a".into())));
        assert!(p.matches(&SExpr::Symbol("b".into())));
        assert!(!p.matches(&SExpr::String("c".into())));
    }

    #[test]
    fn bool_integer_nil() {
        assert!(Pattern::Bool(true).matches(&SExpr::Bool(true)));
        assert!(Pattern::Integer(42).matches(&SExpr::Integer(42)));
        assert!(Pattern::Nil.matches(&SExpr::Nil));
        assert!(!Pattern::Bool(true).matches(&SExpr::Bool(false)));
    }

    // ── Struct pattern ──────────────────────────────────────

    #[test]
    fn struct_pattern_matches_partial_fields() {
        // (attribute (:sensitive true) (:immutable false)) should match
        // a pattern that only checks :sensitive.
        let target = SExpr::parse(
            "(attribute (:sensitive true) (:immutable false))",
        )
        .unwrap();
        let p = Pattern::Struct {
            head: "attribute".into(),
            fields: vec![("sensitive".into(), Pattern::Bool(true))],
        };
        assert!(p.matches(&target));
    }

    #[test]
    fn struct_pattern_rejects_wrong_head() {
        let target = SExpr::parse("(resource (:sensitive true))").unwrap();
        let p = Pattern::Struct {
            head: "attribute".into(),
            fields: vec![("sensitive".into(), Pattern::Bool(true))],
        };
        assert!(!p.matches(&target));
    }

    #[test]
    fn struct_pattern_field_value_mismatch() {
        let target = SExpr::parse("(attribute (:sensitive false))").unwrap();
        let p = Pattern::Struct {
            head: "attribute".into(),
            fields: vec![("sensitive".into(), Pattern::Bool(true))],
        };
        assert!(!p.matches(&target));
    }

    // ── ListHead pattern ────────────────────────────────────

    #[test]
    fn list_head_positional_match() {
        let target = SExpr::parse("(list integer)").unwrap();
        let p = Pattern::ListHead {
            head: "list".into(),
            tail: vec![Pattern::Symbol("integer".into())],
        };
        assert!(p.matches(&target));
    }

    #[test]
    fn list_head_arity_mismatch() {
        let target = SExpr::parse("(list integer string)").unwrap();
        let p = Pattern::ListHead {
            head: "list".into(),
            tail: vec![Pattern::Symbol("integer".into())],
        };
        assert!(!p.matches(&target));
    }

    // ── Evaluation + rule plumbing ──────────────────────────

    fn resource_with_sensitive_mutable() -> SExpr {
        let mut r = test_resource("x");
        r.attributes = vec![
            TestAttributeBuilder::new("password", IacType::String)
                .required()
                .sensitive()
                .build(),
        ];
        r.to_sexpr()
    }

    fn resource_with_sensitive_immutable() -> SExpr {
        let mut r = test_resource("x");
        r.attributes = vec![
            TestAttributeBuilder::new("password", IacType::String)
                .required()
                .sensitive()
                .immutable()
                .build(),
        ];
        r.to_sexpr()
    }

    fn sensitive_must_be_immutable() -> Policy {
        Policy {
            id: "sensitive-immutable".into(),
            description: "every sensitive attribute must be immutable".into(),
            pattern: Pattern::Struct {
                head: "attribute".into(),
                fields: vec![("sensitive".into(), Pattern::Bool(true))],
            },
            rule: Rule::RequireField {
                field: "immutable".into(),
                pattern: Pattern::Bool(true),
            },
        }
    }

    #[test]
    fn policy_fires_when_rule_violated() {
        let target = resource_with_sensitive_mutable();
        let report = evaluate(&[sensitive_must_be_immutable()], &target);
        assert!(report.has_violations());
        assert_eq!(report.violations().len(), 1);
    }

    #[test]
    fn policy_passes_when_rule_satisfied() {
        let target = resource_with_sensitive_immutable();
        let report = evaluate(&[sensitive_must_be_immutable()], &target);
        assert!(!report.has_violations());
        assert_eq!(report.passes().len(), 1);
    }

    #[test]
    fn no_match_yields_empty_report() {
        // IacResource with NO sensitive attributes — pattern matches
        // nothing, so no findings at all.
        let mut r = test_resource("x");
        r.attributes = vec![
            TestAttributeBuilder::new("name", IacType::String).required().build(),
        ];
        let report = evaluate(&[sensitive_must_be_immutable()], &r.to_sexpr());
        assert_eq!(report.total(), 0);
    }

    #[test]
    fn deny_rule_produces_violation_on_every_match() {
        let r = test_resource("x"); // 3 attributes by default
        let policy = Policy {
            id: "no-attributes".into(),
            description: "forbid all attributes".into(),
            pattern: Pattern::Struct {
                head: "attribute".into(),
                fields: vec![],
            },
            rule: Rule::Deny("no attributes allowed".into()),
        };
        let report = evaluate(&[policy], &r.to_sexpr());
        // test_resource builds 3 attributes by default.
        assert_eq!(report.violations().len(), 3);
        assert!(report
            .violations()
            .iter()
            .all(|v| v.reason == "no attributes allowed"));
    }

    #[test]
    fn forbid_field_rule() {
        let target = resource_with_sensitive_immutable();
        let policy = Policy {
            id: "no-sensitive".into(),
            description: "no sensitive flag set".into(),
            pattern: Pattern::Struct {
                head: "attribute".into(),
                fields: vec![],
            },
            rule: Rule::ForbidField {
                field: "sensitive".into(),
            },
        };
        let report = evaluate(&[policy], &target);
        // The sensitive field IS present with value true, so rule fires.
        // (Every matched attribute is evaluated — only those with
        // :sensitive true actually violate.)
        assert!(report.has_violations());
    }

    #[test]
    fn multiple_policies_evaluated_independently() {
        let target = resource_with_sensitive_mutable();
        let p1 = sensitive_must_be_immutable();
        let p2 = Policy {
            id: "never-fires".into(),
            description: "impossible pattern".into(),
            pattern: Pattern::Symbol("nonexistent-head".into()),
            rule: Rule::Deny("x".into()),
        };
        let report = evaluate(&[p1, p2], &target);
        // p1 fires (violation), p2 never matches (no finding).
        assert_eq!(report.findings.len(), 1);
    }

    #[test]
    fn evaluation_is_deterministic() {
        let target = resource_with_sensitive_mutable();
        let a = evaluate(&[sensitive_must_be_immutable()], &target);
        let b = evaluate(&[sensitive_must_be_immutable()], &target);
        assert_eq!(a, b);
    }

    #[test]
    fn finding_path_points_into_struct() {
        let target = resource_with_sensitive_mutable();
        let report = evaluate(&[sensitive_must_be_immutable()], &target);
        let violation = &report.violations()[0];
        // The matched attribute lives under resource → attributes → [0]
        // (struct-form walk labels by keyword, list-form elements by
        // index). We just assert the path is non-empty.
        assert!(!violation.path.is_empty());
        assert!(violation.path.contains("attributes"));
    }

    // ── Report API ─────────────────────────────────────────

    #[test]
    fn report_counts() {
        let target = resource_with_sensitive_mutable();
        let report = evaluate(&[sensitive_must_be_immutable()], &target);
        assert_eq!(report.total(), 1);
        assert_eq!(report.violations().len(), 1);
        assert_eq!(report.passes().len(), 0);
    }

    #[test]
    fn empty_report_when_no_policies() {
        let target = resource_with_sensitive_mutable();
        let report = evaluate(&[], &target);
        assert_eq!(report.total(), 0);
        assert!(!report.has_violations());
    }
}
