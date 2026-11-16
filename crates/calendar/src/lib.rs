use tunic_router::Router;

/// Global email addressing
struct Email {
    username: String,
    domain: String,
}

/// US-Based phone number addressing
///
/// TODO: This should cover all countries but idk
struct PhoneNumber {
    // Country code
    country_code: u8,
    // First block: Numbering plan area code (NXX)
    area_code: u16,
    // Second block: Central office code/Exchange code (NXX)
    exchange_code: u16,
    // Third block: Line number (XXXX)
    line_number: u16,
}

/// US-Based physical addressing
///
/// TODO: Update for global-addressing capablities
struct Address {
    street_number: String,
    street: String,
    apt_code: String,
    city: String,
    state: String,
    country: String,
    zip_code: String,
}

/// Personal contact information
struct Contact {
    id: Uuid,
    name: String,
    email: Email,
    phone: PhoneNumber,
    address: Address,
    notes: Vec<String>,
    // TIMESTAMPS
    created_at: u64,
    accessed_at: u64,
    updated_at: u64,
    deleted_at: u64,
}

/// A Person as a [`Contact`] or [`String`]
///
/// A Person isn't required to be [`Contact`] but can also be a [`String`]
enum Person {
    String(String),
    Contact(Contact),
}

/// Calendar event
struct Event {
    // Start and end time
    start_at: u64,
    end_at: u64,
    // Event details
    title: String,
    description: String,
    people: Vec<Person>,
    location: Address,
    // TIMESTAMPS
    created_at: u64,
    accessed_at: u64,
    updated_at: u64,
    deleted_at: u64,
}

struct Calendar {
    events: Vec<Event>,
}

pub fn router() {
    return;
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
