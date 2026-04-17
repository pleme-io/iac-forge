# The Metacircular Loop — Lisp Generates Rust Generates Lisp

> "There is an incredible relationship with Rust using Lisp to generate
> new Rust primitives to lock in any DSL."

This document names the platform's **evolution mechanism** — how the
closed axiom set at the Rust layer grows. It's the meta-layer above
`CANDIDATE_LOOP.md`: that document describes how candidates are
generated, proved, run, and deallocated over an *existing* axiom set;
this document describes how *the axiom set itself* evolves, while
staying closed and proven.

## The asymmetry at the heart of the platform

The platform has a deliberate asymmetry:

- **Rust layer**: closed, typed, proved, immutable across a given
  build. Invalid states unrepresentable. User cannot invent primitives.
- **Lisp layer**: open, free, compositional, unbounded. User writes
  anything composable within the Rust axiom set.

What was left implicit until now: **Lisp can ALSO be used to generate
PROPOSALS for new Rust axioms.** Those proposals are themselves
sexpr values — typed, content-hashed, subject to the same proof
machinery as any other candidate. If they satisfy the invariants, they
are emitted as Rust source, compiled, and become part of the closed
set. The platform is self-hosting at the axiom level.

This closes the loop:

```
 ┌──────────────────────────────────────────────────────┐
 │   Lisp composes over an axiom set  (CANDIDATE_LOOP)  │
 │                  │                                   │
 │                  │ accumulated pressure              │
 │                  │ (recurring patterns, proposed     │
 │                  │  primitives, compression          │
 │                  │  opportunities)                   │
 │                  ▼                                   │
 │     ┌──────────────────────────────┐                │
 │     │  Lisp proposes a new axiom   │                │
 │     │  (sexpr AxiomProposal)       │                │
 │     └────────────┬─────────────────┘                │
 │                  │                                   │
 │                  ▼                                   │
 │     ┌──────────────────────────────┐                │
 │     │  axiom-forge proves it       │                │
 │     │  (totality, determinism,     │                │
 │     │   round-trip, hash stability,│                │
 │     │   non-interference)          │                │
 │     └────────────┬─────────────────┘                │
 │                  │                                   │
 │                  ▼                                   │
 │     ┌──────────────────────────────┐                │
 │     │  emit Rust source            │                │
 │     │  (quote! + prettyplease,     │                │
 │     │   content-addressed output)  │                │
 │     └────────────┬─────────────────┘                │
 │                  │                                   │
 │                  ▼                                   │
 │     ┌──────────────────────────────┐                │
 │     │  rustc compiles              │                │
 │     │  (final gatekeeper — if      │                │
 │     │   rustc rejects, the axiom   │                │
 │     │   literally doesn't exist)   │                │
 │     └────────────┬─────────────────┘                │
 │                  │                                   │
 │                  ▼                                   │
 │          Axiom set + 1 variant                      │
 │                  │                                   │
 │                  └─── Lisp can now compose          │
 │                        over the new variant         │
 │                        (back to the top)            │
 │                                                      │
 └──────────────────────────────────────────────────────┘
```

The system is a **fixed point**: Lisp generates the Rust that Lisp
generates over.

## Precedents (researched)

This pattern has appeared in pieces across programming-language
research. axiom-forge is the first composition to put them all
together for infrastructure + ML + compliance in one Rust workspace:

- **Racket's `syntax-parse` + phase separation** (Culpepper, POPL)
  established that new primitives live in a compile-time environment
  separated from runtime, with typed pattern matching for proposal
  validation.
- **MetaOCaml + Davies-Pfenning modal staging** (POPL 1996) proved
  that quoted code can carry typing guarantees across stages — the
  `'a code` type prevents ill-typed proposals from ever being spliced.
- **Template Haskell** demonstrated — and warned — what happens if
  you generate code without proving invariants *before* emission:
  splice-time errors end up in expanded output, far from the
  generator. axiom-forge proves at proposal time, not emission time.
- **MLIR ODS** (Operation Definition Specification) separates "what
  the op is" (declaration), "what makes it valid" (verifier), and
  "how it rewrites" (patterns). That tripartite structure maps
  directly onto AxiomProposal: the proposal is the declaration, the
  invariants are the verifier, the proof certificate is the rewrite.
- **DreamCoder** (Ellis et al., PLDI 2021) established the
  wake/sleep library-learning loop: the library (axiom set) grows by
  consolidating solutions found in user programs. Once added, a
  primitive is closed. This is axiom-forge's long-term operational
  model.
- **Rustc + GHC self-hosting** showed that "grow the language from
  the inside" works when every new primitive is expressible in terms
  of previously-accepted primitives. The bootstrap chain IS the
  platform's history of axiom additions.
- **Nix eval-then-build separation** (and sui's pure-Rust mirror)
  gives us the content-addressing story: a proposal hashes to a
  stable identifier before emission; the hash IS the proposal's
  identity.

axiom-forge synthesizes these into a single pipeline: Racket's
typed proposals, MetaOCaml's staged correctness, MLIR's verifier
discipline, DreamCoder's library learning, rustc's bootstrap
irreversibility, Nix's content-addressing.

## The proof obligations

Every new axiom must satisfy — verifiable in pure Rust before any
emission:

1. **Name uniqueness** — variant doesn't collide with existing ones
   in the target enum.
2. **Sexpr round-trip** — `to_sexpr(from_sexpr(x)) == x` for the new
   variant, composed with existing ones.
3. **Content-hash stability** — equal values produce equal hashes;
   distinct values produce distinct hashes. The canonical emission
   rules apply to the new variant as they do to every existing one.
4. **Apply totality** — if the axiom has an `apply` method (it's a
   `Transform` or `Morphism`), that method is total over its declared
   domain.
5. **Apply determinism** — same input always produces same output.
6. **Non-interference** — adding the axiom doesn't break any existing
   frozen test vector. The 38+ cross-language vectors in iac-forge
   must still agree after the axiom is added.
7. **Compliance** — the axiom doesn't violate any platform-wide
   structural invariant (policy patterns, NIST/CIS control
   structures, etc.).

These are checked against the proposal's sexpr form BEFORE any Rust
source is emitted. If any proof fails, the proposal is rejected with
a structured error pointing at the failing invariant.

## What gets emitted

For an accepted proposal, axiom-forge emits:

1. The **new enum variant** (Rust source) in the target enum, with
   the declared fields.
2. **ToSExpr** implementation for the new variant (extended from the
   existing impl's match).
3. **FromSExpr** match arm for the new tag.
4. **Apply** method arm (if it's a Transform or Morphism axiom).
5. **Test cases** — at minimum a round-trip test with a frozen
   content hash vector. The new variant's hash becomes part of the
   cross-language frozen vector set.
6. A **certificate** — sexpr recording the proof transcript. This is
   tameshi-attested; the next version of the workspace carries the
   certificate chain for every axiom in the set.

The emitted Rust is formatted deterministically (prettyplease), so
the same proposal always produces byte-identical output. Content
addressing holds across regenerations.

## The consequences — this is the big thing

### 1. The axiom set is self-improving

Every repeated pattern in user Lisp code is a candidate for
promotion. Write `(sequence ... many-steps ...)` often enough, a
sleep pass identifies that sequence as a macro pattern, generates a
proposal `(axiom new-sequence ...)`, proves it, and adds it to the
Rust axiom set. User code then uses the shorter form.

This is DreamCoder's library learning applied to infrastructure,
ML, compliance. The platform's vocabulary grows where usage
concentrates it.

### 2. Compliance becomes additive and attestable

Every new compliance control (NIST, CIS, FedRAMP, PCI, SOC2, EU AI
Act) becomes a proposed axiom to the `Pattern` / `Rule` enums. The
proof obligation includes "this control is independent of existing
controls" and "this control composes monotonically with the
compliance lattice." Regulators reviewing a system can verify:
"axiom X was added on date Y, by proposal with hash Z, with
certificate signed by Q." Audit becomes *structural*, not
narrative.

### 3. The ML axiom set grows from usage

ml-forge ships with 15 UOps. Over time, usage reveals recurring
sub-patterns: say, a specific attention kernel that appears in every
transformer. A sleep pass proposes `UOp::FlashAttention { causal,
head_dim }`. arch-synthesizer verifies the decomposition into
existing UOps and numerical-equivalence bounds. rustc compiles.
Every backend implements the new op's lowering. User Lisp code
switches to the compact form. The platform's ML vocabulary grows
with the model architecture space.

### 4. Cross-language portability grows at the same rate

Every new axiom comes with a frozen cross-language vector —
`(canonical_text, b3sum_hex)` — added to
`tests/cross_lang_vectors.rs`. When Ruby / Nix / Python / TypeScript
implementations sync, they get the new vector automatically. The
cross-language contract grows monotonically without central
coordination, because BLAKE3 over the canonical form is the
coordinating medium.

### 5. Hot-reload is possible via WASM

Rust is AOT, but WASM is not. Accepted axioms can be emitted as WASM
modules (via substrate-forge) and dynamically linked into a running
substrate-forge Pod. Today this requires a workspace rebuild;
tomorrow it can be a content-hashed pull + instantiate.

### 6. The typescape has a history

Every axiom addition becomes a leaf in the typescape Merkle tree.
tameshi-attested. The typescape's root hash is a function of every
axiom ever accepted. Rolling back is impossible without a new root
(irreversibility by construction — matches rustc bootstrap).

### 7. Rustc is the final gatekeeper

If a proposal's proofs pass but rustc still rejects the emitted
Rust, the axiom doesn't exist. This is the last line of defense: a
bug in axiom-forge's prover cannot result in invalid Rust landing
in the workspace, because rustc will refuse to build it. The whole
system is double-gated — proof, then compile.

### 8. Every DSL gets this mechanism for free

Once axiom-forge exists, any new DSL in the platform (tensor IR,
compliance controls, remediation ops, substrate capabilities,
training steps, storage backends) gets the same evolution loop. You
don't write "a macro system for each DSL"; you write "axiom-forge
sees an enum, knows the patterns, emits new variants" once.

### 9. This is the Lisp promise delivered on Rust

For decades Lisp users have argued that homoiconicity + macros give
you a language that grows to fit any domain. Rust users have
argued that closed typed enums give you correctness by construction.
**axiom-forge gives both, in the same system, without compromise:
Lisp grows over Rust, Rust closes around Lisp.** The wager is that
the mechanism is the gateway — and since Rust's proc-macro
ecosystem is rich enough, the gateway is possible today.

### 10. The platform is now metacircular

The Rust compiler is the evaluator of Rust. axiom-forge is the
evaluator that takes Lisp proposals to new Rust. Lisp is the
evaluator of compositions over the axiom set. The whole system is a
fixed point where every layer evaluates the next by a different
mechanism, and together they form a closed loop the user can extend
by composing within the existing axiom set OR by proposing new
axioms that go through the gate.

## The relationship to CANDIDATE_LOOP.md

| Loop | Level | Output survives between iterations |
|------|-------|------------------------------------|
| **Candidate loop** (CANDIDATE_LOOP.md) | Operational — programs over an axiom set | Content hashes persist; in-memory candidates deallocate |
| **Metacircular loop** (this document) | Foundational — the axiom set itself | Axioms persist (rustc-verified); failed proposals leave no trace |

Both loops have the same shape: Lisp proposes, proof gates, Rust
(either the compiler or the runtime) is the final gatekeeper, winners
persist, losers deallocate. The metacircular loop is slower — a full
workspace rebuild per axiom — but irreversibility is the feature, not
the bug. Axioms are the permanent vocabulary; candidates are the
ephemeral programs.

## Where axiom-forge lives

`pleme-io/axiom-forge` (private). The companion implementation of
this document. Sibling to iac-forge, ml-forge, substrate-forge,
weights-forge, train-forge, and every future -forge crate.

## Further reading

Papers + talks worth reading if you want to go deeper:

- Culpepper, *Refining Syntactic Sugar*, PhD thesis — syntax-parse
  internals
- Davies & Pfenning, *A Modal Analysis of Staged Computation*,
  POPL 1996
- Sheard & Peyton Jones, *Template Meta-programming for Haskell*,
  HW 2002
- Ellis et al., *DreamCoder: Growing generalizable, interpretable
  knowledge with wake-sleep Bayesian program learning*, PLDI 2021
  (arXiv 2006.08381)
- Steele, *Growing a Language*, OOPSLA 1998 keynote
- Abelson & Sussman, *SICP* — the metacircular evaluator (chapter 4)
- Lattner et al., *MLIR*, CGO 2021 — dialect registration discipline
- Flatt, *Binding as Sets of Scopes*, POPL 2016 — modern Racket
  hygiene

---

**Companions:**
- `CANDIDATE_LOOP.md` — the operational loop over a fixed axiom set
- `PATTERNS.md` — the primitives used at both levels
- `SUBSTRATE_VISION.md` — the runtime verifier layer
- `ML_VISION.md` — the ML application of both loops

The platform is now a **fixed point**: candidates evolve over axioms
in the operational loop; axioms evolve over proposals in the
metacircular loop; both share the same typed-sexpr-with-BLAKE3
machinery. Every Lisp program is either a candidate or a proposal;
every Rust enum is either stable or the immediate-past result of a
metacircular step. Infrastructure. Machine learning. Compliance.
All the same shape.
