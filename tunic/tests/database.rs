use serde::{Deserialize, Serialize};
use tunic::Table;
use uuid::Uuid;

#[derive(Deserialize, Serialize)]
struct Circle {
    diameter: u32,
}

#[derive(Deserialize, Serialize)]
struct Square {
    width: u32,
    height: u32,
}

#[derive(Clone, Default)]
struct State {
    circles: Table<Uuid, Circle>,
    squares: Table<Uuid, Square>,
}

fn main() {
    let state = tunic::State::default();
    let config = tunic::Config::default();

    tunic::Serve::new(state, config).run();
}
