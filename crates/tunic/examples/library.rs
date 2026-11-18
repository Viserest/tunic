//! `cargo run -p tunic --example library` - a custom app built on tunic.
use tunic::database::{Record, Uuid};
use tunic::router::{Request, Response, Router};
use tunic::{App, Value};

#[derive(Clone, Debug, PartialEq, Record)]
struct Note {
    id: Uuid,
    owner: Uuid,
    text: String,
}
impl tunic::user::Owned for Note {
    fn owner(&self) -> Uuid {
        self.owner
    }
}

fn main() {
    let running = App::new()
        .disable("food")
        .users("my-app/accounts")
        .files("my-app/files")
        .table::<Note>("my-app/notes", "/notes")
        .router(Router::new().get("/ping", |_: &Request| Response::text("pong")).tagged("public"))
        .on_start("hello", |ctx| {
            let db = ctx.scope("my-app/hello")?;
            ctx.link("Hello", "/hello");
            let _ = db;
            Ok(())
        })
        .default_value("router.protocols.http", Value::Int(9000))
        .start()
        .unwrap_or_else(|e| {
            eprintln!("cannot start: {e}");
            std::process::exit(2);
        });
    running.shutdown_handle().trigger_on_ctrl_c();
    running.wait();
}
