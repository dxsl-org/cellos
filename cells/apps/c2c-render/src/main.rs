#![no_std]
#![cfg_attr(not(test), no_main)]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::{format, string::String, vec, vec::Vec};
#[cfg(test)]
use alloc::string::ToString;
use app_c2c_render::{percentile, pixel_hash, Clock};
use ostd::grant::GrantHandle;
use ostd::syscall::{
    sys_exit, sys_force_exit, sys_set_spawn_args, sys_spawn_from_path, sys_wait, sys_yield,
    SyscallResult,
};
use render_core::{
    render_tile, Output, RenderConfig, RenderError, RenderStats, Request, Response, Scene, Tile,
    COPIED_TILE_MAX_BYTES,
};

api::declare_manifest!(block_io = false, network = false, spawn = true);
api::declare_syscalls![
    Log,
    Send,
    Recv,
    LookupService,
    IpcSubmit,
    IpcTake,
    IpcWait,
    IpcCancel,
    GetTime,
    GrantAlloc,
    GrantShare,
    GrantSlice,
    SpawnFromPath,
    SpawnFromElf,
    StateStash,
    StateRestore,
    Wait,
    VfsMutate,
];

#[cfg(not(test))]
ostd::cell_main!(cell_main);

#[cfg(target_os = "none")]
ostd::declare_custom_heap!(16 * 1024 * 1024);

const USAGE: &str = "c2c-render [baseline|copy|shared|compare|tier2|compare-tier2] [--width N] [--height N] [--samples N] [--depth N] [--seed N] [--tile-size N] [--workers N] [--output /tmp/image.ppm]\nDefaults: compare, 256x192, 64 samples, depth 8, seed 1, tile-size 16, workers 1.\nLocal native CPU rendering, including copied-only Tier2 paged domains. Workers 1..4 (clamped to tile count; baseline uses none); outstanding calls do not imply multicore CPU execution. Tile-size 1..18; image <=4 MiB RGB, <=65536 tiles. Remote/Tier3 are not supported.";
const MAX_IMAGE_BYTES: usize = 4 * 1024 * 1024;
const MAX_TILES: u32 = 65536;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Baseline,
    Copy,
    Shared,
    Compare,
    Tier2,
    CompareTier2,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Self::Baseline => "baseline",
            Self::Copy => "copy",
            Self::Shared => "shared",
            Self::Compare => "compare",
            Self::Tier2 => "tier2",
            Self::CompareTier2 => "compare-tier2",
        }
    }
}

struct Options {
    mode: Mode,
    config: RenderConfig,
    tile_size: u32,
    workers: u32,
    output: String,
}

fn options(args: &[String]) -> Result<Options, String> {
    let mut result = Options {
        mode: Mode::Compare,
        config: RenderConfig {
            width: 256,
            height: 192,
            samples: 64,
            max_bounces: 8,
            seed: 1,
        },
        tile_size: 16,
        workers: 1,
        output: String::from("/tmp/c2c-render.ppm"),
    };
    let mut index = 0;
    if let Some(first) = args.first().filter(|arg| !arg.starts_with('-')) {
        result.mode = match first.as_str() {
            "baseline" => Mode::Baseline,
            "copy" => Mode::Copy,
            "shared" => Mode::Shared,
            "compare" => Mode::Compare,
            "tier2" => Mode::Tier2,
            "compare-tier2" => Mode::CompareTier2,
            "remote" | "tier3" => return Err(String::from(
                "remote/Tier3 rendering is NotSupported; no remote readiness or transport bypass",
            )),
            _ => return Err(format!("unknown mode: {}", first)),
        };
        index = 1;
    }
    while index < args.len() {
        let flag = args[index].as_str();
        let value = args
            .get(index + 1)
            .ok_or_else(|| format!("missing value for {}", flag))?;
        let number = || {
            value
                .parse::<u32>()
                .map_err(|_| format!("invalid integer for {}: {}", flag, value))
        };
        match flag {
            "--width" => result.config.width = number()?,
            "--height" => result.config.height = number()?,
            "--samples" => result.config.samples = number()?,
            "--depth" => result.config.max_bounces = number()?,
            "--seed" => {
                result.config.seed = value
                    .parse::<u64>()
                    .map_err(|_| String::from("invalid seed"))?
            }
            "--tile-size" => result.tile_size = number()?,
            "--workers" => result.workers = number()?,
            "--output" => result.output = value.clone(),
            _ => return Err(format!("unknown option: {}", flag)),
        }
        index += 2;
    }
    result
        .config
        .validate()
        .map_err(|error| format!("invalid render configuration: {:?}", error))?;
    let bytes = result
        .config
        .rgb_bytes()
        .map_err(|error| format!("image size: {:?}", error))?;
    if bytes > MAX_IMAGE_BYTES {
        return Err(String::from(
            "RGB image exceeds 4 MiB application memory limit",
        ));
    }
    if result.tile_size == 0 || result.tile_size > 18 {
        return Err(String::from(
            "tile-size must be 1..18 to keep copied RGB tiles <=1024 bytes",
        ));
    }
    if !(1..=4).contains(&result.workers) {
        return Err(String::from("workers must be 1..4"));
    }
    let count = result
        .config
        .width
        .div_ceil(result.tile_size)
        .checked_mul(result.config.height.div_ceil(result.tile_size))
        .ok_or_else(|| String::from("tile count overflow"))?;
    if count > MAX_TILES {
        return Err(String::from("more than 65536 tiles; increase tile-size"));
    }
    if !result.output.starts_with('/') || result.output.len() > 96 {
        return Err(String::from(
            "output must be an absolute VFS path <=96 bytes",
        ));
    }
    Ok(result)
}

struct Worker {
    tid: usize,
    active: bool,
}

impl Worker {
    fn spawn(scene_hash: u64, path: &'static str) -> Result<Self, String> {
        if !sys_set_spawn_args("") {
            return Err(String::from("could not clear worker spawn arguments"));
        }
        let tid = match sys_spawn_from_path(path) {
            SyscallResult::Ok(tid) if tid > 0 => tid,
            result => return Err(format!("spawn {} failed: {:?}", path, result)),
        };
        let worker = Self { tid, active: true };
        match worker.call(&Request::Ready).map(|(response, _)| response) {
            Ok(Response::Ready { scene_hash: actual }) if actual == scene_hash => Ok(worker),
            result => Err(format!(
                "worker startup/fingerprint validation failed: {:?}",
                result
            )),
        }
    }

    fn call(&self, request: &Request) -> Result<(Response, usize), String> {
        let mut send = [0u8; api::ipc::IPC_BUF_SIZE];
        let mut receive = [0u8; api::ipc::IPC_BUF_SIZE];
        // Request admission and completion belong to the exact returned worker
        // TID and one kernel operation, never a global service or arbitrary recv.
        let encoded = api::ipc::encode(request, &mut send)
            .map_err(|_| String::from("request exceeds IPC frame"))?;
        let request_bytes = encoded.len();
        let call = ostd::ipc::PendingCall::submit(self.tid, encoded)
            .map_err(|error| format!("worker {} admission failed: {:?}", self.tid, error))?;
        let completion = match call.wait_and_take(&mut receive, 0, 1) {
            Ok(Some(completion)) => completion,
            result => {
                let _ = call.cancel();
                let _ = call.try_take(&mut receive);
                return Err(format!(
                    "worker {} operation failed: {:?}",
                    self.tid, result
                ));
            }
        };
        if completion.terminal != ostd::ipc::IpcTerminal::Reply {
            return Err(format!(
                "worker {} operation terminal: {:?}",
                self.tid, completion.terminal
            ));
        }
        let raw = &receive[..completion.len];
        let response =
            api::ipc::decode(raw).map_err(|_| String::from("invalid worker response encoding"))?;
        Ok((response, request_bytes + raw.len()))
    }

    fn require_shared_refusal(&self, config: RenderConfig, scene_hash: u64) -> Result<(), String> {
        // A valid one-pixel request with no real grant tests the worker boundary,
        // not grant allocation or malformed geometry. This is setup traffic.
        let request = Request::Render {
            config,
            tile: Tile {
                id: 0,
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
            scene_hash,
            output: Output::Shared {
                grant_id: u64::MAX,
                bytes: 3,
            },
        };
        match self.call(&request).map(|(response, _)| response) {
            Ok(Response::Error(RenderError::InvalidOutput)) => {
                ostd::io::println("c2c-render: TIER2 SHARED OUTPUT REFUSED");
                Ok(())
            }
            result => Err(format!("Tier2 Shared output refusal failed: {:?}", result)),
        }
    }

    fn terminate(&mut self) -> bool {
        if !self.active {
            return true;
        }
        if matches!(sys_force_exit(self.tid), SyscallResult::Ok(_)) {
            // ForceExit itself is nonblocking. Wait is published by the kernel
            // only after saved contexts have stopped executing on every hart.
            // The forced-exit sentinel is mapped to Err by ostd, not a failure
            // of this quiescence barrier for our known, successfully killed child.
            let _ = sys_wait(self.tid);
            self.active = false;
            true
        } else {
            false
        }
    }

    fn stop(&mut self) -> Result<(), String> {
        match self.call(&Request::Stop) {
            Ok((Response::Stopped, _)) => {
                let result = sys_wait(self.tid);
                self.active = false;
                match result {
                    SyscallResult::Ok(0) => Ok(()),
                    // This exact child acknowledged Stop and retains no grant.
                    // With Wait admitted by our manifest, the kernel's only
                    // error here means the child has already been quiescently
                    // reaped (syscall.rs Wait's InvalidDriverId branch).
                    SyscallResult::Err(ostd::syscall::SyscallError::Unknown) => Ok(()),
                    _ => Err(format!("worker clean exit failed: {:?}", result)),
                }
            }
            result => {
                let _ = self.terminate();
                Err(format!("worker stop failed: {:?}", result))
            }
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        if self.active && !self.terminate() {
            ostd::io::println("c2c-render: ERROR cannot terminate worker; coordinator retained alive to preserve ownership");
            loop {
                sys_yield();
            }
        }
    }
}

struct ActiveTile {
    call: ostd::ipc::PendingCall,
    tile: Tile,
    started_ns: u64,
    request_bytes: usize,
}

struct Slot {
    worker: Worker,
    grant: Option<GrantHandle<u8>>,
    pending: Option<ActiveTile>,
    send: [u8; api::ipc::IPC_BUF_SIZE],
    receive: [u8; api::ipc::IPC_BUF_SIZE],
    completed: u32,
}

impl Slot {
    fn submit(
        &mut self,
        config: RenderConfig,
        tile: Tile,
        scene_hash: u64,
        clock: Clock,
    ) -> Result<(), String> {
        let output = match self.grant.as_ref() {
            Some(grant) => Output::Shared {
                grant_id: grant.id() as u64,
                bytes: tile
                    .rgb_bytes()
                    .map_err(|error| format!("tile bytes: {:?}", error))?
                    as u32,
            },
            None => Output::Copied,
        };
        let request = Request::Render {
            config,
            tile,
            scene_hash,
            output,
        };
        let encoded = api::ipc::encode(&request, &mut self.send)
            .map_err(|_| String::from("request exceeds IPC frame"))?;
        let request_bytes = encoded.len();
        let started_ns = clock.now_ns();
        let call = ostd::ipc::PendingCall::submit(self.worker.tid, encoded)
            .map_err(|error| format!("worker {} admission failed: {:?}", self.worker.tid, error))?;
        // Attach every accepted operation before doing anything else fallible.
        self.pending = Some(ActiveTile {
            call,
            tile,
            started_ns,
            request_bytes,
        });
        Ok(())
    }
}

struct Pool {
    slots: Vec<Slot>,
}

impl Pool {
    fn start(mode: Mode, opts: &Options, scene_hash: u64, tiles: u32) -> Result<Self, String> {
        let count = if mode == Mode::Baseline {
            0
        } else {
            opts.workers.min(tiles)
        };
        let mut pool = Self {
            slots: Vec::with_capacity(count as usize),
        };
        let path = if mode == Mode::Tier2 {
            "/bin/c2c-render-domain-worker"
        } else {
            "/bin/c2c-render-worker"
        };
        for _ in 0..count {
            let worker = Worker::spawn(scene_hash, path)?;
            // Pool ownership precedes allocation/sharing and every later failure.
            pool.slots.push(Slot {
                worker,
                grant: None,
                pending: None,
                send: [0; api::ipc::IPC_BUF_SIZE],
                receive: [0; api::ipc::IPC_BUF_SIZE],
                completed: 0,
            });
            let slot = pool.slots.last_mut().unwrap();
            if mode == Mode::Tier2 && opts.mode == Mode::CompareTier2 {
                slot.worker
                    .require_shared_refusal(opts.config, scene_hash)?;
            }
            if mode == Mode::Shared {
                slot.grant = Some(
                    GrantHandle::<u8>::alloc(COPIED_TILE_MAX_BYTES)
                        .ok_or_else(|| String::from("tile grant allocation failed"))?,
                );
                if !ostd::syscall::sys_grant_share(
                    slot.grant.as_ref().unwrap().id(),
                    slot.worker.tid,
                    2,
                ) {
                    return Err(String::from("sharing tile grant with worker failed"));
                }
            }
        }
        Ok(pool)
    }

    fn finish(&mut self) -> Result<(), String> {
        for slot in &mut self.slots {
            slot.worker.stop()?;
        }
        Ok(())
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        let mut drained = true;
        // Cancellation settles tokens, NOT execution. Retain all grants until
        // every potentially writing child has crossed its quiescence barrier.
        for slot in &mut self.slots {
            if let Some(active) = slot.pending.as_ref() {
                let _ = active.call.cancel();
                match active.call.try_take(&mut slot.receive) {
                    Ok(Some(_)) | Err(ostd::ipc::IpcError::InvalidOperation) => {}
                    _ => drained = false,
                }
            }
        }
        let mut quiescent = true;
        for slot in &mut self.slots {
            // Do not short circuit: attempt termination for ALL holders.
            if !slot.worker.terminate() {
                quiescent = false;
            }
        }
        if !quiescent || !drained {
            ostd::io::println("c2c-render: ERROR pool cleanup incomplete; coordinator retained alive to preserve all worker/grant/operation ownership");
            loop {
                sys_yield();
            }
        }
        // Only now may field destruction release grants and operation handles.
    }
}

/// Distinct hardware endpoint observations, bounded by the native MAX_HARTS=2.
#[derive(Clone, Copy, Default)]
struct ProcessorIds {
    ids: [Option<u32>; 2],
}

impl ProcessorIds {
    fn observe(&mut self, id: Option<u32>) -> Result<(), String> {
        let Some(id) = id else {
            return Ok(());
        };
        if self.ids.contains(&Some(id)) {
            return Ok(());
        }
        let empty = self
            .ids
            .iter_mut()
            .find(|slot| slot.is_none())
            .ok_or_else(|| String::from("processor observations exceed native two-CPU bound"))?;
        *empty = Some(id);
        self.ids.sort_unstable();
        Ok(())
    }
}

impl core::fmt::Display for ProcessorIds {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut separator = "";
        for id in self.ids.iter().flatten() {
            write!(f, "{}{}", separator, id)?;
            separator = ";";
        }
        if separator.is_empty() {
            f.write_str("-")?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Default)]
struct WorkerRecord {
    tid: usize,
    tiles: u32,
    processor_ids: ProcessorIds,
}

struct Measurement {
    mode: Mode,
    wall_ns: u64,
    setup_ns: u64,
    compute_ns: u64,
    wire_bytes: u64,
    pixel_payload_bytes: u64,
    stats: RenderStats,
    latency: Vec<u64>,
    pixels: Vec<u8>,
    workers_started: u32,
    peak_outstanding: u32,
    worker_records: [WorkerRecord; 4],
    processor_ids: ProcessorIds,
}

fn validate_done<'a>(
    raw: &'a [u8],
    tile: Tile,
    config: RenderConfig,
    scene_hash: u64,
    shared: bool,
) -> Result<(RenderStats, u64, &'a [u8], Option<u32>, Option<u32>), String> {
    // Postcard encodes Response::Done as discriminant 0 followed by these
    // fields. Vec<u8>'s length-prefixed byte sequence can be borrowed directly:
    // copied pixels stay in this slot's reusable IPC buffer, without allocating
    // or copying another tile Vec. Tests encode the actual protocol Response.
    let (
        (variant, tile_id, actual, stats, compute_ns, pixels, processor_id_before, processor_id_after),
        remaining,
    ) = postcard::take_from_bytes::<(
        u32,
        u32,
        u64,
        RenderStats,
        u64,
        &[u8],
        Option<u32>,
        Option<u32>,
    )>(raw)
    .map_err(|_| String::from("invalid worker Done response encoding"))?;
    let samples = u64::from(tile.width) * u64::from(tile.height) * u64::from(config.samples);
    let rays_limit = samples
        .checked_mul(u64::from(config.max_bounces))
        .and_then(|value| value.checked_mul(2))
        .ok_or_else(|| String::from("ray bound overflow"))?;
    let bytes = tile
        .rgb_bytes()
        .map_err(|error| format!("tile bytes: {:?}", error))?;
    if variant != 0
        || !remaining.is_empty()
        || tile_id != tile.id
        || actual != scene_hash
        || stats.samples != samples
        || stats.rays < samples
        || stats.rays > rays_limit
        || (shared && !pixels.is_empty())
        || (!shared && pixels.len() != bytes)
    {
        return Err(String::from(
            "worker response ID/fingerprint/stats/pixel-length mismatch",
        ));
    }
    Ok((stats, compute_ns, pixels, processor_id_before, processor_id_after))
}

fn assemble(image: &mut [u8], config: RenderConfig, tile: Tile, pixels: &[u8]) {
    let row_bytes = tile.width as usize * 3;
    for row in 0..tile.height as usize {
        let start = ((tile.y as usize + row) * config.width as usize + tile.x as usize) * 3;
        image[start..start + row_bytes]
            .copy_from_slice(&pixels[row * row_bytes..(row + 1) * row_bytes]);
    }
}

fn tile_at(id: u32, config: RenderConfig, tile_size: u32) -> Tile {
    let columns = config.width.div_ceil(tile_size);
    let x = (id % columns) * tile_size;
    let y = (id / columns) * tile_size;
    Tile {
        id,
        x,
        y,
        width: tile_size.min(config.width - x),
        height: tile_size.min(config.height - y),
    }
}

impl Measurement {
    fn accumulate(
        &mut self,
        stats: RenderStats,
        duration: u64,
        wire_bytes: u64,
        pixel_bytes: u64,
        latency_ns: u64,
    ) -> Result<(), String> {
        self.stats.rays = self
            .stats
            .rays
            .checked_add(stats.rays)
            .ok_or_else(|| String::from("ray total overflow"))?;
        self.stats.samples = self
            .stats
            .samples
            .checked_add(stats.samples)
            .ok_or_else(|| String::from("sample total overflow"))?;
        self.compute_ns = self
            .compute_ns
            .checked_add(duration)
            .ok_or_else(|| String::from("compute duration overflow"))?;
        self.wire_bytes = self
            .wire_bytes
            .checked_add(wire_bytes)
            .ok_or_else(|| String::from("wire byte total overflow"))?;
        self.pixel_payload_bytes = self
            .pixel_payload_bytes
            .checked_add(pixel_bytes)
            .ok_or_else(|| String::from("pixel byte total overflow"))?;
        self.latency.push(latency_ns);
        Ok(())
    }
}

fn render(mode: Mode, opts: &Options, scene: &Scene, clock: Clock) -> Result<Measurement, String> {
    let started = clock.now_ns();
    let config = opts.config;
    let scene_hash = scene.fingerprint();
    let bytes = config
        .rgb_bytes()
        .map_err(|error| format!("image size: {:?}", error))?;
    let tiles = config.width.div_ceil(opts.tile_size) * config.height.div_ceil(opts.tile_size);
    let mut measured = Measurement {
        mode,
        wall_ns: 0,
        setup_ns: 0,
        compute_ns: 0,
        wire_bytes: 0,
        pixel_payload_bytes: 0,
        stats: RenderStats::default(),
        latency: Vec::with_capacity(tiles as usize),
        pixels: vec![0; bytes],
        workers_started: 0,
        peak_outstanding: 0,
        worker_records: [WorkerRecord::default(); 4],
        processor_ids: ProcessorIds::default(),
    };
    let mut scratch = if mode == Mode::Baseline {
        vec![0; COPIED_TILE_MAX_BYTES]
    } else {
        Vec::new()
    };
    let mut pool = Pool::start(mode, opts, scene_hash, tiles)?;
    measured.workers_started = pool.slots.len() as u32;
    measured.setup_ns = clock.elapsed_ns(started);
    if mode == Mode::Baseline {
        for id in 0..tiles {
            let tile = tile_at(id, config, opts.tile_size);
            let bytes = tile
                .rgb_bytes()
                .map_err(|error| format!("tile bytes: {:?}", error))?;
            let processor_id_before = ostd::system_info::processor_id();
            let tile_started = clock.now_ns();
            let stats = render_tile(scene, config, tile, &mut scratch[..bytes])
                .map_err(|error| format!("baseline render: {:?}", error))?;
            let duration = clock.elapsed_ns(tile_started);
            let processor_id_after = ostd::system_info::processor_id();
            measured.processor_ids.observe(processor_id_before)?;
            measured.processor_ids.observe(processor_id_after)?;
            measured.accumulate(stats, duration, 0, 0, duration)?;
            assemble(&mut measured.pixels, config, tile, &scratch[..bytes]);
        }
    } else {
        let mut next_tile = 0;
        let mut completed = 0;
        let mut outstanding = 0;
        while completed < tiles {
            // Fill ALL available slots before consuming even an already-retained
            // terminal. This records accepted, not-yet-consumed operations, not
            // simultaneous computation or an observed processor count.
            for slot in &mut pool.slots {
                if slot.pending.is_none() && next_tile < tiles {
                    slot.submit(
                        config,
                        tile_at(next_tile, config, opts.tile_size),
                        scene_hash,
                        clock,
                    )?;
                    next_tile += 1;
                    outstanding += 1;
                    measured.peak_outstanding = measured.peak_outstanding.max(outstanding);
                }
            }
            let mut progress = false;
            // Scan every worker before waiting; a slow first slot never blocks
            // consuming later workers or refilling their available slots.
            for (index, slot) in pool.slots.iter_mut().enumerate() {
                let Some(active) = slot.pending.as_ref() else {
                    continue;
                };
                let completion = match active.call.try_take(&mut slot.receive) {
                    Ok(Some(completion)) => completion,
                    Ok(None) => continue,
                    Err(error) => {
                        return Err(format!(
                            "worker {} operation failed: {:?}",
                            slot.worker.tid, error,
                        ))
                    }
                };
                let active = slot.pending.take().unwrap();
                outstanding -= 1;
                if completion.terminal != ostd::ipc::IpcTerminal::Reply {
                    return Err(format!(
                        "worker {} operation terminal: {:?}",
                        slot.worker.tid, completion.terminal,
                    ));
                }
                let (stats, duration, pixels, processor_id_before, processor_id_after) = validate_done(
                    &slot.receive[..completion.len],
                    active.tile,
                    config,
                    scene_hash,
                    slot.grant.is_some(),
                )?;
                for id in [processor_id_before, processor_id_after] {
                    measured.processor_ids.observe(id)?;
                    measured.worker_records[index].processor_ids.observe(id)?;
                }
                measured.accumulate(
                    stats,
                    duration,
                    (active.request_bytes + completion.len) as u64,
                    pixels.len() as u64,
                    clock.elapsed_ns(active.started_ns),
                )?;
                if let Some(grant) = slot.grant.as_mut() {
                    let bytes = active
                        .tile
                        .rgb_bytes()
                        .map_err(|error| format!("tile bytes: {:?}", error))?;
                    // Only this slot's validated reply permits reading/reusing
                    // its grant. Image assembly is local, not pixel IPC traffic.
                    grant.with_bytes(|pixels| {
                        assemble(&mut measured.pixels, config, active.tile, &pixels[..bytes]);
                    });
                } else {
                    assemble(&mut measured.pixels, config, active.tile, pixels);
                }
                slot.completed += 1;
                completed += 1;
                progress = true;
            }
            if !progress && !ostd::ipc::wait(0) {
                return Err(String::from("worker pool IPC wait failed"));
            }
        }
    }
    pool.finish()?;
    for (record, slot) in measured.worker_records.iter_mut().zip(&pool.slots) {
        record.tid = slot.worker.tid;
        record.tiles = slot.completed;
    }
    drop(pool);
    measured.wall_ns = clock.elapsed_ns(started);
    measured.latency.sort_unstable();
    Ok(measured)
}

fn report(
    measured: &Measurement,
    opts: &Options,
    scene_hash: u64,
    scene_setup_ns: u64,
    clock: Clock,
) {
    let c = opts.config;
    ostd::io::println(&format!(
        "{},{:016x},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{:016x},{},{},{},{},{},{},{}",
        measured.mode.name(),
        scene_hash,
        c.width,
        c.height,
        c.samples,
        c.max_bounces,
        c.seed,
        opts.tile_size,
        measured.latency.len(),
        clock.frequency(),
        scene_setup_ns,
        measured.wall_ns,
        measured.setup_ns,
        measured.compute_ns,
        measured.wire_bytes,
        measured.pixel_payload_bytes,
        measured.stats.rays,
        measured.stats.samples,
        measured.pixels.len(),
        pixel_hash(&measured.pixels),
        percentile(&measured.latency, 50),
        percentile(&measured.latency, 95),
        percentile(&measured.latency, 99),
        opts.workers,
        measured.workers_started,
        measured.peak_outstanding,
        measured.processor_ids,
    ));
    ostd::io::println(&format!(
        "c2c-render: {} rays={} samples={} RGB_bytes={}",
        measured.mode.name(),
        measured.stats.rays,
        measured.stats.samples,
        measured.pixels.len()
    ));
    ostd::io::println("c2c-render: processor_ids are hardware render instruction endpoint observations, not all-core CPU time or simultaneous-overlap proof; compute_ns is summed elapsed/preemptible render time; peak_outstanding counts unconsumed operations");
    for (index, worker) in measured.worker_records[..measured.workers_started as usize]
        .iter()
        .enumerate()
    {
        ostd::io::println(&format!(
            "c2c-render: worker mode={} index={} tid={} tiles={} processor_ids={}",
            measured.mode.name(),
            index,
            worker.tid,
            worker.tiles,
            worker.processor_ids,
        ));
    }
}

fn save_ppm(path: &str, config: RenderConfig, pixels: &[u8]) -> Result<(), String> {
    use api::ipc::{VfsRequest, VfsResponse};
    let vfs = ostd::service::lookup(ostd::service::service::VFS)
        .ok_or_else(|| String::from("VFS service unavailable; image not saved"))?;
    let mut send = [0u8; api::ipc::IPC_BUF_SIZE];
    let mut receive = [0u8; api::ipc::IPC_BUF_SIZE];
    let header = format!("P6\n{} {}\n255\n", config.width, config.height);
    let mut write = |request: &VfsRequest<'_>| -> Result<(), String> {
        match ostd::ipc::service_call_typed::<_, VfsResponse>(vfs, request, &mut send, &mut receive)
        {
            Ok(VfsResponse::Ok) => Ok(()),
            Ok(response) => Err(format!(
                "VFS write {} failed: {:?}; partial image may remain",
                path, response
            )),
            Err(error) => Err(format!(
                "VFS write {} operation failed: {:?}; completion not proven",
                path, error
            )),
        }
    };
    write(&VfsRequest::Write {
        path,
        content: header.as_bytes(),
    })?;
    for chunk in pixels.chunks(3072) {
        write(&VfsRequest::Append {
            path,
            content: chunk,
        })?;
    }
    // Check the completed file size, not just a successful submission.
    let expected = (header.len() + pixels.len()) as u64;
    match ostd::ipc::service_call_typed::<_, VfsResponse>(
        vfs,
        &VfsRequest::Stat(path),
        &mut send,
        &mut receive,
    ) {
        Ok(VfsResponse::Stat {
            size,
            is_dir: false,
        }) if size == expected => Ok(()),
        result => Err(format!(
            "saved image size verification failed: expected {}, got {:?}",
            expected, result
        )),
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let opts = options(args)?;
    let clock = Clock::new()?;
    let scene_started = clock.now_ns();
    let scene = Scene::demo();
    let scene_setup_ns = clock.elapsed_ns(scene_started);
    let scene_hash = scene.fingerprint();
    ostd::io::println("mode,scene_hash,width,height,samples_per_pixel,max_bounces,seed,tile_size,tiles,timer_hz,scene_setup_ns,wall_ns,setup_ns,compute_ns,render_wire_bytes,pixel_payload_bytes,rays,total_samples,RGB_bytes,pixel_hash,tile_p50_ns,tile_p95_ns,tile_p99_ns,workers_requested,workers_started,peak_outstanding,processor_ids");
    let first = if matches!(opts.mode, Mode::Compare | Mode::CompareTier2) {
        Mode::Baseline
    } else {
        opts.mode
    };
    let baseline = render(first, &opts, &scene, clock)?;
    report(&baseline, &opts, scene_hash, scene_setup_ns, clock);
    if matches!(opts.mode, Mode::Compare | Mode::CompareTier2) {
        let modes: &[Mode] = if opts.mode == Mode::CompareTier2 {
            &[Mode::Tier2]
        } else {
            &[Mode::Copy, Mode::Shared]
        };
        for &mode in modes {
            let measured = render(mode, &opts, &scene, clock)?;
            report(&measured, &opts, scene_hash, scene_setup_ns, clock);
            if measured.pixels != baseline.pixels || measured.stats != baseline.stats {
                let mismatch = measured
                    .pixels
                    .iter()
                    .zip(&baseline.pixels)
                    .position(|(actual, expected)| actual != expected);
                return Err(format!("COMPARE mismatch in {} at RGB byte {:?}; baseline rays/samples {:?}, actual {:?}", mode.name(), mismatch, baseline.stats, measured.stats));
            }
        }
    }
    let save_started = clock.now_ns();
    save_ppm(&opts.output, opts.config, &baseline.pixels)?;
    ostd::io::println(&format!(
        "c2c-render: saved {} bytes={} save_ns={}",
        opts.output,
        baseline.pixels.len(),
        clock.elapsed_ns(save_started)
    ));
    match opts.mode {
        Mode::Compare => ostd::io::println("c2c-render: COMPARE OK baseline=copy=shared"),
        Mode::CompareTier2 => ostd::io::println("c2c-render: COMPARE OK baseline=tier2"),
        _ => ostd::io::println("c2c-render: RENDER OK"),
    }
    Ok(())
}

fn cell_main() {
    let args = ostd::args();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        ostd::io::println(USAGE);
        sys_exit(0);
    }
    match run(&args) {
        Ok(()) => sys_exit(0),
        Err(error) => {
            ostd::io::println(&format!("c2c-render: ERROR {}", error));
            sys_exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_rejects_unsafe_copied_tile_and_remote_modes() {
        assert!(options(&[
            String::from("copy"),
            String::from("--tile-size"),
            String::from("19")
        ])
        .is_err());
        for mode in ["remote", "tier3"] {
            assert!(options(&[String::from(mode)]).is_err());
        }
    }

    #[test]
    fn cli_worker_count_boundaries() {
        for count in ["1", "4"] {
            let opts = options(&[String::from("--workers"), String::from(count)]).unwrap();
            assert_eq!(opts.workers, count.parse::<u32>().unwrap());
        }
        for count in ["0", "5", "-1", "not-a-number"] {
            assert!(options(&[String::from("--workers"), String::from(count)]).is_err());
        }
        assert!(options(&[String::from("--workers")]).is_err());
    }

    #[test]
    fn responses_must_match_identity_budget_and_shared_zero_payload() {
        let config = RenderConfig {
            width: 1,
            height: 1,
            samples: 2,
            max_bounces: 3,
            seed: 0,
        };
        let tile = Tile {
            id: 7,
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        };
        let done = |id, hash, rays, samples, pixels| {
            postcard::to_allocvec(&Response::Done {
                tile_id: id,
                scene_hash: hash,
                stats: RenderStats { rays, samples },
                compute_ns: 1,
                pixels,
                processor_id_before: None,
                processor_id_after: None,
            })
            .unwrap()
        };
        assert!(validate_done(&done(7, 11, 2, 2, Vec::new()), tile, config, 11, true).is_ok());
        assert!(validate_done(&done(8, 11, 2, 2, Vec::new()), tile, config, 11, true).is_err());
        assert!(validate_done(&done(7, 12, 2, 2, Vec::new()), tile, config, 11, true).is_err());
        assert!(validate_done(&done(7, 11, 2, 1, Vec::new()), tile, config, 11, true).is_err());
        for rays in [1, 13] {
            assert!(
                validate_done(&done(7, 11, rays, 2, Vec::new()), tile, config, 11, true).is_err()
            );
        }
        assert!(validate_done(&done(7, 11, 2, 2, vec![0; 3]), tile, config, 11, true).is_err());
        assert!(validate_done(&done(7, 11, 2, 2, Vec::new()), tile, config, 11, false).is_err());
        let copied = done(7, 11, 2, 2, vec![17, 128, 255]);
        let (stats, _duration, pixels, before, after) =
            validate_done(&copied, tile, config, 11, false).unwrap();
        assert_eq!((before, after), (None, None));
        assert_eq!(
            stats,
            RenderStats {
                rays: 2,
                samples: 2
            }
        );
        assert_eq!(pixels, &[17, 128, 255]);
        let mut image = [0; 3];
        assemble(&mut image, config, tile, pixels);
        assert_eq!(image, [17, 128, 255]);
        for end in 0..copied.len() {
            assert!(validate_done(&copied[..end], tile, config, 11, false).is_err());
        }
        let mut trailing = copied.clone();
        trailing.push(0);
        assert!(validate_done(&trailing, tile, config, 11, false).is_err());
        let mut wrong_length = copied.clone();
        let length_index = wrong_length.len() - 6;
        wrong_length[length_index] = 4;
        assert!(validate_done(&wrong_length, tile, config, 11, false).is_err());
        let mut foreign = copied;
        foreign[0] = 1;
        assert!(validate_done(&foreign, tile, config, 11, false).is_err());
        for response in [
            Response::Stopped,
            Response::Ready { scene_hash: 11 },
            Response::Error(RenderError::InvalidOutput),
        ] {
            let raw = postcard::to_allocvec(&response).unwrap();
            assert!(validate_done(&raw, tile, config, 11, false).is_err());
        }
    }

    #[test]
    fn processor_metadata_preserves_missing_and_full_width_endpoint_ids() {
        let config = RenderConfig {
            width: 1,
            height: 1,
            samples: 1,
            max_bounces: 1,
            seed: 0,
        };
        let tile = tile_at(0, config, 1);
        for (before, after) in [
            (None, None),
            (Some(0), None),
            (None, Some(u32::MAX)),
            (Some(256), Some(u32::MAX)),
        ] {
            let raw = postcard::to_allocvec(&Response::Done {
                tile_id: tile.id,
                scene_hash: 11,
                stats: RenderStats { rays: 1, samples: 1 },
                compute_ns: 7,
                pixels: Vec::new(),
                processor_id_before: before,
                processor_id_after: after,
            })
            .unwrap();
            let (_, duration, _, decoded_before, decoded_after) =
                validate_done(&raw, tile, config, 11, true).unwrap();
            assert_eq!((duration, decoded_before, decoded_after), (7, before, after));
        }
    }

    #[test]
    fn processor_observations_deduplicate_and_refuse_capacity_loss() {
        let mut ids = ProcessorIds::default();
        ids.observe(None).unwrap();
        assert_eq!(ids.to_string(), "-");
        ids.observe(Some(u32::MAX)).unwrap();
        ids.observe(Some(u32::MAX)).unwrap();
        ids.observe(None).unwrap();
        ids.observe(Some(0)).unwrap();
        assert_eq!(ids.to_string(), "0;4294967295");
        assert!(ids.observe(Some(1)).is_err());
        assert_eq!(ids.to_string(), "0;4294967295");
        ids.observe(Some(0)).unwrap();
    }

    #[test]
    fn assembling_edge_tiles_preserves_row_order() {
        let config = RenderConfig {
            width: 3,
            height: 2,
            samples: 1,
            max_bounces: 1,
            seed: 0,
        };
        let tile = tile_at(1, config, 2);
        assert_eq!(
            (tile.id, tile.x, tile.y, tile.width, tile.height),
            (1, 2, 0, 1, 2)
        );
        let mut image = [0u8; 18];
        assemble(&mut image, config, tile, &[1, 2, 3, 4, 5, 6]);
        assert_eq!(&image[6..9], &[1, 2, 3]);
        assert_eq!(&image[15..18], &[4, 5, 6]);
    }

    #[test]
    fn nondivisible_tile_grid_covers_each_pixel_once() {
        let config = RenderConfig {
            width: 5,
            height: 3,
            samples: 1,
            max_bounces: 1,
            seed: 0,
        };
        let mut image = [0; 45];
        let mut coverage = [0; 15];
        for id in 0..6 {
            let tile = tile_at(id, config, 2);
            let rgb = [id as u8 + 1; 12];
            assemble(&mut image, config, tile, &rgb[..tile.rgb_bytes().unwrap()]);
            for y in tile.y..tile.y + tile.height {
                for x in tile.x..tile.x + tile.width {
                    coverage[(y * config.width + x) as usize] += 1;
                }
            }
        }
        assert_eq!(coverage, [1; 15]);
        for y in 0..3 {
            for x in 0..5 {
                let offset = (y * 5 + x) * 3;
                assert_eq!(
                    &image[offset..offset + 3],
                    &[1 + (y / 2 * 3 + x / 2) as u8; 3]
                );
            }
        }
    }
}
