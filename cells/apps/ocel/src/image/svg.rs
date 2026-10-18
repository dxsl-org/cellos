// SPDX-License-Identifier: MIT
//! Super-compact flat-path manual SVG rasterizer for Ocel (desktop profile only).

extern crate alloc;
use alloc::vec::Vec;
use super::bmp::DecodedImage;

const MAX_SVG_DIMENSION: usize = 2048;
const MAX_EDGES_PER_SHAPE: usize = 8192;

#[derive(Clone, Copy, Debug)]
struct Point {
    x: f32,
    y: f32,
}

#[derive(Clone, Copy, Debug)]
struct Edge {
    p1: Point,
    p2: Point,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Color {
    b: u8,
    g: u8,
    r: u8,
    a: u8,
}

impl Color {
    const TRANSPARENT: Self = Self { b: 0, g: 0, r: 0, a: 0 };
    const BLACK: Self = Self { b: 0, g: 0, r: 0, a: 255 };

    fn parse(val: &str) -> Option<Self> {
        let val = val.trim();
        if val.eq_ignore_ascii_case("none") || val.eq_ignore_ascii_case("transparent") {
            return Some(Self::TRANSPARENT);
        }
        if let Some(hex) = val.strip_prefix('#') {
            if hex.len() == 6 {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                return Some(Self { b, g, r, a: 255 });
            } else if hex.len() == 3 {
                let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
                let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
                let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
                return Some(Self { b, g, r, a: 255 });
            }
        }
        if val.eq_ignore_ascii_case("black") { return Some(Self::BLACK); }
        if val.eq_ignore_ascii_case("white") { return Some(Self { b: 255, g: 255, r: 255, a: 255 }); }
        if val.eq_ignore_ascii_case("red") { return Some(Self { b: 0, g: 0, r: 255, a: 255 }); }
        if val.eq_ignore_ascii_case("green") { return Some(Self { b: 0, g: 128, r: 0, a: 255 }); }
        if val.eq_ignore_ascii_case("blue") { return Some(Self { b: 255, g: 0, r: 0, a: 255 }); }
        None
    }
}

pub fn svg_dimensions(data: &[u8]) -> Option<(usize, usize)> {
    let header_len = data.len().min(2048);
    let header = core::str::from_utf8(&data[..header_len]).ok()?;
    let svg_idx = header.find("<svg")?;
    let tag_end = header[svg_idx..].find('>').map(|i| svg_idx + i).unwrap_or(header_len);
    let tag = &header[svg_idx..tag_end];

    let w_val = extract_attribute(tag, "width").and_then(parse_number);
    let h_val = extract_attribute(tag, "height").and_then(parse_number);
    let vb = extract_attribute(tag, "viewBox").and_then(parse_viewbox);

    let (w, h) = match (w_val, h_val, vb) {
        (Some(w), Some(h), _) => (w, h),
        (_, _, Some((_, _, vw, vh))) if vw > 0.0 && vh > 0.0 => (vw, vh),
        (Some(w), None, _) => (w, w * 0.75),
        (None, Some(h), _) => (h * 1.33, h),
        _ => (300.0, 150.0),
    };

    let w = (w as usize).clamp(1, MAX_SVG_DIMENSION);
    let h = (h as usize).clamp(1, MAX_SVG_DIMENSION);
    Some((w, h))
}

pub fn decode_svg(data: &[u8]) -> Option<DecodedImage> {
    let (width, height) = svg_dimensions(data)?;
    let content = core::str::from_utf8(data).ok()?;
    let svg_idx = content.find("<svg")?;
    let tag_end = content[svg_idx..].find('>').map(|i| svg_idx + i)?;
    let svg_tag = &content[svg_idx..tag_end];

    let vb = extract_attribute(svg_tag, "viewBox").and_then(parse_viewbox)
        .unwrap_or((0.0, 0.0, width as f32, height as f32));
    let scale_x = if vb.2 > 0.0 { width as f32 / vb.2 } else { 1.0 };
    let scale_y = if vb.3 > 0.0 { height as f32 / vb.3 } else { 1.0 };

    let mut pixels = alloc::vec![0u8; width * height * 4];
    let body = &content[tag_end + 1..];

    // Simple tag iterator
    let mut rest = body;
    while let Some(open) = rest.find('<') {
        rest = &rest[open + 1..];
        if rest.starts_with('/') || rest.starts_with('!') || rest.starts_with('?') {
            continue;
        }
        let Some(close) = rest.find('>') else { break; };
        let tag = &rest[..close];
        rest = &rest[close + 1..];

        let tag_name = tag.split_whitespace().next().unwrap_or("").trim_end_matches('/');
        let fill = extract_attribute(tag, "fill")
            .and_then(Color::parse)
            .unwrap_or(Color::BLACK);
        if fill.a == 0 {
            continue;
        }

        let mut edges = Vec::new();
        if tag_name.eq_ignore_ascii_case("path") {
            if let Some(d) = extract_attribute(tag, "d") {
                parse_path(&d, vb.0, vb.1, scale_x, scale_y, &mut edges);
            }
        } else if tag_name.eq_ignore_ascii_case("rect") {
            let rx = extract_attribute(tag, "x").and_then(parse_number).unwrap_or(0.0);
            let ry = extract_attribute(tag, "y").and_then(parse_number).unwrap_or(0.0);
            if let (Some(rw), Some(rh)) = (extract_attribute(tag, "width").and_then(parse_number), extract_attribute(tag, "height").and_then(parse_number)) {
                let x0 = (rx - vb.0) * scale_x;
                let y0 = (ry - vb.1) * scale_y;
                let x1 = x0 + rw * scale_x;
                let y1 = y0 + rh * scale_y;
                edges.push(Edge { p1: Point { x: x0, y: y0 }, p2: Point { x: x1, y: y0 } });
                edges.push(Edge { p1: Point { x: x1, y: y0 }, p2: Point { x: x1, y: y1 } });
                edges.push(Edge { p1: Point { x: x1, y: y1 }, p2: Point { x: x0, y: y1 } });
                edges.push(Edge { p1: Point { x: x0, y: y1 }, p2: Point { x: x0, y: y0 } });
            }
        } else if tag_name.eq_ignore_ascii_case("circle") {
            let cx = extract_attribute(tag, "cx").and_then(parse_number).unwrap_or(0.0);
            let cy = extract_attribute(tag, "cy").and_then(parse_number).unwrap_or(0.0);
            if let Some(r) = extract_attribute(tag, "r").and_then(parse_number) {
                // Approximate circle with 16 segments
                let mut prev = Point {
                    x: (cx + r - vb.0) * scale_x,
                    y: (cy - vb.1) * scale_y,
                };
                let first = prev;
                for step in 1..16 {
                    let angle = (step as f32) * (core::f32::consts::TAU / 16.0);
                    // Approximate sin/cos with small lookup or simple math
                    let cos_a = cos_approx(angle);
                    let sin_a = sin_approx(angle);
                    let pt = Point {
                        x: (cx + r * cos_a - vb.0) * scale_x,
                        y: (cy + r * sin_a - vb.1) * scale_y,
                    };
                    edges.push(Edge { p1: prev, p2: pt });
                    prev = pt;
                }
                edges.push(Edge { p1: prev, p2: first });
            }
        }

        if !edges.is_empty() {
            rasterize_edges(&edges, width, height, fill, &mut pixels);
        }
    }

    Some(DecodedImage {
        width: width as u32,
        height: height as u32,
        pixels,
    })
}

fn rasterize_edges(edges: &[Edge], width: usize, height: usize, color: Color, pixels: &mut [u8]) {
    if edges.is_empty() { return; }
    let mut min_y = height as f32;
    let mut max_y = 0.0f32;
    for e in edges {
        min_y = min_f32(min_y, min_f32(e.p1.y, e.p2.y));
        max_y = max_f32(max_y, max_f32(e.p1.y, e.p2.y));
    }
    let y_start = (min_y as i32).max(0) as usize;
    let y_end = ((max_y + 1.0) as usize).min(height);

    let mut intersections = Vec::with_capacity(32);
    for y in y_start..y_end {
        let scan_y = y as f32 + 0.5;
        intersections.clear();
        for e in edges {
            let (y1, y2) = (e.p1.y, e.p2.y);
            if (y1 <= scan_y && scan_y < y2) || (y2 <= scan_y && scan_y < y1) {
                let x = e.p1.x + (scan_y - y1) * (e.p2.x - e.p1.x) / (y2 - y1);
                intersections.push(x);
            }
        }
        intersections.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));

        let mut idx = 0;
        while idx + 1 < intersections.len() {
            let x0 = ((intersections[idx] + 0.5) as i32).max(0) as usize;
            let x1 = (((intersections[idx + 1] + 0.5) as i32).max(0) as usize).min(width);
            if x0 < x1 {
                for x in x0..x1 {
                    let p_idx = (y * width + x) * 4;
                    if p_idx + 4 <= pixels.len() {
                        if color.a == 255 {
                            pixels[p_idx] = color.b;
                            pixels[p_idx + 1] = color.g;
                            pixels[p_idx + 2] = color.r;
                            pixels[p_idx + 3] = 255;
                        } else {
                            let a = color.a as u32;
                            let inv = 255 - a;
                            pixels[p_idx] = ((color.b as u32 * a + pixels[p_idx] as u32 * inv) / 255) as u8;
                            pixels[p_idx + 1] = ((color.g as u32 * a + pixels[p_idx + 1] as u32 * inv) / 255) as u8;
                            pixels[p_idx + 2] = ((color.r as u32 * a + pixels[p_idx + 2] as u32 * inv) / 255) as u8;
                            pixels[p_idx + 3] = 255;
                        }
                    }
                }
            }
            idx += 2;
        }
    }
}

fn parse_path(d: &str, vb_x: f32, vb_y: f32, sx: f32, sy: f32, edges: &mut Vec<Edge>) {
    let mut tokens = PathTokenizer::new(d);
    let mut curr = Point { x: 0.0, y: 0.0 };
    let mut start = curr;
    let mut cmd = 'M';

    while let Some(tok) = tokens.next_cmd_or_num() {
        match tok {
            Token::Cmd(c) => {
                cmd = c;
                if matches!(cmd, 'Z' | 'z') {
                    if (curr.x - start.x).abs() > 0.01 || (curr.y - start.y).abs() > 0.01 {
                        edges.push(Edge { p1: curr, p2: start });
                        curr = start;
                    }
                    continue;
                }
            }
            Token::Num(first) => {
                tokens.put_back(first);
                match cmd {
                    'M' | 'm' => {
                        let rel = cmd == 'm';
                        let ox = if rel { curr.x } else { 0.0 };
                        let oy = if rel { curr.y } else { 0.0 };
                        let Some(x) = tokens.next_num() else { break; };
                        let Some(y) = tokens.next_num() else { break; };
                        curr = Point {
                            x: (ox + x - vb_x) * sx,
                            y: (oy + y - vb_y) * sy,
                        };
                        start = curr;
                        cmd = if rel { 'l' } else { 'L' };
                    }
                    'L' | 'l' => {
                        let rel = cmd == 'l';
                        let ox = if rel { curr.x } else { 0.0 };
                        let oy = if rel { curr.y } else { 0.0 };
                        let Some(x) = tokens.next_num() else { break; };
                        let Some(y) = tokens.next_num() else { break; };
                        let next = Point {
                            x: (ox + x - vb_x) * sx,
                            y: (oy + y - vb_y) * sy,
                        };
                        if edges.len() < MAX_EDGES_PER_SHAPE {
                            edges.push(Edge { p1: curr, p2: next });
                        }
                        curr = next;
                    }
                    'H' | 'h' => {
                        let rel = cmd == 'h';
                        let ox = if rel { curr.x } else { 0.0 };
                        let Some(x) = tokens.next_num() else { break; };
                        let next = Point {
                            x: (ox + x - vb_x) * sx,
                            y: curr.y,
                        };
                        if edges.len() < MAX_EDGES_PER_SHAPE {
                            edges.push(Edge { p1: curr, p2: next });
                        }
                        curr = next;
                    }
                    'V' | 'v' => {
                        let rel = cmd == 'v';
                        let oy = if rel { curr.y } else { 0.0 };
                        let Some(y) = tokens.next_num() else { break; };
                        let next = Point {
                            x: curr.x,
                            y: (oy + y - vb_y) * sy,
                        };
                        if edges.len() < MAX_EDGES_PER_SHAPE {
                            edges.push(Edge { p1: curr, p2: next });
                        }
                        curr = next;
                    }
                    'C' | 'c' => {
                        let rel = cmd == 'c';
                        let ox = if rel { curr.x } else { 0.0 };
                        let oy = if rel { curr.y } else { 0.0 };
                        let Some(x1) = tokens.next_num() else { break; };
                        let Some(y1) = tokens.next_num() else { break; };
                        let Some(x2) = tokens.next_num() else { break; };
                        let Some(y2) = tokens.next_num() else { break; };
                        let Some(x3) = tokens.next_num() else { break; };
                        let Some(y3) = tokens.next_num() else { break; };
                        let p0 = curr;
                        let p1 = Point { x: (ox + x1 - vb_x) * sx, y: (oy + y1 - vb_y) * sy };
                        let p2 = Point { x: (ox + x2 - vb_x) * sx, y: (oy + y2 - vb_y) * sy };
                        let p3 = Point { x: (ox + x3 - vb_x) * sx, y: (oy + y3 - vb_y) * sy };

                        // Flatten cubic bezier into 4 line segments
                        let mut prev = p0;
                        for step in [0.25f32, 0.5, 0.75, 1.0] {
                            let t = step;
                            let mt = 1.0 - t;
                            let bx = mt * mt * mt * p0.x
                                + 3.0 * mt * mt * t * p1.x
                                + 3.0 * mt * t * t * p2.x
                                + t * t * t * p3.x;
                            let by = mt * mt * mt * p0.y
                                + 3.0 * mt * mt * t * p1.y
                                + 3.0 * mt * t * t * p2.y
                                + t * t * t * p3.y;
                            let pt = Point { x: bx, y: by };
                            if edges.len() < MAX_EDGES_PER_SHAPE {
                                edges.push(Edge { p1: prev, p2: pt });
                            }
                            prev = pt;
                        }
                        curr = p3;
                    }
                    _ => {
                        // Skip unknown or unsupported command numbers
                        let _ = tokens.next_num();
                    }
                }
            }
        }
    }
}

enum Token {
    Cmd(char),
    Num(f32),
}

struct PathTokenizer<'a> {
    chars: core::str::Chars<'a>,
    putback: Option<f32>,
}

impl<'a> PathTokenizer<'a> {
    fn new(s: &'a str) -> Self {
        Self { chars: s.chars(), putback: None }
    }

    fn put_back(&mut self, num: f32) {
        self.putback = Some(num);
    }

    fn next_cmd_or_num(&mut self) -> Option<Token> {
        if let Some(n) = self.putback.take() {
            return Some(Token::Num(n));
        }
        loop {
            let c = self.chars.clone().next()?;
            if c.is_ascii_whitespace() || c == ',' {
                self.chars.next();
                continue;
            }
            if matches!(c, 'M'|'m'|'L'|'l'|'H'|'h'|'V'|'v'|'C'|'c'|'S'|'s'|'Q'|'q'|'T'|'t'|'A'|'a'|'Z'|'z') {
                self.chars.next();
                return Some(Token::Cmd(c));
            }
            break;
        }
        self.next_num().map(Token::Num)
    }

    fn next_num(&mut self) -> Option<f32> {
        if let Some(n) = self.putback.take() {
            return Some(n);
        }
        // Skip whitespace and commas
        loop {
            let c = self.chars.clone().next()?;
            if c.is_ascii_whitespace() || c == ',' {
                self.chars.next();
                continue;
            }
            break;
        }
        let mut buf = [0u8; 32];
        let mut len = 0;
        let mut has_digit = false;
        while let Some(c) = self.chars.clone().next() {
            if (c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E') && len < buf.len() {
                if c.is_ascii_digit() { has_digit = true; }
                buf[len] = c as u8;
                len += 1;
                self.chars.next();
            } else {
                break;
            }
        }
        if has_digit {
            core::str::from_utf8(&buf[..len]).ok().and_then(parse_number)
        } else {
            None
        }
    }
}

fn extract_attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let mut search = 0;
    while let Some(idx) = tag[search..].find(name) {
        let pos = search + idx;
        search = pos + name.len();
        let before = tag[..pos].chars().last();
        if let Some(c) = before {
            if !c.is_ascii_whitespace() { continue; }
        }
        let rest = tag[pos + name.len()..].trim_start();
        if let Some(stripped) = rest.strip_prefix('=') {
            let trimmed = stripped.trim_start();
            let quote = trimmed.chars().next()?;
            if quote == '"' || quote == '\'' {
                let inner = &trimmed[1..];
                let end = inner.find(quote)?;
                return Some(&inner[..end]);
            }
        }
    }
    None
}

fn parse_number(s: &str) -> Option<f32> {
    let s = s.trim().trim_end_matches("px").trim_end_matches("pt");
    let mut num = 0.0f32;
    let mut sign = 1.0f32;
    let mut chars = s.chars().peekable();
    if let Some(&'-') = chars.peek() { sign = -1.0; chars.next(); }
    else if let Some(&'+') = chars.peek() { chars.next(); }

    let mut fractional = false;
    let mut div = 1.0f32;
    let mut has_digits = false;
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            has_digits = true;
            let d = (c as u8 - b'0') as f32;
            if fractional {
                div *= 10.0;
                num += d / div;
            } else {
                num = num * 10.0 + d;
            }
            chars.next();
        } else if c == '.' && !fractional {
            fractional = true;
            chars.next();
        } else {
            break;
        }
    }
    if has_digits { Some(sign * num) } else { None }
}

fn parse_viewbox(s: &str) -> Option<(f32, f32, f32, f32)> {
    let parts: Vec<f32> = s.split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter(|part| !part.is_empty())
        .filter_map(parse_number)
        .collect();
    if parts.len() >= 4 {
        Some((parts[0], parts[1], parts[2], parts[3]))
    } else {
        None
    }
}

fn min_f32(a: f32, b: f32) -> f32 { if a < b { a } else { b } }
fn max_f32(a: f32, b: f32) -> f32 { if a > b { a } else { b } }

// Simple approximation for circle sin/cos without libm
fn cos_approx(rad: f32) -> f32 {
    let pi = core::f32::consts::PI;
    let tau = core::f32::consts::TAU;
    let mut x = rad % tau;
    if x < 0.0 { x += tau; }
    if x > pi { return -cos_approx(x - pi); }
    // Taylor approximation around pi/2: 1 - x^2/2 + x^4/24
    let x2 = x * x;
    1.0 - (x2 / 2.0) + (x2 * x2 / 24.0)
}

fn sin_approx(rad: f32) -> f32 {
    cos_approx(rad - core::f32::consts::FRAC_PI_2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_rasterizes_minimal_svg_path() {
        let svg = r##"<svg width="40" height="40" viewBox="0 0 40 40">
            <path d="M 0 0 L 40 0 L 40 40 Z" fill="#ff0000"/>
        </svg>"##;
        let img = decode_svg(svg.as_bytes()).expect("decode svg");
        assert_eq!(img.width, 40);
        assert_eq!(img.height, 40);
        assert_eq!(img.pixels.len(), 40 * 40 * 4);
        // Check pixel at (30, 10): should be red [b=0, g=0, r=255, a=255]
        let idx = (10 * 40 + 30) * 4;
        assert_eq!(&img.pixels[idx..idx + 4], &[0, 0, 255, 255]);
    }

    #[test]
    fn parses_rect_and_colors() {
        let svg = r#"<svg viewBox="0 0 100 100"><rect x="10" y="10" width="80" height="80" fill="green"/></svg>"#;
        let (w, h) = svg_dimensions(svg.as_bytes()).unwrap();
        assert_eq!((w, h), (100, 100));
        let img = decode_svg(svg.as_bytes()).unwrap();
        let idx = (50 * 100 + 50) * 4;
        assert_eq!(&img.pixels[idx..idx + 4], &[0, 128, 0, 255]);
    }

}
