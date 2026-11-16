#[macro_export]
// Automatically handles unknown status code as StatusCode::Unknown(u8)
macro_rules! status_code {
    ($vis:vis $enum_name:ident<$enum_type:ident>, $(
        $is_something:ident {$(
            $name:ident, $number:expr, $text:expr;
        )*},
    )*) => {
        // Enum generated
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        $vis enum $enum_name {
            $($($name,)*)*
            Unknown((&'static str, $enum_type)),
        }

        impl ::std::fmt::Display for $enum_name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                match self {
                    $($(Self::$name => write!(f, "{}", $text),)*)*
                    _ => write!(f, "Unknown"),
                }
            }
        }

        // From and Into generated for value
        impl From<$enum_type> for $enum_name {
            fn from(value: $enum_type) -> Self {
                match value {
                    $($($number => Self::$name,)*)*
                    _ => Self::Unknown(("Unknown", value)),
                }
            }
        }
        impl Into<$enum_type> for $enum_name {
            fn into(self) -> $enum_type {
                match self {
                    $($(Self::$name => $number,)*)*
                    Self::Unknown(a) => a.1,
                }
            }
        }

        // From and Into generated for text
        impl From<&'static str> for $enum_name {
            fn from(value: &'static str) -> Self {
                match value {
                    $($($text => Self::$name,)*)*
                    _ => Self::Unknown((value, 0)),
                }
            }
        }
        impl Into<&'static str> for $enum_name {
            fn into(self) -> &'static str {
                match &self {
                    $($(Self::$name => $text,)*)*
                    Self::Unknown(a) => a.0,
                }
            }
        }

        // Status code separation
        impl $enum_name {
            $($vis fn $is_something(&self) -> bool {
                match self {
                    $(Self::$name)|* => true,
                    _ => false,
                }
            })*
        }
    };
}
