use vi_compiler::compile;

fn generate(source: &str) -> String {
    compile(source).expect("source should parse")
}

#[test]
fn unknown_property_reports_widget_property_and_line() {
    let rust = generate("component Broken {\n    Button { typo_property: 7; }\n}");

    assert!(rust.contains(
        "compile_error!(\"vi-compiler: unknown property 'typo_property' on widget 'Button' at line 2\")"
    ));
}

#[test]
fn propertyless_widget_rejects_bindings() {
    let rust = generate("component Broken { Card { elevation: 2; } }");

    assert!(rust.contains("unknown property 'elevation' on widget 'Card'"));
}

#[test]
fn aliases_share_their_widget_property_contract() {
    let valid = generate("component View { Checkbox { checked: true; } }");
    let invalid = generate("component View { Checkbox { value: true; } }");

    assert!(!valid.contains("unknown property"));
    assert!(invalid.contains("unknown property 'value' on widget 'Checkbox'"));
}

#[test]
fn constructor_and_builder_properties_remain_valid() {
    let rust = generate(
        r#"component View {
    VBox { padding: 4; spacing: 2;
        Label { text: "ready"; color: #ffffff; }
        ProgressBar { value: 0.5; color: #00ff00; }
        ListView { items: self.entries; item_height: 20.5; }
    }
}"#,
    );

    assert!(!rust.contains("unknown property"));
    assert!(rust.contains(".color(Color::rgb(0, 255, 0))"));
    assert!(rust.contains(".item_height(20.5_f32)"));
    syn::parse_str::<syn::File>(&rust)
        .unwrap_or_else(|error| panic!("valid properties generated invalid Rust: {error}\n{rust}"));
}

#[test]
fn builder_property_is_rejected_on_the_wrong_widget() {
    let rust = generate("component Broken { Slider { value: 0.5; color: #ffffff; } }");

    assert!(rust.contains("unknown property 'color' on widget 'Slider'"));
    assert!(!rust.contains("Slider::new(signal_slider_0).color("));
}
