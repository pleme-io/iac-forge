# iac-forge

Platform-independent IaC code generation core library. Defines the IR types,
Backend trait, resolver, and shared test fixtures that all `*-forge` backends consume.
The IR is the shared language between two input paths and seven output backends.

## Architecture

Two input paths produce the same IR:

```
Path 1: TOML + OpenAPI (iac-forge-cli)
  ResourceSpec (TOML) + openapi_forge::Spec
       │
       ▼  resolve_resource() / resolve_data_source()
       IacResource / IacDataSource (platform-neutral IR)

Path 2: Terraform Schema (terraform-schema-importer)
  terraform providers schema -json
       │
       ▼  convert_resource()
       IacResource (same IR)

Both paths feed into:
  IacResource / IacDataSource
       │
       ▼  Backend::generate_resource() / generate_all()
  GeneratedArtifact { path, content, kind, source_hash, morphism_chain }
```

Path 1 is used for providers with OpenAPI specs (Akeyless, Datadog, Splunk).
Path 2 is used for all Terraform-native providers (AWS, Azure, GCP, Cloudflare,
and 21 others). Both produce identical IacResource IR that backends consume.

## Module Map

| Module | Purpose |
|--------|---------|
| `ir` | `IacType`, `IacAttribute`, `IacResource`, `IacDataSource`, `IacProvider` |
| `backend` | `Backend` trait, `GeneratedArtifact` (with provenance fields), `ArtifactKind` |
| `resolve` | spec + OpenAPI → IR |
| `spec` | TOML spec types, `ConfigLoader` trait |
| `naming` | case conversion helpers (reuses `meimei`) |
| `type_map` | OpenAPI / takumi types → `IacType` |
| `morphism` | `Morphism`, `ProvenMorphism`, `Composed`, `Identity`, `ResourceInput` |
| `transform` | `Transform<T>`, `ResourceOp`, s-expr `script::parse` |
| `sexpr` | `SExpr`, `ContentHash`, `ToSExpr` / `FromSExpr`, canonical emission |
| `sexpr_ir` | `ToSExpr`/`FromSExpr` impls for the IR types (private) |
| `sexpr_diff` | `Edit`, `diff(old, new)` over sexpr trees |
| `remediation` | `Proposal`, `Outcome`, `apply_proposal`, `outcome_sexpr` |
| `render_cache` | `RenderCache` keyed on (schema-version, platform, content-hash) |
| `fleet` | `Fleet`: named collection of `IacResource` with composite hash |
| `policy` | `Pattern`, `Rule`, `Policy`, `evaluate` — compliance-as-data |
| `testing` | shared fixtures + `testing::fixtures` (sexpr save/load) |

## Key Types

- **`IacType`** — `String | Integer | Float | Numeric | Boolean | List | Set | Map | Object | Enum | Any` (Serialize/Deserialize/Eq/Hash, `#[non_exhaustive]`)
- **`IacAttribute`** — resolved field with `required`, `optional`, `computed`, `sensitive`, `immutable`, `update_only`, `read_path`, `json_encoded`
- **`IacResource`** — resolved resource with attributes, CRUD info, identity
- **`IacDataSource`** — resolved data source with attributes
- **`IacProvider`** — provider config with auth, skip_fields, platform_config
- **`Fleet`** — `BTreeMap<String, IacResource>` with a name and composite content hash
- **`GeneratedArtifact`** — `{ path, content, kind, source_hash, morphism_chain }`
- **`ArtifactKind`** — `Resource | DataSource | Provider | Test | Schema | Signature | Module | Metadata` (`#[non_exhaustive]`)
- **`SExpr`** — canonical s-expression value (Symbol, String, Integer, Float, Bool, Nil, List)
- **`ContentHash`** — BLAKE3 over canonical emission; 64-char lowercase hex via `Display`

## Key Traits

- **`Backend`** — `generate_resource()`, `generate_data_source()`, `generate_provider()`, `generate_test()`, `validate_resource()`, `generate_all()`
- **`NamingConvention`** — `resource_type_name()`, `data_source_type_name()`, `file_name()`, `field_name()`
- **`ConfigLoader`** — `load(path)`, `from_toml(str)` for spec types
- **`Morphism<Src, Dst>`** — `name()`, `apply(&Src) -> Dst` (total, deterministic)
- **`ProvenMorphism<Src, Dst>`** — extends `Morphism` with `check_invariants(src, dst) -> Vec<String>`
- **`Transform<T>`** — endomorphism `apply(T) -> T` (Identity, ComposeTransforms, `Vec<ResourceOp>`)
- **`ToSExpr`** — canonical serialization; blanket `content_hash()` via BLAKE3 over emission
- **`FromSExpr`** — inverse of `ToSExpr`; every impl satisfies `T::from_sexpr(&x.to_sexpr())? == x`

## Blanket Backend → ProvenMorphism

Every `Backend` automatically implements
`ProvenMorphism<ResourceInput, Vec<GeneratedArtifact>>`. `Morphism::apply`:
- calls `generate_resource` through `Backend`
- populates each artifact's `source_hash` (= IR content hash) and `morphism_chain`
  (= `[Backend::<platform>, "generate_resource"]`) when the backend didn't set them

The four invariants `check_invariants` enforces on any backend output:
1. Determinism — re-running apply produces identical artifacts (including provenance)
2. Non-empty artifact list
3. No empty paths or contents
4. No duplicate paths within a single render

Consumer backends (`pangea-forge`, `terraform-forge`, `pulumi-forge`,
`crossplane-forge`, `ansible-forge`, `steampipe-forge`, `helm-forge`) get proof-
bearing composition for free — the proofs attach to the trait, not to each
per-backend test file.

## Canonical S-Expression Interchange

Every `ToSExpr` type has a portable canonical text form and a BLAKE3 content
hash over that form. Format rules:

- Unit enum variants: bare kebab-case symbol (`IacType::String` → `string`)
- Tuple enum variants: `(tag arg1 arg2 …)`
- Struct variants / structs: `(name (:field val) …)`
- `Vec<T>`: `(list item1 item2 …)`; empty = `(list)`
- `Option<T>`: `nil` for None, value for Some
- Strings: double-quoted with `\n`, `\t`, `\"`, `\\` escapes
- Integers vs Floats distinguished at parse (presence of `.` or `e`)
- Bool: `true` / `false` symbols

Embedded formats:
- `serde_json::Value` → `(json-value "<encoded>")`
- `BTreeMap<String, toml::Value>` → `(toml-map "<encoded>")`

Round-trip law (proven by proptest, 256 cases/property):
```
T::from_sexpr(&x.to_sexpr())? == x
T::from_sexpr(&SExpr::parse(&x.to_sexpr().emit())?)? == x
x.to_sexpr().emit() == x.to_sexpr().emit()
```

## Content Addressing

`x.content_hash()` returns a `ContentHash` (BLAKE3 over canonical emission).
Available on any `ToSExpr` implementor via blanket method. Properties (proven):
- Structurally-equal values produce equal hashes
- Distinct values (overwhelmingly) produce distinct hashes
- Hash survives parse → emit → parse round trip
- Hex form is 64 chars, lowercase, via `Display`

The ContentHash is the foundation for:
- `RenderCache` keys
- `GeneratedArtifact::source_hash`
- `Fleet::member_hash` and per-fleet hashing
- `Outcome::before_hash` / `after_hash` in remediation
- Cross-language attestation (see "Portability")

## Render Cache

`render_cache::RenderCache` memoizes backend rendering by content hash:

```rust
let mut cache = RenderCache::new();
let artifacts = cache.render(&backend, &input);   // miss: invokes backend
let artifacts2 = cache.render(&backend, &input);  // hit: hashmap lookup
assert_eq!(cache.stats().hits, 1);
```

Three-part key: `(SCHEMA_VERSION, backend.platform(), ir.content_hash())`.
Provenance is preserved on hit — the cached artifacts keep the `source_hash`
and `morphism_chain` the Morphism apply populated on the first call.

Bump `SCHEMA_VERSION` when the Backend trait's output contract changes in a
way that invalidates cached output.

## Transforms

`iac_forge::transform` provides a bounded endomorphism DSL for user-extensible
IR edits. Atoms in `transform::ops::ResourceOp`:

- `SetDescription(s)` / `SetCategory(s)` — metadata edits
- `MarkSensitive(name)` — flip the sensitive bit on an attribute
- `AddOptionalString { canonical_name, api_name, description }` — idempotent
  append of a new optional String attribute
- `RemoveAttribute(name)` — drop by canonical name

Sequences: `impl Transform<IacResource> for Vec<ResourceOp>`. Composition via
`ComposeTransforms(a, b)`. Identity unit via `Identity`.

s-expression script surface (`transform::script::parse`):

```lisp
; PCI-DSS §3.4 — mark cardholder-data fields sensitive
(set-description "pci-3.4 remediation")
(mark-sensitive "card_number")
(mark-sensitive "cvv")
(add-optional-string "audit_tag" "auditTag" "audit trail marker")
```

Parses deterministically, rejects unknown ops at parse time, composes with
other Transforms uniformly.

## Remediation Harness

`iac_forge::remediation` closes the "bounded, auditable transform application"
loop. `apply_proposal(resource, proposal)` returns an `Outcome` with:

- `before` / `after` — original and transformed IR
- `before_hash` / `after_hash` — BLAKE3 hex of canonical emission
- `ops` — parsed `Vec<ResourceOp>`
- `edits` — structural edit list from `sexpr_diff::diff(before, after)`
- `proposal_reason` — free-form audit string

`apply_proposal_with_invariants` also runs a `&[Invariant]` on the post-state;
any violation blocks the outcome with `RemediationError::InvariantViolations`.

`outcome_sexpr(o)` produces a canonical audit header:
```
(remediation-outcome (:reason "…") (:before-hash "…") (:after-hash "…")
                     (:edit-count N) (:op-count N))
```

Proven under 256 proptest cases/property: `changed() ⟺ !edits.is_empty()
⟺ before_hash ≠ after_hash`; idempotent ops stay idempotent; apply is
deterministic; outcome sexpr round-trips.

## Semantic IR Diff

`sexpr_diff::diff(old, new) → Vec<Edit>` returns a structural edit list:

- `Edit::Added { path, value }`
- `Edit::Removed { path, value }`
- `Edit::Changed { path, old, new }`

Paths are dotted + bracketed (`resource.attributes[2].required`). Struct-form
field reordering is invisible (keyed by keyword, not position). Lists diff
positionally; tuple-tag forms with the same head diff children at bracketed
indices.

## Policy-as-Code

Compliance controls are expressible as data via `iac_forge::policy`:

```rust
Policy {
    id: "sensitive-immutable",
    description: "every sensitive attribute must also be immutable",
    pattern: Pattern::Struct {
        head: "attribute".into(),
        fields: vec![("sensitive".into(), Pattern::Bool(true))],
    },
    rule: Rule::RequireField {
        field: "immutable".into(),
        pattern: Pattern::Bool(true),
    },
}
```

`evaluate(&[policies], &ir.to_sexpr())` walks the tree, applies rules at every
match site, and returns a `PolicyReport` with per-finding paths and reasons.
Deterministic, no Rust code required to author controls — a Policy is pure
data and can be stored, signed, and round-tripped through the sexpr layer.

## Fleet

`fleet::Fleet { name, members: BTreeMap<String, IacResource> }` is the
deploy-level attestation primitive. `BTreeMap` canonicalizes order, so
insertion order doesn't affect the content hash. `fleet.member_hash(name)`
returns the per-resource content hash — unchanged members preserve their hash
across fleet mutations.

One Fleet, one hash, one sekiban gate.

## Test Fixture Interchange

`testing::fixtures::{save_resource, load_resource, …}` reads and writes IR
values as canonical sexpr files. Replaces the backend fixture-drift pattern
where hand-written Rust literals fall behind as `IacAttribute` grows new
fields.

```rust
// Save an IR value as a fixture for backend tests
fixtures::save_resource(&resource, "tests/fixtures/widget.sexpr")?;

// Load in tests (also available via include_str! + load_resource_str)
let r = fixtures::load_resource("tests/fixtures/widget.sexpr")?;
```

## Portability (Cross-Language Content Hash)

The canonical sexpr form + BLAKE3 contract is language-portable. A frozen
set of 38+ `(canonical_text, b3sum_hex)` vectors lives in
`tests/cross_lang_vectors.rs`, verified independently against `b3sum 1.8.4`
(nixpkgs). Any reimplementation that:

1. Emits canonical sexpr per the rules above
2. Hashes with BLAKE3

will produce the same hashes by construction. The vectors cover every
`IacType` variant, every `RubyType` variant (via ruby-synthesizer), every
`RbsType` variant, plus lists/primitives.

A reference Ruby implementation lives in `tests/cross_lang/sexpr_ref.rb`
and a cross-language agreement test at `tests/cross_language.rs` shells out
to it under `nix-shell -p ruby_3_3` when available.

## Non-Exhaustive Variants

Both `IacType` and `ArtifactKind` are `#[non_exhaustive]`. Downstream backend
crates MUST include a wildcard `_ =>` arm in any match over them, or add
explicit arms for every variant. Missing a wildcard breaks on any variant
addition — bump the schema version in `render_cache` + backend tests when
this happens.

## IacType::Numeric

`Numeric` represents Terraform's `number` type, which can be integer or float.
Backends map it to their most appropriate numeric type:
- pangea-forge: `T::Coercible::Float`
- terraform-forge: `schema.TypeFloat`
- pulumi-forge: `pulumi.Number`
- steampipe-forge: `proto.ColumnType_DOUBLE`

## json_encoded Annotation

`IacAttribute.json_encoded = true` marks fields that contain serialized JSON
(e.g., IAM policy documents). Backends can use this hint to generate
appropriate helpers (e.g., `jsonencode()` wrappers in Terraform, `to_json`
coercion in Pangea).

## Testing

Use `iac_forge::testing` for shared fixtures:
```rust
use iac_forge::testing::{test_provider, test_resource, TestAttributeBuilder};
let provider = test_provider("acme");
let resource = test_resource("secret");
let attr = TestAttributeBuilder::new("key", IacType::String).required().sensitive().build();
```

`TestAttributeBuilder` supports: `.required()`, `.computed()`, `.sensitive()`,
`.immutable()`, `.update_only()`, `.read_path(p)`, `.description(d)`,
`.default_value(v)`, `.enum_values(vs)`, `.build()`.

Hyphenated names are auto-converted to snake_case for `canonical_name`
(e.g., `"my-field"` → api_name `"my-field"`, canonical_name `"my_field"`).

## Where iac-forge sits in the platform loop

iac-forge provides the **axiom set** layer for infrastructure. See
`docs/CANDIDATE_LOOP.md` for the full pattern. iac-forge's role:

- **Axioms**: `IacType` (11 variants, `#[non_exhaustive]`), `ArtifactKind`,
  `ResourceOp` (transform DSL), `Pattern` / `Rule` (policy DSL),
  `StageKind` (pipeline DSL). Each is a closed Rust enum; users can't
  invent primitives.
- **Candidate generation**: Lisp / sexpr composes over the axioms via
  the canonical sexpr interchange (every type implements `ToSExpr`
  with BLAKE3 content addressing).
- **Composition proofs**: arch-synthesizer verifies architectural
  compositions of IacResources; iac-forge's own `Morphism::check_invariants`
  gates every transformation.
- **Runtime verifier**: the `Backend` trait (+ blanket `ProvenMorphism`
  impl) renders the candidate; `ml-forge` + `substrate-forge` run
  the rendered artifacts when the domain is execution.
- **Decision**: `RenderCache` + `Fleet::content_hash` + tameshi
  attestation cache the winner. Losing candidates evaporate with their
  Pods.

Every module in iac-forge declares its place in this loop. Downstream
backends inherit the discipline automatically via the blanket impls.

## Test Count

540+ tests across lib + integration covering:
- IR types, resolver, spec loading, naming, type mapping
- Backend trait contract, naming conventions
- Morphism laws (identity units, associativity, proof composition, traceability)
- Transform + script parser (structural + proptest)
- Sexpr emit/parse, round-trip, content hash determinism (unit + proptest)
- Sexpr IR impls for every type (unit + proptest)
- Render cache (hit/miss, provenance preservation, multi-backend isolation)
- Fleet (ordering, mutation, per-member hash)
- Policy engine (patterns, rules, walk)
- Remediation (apply, diff, invariants, outcome sexpr — unit + proptest)
- Frozen cross-language vectors vs `b3sum 1.8.4`
- Helpers + error paths
