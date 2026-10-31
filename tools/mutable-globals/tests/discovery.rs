use mutable_globals::{check, Config, Tri};
use std::fs;
use std::path::{Path, PathBuf};

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("mutable-globals-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("k/src")).unwrap();
        fs::write(
            root.join("k/Cargo.toml"),
            "[package]\nname = \"k\"\n[features]\ndefault = [\"base\"]\nbase = [\"extra\"]\nextra = []\nother = []\n",
        )
        .unwrap();
        Fixture { root }
    }
    fn file(&self, rel: &str, text: &str) -> &Self {
        let p = self.root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
        self
    }
    fn config(&self, extra: &str) -> Config {
        toml::from_str(&format!(
            "[profile]\nname = \"t\"\ntarget_arch = \"x86_64\"\ntarget_os = \"none\"\ntarget_pointer_width = \"64\"\ncfg_false = [\"test\"]\n\
             [[crate]]\nname = \"k\"\nroot = \"k/src/lib.rs\"\nmanifest = \"k/Cargo.toml\"\nactive = true\nfeatures = [\"default\"]\n{extra}"
        ))
        .unwrap()
    }
    fn root(&self) -> &Path {
        &self.root
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn entry(key: &str) -> String {
    format!(
        "[[global]]\nkey = \"{}\"\nlifetime = \"global\"\nmutation = \"atomic\"\nowner = \"k::a\"\ncontexts = [\"normal\"]\n\
         seal = \"none\"\nretirement = \"never\"\nrationale = \"single writer\"\nreview = \"k/src/a.rs:f\"\n",
        key.replace('\\', "\\\\").replace('"', "\\\"")
    )
}

fn required_keys(f: &Fixture, extra: &str) -> Vec<String> {
    let out = check(f.root(), &f.config(extra));
    let mut keys: Vec<String> = out.discovery.decls.iter().filter(|d| d.required()).map(|d| d.key.clone()).collect();
    keys.sort();
    keys
}

#[test]
fn walks_the_real_module_tree_and_ignores_comments_strings_and_lifetimes() {
    let f = Fixture::new("tree");
    f.file(
        "k/src/lib.rs",
        "mod a;\nmod b;\n#[path = \"elsewhere/c_impl.rs\"] mod c;\n\
         mod inline { mod deep; pub static IN: Wrapper = Wrapper; }\n\
         // static COMMENTED: Wrapper = Wrapper;\n\
         const S: &str = \"static IN_STRING: Wrapper\";\n\
         fn f(_: &'static str) {}\n",
    )
    .file("k/src/a.rs", "pub static A: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);\n")
    .file("k/src/b/mod.rs", "mod nested;\n")
    .file("k/src/b/nested.rs", "static mut RAW: u64 = 0;\n")
    .file("k/src/elsewhere/c_impl.rs", "static C: Wrapper = Wrapper;\n")
    .file("k/src/inline/deep.rs", "static D: Wrapper = Wrapper;\n");
    assert_eq!(
        required_keys(&f, ""),
        ["k::a::A", "k::b::nested::RAW", "k::c::C", "k::inline::IN", "k::inline::deep::D"]
    );
}

#[test]
fn cfg_variants_get_distinct_keys_and_only_active_or_undecidable_ones_are_required() {
    let f = Fixture::new("cfg");
    f.file(
        "k/src/lib.rs",
        "#[cfg(target_arch = \"x86_64\")] static V: W = W;\n\
         #[cfg(target_arch = \"riscv64\")] static V: W = W;\n\
         #[cfg(feature = \"extra\")] static IMPLIED: W = W;\n\
         #[cfg(feature = \"other\")] static OFF: W = W;\n\
         #[cfg(test)] static TEST_ONLY: W = W;\n\
         #[cfg(some_build_script_cfg)] static UNDECIDABLE: W = W;\n\
         #[cfg(not(any(test, target_arch = \"riscv64\")))] mod m { static M: W = W; }\n",
    );
    let out = check(f.root(), &f.config(""));
    let by = |k: &str| out.discovery.decls.iter().find(|d| d.key == k).unwrap_or_else(|| panic!("missing {k}"));
    assert_eq!(by("k::V @cfg(target_arch = \"x86_64\")").active, Tri::True);
    assert_eq!(by("k::V @cfg(target_arch = \"riscv64\")").active, Tri::False);
    assert_eq!(by("k::IMPLIED @cfg(feature = \"extra\")").active, Tri::True, "default -> base -> extra");
    assert!(!by("k::OFF @cfg(feature = \"other\")").required());
    assert!(!by("k::TEST_ONLY @cfg(test)").required());
    let undecidable = by("k::UNDECIDABLE @cfg(some_build_script_cfg)");
    assert_eq!(undecidable.active, Tri::Unknown);
    assert!(undecidable.required(), "an undecidable cfg is never silently skipped");
    assert!(by("k::m::M @cfg(not (any (test , target_arch = \"riscv64\")))").required());
}

#[test]
fn only_syntactically_immutable_shapes_are_exempt() {
    let f = Fixture::new("shapes");
    f.file(
        "k/src/lib.rs",
        "static N: u32 = 1;\nstatic S: &str = \"x\";\nstatic T: &[(&str, u8)] = &[];\nstatic A: [u64; 4] = [0; 4];\n\
         static F: fn() = g;\nfn g() {}\n\
         static PLAIN_LOOKING: Config = Config;\nstatic CELL: MyCell<u32> = MyCell::new();\nstatic R: &MyCell<u8> = &X;\n\
         fn body() { static IN_FN: Counter = Counter; }\n\
         impl Foo { fn m() { static IN_METHOD: Counter = Counter; } }\n",
    );
    assert_eq!(
        required_keys(&f, ""),
        ["k::<impl Foo>::m()::IN_METHOD", "k::CELL", "k::PLAIN_LOOKING", "k::R", "k::body()::IN_FN"]
    );
}

#[test]
fn macros_extern_statics_and_global_asm_are_listed_not_expanded() {
    let f = Fixture::new("macros");
    f.file(
        "k/src/lib.rs",
        "macro_rules! slot { ($n:ident) => { static $n: W = W; }; }\n\
         macro_rules! harmless { ($x:expr) => { $x + 1 }; }\n\
         macro_rules! lifetimes_only { () => { fn f(_: &'static str) {} }; }\n\
         slot!(ONE);\nfn g() { slot!(TWO); let _ = harmless!(1); }\n\
         some_external_macro! { anything }\n\
         compile_error_free!();\n\
         core::arch::global_asm!(\".data\\nX: .quad 0\");\n\
         extern \"C\" { static __bss_start: u8; }\n",
    );
    assert_eq!(
        required_keys(&f, ""),
        [
            "k::__bss_start",
            "k::compile_error_free!#1",
            "k::g()::slot!#1",
            "k::global_asm!#1",
            "k::slot!",
            "k::slot!#1",
            "k::some_external_macro!#1",
        ]
    );
}

#[test]
fn an_unclassified_declaration_fails_until_classified_and_a_stale_entry_fails() {
    let f = Fixture::new("gate");
    f.file("k/src/lib.rs", "mod a;\n").file("k/src/a.rs", "pub static NEW: Thing = Thing;\n");
    let out = check(f.root(), &f.config(""));
    assert!(out.problems.iter().any(|p| p.starts_with("unclassified static `k::a::NEW`")), "{:?}", out.problems);

    let out = check(f.root(), &f.config(&entry("k::a::NEW")));
    assert!(out.problems.is_empty(), "{:?}", out.problems);
    assert!(out.report.contains("| `k::a::NEW` |") && out.report.contains("reviewed: k/src/a.rs:f"));

    f.file("k/src/a.rs", "pub static RENAMED: Thing = Thing;\n");
    let out = check(f.root(), &f.config(&entry("k::a::NEW")));
    assert!(out.problems.iter().any(|p| p == "stale entry `k::a::NEW`: no such declaration"), "{:?}", out.problems);
    assert!(out.problems.iter().any(|p| p.starts_with("unclassified static `k::a::RENAMED`")));
}

#[test]
fn unknown_dimensions_need_a_debt_reason_and_debt_is_reported_as_debt() {
    let f = Fixture::new("debt");
    f.file("k/src/lib.rs", "static X: Thing = Thing;\n");
    let unknown = entry("k::X").replace("lifetime = \"global\"", "lifetime = \"unknown\"");
    let out = check(f.root(), &f.config(&unknown));
    assert!(out.problems.iter().any(|p| p.contains("`unknown` dimension but no `debt`")), "{:?}", out.problems);

    let debt = format!("{unknown}debt = \"writer not yet traced\"\n");
    let out = check(f.root(), &f.config(&debt));
    assert!(out.problems.is_empty(), "{:?}", out.problems);
    assert!(out.report.contains("**debt**: writer not yet traced"));

    let bad_vocab = entry("k::X").replace("contexts = [\"normal\"]", "contexts = [\"softirq\"]");
    let out = check(f.root(), &f.config(&bad_vocab));
    assert!(out.problems.iter().any(|p| p.contains("context `softirq`")));

    let dead_and_live = entry("k::X").replace("contexts = [\"normal\"]", "contexts = [\"none\", \"irq\"]");
    let out = check(f.root(), &f.config(&dead_and_live));
    assert!(out.problems.iter().any(|p| p.contains("context `none`")), "{:?}", out.problems);
}

#[test]
fn unreached_source_files_must_be_acknowledged_and_acknowledgements_must_stay_true() {
    let f = Fixture::new("unreached");
    f.file("k/src/lib.rs", "mod a;\n").file("k/src/a.rs", "").file("k/src/orphan.rs", "static HIDDEN: T = T;\n");
    let out = check(f.root(), &f.config(""));
    assert!(out.problems.iter().any(|p| p.starts_with("unreached source file `k/src/orphan.rs`")));

    let ack = "[[unreached]]\npath = \"k/src/orphan.rs\"\nreason = \"not compiled\"\n";
    assert!(check(f.root(), &f.config(ack)).problems.is_empty());

    f.file("k/src/lib.rs", "mod a;\nmod orphan;\n");
    let out = check(f.root(), &f.config(&format!("{ack}{}", entry("k::orphan::HIDDEN"))));
    assert_eq!(out.problems, ["stale unreached acknowledgement `k/src/orphan.rs`"]);
}
