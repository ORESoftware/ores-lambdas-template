//! One strict JSON invocation on stdin → one JSON receipt on stdout. For hosts that own their
//! own event loop (Cloudflare Workers via a sidecar, OCI jobs, cron, local shells).
use crate::runtime::{handle, Provider, Receipt, MAX_INVOCATION_BYTES};
use std::io::{Read, Write};

pub fn run<R: Read, W: Write>(mut input: R, mut output: W) -> std::io::Result<Receipt> {
    let mut raw = Vec::with_capacity(4096);
    input
        .take((MAX_INVOCATION_BYTES + 1) as u64)
        .read_to_end(&mut raw)?;
    let receipt = handle(&raw, Provider::Local, "stdin");
    serde_json::to_writer(&mut output, &receipt)?;
    output.write_all(b"\n")?;
    Ok(receipt)
}
