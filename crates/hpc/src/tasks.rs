use std::collections::HashMap;
use std::sync::Arc;

use utils::sha256::sha256;
use utils::{Value, to_hex};

/// A task: payload in, result out.
pub type TaskFn = Arc<dyn Fn(&Value) -> Result<Value, String> + Send + Sync>;

/// Maps a job `kind` to the function which executes it.
#[derive(Clone, Default)]
pub struct TaskRegistry {
    tasks: HashMap<String, TaskFn>,
}

impl TaskRegistry {
    pub fn empty() -> Self {
        Self::default()
    }

    /// Registry with the built-in tasks.
    pub fn new() -> Self {
        let mut r = Self::empty();
        r.register("echo", |p| Ok(p.clone()));
        r.register("sleep", |p| {
            let ms = p.get("ms").and_then(|m| m.as_u64()).ok_or("`ms` is required")?;
            if ms > 60_000 {
                return Err("sleep is limited to 60000 ms".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(ms));
            Ok(Value::Int(ms as i64))
        });
        r.register("primes", primes);
        r.register("pi", pi);
        r.register("sha256_chain", |p| {
            let data = p.get("data").and_then(|d| d.as_str()).ok_or("`data` is required")?;
            let rounds = p.get("rounds").and_then(|r| r.as_u64()).unwrap_or(1);
            if rounds == 0 || rounds > 50_000_000 {
                return Err("`rounds` must be 1..=50000000".into());
            }
            let mut h = sha256(data.as_bytes());
            for _ in 1..rounds {
                h = sha256(&h);
            }
            Ok(Value::Str(to_hex(&h)))
        });
        r.register("wordcount", |p| {
            let t = p.get("text").and_then(|t| t.as_str()).ok_or("`text` is required")?;
            Ok(Value::Int(t.split_whitespace().count() as i64))
        });
        r
    }

    pub fn register(&mut self, kind: &str, f: impl Fn(&Value) -> Result<Value, String> + Send + Sync + 'static) -> &mut Self {
        self.tasks.insert(kind.to_string(), Arc::new(f));
        self
    }

    pub fn kinds(&self) -> Vec<String> {
        let mut k: Vec<String> = self.tasks.keys().cloned().collect();
        k.sort();
        k
    }

    pub fn has(&self, kind: &str) -> bool {
        self.tasks.contains_key(kind)
    }

    /// Run a task, turning panics into errors.
    pub fn run(&self, kind: &str, payload: &Value) -> Result<Value, String> {
        let f = self.tasks.get(kind).ok_or_else(|| format!("unknown task kind `{kind}`"))?;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(payload))).unwrap_or_else(|_| Err("task panicked".into()))
    }
}

/// Maximum numbers examined by one `primes` job (memory = 1 byte per number).
pub const MAX_PRIME_SPAN: u64 = 100_000_000;

fn primes(p: &Value) -> Result<Value, String> {
    let start = p.get("start").and_then(|v| v.as_u64()).ok_or("`start` is required")?;
    let end = p.get("end").and_then(|v| v.as_u64()).ok_or("`end` is required")?;
    if end < start {
        return Err("`end` must be >= `start`".into());
    }
    if end - start > MAX_PRIME_SPAN {
        return Err(format!("range too large for one job (max {MAX_PRIME_SPAN}); submit a batch"));
    }
    if end > 1u64 << 40 {
        return Err("`end` is limited to 2^40".into());
    }
    Ok(Value::Int(count_primes(start, end) as i64))
}

/// Count primes in `[start, end)` with a segmented sieve.
pub fn count_primes(start: u64, end: u64) -> u64 {
    let start = start.max(2);
    if end <= start {
        return 0;
    }
    let limit = (end as f64).sqrt() as u64 + 1;
    let mut small = vec![true; (limit + 1) as usize];
    let mut base = Vec::new();
    for i in 2..=limit {
        if small[i as usize] {
            base.push(i);
            let mut j = i * i;
            while j <= limit {
                small[j as usize] = false;
                j += i;
            }
        }
    }
    let mut seg = vec![true; (end - start) as usize];
    for &q in &base {
        if q * q >= end {
            break;
        }
        let mut m = ((start + q - 1) / q * q).max(q * q);
        while m < end {
            seg[(m - start) as usize] = false;
            m += q;
        }
    }
    seg.iter().filter(|b| **b).count() as u64
}

fn pi(p: &Value) -> Result<Value, String> {
    let samples = p.get("samples").and_then(|v| v.as_u64()).ok_or("`samples` is required")?;
    let seed = p.get("seed").and_then(|v| v.as_u64()).unwrap_or(1);
    if samples > 2_000_000_000 {
        return Err("`samples` is limited to 2e9 per job".into());
    }
    // xorshift64*: deterministic for a given seed, so a requeued job gives the same answer
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut next = || {
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    };
    let mut hits = 0i64;
    for _ in 0..samples {
        let (a, b) = (next(), next());
        if a * a + b * b <= 1.0 {
            hits += 1;
        }
    }
    Ok(Value::Int(hits))
}

#[cfg(test)]
mod tests {
    use super::*;
    use utils::json_obj;

    #[test]
    fn prime_counts() {
        assert_eq!(count_primes(0, 100), 25);
        assert_eq!(count_primes(0, 100_000), 9592);
        assert_eq!(count_primes(2, 3), 1);
        assert_eq!(count_primes(90, 100), 1); // 97
        assert_eq!(count_primes(100, 90), 0);
        // segments add up
        assert_eq!(count_primes(0, 50_000) + count_primes(50_000, 100_000), 9592);
    }

    #[test]
    fn builtin_tasks() {
        let r = TaskRegistry::new();
        assert_eq!(r.run("echo", &Value::Int(4)), Ok(Value::Int(4)));
        assert_eq!(r.run("primes", &json_obj! {"start" => 0, "end" => 1000}), Ok(Value::Int(168)));
        assert!(r.run("primes", &json_obj! {"start" => 0, "end" => 10_000_000_000u64}).is_err());
        assert!(r.run("nope", &Value::Null).unwrap_err().contains("unknown"));
        assert_eq!(
            r.run("sha256_chain", &json_obj! {"data" => "abc"}),
            Ok(Value::from("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"))
        );
        assert_eq!(r.run("wordcount", &json_obj! {"text" => "a b  c\nd"}), Ok(Value::Int(4)));
        let a = r.run("pi", &json_obj! {"samples" => 1000, "seed" => 3});
        assert_eq!(a, r.run("pi", &json_obj! {"samples" => 1000, "seed" => 3}));
        let hits = r.run("pi", &json_obj! {"samples" => 400_000, "seed" => 1}).unwrap().as_f64().unwrap();
        assert!((4.0 * hits / 400_000.0 - std::f64::consts::PI).abs() < 0.02);
    }

    #[test]
    fn panics_are_contained() {
        let mut r = TaskRegistry::empty();
        r.register("boom", |_| panic!("x"));
        assert!(r.run("boom", &Value::Null).unwrap_err().contains("panicked"));
    }
}
