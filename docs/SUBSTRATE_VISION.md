# On-Demand Verifiable Program Substrate

> "I could build a service I just send a program request to and it
> builds the program in running memory, runs it, and completely
> deallocates it... with enough memory we can do the entire AI
> training lifecycle in memory — test a model, improve on it, delete
> the last one."

This document names a paradigm that emerges when every primitive from
this session is composed with Kubernetes + WASM/WASI. It's the logical
endpoint of **"types → invariants → proofs → render anywhere"** applied
not to static artifacts but to **live running memory**.

## The substrate

```
                    Client
                      │
                      │  (canonical sexpr of program request,
                      │   already content-hashed)
                      ▼
              ╔═════════════════╗
              ║ Gateway         ║  — verifies content hash, looks up
              ║ (Rust + sui)    ║    existing result in sui store cache
              ╚════════╤════════╝
                       │ cache miss
                       ▼
              ╔═════════════════╗
              ║ Scheduler       ║  — Kubernetes; finds a Pod with
              ║                 ║    sufficient memory + GPU
              ╚════════╤════════╝
                       │
                       ▼
              ╔═════════════════╗
              ║ Pod             ║  1. sui_eval materializes program
              ║ (Rust host      ║     from sexpr into in-memory WASM
              ║  + wasmtime     ║     module
              ║  + sui-eval)    ║  2. wasmtime executes under WASI
              ║                 ║     sandbox
              ║                 ║  3. Pipeline+Trace records every step
              ║                 ║  4. Content hash of output stored in
              ║                 ║     sui cache
              ║                 ║  5. Pod terminates; memory returned
              ║                 ║     to scheduler
              ╚════════╤════════╝
                       │
                       ▼
                    Result
                (content-hashed,
                 trace-audited,
                 WASM-verified)
```

**Nothing persists except hashes.** The program is a sexpr value with
a content hash. The output is a sexpr value with a content hash. The
*execution* is memory that existed for milliseconds between them.

## What each layer contributes

| Layer | Contribution |
|-------|--------------|
| **Rust** | Memory safety host; `wasmtime` integration; `sui_eval` in-process |
| **Lisp/sexpr** | Program notation; homoiconic — programs ARE data, no parse step |
| **sui** | Pure-functional evaluator of sexpr programs; content-addressed eval cache |
| **WASM/WASI** | Sandboxed execution target; portable (runs on any host — bare-metal, K8s, browser, edge) |
| **Kubernetes** | Orchestration: spawn Pod with declared resources, schedule on hardware, terminate |
| **ContentHash** | Identity of every program, input, and output |
| **ProvenMorphism** | Every transformation carries its invariants; composition preserves proof |
| **Pipeline + Trace** | Audit lineage of the run, attestable |
| **Fleet** | Composite hash for (program + runtime + hardware tier) closures |

Every layer is already in the platform. **The novel claim is that
their composition IS the substrate.**

## What this enables

### On-demand program materialization

Send: `(compute (matrix-inverse (matrix 3 3 [[1 2 3] [4 5 6] [7 8 10]])))`.
Gateway hashes, sui compiles to WASM, wasmtime runs, returns result,
Pod deallocates. Repeat with 10,000 concurrent requests — each one is a
brand-new memory materialization.

### The ML training lifecycle in memory

The killer application. Classic training persistence assumes:
- Checkpoints on disk
- Metadata in a database (W&B, MLflow)
- Weights as files
- Experiments as rows

In-memory lifecycle collapses all of these:

```
1. Train model A:
     (defpipeline train-run-1
       (validate-dataset ds-hash)
       (forward-pass  arch-hash ds-hash)
       (backward-pass ...)
       (optimizer-step ...)
       (evaluate  eval-set-hash))
     → Fleet A_hash = (arch-hash, weights-A-hash, eval-A-hash)

2. Propose improvement:
     (defpipeline improve
       (fine-tune A-hash new-data-hash)
       (quantize int8)
       (evaluate eval-set-hash))
     → Fleet B_hash

3. Compare in the same memory:
     (compare A_hash B_hash on eval-set-hash)
     → B wins on metric X, A wins on metric Y

4. Keep the winner, deallocate the loser:
     (keep B_hash, deallocate A_hash)
     → memory of A is returned to scheduler
     → B_hash is the only persistence

5. Iterate:
     (propose C from B, evaluate, keep winner, deallocate loser)
```

**The only persistent state is hashes.** Models exist in RAM for as
long as they're useful; when a successor wins, the predecessor's memory
evaporates. The hash remains — if in six months the improvement is
judged regression, `B_hash` can be re-materialized from its sexpr
definition + sui cache.

### Per-request compute isolation

A client's program runs in a WASI sandbox with declared capabilities.
`(request :allow-fs false :allow-net false :memory 8GB)` means the
program cannot escape. Sekiban-style admission gate refuses to spawn a
Pod if the program requests capabilities incompatible with its
provenance (e.g., a sexpr with a non-attested hash can't request
network).

### Verifiable reruns

Every completed run is (input_hash, program_hash, trace_hash,
output_hash). If the same input + program hash appears again, sui's
cache returns the cached output IMMEDIATELY. This isn't memoization as
optimization — it's **content-addressed pure functional evaluation of
the entire platform**.

### Runtime program mutation

Because sexpr programs ARE data, a running program can mutate itself
(in its sandbox) into a new program with a different hash, submit
THAT back to the scheduler, and continue. This is agent autonomy with
structural bounds — the agent can rewrite itself, but every rewrite is
a canonical sexpr with a hash and an attestation. A malicious rewrite
still has to match a ProvenMorphism's invariants or the scheduler
refuses to spawn it.

## Why Rust + Lisp specifically

- **Rust**: memory-safe host, native wasmtime embedding, zero-overhead
  sui-eval link. The Pod dies; Rust's drop semantics guarantee
  deallocation without leaks or GC pauses.
- **Lisp (sexpr)**: homoiconic — programs are data, no parse step
  between "program request" and "program value". A program mutation
  is a value transformation, not a string rewrite. Macros mean
  compile-time metaprogramming inside the sandbox.
- **WASM/WASI**: universal sandboxed execution. The same sexpr program
  compiles to WASM that runs on bare-metal Linux, in a browser, on an
  edge device, in a K8s Pod. Portability is a WASM property, verified
  by spec.
- **Kubernetes**: scheduler + resource quotas + Pod lifecycle +
  auto-scaling. The "substrate for anything" claim requires an
  orchestration layer that treats ephemeral compute as first-class.

None of the alternatives work in isolation:

| Without | Loses |
|---------|-------|
| Rust | Memory safety in host; WASM runtime embedding ergonomics |
| Lisp (sexpr) | Program-as-data, clean IPC format, homoiconic mutation |
| WASM | Universal sandboxed execution, portability across hosts |
| Kubernetes | Elastic compute, hardware-aware scheduling, pod lifecycle |
| Content hashing | Identity of programs, caching, verifiable rerun |
| Proven morphisms | Safety of program mutations, composition proofs |

## The ML Training Triad — fully in memory

This is the concrete realization of the user's observation. Three
pods, one shared sui store:

```
     ┌─── Training Pod (16 H100s, 500GB RAM) ────────┐
     │                                                 │
     │   (defpipeline train-step-N                    │
     │     :input  fleet-N-hash                        │
     │     :batch  data-batch-hash                     │
     │     :output fleet-(N+1)-hash)                  │
     │                                                 │
     │   Weights materialized in GPU memory.           │
     │   Pipeline+Trace records every step.            │
     │   fleet-(N+1)-hash stored in sui cache.         │
     │                                                 │
     └──────────────────┬──────────────────────────────┘
                        │  fleet-(N+1)-hash  (only)
                        ▼
     ┌─── Eval Pod (4 H100s, 200GB RAM) ───────────────┐
     │                                                 │
     │   (defpipeline eval                             │
     │     :fleet   fleet-(N+1)-hash                  │
     │     :dataset eval-set-hash                      │
     │     :output  eval-result-hash)                 │
     │                                                 │
     │   Materialize fleet-(N+1) from cache.           │
     │   Run against frozen eval sexpr.                │
     │   Return eval-result-hash (structured).         │
     │                                                 │
     └──────────────────┬──────────────────────────────┘
                        │  eval-result-hash
                        ▼
     ┌─── Decision Pod (2 CPUs, 16GB RAM) ─────────────┐
     │                                                 │
     │   (compare fleet-N-hash fleet-(N+1)-hash        │
     │            eval-result-hash prior-eval-hash)    │
     │                                                 │
     │   If N+1 wins: retain its hash, drop N's.       │
     │   If N wins:   drop N+1's hash.                 │
     │   Trigger next iteration.                       │
     │                                                 │
     └─────────────────────────────────────────────────┘
```

Between iterations **every pod terminates.** The state that survives
is one hash. The losing fleet's GPU memory is returned to the
scheduler. The winning fleet is cached by sui (possibly evicted later
under pressure; the cache is lossy but the sexpr definition is not —
any hash can be re-materialized from its definition + the training
seed).

With enough memory and GPUs, the entire search is live. With less
memory, the sui cache handles eviction and rematerialization
transparently.

## What's already built

Every primitive this depends on exists in pleme-io today:

- **sui** — pure-Rust Nix evaluator, sui-eval embeddable as a library
  (this session added `iac_forge::sui_transform` as proof)
- **iac-forge** — typed IR, canonical sexpr, BLAKE3 content hashing,
  ProvenMorphism composition, Pipeline+Trace, Fleet, content-addressed
  render cache, Nix backend with `emit_fod` (fixed-output derivations
  keyed on content hash)
- **tameshi / sekiban / kensa** — attestation + admission gating +
  compliance-as-data; already enforce hash-based policy at K8s layer
- **substrate** — Nix builder patterns for Rust services + Kubernetes
  workloads
- **convergence-controller** — clusters as Unix processes with PIDs;
  the scheduler side of the model already built
- **shinryu** — SQL over observability events; queries over training
  runs by content hash work today

What's missing is the **assembly**. No new crate; every piece composes.
The explicit new work:

1. A `substrate-forge` crate that compiles a sexpr program to a WASM
   module via wasmtime + sui. **~2 weeks.**
2. A Kubernetes operator that accepts a program sexpr as a CRD, spawns
   a Pod with declared resources, runs the WASM, collects the result,
   terminates. **~1 week.** (Builds on convergence-controller.)
3. Content-addressed storage gateway (sui-cache + object store) for
   sexpr-keyed artifacts. **~1 week.**
4. First ML pilot: single-GPU in-memory training cycle of a small
   transformer, with the three-pod triad above. **~2 weeks.**

**~6 weeks to a skeleton substrate.** Everything else builds on that.

## The closing claim

This is the ultimate expression of the pleme-io doctrine: **types
that make invalid states unrepresentable + proofs that compose + hashes
that identify anything + rendering that's a morphism + execution that
evaporates**. The substrate is what you get when you stop treating
memory as persistent and start treating it as the reified form of a
canonical sexpr identity.

It's programmable, verifiable, portable, ephemeral, and provably
correct. **It's what a computing platform looks like when "types are
what's real" is taken seriously at every layer.**

---

**Companions:**
- `docs/PATTERNS.md` — the synthesis of what was built this session
- `docs/ML_VISION.md` — how these primitives apply to ML specifically

This document generalizes beyond ML: any computation that can be
expressed as a typed sexpr is a candidate. ML is the highest-leverage
first application because its pain is the most acute and its benefits
compound (reproducibility + compliance + cost). But the substrate
isn't an ML platform — **the substrate is a computing platform, and
ML is one workload.**
