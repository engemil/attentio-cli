#[cfg(unix)]
use std::ffi::CString;
#[cfg(unix)]
use std::mem::MaybeUninit;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
#[cfg(unix)]
use std::os::unix::io::{FromRawFd, RawFd};
use std::collections::VecDeque;
use std::time::Duration;

use btleplug::api::{Characteristic, Peripheral as _, WriteType};
use btleplug::platform::Peripheral;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
#[cfg(windows)]
use tokio_serial::SerialPortBuilderExt;
use tracing::debug;
#[cfg(unix)]
use tracing::warn;

use crate::error::AttentioError;

/// Default timeout for `read_line()` (used by the serial reader stream).
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// Default baud rate used when opening a port on Windows.
///
/// Attentio devices use USB CDC-ACM, where the baud rate is virtual (the
/// wire-level USB transfer rate is fixed), but `tokio_serial::new()` requires
/// a value. 115200 is the conventional default.
#[cfg(windows)]
const DEFAULT_BAUD: u32 = 115_200;

/// An async connection to a device over a serial port.
pub struct DeviceConnection {
    /// Raw file descriptor — retained so Drop can clear TIOCEXCL before close.
    #[cfg(unix)]
    fd: RawFd,
    /// True while the fd is owned by this struct. If `into_parts()` was called,
    /// this becomes false and Drop skips TIOCNXCL — the [`FdGuard`] returned by
    /// `into_parts()` takes over ownership.
    #[cfg(unix)]
    owns_fd: bool,
    reader: BufReader<tokio::io::ReadHalf<tokio_serial::SerialStream>>,
    writer: tokio::io::WriteHalf<tokio_serial::SerialStream>,
    timeout: Duration,
}

impl Drop for DeviceConnection {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            if !self.owns_fd {
                // Halves and the fd guard were extracted via into_parts(); the
                // FdGuard is responsible for clearing TIOCNXCL when it drops.
                return;
            }
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
}

/// Read half of a split connection.
///
/// One variant per transport — serial (USB CDC-ACM) today, with a BLE variant
/// to follow. All variants expose `read_raw`, so consumers
/// (e.g. the permanent reader task in [`crate::protocol::client::ApClient`], or
/// the CLI monitor) are transport-agnostic.
pub enum ConnReader {
    /// Serial read half.
    Serial {
        inner: BufReader<tokio::io::ReadHalf<tokio_serial::SerialStream>>,
    },
    /// BLE read half.
    ///
    /// btleplug delivers RX-notify payloads as an async `Stream`; a pump task
    /// (spawned in [`crate::device::ble::open`]) forwards each notification's
    /// bytes into `rx`. `leftover` holds bytes from a notification that did not
    /// fit in a single `read_raw` call's buffer.
    Ble {
        rx: mpsc::Receiver<Vec<u8>>,
        leftover: VecDeque<u8>,
    },
}

impl ConnReader {
    /// Read raw bytes into `buf`.
    ///
    /// Returns the number of bytes read.
    pub async fn read_raw(&mut self, buf: &mut [u8]) -> Result<usize, AttentioError> {
        match self {
            ConnReader::Serial { inner } => inner.read(buf).await.map_err(AttentioError::Io),
            ConnReader::Ble { rx, leftover } => {
                // Refill from the notification channel when nothing is buffered.
                if leftover.is_empty() {
                    match rx.recv().await {
                        Some(chunk) => leftover.extend(chunk),
                        // Pump task ended (peripheral disconnected) — signal EOF,
                        // matching the serial Ok(0) convention so reader_loop exits.
                        None => return Ok(0),
                    }
                }
                let n = leftover.len().min(buf.len());
                for slot in buf.iter_mut().take(n) {
                    *slot = leftover.pop_front().expect("leftover non-empty");
                }
                Ok(n)
            }
        }
    }
}

/// Write half of a split connection. One variant per transport (see [`ConnReader`]).
pub enum ConnWriter {
    /// Serial write half.
    Serial {
        inner: tokio::io::WriteHalf<tokio_serial::SerialStream>,
    },
    /// BLE write half — writes AP frames to the TX characteristic, chunked to
    /// `chunk_size` (ATT MTU − 3); the device reassembles by AP LEN.
    Ble {
        peripheral: Peripheral,
        tx_char: Characteristic,
        chunk_size: usize,
    },
}

impl ConnWriter {
    /// Write raw bytes and flush.
    pub async fn write_raw(&mut self, data: &[u8]) -> Result<(), AttentioError> {
        match self {
            ConnWriter::Serial { inner } => {
                inner.write_all(data).await.map_err(AttentioError::Io)?;
                inner.flush().await.map_err(AttentioError::Io)?;
                Ok(())
            }
            ConnWriter::Ble {
                peripheral,
                tx_char,
                chunk_size,
            } => {
                // WithResponse matches the verified ble_smoke.py path
                // (`response=True`) and prompts BlueZ to elevate security
                // (Just-Works pairing) on the encryption-required TX char.
                for chunk in data.chunks((*chunk_size).max(1)) {
                    peripheral
                        .write(tx_char, chunk, WriteType::WithResponse)
                        .await
                        .map_err(|e| AttentioError::Ble(e.to_string()))?;
                }
                Ok(())
            }
        }
    }
}

/// Owns the resources of a split [`DeviceConnection`] and releases them on drop.
///
/// Must outlive any [`ConnReader`] / [`ConnWriter`] derived from the same
/// connection (the serial halves use the fd via tokio internally and become
/// invalid once the fd is closed; the BLE halves rely on the peripheral staying
/// connected).
///
/// - `Serial` clears `TIOCEXCL` on drop so the port is immediately available to
///   the next opener. On Windows the fd is absent — Windows opens serial ports
///   exclusively by default (the `serialport` crate uses `CreateFileW` without
///   sharing flags), so no explicit release is needed.
/// - `Ble` holds the connected peripheral so the link stays up for the client's
///   lifetime, and best-effort disconnects it on drop.
pub enum ConnGuard {
    Serial {
        #[cfg(unix)]
        fd: RawFd,
    },
    Ble {
        peripheral: Peripheral,
    },
}

impl Drop for ConnGuard {
    fn drop(&mut self) {
        match self {
            ConnGuard::Serial { .. } => {
                #[cfg(unix)]
                {
                    let ConnGuard::Serial { fd } = self else {
                        return;
                    };
                    unsafe {
                        libc::ioctl(*fd, libc::TIOCNXCL);
                    }
                    debug!("Serial port released (TIOCNXCL cleared via ConnGuard)");
                }
            }
            ConnGuard::Ble { peripheral } => {
                // Drop can't await; spawn a best-effort disconnect. The OS also
                // tears down the link on process exit, so this is a courtesy.
                let p = peripheral.clone();
                tokio::spawn(async move {
                    let _ = p.disconnect().await;
                });
            }
        }
    }
}

impl ConnGuard {
    /// Live signal strength (RSSI, dBm) of the underlying transport, if it has
    /// one. Serial connections have no RSSI and return `None`; for BLE this reads
    /// the connected peripheral's cached advertisement properties (best-effort —
    /// returns `None` if the adapter hasn't surfaced an RSSI yet).
    pub async fn ble_rssi(&self) -> Option<i16> {
        match self {
            ConnGuard::Serial { .. } => None,
            ConnGuard::Ble { peripheral } => {
                peripheral.properties().await.ok().flatten().and_then(|p| p.rssi)
            }
        }
    }
}

impl DeviceConnection {
    /// Open an async serial connection to the given port path.
    ///
    /// On Unix, first checks whether any other process already has the port
    /// open (via `/proc` scan). If so, returns [`AttentioError::PortBusy`]
    /// without touching the device. After a successful open, claims `TIOCEXCL`
    /// so that future non-root processes cannot open the port while we hold it.
    ///
    /// On Windows, exclusive access is provided by `CreateFileW`'s default
    /// (no sharing) flags inside the `serialport` crate, so a separate busy
    /// check is unnecessary.
    pub fn open(port_path: &str) -> Result<Self, AttentioError> {
        debug!("Opening serial port: {}", port_path);

        let port = open_serial(port_path)?;

        #[cfg(unix)]
        let fd = port.0;
        #[cfg(unix)]
        let stream = port.1;
        #[cfg(windows)]
        let stream = port;

        let (read_half, write_half) = tokio::io::split(stream);

        Ok(Self {
            #[cfg(unix)]
            fd,
            #[cfg(unix)]
            owns_fd: true,
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

    /// Split the connection into independent reader, writer, and guard.
    ///
    /// The returned [`ConnReader`] and [`ConnWriter`] can be used concurrently
    /// from different tasks. The [`ConnGuard`] takes over the responsibility of
    /// clearing `TIOCEXCL` on drop and **must** be kept alive at least as long
    /// as either half — when the guard drops, the fd is no longer protected
    /// from races with re-openers. On Windows, the serial guard is a no-op marker.
    #[cfg_attr(not(unix), allow(unused_mut))]
    pub fn into_parts(mut self) -> (ConnReader, ConnWriter, ConnGuard) {
        // Disarm Drop so it doesn't fire TIOCNXCL on the fd we're handing off.
        #[cfg(unix)]
        {
            self.owns_fd = false;
        }
        #[cfg(unix)]
        let fd = self.fd;

        // Move the halves out. We can't pattern-destructure self because of the
        // Drop impl, so we use ManuallyDrop-style replacement via std::ptr::read.
        // Safe: self is owned by us and won't be used again after this function
        // returns. We forget self afterwards so its Drop doesn't run twice on
        // the moved-out fields.
        let reader = unsafe { std::ptr::read(&self.reader) };
        let writer = unsafe { std::ptr::read(&self.writer) };
        std::mem::forget(self);

        (
            ConnReader::Serial { inner: reader },
            ConnWriter::Serial { inner: writer },
            ConnGuard::Serial {
                #[cfg(unix)]
                fd,
            },
        )
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
#[cfg(unix)]
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

/// Open a serial port with exclusive access (Unix).
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
#[cfg(unix)]
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

/// Open a serial port (Windows).
///
/// Uses `tokio_serial`'s high-level builder, which opens the COM port via
/// `CreateFileW` without sharing flags — giving us exclusive access for free.
/// A `serialport::Error` with `ErrorKind::NoDevice` maps to `PortBusy` when
/// another process already holds the port.
#[cfg(windows)]
fn open_serial(port_path: &str) -> Result<tokio_serial::SerialStream, AttentioError> {
    let stream = tokio_serial::new(port_path, DEFAULT_BAUD)
        .open_native_async()
        .map_err(|e| {
            // Windows reports `ERROR_ACCESS_DENIED` when another process has the
            // port open with exclusive access; map that to PortBusy.
            if matches!(e.kind, serialport::ErrorKind::Io(std::io::ErrorKind::PermissionDenied)) {
                AttentioError::PortBusy {
                    port: port_path.to_string(),
                }
            } else {
                AttentioError::Serial(e)
            }
        })?;

    debug!("Serial port opened: {}", port_path);
    Ok(stream)
}

/// Configure termios on an open fd and convert it to an async SerialStream.
///
/// This replicates the essential setup from `serialport::TTYPort::open()`:
///   - Enable CREAD | CLOCAL
///   - cfmakeraw for binary serial I/O
///   - Clear O_NONBLOCK (the async layer re-adds it as needed)
///
/// Then builds the async stream via `TTYPort::from_raw_fd()` → `SerialStream::try_from()`.
#[cfg(unix)]
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A BLE notification larger than the caller's buffer is returned across
    /// multiple `read_raw` calls, and channel close surfaces as EOF (`Ok(0)`).
    #[tokio::test]
    async fn ble_reader_splits_and_eofs() {
        let (tx, rx) = mpsc::channel::<Vec<u8>>(4);
        let mut reader = ConnReader::Ble {
            rx,
            leftover: VecDeque::new(),
        };

        // One 5-byte notification, read 3 bytes at a time.
        tx.send(vec![1, 2, 3, 4, 5]).await.unwrap();

        let mut buf = [0u8; 3];
        let n = reader.read_raw(&mut buf).await.unwrap();
        assert_eq!(n, 3);
        assert_eq!(&buf[..3], &[1, 2, 3]);

        let n = reader.read_raw(&mut buf).await.unwrap();
        assert_eq!(n, 2);
        assert_eq!(&buf[..2], &[4, 5]);

        // Dropping the sender (pump task ended) yields EOF.
        drop(tx);
        let n = reader.read_raw(&mut buf).await.unwrap();
        assert_eq!(n, 0);
    }
}
