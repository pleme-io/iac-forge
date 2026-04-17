# A Rust + Lisp Machine Learning Platform — Synthesis

> "ML frameworks were accidentally five things: graph, autograd, checkpoint, serialization, distributed, registry. Every one of them collapses to: typed IR + canonical s-expression + BLAKE3 + proven morphisms + frozen cross-language vectors."

This document synthesizes the session's findings — the primitives built
in `iac-forge` + its surrounding repos — against the current state of
ML/AI engineering. The claim it makes is strong: the same typed-core +
proven-composition approach that lets `iac-forge` render one IR to
eight backend representations is the missing foundation for ML, and it
composes with the pleme-io convergence-computing model already in
place.

---

## Part 1 — The field today, in pieces

### Rust ML has every primitive separately

From the ecosystem research:

- **Burn** (tracel-ai): `Backend` trait abstracting over ndarray /
  candle / tch / wgpu / CUDA / Metal. Graph-level optimizer
  (`burn-jit`/`cubecl`), tape-based autograd, type-level ranks via
  const generics. Proves ranks and backend compatibility. Doesn't
  prove shape sizes or broadcasting correctness.
- **Candle** (HuggingFace): eager execution, fully dynamic shapes,
  safetensors support, no graph IR. Ad-hoc `Result<Tensor>` everywhere.
- **dfdx** (Corey Lowman): full shape at compile time via const
  generics. Proves ranks + sizes + op compatibility. Collapses on
  dynamic shapes (variable seq_len) — the cautionary tale that rigid
  types alone aren't enough.
- **tract** (Sonos): production ONNX/TF inference with `TDim` — symbolic
  dimensions as affine expressions over named variables. The strongest
  shape-algebra invariants in any Rust framework.
- **Luminal** / **ratchet** / **zyx**: small kernel IRs (~12 primitives)
  in the tinygrad lineage.

### ML compilers have the IR ideas

- **MLIR** (LLVM): multi-level typed IR with composable dialects
  (`linalg`, `tensor`, `tosa`, `stablehlo`, `affine`, `gpu`, `llvm`).
  Each lowering pass IS a morphism between dialects. This is literally
  the morphism graph idea applied to tensors.
- **StableHLO** (OpenXLA): ~100 versioned ops with a **formal spec +
  reference interpreter + compatibility guarantees**. The most
  carefully designed tensor IR currently shipping.
- **tinygrad**: 12 primitive `UOps`. Every framework op lowers to them.
  Proves a *small* IR is sufficient — contra ONNX's ~180 versioned op
  schemas with escape-hatch custom ops.

### Type systems for ML exist in research

- **Dex Lang** (Google/ICFP 2021, Paszke et al.): true dependent types
  for tensors. `Fin n` as first-class index types, named axes,
  shape-correctness is a type error at compile time.
- **Futhark** (DIKU): size-typed purely-functional GPU language.
- **Named tensors**: axes named (`batch`, `channels`, `height`) so
  broadcasting becomes unambiguous.
- **Effect systems**: largely absent. Research langs like Koka could
  model `Rng`, `Mutation`, `Checkpoint`, `Gradient` as algebraic
  effects.

### Content addressing is half-done

- **safetensors** (HF): zero-copy mmap, JSON header, raw bytes. Clean
  and widely adopted — but not internally content-addressed. File
  hashes change when a layer is renamed even if weights are identical.
- **ONNX**: protobuf, no canonical form, bit-identical models hash
  differently.
- **GGUF** (llama.cpp): custom binary + typed KV metadata + 30+
  quantization types. No content addressing; files identified by URL.
  Extensive stringly-typed metadata (`general.architecture` as string).
- **HF Hub + git-lfs + xet**: file-level SHA256, not structural hashing.
- **Reproducibility crisis**: Pineau et al. (JMLR 2021), Gundersen &
  Kjensmo (AAAI 2018), Hutson (*Science* 2018) document ~25–30%
  reproduction rates. Content-addressing the *entire provenance graph*
  (code + hardware + data + RNG + DAG) is the unsolved problem.

### Lisp used to be AI's language

The MIT AI Lab era (1960s–90s) ran on Symbolics Lisp Machines.
Connection Machines used StarLisp. Cyc, Soar, ACT-R, SHRDLU were all
Lisp. Common Lisp ML (Neanderthal, MGL by Gábor Melis who won Kaggle
competitions with it) still exists but has vanishingly small mass.

Lisp lost not because of matmul speed — BLAS is BLAS — but because
NumPy (2006) + SciPy + Matplotlib + Jupyter reached critical ecosystem
mass, Lisp Machines were $100K+ when commodity x86 existed, and AI
Winter bankrupted the hardware vendors. What was **lost**:
homoiconicity (code IS data, graphs aren't special), REPL-driven
development with live model redefinition, condition/restart error
handling strictly more powerful than try/except, symbolic diff in ~200
lines (scmutils vs ~3000 lines of autograd), image-based development.

**JAX's `jaxpr` printer output is literally a Lisp.** PyTorch's
`fx.GraphModule` is reinvented s-expressions in a less powerful form.
DreamCoder's learned abstraction library IS s-expressions. The field
keeps reinventing Lisp badly.

---

## Part 2 — The production pain catalog

From the pain-points research. Every item below is *someone treating
a structured value as bytes* OR *someone running a transformation
without a proof*. Each has our primitive that replaces it.

### Training pipeline failures

- **Silent numeric divergence at scale.** Meta OPT-175B logbook (arXiv
  2205.01068) documented 35+ manual restarts from loss spikes / NaN
  gradients over 3 months. PaLM paper notes ~20 spike-and-skip events.
  Root cause rarely captured — eyeball tensorboard, git-blame configs.
- **Reproducibility crisis.** ~25–30% papers reproduce within 10% of
  reported metrics. CUDA `atomicAdd` non-determinism, NCCL collective
  ordering, mixed-precision rounding all compound.
- **Dataset versioning pain.** DVC hashes files; CSV row reorderings
  and schema-equivalent variants hash differently. LAION-5B retraction
  (2023) exposed that downstream models had no cryptographic proof of
  which snapshot they trained on.
- **Hyperparameter tracking.** W&B captures scalars, can't diff
  configs structurally. Sweep resumes after preemption silently use
  stale configs.

### Model serving + safety

- **Drift detection.** Zillow Offers (2021) $881M writedown partly
  attributed to undetected model drift.
- **Attestation.** "This inference was produced by model X" is an HTTP
  header; trivially forgeable.
- **Canary + rollback.** Seldon / Argo Rollouts split traffic by label,
  no invariant gate. Canary with higher latency and lower accuracy
  still promotes if error rate is within SLO.
- **Prompt injection + tool-use safety.** No framework (LangChain,
  LlamaIndex, Assistants) treats tool-call args as adversarial data.
  Agent loops blow up context and retry indefinitely on ambiguous
  errors.
- **Hallucination attestation.** RAG frameworks attach citations as
  strings the model *generated*, not verified provenance.

### Compliance / regulation

- EU AI Act Articles 10 (data governance), 12 (logging), 17 (QMS)
  demand dataset lineage + training-run logs + lifecycle risk
  management — currently delivered as hand-written PDFs.
- NIST AI RMF (GOVERN / MAP / MEASURE / MANAGE) same story.
- Model cards (Mitchell et al., FAccT 2019) drift from reality
  because they're hand-authored.
- Fine-tuning audit: OpenAI's API gives a job ID, not a proof. "Prove
  this model never saw EU-citizen data after X date" has no answer.

### Interchange nightmares

- **ONNX opset hell.** PyTorch → ONNX → TensorRT routinely fails
  post-opset-17. HuggingFace `optimum` has a 1000+ line compatibility
  matrix.
- **Quantization format sprawl.** GGUF, GPTQ, AWQ, BitsAndBytes,
  SmoothQuant, AQLM — each with its own metadata schema, none
  interchangeable.
- **Tokenizer fragmentation.** SentencePiece vs BPE vs Tiktoken vs
  HuggingFace tokenizers all produce different ids for the same string;
  Llama has 3+ "official" tokenizer variants with subtle differences.
  5–15% quality degradation from the wrong tokenizer silently.

### Distributed training

- Checkpoint format incompatibility: `torch.save` vs DeepSpeed ZeRO-3
  vs FSDP vs Megatron-LM. Resharding 64→32 GPUs is ~12 hours of
  bespoke engineering per event (Meta OPT logbook).

---

## Part 3 — How our primitives map, one-to-one

| Primitive (this session) | Replaces | Concrete ML flow |
|--------------------------|----------|------------------|
| `ContentHash` over canonical SExpr | MLflow `run_id`, DVC file-hash, file-level SHA on safetensors | Stable identity for **(model + data + config + hardware + RNG + DAG)** closure surviving serialization |
| `ProvenMorphism` | `torch.save`, PyTorch→ONNX converters, GGUF packers | Typed, invariant-gated transformations with named composition and violation traceability |
| `Pipeline` with promotions / mutations + `Trace` | W&B sweep logs, notebook lineage, `torch.distributed.elastic` restart logic | Audit trail **by construction**; EU AI Act Article 12 satisfied structurally. Training step is a mutation; `grad`, reshard, quantize are promotions |
| `Policy` / `Pattern` / `Rule` over sexpr | OPA on YAML, manual model-card review | Compliance rules as data: `(forbid (and (after "2024-03-01") (contains-eu-pii ?dataset)))` is a structural check, not a string regex |
| `Fleet` (BTreeMap with composite hash) | Model-URI string, Docker tag, HF repo id | Attestable "this inference used exactly THIS closure": `(weights_hash, tokenizer_hash, preprocess_morphism_hash, postprocess_morphism_hash)` → one hash |
| `Remediation` harness (Proposal + Outcome + invariants) | LangChain `max_iterations=10`, Argo Rollouts, OpenAI fine-tune `job_id` | Bounded agent loops with invariant gates. Canary promotions as proven morphisms that block if latency/accuracy/PII invariants don't hold |
| Cross-language frozen vectors (Rust ↔ Ruby ↔ Nix) | Pickle across versions, ONNX opset mismatch | Rust ↔ Python ↔ TypeScript ↔ Go model-graph interchange without serialization drift |
| `emit_fod` (IR as Nix fixed-output derivation) | Ad-hoc model storage paths | `/nix/store/<model-hash>-<name>` IS the attestation. Nix itself refuses to rebuild unchanged graphs |
| `sui_transform` (pure-Rust Nix in-process) | Shell-out to `python train.py` with env munging | In-process pure-functional transforms over models; JAX-style purity without the Python |

The pattern: every ML pain above is one of two disease-shapes, and our
two core primitives (content-addressed canonical form, proven
morphism) are the two cures.

---

## Part 4 — The platform design

### Layer 1 — `ml-forge` (core)

The `iac-forge` of ML. Typed IR for tensor computation.

- ~15 primitive `UOp`s (tinygrad-small), everything lowers here
- `TensorType` as Rust type: `rank` (const generic) + `dtype` (enum) +
  `device` (enum) + `shape` (vector of `Dim` — **symbolic** in the
  tract `TDim` style, affine expressions over named size variables
  like `batch`, `seq_len`, `d_model`)
- Operations as `IacResource`-shaped `TensorOp` values:
  `{ name, inputs, outputs, kernel, attrs }`
- Graph as a `BTreeMap<NodeId, TensorOp>` — content-hashable exactly
  like `Fleet` already is
- Symbolic shape checker via Presburger arithmetic (proven sound
  against a reference solver)

### Layer 2 — `ml-forge::dialect` (MLIR-style)

Dialect composition, same pattern as `iac_forge::ir` + backend crates.
Each dialect is a typed algebra; lowerings are `ProvenMorphism`s
between dialects.

- `AutogradDialect` — forward/backward annotations; `grad` operator
- `TransformerDialect` — attention, layer norm, MLP as first-class ops
  with shape algebra
- `QuantDialect` — quantization schemes (int8, int4, FP8) as morphisms
  with numerical-equivalence invariants (within declared tolerance)
- `KernelDialect` — hand-written kernels (`flash_attention_v2`,
  `paged_attention`) as opaque leaves with shape + dtype contracts

### Layer 3 — autograd as source-to-source morphism

**Not tracing.** A `ProvenMorphism<Graph, Graph>` named `Grad` takes a
typed IR graph and returns its transposed derivative graph. This is
what `D` in `scmutils` does in ~200 lines; what JAX approximates but
can't fully achieve because it has to trace Python; what every other
Rust framework reimplements from scratch.

Invariants enforced:
- Linear in the cotangent
- Shape-preserving (output shape of `grad(f)` matches input shape of
  `f`)
- Deterministic (same IR → same gradient graph, byte-identical)

Because `Grad` is a morphism, its **gradient graph has a BLAKE3 hash**.
Cross-language frozen vectors guarantee: the Rust-computed gradient
for `(matmul (param :W) (input :x))` hashes to the exact value the
Lisp reference implementation produces. If anything drifts, it's
caught.

### Layer 4 — `ml-forge::backend` — renderings

Seven backends, one IR. Pattern identical to `iac-forge`:

| Backend | Renders to | Quality |
|---------|-----------|---------|
| `WgpuBackend` | WGSL + wgpu runtime | portable GPU |
| `CudaBackend` | CUDA C++ | NVIDIA perf |
| `MetalBackend` | MSL | Apple |
| `StableHLOBackend` | stablehlo textual | XLA ecosystem access |
| `MLIRBackend` | MLIR upstream | LLVM integration |
| `CandleBackend` | candle calls | HuggingFace ecosystem |
| `GGUFBackend` | GGUF bytes | llama.cpp |
| `SafetensorsBackend` | safetensors bytes | weights interchange |

Via the blanket `Backend → ProvenMorphism` impl, every backend
automatically proves determinism, non-empty output, unique paths.
Adding a backend inherits all existing proofs.

### Layer 5 — Lisp surface: `model-forge`

Homoiconic model description. `(defmodel transformer :layers 12
:d_model 768 ...)` is a Lisp macro expanding to typed IR. Because the
IR's canonical sexpr IS the Lisp, there's no serialization layer —
the `.rkt` / `.lsp` file **is** the model.

```lisp
(defmodel gpt-small
  :params (d_model 768) (n_heads 12) (n_layers 12) (vocab 50257)
  :input  (token-ids :shape (Tensor I64 (batch seq))))
  :output (logits :shape (Tensor F32 (batch seq vocab)))
  :body (|>
    (token-embed :input token-ids :weight :W_e)
    (position-embed :weight :W_p)
    (repeat n_layers transformer-block)
    (layer-norm)
    (linear :weight :W_lm :out vocab)))
```

Macros (not decorators — compile-time, source-to-source) handle:
- Weight declaration + Fleet composition
- Auto-fuse opportunities visible at macro time
- Source-level AD
- Quantization as a whole-program transform

### Layer 6 — training as pipeline

```rust
let stages = vec![
    Stage::new("validate-dataset", StageKind::Promotion, dataset_validate)
        .establishes(Quality::new("validated"))
        .establishes(Quality::new("no-pii-post-cutoff")),
    Stage::new("forward",  StageKind::Promotion, forward_pass)
        .requires(Quality::new("validated")),
    Stage::new("backward", StageKind::Promotion, grad_pass),
    Stage::new("optimizer-step", StageKind::Mutation, optimizer_update)
        .establishes(Quality::new("no-nan"))
        .establishes(Quality::new("grad-norm-bounded")),
    Stage::new("checkpoint", StageKind::Promotion, content_address_weights)
        .requires(Quality::new("no-nan"))
        .establishes(Quality::new("attested")),
];
let (final_weights, trace) = run_pipeline(initial, stages, &[])?;
// trace IS the audit log. EU AI Act Article 12: done.
```

If `no-nan` fails at step 3 of 1,000,000, the pipeline stops with the
partial trace — the exact preimage hash of the divergent step.
Training divergence becomes a gated fault, not postmortem archaeology.

### Layer 7 — distributed training

Already built into the pleme-io platform. Each worker computes a
morphism; output hash + input data hash fully identify its
contribution. Parameter servers are **sui-addressable Attic-style
content stores**. Allreduce is `(reduce sum worker-hashes)` over a
**proven commutative monoid** — you can't get wrong gradient
aggregation because the monoid law is a compile-time check.

### Layer 8 — serving

Canary = serve two `Fleet` hashes in parallel, compare on frozen test
vectors. Rollback = pointer flip to previous composite hash. A/B = a
routing `ProvenMorphism` that dispatches by hash. **No model registry
product** — Nix store semantics + `Fleet` hashing are the registry.

Sekiban-style K8s admission gate refuses to serve if the loaded
Fleet's hash ≠ the declared Fleet hash. That gate is already shipped
for IaC — extending it to models is a schema addition, not a new
system.

### Layer 9 — compliance (kensa for AI)

Policy-as-code over the training-config sexpr. EU AI Act Article 10's
"relevant design choices" become pattern matches:

```lisp
(policy :id "eu-act-10-relevance"
  :pattern (struct-form training-config
             (:dataset ?ds) (:model ?m))
  :rule    (forbid (contains-eu-pii ?ds
                    (before (metadata ?m :deployment-date)))))
```

Model cards are **generated from the Fleet hash chain**, not authored.
Drift between the card and reality is structurally impossible because
the card IS a projection of the canonical form.

### Layer 10 — agent safety

Tool calls as typed IR with invariants. Agent loops as
`Remediation::Proposal`s with bounded step count and invariant gates.
Hallucination-attested RAG: the citation is a content-hash pointer
into the retrieved-docs Fleet, not a string.

---

## Part 5 — Integration with the existing platform

This isn't a new platform. It's the existing platform's primitives
applied to ML. Every existing piece has a role:

| Existing | ML application |
|----------|----------------|
| **substrate** | Nix builds for ML training environments (pinned CUDA, NCCL, Python, weights as fixed-output derivations) |
| **tameshi** | Attestation of *models*, not just IaC. `MasterSignature` over a Fleet hash. |
| **sekiban** | K8s admission webhook gates serving on Fleet-hash match |
| **kensa** | AI-specific compliance baselines (EU AI Act, NIST AI RMF) as data, same engine |
| **shinryu** | Training observability. SQL over training `events` table. `SELECT * FROM events WHERE signal='train_step' AND grad_norm > 100.0` |
| **convergence-controller** | Distributed-training cluster PIDs. Each cluster a convergence process |
| **sui** | In-process Nix for training DSLs. Full Rust memory, JAX-level purity |
| **pangea-sim** | Simulate training at zero cost — proptest over 10,000+ random configs for numerical stability |
| **pangea-architectures** | Compose training infrastructure (GPU cluster + data pipeline + checkpoint store) as layered architectures |

The platform's convergence-computing doctrine already applies to
infrastructure. Applying it to models is a direct extension: model =
typed declaration → simulate in-process (at low fidelity) → prove
invariants (shape, numeric stability) → remediate (gradient clipping,
weight decay) → render (to GGUF, safetensors, Nix store path) →
deploy (via sekiban gate) → verify (canary vs frozen eval set) →
reconverge (next training step).

The eight convergence phases map to ML unchanged. We've been building
ML infrastructure the whole time without calling it that.

---

## Part 6 — Concrete crate structure

Proposed workspace:

```
ml-forge/                 -- typed IR core, primitive UOps, shape algebra
  ├── ml-forge-core       (TensorType, UOp, Graph, content-hash)
  ├── ml-forge-autograd   (Grad as ProvenMorphism, source-to-source AD)
  ├── ml-forge-dialects   (AutogradDialect, TransformerDialect,
  │                        QuantDialect, KernelDialect)
  └── ml-forge-sexpr      (ToSExpr / FromSExpr + frozen vectors)

model-forge/              -- Lisp-surface DSL expanding to typed IR
  └── (defmodel macros, shape polymorphism, layer library)

train-forge/              -- training pipeline + policy gates
  ├── train-forge-loop    (Stage / Pipeline adapters for training)
  ├── train-forge-optim   (SGD / Adam / AdamW as ProvenMorphisms)
  └── train-forge-dist    (distributed primitives over sui)

eval-forge/               -- evaluation as policy + canonical test sets
serve-forge/              -- inference server with Fleet-hash admission
weights-forge/            -- stable-safetensors (safetensors + BLAKE3)
dataset-forge/            -- canonical dataset IR + provenance chain

ml-forge-wgpu/            -- backend crates (one per target)
ml-forge-cuda/
ml-forge-metal/
ml-forge-stablehlo/
ml-forge-candle/
ml-forge-gguf/
ml-forge-safetensors/

ml-forge-lisp/            -- Racket/Scheme reference implementation of
                             canonical sexpr + BLAKE3 + gradient semantics
                             (third language in the portability club)
```

Each crate follows the `iac-forge` discipline: types → invariants →
proofs → render anywhere. Adding a backend is a `Backend` impl; it
inherits all existing proofs via the blanket `ProvenMorphism`.

---

## Part 7 — Migration path (shortest fuse, biggest fire)

Don't boil the ocean. In order of leverage:

1. **`ml-forge-core`** — typed IR + UOps + `TensorType` with symbolic
   shapes (tract-style TDim). **One week.** Port one transformer's
   forward pass into it as a smoke test.

2. **`ml-forge-sexpr`** — canonical emission + BLAKE3 + frozen vectors
   for the UOps. **Two days.** Cross-language agreement with Ruby + Nix
   references is automatic once the canonical form is pinned.

3. **`ml-forge-autograd`** — `Grad` as a `ProvenMorphism<Graph,
   Graph>`. **One to two weeks.** Validate against PyTorch on ~100
   representative ops with frozen input vectors. Gradient hashes
   become part of the cross-language contract.

4. **`ml-forge-wgpu`** — first backend. **One week.** Rendering via the
   existing `Backend` trait pattern; inherits all invariants.

5. **`weights-forge`** — stable-safetensors + BLAKE3 header + typed
   metadata. **Three days.** Content-addressed weights are usable
   immediately with existing HF models via a conversion morphism.

6. **`train-forge-loop` MVP** — Pipeline with promotions/mutations +
   Trace for a training step. **One week.** Even without distributed
   primitives, audit-complete single-GPU training is immediately
   better than anything shipping.

7. **`sekiban`-for-models** — extend the existing admission webhook to
   gate model-serving pods on Fleet-hash match. **One week.** Biggest
   compliance win per LOC of any step.

8. **`model-forge` Lisp surface** — Racket or Scheme frontend. **Two
   weeks.** Adds ergonomics; the IR works without it but the Lisp
   layer is where Macros / REPL / symbolic diff pay off.

9. **Everything else** — cross-backend via `Backend` additions,
   `sui_transform` for training DSLs, kensa for AI compliance.

First six items = ~6 weeks for a skeleton platform that already
solves reproducibility + canonical interchange + content-addressed
weights. That's further than most commercial platforms get in a year.

---

## Part 8 — The closing claim

PyTorch, JAX, TensorFlow, and their Rust derivatives (burn, candle,
tch) are Python products with Rust ports. They reproduce Python's
model — mutable tensors, eager tracing, opaque graph construction,
format-of-the-week serialization — with better performance and the
same structural limitations.

Our platform is different not because it's Rust, and not because it's
Lisp. It's different because **it's the first ML platform built on
the observation that every ML framework primitive (graph, autograd,
checkpoint, serialization, distributed, registry) collapses to a
smaller proven core:**

```
graph         = typed IR
autograd      = morphism on IR
checkpoint    = content hash
serialization = canonical sexpr
distributed   = hash-addressed monoid reduction
registry      = Nix store
```

Every pain point in production ML is either an implementation of one
of these concepts that loses structure (protobuf ONNX, pickled
PyTorch) or a transformation without a proof (PyTorch→ONNX converters,
quantization scripts, agent loops). Our primitives fix both classes
simultaneously. That's the leverage — and it's already proven in
infrastructure.

**The platform's convergence-computing doctrine doesn't change.
The artifact type does.**

---

## Further reading (from this session's research)

**ML typed IR:**
- Lattner et al., "MLIR: Scaling Compiler Infrastructure for Domain
  Specific Computation," CGO 2021, arXiv 2002.11054
- Paszke et al., "Getting to the Point" (Dex), ICFP 2021, DOI
  10.1145/3473593
- Kwon et al., "Efficient Memory Management for Large Language Model
  Serving with PagedAttention," SOSP 2023, arXiv 2309.06180

**Reproducibility:**
- Pineau et al., "Improving Reproducibility in Machine Learning
  Research," JMLR 2021
- Hutson, "Artificial intelligence faces reproducibility crisis,"
  *Science* 2018
- Gundersen & Kjensmo, "State of the Art: Reproducibility in Artificial
  Intelligence," AAAI 2018

**Distributed training:**
- Sergeev & Del Balso, "Horovod: fast and easy distributed deep
  learning in TensorFlow," arXiv 1802.05799
- Zhang et al., "OPT: Open Pre-trained Transformer Language Models,"
  arXiv 2205.01068 (the logbook)

**Historical Lisp / symbolic:**
- McCarthy, "Recursive Functions of Symbolic Expressions," CACM 1960
- Abelson & Sussman, *SICP*
- Sussman & Wisdom, *Structure and Interpretation of Classical
  Mechanics* (scmutils — symbolic diff in ~200 lines)
- Steele, "Growing a Language," OOPSLA 1998

**Program synthesis:**
- Ellis et al., "DreamCoder," PLDI 2021, arXiv 2006.08381
- Solar-Lezama, *Program Synthesis by Sketching*, PhD 2008

**Compilers:**
- Ragan-Kelley et al., "Halide," SIGGRAPH 2012
- Chen et al., "TVM," OSDI 2018, arXiv 1802.04799
- Henriksen, Futhark PhD thesis (DIKU)

**Compliance + safety:**
- Mitchell et al., "Model Cards for Model Reporting," FAccT 2019
- EU AI Act, Articles 10 / 12 / 17
- NIST AI RMF 1.0 (2023)
- Willison's prompt-injection series (2022-2024)

---

**Companion document:** `docs/PATTERNS.md` — the synthesis of what was
built in iac-forge this session. Every primitive cited here is
implemented, tested, and committed.
