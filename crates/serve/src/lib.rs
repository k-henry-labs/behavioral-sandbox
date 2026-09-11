//! `tormoni serve`: one box that runs sandboxes for a caller over HTTP.
//!
//! - **One tenant, one token, one box.** No account, no role, no membership, no placement. This
//!   takes a posture and a command and gives back a record, exactly as the CLI does; which box a
//!   job lands on is a question for something that knows about more than one.
//! - **The record is the contract.** A served run leaves byte for byte the archive a local run
//!   leaves. Nothing here writes a record of its own or adds a field to one.
//! - **The posture is not relaxed because it is a server.** `network none` means the same thing
//!   here as it does at a keyboard. A served run that is quietly less isolated than a local one is
//!   the worst defect this product could have.
//! - **Units, never money.** [`meter`] says what a sandbox held and for how long; what that costs
//!   is asked somewhere else.

#![forbid(unsafe_code)]

pub mod config;
pub mod http;
pub mod meter;
