use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::Hash;

use utils::{Uuid, Value, from_hex, to_hex};

/// A type that can live inside a [`crate::Record`].
pub trait FieldValue: Sized {
    fn to_value(&self) -> Value;
    fn from_value(v: &Value) -> Result<Self, String>;
    /// Value used when the field is absent. Only `Option` has one by default.
    fn from_missing() -> Result<Self, String> {
        Err("missing".into())
    }
}

macro_rules! int_field {
    ($($t:ty),*) => {$(
        impl FieldValue for $t {
            fn to_value(&self) -> Value { Value::from(*self) }
            fn from_value(v: &Value) -> Result<Self, String> {
                match v {
                    Value::Int(i) => <$t>::try_from(*i).map_err(|_| format!("{} out of range", i)),
                    Value::Float(f) if f.fract() == 0.0 && *f >= 0.0 && *f < 1.9e19 => Ok(*f as $t),
                    _ => Err("expected an integer".into()),
                }
            }
        }
    )*};
}
int_field!(u8, u16, u32, u64, usize, i8, i16, i32, i64, isize);

impl FieldValue for f64 {
    fn to_value(&self) -> Value {
        Value::Float(*self)
    }
    fn from_value(v: &Value) -> Result<Self, String> {
        v.as_f64().ok_or_else(|| "expected a number".into())
    }
}
impl FieldValue for f32 {
    fn to_value(&self) -> Value {
        Value::Float(*self as f64)
    }
    fn from_value(v: &Value) -> Result<Self, String> {
        v.as_f64().map(|f| f as f32).ok_or_else(|| "expected a number".into())
    }
}
impl FieldValue for bool {
    fn to_value(&self) -> Value {
        Value::Bool(*self)
    }
    fn from_value(v: &Value) -> Result<Self, String> {
        v.as_bool().ok_or_else(|| "expected a boolean".into())
    }
}
impl FieldValue for String {
    fn to_value(&self) -> Value {
        Value::Str(self.clone())
    }
    fn from_value(v: &Value) -> Result<Self, String> {
        v.as_str().map(String::from).ok_or_else(|| "expected a string".into())
    }
}
impl FieldValue for Uuid {
    fn to_value(&self) -> Value {
        Value::Str(self.to_string())
    }
    fn from_value(v: &Value) -> Result<Self, String> {
        Uuid::parse(v.as_str().ok_or("expected a uuid string")?).ok_or_else(|| "invalid uuid".into())
    }
}
impl FieldValue for Value {
    fn to_value(&self) -> Value {
        self.clone()
    }
    fn from_value(v: &Value) -> Result<Self, String> {
        Ok(v.clone())
    }
    fn from_missing() -> Result<Self, String> {
        Ok(Value::Null)
    }
}
impl FieldValue for [u8; 32] {
    fn to_value(&self) -> Value {
        Value::Str(to_hex(self))
    }
    fn from_value(v: &Value) -> Result<Self, String> {
        let b = from_hex(v.as_str().ok_or("expected a hex string")?).ok_or("invalid hex")?;
        b.try_into().map_err(|_| "expected 32 bytes".to_string())
    }
}
impl<T: FieldValue> FieldValue for Option<T> {
    fn to_value(&self) -> Value {
        match self {
            Some(v) => v.to_value(),
            None => Value::Null,
        }
    }
    fn from_value(v: &Value) -> Result<Self, String> {
        if v.is_null() { Ok(None) } else { T::from_value(v).map(Some) }
    }
    fn from_missing() -> Result<Self, String> {
        Ok(None)
    }
}
impl<T: FieldValue> FieldValue for Vec<T> {
    fn to_value(&self) -> Value {
        Value::Array(self.iter().map(FieldValue::to_value).collect())
    }
    fn from_value(v: &Value) -> Result<Self, String> {
        v.as_array().ok_or("expected an array")?.iter().map(T::from_value).collect()
    }
}
impl<T: FieldValue + Hash + Eq> FieldValue for HashSet<T> {
    fn to_value(&self) -> Value {
        Value::Array(self.iter().map(FieldValue::to_value).collect())
    }
    fn from_value(v: &Value) -> Result<Self, String> {
        v.as_array().ok_or("expected an array")?.iter().map(T::from_value).collect()
    }
}
impl<T: FieldValue> FieldValue for BTreeMap<String, T> {
    fn to_value(&self) -> Value {
        Value::Object(self.iter().map(|(k, v)| (k.clone(), v.to_value())).collect())
    }
    fn from_value(v: &Value) -> Result<Self, String> {
        v.as_object().ok_or("expected an object")?.iter().map(|(k, v)| Ok((k.clone(), T::from_value(v)?))).collect()
    }
}
impl<T: FieldValue> FieldValue for HashMap<String, T> {
    fn to_value(&self) -> Value {
        Value::Object(self.iter().map(|(k, v)| (k.clone(), v.to_value())).collect())
    }
    fn from_value(v: &Value) -> Result<Self, String> {
        v.as_object().ok_or("expected an object")?.iter().map(|(k, v)| Ok((k.clone(), T::from_value(v)?))).collect()
    }
}

/// Implement [`FieldValue`] for a fieldless enum, stored as its snake_case variant name.
///
/// ```
/// enum Level { Viewer, Editor }
/// database::field_enum!(Level { Viewer => "viewer", Editor => "editor" });
/// ```
#[macro_export]
macro_rules! field_enum {
    ($name:ident { $($variant:ident => $text:expr),+ $(,)? }) => {
        impl $crate::FieldValue for $name {
            fn to_value(&self) -> $crate::Value {
                match self { $( $name::$variant => $crate::Value::Str($text.to_string()), )+ }
            }
            fn from_value(v: &$crate::Value) -> ::core::result::Result<Self, ::std::string::String> {
                match v.as_str() {
                    $( Some($text) => Ok($name::$variant), )+
                    _ => Err(format!("invalid {} value", stringify!($name))),
                }
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitives_roundtrip() {
        assert_eq!(u64::from_value(&1234567890123u64.to_value()), Ok(1234567890123));
        assert!(u8::from_value(&Value::Int(300)).is_err());
        assert!(u64::from_value(&Value::Int(-1)).is_err());
        assert_eq!(Option::<String>::from_value(&Value::Null), Ok(None));
        assert_eq!(Option::<String>::from_missing(), Ok(None));
        assert!(String::from_missing().is_err());
        let h = [7u8; 32];
        assert_eq!(<[u8; 32]>::from_value(&h.to_value()), Ok(h));
        let v = vec![Uuid::new_v4(), Uuid::new_v4()];
        assert_eq!(Vec::<Uuid>::from_value(&v.to_value()), Ok(v));
        assert!(String::from_value(&Value::Int(1)).is_err());
    }
}
