use serde::{Deserialize, Serialize};
use tunic::{StateTrait, Table};
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
impl StateTrait for State {}

fn main() {
    let state = State::default();

    tunic::Serve::new().run(state);
}
