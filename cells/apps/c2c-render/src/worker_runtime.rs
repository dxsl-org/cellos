//! One renderer service loop for the trusted and copied-only domain entries.

use crate::Clock;
use alloc::vec::Vec;
use ostd::syscall::{sys_exit, sys_recv, SyscallResult};
use render_core::{
    render_tile, Output, RenderError, Request, Response, Scene, COPIED_TILE_MAX_BYTES,
};

/// The domain entry instantiates only `run::<false>`, eliminating its grant path
/// at monomorphization as well as withholding the grant syscall in its manifest.
pub fn run<const ALLOW_SHARED: bool>() -> ! {
    let clock = match Clock::new() {
        Ok(clock) => clock,
        Err(error) => {
            ostd::io::println(&alloc::format!("c2c-render-worker: {}", error));
            sys_exit(1);
        }
    };
    let scene = Scene::demo();
    let scene_hash = scene.fingerprint();
    let mut owner = None;
    let mut copied = Vec::with_capacity(COPIED_TILE_MAX_BYTES);
    let mut incoming = [0u8; api::ipc::IPC_BUF_SIZE];
    let mut outgoing = [0u8; api::ipc::IPC_BUF_SIZE];
    loop {
        incoming.fill(0);
        let sender = match sys_recv(owner.unwrap_or(0), &mut incoming) {
            SyscallResult::Ok(sender) if sender > 0 => sender,
            result => {
                ostd::io::println(&alloc::format!(
                    "c2c-render-worker: receive failed: {:?}",
                    result
                ));
                sys_exit(1);
            }
        };
        // Only kernel-owned operations are served. A raw mailbox message cannot
        // acquire a grant write window or become a renderer reply.
        let Some(operation) = ostd::ipc::current() else {
            continue;
        };
        if owner.is_some_and(|expected| expected != sender) {
            continue;
        }
        let request = postcard::take_from_bytes::<Request>(&incoming)
            .ok()
            .filter(|(_, tail)| tail.iter().all(|byte| *byte == 0))
            .map(|(request, _)| request);
        let mut stopping = false;
        let response = match request {
            Some(Request::Ready) if owner.is_none() || owner == Some(sender) => {
                owner = Some(sender);
                Response::Ready { scene_hash }
            }
            Some(Request::Stop) if owner == Some(sender) => {
                stopping = true;
                Response::Stopped
            }
            Some(Request::Render {
                config,
                tile,
                scene_hash: requested,
                output,
            }) if owner == Some(sender) => {
                // Refuse all Shared outputs at the domain boundary, before any
                // handle conversion, grant access, validation or rendering.
                if !ALLOW_SHARED && matches!(output, Output::Shared { .. }) {
                    Response::Error(RenderError::InvalidOutput)
                } else if requested != scene_hash {
                    Response::Error(RenderError::SceneMismatch)
                } else {
                    match tile.validated(config).and_then(|tile| {
                        let bytes = tile.rgb_bytes()?;
                        let (stats, compute_ns, pixels, processor_id_before, processor_id_after) =
                            match output {
                            Output::Copied => {
                                if bytes > COPIED_TILE_MAX_BYTES {
                                    return Err(RenderError::InvalidOutput);
                                }
                                copied.resize(bytes, 0);
                                let processor_id_before = ostd::system_info::processor_id();
                                let started = clock.now_ns();
                                let stats = render_tile(&scene, config, tile, &mut copied)?;
                                let compute_ns = clock.elapsed_ns(started);
                                let processor_id_after = ostd::system_info::processor_id();
                                (
                                    stats,
                                    compute_ns,
                                    core::mem::take(&mut copied),
                                    processor_id_before,
                                    processor_id_after,
                                )
                            }
                            Output::Shared {
                                grant_id,
                                bytes: declared,
                            } => {
                                // A compile-time boundary also excludes this
                                // branch from the copied-only instantiation.
                                if !ALLOW_SHARED {
                                    return Err(RenderError::InvalidOutput);
                                }
                                if declared as usize != bytes {
                                    return Err(RenderError::OutputLength);
                                }
                                let grant_id = usize::try_from(grant_id)
                                    .map_err(|_| RenderError::InvalidOutput)?;
                                // The coordinator is blocked on this exact operation;
                                // the closure ends before reply and retains no mapping.
                                let (stats, duration, processor_id_before, processor_id_after) =
                                    ostd::grant::with_shared_bytes_mut(grant_id, |buffer| {
                                        let target = buffer
                                            .get_mut(..bytes)
                                            .ok_or(RenderError::OutputLength)?;
                                        let processor_id_before = ostd::system_info::processor_id();
                                        let started = clock.now_ns();
                                        let stats = render_tile(&scene, config, tile, target)?;
                                        let duration = clock.elapsed_ns(started);
                                        let processor_id_after = ostd::system_info::processor_id();
                                        Ok::<_, RenderError>((
                                            stats,
                                            duration,
                                            processor_id_before,
                                            processor_id_after,
                                        ))
                                    })
                                    .ok_or(RenderError::InvalidOutput)??;
                                (
                                    stats,
                                    duration,
                                    Vec::new(),
                                    processor_id_before,
                                    processor_id_after,
                                )
                            }
                        };
                        Ok(Response::Done {
                            tile_id: tile.id,
                            scene_hash,
                            stats,
                            compute_ns,
                            pixels,
                            processor_id_before,
                            processor_id_after,
                        })
                    }) {
                        Ok(response) => response,
                        Err(error) => Response::Error(error),
                    }
                }
            }
            _ => Response::Error(RenderError::InvalidOutput),
        };
        let sent = api::ipc::encode(&response, &mut outgoing)
            .map_err(|_| ostd::ipc::IpcError::Encode)
            .and_then(|encoded| ostd::ipc::reply(operation, encoded));
        if let Response::Done { pixels, .. } = response {
            if !pixels.is_empty() {
                copied = pixels;
            }
        }
        if let Err(error) = sent {
            // No further writes after an expired/dead caller operation.
            ostd::io::println(&alloc::format!(
                "c2c-render-worker: reply failed: {:?}",
                error
            ));
            sys_exit(1);
        }
        if stopping {
            sys_exit(0);
        }
    }
}
