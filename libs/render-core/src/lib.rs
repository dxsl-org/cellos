#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

mod geometry;
use geometry::{Hit, Material, Primitive, Ray, Vec3};

pub const COPIED_TILE_MAX_BYTES: usize = 1024;
pub const MAX_DIMENSION: u32 = 8192;
pub const MAX_SAMPLES: u32 = 65536;
pub const MAX_BOUNCES: u32 = 64;
// Explicit scene/integrator version seeds a canonical hash of geometry, materials,
// area-light sampling parameters and camera. Independent of config and BVH layout.
const DEMO_VERSION: u64 = 0x4352_5452_0001_0001;
const CAMERA_ORIGIN: Vec3 = Vec3::new(0.0, 1.85, 6.4);
const CAMERA_TARGET: Vec3 = Vec3::new(0.0, 1.65, -0.4);
const CAMERA_FOV: f64 = 42.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderConfig {
    pub width: u32,
    pub height: u32,
    pub samples: u32,
    pub max_bounces: u32,
    pub seed: u64,
}

impl RenderConfig {
    pub fn validate(self) -> Result<(), RenderError> {
        if self.width == 0
            || self.height == 0
            || self.width > MAX_DIMENSION
            || self.height > MAX_DIMENSION
            || self.samples == 0
            || self.samples > MAX_SAMPLES
            || self.max_bounces == 0
            || self.max_bounces > MAX_BOUNCES
        {
            return Err(RenderError::InvalidConfig);
        }
        self.rgb_bytes()?;
        Ok(())
    }

    pub fn rgb_bytes(self) -> Result<usize, RenderError> {
        byte_length(self.width, self.height).ok_or(RenderError::InvalidConfig)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tile {
    pub id: u32,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Tile {
    pub fn validated(self, config: RenderConfig) -> Result<Self, RenderError> {
        config.validate()?;
        if self.width == 0
            || self.height == 0
            || self
                .x
                .checked_add(self.width)
                .map_or(true, |end| end > config.width)
            || self
                .y
                .checked_add(self.height)
                .map_or(true, |end| end > config.height)
        {
            return Err(RenderError::InvalidTile);
        }
        self.rgb_bytes()?;
        Ok(self)
    }

    pub fn rgb_bytes(self) -> Result<usize, RenderError> {
        byte_length(self.width, self.height).ok_or(RenderError::InvalidTile)
    }
}

fn byte_length(width: u32, height: u32) -> Option<usize> {
    usize::try_from(
        u64::from(width)
            .checked_mul(u64::from(height))?
            .checked_mul(3)?,
    )
    .ok()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderStats {
    pub rays: u64,
    pub samples: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RenderError {
    InvalidConfig,
    InvalidTile,
    OutputLength,
    SceneMismatch,
    InvalidOutput,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Request {
    Render {
        config: RenderConfig,
        tile: Tile,
        scene_hash: u64,
        output: Output,
    },
    Stop,
    Ready,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Output {
    Shared { grant_id: u64, bytes: u32 },
    Copied,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Response {
    Done {
        tile_id: u32,
        scene_hash: u64,
        stats: RenderStats,
        compute_ns: u64,
        pixels: Vec<u8>,
        processor_id_before: Option<u32>,
        processor_id_after: Option<u32>,
    },
    Error(RenderError),
    Stopped,
    Ready {
        scene_hash: u64,
    },
}

/// Immutable demo geometry. Its BVH is built once, not once per tile or ray.
pub struct Scene {
    acceleration: geometry::Bvh,
    light: Light,
    fingerprint: u64,
}

#[derive(Clone, Copy)]
struct Light {
    corner: Vec3,
    edge_u: Vec3,
    edge_v: Vec3,
    normal: Vec3,
    area: f64,
    emission: Vec3,
}

impl Scene {
    pub fn demo() -> Self {
        let white = Material::Diffuse(Vec3::new(0.73, 0.73, 0.70));
        let red = Material::Diffuse(Vec3::new(0.70, 0.12, 0.09));
        let green = Material::Diffuse(Vec3::new(0.12, 0.55, 0.20));
        let gold = Material::Metal(Vec3::new(0.91, 0.72, 0.35));
        let glass = Material::Dielectric(1.5);
        let light = Light {
            corner: Vec3::new(-0.85, 3.98, -0.9),
            edge_u: Vec3::new(1.7, 0.0, 0.0),
            edge_v: Vec3::new(0.0, 0.0, 1.6),
            normal: Vec3::new(0.0, -1.0, 0.0),
            area: 1.7 * 1.6,
            emission: Vec3::new(13.0, 11.5, 9.5),
        };
        let mut primitives = Vec::with_capacity(48);
        geometry::quad(
            &mut primitives,
            Vec3::new(-2.0, 0.0, 2.0),
            Vec3::new(4.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, -4.0),
            white,
        );
        geometry::quad(
            &mut primitives,
            Vec3::new(-2.0, 4.0, -2.0),
            Vec3::new(4.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 4.0),
            white,
        );
        geometry::quad(
            &mut primitives,
            Vec3::new(-2.0, 0.0, -2.0),
            Vec3::new(4.0, 0.0, 0.0),
            Vec3::new(0.0, 4.0, 0.0),
            white,
        );
        geometry::quad(
            &mut primitives,
            Vec3::new(-2.0, 0.0, 2.0),
            Vec3::new(0.0, 0.0, -4.0),
            Vec3::new(0.0, 4.0, 0.0),
            red,
        );
        geometry::quad(
            &mut primitives,
            Vec3::new(2.0, 0.0, -2.0),
            Vec3::new(0.0, 0.0, 4.0),
            Vec3::new(0.0, 4.0, 0.0),
            green,
        );
        geometry::quad(
            &mut primitives,
            light.corner,
            light.edge_u,
            light.edge_v,
            Material::Emissive(light.emission),
        );
        geometry::box_mesh(
            &mut primitives,
            Vec3::new(-0.95, 0.0, -0.75),
            Vec3::new(1.15, 1.35, 1.1),
            0.32,
            white,
        );
        geometry::box_mesh(
            &mut primitives,
            Vec3::new(0.75, 0.0, -1.15),
            Vec3::new(0.7, 2.0, 0.65),
            -0.24,
            Material::Diffuse(Vec3::new(0.25, 0.35, 0.72)),
        );
        primitives.push(Primitive::sphere(Vec3::new(0.75, 0.78, 0.7), 0.78, glass));
        primitives.push(Primitive::sphere(Vec3::new(-0.9, 0.57, 1.0), 0.57, gold));
        let mut fingerprint = DEMO_VERSION;
        for primitive in &primitives {
            fingerprint = primitive.fingerprint(fingerprint);
        }
        for vector in [
            CAMERA_ORIGIN,
            CAMERA_TARGET,
            light.corner,
            light.edge_u,
            light.edge_v,
            light.normal,
            light.emission,
        ] {
            fingerprint = vector.fingerprint(fingerprint);
        }
        fingerprint = hash_word(fingerprint, CAMERA_FOV.to_bits());
        fingerprint = hash_word(fingerprint, light.area.to_bits());
        Self {
            acceleration: geometry::Bvh::new(primitives),
            light,
            fingerprint,
        }
    }

    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }
}

/// Writes RGB8 pixels in tile-local row-major order. Every pixel owns its full sample
/// budget and random stream; tile IDs, worker order and partition size have no effect.
pub fn render_tile(
    scene: &Scene,
    config: RenderConfig,
    tile: Tile,
    output: &mut [u8],
) -> Result<RenderStats, RenderError> {
    let tile = tile.validated(config)?;
    if output.len() != tile.rgb_bytes()? {
        return Err(RenderError::OutputLength);
    }
    let mut stats = RenderStats::default();
    let aspect = f64::from(config.width) / f64::from(config.height);
    let origin = CAMERA_ORIGIN;
    let forward = (CAMERA_TARGET - origin).unit();
    let right = forward.cross(Vec3::new(0.0, 1.0, 0.0)).unit();
    let up = right.cross(forward);
    let half_height = libm::tan(CAMERA_FOV * core::f64::consts::PI / 360.0);
    let mut index = 0;
    for y in tile.y..tile.y + tile.height {
        for x in tile.x..tile.x + tile.width {
            let mut color = Vec3::ZERO;
            let pixel = u64::from(y) * u64::from(config.width) + u64::from(x);
            for sample in 0..config.samples {
                let mut rng = Rng(mix(config.seed
                    ^ mix(pixel)
                    ^ mix(u64::from(sample).wrapping_add(0x73c4_891d))));
                let sx = (2.0 * (f64::from(x) + rng.uniform()) / f64::from(config.width) - 1.0)
                    * aspect
                    * half_height;
                let sy = (1.0 - 2.0 * (f64::from(y) + rng.uniform()) / f64::from(config.height))
                    * half_height;
                color += trace(
                    scene,
                    Ray {
                        origin,
                        direction: (forward + right * sx + up * sy).unit(),
                    },
                    config.max_bounces,
                    &mut rng,
                    &mut stats,
                );
                stats.samples += 1;
            }
            color = color / f64::from(config.samples);
            output[index] = encode(color.x);
            output[index + 1] = encode(color.y);
            output[index + 2] = encode(color.z);
            index += 3;
        }
    }
    Ok(stats)
}

fn trace(
    scene: &Scene,
    mut ray: Ray,
    max_bounces: u32,
    rng: &mut Rng,
    stats: &mut RenderStats,
) -> Vec3 {
    let mut radiance = Vec3::ZERO;
    let mut throughput = Vec3::ONE;
    let mut specular = true;
    for bounce in 0..max_bounces {
        stats.rays += 1;
        let Some(hit) = scene.acceleration.hit(ray, 1e-6, f64::INFINITY) else {
            radiance += throughput * Vec3::new(0.018, 0.025, 0.04);
            break;
        };
        let direction;
        match hit.material {
            Material::Emissive(emission) => {
                // Diffuse paths already estimate the light with next-event sampling.
                if specular && hit.front {
                    radiance += throughput * emission;
                }
                break;
            }
            Material::Diffuse(albedo) => {
                radiance += throughput * direct_light(scene, hit, albedo, rng, stats);
                throughput = throughput * albedo;
                direction = cosine_direction(hit.normal, rng);
                specular = false;
            }
            Material::Metal(albedo) => {
                direction = ray.direction - hit.normal * (2.0 * ray.direction.dot(hit.normal));
                if direction.dot(hit.normal) <= 0.0 {
                    break;
                }
                throughput = throughput * albedo;
                specular = true;
            }
            Material::Dielectric(index) => {
                let ratio = if hit.front { 1.0 / index } else { index };
                let cosine = (-ray.direction.dot(hit.normal)).clamp(0.0, 1.0);
                let sin_squared = (1.0 - cosine * cosine).max(0.0);
                let base_reflectance = (1.0 - ratio) / (1.0 + ratio);
                let r0 = base_reflectance * base_reflectance;
                let fresnel = r0 + (1.0 - r0) * libm::pow(1.0 - cosine, 5.0);
                if ratio * ratio * sin_squared > 1.0 || rng.uniform() < fresnel {
                    direction = ray.direction + hit.normal * (2.0 * cosine);
                } else {
                    let perpendicular = (ray.direction + hit.normal * cosine) * ratio;
                    direction = perpendicular
                        - hit.normal * libm::sqrt((1.0 - perpendicular.length_squared()).max(0.0));
                }
                specular = true;
            }
        }
        ray = Ray {
            origin: offset(hit, direction),
            direction: direction.unit(),
        };
        if bounce >= 4 {
            let survival = throughput.max_component().clamp(0.05, 0.95);
            if rng.uniform() >= survival {
                break;
            }
            throughput = throughput / survival;
        }
    }
    radiance
}

fn direct_light(
    scene: &Scene,
    hit: Hit,
    albedo: Vec3,
    rng: &mut Rng,
    stats: &mut RenderStats,
) -> Vec3 {
    let light = scene.light;
    let point = light.corner + light.edge_u * rng.uniform() + light.edge_v * rng.uniform();
    let delta = point - hit.point;
    let distance_squared = delta.length_squared();
    if distance_squared <= 1e-12 {
        return Vec3::ZERO;
    }
    let distance = libm::sqrt(distance_squared);
    let direction = delta / distance;
    let cosine_surface = hit.normal.dot(direction).max(0.0);
    let cosine_light = light.normal.dot(-direction).max(0.0);
    if cosine_surface == 0.0 || cosine_light == 0.0 {
        return Vec3::ZERO;
    }
    stats.rays += 1;
    let origin = offset(hit, direction);
    // Use the offset-to-light distance, so the sampled emitter cannot self-occlude.
    let shadow_delta = point - origin;
    let shadow_distance = libm::sqrt(shadow_delta.length_squared());
    let shadow = Ray {
        origin,
        direction: shadow_delta / shadow_distance,
    };
    if scene
        .acceleration
        .hit(shadow, 1e-6, shadow_distance - 2e-5)
        .is_some()
    {
        return Vec3::ZERO;
    }
    albedo
        * light.emission
        * (cosine_surface * cosine_light * light.area / (core::f64::consts::PI * distance_squared))
}

fn offset(hit: Hit, direction: Vec3) -> Vec3 {
    let sign = if direction.dot(hit.normal) >= 0.0 {
        1.0
    } else {
        -1.0
    };
    hit.point + hit.normal * (sign * 1e-5)
}

fn cosine_direction(normal: Vec3, rng: &mut Rng) -> Vec3 {
    let angle = 2.0 * core::f64::consts::PI * rng.uniform();
    let radius_squared = rng.uniform();
    let radius = libm::sqrt(radius_squared);
    let helper = if normal.x.abs() > 0.9 {
        Vec3::new(0.0, 1.0, 0.0)
    } else {
        Vec3::new(1.0, 0.0, 0.0)
    };
    let tangent = helper.cross(normal).unit();
    let bitangent = normal.cross(tangent);
    tangent * (radius * libm::cos(angle))
        + bitangent * (radius * libm::sin(angle))
        + normal * libm::sqrt(1.0 - radius_squared)
}

fn encode(linear: f64) -> u8 {
    // Finite nonnegative radiance, Reinhard exposure mapping and gamma-2 display encoding.
    if !linear.is_finite() || linear <= 0.0 {
        return 0;
    }
    (libm::sqrt(linear / (1.0 + linear)) * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}

fn hash_word(mut hash: u64, word: u64) -> u64 {
    for byte in word.to_le_bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
    hash
}

fn mix(mut value: u64) -> u64 {
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

struct Rng(u64);
impl Rng {
    fn uniform(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        ((mix(self.0) >> 11) as f64) * (1.0 / 9007199254740992.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn config() -> RenderConfig {
        RenderConfig {
            width: 19,
            height: 13,
            samples: 4,
            max_bounces: 8,
            seed: 47,
        }
    }

    #[test]
    fn tiles_equal_full_frame_in_reverse_order() {
        let scene = Scene::demo();
        let config = config();
        let full = Tile {
            id: 0,
            x: 0,
            y: 0,
            width: config.width,
            height: config.height,
        };
        let mut reference = vec![0; config.rgb_bytes().unwrap()];
        let reference_stats = render_tile(&scene, config, full, &mut reference).unwrap();
        let mut assembled = vec![0; reference.len()];
        let mut total = RenderStats::default();
        for y in (0..config.height).rev() {
            for x in (0..config.width).rev() {
                let tile = Tile {
                    id: u32::MAX,
                    x,
                    y,
                    width: 1,
                    height: 1,
                };
                let mut pixel = [0; 3];
                let stats = render_tile(&scene, config, tile, &mut pixel).unwrap();
                let index = ((y * config.width + x) * 3) as usize;
                assembled[index..index + 3].copy_from_slice(&pixel);
                total.samples += stats.samples;
                total.rays += stats.rays;
            }
        }
        assert_eq!(reference, assembled);
        assert_eq!(reference_stats, total);
        assert_eq!(total.samples, 19 * 13 * 4);
        assert!(total.rays >= total.samples);
    }

    #[test]
    fn rejects_extreme_inputs_before_writing() {
        let scene = Scene::demo();
        let config = config();
        let tile = Tile {
            id: 0,
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        };
        for invalid in [
            RenderConfig { width: 0, ..config },
            RenderConfig {
                height: u32::MAX,
                ..config
            },
            RenderConfig {
                samples: 0,
                ..config
            },
            RenderConfig {
                samples: u32::MAX,
                ..config
            },
            RenderConfig {
                max_bounces: 0,
                ..config
            },
            RenderConfig {
                max_bounces: u32::MAX,
                ..config
            },
        ] {
            let mut out = [71; 3];
            assert_eq!(
                render_tile(&scene, invalid, tile, &mut out),
                Err(RenderError::InvalidConfig)
            );
            assert_eq!(out, [71; 3]);
        }
        for invalid in [
            Tile {
                x: u32::MAX,
                ..tile
            },
            Tile {
                width: u32::MAX,
                ..tile
            },
            Tile { height: 0, ..tile },
            Tile {
                x: config.width,
                ..tile
            },
        ] {
            assert_eq!(invalid.validated(config), Err(RenderError::InvalidTile));
        }
        assert_eq!(
            render_tile(&scene, config, tile, &mut [0; 2]),
            Err(RenderError::OutputLength)
        );
        assert_eq!(
            render_tile(&scene, config, tile, &mut [0; 4]),
            Err(RenderError::OutputLength)
        );
    }

    #[test]
    fn finite_display_mapping_and_version_identity() {
        assert_eq!(encode(f64::NAN), 0);
        assert_eq!(encode(f64::INFINITY), 0);
        assert_eq!(encode(-1.0), 0);
        assert_eq!(encode(0.0), 0);
        assert!(encode(1.0) > 100);
        let first = Scene::demo().fingerprint();
        assert_eq!(first, Scene::demo().fingerprint());
        let diffuse = Primitive::sphere(Vec3::ZERO, 1.0, Material::Diffuse(Vec3::ONE));
        let metal = Primitive::sphere(Vec3::ZERO, 1.0, Material::Metal(Vec3::ONE));
        assert_ne!(
            diffuse.fingerprint(DEMO_VERSION),
            metal.fingerprint(DEMO_VERSION)
        );
    }
}
