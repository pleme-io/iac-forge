//! Content-addressed render cache.
//!
//! Caches backend rendering output keyed on the source IR's content
//! hash and the backend's platform identifier. A cache hit skips
//! the entire `generate_resource` call — rendering becomes a hashmap
//! lookup when nothing in the IR changed.
//!
//! # Invariants
//!
//! - **Purity is required.** Cache hits short-circuit `Backend::apply`;
//!   if a backend has observable side effects (touching the filesystem,
//!   writing a log, reading env vars), the cache hides them. All
//!   existing backends are pure renderers — this is safe for them.
//! - **Schema version.** The cache key includes a schema version so
//!   Backend trait upgrades that change output invalidate old entries
//!   automatically. Bump `SCHEMA_VERSION` whenever the Backend contract
//!   changes in a user-visible way.
//! - **Deterministic inputs.** Because `content_hash` is over canonical
//!   emission and canonical emission is deterministic (proven by
//!   proptest), two semantically-equal IR values ALWAYS produce the
//!   same cache key.
//!
//! # Usage
//!
//! ```no_run
//! use iac_forge::render_cache::RenderCache;
//! # use iac_forge::backend::Backend;
//! # use iac_forge::morphism::{Morphism, ResourceInput};
//! # fn example<B: Backend>(backend: &B, input: ResourceInput<'_>) {
//! let mut cache = RenderCache::new();
//! let artifacts = cache.render(backend, &input);
//! // Second call with the same (backend, input) is a lookup.
//! let artifacts2 = cache.render(backend, &input);
//! # let _ = (artifacts, artifacts2);
//! # }
//! ```

use std::collections::HashMap;

use crate::backend::{Backend, GeneratedArtifact};
use crate::morphism::{Morphism, ResourceInput};
use crate::sexpr::{ContentHash, ToSExpr};

/// Schema version embedded in every cache key. Bump when the Backend
/// trait's output contract changes in a way that would invalidate
/// cached artifacts (new mandatory field, changed path scheme, etc.).
pub const SCHEMA_VERSION: u32 = 1;

/// Cache key: (schema_version, backend_platform, ir_content_hash).
///
/// Three-part: the schema version guards against trait evolution, the
/// platform name guards against backend-specific output, the IR hash
/// identifies the input.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    pub schema_version: u32,
    pub platform: String,
    pub ir_hash: ContentHash,
}

impl CacheKey {
    /// Construct a key for the current schema and a (backend, resource).
    ///
    /// Uses the source resource's `content_hash` and the backend's
    /// `platform()` as the two variable parts.
    #[must_use]
    pub fn for_resource<B: Backend>(backend: &B, input: &ResourceInput<'_>) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            platform: backend.platform().to_string(),
            ir_hash: input.resource.content_hash(),
        }
    }
}

/// A stat block for cache observability — useful in tests and telemetry.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
}

impl CacheStats {
    #[must_use]
    pub fn total(&self) -> u64 {
        self.hits + self.misses
    }

    #[must_use]
    pub fn hit_ratio(&self) -> f64 {
        let total = self.total();
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

/// In-memory render cache.
#[derive(Debug, Default)]
pub struct RenderCache {
    entries: HashMap<CacheKey, Vec<GeneratedArtifact>>,
    stats: CacheStats,
}

impl RenderCache {
    /// New empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Render via cache: lookup by (backend, resource) content hash;
    /// on miss, call the backend through the Morphism interface so
    /// artifact provenance is populated, then store.
    #[must_use]
    pub fn render<B: Backend>(
        &mut self,
        backend: &B,
        input: &ResourceInput<'_>,
    ) -> Vec<GeneratedArtifact> {
        let key = CacheKey::for_resource(backend, input);
        if let Some(cached) = self.entries.get(&key) {
            self.stats.hits += 1;
            return cached.clone();
        }
        self.stats.misses += 1;
        let rendered = <B as Morphism<_, _>>::apply(backend, input);
        self.entries.insert(key, rendered.clone());
        rendered
    }

    /// Current cache statistics.
    #[must_use]
    pub fn stats(&self) -> &CacheStats {
        &self.stats
    }

    /// Number of entries currently cached.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Drop all entries (preserves stats).
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Remove a specific entry. Returns whether it was present.
    pub fn invalidate<B: Backend>(
        &mut self,
        backend: &B,
        input: &ResourceInput<'_>,
    ) -> bool {
        self.entries
            .remove(&CacheKey::for_resource(backend, input))
            .is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{ArtifactKind, GeneratedArtifact, NamingConvention};
    use crate::error::IacForgeError;
    use crate::ir::{IacDataSource, IacProvider, IacResource};
    use crate::testing::{test_provider, test_resource};

    // A counting backend: tracks how many times generate_resource was called.
    struct CountingBackend {
        calls: std::cell::Cell<u32>,
    }
    impl CountingBackend {
        fn new() -> Self {
            Self { calls: std::cell::Cell::new(0) }
        }
    }
    struct PlainNaming;
    impl NamingConvention for PlainNaming {
        fn resource_type_name(&self, r: &str, p: &str) -> String {
            format!("{p}_{r}")
        }
        fn file_name(&self, r: &str, _k: &ArtifactKind) -> String {
            format!("{r}.out")
        }
        fn field_name(&self, n: &str) -> String {
            n.to_string()
        }
    }
    impl Backend for CountingBackend {
        fn platform(&self) -> &str {
            "counting"
        }
        fn naming(&self) -> &dyn NamingConvention {
            &PlainNaming
        }
        fn generate_resource(
            &self,
            r: &IacResource,
            _p: &IacProvider,
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            self.calls.set(self.calls.get() + 1);
            Ok(vec![GeneratedArtifact::new(
                format!("{}.out", r.name),
                format!("body for {}", r.name),
                ArtifactKind::Resource,
            )])
        }
        fn generate_data_source(
            &self,
            _d: &IacDataSource,
            _p: &IacProvider,
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![])
        }
        fn generate_provider(
            &self,
            _p: &IacProvider,
            _r: &[IacResource],
            _d: &[IacDataSource],
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![])
        }
        fn generate_test(
            &self,
            _r: &IacResource,
            _p: &IacProvider,
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![])
        }
    }

    fn fixture() -> (IacResource, IacProvider) {
        (test_resource("widget"), test_provider("acme"))
    }

    // ── Basic behaviour ───────────────────────────────────────

    #[test]
    fn hit_after_miss_returns_same_artifacts() {
        let (r, p) = fixture();
        let input = ResourceInput { resource: &r, provider: &p };
        let backend = CountingBackend::new();
        let mut cache = RenderCache::new();

        let a = cache.render(&backend, &input);
        let b = cache.render(&backend, &input);
        assert_eq!(a, b);
    }

    #[test]
    fn cache_short_circuits_backend_after_first_call() {
        let (r, p) = fixture();
        let input = ResourceInput { resource: &r, provider: &p };
        let backend = CountingBackend::new();
        let mut cache = RenderCache::new();

        cache.render(&backend, &input);
        cache.render(&backend, &input);
        cache.render(&backend, &input);
        assert_eq!(
            backend.calls.get(),
            1,
            "backend should only be invoked once across three cache lookups",
        );
    }

    #[test]
    fn stats_track_hits_and_misses() {
        let (r, p) = fixture();
        let input = ResourceInput { resource: &r, provider: &p };
        let backend = CountingBackend::new();
        let mut cache = RenderCache::new();

        cache.render(&backend, &input);
        cache.render(&backend, &input);
        cache.render(&backend, &input);

        assert_eq!(cache.stats().misses, 1);
        assert_eq!(cache.stats().hits, 2);
        assert!((cache.stats().hit_ratio() - (2.0 / 3.0)).abs() < 1e-9);
    }

    #[test]
    fn stats_start_empty() {
        let cache = RenderCache::new();
        assert_eq!(cache.stats().total(), 0);
        assert_eq!(cache.stats().hit_ratio(), 0.0);
        assert!(cache.is_empty());
    }

    #[test]
    fn different_resources_miss_independently() {
        let p = test_provider("acme");
        let r1 = test_resource("widget");
        let r2 = test_resource("gadget");
        let backend = CountingBackend::new();
        let mut cache = RenderCache::new();

        cache.render(
            &backend,
            &ResourceInput { resource: &r1, provider: &p },
        );
        cache.render(
            &backend,
            &ResourceInput { resource: &r2, provider: &p },
        );
        assert_eq!(backend.calls.get(), 2);
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn semantically_equal_resources_hit_the_same_key() {
        // Two resources built from the same fixture helper must
        // produce equal content hashes and therefore share a key.
        let p = test_provider("acme");
        let r_a = test_resource("widget");
        let r_b = test_resource("widget");
        let backend = CountingBackend::new();
        let mut cache = RenderCache::new();

        cache.render(
            &backend,
            &ResourceInput { resource: &r_a, provider: &p },
        );
        cache.render(
            &backend,
            &ResourceInput { resource: &r_b, provider: &p },
        );
        assert_eq!(
            backend.calls.get(),
            1,
            "structurally-equal resources must share a cache key",
        );
    }

    // ── Invalidation ──────────────────────────────────────────

    #[test]
    fn invalidate_removes_specific_entry() {
        let (r, p) = fixture();
        let input = ResourceInput { resource: &r, provider: &p };
        let backend = CountingBackend::new();
        let mut cache = RenderCache::new();

        cache.render(&backend, &input);
        assert_eq!(cache.len(), 1);
        assert!(cache.invalidate(&backend, &input));
        assert!(cache.is_empty());

        // Next render is a miss again.
        cache.render(&backend, &input);
        assert_eq!(backend.calls.get(), 2);
    }

    #[test]
    fn invalidate_reports_false_on_missing_entry() {
        let (r, p) = fixture();
        let input = ResourceInput { resource: &r, provider: &p };
        let backend = CountingBackend::new();
        let mut cache = RenderCache::new();
        assert!(!cache.invalidate(&backend, &input));
    }

    #[test]
    fn clear_drops_all_entries() {
        let p = test_provider("acme");
        let r1 = test_resource("a");
        let r2 = test_resource("b");
        let backend = CountingBackend::new();
        let mut cache = RenderCache::new();

        cache.render(
            &backend,
            &ResourceInput { resource: &r1, provider: &p },
        );
        cache.render(
            &backend,
            &ResourceInput { resource: &r2, provider: &p },
        );
        assert_eq!(cache.len(), 2);
        cache.clear();
        assert!(cache.is_empty());
    }

    // ── Key structure ─────────────────────────────────────────

    #[test]
    fn cache_key_contains_schema_version_and_platform() {
        let (r, p) = fixture();
        let input = ResourceInput { resource: &r, provider: &p };
        let backend = CountingBackend::new();
        let key = CacheKey::for_resource(&backend, &input);
        assert_eq!(key.schema_version, SCHEMA_VERSION);
        assert_eq!(key.platform, "counting");
        assert_eq!(key.ir_hash, r.content_hash());
    }

    #[test]
    fn cached_artifacts_carry_provenance() {
        // Cache hits must preserve the provenance the Morphism apply
        // populated on the first call (otherwise attestation would
        // degrade to "unknown origin" on the second hit).
        let (r, p) = fixture();
        let input = ResourceInput { resource: &r, provider: &p };
        let backend = CountingBackend::new();
        let mut cache = RenderCache::new();

        let first = cache.render(&backend, &input);
        let second = cache.render(&backend, &input);
        assert_eq!(first[0].source_hash, second[0].source_hash);
        assert!(!first[0].source_hash.is_empty());
        assert!(!first[0].morphism_chain.is_empty());
    }

    // ── Cross-backend isolation ──────────────────────────────

    struct OtherBackend;
    impl Backend for OtherBackend {
        fn platform(&self) -> &str {
            "other"
        }
        fn naming(&self) -> &dyn NamingConvention {
            &PlainNaming
        }
        fn generate_resource(
            &self,
            r: &IacResource,
            _p: &IacProvider,
        ) -> Result<Vec<GeneratedArtifact>, IacForgeError> {
            Ok(vec![GeneratedArtifact::new(
                format!("{}.other", r.name),
                "other content".to_string(),
                ArtifactKind::Resource,
            )])
        }
        fn generate_data_source(&self, _d: &IacDataSource, _p: &IacProvider) -> Result<Vec<GeneratedArtifact>, IacForgeError> { Ok(vec![]) }
        fn generate_provider(&self, _p: &IacProvider, _r: &[IacResource], _d: &[IacDataSource]) -> Result<Vec<GeneratedArtifact>, IacForgeError> { Ok(vec![]) }
        fn generate_test(&self, _r: &IacResource, _p: &IacProvider) -> Result<Vec<GeneratedArtifact>, IacForgeError> { Ok(vec![]) }
    }

    #[test]
    fn different_backends_do_not_share_cache_entries() {
        let (r, p) = fixture();
        let input = ResourceInput { resource: &r, provider: &p };
        let counting = CountingBackend::new();
        let other = OtherBackend;
        let mut cache = RenderCache::new();

        cache.render(&counting, &input);
        cache.render(&other, &input);

        // Two distinct entries — platform is part of the key.
        assert_eq!(cache.len(), 2);
        assert_eq!(counting.calls.get(), 1);
    }
}
