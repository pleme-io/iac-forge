//! Semantic diff over sexpr trees.
//!
//! Given two [`SExpr`] values, produce a list of [`Edit`]s describing
//! how to transform the first into the second. The diff is structural:
//! it walks the tree, compares by position and by struct-form keyword
//! labels, and reports adds/removes/changes at the finest granularity
//! it can while still being deterministic.
//!
//! # Why this instead of line diff?
//!
//! - **Survives field reordering**: struct-form `(:field val)` pairs
//!   are compared by keyword, so swapping two fields in a struct
//!   produces zero edits (semantically identical).
//! - **Human-readable paths**: each edit carries a dotted path like
//!   `resource.attributes[2].required` so review isn't a character
//!   hunt.
//! - **Feeds remediation**: an edit list over IR is the same shape
//!   the transform script consumes — you can generate a remediation
//!   script from a diff.
//!
//! # Limitations
//!
//! - List diffing is positional (no LCS). Inserting an element at
//!   index 0 of a list of 5 produces 5 "changed" edits. Good enough
//!   for IR diffs where lists are usually small and stable-ordered.
//!   A full LCS can slot in later behind the same public API.

use crate::sexpr::SExpr;

/// A single edit in a semantic diff.
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    /// A value appeared in the new tree at `path` that was absent in the old.
    Added { path: String, value: SExpr },
    /// A value was present in the old tree at `path` but is absent in the new.
    Removed { path: String, value: SExpr },
    /// A leaf value at `path` changed from `old` to `new`.
    Changed {
        path: String,
        old: SExpr,
        new: SExpr,
    },
}

impl Edit {
    /// Dotted path of the edit location.
    #[must_use]
    pub fn path(&self) -> &str {
        match self {
            Self::Added { path, .. } | Self::Removed { path, .. } | Self::Changed { path, .. } => {
                path
            }
        }
    }
}

/// Compute edits that transform `old` into `new`.
#[must_use]
pub fn diff(old: &SExpr, new: &SExpr) -> Vec<Edit> {
    let mut edits = Vec::new();
    diff_into("".to_string(), old, new, &mut edits);
    edits
}

fn diff_into(path: String, old: &SExpr, new: &SExpr, out: &mut Vec<Edit>) {
    // Short-circuit: equal values emit nothing.
    if old == new {
        return;
    }

    // If either side is a struct-form list `(name (:field val) ...)`
    // and both sides are struct-forms with the same head, do a
    // field-keyed merge so reordering produces zero edits.
    if let (Some((old_name, old_fields)), Some((new_name, new_fields))) =
        (struct_form(old), struct_form(new))
    {
        if old_name == new_name {
            diff_struct(&path, old_fields, new_fields, out);
            return;
        }
    }

    // If both sides are (list ...) forms, walk positionally.
    if let (Some(old_items), Some(new_items)) = (list_form(old), list_form(new)) {
        diff_list(&path, old_items, new_items, out);
        return;
    }

    // If both sides are tuple-tag lists with the same head (e.g.
    // `(list <T>)` as an IacType), walk children positionally.
    if let (SExpr::List(a), SExpr::List(b)) = (old, new) {
        if let (Some(head_a), Some(head_b)) = (a.first(), b.first()) {
            if head_a == head_b && !is_keyword_pair(a) && !is_keyword_pair(b) {
                // Same tag, compare tails.
                let (a_rest, b_rest) = (&a[1..], &b[1..]);
                if a_rest.len() == b_rest.len() {
                    for (i, (ai, bi)) in a_rest.iter().zip(b_rest).enumerate() {
                        let child_path = if path.is_empty() {
                            format!("[{i}]")
                        } else {
                            format!("{path}[{i}]")
                        };
                        diff_into(child_path, ai, bi, out);
                    }
                    return;
                }
            }
        }
    }

    // Fallback: leaf-level change.
    out.push(Edit::Changed {
        path,
        old: old.clone(),
        new: new.clone(),
    });
}

/// If `s` is `(name (:field val) ...)` and every tail item is a keyword
/// pair, return the head symbol and the field map. Otherwise None.
fn struct_form(s: &SExpr) -> Option<(&str, std::collections::BTreeMap<&str, &SExpr>)> {
    let items = match s {
        SExpr::List(items) => items,
        _ => return None,
    };
    let (head, rest) = items.split_first()?;
    let name = match head {
        SExpr::Symbol(s) => s.as_str(),
        _ => return None,
    };
    // The tail must be non-empty AND every item must be a (:keyword value)
    // pair for this to count as a struct-form.
    if rest.is_empty() {
        return None;
    }
    let mut fields = std::collections::BTreeMap::new();
    for item in rest {
        let pair = match item {
            SExpr::List(pair) => pair,
            _ => return None,
        };
        if pair.len() != 2 {
            return None;
        }
        let key = match &pair[0] {
            SExpr::Symbol(k) if k.starts_with(':') => &k[1..],
            _ => return None,
        };
        fields.insert(key, &pair[1]);
    }
    Some((name, fields))
}

/// If `s` is `(list item1 item2 ...)`, return the items slice.
fn list_form(s: &SExpr) -> Option<&[SExpr]> {
    let items = match s {
        SExpr::List(items) => items,
        _ => return None,
    };
    let (head, rest) = items.split_first()?;
    match head {
        SExpr::Symbol(s) if s == "list" => Some(rest),
        _ => None,
    }
}

fn is_keyword_pair(items: &[SExpr]) -> bool {
    // Heuristic: a struct-form-internal pair starts with `:`.
    items.len() == 2
        && matches!(&items[0], SExpr::Symbol(k) if k.starts_with(':'))
}

fn diff_struct(
    base: &str,
    old: std::collections::BTreeMap<&str, &SExpr>,
    new: std::collections::BTreeMap<&str, &SExpr>,
    out: &mut Vec<Edit>,
) {
    // Collect keys from both sides, deterministic by BTreeMap iteration.
    let mut keys: Vec<&str> = old.keys().chain(new.keys()).copied().collect();
    keys.sort_unstable();
    keys.dedup();
    for k in keys {
        let child = if base.is_empty() {
            k.to_string()
        } else {
            format!("{base}.{k}")
        };
        match (old.get(k), new.get(k)) {
            (Some(a), Some(b)) => diff_into(child, a, b, out),
            (None, Some(b)) => out.push(Edit::Added {
                path: child,
                value: (*b).clone(),
            }),
            (Some(a), None) => out.push(Edit::Removed {
                path: child,
                value: (*a).clone(),
            }),
            (None, None) => {}
        }
    }
}

fn diff_list(base: &str, old: &[SExpr], new: &[SExpr], out: &mut Vec<Edit>) {
    let common = old.len().min(new.len());
    for (i, (a, b)) in old.iter().zip(new).enumerate() {
        let child = if base.is_empty() {
            format!("[{i}]")
        } else {
            format!("{base}[{i}]")
        };
        diff_into(child, a, b, out);
    }
    // Surplus in new → Added
    for (i, b) in new.iter().enumerate().skip(common) {
        let child = if base.is_empty() {
            format!("[{i}]")
        } else {
            format!("{base}[{i}]")
        };
        out.push(Edit::Added {
            path: child,
            value: b.clone(),
        });
    }
    // Surplus in old → Removed
    for (i, a) in old.iter().enumerate().skip(common) {
        let child = if base.is_empty() {
            format!("[{i}]")
        } else {
            format!("{base}[{i}]")
        };
        out.push(Edit::Removed {
            path: child,
            value: a.clone(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexpr::ToSExpr;

    fn sym(s: &str) -> SExpr {
        SExpr::Symbol(s.into())
    }

    #[test]
    fn equal_values_yield_no_edits() {
        let a = SExpr::Integer(42);
        let b = SExpr::Integer(42);
        assert!(diff(&a, &b).is_empty());
    }

    #[test]
    fn different_leaves_yield_single_changed() {
        let a = SExpr::Integer(1);
        let b = SExpr::Integer(2);
        let edits = diff(&a, &b);
        assert_eq!(edits.len(), 1);
        assert!(matches!(edits[0], Edit::Changed { .. }));
        assert_eq!(edits[0].path(), "");
    }

    #[test]
    fn struct_form_reordering_is_invisible() {
        // Same fields, different order → zero edits (struct-form merges
        // by keyword).
        let a = SExpr::parse("(x (:a 1) (:b 2))").unwrap();
        let b = SExpr::parse("(x (:b 2) (:a 1))").unwrap();
        assert!(diff(&a, &b).is_empty());
    }

    #[test]
    fn struct_field_change_reports_field_path() {
        let a = SExpr::parse("(x (:a 1) (:b 2))").unwrap();
        let b = SExpr::parse("(x (:a 1) (:b 3))").unwrap();
        let edits = diff(&a, &b);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].path(), "b");
        assert!(matches!(edits[0], Edit::Changed { .. }));
    }

    #[test]
    fn struct_field_added() {
        let a = SExpr::parse("(x (:a 1))").unwrap();
        let b = SExpr::parse("(x (:a 1) (:b 2))").unwrap();
        let edits = diff(&a, &b);
        assert_eq!(edits.len(), 1);
        assert!(matches!(edits[0], Edit::Added { .. }));
        assert_eq!(edits[0].path(), "b");
    }

    #[test]
    fn struct_field_removed() {
        let a = SExpr::parse("(x (:a 1) (:b 2))").unwrap();
        let b = SExpr::parse("(x (:a 1))").unwrap();
        let edits = diff(&a, &b);
        assert_eq!(edits.len(), 1);
        assert!(matches!(edits[0], Edit::Removed { .. }));
        assert_eq!(edits[0].path(), "b");
    }

    #[test]
    fn different_struct_names_full_replacement() {
        // Different heads → can't merge; emits a single Changed at root.
        let a = SExpr::parse("(x (:a 1))").unwrap();
        let b = SExpr::parse("(y (:a 1))").unwrap();
        let edits = diff(&a, &b);
        assert_eq!(edits.len(), 1);
        assert!(matches!(edits[0], Edit::Changed { .. }));
    }

    #[test]
    fn list_form_positional_diff() {
        let a = SExpr::parse("(list 1 2 3)").unwrap();
        let b = SExpr::parse("(list 1 5 3)").unwrap();
        let edits = diff(&a, &b);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].path(), "[1]");
    }

    #[test]
    fn list_form_appended_element() {
        let a = SExpr::parse("(list 1 2)").unwrap();
        let b = SExpr::parse("(list 1 2 3)").unwrap();
        let edits = diff(&a, &b);
        assert_eq!(edits.len(), 1);
        assert!(matches!(edits[0], Edit::Added { .. }));
        assert_eq!(edits[0].path(), "[2]");
    }

    #[test]
    fn list_form_removed_element() {
        let a = SExpr::parse("(list 1 2 3)").unwrap();
        let b = SExpr::parse("(list 1 2)").unwrap();
        let edits = diff(&a, &b);
        assert_eq!(edits.len(), 1);
        assert!(matches!(edits[0], Edit::Removed { .. }));
        assert_eq!(edits[0].path(), "[2]");
    }

    #[test]
    fn tuple_tag_list_same_head_diffs_children() {
        // IacType::List(inner) is (list <inner>) — a tuple-tag form,
        // not a list-form (head is "list" symbol, but it has one child
        // and no "(list ...)" list wrapper). This test uses a different
        // head to avoid confusion with list-form.
        let a = SExpr::List(vec![sym("array"), SExpr::Integer(1)]);
        let b = SExpr::List(vec![sym("array"), SExpr::Integer(2)]);
        let edits = diff(&a, &b);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].path(), "[0]");
    }

    #[test]
    fn nested_struct_dotted_paths() {
        let a = SExpr::parse("(res (:name \"a\") (:meta (info (:v 1))))").unwrap();
        let b = SExpr::parse("(res (:name \"a\") (:meta (info (:v 2))))").unwrap();
        let edits = diff(&a, &b);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].path(), "meta.v");
    }

    #[test]
    fn multiple_simultaneous_changes() {
        let a = SExpr::parse("(res (:a 1) (:b 2) (:c 3))").unwrap();
        let b = SExpr::parse("(res (:a 1) (:b 99) (:c 100))").unwrap();
        let mut edits = diff(&a, &b);
        edits.sort_by(|x, y| x.path().cmp(y.path()));
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0].path(), "b");
        assert_eq!(edits[1].path(), "c");
    }

    #[test]
    fn iac_type_diff_field_level() {
        use crate::ir::IacType;
        let a = IacType::List(Box::new(IacType::String)).to_sexpr();
        let b = IacType::List(Box::new(IacType::Integer)).to_sexpr();
        let edits = diff(&a, &b);
        assert_eq!(edits.len(), 1);
        // (list string) vs (list integer) → tuple-tag diff at [0]
        assert_eq!(edits[0].path(), "[0]");
    }

    #[test]
    fn iac_attribute_diff_single_field() {
        use crate::testing::TestAttributeBuilder;
        use crate::ir::IacType;
        let a = TestAttributeBuilder::new("x", IacType::String).required().build();
        let b = TestAttributeBuilder::new("x", IacType::String).build(); // not required
        let edits = diff(&a.to_sexpr(), &b.to_sexpr());
        // The only differing field must be `required`. (TestAttributeBuilder
        // treats `required` and `optional` as independent flags; we assert
        // precisely what changed.)
        let paths: Vec<&str> = edits.iter().map(|e| e.path()).collect();
        assert!(
            paths.contains(&"required"),
            "expected 'required' in edits, got {paths:?}",
        );
        assert!(!paths.iter().any(|p| p.contains("name")));
        assert!(!paths.iter().any(|p| p.contains("iac-type")));
    }

    #[test]
    fn diff_is_deterministic() {
        let a = SExpr::parse("(res (:a 1) (:b 2))").unwrap();
        let b = SExpr::parse("(res (:a 10) (:b 20))").unwrap();
        let x = diff(&a, &b);
        let y = diff(&a, &b);
        assert_eq!(x, y);
    }

    #[test]
    fn empty_sexpr_comparison() {
        let a = SExpr::List(vec![]);
        let b = SExpr::List(vec![]);
        assert!(diff(&a, &b).is_empty());
    }
}
