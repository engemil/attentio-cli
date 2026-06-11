//! BLE transport for the Attentio Protocol.
//!
//! The device serves AP over a GATT service (see the wireless-module firmware
//! and `scripts/test/ble_smoke.py`):
//! - Service `1209eea1-0001-…`
//! - TX char `1209eea1-0002-…` (host→device, write, **encryption-required**;
//!   the first write triggers Just-Works/LESC pairing + bonding on BlueZ)
//! - RX char `1209eea1-0003-…` (device→host, notify; fragmented at ATT MTU−3)
//!
//! [`open`] scans for the device, connects, subscribes to RX notifications, and
//! returns split transport parts ([`ConnReader`]/[`ConnWriter`]/[`ConnGuard`])
//! that [`crate::protocol::client::ApClient::from_parts`] drives exactly like a
//! serial connection. The AP byte stream is unchanged — the existing
//! [`crate::protocol::packet::ApParser`] reassembles frames from RX bytes.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use btleplug::api::{Central, Characteristic, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::{Manager, Peripheral};
use futures_util::StreamExt;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::device::connection::{ConnGuard, ConnReader, ConnWriter};
use crate::error::AttentioError;

/// Attentio BLE service UUID (advertised in the advertisement PDU).
const SERVICE_UUID: Uuid = Uuid::from_u128(0x1209eea1_0001_0000_0000_000000000000);
/// TX characteristic — host writes AP frames here (encryption-required).
const TX_CHAR_UUID: Uuid = Uuid::from_u128(0x1209eea1_0002_0000_0000_000000000000);
/// RX characteristic — device notifies AP responses/events here.
const RX_CHAR_UUID: Uuid = Uuid::from_u128(0x1209eea1_0003_0000_0000_000000000000);

/// Advertised device name (in the scan response).
const DEVICE_NAME: &str = "AttentioLight-1";

/// Default TX chunk size: ATT default MTU (23) − 3-byte notify/write header.
/// The device reassembles writes by AP LEN, so a conservative fixed value is
/// safe across platforms regardless of the negotiated MTU.
const TX_CHUNK_SIZE: usize = 20;

/// How long to scan before giving up on finding a matching device.
const SCAN_TIMEOUT: Duration = Duration::from_secs(5);
/// Poll interval while scanning.
const SCAN_POLL: Duration = Duration::from_millis(250);

/// Buffer depth for the RX-notification pump channel.
const NOTIF_CHANNEL_DEPTH: usize = 64;

/// Bound for `Device1.Connect` so a stalled connect (e.g. the single-session
/// firmware slot is held by another central) fails fast with a clear message
/// instead of BlueZ's 30 s D-Bus reply timeout.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Bound for post-connect GATT setup (subscribe, notify).
const GATT_OP_TIMEOUT: Duration = Duration::from_secs(10);
/// Overall budget for the TX/RX characteristics to surface. btleplug's
/// `connect()` only waits 5 s for BlueZ's `ServicesResolved`; resolution can take
/// longer, so we poll `discover_services` up to this budget after the link is up.
const SERVICE_RESOLVE_TIMEOUT: Duration = Duration::from_secs(20);

/// How a `--ble` invocation selects which device to connect to.
#[derive(Debug, Clone)]
pub enum BleSelector {
    /// `--ble` with no value — connect to the single advertised AttentioLight-1.
    Any,
    /// `--ble=<name>` — match the advertised local name.
    Name(String),
    /// `--ble=<MAC>` — match the BD_ADDR (case-insensitive).
    Address(String),
    /// `--ble=<N>` — the Nth device (1-based) from `attentio list`'s unified order.
    Index(usize),
}

impl std::fmt::Display for BleSelector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BleSelector::Any => write!(f, "AttentioLight-1 (any)"),
            BleSelector::Name(n) => write!(f, "name '{n}'"),
            BleSelector::Address(a) => write!(f, "address '{a}'"),
            BleSelector::Index(n) => write!(f, "index {n}"),
        }
    }
}

/// Process-global BLE selector, set once in `main.rs` from the `--ble` flag.
///
/// `open_client` reads this to decide between the serial and BLE transports
/// without threading a flag through every command handler.
static SELECTOR: OnceLock<Option<BleSelector>> = OnceLock::new();

/// Record the BLE selector for this process. Called once from `main`.
pub fn set_selector(sel: Option<BleSelector>) {
    let _ = SELECTOR.set(sel);
}

/// The active BLE selector, if `--ble` was given.
pub fn active_selector() -> Option<BleSelector> {
    SELECTOR.get().cloned().flatten()
}

fn ble_err(e: btleplug::Error) -> AttentioError {
    AttentioError::Ble(e.to_string())
}

/// Open a BLE connection to the selected device and return split transport parts.
///
/// Scans (filtered by service UUID), matches the selector, connects, ensures the
/// bond, discovers the TX/RX characteristics, subscribes to RX notifications, and
/// spawns a pump task forwarding notification bytes into the [`ConnReader`].
pub async fn open(
    sel: &BleSelector,
) -> Result<(ConnReader, ConnWriter, ConnGuard), AttentioError> {
    // Resolve a `--ble=<N>` index against the same unified ordering as
    // `attentio list` before touching the adapter for the connect path.
    let resolved = match sel {
        BleSelector::Index(n) => resolve_index(*n).await?,
        other => other.clone(),
    };
    let sel = &resolved;

    let manager = Manager::new().await.map_err(ble_err)?;
    let adapter = manager
        .adapters()
        .await
        .map_err(ble_err)?
        .into_iter()
        .next()
        .ok_or_else(|| AttentioError::Ble("no Bluetooth adapter found".into()))?;

    let peripheral = scan_and_match(&adapter, sel).await?;
    let address = peripheral.address().to_string();

    // Did the host already hold a bond on entry? If so, a service-discovery
    // drop on connect signals a host/device bond *mismatch* — the device's NVS
    // bond was lost (re-flash, NVS erase, or round-robin eviction) while BlueZ
    // still holds an LTK, so it encrypts instead of re-pairing and the device
    // drops the link. We can auto-heal that case below; see PLAN.md "BLE bond
    // desync".
    let was_bonded = paired_status(&address).await == Some(true);

    // Bond BEFORE connecting/writing. The TX characteristic is
    // encryption-required, so the first write must happen over an encrypted
    // link; otherwise BlueZ blocks the write trying to pair and the D-Bus call
    // hits its 25 s reply timeout. ensure_bonded hard-errors (with manual
    // instructions) rather than letting a doomed write hang.
    ensure_bonded(&address).await?;

    match attempt_open(peripheral).await {
        Ok(parts) => Ok(parts),
        // Auto-heal a stale bond: drop the host LTK, re-pair fresh (the
        // firmware's REPEAT_PAIRING handler clears the device side once a real
        // pairing request arrives), and retry the whole connect → discover →
        // subscribe sequence exactly once. The link survives service discovery
        // (an unencrypted attribute-table read) but drops at the first
        // encryption-required op — usually the RX `subscribe` — so the heal must
        // cover that step too. Gated on `was_bonded` so we never wipe a bond we
        // just created.
        Err(e) if was_bonded && is_bond_mismatch(&e) => {
            warn!(
                "BLE {address}: {e}\n\
                 Auto-healing a stale bond: removing the host key and re-pairing."
            );
            remove_bond(&address).await;
            // bluetoothctl remove drops BlueZ's device object, invalidating the
            // old peripheral handle. A fresh scan rediscovers the device (and
            // leaves it in BlueZ's cache so the re-pair below can find it).
            tokio::time::sleep(Duration::from_millis(500)).await;
            let peripheral = scan_and_match(&adapter, sel).await?;
            ensure_bonded(&address).await?;
            attempt_open(peripheral).await
        }
        Err(e) => Err(e),
    }
}

/// One full open attempt: connect, resolve characteristics, subscribe to RX
/// notifications, and spawn the pump task, returning the split transport parts.
///
/// Factored out so [`open`] can retry the whole sequence after a bond heal. It
/// takes ownership of the peripheral so a failed attempt is dropped cleanly and
/// a freshly scanned handle is used on the retry. Nothing here needs unwinding
/// on failure: the pump task is spawned only once every step has succeeded.
async fn attempt_open(
    peripheral: Peripheral,
) -> Result<(ConnReader, ConnWriter, ConnGuard), AttentioError> {
    let (peripheral, tx_char, rx_char) = connect_and_discover(peripheral).await?;

    debug!("BLE subscribing to RX notifications");
    step_timeout(GATT_OP_TIMEOUT, "subscribe", peripheral.subscribe(&rx_char)).await?;

    // Pump notifications (a Stream) into an mpsc the ConnReader::Ble drains.
    let mut notifs =
        step_timeout(GATT_OP_TIMEOUT, "notifications", peripheral.notifications()).await?;
    let (tx, rx) = mpsc::channel::<Vec<u8>>(NOTIF_CHANNEL_DEPTH);
    tokio::spawn(async move {
        while let Some(n) = notifs.next().await {
            // ConnReader dropped — stop pumping.
            if n.uuid == RX_CHAR_UUID && tx.send(n.value).await.is_err() {
                break;
            }
        }
        // Stream ended (disconnect) — dropping `tx` makes read_raw return Ok(0).
        debug!("BLE notification pump exiting");
    });

    let reader = ConnReader::Ble {
        rx,
        leftover: Default::default(),
    };
    let writer = ConnWriter::Ble {
        peripheral: peripheral.clone(),
        tx_char,
        chunk_size: TX_CHUNK_SIZE,
    };
    let guard = ConnGuard::Ble { peripheral };

    Ok((reader, writer, guard))
}

/// Start a service-filtered scan, wait for a peripheral matching `sel`, then
/// stop scanning. Used for the initial connect and to rediscover the device
/// after a bond heal (`bluetoothctl remove` drops BlueZ's device object, so the
/// old [`Peripheral`] handle is stale and must be re-acquired).
async fn scan_and_match(
    adapter: &btleplug::platform::Adapter,
    sel: &BleSelector,
) -> Result<Peripheral, AttentioError> {
    adapter
        .start_scan(ScanFilter {
            services: vec![SERVICE_UUID],
        })
        .await
        .map_err(ble_err)?;
    let result = find_matching(adapter, sel).await;
    let _ = adapter.stop_scan().await;
    result
}

/// Connect to the peripheral and resolve the Attentio TX/RX characteristics.
///
/// Takes ownership and returns the peripheral back alongside the resolved
/// characteristics, so a failed attempt can be dropped and a freshly scanned
/// handle used on the bond-heal retry.
async fn connect_and_discover(
    peripheral: Peripheral,
) -> Result<(Peripheral, Characteristic, Characteristic), AttentioError> {
    connect_with_retry(&peripheral).await?;
    let (tx_char, rx_char) = discover_chars(&peripheral).await?;
    Ok((peripheral, tx_char, rx_char))
}

/// True if a connect/discover/subscribe failure carries the stale-bond
/// signature: the link comes up and survives service discovery (an unencrypted
/// attribute-table read), but the device drops it at the first
/// encryption-required op — the RX `subscribe`/notification setup, or, for slow
/// resolvers, mid-discovery — because its stored bond no longer matches the host
/// LTK. Used only when the host was already bonded, to decide whether to
/// auto-heal by removing the host key and re-pairing.
fn is_bond_mismatch(e: &AttentioError) -> bool {
    matches!(e, AttentioError::Ble(m)
        if m.contains("service discovery")
            || m.contains("did not resolve in time")
            || m.contains("subscribe timed out")
            || m.contains("notifications timed out"))
}

/// Connect to the peripheral, bounded by [`CONNECT_TIMEOUT`], retrying once.
///
/// btleplug's `connect()` does `Device1.Connect` then waits up to 5 s for BlueZ's
/// `ServicesResolved` — on this device resolution is slower than that, so a
/// `Service discovery timed out` error with the **link actually up** is treated
/// as success; [`discover_chars`] then polls for the characteristics with a
/// longer budget. Only the retry forces a clean `disconnect()` first (clearing a
/// stale/half connection); the first attempt reuses any cached GATT for speed.
async fn connect_with_retry(peripheral: &Peripheral) -> Result<(), AttentioError> {
    for attempt in 1..=2u8 {
        if attempt > 1 {
            let _ = peripheral.disconnect().await;
            tokio::time::sleep(Duration::from_millis(300)).await;
        }

        debug!("BLE connect attempt {attempt} to {}", peripheral.address());
        match tokio::time::timeout(CONNECT_TIMEOUT, peripheral.connect()).await {
            Ok(Ok(())) => {
                debug!("BLE connected to {}", peripheral.address());
                return Ok(());
            }
            Ok(Err(e)) => {
                // The link can be up even though btleplug's internal 5 s
                // service-discovery wait elapsed; if so, proceed and resolve
                // services ourselves.
                if peripheral.is_connected().await.unwrap_or(false) {
                    debug!("BLE link up to {} (services pending: {e})", peripheral.address());
                    return Ok(());
                }
                if attempt == 2 {
                    let addr = peripheral.address();
                    return Err(AttentioError::Ble(format!(
                        "the device dropped the link during service discovery ({e}). \
                         This usually means a stale/mismatched bond or another central \
                         holding the single BLE session. Try: make sure no phone is \
                         connected, then `bluetoothctl remove {addr}` and re-pair."
                    )));
                }
                warn!("BLE connect attempt {attempt} failed: {e}; retrying");
            }
            Err(_) if attempt == 2 => {
                return Err(AttentioError::Ble(format!(
                    "connect timed out after {}s — is the device already connected to a \
                     phone or another host? The firmware allows a single BLE session.",
                    CONNECT_TIMEOUT.as_secs()
                )));
            }
            Err(_) => warn!("BLE connect attempt {attempt} timed out; retrying"),
        }
    }
    unreachable!("connect loop always returns by the second attempt")
}

/// Poll BlueZ for the Attentio TX/RX characteristics, tolerating slow service
/// resolution (the link may be up before `ServicesResolved` fires).
async fn discover_chars(
    peripheral: &Peripheral,
) -> Result<(Characteristic, Characteristic), AttentioError> {
    let deadline = Instant::now() + SERVICE_RESOLVE_TIMEOUT;
    debug!("BLE resolving services on {}", peripheral.address());
    loop {
        // Best-effort: ask BlueZ to (re)read the GATT table; ignore transient errors.
        let _ = tokio::time::timeout(GATT_OP_TIMEOUT, peripheral.discover_services()).await;

        let chars = peripheral.characteristics();
        let tx = chars.iter().find(|c| c.uuid == TX_CHAR_UUID).cloned();
        let rx = chars.iter().find(|c| c.uuid == RX_CHAR_UUID).cloned();
        if let (Some(tx), Some(rx)) = (tx, rx) {
            debug!("BLE characteristics resolved");
            return Ok((tx, rx));
        }

        if Instant::now() >= deadline {
            return Err(AttentioError::Ble(
                "the Attentio TX/RX characteristics did not resolve in time — \
                 try moving closer, or re-pair the device"
                    .into(),
            ));
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// Run a btleplug GATT step under a timeout, mapping a stall to a clear error.
async fn step_timeout<F, T>(dur: Duration, step: &str, fut: F) -> Result<T, AttentioError>
where
    F: std::future::Future<Output = Result<T, btleplug::Error>>,
{
    match tokio::time::timeout(dur, fut).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(ble_err(e)),
        Err(_) => Err(AttentioError::Ble(format!(
            "{step} timed out after {}s",
            dur.as_secs()
        ))),
    }
}

/// A device found during a discovery scan (for `attentio list`).
#[derive(Debug, Clone)]
pub struct BleDeviceInfo {
    /// BD_ADDR, e.g. "AA:BB:CC:DD:EE:FF".
    pub address: String,
    /// Advertised local name, if present.
    pub name: Option<String>,
}

/// Best-effort scan for Attentio devices, for `attentio list`.
///
/// Never fails the caller: returns an empty list if no adapter is present or
/// scanning errors (so `list` still shows USB devices on a host without BLE).
pub async fn scan(duration: Duration) -> Vec<BleDeviceInfo> {
    match scan_inner(duration).await {
        Ok(found) => found,
        Err(e) => {
            debug!("BLE scan skipped: {e}");
            Vec::new()
        }
    }
}

async fn scan_inner(duration: Duration) -> Result<Vec<BleDeviceInfo>, AttentioError> {
    let manager = Manager::new().await.map_err(ble_err)?;
    let adapter = manager
        .adapters()
        .await
        .map_err(ble_err)?
        .into_iter()
        .next()
        .ok_or_else(|| AttentioError::Ble("no Bluetooth adapter found".into()))?;

    adapter
        .start_scan(ScanFilter {
            services: vec![SERVICE_UUID],
        })
        .await
        .map_err(ble_err)?;
    tokio::time::sleep(duration).await;
    let peripherals = adapter.peripherals().await.map_err(ble_err)?;
    let _ = adapter.stop_scan().await;

    let mut found = Vec::new();
    for p in peripherals {
        let Ok(Some(props)) = p.properties().await else {
            continue;
        };
        // Keep only Attentio devices (service-filtered scans can still surface
        // cached peripherals from earlier sessions).
        if !props.services.contains(&SERVICE_UUID)
            && props.local_name.as_deref() != Some(DEVICE_NAME)
        {
            continue;
        }
        found.push(BleDeviceInfo {
            address: props.address.to_string(),
            name: props.local_name,
        });
    }
    Ok(found)
}

/// Poll the adapter's peripheral list until one matches `sel` or the scan times out.
async fn find_matching(
    adapter: &btleplug::platform::Adapter,
    sel: &BleSelector,
) -> Result<Peripheral, AttentioError> {
    let deadline = Instant::now() + SCAN_TIMEOUT;

    loop {
        let peripherals = adapter.peripherals().await.map_err(ble_err)?;
        let mut matches: Vec<Peripheral> = Vec::new();

        for p in peripherals {
            let props = match p.properties().await {
                Ok(Some(props)) => props,
                _ => continue,
            };
            let name = props.local_name.as_deref();
            let advertises_service = props.services.contains(&SERVICE_UUID);

            let is_match = match sel {
                BleSelector::Any => name == Some(DEVICE_NAME) || advertises_service,
                BleSelector::Name(n) => name == Some(n.as_str()),
                BleSelector::Address(a) => {
                    props.address.to_string().eq_ignore_ascii_case(a)
                }
                // Index is resolved to an Address by open() before scanning.
                BleSelector::Index(_) => unreachable!("index resolved before scan"),
            };
            if is_match {
                matches.push(p);
            }
        }

        match matches.len() {
            1 => return Ok(matches.into_iter().next().unwrap()),
            n if n > 1 => {
                // Only `Any` can be ambiguous; name/address are specific enough.
                let addrs = matches
                    .iter()
                    .map(|p| p.address().to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(AttentioError::Ble(format!(
                    "multiple BLE devices found ({addrs}) — specify --ble <name|address>"
                )));
            }
            _ => {}
        }

        if Instant::now() >= deadline {
            return Err(AttentioError::BleNotFound {
                selector: sel.to_string(),
            });
        }
        tokio::time::sleep(SCAN_POLL).await;
    }
}

/// Resolve a `--ble=<N>` index against the unified `attentio list` ordering.
///
/// `find_all_devices` lists USB devices first, then BLE — the same order and
/// 1-based numbering shown by `attentio list`, so the user can copy the `#`.
async fn resolve_index(n: usize) -> Result<BleSelector, AttentioError> {
    use crate::device::discovery::{find_all_devices, Transport};

    let devices = find_all_devices().await?;
    if n == 0 || n > devices.len() {
        return Err(AttentioError::BleNotFound {
            selector: format!("index {n}"),
        });
    }
    let dev = &devices[n - 1];
    match (dev.transport, dev.ble_address.as_deref()) {
        (Transport::Ble, Some(addr)) => Ok(BleSelector::Address(addr.to_string())),
        (Transport::Ble, None) => Err(AttentioError::Ble(format!(
            "device #{n} has no BLE address"
        ))),
        (Transport::Usb, _) => Err(AttentioError::Ble(format!(
            "device #{n} is a USB device — omit --ble, or use --device {n}"
        ))),
    }
}

/// Ensure the device is bonded before any encryption-required write.
///
/// On Linux, btleplug 0.11 exposes no `pair()`, so we bond via `bluetoothctl`.
/// If the device is already bonded this is a fast no-op (the link re-encrypts
/// from stored keys with no agent). If not, we attempt a Just-Works auto-pair
/// and, failing that, return an actionable error telling the user to pair once
/// manually — we never fall through to a write that would hang for 25 s.
#[cfg(target_os = "linux")]
async fn ensure_bonded(address: &str) -> Result<(), AttentioError> {
    if is_paired(address).await {
        debug!("BLE device {address} already bonded");
        return Ok(());
    }

    info!("BLE device {address} not bonded — attempting to pair");
    auto_pair(address).await;

    if is_paired(address).await {
        info!("BLE device {address} paired");
        return Ok(());
    }

    Err(AttentioError::BlePairing(format!(
        "device {address} is not paired. Pair it once, then retry:\n  \
         bluetoothctl\n    pair {address}\n    quit\n  \
         (Do NOT 'trust' it — a trusted device is auto-connected by BlueZ and \
         the single-session firmware will refuse our connection.)"
    )))
}

#[cfg(not(target_os = "linux"))]
async fn ensure_bonded(_address: &str) -> Result<(), AttentioError> {
    // macOS/Windows pair via the OS agent on the first encrypted GATT operation.
    Ok(())
}

/// BlueZ pairing status for a device address.
///
/// `Some(true|false)` from `bluetoothctl info <addr>` on Linux; `None` when the
/// status can't be determined (bluetoothctl missing, or a non-Linux host where
/// the OS manages bonding).
#[cfg(target_os = "linux")]
pub async fn paired_status(address: &str) -> Option<bool> {
    use tokio::process::Command;

    match Command::new("bluetoothctl")
        .arg("info")
        .arg(address)
        .output()
        .await
    {
        Ok(out) => Some(String::from_utf8_lossy(&out.stdout).contains("Paired: yes")),
        Err(_) => None,
    }
}

#[cfg(not(target_os = "linux"))]
pub async fn paired_status(_address: &str) -> Option<bool> {
    None
}

/// True if BlueZ reports the device as paired.
#[cfg(target_os = "linux")]
async fn is_paired(address: &str) -> bool {
    paired_status(address).await == Some(true)
}

/// Drop the host's stored bond (LTK) for a device via `bluetoothctl remove`.
///
/// Best-effort and bounded: logs and continues on any failure, because the
/// caller re-pairs immediately afterward. Removing the host key forces a fresh
/// pairing on the next connect, which lets the firmware's `REPEAT_PAIRING`
/// handler clear its (stale/mismatched) side of the bond too.
#[cfg(target_os = "linux")]
async fn remove_bond(address: &str) {
    use tokio::process::Command;

    match Command::new("bluetoothctl")
        .arg("remove")
        .arg(address)
        .output()
        .await
    {
        Ok(out) if out.status.success() => debug!("BLE removed host bond for {address}"),
        Ok(out) => warn!(
            "bluetoothctl remove {address} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ),
        Err(e) => warn!("bluetoothctl unavailable ({e}); could not remove bond for {address}"),
    }
}

#[cfg(not(target_os = "linux"))]
async fn remove_bond(_address: &str) {}

/// Drive `bluetoothctl` non-interactively to Just-Works pair the device.
///
/// Registers a default agent so Just-Works completes without a prompt, then
/// submits `pair`. Bounded so it can never hang. We deliberately do **not**
/// `trust` the device: BlueZ auto-connects trusted devices in the background,
/// and the single-session firmware then refuses our explicit connection (the
/// `Device1.Connect` call stalls until the 30 s D-Bus timeout). Bonding/
/// encryption persist without trust.
#[cfg(target_os = "linux")]
async fn auto_pair(address: &str) {
    use std::process::Stdio;
    use tokio::io::AsyncWriteExt;
    use tokio::process::Command;

    let mut child = match Command::new("bluetoothctl")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            warn!("bluetoothctl unavailable ({e}); pair the device manually");
            return;
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        let setup = format!("power on\nagent on\ndefault-agent\npair {address}\n");
        let _ = stdin.write_all(setup.as_bytes()).await;
        let _ = stdin.flush().await;
        // Give Just-Works pairing time to complete before quitting.
        tokio::time::sleep(Duration::from_secs(8)).await;
        let _ = stdin.write_all(b"quit\n").await;
        let _ = stdin.flush().await;
        drop(stdin);
    }

    let _ = tokio::time::timeout(Duration::from_secs(5), child.wait()).await;
    let _ = child.start_kill();
}
