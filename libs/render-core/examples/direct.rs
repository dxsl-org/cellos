#![forbid(unsafe_code)]

use render_core::{render_tile, RenderConfig, RenderError, RenderStats, Scene, Tile};
use std::error::Error;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;
use std::time::Instant;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

struct Options {
    config: RenderConfig,
    output: PathBuf,
    csv: Option<PathBuf>,
    compare: bool,
    tile_size: u32,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn render_error(error: RenderError) -> io::Error {
    invalid(format!("render error: {error:?}"))
}

fn options() -> Result<Option<Options>> {
    let mut options = Options {
        config: RenderConfig {
            width: 640,
            height: 480,
            samples: 64,
            max_bounces: 12,
            seed: 1,
        },
        output: "render.ppm".into(),
        csv: None,
        compare: false,
        tile_size: 32,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!("Native CPU triangle/BVH path tracer\n\
                    --width N --height N --samples N --depth N --seed N\n\
                    --output PATH       RGB8 binary PPM (default render.ppm)\n\
                    --csv PATH          benchmark CSV (default stdout)\n\
                    --compare           render full frame and partitioned tiles; require byte and stats equality\n\
                    --tile-size N       comparison tile edge (default 32)\n\
                    Timings exclude PPM/CSV disk I/O. setup_ns is Scene::demo/BVH construction.\n\
                    compute_ns sums render_tile calls; wall_ns includes output allocation and tile assembly.\n\
                    Rays include camera/continuation and actual light-visibility rays.");
                return Ok(None);
            }
            "--compare" => options.compare = true,
            "--width" | "--height" | "--samples" | "--depth" | "--seed" | "--output" | "--csv"
            | "--tile-size" => {
                let value = args
                    .next()
                    .ok_or_else(|| invalid(format!("missing value for {arg}")))?;
                match arg.as_str() {
                    "--width" => options.config.width = value.parse()?,
                    "--height" => options.config.height = value.parse()?,
                    "--samples" => options.config.samples = value.parse()?,
                    "--depth" => options.config.max_bounces = value.parse()?,
                    "--seed" => options.config.seed = value.parse()?,
                    "--output" => options.output = value.into(),
                    "--csv" => options.csv = Some(value.into()),
                    "--tile-size" => options.tile_size = value.parse()?,
                    _ => unreachable!(),
                }
            }
            _ => return Err(invalid(format!("unknown argument {arg}; use --help")).into()),
        }
    }
    options.config.validate().map_err(render_error)?;
    if options.tile_size == 0 || options.tile_size > render_core::MAX_DIMENSION {
        return Err(invalid("tile-size must be between 1 and MAX_DIMENSION").into());
    }
    if options
        .csv
        .as_ref()
        .is_some_and(|csv| csv == &options.output)
    {
        return Err(invalid("PPM and CSV paths must differ").into());
    }
    Ok(Some(options))
}

struct Measurement {
    mode: &'static str,
    stats: RenderStats,
    compute_ns: u128,
    wall_ns: u128,
    checksum: u64,
    equal: Option<bool>,
}

fn checksum(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

// Open without truncation, then compare the actual destinations. Canonical paths
// catch relative/symlink aliases; Unix file identities also catch hard links.
fn open_outputs(
    image: &std::path::Path,
    csv: Option<&std::path::Path>,
) -> Result<(File, Option<File>)> {
    let open = |path: &std::path::Path| {
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
    };
    let image_file = open(image)?;
    let csv_file = match csv {
        Some(path) => {
            let file = open(path)?;
            let mut same = std::fs::canonicalize(image)? == std::fs::canonicalize(path)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                let left = image_file.metadata()?;
                let right = file.metadata()?;
                same |= left.dev() == right.dev() && left.ino() == right.ino();
            }
            if same {
                return Err(invalid("PPM and CSV paths identify the same file").into());
            }
            Some(file)
        }
        None => None,
    };
    Ok((image_file, csv_file))
}

fn full_frame(scene: &Scene, config: RenderConfig) -> Result<(Vec<u8>, Measurement)> {
    let wall = Instant::now();
    let mut pixels = vec![0; config.rgb_bytes().map_err(render_error)?];
    let tile = Tile {
        id: 0,
        x: 0,
        y: 0,
        width: config.width,
        height: config.height,
    };
    let compute = Instant::now();
    let stats = render_tile(scene, config, tile, &mut pixels).map_err(render_error)?;
    let compute_ns = compute.elapsed().as_nanos();
    let wall_ns = wall.elapsed().as_nanos();
    let measurement = Measurement {
        mode: "direct_full",
        stats,
        compute_ns,
        wall_ns,
        checksum: checksum(&pixels),
        equal: None,
    };
    Ok((pixels, measurement))
}

fn partitioned(
    scene: &Scene,
    config: RenderConfig,
    edge: u32,
    reference: &[u8],
    reference_stats: RenderStats,
) -> Result<Measurement> {
    let wall = Instant::now();
    let mut assembled = vec![0; config.rgb_bytes().map_err(render_error)?];
    let max_tile = Tile {
        id: 0,
        x: 0,
        y: 0,
        width: edge.min(config.width),
        height: edge.min(config.height),
    };
    let mut buffer = vec![0; max_tile.rgb_bytes().map_err(render_error)?];
    let mut stats = RenderStats::default();
    let mut compute_ns = 0;
    let mut id = 0;
    for y in (0..config.height).step_by(edge as usize) {
        for x in (0..config.width).step_by(edge as usize) {
            let tile = Tile {
                id,
                x,
                y,
                width: edge.min(config.width - x),
                height: edge.min(config.height - y),
            };
            id += 1;
            let bytes = tile.rgb_bytes().map_err(render_error)?;
            let compute = Instant::now();
            let result =
                render_tile(scene, config, tile, &mut buffer[..bytes]).map_err(render_error)?;
            compute_ns += compute.elapsed().as_nanos();
            stats.rays += result.rays;
            stats.samples += result.samples;
            for row in 0..tile.height {
                let source = (row * tile.width * 3) as usize;
                let destination = (((y + row) * config.width + x) * 3) as usize;
                let length = (tile.width * 3) as usize;
                assembled[destination..destination + length]
                    .copy_from_slice(&buffer[source..source + length]);
            }
        }
    }
    let wall_ns = wall.elapsed().as_nanos();
    let equal = assembled == reference && stats == reference_stats;
    let measurement = Measurement {
        mode: "direct_partitioned",
        stats,
        compute_ns,
        wall_ns,
        checksum: checksum(&assembled),
        equal: Some(equal),
    };
    if !equal {
        return Err(io::Error::other(
            "partitioned output or ray/sample totals differ from full frame",
        )
        .into());
    }
    Ok(measurement)
}

fn write_csv(
    output: &mut dyn Write,
    scene: &Scene,
    config: RenderConfig,
    setup_ns: u128,
    measurements: &[Measurement],
) -> Result<()> {
    writeln!(output, "mode,scene_hash,width,height,samples_per_pixel,max_bounces,seed,samples,rays,setup_ns,compute_ns,wall_ns,samples_per_second,rays_per_second,rgb_checksum,partition_equal")?;
    for measurement in measurements {
        let seconds = measurement.wall_ns as f64 / 1_000_000_000.0;
        let equal = match measurement.equal {
            Some(true) => "true",
            Some(false) => "false",
            None => "not_checked",
        };
        writeln!(
            output,
            "{},{:016x},{},{},{},{},{},{},{},{},{},{},{:.3},{:.3},{:016x},{}",
            measurement.mode,
            scene.fingerprint(),
            config.width,
            config.height,
            config.samples,
            config.max_bounces,
            config.seed,
            measurement.stats.samples,
            measurement.stats.rays,
            setup_ns,
            measurement.compute_ns,
            measurement.wall_ns,
            measurement.stats.samples as f64 / seconds,
            measurement.stats.rays as f64 / seconds,
            measurement.checksum,
            equal
        )?;
    }
    output.flush()?;
    Ok(())
}

fn main() -> Result<()> {
    let Some(options) = options()? else {
        return Ok(());
    };
    let (image_file, csv_file) = open_outputs(&options.output, options.csv.as_deref())?;
    let setup = Instant::now();
    let scene = Scene::demo();
    let setup_ns = setup.elapsed().as_nanos();
    let (pixels, full) = full_frame(&scene, options.config)?;
    let full_stats = full.stats;
    let mut measurements = vec![full];
    if options.compare {
        measurements.push(partitioned(
            &scene,
            options.config,
            options.tile_size,
            &pixels,
            full_stats,
        )?);
        eprintln!("Partition comparison: identical RGB bytes and actual ray/sample counts");
    }
    image_file.set_len(0)?;
    let mut image = BufWriter::new(image_file);
    write!(
        image,
        "P6\n{} {}\n255\n",
        options.config.width, options.config.height
    )?;
    image.write_all(&pixels)?;
    image.flush()?;
    match csv_file {
        Some(file) => {
            file.set_len(0)?;
            write_csv(
                &mut BufWriter::new(file),
                &scene,
                options.config,
                setup_ns,
                &measurements,
            )?;
        }
        None => write_csv(
            &mut io::stdout().lock(),
            &scene,
            options.config,
            setup_ns,
            &measurements,
        )?,
    }
    eprintln!(
        "Wrote {}x{} lit 3D RGB image to {}",
        options.config.width,
        options.config.height,
        options.output.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn output_aliases_are_rejected_without_truncating_existing_image() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let scratch = Scratch(std::env::temp_dir().join(format!(
            "cellos-render-alias-{}-{nonce}",
            std::process::id()
        )));
        std::fs::create_dir(&scratch.0).unwrap();
        std::fs::create_dir(scratch.0.join("child")).unwrap();
        let image = scratch.0.join("image.ppm");
        let original = b"P6\n1 1\n255\n\x80\x90\xA0";
        std::fs::write(&image, original).unwrap();
        let alias = scratch.0.join("child/../image.ppm");
        assert!(open_outputs(&image, Some(&alias)).is_err());
        assert_eq!(std::fs::read(&image).unwrap(), original);
        #[cfg(unix)]
        {
            let hard_link = scratch.0.join("hard.ppm");
            std::fs::hard_link(&image, &hard_link).unwrap();
            assert!(open_outputs(&image, Some(&hard_link)).is_err());
            let symbolic_link = scratch.0.join("symbolic.ppm");
            std::os::unix::fs::symlink(&image, &symbolic_link).unwrap();
            assert!(open_outputs(&image, Some(&symbolic_link)).is_err());
            assert_eq!(std::fs::read(&image).unwrap(), original);
        }
        let csv = scratch.0.join("results.csv");
        let (_, csv_file) = open_outputs(&image, Some(&csv)).unwrap();
        drop(csv_file);
        assert_eq!(std::fs::read(&image).unwrap(), original);
    }
}
