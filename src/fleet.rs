//! Fleet-level IR aggregation and hashing.
//!
//! A `Fleet` is a named collection of `IacResource` values — an entire
//! cluster's worth of declarations in one value. Like individual
//! resources, it emits a canonical sexpr, content-hashes over that
//! emission, and round-trips losslessly through `ToSExpr`/`FromSExpr`.
//!
//! # Why it matters
//!
//! Individual resources attest themselves. A Fleet attests the *whole
//! set* — sekiban can gate admission on fleet membership + hash match,
//! so a single deploy carries a single verifiable identity for an
//! entire environment. Drift at any resource changes the fleet hash;
//! drift nowhere leaves it stable.
//!
//! # Design
//!
//! - Storage: `BTreeMap<String, IacResource>` — deterministic order by
//!   member name, which is what canonical emission needs.
//! - Sexpr form: `(fleet (:name "…") (:members (list (member (:name "a") (:resource <r>)) …)))`.
//! - Content hash: BLAKE3 over the canonical emission, same pattern as
//!   every other ToSExpr type.
//! - `member_hash(name)` exposes the per-resource hash so callers can
//!   attest specific members without re-hashing the fleet.

use std::collections::BTreeMap;

use crate::ir::IacResource;
use crate::sexpr::{
    parse_struct, struct_expr, take_field, FromSExpr, SExpr, SExprError, ToSExpr,
};

/// A named collection of `IacResource` values with a stable canonical
/// order (by member name).
#[derive(Debug, Clone, Default)]
pub struct Fleet {
    /// Human-readable fleet name (often an environment or cluster id).
    pub name: String,
    /// Members keyed by member name — `BTreeMap` so iteration is sorted.
    pub members: BTreeMap<String, IacResource>,
}

impl Fleet {
    /// New empty fleet with the given name.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            members: BTreeMap::new(),
        }
    }

    /// Insert or replace a member by name. Returns the previous value.
    pub fn insert(
        &mut self,
        name: impl Into<String>,
        resource: IacResource,
    ) -> Option<IacResource> {
        self.members.insert(name.into(), resource)
    }

    /// Remove a member. Returns the removed value.
    pub fn remove(&mut self, name: &str) -> Option<IacResource> {
        self.members.remove(name)
    }

    /// Number of members.
    #[must_use]
    pub fn len(&self) -> usize {
        self.members.len()
    }

    /// Whether the fleet is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// Look up a member by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&IacResource> {
        self.members.get(name)
    }

    /// Per-member content hash in hex (the same hash the blanket
    /// Backend→Morphism impl records as `source_hash`).
    #[must_use]
    pub fn member_hash(&self, name: &str) -> Option<String> {
        self.members
            .get(name)
            .map(|r| r.content_hash().to_hex())
    }

    /// Member names in canonical (sorted) order.
    #[must_use]
    pub fn member_names(&self) -> Vec<&str> {
        self.members.keys().map(String::as_str).collect()
    }
}

// ── Sexpr serialization ─────────────────────────────────────────────

impl ToSExpr for Fleet {
    fn to_sexpr(&self) -> SExpr {
        let members: Vec<SExpr> = self
            .members
            .iter()
            .map(|(name, resource)| {
                struct_expr(
                    "member",
                    vec![
                        ("name", name.to_sexpr()),
                        ("resource", resource.to_sexpr()),
                    ],
                )
            })
            .collect();
        let mut member_list = Vec::with_capacity(members.len() + 1);
        member_list.push(SExpr::Symbol("list".into()));
        member_list.extend(members);
        struct_expr(
            "fleet",
            vec![
                ("name", self.name.to_sexpr()),
                ("members", SExpr::List(member_list)),
            ],
        )
    }
}

impl FromSExpr for Fleet {
    fn from_sexpr(s: &SExpr) -> Result<Self, SExprError> {
        let f = parse_struct(s, "fleet")?;
        let name = String::from_sexpr(take_field(&f, "name")?)?;
        let members_sexpr = take_field(&f, "members")?;
        let items = members_sexpr.as_list()?;
        let (head, rest) = items.split_first().ok_or_else(|| {
            SExprError::Shape("fleet members must be a (list …) form".into())
        })?;
        let tag = head.as_symbol()?;
        if tag != "list" {
            return Err(SExprError::Shape(format!(
                "expected 'list' for fleet members, got '{tag}'"
            )));
        }
        let mut members = BTreeMap::new();
        for item in rest {
            let mf = parse_struct(item, "member")?;
            let member_name = String::from_sexpr(take_field(&mf, "name")?)?;
            let resource = IacResource::from_sexpr(take_field(&mf, "resource")?)?;
            members.insert(member_name, resource);
        }
        Ok(Self { name, members })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexpr::SExpr;
    use crate::testing::test_resource;

    fn two_member_fleet() -> Fleet {
        let mut fleet = Fleet::new("prod");
        fleet.insert("api", test_resource("api"));
        fleet.insert("worker", test_resource("worker"));
        fleet
    }

    // ── Basic construction ────────────────────────────────────

    #[test]
    fn new_fleet_is_empty() {
        let fleet = Fleet::new("x");
        assert!(fleet.is_empty());
        assert_eq!(fleet.len(), 0);
        assert_eq!(fleet.name, "x");
    }

    #[test]
    fn insert_and_get() {
        let mut fleet = Fleet::new("x");
        fleet.insert("a", test_resource("a"));
        assert_eq!(fleet.len(), 1);
        assert_eq!(fleet.get("a").unwrap().name, "a");
    }

    #[test]
    fn remove_returns_value() {
        let mut fleet = two_member_fleet();
        let removed = fleet.remove("api");
        assert!(removed.is_some());
        assert_eq!(fleet.len(), 1);
    }

    #[test]
    fn insert_replaces_returns_previous() {
        let mut fleet = Fleet::new("x");
        let a1 = test_resource("a");
        let mut a2 = test_resource("a");
        a2.description = "different".into();

        assert!(fleet.insert("a", a1).is_none());
        let prev = fleet.insert("a", a2);
        assert!(prev.is_some());
        assert_eq!(fleet.len(), 1);
    }

    #[test]
    fn member_names_are_sorted() {
        let mut fleet = Fleet::new("x");
        fleet.insert("z", test_resource("z"));
        fleet.insert("a", test_resource("a"));
        fleet.insert("m", test_resource("m"));
        assert_eq!(fleet.member_names(), vec!["a", "m", "z"]);
    }

    // ── Sexpr round-trip ──────────────────────────────────────

    #[test]
    fn round_trip_empty() {
        let fleet = Fleet::new("empty");
        let parsed = Fleet::from_sexpr(&fleet.to_sexpr()).unwrap();
        assert_eq!(parsed.content_hash(), fleet.content_hash());
        assert_eq!(parsed.name, fleet.name);
        assert_eq!(parsed.member_names(), fleet.member_names());
    }

    #[test]
    fn round_trip_single_member() {
        let mut fleet = Fleet::new("one");
        fleet.insert("only", test_resource("only"));
        let parsed = Fleet::from_sexpr(&fleet.to_sexpr()).unwrap();
        assert_eq!(parsed.content_hash(), fleet.content_hash());
        assert_eq!(parsed.name, fleet.name);
        assert_eq!(parsed.member_names(), fleet.member_names());
    }

    #[test]
    fn round_trip_multiple_members() {
        let fleet = two_member_fleet();
        let parsed = Fleet::from_sexpr(&fleet.to_sexpr()).unwrap();
        assert_eq!(parsed.content_hash(), fleet.content_hash());
        assert_eq!(parsed.name, fleet.name);
        assert_eq!(parsed.member_names(), fleet.member_names());
    }

    #[test]
    fn round_trip_through_text_boundary() {
        let fleet = two_member_fleet();
        let emitted = fleet.to_sexpr().emit();
        let parsed = Fleet::from_sexpr(&SExpr::parse(&emitted).unwrap()).unwrap();
        assert_eq!(parsed.content_hash(), fleet.content_hash());
        assert_eq!(parsed.member_names(), fleet.member_names());
    }

    // ── Hashing ───────────────────────────────────────────────

    #[test]
    fn fleet_content_hash_is_deterministic() {
        let a = two_member_fleet();
        let b = two_member_fleet();
        assert_eq!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn different_fleets_differ_in_hash() {
        let a = two_member_fleet();
        let b = Fleet::new("prod");
        assert_ne!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn adding_a_member_changes_hash() {
        let a = two_member_fleet();
        let mut b = two_member_fleet();
        b.insert("extra", test_resource("extra"));
        assert_ne!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn insertion_order_does_not_affect_hash() {
        // BTreeMap canonicalizes order, so insertion order is irrelevant.
        let mut a = Fleet::new("prod");
        a.insert("z", test_resource("z"));
        a.insert("a", test_resource("a"));

        let mut b = Fleet::new("prod");
        b.insert("a", test_resource("a"));
        b.insert("z", test_resource("z"));

        assert_eq!(a.content_hash(), b.content_hash());
    }

    #[test]
    fn renaming_fleet_changes_hash() {
        let mut a = Fleet::new("prod");
        a.insert("api", test_resource("api"));
        let mut b = Fleet::new("staging");
        b.insert("api", test_resource("api"));
        assert_ne!(a.content_hash(), b.content_hash());
    }

    // ── Member-level attestation ──────────────────────────────

    #[test]
    fn member_hash_matches_resource_content_hash() {
        let fleet = two_member_fleet();
        let expected = test_resource("api").content_hash().to_hex();
        assert_eq!(fleet.member_hash("api").unwrap(), expected);
    }

    #[test]
    fn member_hash_is_none_for_missing() {
        let fleet = two_member_fleet();
        assert!(fleet.member_hash("nope").is_none());
    }

    #[test]
    fn mutating_one_member_changes_fleet_hash() {
        let mut a = two_member_fleet();
        let mut b = two_member_fleet();
        assert_eq!(a.content_hash(), b.content_hash());

        // Mutate one member in b.
        if let Some(r) = b.members.get_mut("api") {
            r.description = "mutated".into();
        }
        assert_ne!(a.content_hash(), b.content_hash());

        // And the member hash of that member also differs.
        assert_ne!(a.member_hash("api"), b.member_hash("api"));
        // Unchanged member's hash is still equal.
        assert_eq!(a.member_hash("worker"), b.member_hash("worker"));
    }

    // ── Error paths ───────────────────────────────────────────

    #[test]
    fn from_sexpr_rejects_wrong_top_level() {
        let err = Fleet::from_sexpr(&SExpr::parse("(resource (:name \"x\"))").unwrap())
            .unwrap_err();
        assert!(matches!(err, SExprError::Shape(_)));
    }

    #[test]
    fn from_sexpr_rejects_non_list_members() {
        let s = SExpr::parse("(fleet (:name \"x\") (:members 42))").unwrap();
        let err = Fleet::from_sexpr(&s).unwrap_err();
        assert!(matches!(err, SExprError::Shape(_)));
    }
}
