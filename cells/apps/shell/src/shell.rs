use crate::async_utils::AsyncStdin;
use crate::commands;
use crate::config_client::ConfigClient;
use api::config::ViConfig;
use ostd::prelude::*;

use alloc::collections::VecDeque;
use alloc::string::String;

pub struct ViShell<'a> {
    prompt: &'a str,
    config: ConfigClient,
    history: VecDeque<String>,
}

impl<'a> ViShell<'a> {
    pub fn new() -> Self {
        // Assume Config Service is Cell 2 (Init=1)
        Self {
            prompt: "ViOS > ",
            config: ConfigClient::new(2),
            history: VecDeque::with_capacity(32),
        }
    }

    pub async fn run(&mut self) {
        let stdin = AsyncStdin;
        loop {
            // Show custom prompt if PATH set? Or USER?
            // For now static prompt.
            ostd::io::print(self.prompt);

            let buffer = stdin.read_line(128, &mut self.history).await;
            let len = buffer.len();

            if len > 0 {
                if let Ok(line) = core::str::from_utf8(&buffer) {
                    // Add to history if not empty and not repeat of last
                    let trim_line = line.trim();
                    if !trim_line.is_empty() {
                         if self.history.back().map(|s| s.as_str()) != Some(trim_line) {
                             if self.history.len() >= 32 {
                                 self.history.pop_front();
                             }
                             self.history.push_back(String::from(trim_line));
                         }
                    }
                    
                    let _ = self.dispatch(line).await;
                }
            }
        }
    }

    pub async fn dispatch(&self, line: &str) -> ViResult<()> {
        let mut parts = line.trim().split_whitespace();
        let cmd = parts.next().ok_or(ViError::InvalidInput)?;

        match cmd {
            "help" => commands::cmd_help(),
            "clear" => commands::cmd_clear(),
            "exec" => commands::cmd_exec(parts),
            "ls" => commands::cmd_ls(parts),
            "cat" => commands::cmd_cat(parts),
            "ps" => commands::cmd_ps(parts),
            "" => Ok(()),
            "export" => {
                // export KEY=VALUE
                if let Some(arg) = parts.next() {
                    if let Some((k, v)) = arg.split_once('=') {
                        // self.config is immutable reference here, but set requires mutable?
                        // Actually I defined set(&mut self) in Trait?
                        // Let's check config.rs. Yes `set(&mut self)`.
                        // But `dispatch` takes `&self`.
                        // We need interior mutability for client? No, client just sends IPC.
                        // I should change `ConfigClient::set` to take `&self`. IPC doesn't modify local state.
                        // Let's modify Trait first? No, ABI stability.
                        // Wait, `ConfigClient` is just a handle. It doesn't need to be mutable to send message.
                        // I will update `config_client.rs` to take `&self` for `set`.
                        // And update trait if possible?
                        // Trait says `set(&mut self)`.
                        // So I must have `&mut self` in dispatch?
                        // `run` calls `dispatch`. `run` has `&self`.
                        // ViShell needs to be mutable? Or wrap ConfigClient in RefCell.
                        // Or just bypass trait for now and call method directly?
                        // Let's use unsafe cast to mutable for MVP since we know Client is stateless.
                        // Or better: Clone the client? It's lightweight.
                        let mut client = ConfigClient::new(2);
                        client.set(k, v)
                    } else {
                        Ok(())
                    }
                } else {
                    Ok(())
                }
            }
            "echo" => {
                // echo $VAR or echo text
                for arg in parts {
                    if arg.starts_with('$') {
                        let key = &arg[1..];
                        if let Ok(val) = self.config.get(key) {
                            ostd::io::print(val);
                        }
                    } else {
                        ostd::io::print(arg);
                    }
                    ostd::io::print(" ");
                }
                ostd::io::println("");
                Ok(())
            }
            _ => {
                ostd::io::print("ViOS: command not found: ");
                ostd::io::println(cmd);
                Ok(())
            }
        }
    }
}
