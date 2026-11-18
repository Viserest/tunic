//! Frame format: `[u8 type][u32 big-endian length][json payload]`.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use utils::{Shutdown, Value};

pub const REQUEST: u8 = 0;
pub const RESPONSE: u8 = 1;
pub const SYNC: u8 = 2;
pub const HELLO: u8 = 3;

pub const MAGIC: &str = "tunic-db/1";
const MAX_FRAME: usize = 256 * 1024 * 1024;

pub fn write_frame(w: &mut impl Write, ty: u8, v: &Value) -> io::Result<()> {
    let payload = v.to_json().into_bytes();
    if payload.len() > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "frame too large"));
    }
    let mut buf = Vec::with_capacity(5 + payload.len());
    buf.push(ty);
    buf.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    buf.extend_from_slice(&payload);
    w.write_all(&buf)?;
    w.flush()
}

fn is_timeout(e: &io::Error) -> bool {
    matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted)
}

/// Read one frame. With a `shutdown`, an idle connection is abandoned when it triggers (the
/// stream must have a short read timeout). `Ok(None)` = clean EOF or shutdown.
pub fn read_frame(r: &mut BufReader<TcpStream>, shutdown: Option<&Shutdown>) -> io::Result<Option<(u8, Value)>> {
    // wait for the first byte
    loop {
        match r.fill_buf() {
            Ok(b) if b.is_empty() => return Ok(None),
            Ok(_) => break,
            Err(e) if is_timeout(&e) => {
                if shutdown.is_some_and(|s| s.is_triggered()) {
                    return Ok(None);
                }
                if shutdown.is_none() {
                    return Err(e);
                }
            }
            Err(e) => return Err(e),
        }
    }
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut head = [0u8; 5];
    read_exact(r, &mut head, deadline)?;
    let len = u32::from_be_bytes([head[1], head[2], head[3], head[4]]) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large"));
    }
    let mut payload = vec![0u8; len];
    read_exact(r, &mut payload, deadline)?;
    let text = String::from_utf8(payload).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "frame is not utf-8"))?;
    let v = utils::json::parse(&text).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    Ok(Some((head[0], v)))
}

fn read_exact(r: &mut impl Read, buf: &mut [u8], deadline: Instant) -> io::Result<()> {
    let mut got = 0;
    while got < buf.len() {
        match r.read(&mut buf[got..]) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => got += n,
            Err(e) if is_timeout(&e) && Instant::now() < deadline => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
