use ostd::prelude::*;
use ostd::fs;
use ostd::syscall;

pub fn cmd_help() -> ViResult<()> {
    ostd::io::println("Available commands: help, ls, cat, clear, exec");
    Ok(())
}

pub fn cmd_clear() -> ViResult<()> {
    // ANSI escape code for clear screen
    ostd::io::print("\x1b[2J\x1b[1;1H");
    Ok(())
}

pub fn cmd_exec<'a>(mut args: core::str::SplitWhitespace<'a>) -> ViResult<()> {
    let path = args.next();
    if path.is_none() {
        ostd::io::println("Usage: exec <path>");
        return Ok(());
    }
    let path = path.unwrap();

    // Read file from VFS into memory
    // This uses the User VFS (RamDisk), bypassing Kernel VIFS1 mismatch.
    match syscall::sys_open(path) {
        Ok(fd) => {
            // Read all data
            // Since we don't know size, we read chunks until EOF.
            // Vector to hold data
            let mut data: Vec<u8> = Vec::new();
            let mut buf = [0u8; 512];
            loop {
                match syscall::sys_read(fd, &mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        data.extend_from_slice(&buf[..n]);
                    },
                    Err(_) => {
                        ostd::io::println("exec: read error");
                        syscall::sys_close(fd);
                        return Ok(());
                    }
                }
            }
            syscall::sys_close(fd);

            if data.is_empty() {
                ostd::io::println("exec: empty file");
                return Ok(());
            }

            // Invoke SpawnFromMem
            // We pass the slice pointer and length
            ostd::io::print("exec: spawning ");
            ostd::io::print(path);
            ostd::io::print(" (");
            // print len? we don't have int formatting in ostd::io::print easily unless using format!
            ostd::io::println(" bytes)");

            match syscall::sys_spawn_from_mem(&data, path) {
                syscall::SyscallResult::Ok(tid) => {
                     // Wait for task
                     ostd::io::print("exec: waiting for pid ");
                     // ostd::io::print(tid...);
                     ostd::io::println("...");

                     match syscall::sys_wait(tid) {
                         syscall::SyscallResult::Ok(code) => {
                             ostd::io::print("exec: process exited with code ");
                             // ostd::io::println(code...);
                             ostd::io::println(".");
                         },
                         _ => {
                             ostd::io::println("exec: wait failed (detached?)");
                         }
                     }
                },
                syscall::SyscallResult::Err(_) => {
                     ostd::io::println("exec: spawn failed");
                }
            }
            Ok(())
        },
        Err(_) => {
            ostd::io::print("exec: cannot open '");
            ostd::io::print(path);
            ostd::io::println("'");
            Ok(())
        }
    }
}

pub fn cmd_ls<'a>(mut args: core::str::SplitWhitespace<'a>) -> ViResult<()> {
    let path = args.next().unwrap_or("/");

    // Using ostd::fs::read_dir
    match fs::read_dir(path) {
        Ok(iter) => {
            for entry in iter {
                // entry is DirEntry
                let name = core::str::from_utf8(&entry.name).unwrap_or("???");
                // trimming null bytes
                let name = name.trim_matches('\0');
                ostd::io::println(name);
            }
            Ok(())
        },
        Err(e) => {
             // Use e to avoid unused variable warning
             ostd::io::print("ls: cannot access '");
             ostd::io::print(path);
             ostd::io::print("': ");
             match e {
                 ViError::NotFound => ostd::io::println("No such file or directory"),
                 ViError::PermissionDenied => ostd::io::println("Permission denied"),
                 _ => ostd::io::println("Error"),
             }
             // Return Ok so shell doesn't crash on user error
             Ok(())
        }
    }
}

pub fn cmd_cat<'a>(mut args: core::str::SplitWhitespace<'a>) -> ViResult<()> {
    let path = args.next();
    if path.is_none() {
        ostd::io::println("Usage: cat <filename>");
        return Ok(());
    }
    let path = path.unwrap();

    match syscall::sys_open(path) {
        Ok(fd) => {
            let mut buffer = [0u8; 256]; // Stack buffer
            let mut pending = 0; // Number of bytes pending from previous read
            loop {
                // Read into buffer starting after pending bytes
                // Buffer size is small, so we must be careful not to overflow if pending is large (max utf8 char is 4 bytes)
                let _max_read = buffer.len() - pending;
                match syscall::sys_read(fd, &mut buffer[pending..]) {
                    Ok(n) if n > 0 => {
                        let total = pending + n;

                        match core::str::from_utf8(&buffer[..total]) {
                            Ok(s) => {
                                ostd::io::print(s);
                                pending = 0;
                            },
                            Err(e) => {
                                let valid_len = e.valid_up_to();
                                if valid_len > 0 {
                                    let s = unsafe { core::str::from_utf8_unchecked(&buffer[..valid_len]) };
                                    ostd::io::print(s);
                                }

                                if let Some(error_len) = e.error_len() {
                                    // Invalid sequence encountered
                                    ostd::io::print("\u{FFFD}"); // Replacement char
                                    // Skip the invalid part
                                    let start = valid_len + error_len;
                                    let remaining = total - start;
                                    // Move remaining to start of buffer
                                    for i in 0..remaining {
                                        buffer[i] = buffer[start + i];
                                    }
                                    pending = remaining;
                                } else {
                                    // Incomplete sequence at end
                                    let remaining = total - valid_len;
                                    // Move remaining to start of buffer
                                    for i in 0..remaining {
                                        buffer[i] = buffer[valid_len + i];
                                    }
                                    pending = remaining;
                                }
                            }
                        }
                    },
                    Ok(0) => {
                         // EOF
                         if pending > 0 {
                             // Print remaining bytes as replacement chars or similar?
                             // Just ignore for now or print replacement
                             ostd::io::print("\u{FFFD}");
                         }
                         break;
                    },
                    Err(_) => {
                        ostd::io::println("cat: read error");
                        break;
                    }
                     _ => break,
                }
            }
            syscall::sys_close(fd);
            ostd::io::println(""); // Newline at end
            Ok(())
        },
        Err(_) => {
            ostd::io::print("cat: ");
            ostd::io::print(path);
            ostd::io::println(": No such file or directory");
             // Return Ok to keep shell running
             Ok(())
        }
    }
}
