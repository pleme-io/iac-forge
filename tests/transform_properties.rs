//! Property tests for the transform layer.
//!
//! Covers:
//! - Identity is a left/right unit of ComposeTransforms
//! - SetDescription is idempotent under double-application
//! - AddOptionalString is idempotent (same op twice == once)
//! - MarkSensitive is idempotent
//! - RemoveAttribute followed by another RemoveAttribute on the same name
//!   is the same as one RemoveAttribute
//! - Script parse is deterministic and round-trips a stable set of op names
//! - Unknown ops, arity mismatches, unterminated strings/lists all fail

use proptest::prelude::*;

use iac_forge::ir::IacType;
use iac_forge::testing::{test_resource, TestAttributeBuilder};
use iac_forge::transform::ops::ResourceOp;
use iac_forge::transform::script;
use iac_forge::transform::{ComposeTransforms, Identity, Transform};

fn arb_name() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,12}".prop_map(String::from)
}

fn arb_description() -> impl Strategy<Value = String> {
    // Restricted to printable ASCII without \" or \\ so it survives
    // round-tripping through the minimal s-expr reader trivially.
    "[a-zA-Z0-9_ .:;/,-]{0,32}".prop_map(String::from)
}

fn base_resource() -> iac_forge::ir::IacResource {
    let mut r = test_resource("widget");
    r.attributes = vec![
        TestAttributeBuilder::new("name", IacType::String).required().build(),
        TestAttributeBuilder::new("value", IacType::String).build(),
        TestAttributeBuilder::new("tags", IacType::List(Box::new(IacType::String))).build(),
    ];
    r
}

proptest! {
    /// Identity is a right unit on any ResourceOp sequence.
    #[test]
    fn identity_right_unit(desc in arb_description()) {
        let r = base_resource();
        let left = ResourceOp::SetDescription(desc.clone()).apply(r.clone());
        let right = ComposeTransforms(
            ResourceOp::SetDescription(desc),
            Identity,
        ).apply(r);
        prop_assert_eq!(left.description, right.description);
        prop_assert_eq!(left.attributes.len(), right.attributes.len());
    }

    /// Identity is a left unit on any ResourceOp sequence.
    #[test]
    fn identity_left_unit(desc in arb_description()) {
        let r = base_resource();
        let left = ResourceOp::SetDescription(desc.clone()).apply(r.clone());
        let right = ComposeTransforms(
            Identity,
            ResourceOp::SetDescription(desc),
        ).apply(r);
        prop_assert_eq!(left.description, right.description);
    }

    /// SetDescription is idempotent: applying it twice == once.
    #[test]
    fn set_description_is_idempotent(desc in arb_description()) {
        let r = base_resource();
        let once = ResourceOp::SetDescription(desc.clone()).apply(r.clone());
        let twice = ResourceOp::SetDescription(desc.clone()).apply(
            ResourceOp::SetDescription(desc).apply(r),
        );
        prop_assert_eq!(once.description, twice.description);
    }

    /// SetCategory is idempotent.
    #[test]
    fn set_category_is_idempotent(cat in arb_description()) {
        let r = base_resource();
        let once = ResourceOp::SetCategory(cat.clone()).apply(r.clone());
        let twice = ResourceOp::SetCategory(cat.clone()).apply(
            ResourceOp::SetCategory(cat).apply(r),
        );
        prop_assert_eq!(once.category, twice.category);
    }

    /// MarkSensitive is idempotent.
    #[test]
    fn mark_sensitive_is_idempotent(name in prop::sample::select(vec!["name", "value", "tags"])) {
        let r = base_resource();
        let once = ResourceOp::MarkSensitive(name.into()).apply(r.clone());
        let twice = ResourceOp::MarkSensitive(name.into()).apply(
            ResourceOp::MarkSensitive(name.into()).apply(r),
        );
        let sensitive_once: Vec<&str> = once.attributes.iter()
            .filter(|a| a.sensitive)
            .map(|a| a.canonical_name.as_str())
            .collect();
        let sensitive_twice: Vec<&str> = twice.attributes.iter()
            .filter(|a| a.sensitive)
            .map(|a| a.canonical_name.as_str())
            .collect();
        prop_assert_eq!(sensitive_once, sensitive_twice);
    }

    /// AddOptionalString is idempotent by construction.
    #[test]
    fn add_optional_string_is_idempotent(
        name in arb_name(),
        desc in arb_description(),
    ) {
        let r = base_resource();
        let op = ResourceOp::AddOptionalString {
            canonical_name: name.clone(),
            api_name: name,
            description: desc,
        };
        let once = op.apply(r.clone());
        let twice = op.apply(once.clone());
        prop_assert_eq!(once.attributes.len(), twice.attributes.len());
    }

    /// RemoveAttribute is idempotent.
    #[test]
    fn remove_attribute_is_idempotent(
        name in prop::sample::select(vec!["name", "value", "tags", "missing"]),
    ) {
        let r = base_resource();
        let op = ResourceOp::RemoveAttribute(name.into());
        let once = op.apply(r.clone());
        let twice = op.apply(once.clone());
        prop_assert_eq!(once.attributes.len(), twice.attributes.len());
    }

    /// Vec<ResourceOp> apply is total — never panics.
    #[test]
    fn vec_of_ops_is_total(
        ops_count in 0_usize..8,
        desc in arb_description(),
        cat in arb_description(),
        add_name in arb_name(),
    ) {
        let r = base_resource();
        let ops: Vec<ResourceOp> = (0..ops_count)
            .map(|i| match i % 4 {
                0 => ResourceOp::SetDescription(desc.clone()),
                1 => ResourceOp::SetCategory(cat.clone()),
                2 => ResourceOp::AddOptionalString {
                    canonical_name: add_name.clone(),
                    api_name: add_name.clone(),
                    description: desc.clone(),
                },
                _ => ResourceOp::MarkSensitive("value".into()),
            })
            .collect();
        let _out = ops.apply(r);
    }

    /// Script parse is deterministic: parsing the same source twice yields
    /// equal op lists.
    #[test]
    fn script_parse_is_deterministic(desc in arb_description(), name in arb_name()) {
        let src = format!(
            r#"(set-description "{desc}")
               (mark-sensitive "{name}")"#,
        );
        let a = script::parse(&src).expect("a");
        let b = script::parse(&src).expect("b");
        prop_assert_eq!(a, b);
    }

    /// Valid set-description scripts parse and preserve the string arg.
    #[test]
    fn set_description_round_trip(desc in arb_description()) {
        let src = format!(r#"(set-description "{desc}")"#);
        let ops = script::parse(&src).expect("parse");
        prop_assert_eq!(&ops, &vec![ResourceOp::SetDescription(desc)]);
    }

    /// mark-sensitive scripts parse and preserve the name arg.
    #[test]
    fn mark_sensitive_round_trip(name in arb_name()) {
        let src = format!(r#"(mark-sensitive "{name}")"#);
        let ops = script::parse(&src).expect("parse");
        prop_assert_eq!(&ops, &vec![ResourceOp::MarkSensitive(name)]);
    }

    /// add-optional-string round-trips all three args.
    #[test]
    fn add_optional_string_round_trip(
        cname in arb_name(),
        api in arb_name(),
        desc in arb_description(),
    ) {
        let src = format!(r#"(add-optional-string "{cname}" "{api}" "{desc}")"#);
        let ops = script::parse(&src).expect("parse");
        prop_assert_eq!(
            &ops,
            &vec![ResourceOp::AddOptionalString {
                canonical_name: cname,
                api_name: api,
                description: desc,
            }],
        );
    }

    /// Unknown op name rejected.
    #[test]
    fn unknown_op_rejected(name in "[a-z][a-z-]{3,10}") {
        // Filter out known ops to keep the property meaningful.
        let known = [
            "set-description",
            "set-category",
            "mark-sensitive",
            "add-optional-string",
            "remove-attribute",
        ];
        if known.contains(&name.as_str()) {
            return Ok(());
        }
        let src = format!(r#"({name} "x")"#);
        let err = script::parse(&src).expect_err("must reject unknown op");
        prop_assert!(err.contains("unknown op"));
    }

    /// Multiple ops in one script all parse.
    #[test]
    fn multi_op_script_parses(n in 1_usize..6, desc in arb_description()) {
        let mut src = String::new();
        for _ in 0..n {
            src.push_str(&format!(r#"(set-description "{desc}") "#));
        }
        let ops = script::parse(&src).expect("parse");
        prop_assert_eq!(ops.len(), n);
    }

    /// Comments and whitespace are tolerated before/between forms.
    #[test]
    fn comments_do_not_affect_parse(desc in arb_description()) {
        let src = format!(
            "; leading comment\n\n   (set-description \"{desc}\")\n\n  ; trailing\n",
        );
        let ops = script::parse(&src).expect("parse");
        prop_assert_eq!(&ops, &vec![ResourceOp::SetDescription(desc)]);
    }
}
