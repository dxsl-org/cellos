use proc_macro::{TokenStream, TokenTree};

/// Inline `.vi` DSL component declaration.
///
/// Accepts a raw string literal containing ViUI v2 `.vi` syntax and expands
/// it to the equivalent Rust struct + `impl` block — identical output to what
/// `viui-build` generates via `build.rs` + `include!()`.
///
/// # Requirements
/// The calling crate must have `extern crate alloc` in scope; the generated
/// code uses `alloc::format!` and `alloc::vec!`.
///
/// # Example
/// ```ignore
/// use viui::vi_design;
///
/// vi_design!(r#"
/// component Counter {
///     in-out property <int> count: 0;
///     VerticalLayout {
///         Text { text: "Count: \{count}"; color: #ffffff; }
///         Button { text: "Increment"; clicked => { count += 1; } }
///     }
/// }
/// "#);
///
/// let (state, ui) = Counter::build();
/// ```
///
/// # String interpolation
/// Use raw strings (`r#"..."#`) for components containing `\{var}` interpolation.
/// `\{` is not a valid Rust escape sequence inside regular string literals.
#[proc_macro]
pub fn vi_design(input: TokenStream) -> TokenStream {
    let src = extract_string_literal(input);
    let vi_file =
        vi_compiler::compile_str(&src).unwrap_or_else(|e| panic!("vi_design!: parse error: {e}"));
    let rust_src = vi_compiler::codegen::CodeGen::new().generate(&vi_file);
    rust_src
        .parse()
        .unwrap_or_else(|e| panic!("vi_design!: codegen produced invalid Rust: {e}"))
}

fn extract_string_literal(input: TokenStream) -> String {
    let mut iter = input.into_iter();
    let first = iter
        .next()
        .unwrap_or_else(|| panic!("vi_design! expects a string literal argument"));
    if iter.next().is_some() {
        panic!("vi_design! expects exactly one string literal argument");
    }
    let lit_text = match first {
        TokenTree::Literal(lit) => lit.to_string(),
        _ => panic!("vi_design! expects a string literal (use r#\"...\"# for .vi content)"),
    };
    parse_literal_text(&lit_text)
}

fn parse_literal_text(s: &str) -> String {
    let s = s.trim();
    // Raw strings: r"..." r#"..."# r##"..."## etc.
    if let Some(after_r) = s.strip_prefix('r') {
        let hash_count = after_r.chars().take_while(|c| *c == '#').count();
        let hashes = "#".repeat(hash_count);
        let open = format!("r{}\"", hashes);
        let close = format!("\"{}", hashes);
        if s.starts_with(open.as_str()) && s.ends_with(close.as_str()) {
            let inner_start = open.len();
            let inner_end = s.len() - close.len();
            return s[inner_start..inner_end].to_owned();
        }
    }
    // Regular string literal "..." — basic unescape (no \{ support).
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        let inner = &s[1..s.len() - 1];
        return inner
            .replace("\\n", "\n")
            .replace("\\t", "\t")
            .replace("\\r", "\r")
            .replace("\\\\", "\\")
            .replace("\\\"", "\"");
    }
    panic!("vi_design! expects a string literal (use r#\"...\"# for .vi content)");
}
