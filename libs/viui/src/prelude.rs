//! Common re-exports for ViUI consumers.
//!
//! Add `use viui::prelude::*` to import the most-used types without full paths.

pub use crate::canvas::{Color, FramebufferCanvas, TextStyle, ViCanvas};
pub use crate::elm::{Element, ViApp};
pub use crate::event::{Event, EventStatus, KeyCode, Modifiers, MouseButton};
pub use crate::layout::{Axis, Constraints, LayoutNode, Length, Padding, Point, Rect, Size};
pub use crate::response::Response;
pub use crate::state_store::{FocusManager, WidgetFlags, WidgetState, WidgetStateStore};
pub use crate::theme::{
    theme_for_palette, DarkTheme, KioskTheme, LightTheme, PaperTheme, RobotTheme, SystemPalette,
    ThemePreferenceState, ViTheme, DARK_THEME, LIGHT_THEME, PAPER_THEME,
};
pub use crate::widget::{PaintCx, ViWidget, WidgetId, WidgetTree};
pub use crate::widgets::{MarkdownBlock, MarkdownView, RichTextView, TextSpan};
