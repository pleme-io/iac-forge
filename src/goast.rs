//! Typed Go AST + printer.
//!
//! Mirrors the role [`crate::nix`] plays for Nix emission: every backend
//! that needs to emit Go source code builds a [`GoFile`] value
//! structurally, then renders it through [`GoPrinter`]. **No backend
//! should ever construct Go source via `format!()` strings of Go
//! syntax** — that bypasses the [`crate::backend::Backend`] /
//! [`Synthesizer`] morphism and loses every benefit of typed
//! intermediates (refactor safety, structural diff, kubebuilder-tag
//! composition, future syntax-rewriting transforms, etc.).
//!
//! ## Design
//!
//! The AST covers the *minimum* Go surface our emitters need today,
//! shaped by:
//!
//!   - **Crossplane managed-resource types** — Spec/Status/Parameters/
//!     Observation structs with json + kubebuilder tags, GroupVersion
//!     boilerplate, deepcopy-friendly shapes.
//!   - **Crossplane controllers** — `external` struct + ExternalClient
//!     methods (Observe/Create/Update/Delete), `connector` struct +
//!     Connect, Setup function wiring controller-runtime.
//!   - **Provider runtime scaffold** — main.go, internal/controller/setup.go,
//!     ProviderConfig types.
//!
//! The AST is **not** a complete Go grammar. Adding a new syntactic form
//! (range loops, type switches, generics) means one explicit enum
//! variant; that's the substrate-hygienic way to surface need rather
//! than re-introducing `format!()` shortcuts.
//!
//! ## Printer guarantees
//!
//! - Emitted output is gofmt-stable: tabs for indentation, gofmt-style
//!   import grouping (stdlib first, then third-party).
//! - Two structurally identical [`GoFile`] values render to byte-equal
//!   output — tests assert on the AST, not on substring presence.
//! - Trailing newline on every file; no double blank lines except where
//!   explicitly requested via [`GoStmt::Blank`].

use std::fmt;

// ── File / package level ──────────────────────────────────────────────────

/// A single Go source file. Top-level node every emitter constructs.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct GoFile {
    /// File-level doc comment (placed above the package declaration).
    pub doc: Option<String>,
    /// `package <name>` declaration.
    pub package: String,
    /// Markers that go above the package declaration (e.g. kubebuilder
    /// `+kubebuilder:object:generate=true`, `+groupName=...`).
    pub markers: Vec<KubeMarker>,
    /// Import declarations. Printer groups stdlib first, third-party
    /// second; alphabetised within each group.
    pub imports: Vec<GoImport>,
    /// Top-level declarations in source order.
    pub decls: Vec<GoDecl>,
}

impl GoFile {
    #[must_use]
    pub fn new(package: impl Into<String>) -> Self {
        Self {
            doc: None,
            package: package.into(),
            markers: Vec::new(),
            imports: Vec::new(),
            decls: Vec::new(),
        }
    }
}

/// `import "<path>"` or `<alias> "<path>"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoImport {
    pub path: String,
    pub alias: Option<String>,
}

impl GoImport {
    #[must_use]
    pub fn plain(path: impl Into<String>) -> Self {
        Self { path: path.into(), alias: None }
    }

    #[must_use]
    pub fn aliased(alias: impl Into<String>, path: impl Into<String>) -> Self {
        Self { path: path.into(), alias: Some(alias.into()) }
    }

    /// Stdlib paths have no `.` and no `/` (or only standard module
    /// segments) — used by the printer for grouping.
    #[must_use]
    pub fn is_stdlib(&self) -> bool {
        !self.path.contains('.')
    }
}

// ── Declarations ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum GoDecl {
    Type(GoTypeDecl),
    Func(GoFuncDecl),
    Var(GoVarDecl),
    /// A line comment placed at file scope between decls.
    Comment(String),
    /// Blank line marker — printer emits an empty line.
    Blank,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoTypeDecl {
    pub name: String,
    pub doc: Option<String>,
    pub markers: Vec<KubeMarker>,
    pub body: GoTypeBody,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum GoTypeBody {
    Struct(Vec<GoField>),
    Alias(GoType),
}

/// A struct field. `name: None` means an embedded field (the printer
/// emits just the type, which Go interprets as embedding).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoField {
    pub name: Option<String>,
    pub ty: GoType,
    pub doc: Option<String>,
    pub markers: Vec<KubeMarker>,
    pub tags: Vec<GoStructTag>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum GoStructTag {
    Json(JsonTag),
    Yaml(YamlTag),
    /// Free-form key-value tag for cases the structured variants don't
    /// cover (e.g. `validate:"..."`, `protobuf:"..."`). Discouraged when
    /// a structured variant exists.
    Custom { key: String, value: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JsonTag {
    pub name: String,
    pub omitempty: bool,
    pub inline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct YamlTag {
    pub name: String,
    pub omitempty: bool,
    pub inline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoFuncDecl {
    pub name: String,
    pub doc: Option<String>,
    pub recv: Option<GoRecv>,
    pub params: Vec<GoParam>,
    pub returns: Vec<GoType>,
    pub body: GoBlock,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoRecv {
    pub name: String,
    pub ty: GoType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoParam {
    pub name: String,
    pub ty: GoType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoVarDecl {
    pub name: String,
    pub ty: Option<GoType>,
    pub value: Option<GoExpr>,
    pub doc: Option<String>,
    /// `var ( ... )` block grouping: when several VarDecls share a
    /// `GoVarBlockId`, the printer groups them in a single `var (...)`.
    /// `None` means standalone `var <name> ...` line.
    pub block_id: Option<u32>,
}

// ── Types ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum GoType {
    /// `string`, `int64`, `error`, `MyStruct` — anything in the current
    /// package (or builtin).
    Named(String),
    /// `pkg.Name` — references a type from another package by alias.
    Qualified { pkg: String, name: String },
    /// `*T`.
    Pointer(Box<GoType>),
    /// `[]T`.
    Slice(Box<GoType>),
    /// `map[K]V`.
    Map(Box<GoType>, Box<GoType>),
    /// `interface{}`.
    EmptyInterface,
    /// `func(P1, P2, ...) (R1, R2, ...)` as a *type* — used as the
    /// element type of a slice of function values, the value type of a
    /// map, etc. Parameter names are not represented because Go function
    /// types don't carry parameter names structurally.
    FuncSignature {
        params: Vec<GoType>,
        returns: Vec<GoType>,
    },
}

impl GoType {
    #[must_use]
    pub fn named(s: impl Into<String>) -> Self {
        Self::Named(s.into())
    }

    #[must_use]
    pub fn qualified(pkg: impl Into<String>, name: impl Into<String>) -> Self {
        Self::Qualified { pkg: pkg.into(), name: name.into() }
    }

    #[must_use]
    pub fn pointer(inner: GoType) -> Self {
        Self::Pointer(Box::new(inner))
    }

    #[must_use]
    pub fn slice(inner: GoType) -> Self {
        Self::Slice(Box::new(inner))
    }
}

// ── Statements ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GoBlock {
    pub stmts: Vec<GoStmt>,
}

impl GoBlock {
    #[must_use]
    pub fn new() -> Self { Self::default() }

    pub fn push(&mut self, s: GoStmt) {
        self.stmts.push(s);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum GoStmt {
    /// Just an expression (typically a call) used as a statement.
    Expr(GoExpr),
    Return(Vec<GoExpr>),
    /// `lhs = rhs` (regular assignment).
    Assign { lhs: Vec<GoExpr>, rhs: Vec<GoExpr> },
    /// `lhs := rhs` (short variable declaration).
    ShortDecl { names: Vec<String>, values: Vec<GoExpr> },
    /// `if init; cond { body } else { else_body }`. `init` and `else_body`
    /// are optional. `else_body` is itself a Stmt to allow `else if`
    /// chaining (the parser accepts that as `If { else_body: Some(Box<If>)
    /// }`); Go requires the else block to be `{}` or another `if`.
    If {
        init: Option<Box<GoStmt>>,
        cond: GoExpr,
        body: GoBlock,
        else_body: Option<Box<GoStmt>>,
    },
    /// `// comment` placed inside a block.
    Comment(String),
    /// Blank line inside a block.
    Blank,
    /// `<block>` — a nested block (for else branches that are blocks
    /// rather than if-chains).
    Block(GoBlock),
    /// `for k, v := range expr { body }`. Either of `key` / `value` may
    /// be `None`; both `None` is `for range expr {...}` (Go 1.22+
    /// equivalent of `for _ = range expr`). The iteration variables are
    /// declared with `:=`.
    ForRange {
        key: Option<String>,
        value: Option<String>,
        range: GoExpr,
        body: GoBlock,
    },
}

// ── Expressions ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum GoExpr {
    Ident(String),
    Lit(GoLit),
    /// `fun(args...)`.
    Call { fun: Box<GoExpr>, args: Vec<GoExpr> },
    /// `recv.sel`.
    Selector { recv: Box<GoExpr>, sel: String },
    /// `&T{ field: value, ... }` or `T{ ... }` depending on `addr_of`.
    Composite {
        ty: GoType,
        fields: Vec<(Option<String>, GoExpr)>, // None = positional (rare for structs)
        addr_of: bool,
    },
    /// `*x`.
    Star(Box<GoExpr>),
    /// `&x`.
    AddressOf(Box<GoExpr>),
    /// `x.(T)` (no ok) or `x.(T), ok` via `with_ok=true`.
    /// When `with_ok=true`, the parent must be a `ShortDecl` with two
    /// names (the second is the bool); the printer enforces this.
    TypeAssert { x: Box<GoExpr>, ty: GoType, with_ok: bool },
    /// `[]T{e1, e2, ...}` — typed slice literal. Use `Composite` for
    /// struct literals; this variant is specifically for slices.
    SliceLit { elem_type: GoType, elements: Vec<GoExpr> },
    /// A type used in expression position — primarily as an argument to
    /// builtins like `make([]T, n)`, `new(T)`, or `reflect.TypeOf(T)`.
    /// Renders as the type alone (no `{}` or other punctuation).
    TypeExpr(GoType),
}

impl GoExpr {
    #[must_use]
    pub fn ident(s: impl Into<String>) -> Self {
        Self::Ident(s.into())
    }

    #[must_use]
    pub fn str(s: impl Into<String>) -> Self {
        Self::Lit(GoLit::Str(s.into()))
    }

    #[must_use]
    pub fn nil() -> Self {
        Self::Lit(GoLit::Nil)
    }

    #[must_use]
    pub fn call(fun: GoExpr, args: Vec<GoExpr>) -> Self {
        Self::Call { fun: Box::new(fun), args }
    }

    #[must_use]
    pub fn sel(recv: GoExpr, sel: impl Into<String>) -> Self {
        Self::Selector { recv: Box::new(recv), sel: sel.into() }
    }

    /// Build a chained selector: `a.b.c.d` from `["a","b","c","d"]`.
    /// Empty `path` panics in debug.
    #[must_use]
    pub fn path(segments: &[&str]) -> Self {
        debug_assert!(!segments.is_empty(), "GoExpr::path needs ≥1 segment");
        let mut iter = segments.iter();
        let mut acc = Self::Ident((*iter.next().unwrap()).to_string());
        for s in iter {
            acc = Self::Selector { recv: Box::new(acc), sel: (*s).to_string() };
        }
        acc
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum GoLit {
    Str(String),
    Int(i64),
    Bool(bool),
    Nil,
}

// ── Kubebuilder markers (structural — not strings) ────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum KubeMarker {
    Required,
    Optional,
    XValidationCEL { rule: String, message: String },
    ObjectGenerate(bool),
    ObjectRoot,
    Subresource(SubresourceKind),
    Resource { scope: ResourceScope, categories: Vec<String> },
    PrintColumn { name: String, ty: String, json_path: String, priority: Option<u32> },
    GroupName(String),
    /// Free-form fallback for kubebuilder markers we haven't structured
    /// yet. Discouraged when a structured variant exists; the goal is
    /// for every emitter to use only structured variants.
    Free(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SubresourceKind {
    Status,
    Scale,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ResourceScope {
    Cluster,
    Namespaced,
}

// ── Printer ───────────────────────────────────────────────────────────────

/// Render a [`GoFile`] to gofmt-stable Go source.
#[must_use]
pub fn print_file(file: &GoFile) -> String {
    let mut p = GoPrinter::new();
    p.print_file(file);
    p.finish()
}

#[derive(Default)]
pub struct GoPrinter {
    out: String,
    indent: usize,
}

impl GoPrinter {
    #[must_use]
    pub fn new() -> Self { Self::default() }

    pub fn finish(self) -> String { self.out }

    fn write(&mut self, s: &str) {
        self.out.push_str(s);
    }

    fn newline(&mut self) {
        self.out.push('\n');
    }

    fn write_indent(&mut self) {
        for _ in 0..self.indent {
            self.out.push('\t');
        }
    }

    fn write_doc(&mut self, doc: &Option<String>) {
        if let Some(d) = doc {
            for line in d.lines() {
                self.write_indent();
                self.write("// ");
                self.write(line);
                self.newline();
            }
        }
    }

    fn write_markers(&mut self, markers: &[KubeMarker]) {
        for m in markers {
            self.write_indent();
            self.write("// ");
            self.write_marker(m);
            self.newline();
        }
    }

    fn write_marker(&mut self, m: &KubeMarker) {
        match m {
            KubeMarker::Required => self.write("+kubebuilder:validation:Required"),
            KubeMarker::Optional => self.write("+optional"),
            KubeMarker::XValidationCEL { rule, message } => {
                self.write(&format_marker_xvalidation(rule, message));
            }
            KubeMarker::ObjectGenerate(b) => {
                self.write(&format!("+kubebuilder:object:generate={b}"));
            }
            KubeMarker::ObjectRoot => self.write("+kubebuilder:object:root=true"),
            KubeMarker::Subresource(SubresourceKind::Status) => {
                self.write("+kubebuilder:subresource:status");
            }
            KubeMarker::Subresource(SubresourceKind::Scale) => {
                self.write("+kubebuilder:subresource:scale");
            }
            KubeMarker::Resource { scope, categories } => {
                let s = match scope {
                    ResourceScope::Cluster => "Cluster",
                    ResourceScope::Namespaced => "Namespaced",
                };
                let cats = categories.join(",");
                self.write(&format!("+kubebuilder:resource:scope={s},categories={{{cats}}}"));
            }
            KubeMarker::PrintColumn { name, ty, json_path, priority } => {
                let prio = priority
                    .map_or_else(String::new, |p| format!(",priority={p}"));
                self.write(&format!(
                    "+kubebuilder:printcolumn:name=\"{name}\",type=\"{ty}\",JSONPath=\"{json_path}\"{prio}"
                ));
            }
            KubeMarker::GroupName(g) => self.write(&format!("+groupName={g}")),
            KubeMarker::Free(s) => self.write(s),
        }
    }

    pub fn print_file(&mut self, f: &GoFile) {
        self.write("// Code generated by iac-forge. DO NOT EDIT.");
        self.newline();
        self.newline();
        self.write_doc(&f.doc);
        // Markers above the package declaration (kubebuilder convention)
        self.write_markers(&f.markers);
        self.write(&format!("package {}", f.package));
        self.newline();
        if !f.imports.is_empty() {
            self.newline();
            self.print_imports(&f.imports);
        }
        for d in &f.decls {
            self.newline();
            self.print_decl(d);
        }
        // Final newline
        if !self.out.ends_with('\n') {
            self.newline();
        }
    }

    fn print_imports(&mut self, imports: &[GoImport]) {
        // Group: stdlib first, then third-party. Alphabetise within each.
        let mut stdlib: Vec<&GoImport> = imports.iter().filter(|i| i.is_stdlib()).collect();
        let mut third: Vec<&GoImport> = imports.iter().filter(|i| !i.is_stdlib()).collect();
        stdlib.sort_by(|a, b| a.path.cmp(&b.path));
        third.sort_by(|a, b| a.path.cmp(&b.path));

        if imports.len() == 1 {
            let i = imports[0].clone();
            self.write("import ");
            self.print_one_import_inline(&i);
            self.newline();
            return;
        }

        self.write("import (");
        self.newline();
        self.indent += 1;
        for i in &stdlib {
            self.write_indent();
            self.print_one_import_inline(i);
            self.newline();
        }
        if !stdlib.is_empty() && !third.is_empty() {
            self.newline();
        }
        for i in &third {
            self.write_indent();
            self.print_one_import_inline(i);
            self.newline();
        }
        self.indent -= 1;
        self.write(")");
        self.newline();
    }

    fn print_one_import_inline(&mut self, i: &GoImport) {
        if let Some(alias) = &i.alias {
            self.write(alias);
            self.write(" ");
        }
        self.write("\"");
        self.write(&i.path);
        self.write("\"");
    }

    fn print_decl(&mut self, d: &GoDecl) {
        match d {
            GoDecl::Type(t) => self.print_type_decl(t),
            GoDecl::Func(f) => self.print_func_decl(f),
            GoDecl::Var(v) => self.print_var_decl_standalone(v),
            GoDecl::Comment(c) => {
                for line in c.lines() {
                    self.write("// ");
                    self.write(line);
                    self.newline();
                }
            }
            GoDecl::Blank => { /* the leading newline already covers it */ }
        }
    }

    fn print_type_decl(&mut self, t: &GoTypeDecl) {
        self.write_doc(&t.doc);
        self.write_markers(&t.markers);
        self.write(&format!("type {} ", t.name));
        match &t.body {
            GoTypeBody::Struct(fields) => {
                self.write("struct {");
                self.newline();
                self.indent += 1;
                for f in fields {
                    self.print_field(f);
                }
                self.indent -= 1;
                self.write("}");
            }
            GoTypeBody::Alias(ty) => {
                self.print_type(ty);
            }
        }
        self.newline();
    }

    fn print_field(&mut self, f: &GoField) {
        self.write_doc_indented(&f.doc);
        self.write_markers(&f.markers);
        self.write_indent();
        if let Some(name) = &f.name {
            self.write(name);
            self.write(" ");
        }
        self.print_type(&f.ty);
        if !f.tags.is_empty() {
            self.write(" `");
            for (i, t) in f.tags.iter().enumerate() {
                if i > 0 {
                    self.write(" ");
                }
                self.print_struct_tag(t);
            }
            self.write("`");
        }
        self.newline();
    }

    fn write_doc_indented(&mut self, doc: &Option<String>) {
        if let Some(d) = doc {
            for line in d.lines() {
                self.write_indent();
                self.write("// ");
                self.write(line);
                self.newline();
            }
        }
    }

    fn print_struct_tag(&mut self, t: &GoStructTag) {
        match t {
            GoStructTag::Json(j) => {
                self.write("json:\"");
                self.write_tag_body(&j.name, j.omitempty, j.inline);
                self.write("\"");
            }
            GoStructTag::Yaml(y) => {
                self.write("yaml:\"");
                self.write_tag_body(&y.name, y.omitempty, y.inline);
                self.write("\"");
            }
            GoStructTag::Custom { key, value } => {
                self.write(key);
                self.write(":\"");
                self.write(value);
                self.write("\"");
            }
        }
    }

    fn write_tag_body(&mut self, name: &str, omitempty: bool, inline: bool) {
        // `,inline` is mutually exclusive with name in practice — when
        // inline, name is ",inline".
        if inline {
            self.write(",inline");
            return;
        }
        self.write(name);
        if omitempty {
            self.write(",omitempty");
        }
    }

    fn print_type(&mut self, ty: &GoType) {
        match ty {
            GoType::Named(n) => self.write(n),
            GoType::Qualified { pkg, name } => {
                self.write(pkg);
                self.write(".");
                self.write(name);
            }
            GoType::Pointer(t) => {
                self.write("*");
                self.print_type(t);
            }
            GoType::Slice(t) => {
                self.write("[]");
                self.print_type(t);
            }
            GoType::Map(k, v) => {
                self.write("map[");
                self.print_type(k);
                self.write("]");
                self.print_type(v);
            }
            GoType::EmptyInterface => self.write("interface{}"),
            GoType::FuncSignature { params, returns } => {
                self.write("func(");
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.print_type(p);
                }
                self.write(")");
                if !returns.is_empty() {
                    if returns.len() == 1 {
                        self.write(" ");
                        self.print_type(&returns[0]);
                    } else {
                        self.write(" (");
                        for (i, r) in returns.iter().enumerate() {
                            if i > 0 {
                                self.write(", ");
                            }
                            self.print_type(r);
                        }
                        self.write(")");
                    }
                }
            }
        }
    }

    fn print_func_decl(&mut self, f: &GoFuncDecl) {
        self.write_doc(&f.doc);
        self.write("func ");
        if let Some(r) = &f.recv {
            self.write("(");
            self.write(&r.name);
            self.write(" ");
            self.print_type(&r.ty);
            self.write(") ");
        }
        self.write(&f.name);
        self.write("(");
        for (i, p) in f.params.iter().enumerate() {
            if i > 0 {
                self.write(", ");
            }
            self.write(&p.name);
            self.write(" ");
            self.print_type(&p.ty);
        }
        self.write(")");
        if !f.returns.is_empty() {
            if f.returns.len() == 1 {
                self.write(" ");
                self.print_type(&f.returns[0]);
            } else {
                self.write(" (");
                for (i, r) in f.returns.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.print_type(r);
                }
                self.write(")");
            }
        }
        self.write(" ");
        self.print_block(&f.body);
        self.newline();
    }

    fn print_var_decl_standalone(&mut self, v: &GoVarDecl) {
        self.write_doc(&v.doc);
        self.write("var ");
        self.write(&v.name);
        if let Some(ty) = &v.ty {
            self.write(" ");
            self.print_type(ty);
        }
        if let Some(val) = &v.value {
            self.write(" = ");
            self.print_expr(val);
        }
        self.newline();
    }

    fn print_block(&mut self, b: &GoBlock) {
        self.write("{");
        self.newline();
        self.indent += 1;
        for s in &b.stmts {
            self.print_stmt(s);
        }
        self.indent -= 1;
        self.write_indent();
        self.write("}");
    }

    fn print_stmt(&mut self, s: &GoStmt) {
        match s {
            GoStmt::Expr(e) => {
                self.write_indent();
                self.print_expr(e);
                self.newline();
            }
            GoStmt::Return(exprs) => {
                self.write_indent();
                self.write("return");
                if !exprs.is_empty() {
                    self.write(" ");
                    for (i, e) in exprs.iter().enumerate() {
                        if i > 0 {
                            self.write(", ");
                        }
                        self.print_expr(e);
                    }
                }
                self.newline();
            }
            GoStmt::Assign { lhs, rhs } => {
                self.write_indent();
                for (i, e) in lhs.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.print_expr(e);
                }
                self.write(" = ");
                for (i, e) in rhs.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.print_expr(e);
                }
                self.newline();
            }
            GoStmt::ShortDecl { names, values } => {
                self.write_indent();
                self.write(&names.join(", "));
                self.write(" := ");
                for (i, e) in values.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.print_expr(e);
                }
                self.newline();
            }
            GoStmt::If { init, cond, body, else_body } => {
                self.write_indent();
                self.write("if ");
                if let Some(s_init) = init {
                    self.print_inline_simple_stmt(s_init);
                    self.write("; ");
                }
                self.print_expr(cond);
                self.write(" ");
                self.print_block(body);
                if let Some(eb) = else_body {
                    self.write(" else ");
                    match eb.as_ref() {
                        GoStmt::If { .. } => {
                            // else if chain — print without re-indenting prefix
                            // and without the "if" keyword (we already wrote " else ")
                            // Just print the If, but inline starting here.
                            self.print_else_if_inline(eb);
                        }
                        GoStmt::Block(b) => self.print_block(b),
                        other => {
                            // Wrap a single statement in a block — Go syntax requires it
                            self.write("{");
                            self.newline();
                            self.indent += 1;
                            self.print_stmt(other);
                            self.indent -= 1;
                            self.write_indent();
                            self.write("}");
                        }
                    }
                }
                self.newline();
            }
            GoStmt::Comment(c) => {
                for line in c.lines() {
                    self.write_indent();
                    self.write("// ");
                    self.write(line);
                    self.newline();
                }
            }
            GoStmt::Blank => self.newline(),
            GoStmt::Block(b) => {
                self.write_indent();
                self.print_block(b);
                self.newline();
            }
            GoStmt::ForRange { key, value, range, body } => {
                self.write_indent();
                self.write("for ");
                match (key.as_deref(), value.as_deref()) {
                    (Some(k), Some(v)) => {
                        self.write(k);
                        self.write(", ");
                        self.write(v);
                        self.write(" := range ");
                    }
                    (Some(k), None) => {
                        self.write(k);
                        self.write(" := range ");
                    }
                    (None, Some(v)) => {
                        // Go syntax: for _, v := range x
                        self.write("_, ");
                        self.write(v);
                        self.write(" := range ");
                    }
                    (None, None) => {
                        // Go 1.22+: for range x {...}
                        self.write("range ");
                    }
                }
                self.print_expr(range);
                self.write(" ");
                self.print_block(body);
                self.newline();
            }
        }
    }

    fn print_else_if_inline(&mut self, s: &GoStmt) {
        match s {
            GoStmt::If { init, cond, body, else_body } => {
                self.write("if ");
                if let Some(i) = init {
                    self.print_inline_simple_stmt(i);
                    self.write("; ");
                }
                self.print_expr(cond);
                self.write(" ");
                self.print_block(body);
                if let Some(eb) = else_body {
                    self.write(" else ");
                    self.print_else_if_inline(eb);
                }
            }
            other => self.print_stmt(other),
        }
    }

    fn print_inline_simple_stmt(&mut self, s: &GoStmt) {
        // Used inside `if init; cond` — strip leading indent + trailing newline.
        let saved_indent = self.indent;
        self.indent = 0;
        let pre_len = self.out.len();
        self.print_stmt(s);
        // Strip the trailing newline we just emitted.
        if self.out.ends_with('\n') {
            self.out.pop();
        }
        let _ = pre_len;
        self.indent = saved_indent;
    }

    fn print_expr(&mut self, e: &GoExpr) {
        match e {
            GoExpr::Ident(s) => self.write(s),
            GoExpr::Lit(l) => self.print_lit(l),
            GoExpr::Call { fun, args } => {
                self.print_expr(fun);
                self.write("(");
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        self.write(", ");
                    }
                    self.print_expr(a);
                }
                self.write(")");
            }
            GoExpr::Selector { recv, sel } => {
                self.print_expr(recv);
                self.write(".");
                self.write(sel);
            }
            GoExpr::Composite { ty, fields, addr_of } => {
                if *addr_of {
                    self.write("&");
                }
                self.print_type(ty);
                self.write("{");
                if fields.is_empty() {
                    // Empty composite stays on one line: `&T{}` or `T{}`.
                } else {
                    self.newline();
                    self.indent += 1;
                    for (name, expr) in fields {
                        self.write_indent();
                        if let Some(n) = name {
                            self.write(n);
                            self.write(": ");
                        }
                        self.print_expr(expr);
                        self.write(",");
                        self.newline();
                    }
                    self.indent -= 1;
                    self.write_indent();
                }
                self.write("}");
            }
            GoExpr::Star(x) => {
                self.write("*");
                self.print_expr(x);
            }
            GoExpr::AddressOf(x) => {
                self.write("&");
                self.print_expr(x);
            }
            GoExpr::TypeAssert { x, ty, with_ok } => {
                self.print_expr(x);
                self.write(".(");
                self.print_type(ty);
                self.write(")");
                let _ = with_ok; // The ", ok" is rendered by ShortDecl, not here
            }
            GoExpr::TypeExpr(ty) => {
                self.print_type(ty);
            }
            GoExpr::SliceLit { elem_type, elements } => {
                self.write("[]");
                self.print_type(elem_type);
                self.write("{");
                if elements.is_empty() {
                    // Empty slice stays one-line: `[]T{}`
                } else {
                    self.newline();
                    self.indent += 1;
                    for e in elements {
                        self.write_indent();
                        self.print_expr(e);
                        self.write(",");
                        self.newline();
                    }
                    self.indent -= 1;
                    self.write_indent();
                }
                self.write("}");
            }
        }
    }

    fn print_lit(&mut self, l: &GoLit) {
        match l {
            GoLit::Str(s) => {
                self.write("\"");
                self.write(&escape_go_string(s));
                self.write("\"");
            }
            GoLit::Int(i) => self.write(&i.to_string()),
            GoLit::Bool(b) => self.write(if *b { "true" } else { "false" }),
            GoLit::Nil => self.write("nil"),
        }
    }
}

fn format_marker_xvalidation(rule: &str, message: &str) -> String {
    let r = escape_go_string(rule);
    let m = escape_go_string(message);
    format!("+kubebuilder:validation:XValidation:rule=\"{r}\",message=\"{m}\"")
}

/// Escape a Rust string for embedding inside Go's `"..."` syntax.
/// Only handles backslashes and double-quotes — the AST never carries
/// control characters that would need further escaping in our use cases.
fn escape_go_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            other => out.push(other),
        }
    }
    out
}

impl fmt::Display for GoFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&print_file(self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(file: &GoFile) -> String {
        print_file(file)
    }

    #[test]
    fn empty_file_has_header_and_package() {
        let f = GoFile::new("foo");
        let s = render(&f);
        assert!(s.starts_with("// Code generated by iac-forge. DO NOT EDIT.\n"));
        assert!(s.contains("\npackage foo\n"));
    }

    #[test]
    fn imports_grouped_stdlib_then_third_party() {
        let mut f = GoFile::new("foo");
        f.imports.push(GoImport::plain("fmt"));
        f.imports.push(GoImport::plain("github.com/example/x"));
        f.imports.push(GoImport::plain("context"));
        let s = render(&f);
        let stdlib_pos = s.find("\"context\"").unwrap();
        let fmt_pos = s.find("\"fmt\"").unwrap();
        let third_pos = s.find("\"github.com/example/x\"").unwrap();
        assert!(stdlib_pos < fmt_pos);
        assert!(fmt_pos < third_pos);
    }

    #[test]
    fn import_alias_is_emitted() {
        let mut f = GoFile::new("foo");
        f.imports.push(GoImport::aliased("xpv1", "github.com/crossplane/crossplane-runtime/apis/common/v1"));
        let s = render(&f);
        assert!(s.contains("xpv1 \"github.com/crossplane/crossplane-runtime/apis/common/v1\""));
    }

    #[test]
    fn struct_with_json_tags() {
        let mut f = GoFile::new("v1alpha1");
        let field = GoField {
            name: Some("Name".to_string()),
            ty: GoType::named("string"),
            doc: Some("Name of the resource.".to_string()),
            markers: vec![KubeMarker::Required],
            tags: vec![GoStructTag::Json(JsonTag { name: "name".to_string(), ..Default::default() })],
        };
        f.decls.push(GoDecl::Type(GoTypeDecl {
            name: "Foo".to_string(),
            doc: Some("Foo is a thing.".to_string()),
            markers: vec![],
            body: GoTypeBody::Struct(vec![field]),
        }));
        let s = render(&f);
        assert!(s.contains("type Foo struct {"));
        assert!(s.contains("// Foo is a thing."));
        assert!(s.contains("// Name of the resource."));
        assert!(s.contains("// +kubebuilder:validation:Required"));
        assert!(s.contains("Name string `json:\"name\"`"));
    }

    #[test]
    fn embedded_field_is_anonymous() {
        let f = GoField {
            name: None,
            ty: GoType::qualified("xpv1", "ResourceSpec"),
            doc: None,
            markers: vec![],
            tags: vec![GoStructTag::Json(JsonTag { name: String::new(), inline: true, omitempty: false })],
        };
        let mut file = GoFile::new("foo");
        file.decls.push(GoDecl::Type(GoTypeDecl {
            name: "Spec".to_string(),
            doc: None,
            markers: vec![],
            body: GoTypeBody::Struct(vec![f]),
        }));
        let s = render(&file);
        assert!(s.contains("xpv1.ResourceSpec `json:\",inline\"`"));
        // No `xpv1.ResourceSpec xpv1.ResourceSpec` (would be wrong)
        assert!(!s.contains("xpv1.ResourceSpec xpv1.ResourceSpec"));
    }

    #[test]
    fn xvalidation_marker_emits_correct_string() {
        let mut f = GoFile::new("foo");
        let field = GoField {
            name: Some("X".to_string()),
            ty: GoType::named("string"),
            doc: None,
            markers: vec![KubeMarker::XValidationCEL {
                rule: "self == oldSelf".to_string(),
                message: "field is immutable".to_string(),
            }],
            tags: vec![],
        };
        f.decls.push(GoDecl::Type(GoTypeDecl {
            name: "T".to_string(),
            doc: None,
            markers: vec![],
            body: GoTypeBody::Struct(vec![field]),
        }));
        let s = render(&f);
        assert!(s.contains(
            "// +kubebuilder:validation:XValidation:rule=\"self == oldSelf\",message=\"field is immutable\""
        ));
    }

    #[test]
    fn func_decl_with_receiver_returns_and_body() {
        let mut body = GoBlock::new();
        body.push(GoStmt::Return(vec![GoExpr::nil()]));
        let f = GoFuncDecl {
            name: "Close".to_string(),
            doc: Some("Close shuts down the client.".to_string()),
            recv: Some(GoRecv {
                name: "c".to_string(),
                ty: GoType::pointer(GoType::named("Client")),
            }),
            params: vec![],
            returns: vec![GoType::named("error")],
            body,
        };
        let mut file = GoFile::new("client");
        file.decls.push(GoDecl::Func(f));
        let s = render(&file);
        assert!(s.contains("// Close shuts down the client.\n"));
        assert!(s.contains("func (c *Client) Close() error {"));
        assert!(s.contains("\treturn nil"));
        assert!(s.contains("\n}\n"));
    }

    #[test]
    fn type_assert_in_short_decl() {
        let mut body = GoBlock::new();
        body.push(GoStmt::ShortDecl {
            names: vec!["cr".to_string(), "ok".to_string()],
            values: vec![GoExpr::TypeAssert {
                x: Box::new(GoExpr::ident("mg")),
                ty: GoType::pointer(GoType::qualified("v1alpha1", "Foo")),
                with_ok: true,
            }],
        });
        let f = GoFuncDecl {
            name: "F".to_string(),
            doc: None,
            recv: None,
            params: vec![GoParam { name: "mg".to_string(), ty: GoType::named("interface{}") }],
            returns: vec![],
            body,
        };
        let mut file = GoFile::new("p");
        file.decls.push(GoDecl::Func(f));
        let s = render(&file);
        assert!(s.contains("cr, ok := mg.(*v1alpha1.Foo)"));
    }

    #[test]
    fn composite_literal_with_named_fields() {
        let expr = GoExpr::Composite {
            ty: GoType::qualified("akeyless", "AuthMethod"),
            fields: vec![
                (Some("Name".to_string()), GoExpr::str("admin")),
                (Some("AccessID".to_string()), GoExpr::str("p-abc")),
            ],
            addr_of: false,
        };
        let mut body = GoBlock::new();
        body.push(GoStmt::Return(vec![expr]));
        let mut file = GoFile::new("p");
        file.decls.push(GoDecl::Func(GoFuncDecl {
            name: "F".to_string(),
            doc: None,
            recv: None,
            params: vec![],
            returns: vec![GoType::qualified("akeyless", "AuthMethod")],
            body,
        }));
        let s = render(&file);
        assert!(s.contains("return akeyless.AuthMethod{"));
        assert!(s.contains("\t\tName: \"admin\","));
        assert!(s.contains("\t\tAccessID: \"p-abc\","));
    }

    #[test]
    fn package_level_groupname_marker_above_package_decl() {
        let mut f = GoFile::new("v1alpha1");
        f.markers.push(KubeMarker::ObjectGenerate(true));
        f.markers.push(KubeMarker::GroupName("akeyless.crossplane.io".to_string()));
        let s = render(&f);
        // Markers come before package
        let pkg_pos = s.find("package v1alpha1").unwrap();
        let marker_pos = s.find("// +groupName=akeyless.crossplane.io").unwrap();
        assert!(marker_pos < pkg_pos);
    }

    #[test]
    fn deterministic_render_for_identical_ast() {
        let f1 = GoFile::new("foo");
        let f2 = GoFile::new("foo");
        assert_eq!(render(&f1), render(&f2));
    }

    #[test]
    fn escape_quotes_in_string_literal() {
        let mut body = GoBlock::new();
        body.push(GoStmt::Return(vec![GoExpr::str("expected \"quote\"")]));
        let mut file = GoFile::new("p");
        file.decls.push(GoDecl::Func(GoFuncDecl {
            name: "F".to_string(),
            doc: None,
            recv: None,
            params: vec![],
            returns: vec![GoType::named("string")],
            body,
        }));
        let s = render(&file);
        assert!(s.contains("return \"expected \\\"quote\\\"\""));
    }

    #[test]
    fn slice_literal_prints_with_typed_element_type() {
        let lit = GoExpr::SliceLit {
            elem_type: GoType::FuncSignature {
                params: vec![GoType::qualified("ctrl", "Manager"), GoType::qualified("time", "Duration")],
                returns: vec![GoType::named("error")],
            },
            elements: vec![
                GoExpr::Selector {
                    recv: Box::new(GoExpr::ident("foo")),
                    sel: "Setup".to_string(),
                },
                GoExpr::Selector {
                    recv: Box::new(GoExpr::ident("bar")),
                    sel: "Setup".to_string(),
                },
            ],
        };
        let mut body = GoBlock::new();
        body.push(GoStmt::ShortDecl {
            names: vec!["xs".to_string()],
            values: vec![lit],
        });
        let mut file = GoFile::new("p");
        file.decls.push(GoDecl::Func(GoFuncDecl {
            name: "F".to_string(),
            doc: None,
            recv: None,
            params: vec![],
            returns: vec![],
            body,
        }));
        let s = render(&file);
        assert!(s.contains("[]func(ctrl.Manager, time.Duration) error{"));
        assert!(s.contains("\t\tfoo.Setup,"));
        assert!(s.contains("\t\tbar.Setup,"));
    }

    #[test]
    fn for_range_prints_with_two_iter_vars() {
        let mut for_body = GoBlock::new();
        for_body.push(GoStmt::Expr(GoExpr::call(
            GoExpr::ident("use"),
            vec![GoExpr::ident("v")],
        )));
        let stmt = GoStmt::ForRange {
            key: Some("i".to_string()),
            value: Some("v".to_string()),
            range: GoExpr::ident("xs"),
            body: for_body,
        };
        let mut block = GoBlock::new();
        block.push(stmt);
        let mut file = GoFile::new("p");
        file.decls.push(GoDecl::Func(GoFuncDecl {
            name: "F".to_string(),
            doc: None,
            recv: None,
            params: vec![],
            returns: vec![],
            body: block,
        }));
        let s = render(&file);
        assert!(s.contains("for i, v := range xs {"));
    }

    #[test]
    fn for_range_with_only_value_uses_underscore_for_index() {
        let stmt = GoStmt::ForRange {
            key: None,
            value: Some("s".to_string()),
            range: GoExpr::ident("setups"),
            body: GoBlock::new(),
        };
        let mut block = GoBlock::new();
        block.push(stmt);
        let mut file = GoFile::new("p");
        file.decls.push(GoDecl::Func(GoFuncDecl {
            name: "F".to_string(),
            doc: None,
            recv: None,
            params: vec![],
            returns: vec![],
            body: block,
        }));
        let s = render(&file);
        assert!(s.contains("for _, s := range setups {"));
    }

    #[test]
    fn func_signature_type_prints_correctly() {
        let ty = GoType::FuncSignature {
            params: vec![GoType::named("int"), GoType::named("string")],
            returns: vec![GoType::named("error")],
        };
        let mut p = GoPrinter::new();
        p.print_type(&ty);
        let s = p.finish();
        assert_eq!(s, "func(int, string) error");
    }

    #[test]
    fn type_expr_renders_as_bare_type() {
        // Used inside `make([]Foo, n)` and similar builtins.
        let mut body = GoBlock::new();
        body.push(GoStmt::ShortDecl {
            names: vec!["xs".to_string()],
            values: vec![GoExpr::call(
                GoExpr::ident("make"),
                vec![
                    GoExpr::TypeExpr(GoType::slice(GoType::named("Foo"))),
                    GoExpr::ident("n"),
                ],
            )],
        });
        let mut file = GoFile::new("p");
        file.decls.push(GoDecl::Func(GoFuncDecl {
            name: "F".to_string(),
            doc: None,
            recv: None,
            params: vec![],
            returns: vec![],
            body,
        }));
        let s = render(&file);
        assert!(s.contains("make([]Foo, n)"));
        assert!(!s.contains("make([]Foo{}, n)"), "TypeExpr must not emit slice-literal braces");
    }

    #[test]
    fn func_signature_with_multi_return_uses_parens() {
        let ty = GoType::FuncSignature {
            params: vec![GoType::named("int")],
            returns: vec![GoType::named("string"), GoType::named("error")],
        };
        let mut p = GoPrinter::new();
        p.print_type(&ty);
        assert_eq!(p.finish(), "func(int) (string, error)");
    }

    #[test]
    fn print_column_marker_with_priority() {
        let m = KubeMarker::PrintColumn {
            name: "READY".to_string(),
            ty: "string".to_string(),
            json_path: ".status.conditions[?(@.type=='Ready')].status".to_string(),
            priority: Some(1),
        };
        let mut p = GoPrinter::new();
        p.write_markers(&[m]);
        let s = p.finish();
        assert!(s.contains(",priority=1"));
        assert!(s.contains("name=\"READY\""));
        assert!(s.contains("JSONPath=\".status.conditions[?(@.type=='Ready')].status\""));
    }
}
