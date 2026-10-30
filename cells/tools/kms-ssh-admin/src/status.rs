//! `kms-ssh-admin status [NAME]`: leaf status (no name) or one key's facts (op 19).

use crate::console::Console;
use crate::kms::Kms;
use crate::report::{self, EXIT_NO_TERMINAL, EXIT_OK};

pub fn run(name: Option<&str>, console: &mut dyn Console, kms: &dyn Kms) -> i32 {
    match kms.status(name.unwrap_or("").as_bytes()) {
        Ok(status) => {
            let text = match name {
                Some(name) => report::key_status(name, &status),
                None => report::leaf_status(&status),
            };
            console.out(&text);
            if console.lost() {
                EXIT_NO_TERMINAL
            } else {
                EXIT_OK
            }
        }
        Err(error) => {
            let (code, message) = report::kms_failure(error);
            console.err(&message);
            code
        }
    }
}
