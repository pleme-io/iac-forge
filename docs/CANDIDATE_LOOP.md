# The Candidate-Generate → Prove → Run → Deallocate Loop

> "A combination of arch-synthesizer/typescape activity with lisp
> could easily run and prove and improve runtime programs in memory
> until we are sure of all our optimizations."
>
> "Lisp is the candidate generator."

This is the canonical pattern the platform realizes. **Every new
system should document where it sits in this loop.** This document
names the roles, pins down which crate plays which part, and describes
how the loop terminates safely.

---

## The five roles

| Role | Who plays it | Why this actor, not another |
|------|--------------|-----------------------------|
| **Axiom set** | Rust enums (`UOp`, `ResourceOp`, `Pattern`, `Rule`, `IacType`, …) | Closed, `#[non_exhaustive]`. New variants require a Rust PR + proof. Users can't invent primitives. |
| **Candidate generator** | Lisp (sexpr) / macros over the axioms | Homoiconic — programs ARE data. Macros at compile time, `macroexpand` for debugging, REPL for iteration. Generates arbitrarily many variations bounded by the axiom set. |
| **Composition prover** | `arch-synthesizer` + `typescape` | Walks the typescape, verifies the candidate composition satisfies AST-domain laws, morphism totality, compliance invariants. Emits a certificate — zero cloud cost. |
| **Runtime verifier** | `substrate-forge` + `wasmtime` + frozen test vectors | Materializes the candidate in memory as WASM, runs against a fixed evaluation set, compares output hashes to the previous winner's. |
| **Decision + deallocation** | Kubernetes + `sui` cache + `tameshi` | Winner's hash is cached/attested. Loser's memory returns to the scheduler. The Pod terminates. Only hashes persist. |

---

## The loop, step by step

```text
┌─────────────────────────────────────────────────────────────────┐
│ 1. CANDIDATE GENERATE (Lisp)                                    │
│    A Lisp macro expands to a composition of Rust axioms:        │
│    (defmodel v2 ...) → Graph::push(UOp::MatMul { ... })         │
│    The output is a typed value — an IacResource, a Graph, a     │
│    Policy, a training-step Pipeline.                            │
└────────────────┬────────────────────────────────────────────────┘
                 │  typed value with content_hash (BLAKE3 of sexpr)
                 ▼
┌─────────────────────────────────────────────────────────────────┐
│ 2. PROVE COMPOSITION (arch-synthesizer + typescape)             │
│    - AST-domain invariants: every primitive used is in the      │
│      closed axiom set                                           │
│    - Morphism totality: every transformation in the candidate   │
│      lands where it claims                                      │
│    - Type compatibility: shapes, dtypes, capabilities agree     │
│    - Compliance invariants (kensa): NIST/CIS/FedRAMP controls   │
│      hold structurally                                          │
│    Output: certificate (attested sexpr) OR a list of            │
│    violations with source-traced names.                         │
└────────────────┬────────────────────────────────────────────────┘
                 │  certified sexpr (only if zero violations)
                 ▼
┌─────────────────────────────────────────────────────────────────┐
│ 3. RUN (substrate-forge + wasmtime + WASI sandbox)              │
│    Compile sexpr → WASM, instantiate in a Pod with declared     │
│    capabilities, execute against a frozen evaluation set.       │
│    Output: (output_sexpr_hash, trace_hash).                     │
└────────────────┬────────────────────────────────────────────────┘
                 │  output hash + execution trace
                 ▼
┌─────────────────────────────────────────────────────────────────┐
│ 4. COMPARE (eval-forge + policy-as-code)                        │
│    - Hash the output; compare to prior winner's hash            │
│    - Policy pattern-match: does the output satisfy quality      │
│      requirements? (better accuracy, lower latency, no new PII) │
│    - If winner: proceed to 5. If loser: deallocate (5 also).    │
└────────────────┬────────────────────────────────────────────────┘
                 │  decision
                 ▼
┌─────────────────────────────────────────────────────────────────┐
│ 5. COMMIT or DEALLOCATE                                         │
│    Winner: cache (sui-cache-eval stores the sexpr → output hash │
│    mapping), attest (tameshi Merkle-root over the full loop),   │
│    update pointer to the new canonical hash, terminate Pod.     │
│    Loser: terminate Pod, memory evaporates, hash IS still       │
│    recoverable from sui cache + sexpr if we ever want to        │
│    re-materialize it.                                           │
└────────────────┬────────────────────────────────────────────────┘
                 │
                 ▼
            Next candidate
```

**State at rest after N iterations:** one winning hash, all losing
candidates reclaimed. Memory is proportional to the number of
currently-live Pods, not to N.

---

## Why each actor is the right choice

### Lisp as candidate generator

- **Homoiconic** — programs ARE data. Generating a candidate is
  generating a sexpr value, not printing a Python string and parsing
  it back.
- **Macros at read/compile time** — a Lisp macro sees the source
  tree as a value. No tracing runtime. No AST capture layer.
- **REPL** — candidates can be inspected, forced, modified,
  re-evaluated without restarting the process.
- **Symbolic manipulation** — algebraic transformations on the
  candidate (substitute, factor, unroll) are a few lines of tree
  rewriting.

### Rust as axiom enforcer

- **Closed enums** — `UOp`, `ResourceOp`, `Pattern`, `Rule`. A user
  can only compose these; they can't invent a new primitive without
  a Rust PR and a proof obligation.
- **`#[non_exhaustive]`** — downstream backends must include `_ =>`
  wildcards, so adding an axiom doesn't break consumers silently.
- **Types make invalid states unrepresentable** — a `Graph::push`
  refuses a shape-mismatched Add; a Capability that's not a subset
  of host policy can't be instantiated; a canonical emission of a
  malformed sexpr doesn't exist because the sexpr can't be constructed.

### arch-synthesizer as composition prover

Already proves (from its 1,248 tests):
- Typescape coverage (19 AST domains, 12 morphisms, 11 controllers)
- AST-domain invariants (small primitive sets; all expressiveness
  is composition)
- Morphism totality + determinism
- Compliance-lattice composition (NIST ∩ CIS ∩ FedRAMP ∩ PCI ∩ SOC2)
- 30 repo types fully proven
- Cross-repo topology (dependency contracts)

These are the exact proofs you want over a candidate composition
before spending WASM execution time on it.

### substrate-forge as runtime verifier

- **WASM** is universal, sandboxed, portable; the candidate runs on
  any host (K8s Pod, browser, edge).
- **wasmtime** is native Rust, zero-overhead embedding; the Pod's
  memory IS the runtime.
- **WASI capability intersection** (built into substrate-forge's
  `Capability::is_subset_of`): the candidate's declared capabilities
  must be a subset of host policy. The sandbox enforces; the type
  system declares.
- **Fresh instantiation per run** — drop the Pod, drop the memory.
  No persistent mutable state between iterations. Determinism by
  construction.

### K8s + sui + tameshi as decision + deallocation layer

- **K8s** schedules Pods with declared CPU/GPU/memory, terminates
  on completion.
- **sui-cache-eval** is already content-addressed by BLAKE3 — the
  loop's winning hashes naturally become cache entries.
- **tameshi** Merkle-roots the attested sequence of loop iterations,
  producing a cryptographic audit trail. `sekiban` admission-gates
  the next iteration on the prior attestation.

---

## This is the shape of every DSL in the platform

The loop has different inputs at different layers, but the same
shape everywhere:

| Layer | Axiom set | Candidate | Prover | Runtime | Decision |
|-------|-----------|-----------|--------|---------|----------|
| **IaC** | `IacType` + `ResourceOp` | Lisp-proposed resource + transforms | arch-synthesizer + pangea-sim | pangea-forge rendering | `Backend` produces artifacts, tameshi attests |
| **ML** | `UOp` (ml-forge) | `(defmodel …)` expanding to `Graph` | arch-synthesizer over Graph | ml-forge backends → kernels; substrate-forge runs eval | Eval-hash compare, keep winner |
| **Compliance** | `Pattern` + `Rule` (iac-forge::policy) | Author writes `(policy …)` | policy::evaluate produces report | no runtime — pure verification | Pass / fail finding |
| **Remediation** | `ResourceOp` (iac-forge::transform) | LLM-proposed sexpr script | apply_proposal + invariants | sexpr-bounded transform applied | Outcome with edit list, attested |
| **Substrate execution** | `ProgramSource` (substrate-forge) | Any sexpr program | Capability + schema check | wasmtime sandbox | Output hash compared to expected |

**Every new DSL in the platform must fit this shape.** If a proposal
cannot be decomposed into (axioms, candidate generator, prover,
runtime, decision), the shape is wrong — revisit the design.

---

## What every module's CLAUDE.md should document

1. **Which axiom set does this module expose?** (what's the closed
   Rust enum?)
2. **Where does Lisp / sexpr compose over these axioms?** (what's
   the candidate-generation surface?)
3. **Who proves compositions involving this module?**
   (arch-synthesizer? A sibling pipeline? Something else?)
4. **Where does runtime verification happen?**
   (substrate-forge? A backend's render output? Direct cargo tests?)
5. **What's kept and what's deallocated?**
   (hashes persist; in-memory candidates evaporate)

Every crate in the platform should point to its place in this loop.
That's what "deep internalization" means.

---

## The terminal claim

The platform is not a collection of infrastructure tools + an ML
framework + a compliance engine + a substrate + attestation chain +
content-addressed storage + pure-Rust Nix. It's **one loop over
typed sexpr values with BLAKE3 identity, rendered across any backend
that implements the axiom set it depends on, proven before execution,
executed ephemerally, decided and cached by hash**.

Infrastructure, ML, compliance, remediation, substrate execution —
they're different axiom sets plugged into the same loop.

The loop terminates safely because:
- Axioms are closed; candidates cannot escape the Rust-defined grammar
- Proofs run at zero cost before any Pod is spawned
- Capabilities are declared + intersected; the sandbox enforces
- Losers deallocate fully; memory is proportional to live Pods

This is what "concentrate tooling in Rust memory" looks like when
it takes its type discipline seriously at every layer, including
the runtime boundary.

---

**Companions:**
- `PATTERNS.md` — the primitives this loop composes
- `ML_VISION.md` — the ML application of the loop
- `SUBSTRATE_VISION.md` — the substrate layer (step 3 + 5 of the loop)
