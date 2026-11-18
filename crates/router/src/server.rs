use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use utils::Shutdown;

use crate::{Protocol, Router};

/// A connection the codecs can read from and write to (a plain socket, or a TLS session).
pub trait Stream: Read + Write + Send {
    fn set_read_timeout(&self, d: Option<Duration>) -> io::Result<()>;
}

impl Stream for TcpStream {
    fn set_read_timeout(&self, d: Option<Duration>) -> io::Result<()> {
        TcpStream::set_read_timeout(self, d)
    }
}

/// Turns an accepted socket into a [`Stream`]. The default [`PlainAcceptor`] does nothing;
/// implement this to add TLS (required by Gemini) without changing any other code.
pub trait Acceptor: Send + Sync {
    fn accept(&self, stream: TcpStream) -> io::Result<Box<dyn Stream>>;
}

pub struct PlainAcceptor;

impl Acceptor for PlainAcceptor {
    fn accept(&self, stream: TcpStream) -> io::Result<Box<dyn Stream>> {
        Ok(Box::new(stream))
    }
}

#[derive(Clone, Debug)]
pub struct Limits {
    pub max_header_bytes: usize,
    pub max_body_bytes: usize,
    pub max_connections: usize,
    /// Time allowed to receive one complete request once its first byte arrived.
    pub request_timeout: Duration,
    /// How long an idle keep-alive connection is held open.
    pub keepalive_timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_header_bytes: 64 * 1024,
            max_body_bytes: 64 * 1024 * 1024,
            max_connections: 1024,
            request_timeout: Duration::from_secs(30),
            keepalive_timeout: Duration::from_secs(15),
        }
    }
}

pub(crate) struct Conn {
    pub reader: BufReader<Box<dyn Stream>>,
    pub peer: Option<SocketAddr>,
    pub local_port: u16,
    pub router: Arc<Router>,
    pub limits: Limits,
    pub shutdown: Shutdown,
}

/// Read one line (without the line ending). `None` on a clean EOF before any byte.
pub(crate) fn read_line(r: &mut impl BufRead, max: usize, deadline: Instant) -> io::Result<Option<Vec<u8>>> {
    let mut line = Vec::new();
    loop {
        let buf = match r.fill_buf() {
            Ok(b) => b,
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted) => {
                if Instant::now() > deadline {
                    return Err(io::Error::new(io::ErrorKind::TimedOut, "request timed out"));
                }
                continue;
            }
            Err(e) => return Err(e),
        };
        if buf.is_empty() {
            return if line.is_empty() { Ok(None) } else { Err(io::ErrorKind::UnexpectedEof.into()) };
        }
        match buf.iter().position(|b| *b == b'\n') {
            Some(i) => {
                line.extend_from_slice(&buf[..i]);
                r.consume(i + 1);
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                if line.len() > max {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "line too long"));
                }
                return Ok(Some(line));
            }
            None => {
                let n = buf.len();
                line.extend_from_slice(buf);
                r.consume(n);
                if line.len() > max {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "line too long"));
                }
            }
        }
    }
}

/// Read exactly `n` bytes, tolerating read timeouts until `deadline`.
pub(crate) fn read_exact_deadline(r: &mut impl Read, n: usize, deadline: Instant) -> io::Result<Vec<u8>> {
    let mut out = vec![0u8; n];
    let mut got = 0;
    while got < n {
        match r.read(&mut out[got..]) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(k) => got += k,
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted) => {
                if Instant::now() > deadline {
                    return Err(io::Error::new(io::ErrorKind::TimedOut, "request timed out"));
                }
            }
            Err(e) => return Err(e),
        }
    }
    Ok(out)
}

/// Wait for the first byte of the next request, honouring shutdown and the keep-alive timeout.
/// Returns `false` when the connection should be closed.
pub(crate) fn wait_for_data(conn: &mut Conn, idle: Duration) -> bool {
    let until = Instant::now() + idle;
    loop {
        match conn.reader.fill_buf() {
            Ok(b) => return !b.is_empty(),
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted) => {
                if conn.shutdown.is_triggered() || Instant::now() > until {
                    return false;
                }
            }
            Err(_) => return false,
        }
    }
}

struct ActiveGuard(Arc<AtomicUsize>);

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

pub struct Listener {
    pub protocol: Protocol,
    pub addr: SocketAddr,
    handle: JoinHandle<()>,
}

impl Listener {
    /// Wait for the accept loop (and in-flight connections) to finish after shutdown.
    pub fn join(self) {
        let _ = self.handle.join();
    }
}

/// Serves a [`Router`] over one or more protocols and ports.
pub struct Server {
    router: Arc<Router>,
    limits: Limits,
    acceptors: HashMap<Protocol, Arc<dyn Acceptor>>,
}

impl Server {
    pub fn new(router: Router) -> Self {
        Server { router: Arc::new(router), limits: Limits::default(), acceptors: HashMap::new() }
    }

    pub fn limits(mut self, l: Limits) -> Self {
        self.limits = l;
        self
    }

    /// Use a custom [`Acceptor`] (e.g. TLS) for one protocol.
    pub fn acceptor(mut self, protocol: Protocol, a: Arc<dyn Acceptor>) -> Self {
        self.acceptors.insert(protocol, a);
        self
    }

    /// Bind `addr` (e.g. `0.0.0.0:80`, or port `0` for an ephemeral one) and serve `protocol`.
    pub fn listen(&self, protocol: Protocol, addr: &str, shutdown: &Shutdown) -> io::Result<Listener> {
        let listener = TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        let local = listener.local_addr()?;
        let router = self.router.clone();
        let limits = self.limits.clone();
        let acceptor = self.acceptors.get(&protocol).cloned().unwrap_or_else(|| Arc::new(PlainAcceptor));
        let shutdown = shutdown.clone();
        let active = Arc::new(AtomicUsize::new(0));
        let handle = std::thread::Builder::new().name(format!("{}-accept-{}", protocol.name(), local.port())).spawn(move || {
            while !shutdown.is_triggered() {
                match listener.accept() {
                    Ok((stream, peer)) => {
                        if active.load(Ordering::SeqCst) >= limits.max_connections {
                            drop(stream); // shed load
                            continue;
                        }
                        let _ = stream.set_nonblocking(false);
                        let _ = stream.set_nodelay(true);
                        let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                        let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
                        let (router, limits, shutdown, acceptor) =
                            (router.clone(), limits.clone(), shutdown.clone(), acceptor.clone());
                        active.fetch_add(1, Ordering::SeqCst);
                        let guard = ActiveGuard(active.clone());
                        let _ = std::thread::Builder::new().name(format!("{}-conn", protocol.name())).spawn(move || {
                            let _guard = guard; // decrements on exit, or if the thread never started
                            if let Ok(s) = acceptor.accept(stream) {
                                let mut conn = Conn {
                                    reader: BufReader::new(s),
                                    peer: Some(peer),
                                    local_port: local.port(),
                                    router,
                                    limits,
                                    shutdown,
                                };
                                match protocol {
                                    Protocol::Http => crate::http::serve(&mut conn),
                                    Protocol::Gopher => crate::gopher::serve(&mut conn),
                                    Protocol::Gemini => crate::gemini::serve(&mut conn),
                                }
                            }
                        });
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        shutdown.sleep(Duration::from_millis(10));
                    }
                    Err(_) => {
                        shutdown.sleep(Duration::from_millis(50));
                    }
                }
            }
            // drain in-flight connections briefly
            let until = Instant::now() + Duration::from_secs(3);
            while active.load(Ordering::SeqCst) > 0 && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(20));
            }
        })?;
        Ok(Listener { protocol, addr: local, handle })
    }
}
