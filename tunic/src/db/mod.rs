mod feature;
pub mod list;
pub mod table;

pub use feature::Feature;
pub use list::List;
use serde::Deserialize;
pub use table::Table;

#[derive(Deserialize)]
pub struct ListPayload {
    limit: Option<usize>,
    offset: Option<usize>,
}
