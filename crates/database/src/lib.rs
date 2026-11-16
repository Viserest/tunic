mod list;
pub use list::List;

mod table;
pub use table::Table;

#[derive(Deserialize)]
pub struct ListPayload {
    limit: Option<usize>,
    offset: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
