#![no_std]
extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String;

#[no_mangle]
pub fn app_main() -> ! {
    ostd::println!("\n\n=== ViOS Interactive Shell v0.1 ===");
    ostd::println!("Type 'help' for commands.");

    let mut history: Vec<String> = Vec::new();

    loop {
        // Print Prompt
        ostd::print!("\nViOS$ ");
        
        let mut line = String::new();
        if let Ok(_) = ostd::io::stdin().read_line(&mut line) {
            let input = line.trim();
            if input.is_empty() { continue; }
            
            match input {
                "help" => {
                    ostd::println!("\nAvailable Commands:");
                    ostd::println!("  help       : Show this help");
                    ostd::println!("  clear      : Clear screen");
                    ostd::println!("  cat <file> : Read file content");
                    ostd::println!("  echo <txt> : Print text");
                    ostd::println!("  history    : Show command history");
                }
                "clear" => ostd::print!("\x1b[2J\x1b[H"),
                "history" => {
                    ostd::println!("");
                    for (i, cmd) in history.iter().enumerate() {
                        ostd::println!("  {}: {}", i, cmd);
                    }
                }
                cmd if cmd.starts_with("cat ") => {
                    let path = cmd[4..].trim();
                    ostd::println!(""); 
                    match ostd::syscall::sys_open(path) {
                        Ok(fd) => {
                            let mut buf = [0u8; 64];
                            loop {
                                match ostd::syscall::sys_read(fd, &mut buf) {
                                    Ok(0) => break, // EOF
                                    Ok(n) => {
                                        if let Ok(s) = core::str::from_utf8(&buf[0..n]) {
                                            ostd::print!("{}", s);
                                        } else {
                                            ostd::print!(".");
                                        }
                                    }
                                    Err(_) => {
                                        ostd::println!("Error reading file");
                                        break;
                                    }
                                }
                            }
                            ostd::syscall::sys_close(fd);
                        } 
                        Err(_) => ostd::println!("File not found: {}", path),
                    }
                }
                "ls" => {
                    // TODO: Use readdir when available.
                    // For now, listing is not fully supported via readdir syscall.
                    // We can just print a message or try to open "." if implemented.
                   ostd::println!("\n[.]");
                   ostd::println!("hello.txt (File)");
                   ostd::println!("EFI (Dir)");
                }
                "pwd" => {
                    let mut buf = [0u8; 256];
                    match ostd::syscall::sys_getcwd(&mut buf) {
                         ostd::syscall::SyscallResult::Ok(len) => {
                             if let Ok(path) = core::str::from_utf8(&buf[..len]) {
                                 ostd::println!("\n{}", path);
                             }
                         }
                         _ => ostd::println!("\n/ (unknown)"),
                    }
                }
                cmd if cmd.starts_with("cd ") => {
                    let path = cmd[3..].trim();
                    match ostd::syscall::sys_chdir(path) {
                        ostd::syscall::SyscallResult::Ok(_) => {}, // success
                        _ => ostd::println!("\nDirectory not found: {}", path),
                    }
                }
                cmd if cmd.starts_with("echo ") => {
                    ostd::println!("\n{}", &cmd[5..]);
                }
                _ => ostd::println!("\nUnknown command: '{}'", input),
            }
            
            history.push(String::from(input));
        }
    }
}
