use super::SedPattern;
use crate::sed::replacement::Replacement;

#[test]
fn regex_replace_supports_matched_expansion() {
    let pattern = SedPattern::compile(r"foo([0-9]+)", '/').expect("pattern compiles");
    let replacement = Replacement::compile(r"<&>", '/').expect("replacement compiles");
    let (out, changed) = pattern
        .replace("foo12 bar", &replacement, false)
        .expect("replace works");
    assert!(changed);
    assert_eq!(out, "<foo12> bar");
}

#[test]
fn literal_replace_honors_global_flag() {
    let pattern = SedPattern::compile(r"a\|b", '|').expect("pattern compiles");
    let replacement = Replacement::compile("x", '|').expect("replacement compiles");
    let (out, changed) = pattern
        .replace("a|b a|b", &replacement, true)
        .expect("replace works");
    assert!(changed);
    assert_eq!(out, "x x");
}
