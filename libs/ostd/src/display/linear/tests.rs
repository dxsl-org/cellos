use super::*;

const W: u32 = 48;
const H: u32 = 40;
const INK: Bgra = [1, 2, 3, 255];

fn buffer() -> alloc::vec::Vec<u8> {
    alloc::vec![0u8; (W * H * 4) as usize]
}

fn inked(pixels: &[u8], x: i64, y: i64) -> bool {
    if x < 0 || y < 0 || x >= i64::from(W) || y >= i64::from(H) {
        return false;
    }
    let at = (y as usize * W as usize + x as usize) * 4;
    pixels[at..at + 4] == INK
}

#[test]
fn quarter_circle_insets_follow_pixel_centres() {
    let insets: alloc::vec::Vec<u32> = (0..7).map(|row| corner_inset(6, row)).collect();
    assert_eq!(insets, [4, 2, 1, 1, 0, 0, 0]);
    assert_eq!(corner_inset(0, 0), 0);
    assert_eq!(corner_inset(1, 0), 0);
    assert_eq!(corner_inset(2, 0), 1);
}

#[test]
fn outline_is_exactly_the_boundary_of_the_fill() {
    let shapes = [
        RoundRect::new(3, 2, 30, 20, Radii::all(6)),
        RoundRect::new(
            1,
            1,
            40,
            30,
            Radii {
                top_left: 0,
                top_right: 6,
                bottom_right: 9,
                bottom_left: 3,
            },
        ),
        RoundRect::new(5, 5, 7, 5, Radii::all(6)),
        RoundRect::new(2, 2, 1, 1, Radii::all(4)),
        RoundRect::new(-4, -3, 20, 12, Radii::all(5)),
    ];
    for shape in shapes {
        let mut fill = buffer();
        Canvas::new(&mut fill, W, H, W as usize * 4).fill_round(&shape, INK);
        let mut stroke = buffer();
        Canvas::new(&mut stroke, W, H, W as usize * 4).stroke_round(&shape, INK);
        for y in 0..i64::from(H) {
            for x in 0..i64::from(W) {
                // Pixels clipped by the buffer edge count as interior neighbours.
                let inside = |nx: i64, ny: i64| {
                    let (rx, ry) = (nx - i64::from(shape.x), ny - i64::from(shape.y));
                    if rx < 0 || ry < 0 || rx >= i64::from(shape.w) || ry >= i64::from(shape.h) {
                        return false;
                    }
                    let (li, ri) = shape.insets(ry as u32);
                    rx >= i64::from(li) && rx < i64::from(shape.w) - i64::from(ri)
                };
                let boundary = inside(x, y)
                    && !(inside(x - 1, y)
                        && inside(x + 1, y)
                        && inside(x, y - 1)
                        && inside(x, y + 1));
                assert_eq!(
                    inked(&fill, x, y),
                    inside(x, y),
                    "fill {shape:?} at {x},{y}"
                );
                assert_eq!(
                    inked(&stroke, x, y),
                    boundary,
                    "stroke {shape:?} at {x},{y}"
                );
            }
        }
    }
}

#[test]
fn clip_limits_every_primitive() {
    let mut pixels = buffer();
    let mut canvas = Canvas::new(&mut pixels, W, H, W as usize * 4).clip_to(10, 10, 5, 5);
    canvas.fill_rect(0, 0, W, H, INK);
    canvas.line(0, 0, 47, 39, INK);
    canvas.stroke_round(&RoundRect::new(0, 0, W, H, Radii::all(6)), INK);
    for y in 0..i64::from(H) {
        for x in 0..i64::from(W) {
            assert_eq!(
                inked(&pixels, x, y),
                (10..15).contains(&x) && (10..15).contains(&y)
            );
        }
    }
}

#[test]
fn lines_are_one_pixel_and_include_both_ends() {
    let mut pixels = buffer();
    Canvas::new(&mut pixels, W, H, W as usize * 4).line(2, 3, 12, 8, INK);
    let count = pixels.chunks_exact(4).filter(|px| *px == INK).count();
    assert_eq!(count, 11, "one pixel per major-axis step");
    assert!(inked(&pixels, 2, 3) && inked(&pixels, 12, 8));
}

#[test]
fn drop_shadow_darkens_smoothly_with_distance_and_spares_the_caster() {
    let caster = RoundRect::new(12, 8, 20, 14, Radii::all(4));
    let shadow = Shadow {
        color: [0, 0, 0, 255],
        opacity: 200,
        spread: 12,
        offset_x: 0,
        offset_y: 0,
    };
    let mut pixels = alloc::vec![200u8; (W * H * 4) as usize];
    Canvas::new(&mut pixels, W, H, W as usize * 4).drop_shadow(&[caster], &shadow);
    let blue = |x: i64, y: i64| pixels[(y as usize * W as usize + x as usize) * 4];

    // Below the straight bottom edge (row 21), one column: strictly lighter
    // with every pixel of distance, darkest next to the caster, untouched at
    // and beyond `spread`.
    let column: alloc::vec::Vec<u8> = (1..=13).map(|d| blue(20, 21 + d)).collect();
    assert!(
        column[0] < 120,
        "contact shadow is clearly visible: {column:?}"
    );
    assert!(
        column.windows(2).take(11).all(|p| p[0] < p[1]),
        "smooth fade: {column:?}"
    );
    assert_eq!(column[12], 200, "nothing beyond spread");
    // Same distance, same darkness along the edge: no dither pattern.
    assert!((16..28).all(|x| blue(x, 24) == blue(20, 24)));
    for y in 8..22 {
        for x in 12..32 {
            let (li, ri) = caster.insets((y - 8) as u32);
            if x - 12 >= i64::from(li) && x < 32 - i64::from(ri) {
                assert_eq!(blue(x, y), 200, "caster pixel {x},{y} untouched");
            }
        }
    }
}
