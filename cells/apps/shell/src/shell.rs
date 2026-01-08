use ostd::prelude::*;
use crate::commands;
use crate::async_utils::AsyncStdin;

pub struct ViShell<'a> {
    prompt: &'a str,
}

impl<'a> ViShell<'a> {
    pub fn new() -> Self {
        Self { prompt: "ViOS > " }
    }

    pub async fn run(&self) {
        let stdin = AsyncStdin;
        loop {
            // Print prompt
            ostd::io::print(self.prompt);

            // Read input asynchronously
            let mut buffer = [0u8; 128];
            let len = stdin.read_line(&mut buffer).await;

            if len > 0 {
                if let Ok(line) = core::str::from_utf8(&buffer[..len]) {
                     let _ = self.dispatch(line).await;
                }
            }
        }
    }

    pub async fn dispatch(&self, line: &str) -> ViResult<()> {
        let mut parts = line.trim().split_whitespace();
        let cmd = parts.next().ok_or(ViError::InvalidInput)?;

        match cmd {
            "ls" => commands::cmd_ls(parts), // ls is sync for now
            "cat" => commands::cmd_cat(parts), // cat is sync for now
            "help" => commands::cmd_help(),
            "clear" => commands::cmd_clear(),
            "exec" => commands::cmd_exec(parts),
            _ => {
                ostd::io::print("ViOS: command not found: ");
                ostd::io::println(cmd);
                Ok(())
            }
        }
    }
}
