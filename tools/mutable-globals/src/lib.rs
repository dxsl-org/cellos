//! Writable-global ownership inventory (ginux-lessons Phase 02).
//!
//! Discovery walks each configured crate's real module tree with `syn` (so
//! comments, strings and `'static` lifetimes are never mistaken for
//! declarations), carries every enclosing `#[cfg]` into the declaration's key,
//! and evaluates it against one target profile. It finds:
//!
//! * `static` and `static mut` items at any depth, including inside function
//!   bodies and `const` blocks;
//! * `extern { static … }` linker symbols;
//! * `macro_rules!` definitions whose body contains a `static` token, and every
//!   invocation of such a macro (the expansion itself is NOT inspected);
//! * item-level macro invocations of any other macro, and `global_asm!`, both of
//!   which can define storage this tool cannot see.
//!
//! This is a discovery and drift aid, not a proof of synchronization or of
//! memory safety. Type names are never trusted to imply interior mutability:
//! only a closed set of syntactically immutable shapes (primitives, `&str`,
//! shared slices/arrays/tuples of those, `fn` pointers) is exempt; every other
//! active declaration needs a reviewed classification or an explicit debt entry.

use proc_macro2::{TokenStream, TokenTree};
use quote::ToTokens;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use syn::punctuated::Punctuated;
use syn::visit::Visit;

// ---------------------------------------------------------------- config

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub profile: Profile,
    #[serde(rename = "crate")]
    pub crates: Vec<CrateCfg>,
    #[serde(default)]
    pub unreached: Vec<Unreached>,
    #[serde(default)]
    pub global: Vec<Entry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    pub target_arch: String,
    pub target_os: String,
    pub target_pointer_width: String,
    /// Bare cfg names known false in this profile (`test`, build-script cfgs).
    #[serde(default)]
    pub cfg_false: Vec<String>,
    /// Bare cfg names known true in this profile.
    #[serde(default)]
    pub cfg_true: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrateCfg {
    pub name: String,
    pub root: String,
    pub manifest: String,
    /// Linked into the profiled image. Declarations of inactive crates are
    /// inventoried but not required to be classified (platform not qualified).
    pub active: bool,
    #[serde(default)]
    pub features: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unreached {
    pub path: String,
    pub reason: String,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub key: String,
    pub lifetime: String,
    pub mutation: String,
    pub owner: String,
    pub contexts: Vec<String>,
    pub seal: String,
    pub retirement: String,
    pub rationale: String,
    pub review: String,
    /// Present for an unresolved existing declaration: why it is not yet
    /// classified. Debt is listed as debt, never as reviewed-safe.
    #[serde(default)]
    pub debt: Option<String>,
}

const LIFETIMES: &[&str] = &["boot", "per-cpu", "per-cell", "per-thread", "global", "unknown"];
/// `none`: no reader or writer is compiled into this profile (dead storage);
/// it must be the only context listed.
const CONTEXTS: &[&str] = &["boot", "normal", "irq", "nmi", "ap", "exception", "none", "unknown"];
const MUTATIONS: &[&str] = &[
    "atomic", "spinlock", "lock-other", "once", "static-mut", "unsafe-cell", "immutable",
    "linker", "hardware-written", "macro", "asm", "none", "unknown",
];

// ---------------------------------------------------------------- cfg

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tri {
    True,
    False,
    Unknown,
}

impl Tri {
    fn and(self, o: Tri) -> Tri {
        match (self, o) {
            (Tri::False, _) | (_, Tri::False) => Tri::False,
            (Tri::True, Tri::True) => Tri::True,
            _ => Tri::Unknown,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Tri::True => "active",
            Tri::False => "inactive",
            Tri::Unknown => "cfg-unknown",
        }
    }
}

struct Env<'a> {
    profile: &'a Profile,
    features: BTreeSet<String>,
}

impl Env<'_> {
    fn eval(&self, meta: &syn::Meta) -> Tri {
        match meta {
            syn::Meta::Path(p) => {
                let name = p.to_token_stream().to_string();
                if self.profile.cfg_true.iter().any(|c| *c == name) {
                    Tri::True
                } else if self.profile.cfg_false.iter().any(|c| *c == name) {
                    Tri::False
                } else {
                    Tri::Unknown
                }
            }
            syn::Meta::NameValue(nv) => {
                let key = nv.path.to_token_stream().to_string();
                let syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. }) = &nv.value else {
                    return Tri::Unknown;
                };
                let v = s.value();
                let known = match key.as_str() {
                    "feature" => Some(self.features.contains(&v)),
                    "target_arch" => Some(v == self.profile.target_arch),
                    "target_os" => Some(v == self.profile.target_os),
                    "target_pointer_width" => Some(v == self.profile.target_pointer_width),
                    _ => None,
                };
                match known {
                    Some(true) => Tri::True,
                    Some(false) => Tri::False,
                    None => Tri::Unknown,
                }
            }
            syn::Meta::List(list) => {
                let name = list.path.to_token_stream().to_string();
                let Ok(items) =
                    list.parse_args_with(Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
                else {
                    return Tri::Unknown;
                };
                let vals: Vec<Tri> = items.iter().map(|m| self.eval(m)).collect();
                match name.as_str() {
                    "all" => vals.iter().fold(Tri::True, |a, b| a.and(*b)),
                    "any" => {
                        if vals.contains(&Tri::True) {
                            Tri::True
                        } else if vals.iter().all(|v| *v == Tri::False) {
                            Tri::False
                        } else {
                            Tri::Unknown
                        }
                    }
                    "not" if vals.len() == 1 => match vals[0] {
                        Tri::True => Tri::False,
                        Tri::False => Tri::True,
                        Tri::Unknown => Tri::Unknown,
                    },
                    _ => Tri::Unknown,
                }
            }
        }
    }
}

/// The `#[cfg(..)]` predicates on an item, as (normalized text, value).
fn cfgs(env: &Env, attrs: &[syn::Attribute]) -> Vec<(String, Tri)> {
    attrs
        .iter()
        .filter(|a| a.path().is_ident("cfg"))
        .map(|a| match a.parse_args::<syn::Meta>() {
            Ok(m) => (m.to_token_stream().to_string(), env.eval(&m)),
            Err(_) => (a.to_token_stream().to_string(), Tri::Unknown),
        })
        .collect()
}

fn path_attr(attrs: &[syn::Attribute]) -> Option<String> {
    attrs.iter().find(|a| a.path().is_ident("path")).and_then(|a| match &a.meta {
        syn::Meta::NameValue(nv) => match &nv.value {
            syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(s), .. }) => Some(s.value()),
            _ => None,
        },
        _ => None,
    })
}

/// Feature closure of `wanted` (with `default` expanded) over the manifest's
/// `[features]` table; `dep:` and `crate/feature` entries are not this crate's.
fn feature_closure(manifest: &Path, wanted: &[String]) -> Result<BTreeSet<String>, String> {
    let text = fs::read_to_string(manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let value: toml::Value = toml::from_str(&text).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let table = value.get("features").and_then(|f| f.as_table()).cloned().unwrap_or_default();
    let mut out = BTreeSet::new();
    let mut work: Vec<String> = wanted.to_vec();
    while let Some(f) = work.pop() {
        if f.starts_with("dep:") || f.contains('/') || !out.insert(f.clone()) {
            continue;
        }
        if let Some(list) = table.get(&f).and_then(|v| v.as_array()) {
            work.extend(list.iter().filter_map(|v| v.as_str().map(str::to_owned)));
        } else if f != "default" {
            return Err(format!("{}: unknown feature `{f}`", manifest.display()));
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- discovery

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Static,
    StaticMut,
    Extern,
    MacroEmitter,
    MacroEmission,
    ItemMacro,
    GlobalAsm,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Static => "static",
            Kind::StaticMut => "static mut",
            Kind::Extern => "extern static",
            Kind::MacroEmitter => "macro emitting static",
            Kind::MacroEmission => "emitter invocation",
            Kind::ItemMacro => "item macro",
            Kind::GlobalAsm => "global_asm",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Decl {
    pub crate_name: String,
    pub key: String,
    pub file: String,
    pub line: usize,
    pub kind: Kind,
    pub ty: String,
    pub active: Tri,
    pub crate_active: bool,
    pub auto_immutable: bool,
    /// Lock rank named by a `Spinlock::ranked(_, …::NAME)` initializer.
    pub rank: Option<String>,
}

impl Decl {
    /// Needs a reviewed classification or debt entry.
    pub fn required(&self) -> bool {
        self.crate_active && self.active != Tri::False && !self.auto_immutable
    }
}

const PRIMS: &[&str] = &[
    "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64", "i128", "isize",
    "bool", "char", "f32", "f64", "str",
];

/// Closed set of syntactically immutable shapes. Anything else (every named
/// type, however innocent its name) is unknown and must be classified.
fn immutable_shape(ty: &syn::Type) -> bool {
    match ty {
        syn::Type::Path(p) => {
            p.qself.is_none()
                && p.path.segments.len() == 1
                && p.path.segments[0].arguments.is_none()
                && PRIMS.contains(&p.path.segments[0].ident.to_string().as_str())
        }
        syn::Type::Reference(r) => r.mutability.is_none() && immutable_shape(&r.elem),
        syn::Type::Array(a) => immutable_shape(&a.elem),
        syn::Type::Slice(s) => immutable_shape(&s.elem),
        syn::Type::Tuple(t) => t.elems.iter().all(immutable_shape),
        syn::Type::BareFn(_) => true,
        syn::Type::Paren(p) => immutable_shape(&p.elem),
        syn::Type::Group(g) => immutable_shape(&g.elem),
        _ => false,
    }
}

/// Whether a macro body contains a `static` item token. `'static` lifetimes
/// are excluded.
fn contains_static(ts: TokenStream) -> bool {
    let mut prev_tick = false;
    for tt in ts {
        match tt {
            TokenTree::Ident(i) => {
                if i == "static" && !prev_tick {
                    return true;
                }
                prev_tick = false;
            }
            TokenTree::Punct(p) => prev_tick = p.as_char() == '\'',
            TokenTree::Group(g) => {
                if contains_static(g.stream()) {
                    return true;
                }
                prev_tick = false;
            }
            TokenTree::Literal(_) => prev_tick = false,
        }
    }
    false
}

/// Item macros that cannot define storage.
const NON_EMITTING: &[&str] = &["compile_error"];

struct PendingMod {
    file: PathBuf,
    mod_dir: PathBuf,
    scope: Vec<String>,
    cfg: Vec<(String, Tri)>,
}

struct FileVisitor<'a, 'e> {
    env: &'a Env<'e>,
    crate_name: &'a str,
    crate_active: bool,
    rel_file: String,
    file_dir: PathBuf,
    mod_dir: PathBuf,
    scope: Vec<String>,
    cfg: Vec<(String, Tri)>,
    emitters: &'a BTreeSet<String>,
    found_emitters: BTreeSet<String>,
    counters: BTreeMap<String, usize>,
    decls: Vec<Decl>,
    pending: Vec<PendingMod>,
    errors: Vec<String>,
}

impl FileVisitor<'_, '_> {
    fn key(&self, item: &str) -> String {
        let mut k = self.crate_name.to_owned();
        for s in &self.scope {
            k.push_str("::");
            k.push_str(s);
        }
        k.push_str("::");
        k.push_str(item);
        if !self.cfg.is_empty() {
            let c: Vec<&str> = self.cfg.iter().map(|(t, _)| t.as_str()).collect();
            let _ = write!(k, " @cfg({})", c.join(" & "));
        }
        k
    }
    fn active(&self) -> Tri {
        self.cfg.iter().fold(Tri::True, |a, (_, v)| a.and(*v))
    }
    fn record(&mut self, item: String, kind: Kind, ty: String, line: usize, auto_immutable: bool) {
        let key = self.key(&item);
        let active = self.active();
        self.decls.push(Decl {
            crate_name: self.crate_name.to_owned(),
            key,
            file: self.rel_file.clone(),
            line,
            kind,
            ty,
            active,
            crate_active: self.crate_active,
            auto_immutable,
            rank: None,
        });
    }
    fn numbered(&mut self, name: &str) -> String {
        let scope_key = format!("{}|{}", self.key(""), name);
        let n = self.counters.entry(scope_key).or_insert(0);
        *n += 1;
        format!("{name}!#{n}")
    }
    fn with_scope<F: FnOnce(&mut Self)>(&mut self, seg: Option<String>, attrs: &[syn::Attribute], f: F) {
        let cfg = cfgs(self.env, attrs);
        let n = cfg.len();
        self.cfg.extend(cfg);
        if let Some(s) = &seg {
            self.scope.push(s.clone());
        }
        f(self);
        if seg.is_some() {
            self.scope.pop();
        }
        self.cfg.truncate(self.cfg.len() - n);
    }
}

fn compact(ts: impl ToTokens) -> String {
    ts.to_token_stream().to_string().replace(' ', "")
}

impl<'ast> Visit<'ast> for FileVisitor<'_, '_> {
    fn visit_item_mod(&mut self, i: &'ast syn::ItemMod) {
        let name = i.ident.to_string();
        if let Some((_, items)) = &i.content {
            let cfg = cfgs(self.env, &i.attrs);
            let n = cfg.len();
            self.cfg.extend(cfg);
            self.scope.push(name.clone());
            let saved = self.mod_dir.clone();
            self.mod_dir = saved.join(&name);
            for it in items {
                self.visit_item(it);
            }
            self.mod_dir = saved;
            self.scope.pop();
            self.cfg.truncate(self.cfg.len() - n);
            return;
        }
        let mut cfg = self.cfg.clone();
        cfg.extend(cfgs(self.env, &i.attrs));
        let mut scope = self.scope.clone();
        scope.push(name.clone());
        let (file, mod_dir) = if let Some(p) = path_attr(&i.attrs) {
            let f = normalize(&self.file_dir.join(p));
            let d = f.parent().map(Path::to_path_buf).unwrap_or_default();
            (f, d)
        } else {
            let a = self.mod_dir.join(format!("{name}.rs"));
            let b = self.mod_dir.join(&name).join("mod.rs");
            if a.exists() {
                (a, self.mod_dir.join(&name))
            } else if b.exists() {
                (b, self.mod_dir.join(&name))
            } else {
                let active = cfg.iter().fold(Tri::True, |x, (_, v)| x.and(*v));
                if active != Tri::False {
                    self.errors.push(format!(
                        "{}: module `{}` has no file ({} or {})",
                        self.rel_file,
                        name,
                        a.display(),
                        b.display()
                    ));
                }
                return;
            }
        };
        self.pending.push(PendingMod { file, mod_dir, scope, cfg });
    }
    fn visit_item_fn(&mut self, i: &'ast syn::ItemFn) {
        self.with_scope(Some(format!("{}()", i.sig.ident)), &i.attrs, |v| syn::visit::visit_item_fn(v, i));
    }
    fn visit_item_impl(&mut self, i: &'ast syn::ItemImpl) {
        let seg = match &i.trait_ {
            Some((_, p, _)) => format!("<impl {} for {}>", compact(p), compact(&i.self_ty)),
            None => format!("<impl {}>", compact(&i.self_ty)),
        };
        self.with_scope(Some(seg), &i.attrs, |v| syn::visit::visit_item_impl(v, i));
    }
    fn visit_impl_item_fn(&mut self, i: &'ast syn::ImplItemFn) {
        self.with_scope(Some(format!("{}()", i.sig.ident)), &i.attrs, |v| syn::visit::visit_impl_item_fn(v, i));
    }
    fn visit_item_trait(&mut self, i: &'ast syn::ItemTrait) {
        self.with_scope(Some(format!("<trait {}>", i.ident)), &i.attrs, |v| syn::visit::visit_item_trait(v, i));
    }
    fn visit_trait_item_fn(&mut self, i: &'ast syn::TraitItemFn) {
        self.with_scope(Some(format!("{}()", i.sig.ident)), &i.attrs, |v| syn::visit::visit_trait_item_fn(v, i));
    }
    fn visit_item_const(&mut self, i: &'ast syn::ItemConst) {
        self.with_scope(Some(format!("<const {}>", i.ident)), &i.attrs, |v| syn::visit::visit_item_const(v, i));
    }
    fn visit_item_static(&mut self, i: &'ast syn::ItemStatic) {
        self.with_scope(None, &i.attrs, |v| {
            let mutable = matches!(i.mutability, syn::StaticMutability::Mut(_));
            let kind = if mutable { Kind::StaticMut } else { Kind::Static };
            let auto = !mutable && immutable_shape(&i.ty);
            v.record(i.ident.to_string(), kind, compact(&i.ty), i.ident.span().start().line, auto);
            if let Some(d) = v.decls.last_mut() {
                d.rank = ranked_initializer(&i.expr);
            }
            v.with_scope(Some(format!("<static {}>", i.ident)), &[], |w| syn::visit::visit_expr(w, &i.expr));
        });
    }
    fn visit_foreign_item_static(&mut self, i: &'ast syn::ForeignItemStatic) {
        self.with_scope(None, &i.attrs, |v| {
            v.record(i.ident.to_string(), Kind::Extern, compact(&i.ty), i.ident.span().start().line, false);
        });
    }
    fn visit_item_macro(&mut self, i: &'ast syn::ItemMacro) {
        let name = i.mac.path.segments.last().map(|s| s.ident.to_string()).unwrap_or_default();
        let line = i.mac.path.segments.last().map(|s| s.ident.span().start().line).unwrap_or(0);
        self.with_scope(None, &i.attrs, |v| {
            if name == "macro_rules" {
                if let Some(id) = &i.ident {
                    if contains_static(i.mac.tokens.clone()) {
                        v.found_emitters.insert(id.to_string());
                        v.record(format!("{id}!"), Kind::MacroEmitter, String::new(), line, false);
                    }
                }
                return;
            }
            if name == "global_asm" {
                let item = v.numbered("global_asm");
                v.record(item, Kind::GlobalAsm, String::new(), line, false);
                return;
            }
            if !v.emitters.contains(&name) && !NON_EMITTING.contains(&name.as_str()) {
                let item = v.numbered(&name);
                v.record(item, Kind::ItemMacro, String::new(), line, false);
            }
            syn::visit::visit_item_macro(v, i);
        });
    }
    fn visit_macro(&mut self, m: &'ast syn::Macro) {
        let name = m.path.segments.last().map(|s| s.ident.to_string()).unwrap_or_default();
        if self.emitters.contains(&name) {
            let line = m.path.segments.last().map(|s| s.ident.span().start().line).unwrap_or(0);
            let item = self.numbered(&name);
            self.record(item, Kind::MacroEmission, String::new(), line, false);
        }
    }
}

pub struct Discovery {
    pub decls: Vec<Decl>,
    pub reached: BTreeSet<String>,
    pub all_files: BTreeSet<String>,
    pub emitters: BTreeSet<String>,
    pub errors: Vec<String>,
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

/// Lexically resolve `..` (a `#[path = "../x.rs"]` module), so a file is keyed
/// by one name however it is reached. `components()` already drops interior
/// `.`; a leading `.` (relative crate root) stays.
fn normalize(p: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in p.components() {
        if c == Component::ParentDir && matches!(out.components().next_back(), Some(Component::Normal(_))) {
            out.pop();
        } else {
            out.push(c);
        }
    }
    out
}

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}

fn discover_pass(root: &Path, cfg: &Config, emitters: &BTreeSet<String>) -> Discovery {
    let mut d = Discovery {
        decls: Vec::new(),
        reached: BTreeSet::new(),
        all_files: BTreeSet::new(),
        emitters: BTreeSet::new(),
        errors: Vec::new(),
    };
    for c in &cfg.crates {
        let features = match feature_closure(&root.join(&c.manifest), &c.features) {
            Ok(f) => f,
            Err(e) => {
                d.errors.push(e);
                continue;
            }
        };
        let env = Env { profile: &cfg.profile, features };
        let root_file = root.join(&c.root);
        let src_dir = root_file.parent().map(Path::to_path_buf).unwrap_or_default();
        let mut files = Vec::new();
        rs_files(&src_dir, &mut files);
        d.all_files.extend(files.iter().map(|f| rel(root, f)));
        let mut work = vec![PendingMod { file: root_file, mod_dir: src_dir, scope: Vec::new(), cfg: Vec::new() }];
        while let Some(m) = work.pop() {
            let rel_file = rel(root, &m.file);
            if !d.reached.insert(format!("{}|{}", rel_file, m.cfg.iter().map(|c| c.0.as_str()).collect::<Vec<_>>().join("&"))) {
                continue;
            }
            d.reached.insert(rel_file.clone());
            let text = match fs::read_to_string(&m.file) {
                Ok(t) => t,
                Err(e) => {
                    d.errors.push(format!("{rel_file}: {e}"));
                    continue;
                }
            };
            let file = match syn::parse_file(&text) {
                Ok(f) => f,
                Err(e) => {
                    d.errors.push(format!("{rel_file}:{}: parse error: {e}", e.span().start().line));
                    continue;
                }
            };
            let mut v = FileVisitor {
                env: &env,
                crate_name: &c.name,
                crate_active: c.active,
                rel_file,
                file_dir: m.file.parent().map(Path::to_path_buf).unwrap_or_default(),
                mod_dir: m.mod_dir,
                scope: m.scope,
                cfg: m.cfg,
                emitters,
                found_emitters: BTreeSet::new(),
                counters: BTreeMap::new(),
                decls: Vec::new(),
                pending: Vec::new(),
                errors: Vec::new(),
            };
            // Inner `#![cfg(..)]` on the file applies to everything in it.
            let inner = cfgs(&env, &file.attrs);
            v.cfg.extend(inner);
            for item in &file.items {
                v.visit_item(item);
            }
            d.decls.extend(v.decls);
            d.errors.extend(v.errors);
            d.emitters.extend(v.found_emitters);
            work.extend(v.pending);
        }
    }
    d.reached.retain(|r| !r.contains('|'));
    // Same key twice (identical cfg chain): disambiguate deterministically.
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for decl in &mut d.decls {
        let n = seen.entry(decl.key.clone()).or_insert(0);
        *n += 1;
        if *n > 1 {
            decl.key = format!("{} #{}", decl.key, n);
        }
    }
    d
}

/// Two passes: the first finds every `static`-emitting `macro_rules!` name so
/// the second can record their invocations wherever they occur.
pub fn discover(root: &Path, cfg: &Config) -> Discovery {
    let first = discover_pass(root, cfg, &BTreeSet::new());
    discover_pass(root, cfg, &first.emitters)
}

/// `NAME` of a `Spinlock::ranked(data, path::NAME)` initializer.
fn ranked_initializer(e: &syn::Expr) -> Option<String> {
    let syn::Expr::Call(call) = e else { return None };
    let syn::Expr::Path(func) = &*call.func else { return None };
    if func.path.segments.last()?.ident != "ranked" || call.args.len() != 2 {
        return None;
    }
    let syn::Expr::Path(rank) = call.args.last()? else { return None };
    Some(rank.path.segments.last()?.ident.to_string())
}

/// Ranks in acquisition order, from the `lock_ranks! { … }` table.
fn rank_table(root: &Path) -> Option<Vec<String>> {
    let text = fs::read_to_string(root.join("kernel/src/sync/rank.rs")).ok()?;
    let start = text.find("lock_ranks! {")? + "lock_ranks! {".len();
    let end = start + text[start..].find('}')?;
    Some(
        text[start..end]
            .lines()
            .map(|l| l.trim().trim_end_matches(','))
            .filter(|l| !l.is_empty() && !l.starts_with("//"))
            .map(str::to_owned)
            .collect(),
    )
}

/// Kernel static spinlocks without a rank, ranks missing from the table, and
/// the generated `lock-ranks.md`.
fn lock_ranks(root: &Path, d: &Discovery, problems: &mut Vec<String>) -> Option<String> {
    let table = rank_table(root)?;
    let locks: Vec<&Decl> = d
        .decls
        .iter()
        .filter(|x| x.crate_name == "kernel" && x.kind == Kind::Static && x.ty.contains("Spinlock<"))
        .collect();
    for x in &locks {
        match &x.rank {
            None => problems.push(format!("static spinlock `{}` ({}:{}) has no lock rank", x.key, x.file, x.line)),
            Some(r) if !table.contains(r) => problems.push(format!("`{}` names unknown lock rank `{r}`", x.key)),
            Some(_) => {}
        }
    }
    let mut o = String::new();
    let _ = writeln!(o, "<!-- Generated by tools/mutable-globals from kernel/src/sync/rank.rs. Do not edit. -->");
    let _ = writeln!(o, "# Kernel lock ranks\n");
    let _ = writeln!(
        o,
        "Acquisition order of every static `Spinlock`, outermost first: a hart may take a ranked lock only while every ranked lock it holds is listed above it. Checked at run time in the test posture (`test-hooks`) and in host tests; dev and release kernels carry no check. Instance locks inside per-object structs are unranked.\n"
    );
    let _ = writeln!(o, "| rank | name | static(s) |");
    let _ = writeln!(o, "|---:|---|---|");
    for (i, name) in table.iter().enumerate() {
        let users: Vec<String> = locks
            .iter()
            .filter(|x| x.rank.as_deref() == Some(name.as_str()))
            .map(|x| format!("`{}` ({}{})", x.key, x.file, if x.active == Tri::False { ", cfg inactive here" } else { "" }))
            .collect();
        let _ = writeln!(o, "| {} | `{}` | {} |", i + 1, name, cell(&users.join("; ")));
    }
    Some(o)
}

// ---------------------------------------------------------------- check

pub struct Outcome {
    pub discovery: Discovery,
    pub problems: Vec<String>,
    pub report: String,
    /// `docs/generated/lock-ranks.md`, when the tree has a rank table.
    pub lock_ranks: Option<String>,
}

fn validate_entry(e: &Entry) -> Vec<String> {
    let mut p = Vec::new();
    let debt = e.debt.as_deref().is_some_and(|d| !d.trim().is_empty());
    if e.debt.is_some() && !debt {
        p.push(format!("{}: empty debt reason", e.key));
    }
    if !LIFETIMES.contains(&e.lifetime.as_str()) {
        p.push(format!("{}: lifetime `{}` not in {:?}", e.key, e.lifetime, LIFETIMES));
    }
    if !MUTATIONS.contains(&e.mutation.as_str()) {
        p.push(format!("{}: mutation `{}` not in {:?}", e.key, e.mutation, MUTATIONS));
    }
    if e.contexts.is_empty() {
        p.push(format!("{}: no execution contexts", e.key));
    }
    if e.contexts.len() > 1 && e.contexts.iter().any(|c| c == "none") {
        p.push(format!("{}: context `none` (no accessor in this profile) cannot be combined with others", e.key));
    }
    for c in &e.contexts {
        if !CONTEXTS.contains(&c.as_str()) {
            p.push(format!("{}: context `{c}` not in {:?}", e.key, CONTEXTS));
        }
    }
    for (name, v) in [("owner", &e.owner), ("seal", &e.seal), ("retirement", &e.retirement), ("rationale", &e.rationale), ("review", &e.review)] {
        if v.trim().is_empty() {
            p.push(format!("{}: empty `{name}`", e.key));
        }
    }
    if e.owner.trim() == "unknown" || e.review.trim() == "unknown" {
        p.push(format!("{}: owner and review are required even for debt", e.key));
    }
    // A reviewed classification may not hide an unknown dimension: that is debt.
    let unknown = e.lifetime == "unknown"
        || e.mutation == "unknown"
        || e.contexts.iter().any(|c| c == "unknown")
        || [&e.seal, &e.retirement, &e.rationale].iter().any(|v| v.trim() == "unknown");
    if unknown && !debt {
        p.push(format!("{}: has an `unknown` dimension but no `debt` reason", e.key));
    }
    p
}

pub fn check(root: &Path, cfg: &Config) -> Outcome {
    let discovery = discover(root, cfg);
    let mut problems = discovery.errors.clone();
    let mut entries: BTreeMap<&str, &Entry> = BTreeMap::new();
    for e in &cfg.global {
        if entries.insert(e.key.as_str(), e).is_some() {
            problems.push(format!("duplicate entry `{}`", e.key));
        }
        problems.extend(validate_entry(e));
    }
    let keys: BTreeSet<&str> = discovery.decls.iter().map(|d| d.key.as_str()).collect();
    for e in &cfg.global {
        if !keys.contains(e.key.as_str()) {
            problems.push(format!("stale entry `{}`: no such declaration", e.key));
        }
    }
    for d in &discovery.decls {
        if d.required() && !entries.contains_key(d.key.as_str()) {
            problems.push(format!(
                "unclassified {} `{}` ({}:{}, {}){}",
                d.kind.label(),
                d.key,
                d.file,
                d.line,
                d.active.label(),
                if d.ty.is_empty() { String::new() } else { format!(": {}", d.ty) }
            ));
        }
    }
    let acked: BTreeMap<&str, &Unreached> = cfg.unreached.iter().map(|u| (u.path.as_str(), u)).collect();
    for f in &discovery.all_files {
        if !discovery.reached.contains(f) && !acked.contains_key(f.as_str()) {
            problems.push(format!("unreached source file `{f}`: not in the module tree and not acknowledged"));
        }
    }
    for u in &cfg.unreached {
        if u.reason.trim().is_empty() {
            problems.push(format!("unreached `{}`: empty reason", u.path));
        }
        if !discovery.all_files.contains(&u.path) || discovery.reached.contains(&u.path) {
            problems.push(format!("stale unreached acknowledgement `{}`", u.path));
        }
    }
    let report = render(cfg, &discovery, &entries);
    let lock_ranks = lock_ranks(root, &discovery, &mut problems);
    Outcome { discovery, problems, report, lock_ranks }
}

fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

fn render(cfg: &Config, d: &Discovery, entries: &BTreeMap<&str, &Entry>) -> String {
    let mut o = String::new();
    let _ = writeln!(o, "<!-- Generated by tools/mutable-globals from scripts/mutable-globals.toml. Do not edit. -->");
    let _ = writeln!(o, "# Writable-global inventory: profile `{}`\n", cfg.profile.name);
    let _ = writeln!(
        o,
        "Discovery and drift aid only. A row records ownership, contexts and lifetime as reviewed; it is not a proof of synchronization, of memory safety, or of anything about dependencies or compiler output. **Debt** rows are unresolved and are not reviewed-safe.\n"
    );
    let _ = writeln!(o, "## Summary\n");
    let _ = writeln!(o, "| crate | linked | declarations | needs review | classified | debt | syntactically immutable | inactive cfg |");
    let _ = writeln!(o, "|---|---|---|---|---|---|---|---|");
    for c in &cfg.crates {
        let ds: Vec<&Decl> = d.decls.iter().filter(|x| x.crate_name == c.name).collect();
        let req: Vec<&&Decl> = ds.iter().filter(|x| x.required()).collect();
        let debt = req.iter().filter(|x| entries.get(x.key.as_str()).is_some_and(|e| e.debt.is_some())).count();
        let classified = req.iter().filter(|x| entries.get(x.key.as_str()).is_some_and(|e| e.debt.is_none())).count();
        let _ = writeln!(
            o,
            "| {} | {} | {} | {} | {} | {} | {} | {} |",
            c.name,
            if c.active { "yes" } else { "no (not qualified)" },
            ds.len(),
            req.len(),
            classified,
            debt,
            ds.iter().filter(|x| x.auto_immutable).count(),
            ds.iter().filter(|x| x.active == Tri::False).count()
        );
    }
    let _ = writeln!(o, "\n## Coverage and exclusions\n");
    let _ = writeln!(o, "- Parsed with `syn` along each crate's module tree (`mod`, `#[path]`, inline modules, file-level `#![cfg]`); comments, strings and `'static` lifetimes cannot produce findings.");
    let _ = writeln!(o, "- `#[cfg]` is evaluated for `target_arch = \"{}\"`, `target_os = \"{}\"`, `target_pointer_width = \"{}\"` and each crate's resolved features. Any other predicate is `cfg-unknown` and is treated as active.", cfg.profile.target_arch, cfg.profile.target_os, cfg.profile.target_pointer_width);
    let _ = writeln!(o, "- Macros are **not expanded**. `macro_rules!` bodies containing a `static` token are listed as emitters ({}), and every invocation of one is listed separately. Every other item-level macro and every `global_asm!` is listed because it may define storage this tool cannot see.", if d.emitters.is_empty() { "none".to_owned() } else { d.emitters.iter().map(|e| format!("`{e}!`")).collect::<Vec<_>>().join(", ") });
    let _ = writeln!(o, "- Only primitives, `&str`, and shared references, slices, arrays and tuples of those, plus `fn` pointers, are exempt as syntactically immutable. No type name is trusted to imply or exclude interior mutability.");
    let _ = writeln!(o, "- Declarations of crates not linked into this profile are inventoried below but not required to be classified; their platforms are not qualified by this report.");
    if !cfg.unreached.is_empty() {
        let _ = writeln!(o, "\nSource files outside the module tree:\n");
        for u in &cfg.unreached {
            let _ = writeln!(o, "- `{}`: {}", u.path, cell(&u.reason));
        }
    }
    let _ = writeln!(o, "\n## Declarations needing review\n");
    let _ = writeln!(o, "| key | file | kind | mutation | lifetime | contexts | owner | seal | retirement | status |");
    let _ = writeln!(o, "|---|---|---|---|---|---|---|---|---|---|");
    let mut req: Vec<&Decl> = d.decls.iter().filter(|x| x.required()).collect();
    req.sort_by(|a, b| a.key.cmp(&b.key));
    for x in req {
        match entries.get(x.key.as_str()) {
            Some(e) => {
                let _ = writeln!(
                    o,
                    "| `{}` | `{}` | {} | {} | {} | {} | {} | {} | {} | {} |",
                    cell(&x.key),
                    x.file,
                    x.kind.label(),
                    e.mutation,
                    e.lifetime,
                    e.contexts.join(", "),
                    cell(&e.owner),
                    cell(&e.seal),
                    cell(&e.retirement),
                    match &e.debt {
                        Some(r) => format!("**debt**: {}", cell(r)),
                        None => format!("reviewed: {}", cell(&e.review)),
                    }
                );
            }
            None => {
                let _ = writeln!(o, "| `{}` | `{}` | {} | | | | | | | **UNCLASSIFIED** |", cell(&x.key), x.file, x.kind.label());
            }
        }
    }
    let other: Vec<&Decl> = d.decls.iter().filter(|x| !x.required()).collect();
    let _ = writeln!(o, "\n## Other declarations ({})\n", other.len());
    let _ = writeln!(o, "| key | file | kind | type | why not reviewed here |");
    let _ = writeln!(o, "|---|---|---|---|---|");
    let mut other = other;
    other.sort_by(|a, b| a.key.cmp(&b.key));
    for x in other {
        let why = if !x.crate_active {
            "crate not linked in this profile"
        } else if x.active == Tri::False {
            "cfg inactive in this profile"
        } else {
            "syntactically immutable"
        };
        let _ = writeln!(o, "| `{}` | `{}` | {} | `{}` | {} |", cell(&x.key), x.file, x.kind.label(), cell(&x.ty), why);
    }
    o
}

pub fn load_config(path: &Path) -> Result<Config, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}
