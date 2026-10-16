#![expect(
    clippy::unwrap_used,
    reason = "a test helper panics on a failed step, which fails the calling test"
)]

mod docs;
mod flows;
mod harness;
mod repo;
mod reports;
