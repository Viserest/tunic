#[macro_export]
macro_rules! event {
    ($level:expr, $fmt:expr) => {
        crate::log::event($level, $fmt)
    };
    ($level:expr, $fmt:expr, $($args:tt)*) => {
        crate::log::event($level, format!($fmt, $($args)*))
    };
}

#[macro_export]
macro_rules! span {
    ($level:expr, $fmt:expr) => {
        crate::log::span($level, $fmt)
    };
    ($level:expr, $fmt:expr, $($args:tt)*) => {
        crate::log::span($level, format!($fmt, $($args)*))
    };
}
