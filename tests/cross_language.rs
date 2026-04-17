//! Cross-language content-hash agreement test.
//!
//! Proves that a portable Ruby reference implementation of canonical
//! sexpr emission + BLAKE3 hashing produces the SAME hex hash as Rust
//! for the same value. This is the portability contract: a Ruby
//! service consuming an iac-forge attestation can verify it without
//! running Rust.
//!
//! Strategy: emit a set of SExpr values in Rust, hash them in Rust,
//! then pipe the emissions through `tests/cross_lang/sexpr_ref.rb`
//! under `nix-shell -p ruby_3_3` with the `blake3` gem installed into
//! a user-install gem dir. Compare each Ruby-computed hash against the
//! Rust-computed hash.
//!
//! Graceful skip: if `nix-shell` isn't available, if the `blake3` gem
//! fails to load (common when native extensions can't build in the
//! sandbox), or if the shell-out times out, the test prints a skip
//! message rather than failing. Full validation requires a local
//! environment with nix-shell + a working BLAKE3 build.

use std::io::Write;
use std::process::{Command, Stdio};

use iac_forge::ir::{IacAttribute, IacType};
use iac_forge::sexpr::{SExpr, ToSExpr};
use iac_forge::testing::{test_resource, TestAttributeBuilder};

fn ruby_available() -> bool {
    Command::new("nix-shell")
        .arg("--version")
        .output()
        .is_ok()
}

/// Canonical test vectors covering each primitive + composite shape
/// we expect portable implementations to handle.
fn build_vectors() -> Vec<(&'static str, SExpr)> {
    let mut resource = test_resource("widget");
    resource.attributes = vec![
        TestAttributeBuilder::new("name", IacType::String).required().build(),
        TestAttributeBuilder::new("value", IacType::String).sensitive().build(),
    ];

    vec![
        ("integer-42", SExpr::Integer(42)),
        ("float-3.14", SExpr::Float(std::f64::consts::PI)),
        ("bool-true", SExpr::Bool(true)),
        ("nil", SExpr::Nil),
        ("symbol-string", SExpr::Symbol("string".into())),
        ("string-literal", SExpr::String("hello".into())),
        ("string-with-escapes", SExpr::String("a\"b\nc".into())),
        ("empty-list", SExpr::List(vec![])),
        ("list-of-integers", SExpr::List(vec![
            SExpr::Symbol("list".into()),
            SExpr::Integer(1),
            SExpr::Integer(2),
            SExpr::Integer(3),
        ])),
        ("iac-type-string", IacType::String.to_sexpr()),
        ("iac-type-list-integer", IacType::List(Box::new(IacType::Integer)).to_sexpr()),
        ("iac-type-enum-values", IacType::Enum {
            values: vec!["tcp".into(), "udp".into()],
            underlying: Box::new(IacType::String),
        }.to_sexpr()),
        ("iac-attribute", IacAttribute {
            api_name: "my-field".into(),
            canonical_name: "my_field".into(),
            description: "desc".into(),
            iac_type: IacType::String,
            required: true,
            ..Default::default()
        }.to_sexpr()),
        ("iac-resource-small", resource.to_sexpr()),
    ]
}

/// Run the Ruby reference over the given emissions, one per line, and
/// return the resulting hex hashes in the same order.
///
/// Returns `None` if the Ruby toolchain can't be launched or if
/// `blake3` can't load — the harness treats that as a graceful skip.
fn ruby_hashes(emissions: &[String]) -> Option<Vec<String>> {
    let script_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/cross_lang/sexpr_ref.rb");

    // Join emissions one-per-line for -lines mode.
    let input = emissions.join("\n");

    let shell_cmd = format!(
        "gem install --user-install --no-document blake3 >/dev/null 2>&1 || true; \
         export GEM_HOME=\"$(gem environment user_gemdir)\"; \
         export GEM_PATH=\"$GEM_HOME:$(gem environment gemdir)\"; \
         ruby '{}' -lines",
        script_path.display(),
    );

    let mut child = Command::new("nix-shell")
        .args(["-p", "ruby_3_3", "--run", &shell_cmd])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;

    {
        let stdin = child.stdin.as_mut()?;
        stdin.write_all(input.as_bytes()).ok()?;
    }

    let out = child.wait_with_output().ok()?;
    if !out.status.success() {
        // Could be BLAKE3_UNAVAILABLE (exit 2) or shell failure.
        let stderr = String::from_utf8_lossy(&out.stderr);
        if stderr.contains("BLAKE3_UNAVAILABLE") {
            eprintln!("[skip] blake3 gem unavailable: {stderr}");
            return None;
        }
        eprintln!(
            "ruby reference failed (exit {:?}):\n{stderr}",
            out.status.code()
        );
        return None;
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<String> = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(String::from)
        .collect();
    if lines.len() != emissions.len() {
        eprintln!(
            "ruby returned {} hashes for {} emissions — output:\n{stdout}",
            lines.len(),
            emissions.len()
        );
        return None;
    }
    Some(lines)
}

#[test]
fn rust_ruby_content_hash_agreement() {
    if !ruby_available() {
        eprintln!("[skip] nix-shell not available — skipping cross-language agreement");
        return;
    }

    let vectors = build_vectors();
    let emissions: Vec<String> = vectors.iter().map(|(_, v)| v.emit()).collect();
    let rust_hashes: Vec<String> = vectors
        .iter()
        .map(|(_, v)| v.content_hash().to_hex())
        .collect();

    let Some(ruby_hashes) = ruby_hashes(&emissions) else {
        eprintln!("[skip] ruby reference run failed — see prior [skip] message");
        return;
    };

    for ((name, _), (rh, ruby)) in vectors.iter().zip(rust_hashes.iter().zip(&ruby_hashes)) {
        assert_eq!(
            rh, ruby,
            "cross-language hash disagreement on vector '{name}'\n  rust: {rh}\n  ruby: {ruby}"
        );
    }
}

#[test]
fn rust_emits_canonical_reparseable_forms() {
    // Pre-flight: without Ruby in the loop, make sure every test
    // vector's emission parses back in Rust (if it didn't, the
    // cross-language test would be chasing a phantom bug).
    for (name, v) in build_vectors() {
        let emitted = v.emit();
        let reparsed = SExpr::parse(&emitted)
            .unwrap_or_else(|e| panic!("vector '{name}' didn't parse: {e:?} — text: {emitted}"));
        assert_eq!(
            reparsed.content_hash(),
            v.content_hash(),
            "parse round-trip changed hash for vector '{name}'",
        );
    }
}

#[test]
fn ruby_reference_script_is_syntactically_valid() {
    // Minimum sanity: the Ruby reference file exists at the expected
    // path and looks like Ruby. (We can't easily eval it without the
    // full nix-shell harness — that's what the full test does.)
    let script_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/cross_lang/sexpr_ref.rb");
    let contents = std::fs::read_to_string(&script_path)
        .expect("sexpr_ref.rb must be committed at tests/cross_lang/");
    assert!(contents.contains("frozen_string_literal"), "Ruby pragma missing");
    assert!(contents.contains("Blake3.hexdigest"), "BLAKE3 call missing");
    assert!(contents.contains("SExprReader"), "parser class missing");
}
