use database::{Record, Uuid, Value, field_enum};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobState {
    Queued,
    Running,
    Done,
    Failed,
}
field_enum!(JobState { Queued => "queued", Running => "running", Done => "done", Failed => "failed" });

impl JobState {
    pub fn is_terminal(&self) -> bool {
        matches!(self, JobState::Done | JobState::Failed)
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            JobState::Queued => "queued",
            JobState::Running => "running",
            JobState::Done => "done",
            JobState::Failed => "failed",
        }
    }
    pub fn parse(s: &str) -> Option<JobState> {
        Some(match s {
            "queued" => JobState::Queued,
            "running" => JobState::Running,
            "done" => JobState::Done,
            "failed" => JobState::Failed,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reduce {
    Sum,
    Max,
    Min,
    /// Array of all results in job order.
    Concat,
}
field_enum!(Reduce { Sum => "sum", Max => "max", Min => "min", Concat => "concat" });

impl Reduce {
    pub fn parse(s: &str) -> Option<Reduce> {
        Some(match s {
            "sum" => Reduce::Sum,
            "max" => Reduce::Max,
            "min" => Reduce::Min,
            "concat" => Reduce::Concat,
            _ => return None,
        })
    }

    /// Combine job results (in job order).
    pub fn apply(&self, results: &[Value]) -> Result<Value, String> {
        match self {
            Reduce::Concat => Ok(Value::Array(results.to_vec())),
            _ if results.is_empty() => Ok(Value::Null),
            Reduce::Sum => {
                if results.iter().all(|r| matches!(r, Value::Int(_))) {
                    let mut t: i64 = 0;
                    for r in results {
                        t = t.checked_add(r.as_i64().unwrap()).ok_or("sum overflow")?;
                    }
                    Ok(Value::Int(t))
                } else {
                    results.iter().map(|r| r.as_f64().ok_or_else(|| "cannot sum non-numbers".to_string())).sum::<Result<f64, _>>().map(Value::Float)
                }
            }
            Reduce::Max | Reduce::Min => {
                let nums: Vec<f64> = results.iter().map(|r| r.as_f64().ok_or_else(|| "non-number result".to_string())).collect::<Result<_, _>>()?;
                let pick = if *self == Reduce::Max { nums.iter().cloned().fold(f64::MIN, f64::max) } else { nums.iter().cloned().fold(f64::MAX, f64::min) };
                let idx = nums.iter().position(|n| *n == pick).unwrap();
                Ok(results[idx].clone())
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Record)]
pub struct Job {
    pub id: Uuid,
    pub batch_id: Option<Uuid>,
    pub kind: String,
    pub payload: Value,
    pub state: JobState,
    pub worker: Option<String>,
    pub attempts: u32,
    pub max_attempts: u32,
    pub created_at: u64,
    #[record(default)]
    pub started_at: u64,
    #[record(default)]
    pub finished_at: u64,
    #[record(default)]
    pub lease_expires_at: u64,
    pub result: Option<Value>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Record)]
pub struct Batch {
    pub id: Uuid,
    pub kind: String,
    pub reduce: Reduce,
    pub job_ids: Vec<Uuid>,
    pub state: JobState,
    pub created_at: u64,
    #[record(default)]
    pub finished_at: u64,
    pub result: Option<Value>,
    pub error: Option<String>,
}

impl Job {
    pub fn to_api(&self) -> Value {
        let mut v = self.to_value();
        if let Some(o) = v.as_object_mut() {
            o.insert("state".into(), self.state.as_str().into());
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reductions() {
        let ints = [Value::Int(1), Value::Int(2), Value::Int(3)];
        assert_eq!(Reduce::Sum.apply(&ints), Ok(Value::Int(6)));
        assert_eq!(Reduce::Max.apply(&ints), Ok(Value::Int(3)));
        assert_eq!(Reduce::Min.apply(&ints), Ok(Value::Int(1)));
        assert_eq!(Reduce::Sum.apply(&[Value::Int(1), Value::Float(0.5)]), Ok(Value::Float(1.5)));
        assert!(Reduce::Sum.apply(&[Value::from("x")]).is_err());
        assert_eq!(Reduce::Concat.apply(&ints).unwrap().as_array().unwrap().len(), 3);
        assert!(Reduce::Sum.apply(&[Value::Int(i64::MAX), Value::Int(1)]).is_err());
    }

    #[test]
    fn job_record_roundtrip() {
        let j = Job {
            id: Uuid::new_v4(), batch_id: None, kind: "echo".into(), payload: Value::Int(1), state: JobState::Queued,
            worker: None, attempts: 0, max_attempts: 3, created_at: 5, started_at: 0, finished_at: 0, lease_expires_at: 0,
            result: None, error: None,
        };
        assert_eq!(Job::from_value(&j.to_value()).unwrap(), j);
    }
}
