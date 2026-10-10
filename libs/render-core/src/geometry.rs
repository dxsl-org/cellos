use alloc::vec::Vec;
use core::ops::{Add, AddAssign, Div, Mul, Neg, Sub};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}
impl Vec3 {
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);
    pub const ONE: Self = Self::new(1.0, 1.0, 1.0);
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }
    pub fn dot(self, rhs: Self) -> f64 {
        self.x * rhs.x + self.y * rhs.y + self.z * rhs.z
    }
    pub fn cross(self, rhs: Self) -> Self {
        Self::new(
            self.y * rhs.z - self.z * rhs.y,
            self.z * rhs.x - self.x * rhs.z,
            self.x * rhs.y - self.y * rhs.x,
        )
    }
    pub fn length_squared(self) -> f64 {
        self.dot(self)
    }
    pub fn unit(self) -> Self {
        let length = libm::sqrt(self.length_squared());
        if length > 0.0 && length.is_finite() {
            self / length
        } else {
            Self::new(0.0, 1.0, 0.0)
        }
    }
    pub fn max_component(self) -> f64 {
        self.x.max(self.y).max(self.z)
    }
    fn component(self, axis: usize) -> f64 {
        match axis {
            0 => self.x,
            1 => self.y,
            _ => self.z,
        }
    }
    fn min(self, rhs: Self) -> Self {
        Self::new(self.x.min(rhs.x), self.y.min(rhs.y), self.z.min(rhs.z))
    }
    fn max(self, rhs: Self) -> Self {
        Self::new(self.x.max(rhs.x), self.y.max(rhs.y), self.z.max(rhs.z))
    }
    pub fn fingerprint(self, mut hash: u64) -> u64 {
        for component in [self.x, self.y, self.z] {
            hash = crate::hash_word(hash, component.to_bits());
        }
        hash
    }
}
impl Add for Vec3 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}
impl AddAssign for Vec3 {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}
impl Sub for Vec3 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}
impl Neg for Vec3 {
    type Output = Self;
    fn neg(self) -> Self {
        Self::new(-self.x, -self.y, -self.z)
    }
}
impl Mul<f64> for Vec3 {
    type Output = Self;
    fn mul(self, rhs: f64) -> Self {
        Self::new(self.x * rhs, self.y * rhs, self.z * rhs)
    }
}
impl Mul for Vec3 {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Self::new(self.x * rhs.x, self.y * rhs.y, self.z * rhs.z)
    }
}
impl Div<f64> for Vec3 {
    type Output = Self;
    fn div(self, rhs: f64) -> Self {
        self * (1.0 / rhs)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Ray {
    pub origin: Vec3,
    pub direction: Vec3,
}
#[derive(Clone, Copy)]
pub(crate) enum Material {
    Diffuse(Vec3),
    Metal(Vec3),
    Dielectric(f64),
    Emissive(Vec3),
}
#[derive(Clone, Copy)]
pub(crate) struct Hit {
    pub distance: f64,
    pub point: Vec3,
    pub normal: Vec3,
    pub front: bool,
    pub material: Material,
}
impl Hit {
    fn new(ray: Ray, distance: f64, outward: Vec3, material: Material) -> Self {
        let front = ray.direction.dot(outward) < 0.0;
        Self {
            distance,
            point: ray.origin + ray.direction * distance,
            normal: if front { outward } else { -outward },
            front,
            material,
        }
    }
}

#[derive(Clone, Copy)]
enum Shape {
    Triangle {
        a: Vec3,
        edge_b: Vec3,
        edge_c: Vec3,
        normal: Vec3,
    },
    Sphere {
        center: Vec3,
        radius: f64,
    },
}
#[derive(Clone, Copy)]
pub(crate) struct Primitive {
    shape: Shape,
    material: Material,
    bounds: Bounds,
}
impl Primitive {
    fn triangle(a: Vec3, b: Vec3, c: Vec3, material: Material) -> Self {
        let edge_b = b - a;
        let edge_c = c - a;
        // Pad zero-thickness triangle boxes to include edge and coplanar slab rounding.
        let padding = Vec3::new(1e-7, 1e-7, 1e-7);
        Self {
            shape: Shape::Triangle {
                a,
                edge_b,
                edge_c,
                normal: edge_b.cross(edge_c).unit(),
            },
            material,
            bounds: Bounds {
                min: a.min(b).min(c) - padding,
                max: a.max(b).max(c) + padding,
            },
        }
    }
    pub fn sphere(center: Vec3, radius: f64, material: Material) -> Self {
        let extent = Vec3::ONE * radius;
        Self {
            shape: Shape::Sphere { center, radius },
            material,
            bounds: Bounds {
                min: center - extent,
                max: center + extent,
            },
        }
    }
    pub fn fingerprint(&self, mut hash: u64) -> u64 {
        match self.shape {
            Shape::Triangle {
                a,
                edge_b,
                edge_c,
                normal,
            } => {
                hash = crate::hash_word(hash, 0);
                for vector in [a, edge_b, edge_c, normal] {
                    hash = vector.fingerprint(hash);
                }
            }
            Shape::Sphere { center, radius } => {
                hash = center.fingerprint(crate::hash_word(hash, 1));
                hash = crate::hash_word(hash, radius.to_bits());
            }
        }
        match self.material {
            Material::Diffuse(color) => color.fingerprint(crate::hash_word(hash, 0)),
            Material::Metal(color) => color.fingerprint(crate::hash_word(hash, 1)),
            Material::Dielectric(index) => {
                crate::hash_word(crate::hash_word(hash, 2), index.to_bits())
            }
            Material::Emissive(color) => color.fingerprint(crate::hash_word(hash, 3)),
        }
    }
    fn hit(&self, ray: Ray, minimum: f64, maximum: f64) -> Option<Hit> {
        match self.shape {
            Shape::Triangle {
                a,
                edge_b,
                edge_c,
                normal,
            } => {
                let p = ray.direction.cross(edge_c);
                let determinant = edge_b.dot(p);
                if determinant.abs() < 1e-12 {
                    return None;
                }
                let inverse = 1.0 / determinant;
                let translated = ray.origin - a;
                let u = translated.dot(p) * inverse;
                if !(0.0..=1.0).contains(&u) {
                    return None;
                }
                let q = translated.cross(edge_b);
                let v = ray.direction.dot(q) * inverse;
                if v < 0.0 || u + v > 1.0 {
                    return None;
                }
                let distance = edge_c.dot(q) * inverse;
                if !distance.is_finite() || distance < minimum || distance > maximum {
                    return None;
                }
                Some(Hit::new(ray, distance, normal, self.material))
            }
            Shape::Sphere { center, radius } => {
                let relative = ray.origin - center;
                let a = ray.direction.length_squared();
                if a <= 0.0 {
                    return None;
                }
                let half_b = relative.dot(ray.direction);
                let c = relative.length_squared() - radius * radius;
                let discriminant = half_b * half_b - a * c;
                if discriminant < 0.0 {
                    return None;
                }
                let root = libm::sqrt(discriminant);
                // Stable quadratic roots avoid cancellation for distant spheres.
                let q = -half_b - if half_b >= 0.0 { root } else { -root };
                let (first, second) = if q == 0.0 {
                    (-half_b / a, -half_b / a)
                } else {
                    let t0 = q / a;
                    let t1 = c / q;
                    (t0.min(t1), t0.max(t1))
                };
                let distance = if first >= minimum && first <= maximum {
                    first
                } else if second >= minimum && second <= maximum {
                    second
                } else {
                    return None;
                };
                if !distance.is_finite() {
                    return None;
                }
                Some(Hit::new(
                    ray,
                    distance,
                    (ray.origin + ray.direction * distance - center) / radius,
                    self.material,
                ))
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Bounds {
    min: Vec3,
    max: Vec3,
}
impl Bounds {
    fn union(self, rhs: Self) -> Self {
        Self {
            min: self.min.min(rhs.min),
            max: self.max.max(rhs.max),
        }
    }
    fn entry(self, ray: Ray, mut minimum: f64, mut maximum: f64) -> Option<f64> {
        for axis in 0..3 {
            let origin = ray.origin.component(axis);
            let direction = ray.direction.component(axis);
            let low = self.min.component(axis);
            let high = self.max.component(axis);
            // Explicit parallel handling avoids 0 * infinity NaNs at slab boundaries.
            if direction == 0.0 {
                if origin < low || origin > high {
                    return None;
                }
            } else {
                let first = (low - origin) / direction;
                let second = (high - origin) / direction;
                minimum = minimum.max(first.min(second));
                maximum = maximum.min(first.max(second));
                if maximum < minimum {
                    return None;
                }
            }
        }
        Some(minimum)
    }
}

#[derive(Clone, Copy)]
struct Node {
    bounds: Bounds,
    start: usize,
    count: usize,
    left: usize,
    right: usize,
}
pub(crate) struct Bvh {
    primitives: Vec<Primitive>,
    nodes: Vec<Node>,
}
impl Bvh {
    pub fn new(primitives: Vec<Primitive>) -> Self {
        assert!(!primitives.is_empty());
        let count = primitives.len();
        let mut bvh = Self {
            primitives,
            nodes: Vec::with_capacity(count * 2),
        };
        bvh.build(0, count);
        bvh
    }
    fn build(&mut self, start: usize, end: usize) -> usize {
        let mut bounds = self.primitives[start].bounds;
        for primitive in &self.primitives[start + 1..end] {
            bounds = bounds.union(primitive.bounds);
        }
        let node = self.nodes.len();
        self.nodes.push(Node {
            bounds,
            start,
            count: end - start,
            left: 0,
            right: 0,
        });
        if end - start > 4 {
            let extent = bounds.max - bounds.min;
            let axis = if extent.x >= extent.y && extent.x >= extent.z {
                0
            } else if extent.y >= extent.z {
                1
            } else {
                2
            };
            self.primitives[start..end].sort_unstable_by(|a, b| {
                let ca = a.bounds.min.component(axis) + a.bounds.max.component(axis);
                let cb = b.bounds.min.component(axis) + b.bounds.max.component(axis);
                ca.total_cmp(&cb)
            });
            let middle = start + (end - start) / 2;
            let left = self.build(start, middle);
            let right = self.build(middle, end);
            self.nodes[node].count = 0;
            self.nodes[node].left = left;
            self.nodes[node].right = right;
        }
        node
    }
    pub fn hit(&self, ray: Ray, minimum: f64, mut maximum: f64) -> Option<Hit> {
        // Median-split private demo builder has logarithmic depth (< 64 on any
        // addressable vector). Traversal storage is stack-local, never allocated.
        let mut stack = [0usize; 64];
        let mut pending = 1;
        let mut nearest = None;
        while pending > 0 {
            pending -= 1;
            let node = self.nodes[stack[pending]];
            if node.bounds.entry(ray, minimum, maximum).is_none() {
                continue;
            }
            if node.count > 0 {
                for primitive in &self.primitives[node.start..node.start + node.count] {
                    if let Some(hit) = primitive.hit(ray, minimum, maximum) {
                        maximum = hit.distance;
                        nearest = Some(hit);
                    }
                }
            } else {
                let left = self.nodes[node.left].bounds.entry(ray, minimum, maximum);
                let right = self.nodes[node.right].bounds.entry(ray, minimum, maximum);
                match (left, right) {
                    (Some(a), Some(b)) => {
                        let (near, far) = if a <= b {
                            (node.left, node.right)
                        } else {
                            (node.right, node.left)
                        };
                        stack[pending] = far;
                        stack[pending + 1] = near;
                        pending += 2;
                    }
                    (Some(_), None) => {
                        stack[pending] = node.left;
                        pending += 1;
                    }
                    (None, Some(_)) => {
                        stack[pending] = node.right;
                        pending += 1;
                    }
                    (None, None) => {}
                }
            }
        }
        nearest
    }
}

pub(crate) fn quad(
    primitives: &mut Vec<Primitive>,
    corner: Vec3,
    edge_u: Vec3,
    edge_v: Vec3,
    material: Material,
) {
    primitives.push(Primitive::triangle(
        corner,
        corner + edge_u,
        corner + edge_u + edge_v,
        material,
    ));
    primitives.push(Primitive::triangle(
        corner,
        corner + edge_u + edge_v,
        corner + edge_v,
        material,
    ));
}

pub(crate) fn box_mesh(
    primitives: &mut Vec<Primitive>,
    base: Vec3,
    size: Vec3,
    rotation: f64,
    material: Material,
) {
    let u = Vec3::new(libm::cos(rotation), 0.0, -libm::sin(rotation)) * size.x;
    let v = Vec3::new(0.0, size.y, 0.0);
    let w = Vec3::new(libm::sin(rotation), 0.0, libm::cos(rotation)) * size.z;
    let corner = base - (u + w) * 0.5;
    quad(primitives, corner, u, w, material);
    quad(primitives, corner + v, w, u, material);
    quad(primitives, corner, v, u, material);
    quad(primitives, corner + w, u, v, material);
    quad(primitives, corner, w, v, material);
    quad(primitives, corner + u, v, w, material);
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    const WHITE: Material = Material::Diffuse(Vec3::ONE);

    #[test]
    fn triangles_handle_edges_parallel_and_back_faces() {
        let triangle = Primitive::triangle(
            Vec3::ZERO,
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            WHITE,
        );
        for point in [
            Vec3::new(0.2, 0.2, 1.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(0.5, 0.5, 1.0),
        ] {
            let hit = triangle
                .hit(
                    Ray {
                        origin: point,
                        direction: Vec3::new(0.0, 0.0, -1.0),
                    },
                    0.0,
                    2.0,
                )
                .unwrap();
            assert_eq!(hit.distance, 1.0);
            assert!(hit.front);
        }
        let hit = triangle
            .hit(
                Ray {
                    origin: Vec3::new(0.2, 0.2, -1.0),
                    direction: Vec3::new(0.0, 0.0, 1.0),
                },
                0.0,
                2.0,
            )
            .unwrap();
        assert!(!hit.front);
        assert!(triangle
            .hit(
                Ray {
                    origin: Vec3::new(0.2, 0.2, 1.0),
                    direction: Vec3::new(1.0, 0.0, 0.0)
                },
                0.0,
                2.0
            )
            .is_none());
        assert!(triangle
            .hit(
                Ray {
                    origin: Vec3::new(0.8, 0.8, 1.0),
                    direction: Vec3::new(0.0, 0.0, -1.0)
                },
                0.0,
                2.0
            )
            .is_none());
    }

    #[test]
    fn spheres_handle_inside_tangent_and_nonunit_rays() {
        let sphere = Primitive::sphere(Vec3::ZERO, 1.0, WHITE);
        let inside = sphere
            .hit(
                Ray {
                    origin: Vec3::ZERO,
                    direction: Vec3::new(2.0, 0.0, 0.0),
                },
                1e-6,
                9.0,
            )
            .unwrap();
        assert_eq!(inside.distance, 0.5);
        assert!(!inside.front);
        let tangent = sphere
            .hit(
                Ray {
                    origin: Vec3::new(1.0, 0.0, 3.0),
                    direction: Vec3::new(0.0, 0.0, -1.0),
                },
                1e-6,
                9.0,
            )
            .unwrap();
        assert_eq!(tangent.distance, 3.0);
        assert!(sphere
            .hit(
                Ray {
                    origin: Vec3::new(0.0, 0.0, 3.0),
                    direction: Vec3::new(0.0, 0.0, -1.0)
                },
                1e-6,
                1.5
            )
            .is_none());
    }

    #[test]
    fn box_faces_point_outward() {
        let mut mesh = Vec::new();
        box_mesh(&mut mesh, Vec3::ZERO, Vec3::new(2.0, 2.0, 2.0), 0.0, WHITE);
        let bvh = Bvh::new(mesh);
        for (origin, direction) in [
            (Vec3::new(0.0, 3.0, 0.0), Vec3::new(0.0, -1.0, 0.0)),
            (Vec3::new(0.0, -1.0, 0.0), Vec3::new(0.0, 1.0, 0.0)),
            (Vec3::new(2.0, 1.0, 0.0), Vec3::new(-1.0, 0.0, 0.0)),
            (Vec3::new(-2.0, 1.0, 0.0), Vec3::new(1.0, 0.0, 0.0)),
            (Vec3::new(0.0, 1.0, 2.0), Vec3::new(0.0, 0.0, -1.0)),
            (Vec3::new(0.0, 1.0, -2.0), Vec3::new(0.0, 0.0, 1.0)),
        ] {
            let hit = bvh.hit(Ray { origin, direction }, 1e-6, 10.0).unwrap();
            assert!(hit.front);
            assert_eq!(hit.distance, 1.0);
        }
    }

    #[test]
    fn parallel_slab_boundary_is_not_nan() {
        let bounds = Bounds {
            min: Vec3::ZERO,
            max: Vec3::ONE,
        };
        assert!(bounds
            .entry(
                Ray {
                    origin: Vec3::new(0.0, 0.5, -1.0),
                    direction: Vec3::new(0.0, 0.0, 1.0)
                },
                0.0,
                10.0
            )
            .is_some());
        assert!(bounds
            .entry(
                Ray {
                    origin: Vec3::new(-0.1, 0.5, -1.0),
                    direction: Vec3::new(0.0, 0.0, 1.0)
                },
                0.0,
                10.0
            )
            .is_none());
    }

    #[test]
    fn bvh_matches_brute_force_nearest_hit() {
        let scene = crate::Scene::demo();
        let bvh = &scene.acceleration;
        for y in 0..31 {
            for x in 0..41 {
                let ray = Ray {
                    origin: Vec3::new(0.0, 1.7, 6.0),
                    direction: Vec3::new(
                        (f64::from(x) - 20.0) / 10.0,
                        (f64::from(y) - 15.0) / 10.0,
                        -5.0,
                    )
                    .unit(),
                };
                let accelerated = bvh.hit(ray, 1e-6, f64::INFINITY);
                let mut closest = f64::INFINITY;
                for primitive in &bvh.primitives {
                    if let Some(hit) = primitive.hit(ray, 1e-6, closest) {
                        closest = hit.distance;
                    }
                }
                assert_eq!(accelerated.is_some(), closest.is_finite());
                if let Some(hit) = accelerated {
                    assert!((hit.distance - closest).abs() < 1e-10);
                }
            }
        }
        let lone = Bvh::new(vec![Primitive::sphere(Vec3::ZERO, 1.0, WHITE)]);
        assert_eq!(
            lone.hit(
                Ray {
                    origin: Vec3::new(0.0, 0.0, 3.0),
                    direction: Vec3::new(0.0, 0.0, -1.0)
                },
                1e-6,
                9.0
            )
            .unwrap()
            .distance,
            2.0
        );
    }
}
