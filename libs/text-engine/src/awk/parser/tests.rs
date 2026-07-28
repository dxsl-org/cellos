use super::parse;

#[test]
fn parses_filter_and_print() {
    let program = parse("$2 >= 10 { print NR, $1 }").expect("parse");
    assert_eq!(program.print.len(), 2);
    assert!(program.filter.is_some());
}

#[test]
fn keeps_regex_filter_raw() {
    let program = parse("/ERR[0-9]+/ { print $0 }").expect("parse");
    assert!(program.filter.is_some());
}
