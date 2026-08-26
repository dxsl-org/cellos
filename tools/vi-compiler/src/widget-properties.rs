//! Property contracts for widgets supported by the `.vi` code generator.

/// Returns the properties consumed by codegen for a recognized widget.
///
/// `None` means the element has its own diagnostic path (charts or unknown
/// widgets), while `Some(&[])` marks a recognized widget with no DSL bindings.
pub(crate) fn supported_properties(widget: &str) -> Option<&'static [&'static str]> {
    match widget {
        "VerticalLayout" | "VBox" | "Column" | "HorizontalLayout" | "HBox" | "Row" => {
            Some(&["padding", "spacing"])
        }
        "Text" | "Label" => Some(&["text", "color"]),
        "Button" => Some(&["text"]),
        "ProgressBar" | "Progress" => Some(&["value", "color"]),
        "Slider" => Some(&["value"]),
        "CheckBox" | "Checkbox" => Some(&["checked"]),
        "TextInput" | "TextEdit" => Some(&["text"]),
        "ListView" | "List" => Some(&["items", "item_height"]),
        "TouchArea" | "ScrollArea" | "ScrollView" | "Image" | "Card" | "Divider" => Some(&[]),
        "FlexBox" | "HFlex" | "VFlex" => Some(&[
            "gap",
            "padding",
            "wrap",
            "align_content",
            "align_items",
            "justify",
        ]),
        "Space" => Some(&["width", "height"]),
        "Dialog" => Some(&["title", "message"]),
        "DropDown" | "Dropdown" => Some(&["selected"]),
        _ => None,
    }
}
