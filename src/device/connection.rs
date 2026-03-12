use std::ffi::CString;
use std::mem::MaybeUninit;
use std::os::unix::fs::MetadataExt;
use std::os::unix::io::FromRawFd;
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, trace, warn};

use crate::error::AttentioError;

/// Default timeout for `read_line()` (used by the debug reader stream).
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// Hard timeout for the entire `send_command()` response.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

/// Inter-line timeout: if we've received at least one response line and no
/// more data arrives within this window, the response is considered complete.
/// Handles commands (like ChibiOS built-in `help`) that don't send an
/// `OK`/`ERROR` terminator — they just print output and return to the prompt.
const INTER_LINE_TIMEOUT: Duration = Duration::from_millis(300);

/// An async connection to a device over a serial port.
pub struct DeviceConnection {
    reader: BufReader<tokio::io::ReadHalf<tokio_serial::SerialStream>>,
    writer: tokio::io::WriteHalf<tokio_serial::SerialStream>,
    timeout: Duration,
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

        let port = open_serial(port_path)?;
        let (read_half, write_half) = tokio::io::split(port);

        Ok(Self {
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

    /// Send a command and wait for the response.
    ///
    /// Protocol: send `<cmd>\r\n`, read lines until `OK\r\n` or `ERROR <msg>\r\n`.
    /// Returns the response body (lines before the OK/ERROR terminator).
    ///
    /// ChibiOS shell compatibility:
    /// - **Echo handling**: The device echoes back the sent command. The first
    ///   received line is checked — if it ends with the sent command text, it is
    ///   silently discarded.
    /// - **Missing terminator**: Some built-in ChibiOS commands (like `help`)
    ///   don't send `OK`/`ERROR`. After receiving at least one response line,
    ///   if no more data arrives within [`INTER_LINE_TIMEOUT`], the response is
    ///   considered complete.
    /// - **Prompt handling**: Before sending, any stale data in the read buffer
    ///   (like the device's `attentio> ` prompt) is drained.
    pub async fn send_command(&mut self, cmd: &str) -> Result<String, AttentioError> {
        debug!("Sending command: {:?}", cmd);

        // Drain any stale data in the read buffer (e.g., the device's prompt
        // from a previous command).
        self.drain_pending().await;

        // Send command with \r\n terminator
        let cmd_bytes = format!("{}\r\n", cmd);
        self.writer
            .write_all(cmd_bytes.as_bytes())
            .await
            .map_err(AttentioError::Io)?;
        self.writer.flush().await.map_err(AttentioError::Io)?;

        trace!("Command sent, waiting for response...");

        let cmd_trimmed = cmd.trim();
        let start = Instant::now();
        let mut response_lines: Vec<String> = Vec::new();
        let mut echo_skipped = false;

        loop {
            let elapsed = start.elapsed();
            let remaining = COMMAND_TIMEOUT.checked_sub(elapsed).unwrap_or_default();
            if remaining.is_zero() {
                // Hard timeout expired
                if !response_lines.is_empty() {
                    // We have partial data — return it rather than erroring
                    debug!(
                        "Command hard timeout with {} lines collected",
                        response_lines.len()
                    );
                    return Ok(response_lines.join("\n"));
                }
                return Err(AttentioError::Timeout {
                    seconds: COMMAND_TIMEOUT.as_secs(),
                });
            }

            // If we already have at least one response line, use the shorter
            // inter-line timeout — if no more data arrives, the response is done.
            let read_timeout = if response_lines.is_empty() {
                remaining
            } else {
                remaining.min(INTER_LINE_TIMEOUT)
            };

            let mut line = String::new();
            let read_result = tokio::time::timeout(read_timeout, async {
                self.reader
                    .read_line(&mut line)
                    .await
                    .map_err(AttentioError::Io)
            })
            .await;

            match read_result {
                Ok(Ok(0)) => {
                    return Err(AttentioError::Protocol {
                        message: "connection closed unexpectedly".to_string(),
                    });
                }
                Ok(Ok(_)) => {
                    let trimmed = line.trim_end_matches(['\r', '\n']);
                    trace!("Received line: {:?}", trimmed);

                    // Skip the echo line — the first line that ends with the
                    // sent command text. The device may prepend its prompt
                    // (e.g., "attentio> help"), so we check `ends_with`.
                    if !echo_skipped {
                        echo_skipped = true;
                        if trimmed.ends_with(cmd_trimmed) {
                            trace!("Skipping echo line: {:?}", trimmed);
                            continue;
                        }
                        // Not an echo — fall through to process as response
                    }

                    if trimmed == "OK" {
                        debug!("Command completed successfully");
                        return Ok(response_lines.join("\n"));
                    } else if let Some(err_msg) = trimmed.strip_prefix("ERROR ") {
                        return Err(AttentioError::Protocol {
                            message: err_msg.to_string(),
                        });
                    } else if trimmed == "ERROR" {
                        return Err(AttentioError::Protocol {
                            message: "unknown error".to_string(),
                        });
                    } else {
                        response_lines.push(trimmed.to_string());
                    }
                }
                Ok(Err(e)) => return Err(e),
                Err(_) => {
                    // Timeout fired
                    if !response_lines.is_empty() {
                        // Inter-line timeout: we have data, response is complete.
                        // This handles commands like `help` that don't send OK.
                        debug!(
                            "Response complete (inter-line timeout, {} lines)",
                            response_lines.len()
                        );
                        return Ok(response_lines.join("\n"));
                    }
                    // No data at all yet — keep waiting until hard timeout
                    // (the loop condition checks remaining time)
                }
            }
        }
    }

    /// Drain any pending data from the read buffer.
    ///
    /// Discards stale bytes that may be left over from a previous command
    /// (e.g., the device's shell prompt `attentio> ` which doesn't end with
    /// a newline and sits in the buffer).
    async fn drain_pending(&mut self) {
        // First, discard any data already in the BufReader's internal buffer.
        let buffered = self.reader.buffer().len();
        if buffered > 0 {
            trace!("Draining {} buffered bytes", buffered);
            self.reader.consume(buffered);
        }
        // Then, try a very short non-blocking read to discard any data that
        // has arrived in the OS buffer but isn't yet in the BufReader.
        let mut discard = [0u8; 512];
        let _ = tokio::time::timeout(Duration::from_millis(5), async {
            loop {
                match self.reader.read(&mut discard).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        trace!("Drained {} bytes from OS buffer", n);
                        continue;
                    }
                }
            }
        })
        .await;
    }

    /// Read a single line from the port (useful for debug print streams).
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

    /// Write raw bytes to the port (useful for interactive shell).
    #[allow(dead_code)] // Reserved for future shell/TUI improvements
    pub async fn write_raw(&mut self, data: &[u8]) -> Result<(), AttentioError> {
        self.writer
            .write_all(data)
            .await
            .map_err(AttentioError::Io)?;
        self.writer.flush().await.map_err(AttentioError::Io)?;
        Ok(())
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
fn open_serial(port_path: &str) -> Result<tokio_serial::SerialStream, AttentioError> {
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
    result
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
