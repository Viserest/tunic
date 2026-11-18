use utils::{Uuid, Value};

/// A struct stored in a [`crate::Table`]. Normally implemented with `#[derive(Record)]`.
pub trait Record: Sized + Clone + Send + Sync + 'static {
    /// Collection (table) name inside the scope. Letters, digits, `_` and `-` only.
    const COLLECTION: &'static str;
    fn id(&self) -> Uuid;
    fn to_value(&self) -> Value;
    fn from_value(v: &Value) -> Result<Self, String>;
    /// Fields which generic http crud routes never reveal or accept (`#[record(hidden)]`).
    fn hidden_fields() -> &'static [&'static str] {
        &[]
    }
    /// [`Record::to_value`] without the hidden fields.
    fn public_value(&self) -> Value {
        let mut v = self.to_value();
        if let Some(o) = v.as_object_mut() {
            for h in Self::hidden_fields() {
                o.remove(*h);
            }
        }
        v
    }
}
