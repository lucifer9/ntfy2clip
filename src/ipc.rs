use crate::clipboard::Snapshot;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};

pub const MAX_FRAME: usize = 16 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub operation: Operation,
    pub budget_ms: u64,
}
#[derive(Serialize, Deserialize)]
pub enum Operation {
    Capabilities,
    Read,
    Write(String),
}
#[derive(Serialize, Deserialize)]
pub struct Response {
    pub pid: u32,
    pub version: u32,
    pub id: u64,
    pub result: ResponseResult,
}
#[derive(Serialize, Deserialize)]
pub enum ResponseResult {
    Ready,
    Snapshot { value: Snapshot, revision: u64 },
    Written,
    Temporary,
    TemporaryReason(String),
    Permanent,
}

#[derive(Debug)]
pub struct EndOfStream;
impl std::fmt::Display for EndOfStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("IPC input closed")
    }
}
impl std::error::Error for EndOfStream {}

pub fn read_frame<T: serde::de::DeserializeOwned>(input: &mut impl Read) -> Result<T> {
    let mut length = [0; 4];
    if input.read(&mut length[..1])? == 0 {
        return Err(EndOfStream.into());
    }
    input.read_exact(&mut length[1..])?;
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > MAX_FRAME {
        bail!("invalid IPC frame length");
    }
    let mut bytes = vec![0; length];
    input.read_exact(&mut bytes)?;
    serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("invalid IPC frame"))
}
pub fn write_frame<T: Serialize>(output: &mut impl Write, value: &T) -> Result<()> {
    let body = encode(value)?;
    output.write_all(&body)?;
    output.flush()?;
    Ok(())
}
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let body = serde_json::to_vec(value)?;
    if body.len() > MAX_FRAME {
        bail!("IPC frame exceeds limit");
    }
    let mut framed = Vec::with_capacity(body.len() + 4);
    framed.extend_from_slice(&(body.len() as u32).to_be_bytes());
    framed.extend(body);
    Ok(framed)
}
