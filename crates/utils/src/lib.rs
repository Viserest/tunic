//! Dependency-free helpers shared by every tunic crate.
//!
//! * [`Uuid`], [`sha256`](sha256::sha256), [`password`] hashing
//! * [`json`] and [`toml`] (subset) parsers sharing one [`Value`] tree
//! * [`NetworkRule`] (`ip/prefix:port/port`) used by router access rules and database ports
//! * [`Shutdown`] cooperative shutdown signal (with Ctrl-C support on unix)

pub mod json;
pub mod net;
pub mod password;
pub mod sha256;
pub mod shutdown;
pub mod text;
pub mod toml;
pub mod uuid;

pub use json::Value;
pub use net::NetworkRule;
pub use shutdown::Shutdown;
pub use text::*;
pub use uuid::Uuid;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Seconds since the unix epoch.
pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

/// Milliseconds since the unix epoch.
pub fn unix_now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64
}

/// Cryptographically secure random bytes (from the OS).
///
/// Falls back to a clock/address mixed generator only if the OS source is unavailable, which
/// never happens on supported (unix) platforms.
pub fn random_bytes(n: usize) -> Vec<u8> {
    use std::io::Read;
    let mut buf = vec![0u8; n];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        if f.read_exact(&mut buf).is_ok() {
            return buf;
        }
    }
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let mut i = 0;
    while i < n {
        let mut h = RandomState::new().build_hasher();
        h.write_u128(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos());
        h.write_usize(i);
        for b in h.finish().to_le_bytes() {
            if i < n {
                buf[i] = b;
                i += 1;
            }
        }
    }
    buf
}

/// A uniformly distributed u64 from [`random_bytes`].
pub fn random_u64() -> u64 {
    let b = random_bytes(8);
    u64::from_le_bytes(b.try_into().unwrap())
}

/// Lock a mutex, recovering the data if another thread panicked while holding it.
pub fn lock<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
