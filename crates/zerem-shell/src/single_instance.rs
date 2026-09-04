//! One process, however many times it is launched.
//!
//! Two things make this necessary rather than tidy. A second copy of a torrent
//! client opens the *same* session folder and the *same* download folder as the
//! first — two processes writing the same state and the same files, which is the
//! worst corruption risk the app has. And a `magnet:` handler is only useful if
//! clicking a link reaches the window that is already open.
//!
//! The mechanism is a named pipe. The first launch owns it; a later launch
//! connects, hands over its argument, waits for the owner to acknowledge having
//! taken it, and exits. **No acknowledgement means the owner is wedged**: it is
//! terminated and the pipe taken. Without that probe a hung process makes the
//! app unlaunchable — every later start forwards into the void and exits
//! silently, which is the failure Clipo shipped once and had to fix.

use std::time::Duration;

/// How long each side waits for the owner to take a forwarded argument.
///
/// Deliberately generous: a healthy owner answers in milliseconds, and the cost
/// of guessing wrong is killing a live instance.
pub const ACK_TIMEOUT: Duration = Duration::from_secs(5);

pub enum Instance {
    /// This process owns the name. Call [`Server::run`] to receive what later
    /// launches forward.
    Primary(Server),
    /// Another process took our argument. This one should exit quietly — from
    /// the user's point of view the click worked, in the window already open.
    Secondary,
}

/// Claim `name` for this process, forwarding `arg` if someone else holds it.
///
/// `name` must be stable across launches and distinct per application; it is
/// namespaced per user internally, so two accounts on one machine do not
/// collide.
#[must_use]
pub fn acquire(name: &str, arg: &str) -> Instance {
    imp::acquire(name, arg)
}

pub struct Server(imp::ServerImpl);

impl Server {
    /// Receive forwarded arguments until the process ends.
    ///
    /// `on_arg` runs on a background thread and must not block: whatever it
    /// does is what the other process is waiting on. Returning `true`
    /// acknowledges — which is what tells the other side this process is alive
    /// rather than wedged.
    pub fn run(self, on_arg: impl Fn(&str) -> bool + Send + 'static) {
        self.0.run(on_arg);
    }
}

#[cfg(windows)]
mod imp {
    use std::io::{Read as _, Write as _};
    use std::thread;
    use std::time::Duration;

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, ERROR_PIPE_CONNECTED, HANDLE, INVALID_HANDLE_VALUE};
    use windows::Win32::Storage::FileSystem::{FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX};
    use windows::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeServerProcessId,
        PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT,
    };
    use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};

    use super::{Instance, Server, ACK_TIMEOUT};

    /// Written back once the owner has actually taken the argument. Its absence
    /// is the whole signal.
    const ACK: u8 = 1;

    /// Terminates a forwarded argument.
    ///
    /// It is what makes a bare relaunch — an empty argument — still put a byte
    /// on the wire. Without it the owner would block reading while the relaunch
    /// blocks waiting for the ack, and that circular wait looks exactly like a
    /// wedged owner.
    const EOM: u8 = b'\n';

    /// Attempts to take a pipe the previous holder is still releasing, after we
    /// killed it or it exited a moment ago.
    const CLAIM_ATTEMPTS: u32 = 20;
    const CLAIM_RETRY: Duration = Duration::from_millis(150);

    /// Pipe names are machine-global, so namespace by user: two accounts on one
    /// machine each get their own instance.
    fn pipe_path(name: &str) -> Vec<u16> {
        let user = std::env::var("USERNAME").unwrap_or_default();
        let path = format!(r"\\.\pipe\{name}.SingleInstance.{user}");
        path.encode_utf16().chain(std::iter::once(0)).collect()
    }

    struct Pipe(HANDLE);

    /// A kernel `HANDLE` is a token owned by the *process*, valid from any of
    /// its threads. It is `!Send` only because the binding wraps it in a raw
    /// pointer, which is too conservative for this case: the server loop is
    /// moved onto its own thread and there is exactly one owner throughout.
    unsafe impl Send for Pipe {}

    impl Drop for Pipe {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    /// Create the pipe, or fail because someone else owns it.
    fn create(path: &[u16]) -> Option<Pipe> {
        let handle = unsafe {
            CreateNamedPipeW(
                PCWSTR(path.as_ptr()),
                PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                1,
                4096,
                4096,
                0,
                None,
            )
        };
        (handle != INVALID_HANDLE_VALUE).then_some(Pipe(handle))
    }

    pub fn acquire(name: &str, arg: &str) -> Instance {
        let path = pipe_path(name);

        if let Some(pipe) = create(&path) {
            return Instance::Primary(Server(ServerImpl { pipe, path }));
        }

        // Someone holds it. Hand over the argument and see whether they are
        // alive enough to take it.
        match forward(&path, arg) {
            Ok(()) => Instance::Secondary,
            Err(owner) => {
                tracing::warn!("the running instance did not answer; taking over");
                if let Some(pid) = owner {
                    terminate(pid);
                }
                claim(&path).map_or_else(
                    || {
                        // Could not take it either. Running as a second copy is
                        // worse than running without the guard, but refusing to
                        // start at all is worse than both.
                        tracing::error!("could not take the instance name; continuing unguarded");
                        Instance::Primary(Server(ServerImpl {
                            pipe: Pipe(INVALID_HANDLE_VALUE),
                            path: Vec::new(),
                        }))
                    },
                    |pipe| Instance::Primary(Server(ServerImpl { pipe, path })),
                )
            }
        }
    }

    /// Send `arg` to the owner. `Err(pid)` means it never acknowledged.
    fn forward(path: &[u16], arg: &str) -> Result<(), Option<u32>> {
        let name = String::from_utf16_lossy(&path[..path.len() - 1]);
        let mut file = std::fs::OpenOptions::new().read(true).write(true).open(&name).map_err(|_| None)?;

        let pid = server_pid(&file);

        let mut message = arg.as_bytes().to_vec();
        message.push(EOM);
        file.write_all(&message).map_err(|_| pid)?;
        file.flush().map_err(|_| pid)?;

        // The read is what waits for the ack. A wedged owner never writes it,
        // and the OS gives up on the pipe rather than hanging forever.
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let mut ack = [0_u8; 1];
            let _ = tx.send(file.read_exact(&mut ack).is_ok() && ack[0] == ACK);
        });
        match rx.recv_timeout(ACK_TIMEOUT) {
            Ok(true) => Ok(()),
            _ => Err(pid),
        }
    }

    fn server_pid(file: &std::fs::File) -> Option<u32> {
        use std::os::windows::io::AsRawHandle as _;
        let mut pid = 0_u32;
        let handle = HANDLE(file.as_raw_handle().cast());
        unsafe { GetNamedPipeServerProcessId(handle, &raw mut pid) }.ok().map(|()| pid)
    }

    fn terminate(pid: u32) {
        unsafe {
            if let Ok(process) = OpenProcess(PROCESS_TERMINATE, false, pid) {
                let _ = TerminateProcess(process, 1);
                let _ = CloseHandle(process);
            }
        }
    }

    /// Take a pipe whose previous holder is still being released by the OS.
    fn claim(path: &[u16]) -> Option<Pipe> {
        (0..CLAIM_ATTEMPTS).find_map(|_| {
            create(path).or_else(|| {
                thread::sleep(CLAIM_RETRY);
                None
            })
        })
    }

    pub struct ServerImpl {
        pipe: Pipe,
        path: Vec<u16>,
    }

    impl ServerImpl {
        pub fn run(self, on_arg: impl Fn(&str) -> bool + Send + 'static) {
            if self.path.is_empty() {
                return; // unguarded, see `acquire`
            }
            thread::spawn(move || {
                let pipe = self.pipe;
                loop {
                    // `ERROR_PIPE_CONNECTED` means a client beat us to the
                    // connect call, which is a success and not an error.
                    let connected = unsafe { ConnectNamedPipe(pipe.0, None) };
                    if let Err(e) = &connected {
                        if e.code() != ERROR_PIPE_CONNECTED.into() {
                            thread::sleep(CLAIM_RETRY);
                            continue;
                        }
                    }
                    serve_one(&pipe, &on_arg);
                    unsafe {
                        let _ = DisconnectNamedPipe(pipe.0);
                    }
                }
            });
        }
    }

    fn serve_one(pipe: &Pipe, on_arg: &impl Fn(&str) -> bool) {
        use std::os::windows::io::FromRawHandle as _;

        // Borrowed, not owned: dropping this `File` would close the pipe that
        // `Pipe` is responsible for.
        let mut file =
            std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_handle(pipe.0 .0.cast()) });

        let mut message = Vec::new();
        let mut byte = [0_u8; 1];
        while file.read_exact(&mut byte).is_ok() && byte[0] != EOM {
            message.push(byte[0]);
        }

        let arg = String::from_utf8_lossy(&message).into_owned();
        if on_arg(&arg) {
            let _ = file.write_all(&[ACK]);
            let _ = file.flush();
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::{Instance, Server};

    /// Not implemented here yet.
    ///
    /// Said plainly rather than pretended: every launch is primary, so a second
    /// copy is possible and would share the session folder with the first. The
    /// Unix answer is an abstract socket or a lock file in the runtime dir, and
    /// it lands with the Linux build.
    pub fn acquire(_name: &str, _arg: &str) -> Instance {
        tracing::warn!("single-instance is not implemented on this platform");
        Instance::Primary(Server(ServerImpl))
    }

    pub struct ServerImpl;

    impl ServerImpl {
        pub fn run(self, _on_arg: impl Fn(&str) -> bool + Send + 'static) {}
    }
}
