# Patterns — what composes, what it proves, and how to maximize it

This document is the synthesis of the primitives iac-forge now exposes
and the patterns they compose into. The target audience is anyone — human
or LLM — who needs to reason about *what's possible* given the toolbox
rather than read individual module docs.

## The four core primitives

Everything built in the recent cycle rests on four primitives. They are
strictly orthogonal: each one works without the others, and their value
compounds when they're composed.

| # | Primitive | Crate module | What it provides |
|---|-----------|--------------|------------------|
| 1 | **Canonical SExpr** | `sexpr` | Every IR value has one canonical text form + BLAKE3 content hash. Round-trip is lossless (proven by `tests/reversibility_lemma.rs`). |
| 2 | **ProvenMorphism** | `morphism` | Named, composable transformations `Src → Dst` that carry invariants. Composition inherits both participants' proofs; violations are traced to the source morphism by name. |
| 3 | **Typed bridges** | `ir` / `ruby-synthesizer::iac_bridge` / `nix` | Structure-preserving maps between representations: `IacType ↔ RubyType ↔ RbsType ↔ NixValue`. Each bridge is a `ProvenMorphism`. |
| 4 | **Backend trait** | `backend` | `IacResource → Vec<GeneratedArtifact>`. Every `Backend` is automatically a `ProvenMorphism` via the blanket impl. Seven backends (Ruby, RBS, Terraform, Pulumi, Crossplane, Ansible, Steampipe, Helm, Nix). |

Everything below is a composition of these four.

## The reversibility lemma

**For every `T` with `ToSExpr + FromSExpr`:**
```
∀ x : T .  T::from_sexpr(&x.to_sexpr())?  ≡  x
```
`≡` is content-hash equivalence (strict equality where `T: PartialEq`,
content-hash equality otherwise).

This lemma is what makes every derived pattern trustworthy:

- **Fixture interchange** works because save+load is the round-trip
- **Cross-language hashing** holds because all languages emit the same canonical form
- **Attestation signatures** survive re-parse because bytes reproduce
- **Render caches** are sound because equal IRs produce equal hashes

Proof: `tests/reversibility_lemma.rs` — 18 tests covering every
primitive and every major IR type, including a proptest over arbitrary
`IacType` values.

## Quality selection

Every representation optimizes different qualities. The `Backend` trait
plus `iac_forge::nix::NixValue` mean the SAME IR can be rendered into
any of:

| Representation | Best at | Worst at |
|----------------|---------|----------|
| **Ruby (Dry::Struct)** | Fluent DSL, runtime introspection | Static checking, reproducibility |
| **RBS** | Static type checking with `steep` | Runtime representation |
| **Rust types** | Compile-time proofs, perf | Edit friction, author ergonomics |
| **Nix attribute set** | Reproducible evaluation, fixed-output derivations | Readable diffs at scale |
| **Terraform HCL** | Industry standard, rich provider ecosystem | Type safety |
| **JSON** | Wire format, every-language support | Comments, references |
| **SExpr (canonical)** | Language-portable, content-addressable, attestable | Human authoring |

Selection strategy: pick the representation whose *qualities* match
the task. A `Quality` system now exists in `pipeline::Quality` — any
stage can declare which qualities it requires (`"validated"`,
`"compliance:nist-800-53"`) and which it establishes. Pipelines stop
at the first stage whose required qualities aren't held.

## Pipelines: promotions and mutations

`iac_forge::pipeline` gives the primitive:

- **Promotion** (`A → B`): cross a representation boundary. TOML →
  `IacResource` is a promotion that adds type safety. `IacResource` →
  `Vec<GeneratedArtifact>` is a promotion that adds renderability.
  Promotions are where new guarantees appear.

- **Mutation** (`A → A`): stay in the same representation, change
  content. Applying a `ResourceOp`, running a Nix transform, normalising
  fields. Mutations are where existing guarantees must still hold.

A `Stage<Src, Dst>` wraps a `Box<dyn ProvenMorphism<Src, Dst>>` plus
metadata: name, `StageKind`, `requires: Vec<Quality>`, `establishes:
Vec<Quality>`. Running a stage produces a `TraceStep` with:

- Stage and morphism names
- Input/output content hashes
- Qualities established
- Any invariant violations (empty on success)

`run_mutation_chain(start, stages, initial_qualities)` runs a chain
and returns `(T, Trace)` on success or `(PipelineError, partial_trace)`
on failure. The trace is itself a `ToSExpr` value — attestable,
hashable, storable.

**Quality selection in practice:**
```rust
// I want a value that has been validated AND is content-addressed.
let stages = vec![
    Stage::new("validate", StageKind::Promotion, …)
        .establishes(Quality::new("validated")),
    Stage::new("normalise", StageKind::Mutation, …)
        .requires(Quality::new("validated"))
        .establishes(Quality::new("content-addressed")),
];
let (value, trace) = run_mutation_chain(raw_input, stages, &[])?;
assert!(trace.established().contains(&Quality::new("content-addressed")));
```

## The content-addressing triangle

```
        ContentHash (BLAKE3)
         /    |    \
        /     |     \
       /      |      \
 source_hash  |   fleet hash
 on artifact  |   (Fleet of IRs)
              |
       source IR's
       content_hash()
```

- **Artifact.source_hash** ↔ hash of the IR that produced it
- **Fleet.content_hash** ↔ composite hash of all members (via BTreeMap
  canonical ordering)
- **RenderCache key** ↔ `(SCHEMA_VERSION, platform, ir_hash)` triple

A single deploy can now answer: "which IR produced this artifact?" via
`source_hash`, "which morphism chain?" via `morphism_chain`, "has
anything changed in the fleet?" via a one-word comparison of two
fleet hashes.

## Cross-language portability

**Any language that correctly implements:**
1. Canonical SExpr emission (rules in `sexpr.rs` module docs)
2. BLAKE3 over the emission

**will agree with Rust on all 38 frozen test vectors.** The vectors
are in `tests/cross_lang_vectors.rs` and verified against `b3sum 1.8.4`
out-of-band.

Implementations shipped so far:
- **Rust** (primary, this crate)
- **Ruby** (`tests/cross_lang/sexpr_ref.rb`, uses the `blake3` gem)
- **Nix** (`tests/cross_lang/sexpr.nix`, uses native `builtins.hashString
  "blake3"` — Nix ≥ 2.19 with `blake3-hashes` experimental feature)

The contract is inductive: any new language (Python, Go, TypeScript,
etc.) that reproduces the vectors is automatically interoperable with
every existing one. No central registry; BLAKE3 is the schelling point.

## Sui: Nix evaluation in Rust memory

`iac_forge::sui_transform` (feature: `sui-eval`) embeds `sui-eval`
(pure-Rust Nix) directly. A Nix transform runs *in-process* — no
serialization, no shell-out, no JSON round-trip. `sui::Value` ↔
`SExpr` is a direct conversion.

This is the realization of "concentrate an incredible vast amount of
tooling in Rust memory": one address space, one type system, Nix
evaluation, sexpr, typed IR, rendering, all composable.

## Nix as a first-class convergence surface

- `NixBackend`: IR → Nix attribute set (`resources/<provider>_<name>.nix`)
- `emit_fod`: IR → **fixed-output derivation** where `outputHash` =
  `content_hash().to_hex()`. The Nix store path IS the IR's identity.
  Re-build produces the same path; drift produces a new one.
- `pure-Nix sexpr reference`: verification in the Nix evaluator itself.
  Any flake can import an attestation and verify it with pure Nix, no
  IFD.

## Emergent wrapper patterns

Because `Backend` is a trait, a wrapper `Backend<B: Backend>` can add
cross-cutting concerns to any concrete backend without modifying it.
Three natural wrappers, not yet shipped but trivially derivable:

- **`PolicyGate<B>`**: runs `policy::evaluate` against the IR before
  delegating to B. Refuses rendering if any violation.
- **`CachedBackend<B>`**: threads a `RenderCache` through B.
- **`AttestedBackend<B>`**: emits an additional audit manifest
  artifact per resource recording morphism_chain + source_hash + a
  signature slot.

Each is a few dozen lines. The pattern they share — "wrap any Backend
to add cross-cutting enforcement" — generalizes: the next cross-cutting
concern (rate limiting, licensing, vendor-lock annotations) is the
same shape.

## Remediation as audit-complete

`iac_forge::remediation::{Proposal, Outcome, apply_proposal,
outcome_sexpr}` gives a bounded, auditable transformation loop:

1. **Proposal** — reason + script text
2. **Script parse** — transform DSL is bounded to declared
   `ResourceOp`s; malformed scripts are rejected before anything runs
3. **Apply** — transform IR → IR, retains all invariants
4. **Diff** — `sexpr_diff::diff(before, after)` produces typed edits
5. **Invariant check** — optional post-state invariants gate the
   outcome; violations convert to `InvariantViolations` error
6. **Audit header** — `outcome_sexpr(outcome)` emits a canonical
   attestation record with before/after hashes, edit count, op count,
   reason

All six are composable into `Pipeline` stages.

## The LLM-as-remediation-author loop

With the harness above, an LLM can safely propose remediations:

1. MCP tool: `propose_remediation(resource_sexpr, goal) → script_text`
2. LLM reads the IR as sexpr (it's the canonical form), reasons about
   the goal, emits a transform script
3. `apply_proposal` validates:
   - Script parses (bounded DSL)
   - Applied transform leaves IR invariants intact
   - Post-state invariants hold
4. The `Outcome` is attested via `outcome_sexpr` and signed

The LLM is the search heuristic; the structural proofs keep it safe.
An LLM that writes malformed scripts, scripts that violate invariants,
or scripts that don't achieve the goal fails at a specific, diagnosable
stage — the Outcome tells you *exactly* where.

## What to build next (if you want to maximize further)

In rough order of leverage-per-hour:

1. **`PolicyGate<B>` / `CachedBackend<B>` / `AttestedBackend<B>`**
   wrappers — ~100 lines each, huge leverage
2. **Pipeline executor with parallelism** — independent stages run
   concurrently
3. **Reverse rendering** (`RubyNode → IacType`, `NixValue → IR`) for
   round-trip from generated code back to IR — useful for import
4. **Python/Go/JavaScript references** — add more languages to the
   portability club
5. **Tameshi binding for `Outcome` and `Fleet`** — actual signatures
   over the canonical forms; sekiban admission gates on them
6. **`BackendQualities` metadata + registry** — declarative selection
   ("I want `reproducible-build` + `type-safe`; which backends match?")

## Test counts (current)

- iac-forge: 615 tests (lib + integration + proptest)
- ruby-synthesizer: 360 tests (incl. RBS + cross-language vectors)
- pangea-forge: 595 tests (incl. RBS toolchain validation)
- steampipe-forge: 79 tests

Total across the four repos this session touched: **1,649 tests**.

## Closing thought

The typed core doesn't restrict what you can represent — it just
prevents invalid states from being representable. Everything above is
a composition of that single idea + BLAKE3 + a small number of
well-chosen morphisms. Adding a new representation is a local change;
adding a new quality requires no central coordination; adding a new
language to the portability club requires only matching the canonical
form.

This is what "concentrate tooling in Rust memory" looks like when the
types are right.
