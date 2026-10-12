//! The `app` role. The window is the app's own; this process is what Clatch launched. It holds
//! the control pipe, starts the window as its child, and carries the agent's requests to the
//! window's control port and the answers back:
//!
//! ```text
//!   agent ─ <cli> <group> … ─▶ ~/.<cli>/<cli>.sock ─▶ here ─ JSON lines, keyed ─▶ the window
//! ```
//!
//! Two rules live here rather than in the CLI, so that something talking to the socket
//! directly is held to them as well: nothing the person has not saved is ever thrown away
//! (files are only touched through the `file` operations below, which refuse to close or
//! replace a document with unsaved changes), and the methods that would quit the app, run a
//! plug-in or change preferences are never forwarded (`policy::forward`).

use crate::{craft, policy, CLI};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio::process::Command;
use tokio::sync::{watch, Mutex};

/// A cold start of the window measured under 3 s; this leaves room for a slow disk.
const START_TIMEOUT: Duration = Duration::from_secs(60);
/// The window answers within 60 s or says it timed out; past this it is not answering at all.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(75);
/// The windows close a control connection idle for 30 s. One idle this long is replaced
/// before a request is sent on it, so no request is lost into a connection being closed.
const IDLE: Duration = Duration::from_secs(20);
/// An answer bigger than this travels as a file: the CLI's socket takes 1 MiB at most.
const SPILL: usize = 512 << 10;

pub fn run() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap_or_else(|e| fail(&format!("cannot start a runtime: {e}")));
    let code = runtime.block_on(async {
        match serve().await {
            Ok(code) => code,
            Err(e) => {
                eprintln!("{CLI}: {e:#}");
                1
            }
        }
    });
    std::process::exit(code);
}

async fn serve() -> Result<i32> {
    // One window per user: a second launch raises the first instead of opening another.
    if clappkit::ipc::request(CLI, &json!({"op": "focus"})).await.is_ok() {
        eprintln!("{CLI}: already running - raised the window");
        return Ok(0);
    }

    let exe = window_exe()?;
    let data = clappkit::paths::try_data_dir(CLI)?;
    // Files handed between the window and the CLI live here for the length of one request.
    // Whatever an earlier run left behind is swept away first.
    let exchange = data.join("exchange");
    let _ = std::fs::remove_dir_all(&exchange);
    std::fs::create_dir_all(&exchange).with_context(|| format!("cannot create {}", exchange.display()))?;
    clappkit::ensure_private_dir(&exchange)?;

    let port = free_port()?;
    let (args, secret) = craft::launch(port, &data)?;
    let link = Arc::new(Link::new(port, secret));
    let (ready_tx, ready) = watch::channel(false);

    // The CLI's door opens before the window starts, so an agent that calls the moment Clatch
    // has launched us waits for the window instead of being told nothing is running.
    let door = Door { link: link.clone(), ready: ready.clone(), exchange: Arc::new(exchange.clone()), next: Arc::new(AtomicU64::new(1)) };
    let addr = clappkit::ipc::address(CLI);
    tokio::spawn(async move {
        let served = clappkit::ipc::serve(&addr, move |request: Value| {
            let door = door.clone();
            async move { door.answer(request).await }
        })
        .await;
        if let Err(e) = served {
            eprintln!("{CLI}: the agent's CLI cannot reach this window: {e:#}");
        }
    });

    let mut child = Command::new(&exe)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("cannot start {}", exe.display()))?;

    // Clatch says stop, or its pipe ends. The window does not wait for this hook: clappkit answers
    // Clatch first and runs the hook after, and Clatch kills this process the moment the answer
    // arrives, so a hook here rarely runs at all. The lifeline below is what closes the window.
    let on_shutdown: clappkit::OnShutdown = Arc::new(move |cause| {
        eprintln!("{CLI}: {cause}; the window closes with this launcher, asking about unsaved work first");
    });
    let _control = clappkit::Control::connect_with(clappkit::declared_signals(), on_shutdown).await?;

    // Ready once the control port takes our key. Until then every request waits (`Door::wait`).
    // The lifeline goes up first: from then on, however this process ends, the window closes the
    // way a person would (`app.closeOnDisconnect`, the clapp's patch to the window's transport).
    let starter = link.clone();
    tokio::spawn(async move {
        let deadline = Instant::now() + START_TIMEOUT;
        while Instant::now() < deadline {
            if starter.ensure().await.is_ok() {
                match starter.lifeline().await {
                    Ok((mut read, write, _)) => {
                        tokio::spawn(async move {
                            let _held = write;
                            let mut nothing = String::new();
                            let _ = read.read_line(&mut nothing).await; // the window's end, if first
                        });
                    }
                    Err(e) => eprintln!("{CLI}: no lifeline to the window ({e}); stopping this launcher will leave it open"),
                }
                let _ = ready_tx.send(true);
                return;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        eprintln!("{CLI}: the window's control port did not answer within {}s", START_TIMEOUT.as_secs());
    });

    let status = child.wait().await?;
    craft::cleanup(port, &data);
    let _ = std::fs::remove_dir_all(&exchange);
    Ok(status.code().unwrap_or(0))
}

/// The secret a window's control port wants: one this process made and handed over, or one
/// the window writes to a file of its own once its port is bound.
// Each Craft app uses one of the two; this file is the same in every Craft clapp.
#[allow(dead_code)]
pub enum Secret {
    Given(String),
    File(PathBuf),
}

impl Secret {
    fn value(&self) -> Result<String, String> {
        match self {
            Secret::Given(s) => Ok(s.clone()),
            Secret::File(p) => std::fs::read_to_string(p)
                .map(|k| k.trim().to_string())
                .ok()
                .filter(|k| !k.is_empty())
                .ok_or_else(|| format!("the window has not written its key yet ({})", p.display())),
        }
    }
}

type Conn = (BufReader<OwnedReadHalf>, OwnedWriteHalf, Instant);

/// One authenticated connection to the window's control port, used one request at a time.
pub struct Link {
    port: u16,
    secret: Secret,
    conn: Mutex<Option<Conn>>,
    next: AtomicU64,
}

impl Link {
    fn new(port: u16, secret: Secret) -> Self {
        Link { port, secret, conn: Mutex::new(None), next: AtomicU64::new(1) }
    }

    async fn open(&self) -> Result<Conn, String> {
        let stream = TcpStream::connect(("127.0.0.1", self.port)).await.map_err(|e| format!("the window's control port: {e}"))?;
        let _ = stream.set_nodelay(true);
        let (read, mut write) = stream.into_split();
        let mut read = BufReader::new(read);
        let mut auth = craft::auth_frame(&self.secret.value()?);
        auth["id"] = json!("auth");
        let reply = exchange_line(&mut read, &mut write, &auth).await?;
        if reply.get("ok").and_then(Value::as_bool) != Some(true) {
            return Err(format!("the window refused this launcher's key: {}", error_of(&reply)));
        }
        Ok((read, write, Instant::now()))
    }

    /// Connect and authenticate if there is no live connection. Nothing is sent but the key.
    async fn ensure(&self) -> Result<(), String> {
        let mut conn = self.conn.lock().await;
        if conn.as_ref().is_some_and(|c| c.2.elapsed() < IDLE) {
            return Ok(());
        }
        *conn = Some(self.open().await?);
        Ok(())
    }

    /// Call one method; `Ok` is its `result`, `Err` the window's refusal or why it was not
    /// reached. After a failure past the send nothing is retried: the window may have done it.
    pub async fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let mut conn = self.conn.lock().await;
        if !conn.as_ref().is_some_and(|c| c.2.elapsed() < IDLE) {
            *conn = Some(self.open().await?);
        }
        let Some((read, write, used)) = conn.as_mut() else { return Err("no connection to the window".into()) };
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let request = json!({"id": id, "method": method, "params": params});
        match exchange_line(read, write, &request).await {
            Ok(reply) => {
                *used = Instant::now();
                if reply.get("ok").and_then(Value::as_bool) == Some(true) {
                    Ok(reply.get("result").cloned().unwrap_or(Value::Null))
                } else {
                    Err(error_of(&reply))
                }
            }
            Err(e) => {
                *conn = None;
                Err(format!("{e}; the window may still have carried it out, so look before repeating it"))
            }
        }
    }

    /// A second connection, held for this process's life and never written to again: when it ends,
    /// the window closes the way a person would.
    async fn lifeline(&self) -> Result<Conn, String> {
        let (mut read, mut write, at) = self.open().await?;
        let reply = exchange_line(&mut read, &mut write, &json!({"id": "life", "method": "app.closeOnDisconnect"})).await?;
        if reply.get("ok").and_then(Value::as_bool) != Some(true) {
            return Err(error_of(&reply));
        }
        Ok((read, write, at))
    }

    /// [`Self::call`] for `engine.execute`.
    pub async fn exec(&self, command: &str, params: Value) -> Result<Value, String> {
        self.call("engine.execute", json!({"command": command, "params": params})).await
    }
}

async fn exchange_line(read: &mut BufReader<OwnedReadHalf>, write: &mut OwnedWriteHalf, msg: &Value) -> Result<Value, String> {
    let line = format!("{msg}\n");
    write.write_all(line.as_bytes()).await.map_err(|e| format!("cannot send to the window: {e}"))?;
    let mut reply = String::new();
    match tokio::time::timeout(ANSWER_TIMEOUT, read.read_line(&mut reply)).await {
        Ok(Ok(0)) => Err("the window closed the connection".into()),
        Ok(Ok(_)) => serde_json::from_str(reply.trim()).map_err(|e| format!("an unreadable answer from the window: {e}")),
        Ok(Err(e)) => Err(format!("cannot read the window's answer: {e}")),
        Err(_) => Err(format!("the window did not answer within {}s", ANSWER_TIMEOUT.as_secs())),
    }
}

fn error_of(reply: &Value) -> String {
    reply.get("error").and_then(Value::as_str).unwrap_or("the window refused without saying why").to_string()
}

/// Everything a CLI request needs: the window, whether it is up, and where files are handed over.
#[derive(Clone)]
struct Door {
    link: Arc<Link>,
    ready: watch::Receiver<bool>,
    exchange: Arc<PathBuf>,
    next: Arc<AtomicU64>,
}

impl Door {
    async fn answer(self, request: Value) -> Value {
        let op = request.get("op").and_then(Value::as_str).unwrap_or("").to_string();
        if op == "status" {
            return json!({"ok": true, "name": craft::NAME, "ready": *self.ready.borrow()});
        }
        if let Err(e) = self.wait().await {
            return refusal(&e);
        }
        let answer = match op.as_str() {
            "focus" => self.link.call("ui.focus", json!({})).await.map(|_| json!({})),
            "call" => self.forward(&request).await,
            "stage" => Ok(json!({"file": self.fresh(request.get("ext").and_then(Value::as_str).unwrap_or("bin"))})),
            "render" => {
                let file = self.fresh("png");
                let page = request.get("page").and_then(Value::as_u64);
                let scale = request.get("scale").and_then(Value::as_f64);
                craft::render(&self.link, &file, page, scale).await.map(|r| with_file(r, &file))
            }
            "shot" => {
                let file = self.fresh("png");
                self.link.call("ui.screenshot", json!({"path": file})).await.map(|r| with_file(r, &file))
            }
            "file" => craft::file(&self.link, &request).await,
            "" => Err("a request names its `op`".into()),
            other => Err(format!("unknown op `{other}`")),
        };
        match answer {
            Ok(result) => self.deliver(result),
            Err(e) => refusal(&e),
        }
    }

    /// Wait for the window's port, the first time only.
    async fn wait(&self) -> Result<(), String> {
        if *self.ready.borrow() {
            return Ok(());
        }
        let mut ready = self.ready.clone();
        let up = matches!(tokio::time::timeout(START_TIMEOUT, ready.wait_for(|up| *up)).await, Ok(Ok(_)));
        if up {
            Ok(())
        } else {
            Err(format!("the {} window has not finished starting", craft::NAME))
        }
    }

    async fn forward(&self, request: &Value) -> Result<Value, String> {
        let method = request.get("method").and_then(Value::as_str).ok_or("a call names its `method`")?;
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
        policy::forward(method, &params)?;
        self.link.call(method, params).await
    }

    /// A path for one file handed over in this request. The name is new every time.
    fn fresh(&self, ext: &str) -> String {
        let ext: String = ext.chars().filter(char::is_ascii_alphanumeric).take(8).collect();
        let n = self.next.fetch_add(1, Ordering::Relaxed);
        self.exchange.join(format!("{n}.{}", if ext.is_empty() { "bin" } else { &ext })).to_string_lossy().into_owned()
    }

    /// The answer, or a file holding it when it is too big for the socket.
    fn deliver(&self, result: Value) -> Value {
        let body = json!({"ok": true, "result": result});
        let text = body.to_string();
        if text.len() <= SPILL {
            return body;
        }
        let file = self.fresh("json");
        match std::fs::write(&file, text) {
            Ok(()) => json!({"ok": true, "spill": file}),
            Err(e) => refusal(&format!("an answer too big to send, and cannot write {file}: {e}")),
        }
    }
}

fn with_file(mut result: Value, file: &str) -> Value {
    if !result.is_object() {
        result = json!({});
    }
    result["file"] = json!(file);
    result
}

fn refusal(why: &str) -> Value {
    json!({"ok": false, "error": why})
}

/// A loopback port nobody holds right now, for the window to bind.
fn free_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).context("cannot find a free loopback port")?;
    Ok(listener.local_addr()?.port())
}

/// The window this launcher starts: the environment's development build, else the one
/// shipped in the depot beside `bin/`.
fn window_exe() -> Result<PathBuf> {
    if let Some(dev) = std::env::var_os(craft::GUI_ENV).filter(|p| !p.is_empty()) {
        return Ok(PathBuf::from(dev));
    }
    let exe = clappkit::paths::install_root().join(craft::BUNDLE_EXE);
    if !exe.is_file() {
        bail!("the {} window is missing from this install: {}", craft::NAME, exe.display());
    }
    Ok(exe)
}

/// 32 random bytes from the operating system, as hex: a control token, for the apps whose
/// launcher makes it.
#[allow(dead_code)]
pub fn random_hex() -> Result<String> {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).map_err(|e| anyhow::anyhow!("no randomness for a control token: {e}"))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

/// Write `text` to a new file only this user can read.
#[allow(dead_code)]
pub fn write_private(path: &Path, text: &str) -> Result<()> {
    use std::io::Write;
    let _ = std::fs::remove_file(path);
    let mut open = std::fs::OpenOptions::new();
    open.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut open, 0o600);
    let mut f = open.open(path).with_context(|| format!("cannot create {}", path.display()))?;
    f.write_all(text.as_bytes()).with_context(|| format!("cannot write {}", path.display()))
}

fn fail(msg: &str) -> ! {
    eprintln!("{CLI}: {msg}");
    std::process::exit(1)
}
