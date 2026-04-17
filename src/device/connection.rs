use std::ffi::CString;
use std::mem::MaybeUninit;
use std::os::unix::fs::MetadataExt;
use std::os::unix::io::{FromRawFd, RawFd};
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, warn};

use crate::error::AttentioError;

/// Default timeout for `read_line()` (used by the debug reader stream).
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// An async connection to a device over a serial port.
pub struct DeviceConnection {
    /// Raw file descriptor — retained so Drop can clear TIOCEXCL before close.
    fd: RawFd,
    reader: BufReader<tokio::io::ReadHalf<tokio_serial::SerialStream>>,
    writer: tokio::io::WriteHalf<tokio_serial::SerialStream>,
    timeout: Duration,
}

impl Drop for DeviceConnection {
    fn drop(&mut self) {
        // Clear exclusive mode so the port is immediately available to the next
        // opener. The kernel releases TIOCEXCL on close() anyway, but doing it
        // explicitly here means the port is unlocked before the fd is fully
        // torn down by the tokio/serialport layers — avoiding a brief window
        // where check_port_in_use() (via /proc scan) would still see it as open.
        unsafe {
            libc::ioctl(self.fd, libc::TIOCNXCL);
        }
        debug!("Serial port released (TIOCNXCL cleared)");
    }
}

impl DeviceConnection {
    /// Open an async serial connection to the given port path.
    ///
    /// First checks whether any other process already has the port open (via
    /// `/proc` scan). If so, returns [`AttentioError::PortBusy`] without
    /// touching the device. After a successful open, claims `TIOCEXCL` so
    /// that future non-root processes cannot open the port while we hold it.
    pub fn open(port_path: &str) -> Result<Self, AttentioError> {
        debug!("Opening serial port: {}", port_path);

        let (fd, port) = open_serial(port_path)?;
        let (read_half, write_half) = tokio::io::split(port);

        Ok(Self {
            fd,
            reader: BufReader::new(read_half),
            writer: write_half,
            timeout: DEFAULT_TIMEOUT,
        })
    }

    /// Set the command response timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Read a single line from the port (useful for serial print streams).
    pub async fn read_line(&mut self) -> Result<String, AttentioError> {
        let mut line = String::new();

        let bytes_read = tokio::time::timeout(self.timeout, async {
            self.reader
                .read_line(&mut line)
                .await
                .map_err(AttentioError::Io)
        })
        .await;

        match bytes_read {
            Ok(Ok(0)) => Err(AttentioError::Protocol {
                message: "connection closed".to_string(),
            }),
            Ok(Ok(_)) => {
                let trimmed = line.trim_end_matches(['\r', '\n']).to_string();
                Ok(trimmed)
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(AttentioError::Timeout {
                seconds: self.timeout.as_secs(),
            }),
        }
    }

    /// Write raw bytes to the port (used by DFU enter to send AP packets).
    pub async fn write_raw(&mut self, data: &[u8]) -> Result<(), AttentioError> {
        self.writer
            .write_all(data)
            .await
            .map_err(AttentioError::Io)?;
        self.writer.flush().await.map_err(AttentioError::Io)?;
        Ok(())
    }

    /// Read raw bytes from the port into `buf`.
    ///
    /// Returns the number of bytes read. Used by the AP protocol parser to
    /// consume response bytes from the device.
    pub async fn read_raw(&mut self, buf: &mut [u8]) -> Result<usize, AttentioError> {
        self.reader.read(buf).await.map_err(AttentioError::Io)
    }
}

/// Check whether another process already has the given port open.
///
/// Scans `/proc/*/fd/` to find any file descriptor (belonging to a different
/// process) that points to the same device as `port_path`. Returns
/// [`AttentioError::PortBusy`] if a match is found.
///
/// Permission errors on individual `/proc/<pid>/fd` directories are silently
/// skipped — this is normal for processes owned by other users.
fn check_port_in_use(port_path: &str) -> Result<(), AttentioError> {
    // Stat the target port to get its device number (st_rdev).
    let port_meta = match std::fs::metadata(port_path) {
        Ok(m) => m,
        Err(e) => {
            // If we can't stat the port itself, let the caller's open() report the
            // real error (ENOENT, EACCES, etc.).
            debug!("Cannot stat {} for busy check: {}", port_path, e);
            return Ok(());
        }
    };
    let target_rdev = port_meta.rdev();
    if target_rdev == 0 {
        // Not a device file — skip the check.
        return Ok(());
    }

    let my_pid = std::process::id();

    let proc_dir = match std::fs::read_dir("/proc") {
        Ok(d) => d,
        Err(e) => {
            debug!("Cannot read /proc for busy check: {}", e);
            return Ok(());
        }
    };

    for entry in proc_dir.flatten() {
        // Only look at numeric directories (PIDs).
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let pid: u32 = match name_str.parse() {
            Ok(p) => p,
            Err(_) => continue,
        };

        // Skip our own process.
        if pid == my_pid {
            continue;
        }

        let fd_dir = format!("/proc/{}/fd", pid);
        let fds = match std::fs::read_dir(&fd_dir) {
            Ok(d) => d,
            Err(_) => continue, // EACCES / ENOENT — normal
        };

        for fd_entry in fds.flatten() {
            // Stat the symlink target (not the symlink itself).
            let fd_meta = match std::fs::metadata(fd_entry.path()) {
                Ok(m) => m,
                Err(_) => continue,
            };

            if fd_meta.rdev() == target_rdev {
                debug!(
                    "Port {} is held by PID {} (fd {:?})",
                    port_path,
                    pid,
                    fd_entry.file_name()
                );
                return Err(AttentioError::PortBusy {
                    port: port_path.to_string(),
                });
            }
        }
    }

    Ok(())
}

/// Open a serial port with exclusive access.
///
/// Before opening, scans `/proc/*/fd/` to detect whether any other process
/// already has the port open — if so, returns [`AttentioError::PortBusy`]
/// immediately. After a successful open, claims `TIOCEXCL` so that future
/// non-root openers receive `EBUSY`.
///
/// Steps:
///   1. Scan `/proc` for existing holders -> `PortBusy`
///   2. Open the fd via `libc::open()`
///   3. Claim `TIOCEXCL` (forward protection against new openers)
///   4. Configure termios and build async stream
///
/// Returns both the raw fd and the async stream. The caller stores the fd in
/// `DeviceConnection` so `Drop` can call `TIOCNXCL` before the fd is closed.
fn open_serial(port_path: &str) -> Result<(RawFd, tokio_serial::SerialStream), AttentioError> {
    // Step 1: Check if another process already has the port open.
    check_port_in_use(port_path)?;

    let c_path = CString::new(port_path).map_err(|_| {
        AttentioError::Other(format!(
            "invalid port path (contains null byte): {}",
            port_path
        ))
    })?;

    // Step 2: Open the fd directly.
    let fd = unsafe {
        libc::open(
            c_path.as_ptr(),
            libc::O_RDWR | libc::O_NOCTTY | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        let err = std::io::Error::last_os_error();
        // EBUSY means TIOCEXCL is set on the TTY — the kernel ACM driver holds
        // exclusive access (e.g. during device initialisation). Map this to
        // PortBusy so callers treat it the same as "another process has it open".
        if err.raw_os_error() == Some(libc::EBUSY) {
            return Err(AttentioError::PortBusy {
                port: port_path.to_string(),
            });
        }
        return Err(AttentioError::Other(format!(
            "failed to open {}: {}",
            port_path, err
        )));
    }

    // Step 3: Claim exclusive access (forward protection). Prevents other non-root
    // processes from opening this port while we hold it. Note: TIOCEXCL always
    // succeeds — it cannot detect already-open fds (that's what step 1 is for).
    let excl_ret = unsafe { libc::ioctl(fd, libc::TIOCEXCL) };
    if excl_ret < 0 {
        let err = std::io::Error::last_os_error();
        warn!("TIOCEXCL failed for {} (non-fatal): {}", port_path, err);
    }

    // From here on, if we encounter an error we must close the fd to avoid a leak.
    let result = configure_and_convert(fd, port_path);
    if result.is_err() {
        unsafe { libc::close(fd) };
    }
    result.map(|stream| (fd, stream))
}

/// Configure termios on an open fd and convert it to an async SerialStream.
///
/// This replicates the essential setup from `serialport::TTYPort::open()`:
///   - Enable CREAD | CLOCAL
///   - cfmakeraw for binary serial I/O
///   - Clear O_NONBLOCK (the async layer re-adds it as needed)
///
/// Then builds the async stream via `TTYPort::from_raw_fd()` → `SerialStream::try_from()`.
fn configure_and_convert(
    fd: i32,
    port_path: &str,
) -> Result<tokio_serial::SerialStream, AttentioError> {
    // Step 3: Configure termios for raw binary serial I/O.
    unsafe {
        let mut termios = MaybeUninit::<libc::termios>::uninit();
        if libc::tcgetattr(fd, termios.as_mut_ptr()) != 0 {
            return Err(AttentioError::Other(format!(
                "tcgetattr failed for {}: {}",
                port_path,
                std::io::Error::last_os_error()
            )));
        }
        let mut termios = termios.assume_init();

        // Enable reading and ignore modem control lines
        termios.c_cflag |= libc::CREAD | libc::CLOCAL;

        // Raw mode — disable all input/output processing
        libc::cfmakeraw(&mut termios);

        if libc::tcsetattr(fd, libc::TCSANOW, &termios) != 0 {
            return Err(AttentioError::Other(format!(
                "tcsetattr failed for {}: {}",
                port_path,
                std::io::Error::last_os_error()
            )));
        }
    }

    // Step 4: Clear O_NONBLOCK — the async layer (mio/tokio) will set it as needed.
    unsafe {
        libc::fcntl(fd, libc::F_SETFL, 0);
    }

    // Step 5: Build TTYPort from the raw fd.
    // We already hold TIOCEXCL; `FromRawFd` may attempt flock as best-effort.
    let tty_port = unsafe { serialport::TTYPort::from_raw_fd(fd) };

    // Step 6: Convert to async tokio_serial::SerialStream.
    let stream = tokio_serial::SerialStream::try_from(tty_port).map_err(|e| {
        // Don't close fd here — TTYPort took ownership and will close it on drop.
        // (We only close fd in the caller if we haven't reached this point.)
        AttentioError::Serial(e)
    })?;

    debug!("Serial port opened (exclusive): {}", port_path);
    Ok(stream)
}
