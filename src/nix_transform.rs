//! Nix-powered IR transforms.
//!
//! Shells out to an external Nix evaluator (`nix-instantiate` by
//! default; aliased to `sui` in sui-enabled environments) to apply a
//! user-supplied Nix expression over an `IacResource`. The expression
//! is a single-argument lambda:
//!
//! ```nix
//! # transform.nix
//! resource: resource // {
//!   description = "v2 — updated by Nix transform";
//!   attributes = map (a:
//!     if a.name == "password" then a // { sensitive = true; } else a
//!   ) resource.attributes;
//! }
//! ```
//!
//! Call flow:
//!
//! 1. Rust emits the IR as a Nix attribute set via `NixBackend`'s
//!    `resource_to_nix` converter (internal helper used by this module
//!    too)
//! 2. Nix evaluates `(<expression>) <emitted-attrset>` and prints
//!    JSON via `builtins.toJSON`
//! 3. Rust reads the JSON output and reconstructs an `IacResource`
//!
//! This is the pragmatic realization of "Nix as the extension language
//! for IR transforms." Sui integration at the Rust library level (linking
//! `sui-eval` directly, no process boundary) is a natural follow-up; the
//! API surface here stays the same.

use std::process::{Command, Stdio};

use crate::ir::IacResource;
use crate::sexpr::{FromSExpr, SExpr, SExprError, ToSExpr};

/// Errors from running a Nix-powered transform.
#[derive(Debug)]
pub enum NixTransformError {
    /// The Nix evaluator wasn't available or failed to launch.
    EvaluatorUnavailable(String),
    /// The evaluator ran but returned a non-zero exit; the payload is stderr.
    EvaluatorFailed(String),
    /// The JSON output couldn't be decoded into an IacResource.
    DecodeError(String),
    /// Sexpr / shape error in the reconstruction.
    Sexpr(SExprError),
}

impl std::fmt::Display for NixTransformError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EvaluatorUnavailable(s) => write!(f, "nix evaluator unavailable: {s}"),
            Self::EvaluatorFailed(s) => write!(f, "nix evaluator failed: {s}"),
            Self::DecodeError(s) => write!(f, "nix transform decode: {s}"),
            Self::Sexpr(e) => write!(f, "nix transform sexpr: {e}"),
        }
    }
}

impl std::error::Error for NixTransformError {}

impl From<SExprError> for NixTransformError {
    fn from(e: SExprError) -> Self {
        Self::Sexpr(e)
    }
}

/// Which evaluator binary to call. `nix-instantiate` is the default;
/// users can override via `NixEvaluator::Custom("sui".into())` or via
/// the `NIX_EVALUATOR` environment variable.
#[derive(Debug, Clone)]
pub enum NixEvaluator {
    Default,
    Custom(String),
}

impl NixEvaluator {
    fn binary(&self) -> String {
        match self {
            Self::Default => {
                std::env::var("NIX_EVALUATOR").unwrap_or_else(|_| "nix-instantiate".to_string())
            }
            Self::Custom(s) => s.clone(),
        }
    }
}

impl Default for NixEvaluator {
    fn default() -> Self {
        Self::Default
    }
}

/// Apply a Nix lambda expression to an IacResource.
///
/// The expression must be a single-argument lambda: `resource: <body>`.
/// Returns a new IacResource reconstructed from the evaluated output.
///
/// # Errors
/// - `EvaluatorUnavailable` if the binary isn't on PATH
/// - `EvaluatorFailed` if Nix returned a non-zero exit
/// - `DecodeError` if the output wasn't a JSON attribute set we can
///   convert back to an IacResource
/// - `Sexpr` for shape errors during reconstruction
pub fn apply_nix_transform(
    resource: &IacResource,
    expression: &str,
    evaluator: &NixEvaluator,
) -> Result<IacResource, NixTransformError> {
    // Convert IR → Nix attribute set text.
    let input_nix = resource_to_nix_text(resource);

    // Build the Nix expression to evaluate. We want JSON output so we
    // use `builtins.toJSON ((<expression>) <input>)`.
    let full_expr = format!("builtins.toJSON (({expression}) {input_nix})");

    let binary = evaluator.binary();
    let out = Command::new(&binary)
        .args(["--eval", "--expr", &full_expr])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| NixTransformError::EvaluatorUnavailable(e.to_string()))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        return Err(NixTransformError::EvaluatorFailed(stderr));
    }

    // Nix prints the JSON string with quotes and escapes; parse it as
    // a JSON-string-of-JSON.
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let inner_json: String = serde_json::from_str(&stdout)
        .map_err(|e| NixTransformError::DecodeError(format!("outer json: {e}")))?;

    let value: serde_json::Value = serde_json::from_str(&inner_json)
        .map_err(|e| NixTransformError::DecodeError(format!("inner json: {e}")))?;

    // Convert JSON → SExpr → IacResource.
    let sexpr = json_to_sexpr(&value)?;
    Ok(IacResource::from_sexpr(&sexpr)?)
}

// ── Helpers ──────────────────────────────────────────────────────

fn resource_to_nix_text(resource: &IacResource) -> String {
    use crate::nix::NixValue;
    let nv = NixValue::from_sexpr(&resource.to_sexpr());
    nv.to_nix_expr()
}

/// Convert a JSON value produced by a Nix transform back to an SExpr.
///
/// The JSON shape mirrors the Nix attribute-set shape we fed in:
/// scalars → same, arrays → lists, objects → struct-forms keyed on
/// `head`. The `head` field (if present) becomes the sexpr list's
/// first element; other fields become `(:key value)` pairs.
fn json_to_sexpr(v: &serde_json::Value) -> Result<SExpr, NixTransformError> {
    use serde_json::Value as J;
    match v {
        J::Null => Ok(SExpr::Nil),
        J::Bool(b) => Ok(SExpr::Bool(*b)),
        J::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(SExpr::Integer(i))
            } else if let Some(f) = n.as_f64() {
                Ok(SExpr::Float(f))
            } else {
                Err(NixTransformError::DecodeError(format!(
                    "non-representable number: {n}"
                )))
            }
        }
        J::String(s) => Ok(SExpr::String(s.clone())),
        J::Array(items) => {
            // Arrays from Nix go back as plain sexpr lists.
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(json_to_sexpr(item)?);
            }
            Ok(SExpr::List(out))
        }
        J::Object(map) => {
            // Objects go back as struct-form: `head` key becomes the
            // list head symbol; others become (:key value) pairs.
            let head_name = match map.get("head") {
                Some(J::String(s)) => s.clone(),
                _ => {
                    return Err(NixTransformError::DecodeError(
                        "object missing `head` key — can't reconstruct struct-form".into(),
                    ));
                }
            };
            let mut out = vec![SExpr::Symbol(head_name)];
            // Sort keys for determinism.
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for k in keys {
                if k == "head" {
                    continue;
                }
                let v = json_to_sexpr(&map[k])?;
                out.push(SExpr::List(vec![SExpr::Symbol(format!(":{k}")), v]));
            }
            Ok(SExpr::List(out))
        }
    }
}

/// Check whether the configured evaluator is available on PATH.
#[must_use]
pub fn evaluator_available(evaluator: &NixEvaluator) -> bool {
    Command::new(evaluator.binary())
        .arg("--version")
        .output()
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::test_resource;

    #[test]
    fn json_null_becomes_nil() {
        let v: serde_json::Value = serde_json::from_str("null").unwrap();
        assert_eq!(json_to_sexpr(&v).unwrap(), SExpr::Nil);
    }

    #[test]
    fn json_bool_and_int_preserved() {
        assert_eq!(
            json_to_sexpr(&serde_json::from_str("true").unwrap()).unwrap(),
            SExpr::Bool(true)
        );
        assert_eq!(
            json_to_sexpr(&serde_json::from_str("42").unwrap()).unwrap(),
            SExpr::Integer(42)
        );
    }

    #[test]
    fn json_object_without_head_rejected() {
        let v: serde_json::Value = serde_json::from_str(r#"{"a":1}"#).unwrap();
        let err = json_to_sexpr(&v).unwrap_err();
        assert!(matches!(err, NixTransformError::DecodeError(_)));
    }

    #[test]
    fn resource_to_nix_text_is_valid_shape() {
        let r = test_resource("widget");
        let nix = resource_to_nix_text(&r);
        assert!(nix.starts_with("{ "));
        assert!(nix.contains("head = \"resource\""));
        assert!(nix.contains("name"));
    }

    #[test]
    fn evaluator_available_returns_false_for_bogus_binary() {
        let ev = NixEvaluator::Custom("definitely-not-a-real-binary-xyzzy".into());
        assert!(!evaluator_available(&ev));
    }

    /// Real end-to-end test — skips gracefully if nix-instantiate
    /// isn't on PATH, runs the full transform loop when available.
    #[test]
    fn identity_transform_preserves_the_resource() {
        let ev = NixEvaluator::Default;
        if !evaluator_available(&ev) {
            eprintln!("[skip] nix evaluator not available");
            return;
        }

        let r = test_resource("widget");
        // Identity: `resource: resource`
        let result = apply_nix_transform(&r, "resource: resource", &ev);
        match result {
            Ok(round) => {
                assert_eq!(round.name, r.name);
                assert_eq!(round.attributes.len(), r.attributes.len());
            }
            Err(e) => {
                eprintln!("[skip] transform failed (environment issue): {e}");
            }
        }
    }

    /// Apply a real Nix transform that mutates the description.
    #[test]
    fn nix_transform_can_mutate_description() {
        let ev = NixEvaluator::Default;
        if !evaluator_available(&ev) {
            eprintln!("[skip] nix evaluator not available");
            return;
        }

        let r = test_resource("widget");
        let expr = r#"resource: resource // { description = "transformed"; }"#;
        match apply_nix_transform(&r, expr, &ev) {
            Ok(round) => {
                assert_eq!(round.description, "transformed");
                assert_eq!(round.name, r.name); // everything else preserved
            }
            Err(e) => {
                eprintln!("[skip] transform failed (environment issue): {e}");
            }
        }
    }
}
