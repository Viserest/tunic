//! Line-based TCP relay for simple bots.
//!
//! ```text
//! AUTH <token>
//! SEND <group-uuid> <name> <text...>
//! QUIT
//! ```
//! Replies are `OK` or `ERR <reason>`. There is no encryption; bind to localhost or tunnel it.

use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread::JoinHandle;
use std::time::Duration;

use database::Uuid;
use utils::Shutdown;

use crate::Messages;

pub struct RelayServer {
    pub addr: SocketAddr,
    handle: JoinHandle<()>,
}

impl RelayServer {
    pub fn start(msgs: Messages, addr: &str, token: String, shutdown: &Shutdown) -> std::io::Result<RelayServer> {
        let l = TcpListener::bind(addr)?;
        l.set_nonblocking(true)?;
        let addr = l.local_addr()?;
        let sd = shutdown.clone();
        let handle = std::thread::spawn(move || {
            while !sd.is_triggered() {
                match l.accept() {
                    Ok((s, _)) => {
                        let (m, t) = (msgs.clone(), token.clone());
                        let _ = std::thread::Builder::new().spawn(move || serve(s, m, &t));
                    }
                    Err(_) => {
                        sd.sleep(Duration::from_millis(25));
                    }
                }
            }
        });
        Ok(RelayServer { addr, handle })
    }

    pub fn join(self) {
        let _ = self.handle.join();
    }
}

fn serve(stream: TcpStream, msgs: Messages, token: &str) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(60)));
    let mut out = match stream.try_clone() {
        Ok(o) => o,
        Err(_) => return,
    };
    let mut authed = false;
    // cap line length so a client cannot make us buffer without bound
    let mut reader = BufReader::new(std::io::Read::take(stream, 64 * 1024));
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        reader.get_mut().set_limit(64 * 1024);
        let line = line.trim_end();
        let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
        let reply = match cmd.to_ascii_uppercase().as_str() {
            "AUTH" => {
                authed = utils::password::ct_eq(rest.as_bytes(), token.as_bytes());
                if authed { "OK".to_string() } else { "ERR bad token".to_string() }
            }
            "SEND" if authed => {
                let mut p = rest.splitn(3, ' ');
                match (p.next().and_then(Uuid::parse), p.next(), p.next()) {
                    (Some(g), Some(name), Some(text)) => match msgs.ingest(&g, "relay", name, text) {
                        Ok(_) => "OK".into(),
                        Err(e) => format!("ERR {e}"),
                    },
                    _ => "ERR usage: SEND <group> <name> <text>".into(),
                }
            }
            "SEND" => "ERR authenticate first".into(),
            "QUIT" => return,
            _ => "ERR unknown command".into(),
        };
        if writeln!(out, "{reply}").is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::Database;

    #[test]
    fn relay_roundtrip() {
        let m = Messages::new(Database::memory().scope("m").unwrap()).unwrap();
        let g = m.create_group(Uuid::new_v4(), "g").unwrap();
        let sd = Shutdown::new();
        let srv = RelayServer::start(m.clone(), "127.0.0.1:0", "tok".into(), &sd).unwrap();
        let mut c = TcpStream::connect(srv.addr).unwrap();
        let mut r = BufReader::new(c.try_clone().unwrap());
        let mut ask = |q: String| {
            writeln!(c, "{q}").unwrap();
            let mut s = String::new();
            r.read_line(&mut s).unwrap();
            s.trim().to_string()
        };
        assert!(ask(format!("SEND {} bot hi", g.id)).starts_with("ERR"));
        assert!(ask("AUTH wrong".into()).starts_with("ERR"));
        assert_eq!(ask("AUTH tok".into()), "OK");
        assert_eq!(ask(format!("SEND {} bot hello there", g.id)), "OK");
        assert!(ask("SEND nope".into()).starts_with("ERR"));
        assert_eq!(m.messages.all().unwrap()[0].body, "hello there");
        sd.trigger();
        srv.join();
    }
}
