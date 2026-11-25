// SPDX-License-Identifier: MIT
use super::*;

const GEOM: Geometry = Geometry {
    cell_w: 9.0,
    cell_h: 18.0,
    cols: 10,
    rows: 3,
};

fn cell(col: usize, row: usize) -> Cell {
    Cell { row, col }
}

#[test]
fn pixels_map_to_cells_inside_the_padded_grid_only() {
    assert_eq!(GEOM.cell_at(8, 6), Some(cell(0, 0)));
    assert_eq!(GEOM.cell_at(16, 23), Some(cell(0, 0)));
    assert_eq!(GEOM.cell_at(8 + 9 * 2 + 4, 6 + 18 + 1), Some(cell(2, 1)));
    assert_eq!(GEOM.cell_at(7, 6), None);
    assert_eq!(GEOM.cell_at(8, 5), None);
    assert_eq!(GEOM.cell_at(-50, -50), None);
    assert_eq!(GEOM.cell_at(8 + 9 * 10, 6), None);
    assert_eq!(GEOM.cell_at(8, 6 + 18 * 3), None);
    // A drag past any edge stops at the edge.
    assert_eq!(GEOM.clamped_cell(-50, -50), cell(0, 0));
    assert_eq!(GEOM.clamped_cell(5000, 5000), cell(9, 2));
    assert_eq!(GEOM.clamped_cell(8 + 9 * 4, 5000), cell(4, 2));
}

#[test]
fn a_backwards_drag_selects_the_same_cells_in_reading_order() {
    let forward = Selection {
        anchor: cell(7, 0),
        head: cell(2, 1),
    };
    let backward = Selection {
        anchor: cell(2, 1),
        head: cell(7, 0),
    };
    assert_eq!(forward.bounds(), (cell(7, 0), cell(2, 1)));
    assert_eq!(backward.bounds(), forward.bounds());
    for sel in [forward, backward] {
        assert!(sel.contains(7, 0) && sel.contains(9, 0));
        assert!(sel.contains(0, 1) && sel.contains(2, 1));
        assert!(!sel.contains(6, 0) && !sel.contains(3, 1) && !sel.contains(0, 2));
    }
}

#[test]
fn text_joins_rows_trims_trailing_spaces_and_skips_trailing_halves() {
    let mut grid = TerminalGrid::new(10, 3);
    grid.write_str("ab  cd\r\nx\u{4e16}y  \r\n  z");
    let all = Selection {
        anchor: cell(9, 2),
        head: cell(0, 0),
    };
    assert_eq!(all.text(&grid), "ab  cd\nx\u{4e16}y\n  z");
    // Partial first and last rows; a blank-tail row becomes an empty line.
    let mut blank = TerminalGrid::new(10, 3);
    blank.write_str("hello\r\n\r\nworld");
    let partial = Selection {
        anchor: cell(3, 0),
        head: cell(2, 2),
    };
    assert_eq!(partial.text(&blank), "lo\n\nwor");
    // Ending on a wide character's trailing half copies it once.
    let wide = Selection {
        anchor: cell(1, 1),
        head: cell(2, 1),
    };
    assert_eq!(wide.text(&grid), "\u{4e16}");
}

#[test]
fn a_drag_shows_a_selection_and_a_click_clears_it() {
    let mut sel = Selector::default();
    // No pointer position yet: a press selects nothing.
    assert!(!sel.on_press(GEOM));
    sel.on_release();
    assert!(sel.selection().is_none());

    assert!(!sel.on_move(8, 6, GEOM));
    assert!(!sel.on_press(GEOM));
    // Movement inside the anchor cell shows nothing.
    assert!(!sel.on_move(10, 8, GEOM));
    assert!(sel.selection().is_none());
    assert!(sel.on_move(8 + 9 * 3, 6 + 18, GEOM));
    assert_eq!(
        sel.selection().map(Selection::bounds),
        Some((cell(0, 0), cell(3, 1)))
    );
    sel.on_release();
    // Moves after the release do not extend it.
    assert!(!sel.on_move(8 + 9 * 5, 6 + 18 * 2, GEOM));
    assert_eq!(sel.selection().map(|s| s.head), Some(cell(3, 1)));

    // A click without movement clears the shown selection.
    assert!(sel.on_press(GEOM));
    sel.on_release();
    assert!(sel.selection().is_none());

    // A press outside the grid starts nothing.
    sel.on_move(0, 0, GEOM);
    assert!(!sel.on_press(GEOM));
    assert!(!sel.on_move(8 + 9 * 3, 6, GEOM));
    assert!(sel.selection().is_none());
}
