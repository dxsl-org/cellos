//! `kms-ssh-admin delete NAME`: status first (op 19) for the stored public key, print what
//! is about to go, require the operator to type `delete` + Enter (Escape or any other word
//! cancels), then op 20 with the public key and the name. Without the confirmation no
//! op 20 is ever sent.

use crate::console::{Console, Word};
use crate::kms::Kms;
use crate::report::{self, EXIT_CANCELLED, EXIT_NO_TERMINAL, EXIT_OK};

pub const CONFIRMATION: &str = "delete";

pub fn run(name: &str, console: &mut dyn Console, kms: &dyn Kms) -> i32 {
    let status = match kms.status(name.as_bytes()) {
        Ok(status) => status,
        Err(error) => {
            let (code, message) = report::kms_failure(error);
            console.err(&message);
            return code;
        }
    };
    console.out(&report::delete_target(name, &status));
    console.err(report::DELETE_PROMPT);
    let word = console.read_word();
    console.err("\n");
    match word {
        Word::Given(word) if word == CONFIRMATION => {}
        Word::Lost => return EXIT_NO_TERMINAL,
        Word::Given(_) | Word::Cancelled => {
            console.err(report::DELETE_CANCELLED);
            return EXIT_CANCELLED;
        }
    }
    match kms.delete(name.as_bytes(), &status.public_key) {
        Ok(()) => {
            console.out(&report::deleted(name));
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
