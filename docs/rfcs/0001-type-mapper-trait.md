# RFC 0001 — TypeMapper<T> trait for IacType → platform type

**Status:** Draft
**Author:** cross-repo Rust survey, 2026-04-19
**Discussion scope:** iac-forge, terraform-forge, pulumi-forge, crossplane-forge, ansible-forge, pangea-forge, steampipe-forge, helm-forge

## Summary

Six `*-forge` backends each reimplement `IacType` → platform-type conversion.
The logic is recursive (IacType::List(inner) requires mapping `inner` too)
and spans these files:

| Crate | File | Function | Output |
|---|---|---|---|
| terraform-forge | `src/type_map.rs` | IacType → GoType → TfAttrType | Go + schema |
| ansible-forge | `src/module_gen.rs` | `AnsibleTypeExt::ansible_type` | Python str |
| crossplane-forge | `src/crd.rs` | `iac_type_to_schema` | OpenAPI v3 JSON |
| pulumi-forge | `src/backend.rs` | `iac_type_to_property_spec` | PropertySpec |
| steampipe-forge | `src/backend.rs` | IacType → `proto.ColumnType` | Go proto |
| helm-forge | `src/schema_gen.rs` | IacType → JSON Schema | JSON Schema |

All follow the same shape: `match iac_type { ... }` with recursive calls
for List/Set/Map/Object/Enum. The only per-backend variance is the output
type and the per-variant mapping.

## Motivation

- Adding a variant to `IacType` (e.g. `IacType::Duration`) requires editing
  all 6 files plus any downstream tests. Miss one → backend panics at
  render time.
- `IacType` is already `#[non_exhaustive]`, which enforces wildcard arms
  in each match — but that turns a compile error into a silent fallback.
  A shared abstraction would make "handle all variants" provable.
- Backends that don't need all variants (e.g. steampipe doesn't really
  care about Set vs List) currently duplicate the full match anyway.

## Proposal

Add a `TypeMapper<T>` trait to `iac-forge::type_map`:

```rust
/// Map an IacType to a target representation.
///
/// Implementations only need to handle the primitive variants; the blanket
/// methods recurse for List, Set, Map, and Object.
pub trait TypeMapper {
    type Output;

    // Primitives — must be implemented.
    fn map_string(&self) -> Self::Output;
    fn map_integer(&self) -> Self::Output;
    fn map_float(&self) -> Self::Output;
    fn map_numeric(&self) -> Self::Output;
    fn map_boolean(&self) -> Self::Output;
    fn map_any(&self) -> Self::Output;

    // Collections — implementations express how to wrap an inner type.
    fn map_list(&self, inner: Self::Output) -> Self::Output;
    fn map_set(&self, inner: Self::Output) -> Self::Output;
    fn map_map(&self, inner: Self::Output) -> Self::Output;
    fn map_object(&self, fields: BTreeMap<String, Self::Output>) -> Self::Output;
    fn map_enum(&self, values: &[String]) -> Self::Output;

    /// Default walk — handles the recursive structure once, dispatches to
    /// the primitive methods per variant. Implementations rarely need to
    /// override this.
    fn map(&self, ty: &IacType) -> Self::Output {
        match ty {
            IacType::String => self.map_string(),
            IacType::Integer => self.map_integer(),
            IacType::Float => self.map_float(),
            IacType::Numeric => self.map_numeric(),
            IacType::Boolean => self.map_boolean(),
            IacType::Any => self.map_any(),
            IacType::List(inner) => {
                let inner = self.map(inner);
                self.map_list(inner)
            }
            IacType::Set(inner) => {
                let inner = self.map(inner);
                self.map_set(inner)
            }
            IacType::Map(inner) => {
                let inner = self.map(inner);
                self.map_map(inner)
            }
            IacType::Object { fields } => {
                let mapped: BTreeMap<_, _> = fields
                    .iter()
                    .map(|(k, v)| (k.clone(), self.map(v)))
                    .collect();
                self.map_object(mapped)
            }
            IacType::Enum { values, .. } => self.map_enum(values),
            // Wildcard handled at compile time via `match` exhaustiveness
            // on the sealed subset.
        }
    }
}
```

`IacType` is `#[non_exhaustive]` — new variants land in iac-forge with a
default method that panics with a clear message. Backends opt into
supporting the new variant by overriding the method. This is strictly
better than today's silent wildcard behaviour.

## Migration path

1. Land `TypeMapper<T>` in `iac-forge::type_map` with the default `map()` walk.
2. Migrate one backend as a reference (recommend **steampipe** — it has the
   simplest output type, Go `proto.ColumnType`).
3. Then one at a time: crossplane (JSON Schema), helm (JSON Schema — shares
   ~80% with crossplane, consider extracting a shared `JsonSchemaMapper`),
   pulumi, ansible, terraform (most complex, do last).
4. Delete the per-backend `iac_type_to_*` functions as each migration lands.

## Non-goals

- Forcing terraform-forge's two-level `GoType → TfAttrType` to collapse.
  terraform-forge has legitimate reasons for a two-step lowering (Go type
  first, then terraform-plugin-framework AttrType on top). It can
  implement two `TypeMapper`s and compose them — the trait doesn't
  prescribe a single-step mapping.
- Unifying the output types. Each backend still emits its own shape.

## Related

- [RFC 0003 (convergence-trait/docs/rfcs/0003-ast-domains-crate.md)](../../../convergence-trait/docs/rfcs/0003-ast-domains-crate.md)
  proposes the parallel ast-domains crate for 16 synthesizers. `TypeMapper`
  is the IaC-layer analogue — `IacType` is to iac-forge what `Primitive`
  would be to ast-domains.

## Questions to resolve

1. Should `map_enum` accept the `base_type` as well (Enum values could be
   strings or integers)? Currently `IacType::Enum { values, .. }` carries
   a `base_type` — check whether any backend currently consumes it.
2. Is `BTreeMap<String, _>` the right container for `map_object`, or should
   it be `IndexMap` (insertion order preservation for generated code)?
3. Where do we put the test utility `TypeMapperHarness` that walks through
   every IacType variant and asserts the output type? In `iac-forge::testing`
   seems right.

## Open work

- terraform-forge's `type_map.rs` is the most complex consumer and has
  pending tasks around `IacType::Numeric` mapping to `Float` vs `Integer`.
  That investigation would naturally slot into the migration.
- helm-forge and crossplane-forge both emit JSON Schema but with slightly
  different shapes (required vs. default handling). Migration is a chance
  to spot the delta and either unify or document the intentional divergence.
