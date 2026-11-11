// SPDX-License-Identifier: MIT
//! Places sidebar geometry and hit-testing for Files application; the sidebar itself is
//! drawn by ViUI's `PlacesSidebar` (see [`crate::render`]).

use crate::model::PlacesItem;

pub const PLACES_WIDTH: u32 = 160;

pub fn handle_click(places: &[PlacesItem], x: i32, y: i32) -> Option<&'static str> {
    if x < 6 || x >= PLACES_WIDTH as i32 - 6 {
        return None;
    }
    let start_y = 76;
    let row_h = 28;
    if y < start_y {
        return None;
    }
    let idx = ((y - start_y) / row_h) as usize;
    places.get(idx).map(|item| item.path)
}
