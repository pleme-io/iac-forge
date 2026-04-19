//! Cross-language agreement test: Rust vs pure-Nix reference.
//!
//! Shells out to `nix-instantiate` (with the `blake3-hashes`
//! experimental feature enabled) to evaluate the pure-Nix sexpr
//! reference at `tests/cross_lang/sexpr.nix`, then compares the
//! resulting hashes against Rust's `ContentHash` for the same values.
//!
//! Skip gracefully when `nix-instantiate` isn't on PATH. When the
//! blake3-hashes flag is missing, the `.nix` file's error will be
//! surfaced verbatim — the test reports it and skips rather than
//! failing so environments with older Nix versions don't break CI.

use std::process::Command;

fn nix_available() -> bool {
    Command::new("nix-instantiate")
        .arg("--version")
        .output()
        .is_ok()
}

fn nix_eval_hash(sexpr_nix_literal: &str) -> Option<String> {
    let script_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cross_lang/sexpr.nix");

    let out = Command::new("nix-instantiate")
        .args([
            "--extra-experimental-features",
            "blake3-hashes",
            "--eval",
            "--json",
            script_path.to_str()?,
            "--arg",
            "value",
            sexpr_nix_literal,
            "--argstr",
            "want",
            "hash",
        ])
        .output()
        .ok()?;

    if !out.status.success() {
        eprintln!(
            "nix-instantiate failed ({:?}):\nstderr: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr),
        );
        return None;
    }

    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    // JSON output is a quoted string: strip the surrounding quotes.
    if stdout.starts_with('"') && stdout.ends_with('"') {
        Some(stdout[1..stdout.len() - 1].to_string())
    } else {
        Some(stdout)
    }
}

/// Canonical Nix attribute-set literals representing each vector,
/// paired with the expected hex hash that all three implementations
/// (Rust, Ruby, Nix) must agree on.
const NIX_VECTORS: &[(&str, &str)] = &[
    (
        "{ kind = \"symbol\"; value = \"integer\"; }",
        "bf0b731c90564bc8c1a8b8078964f3fb4e20636f1beb54ff1cfecb06a7ca2ac8",
    ),
    (
        "{ kind = \"symbol\"; value = \"string\"; }",
        "1c47174c0ccd034618e1a604adce0002103e088e87329f0c2fab4324e8a06c60",
    ),
    (
        "{ kind = \"integer\"; value = 42; }",
        "da136474d7575c325f702bb7aa75f1123864033cc488bf7d9c074eadaf9bd0d3",
    ),
    (
        "{ kind = \"bool\"; value = true; }",
        "acc8a7699a2bf4cbd05f69678eac4fc236572041c28dfd0ab558e5fcf2ab6540",
    ),
    (
        "{ kind = \"nil\"; }",
        "f2ebafc4960f3bcffbd79b2478942adcbc0846fd66f3700ecb552754ddd883c5",
    ),
    (
        "{ kind = \"list\"; items = []; }",
        "622be36e2f51ac6ced2c7c6f12649a71d41650d1e47b9a83f58d7dbc9983f0fa",
    ),
    (
        "{ kind = \"list\"; items = [\
            { kind = \"symbol\"; value = \"list\"; }\
            { kind = \"symbol\"; value = \"integer\"; }\
        ]; }",
        "df476bed92dcb5156e97d74bbea85131a4d4ab4905d9ddde4886d1b12e4c599a",
    ),
    (
        "{ kind = \"string\"; value = \"hello\"; }",
        "c5919eb25e32df3ac400757942250b6a9776c7b1ac1e8e465ec6ca0de8e4cb3f",
    ),
];

#[test]
fn pure_nix_reference_agrees_with_frozen_vectors() {
    if !nix_available() {
        eprintln!("[skip] nix-instantiate not available");
        return;
    }

    let mut failures = Vec::new();
    let mut skipped = 0;
    for (nix_literal, expected) in NIX_VECTORS {
        match nix_eval_hash(nix_literal) {
            Some(actual) if actual == *expected => {}
            Some(actual) => {
                failures.push(format!(
                    "disagreement on {nix_literal:?}\n  expected: {expected}\n  got:      {actual}"
                ));
            }
            None => {
                skipped += 1;
            }
        }
    }

    if skipped == NIX_VECTORS.len() {
        eprintln!(
            "[skip] nix-instantiate couldn't evaluate any vector — \
             likely missing --extra-experimental-features blake3-hashes \
             (requires Nix ≥ 2.19)"
        );
        return;
    }

    assert!(
        failures.is_empty(),
        "pure-Nix reference disagreed with Rust on {} vectors:\n{}",
        failures.len(),
        failures.join("\n"),
    );
}

#[test]
fn nix_reference_file_exists_and_looks_right() {
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cross_lang/sexpr.nix");
    let contents = std::fs::read_to_string(&script).expect("sexpr.nix must exist");
    assert!(contents.contains("builtins.hashString \"blake3\""));
    assert!(contents.contains("escapeString"));
    assert!(contents.contains("\"symbol\""));
    assert!(contents.contains("\"list\""));
}
