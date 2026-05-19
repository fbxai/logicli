use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use hidapi::{DeviceInfo, HidApi, HidDevice};
use std::time::{Duration, Instant};

const LOGITECH_VID: u16 = 0x046D;

const RECEIVER_PIDS: &[(u16, &str)] = &[
    (0xC52B, "Unifying Receiver"),
    (0xC52F, "Nano Receiver"),
    (0xC534, "Nano Receiver"),
    (0xC539, "Lightspeed Receiver"),
    (0xC53A, "Powerplay Receiver"),
    (0xC53F, "Lightspeed Receiver"),
    (0xC541, "Lightspeed Receiver"),
    (0xC547, "Lightspeed Receiver"),
    (0xC548, "Bolt Receiver"),
];

// HID++ report IDs
const HIDPP_SHORT: u8 = 0x10;
const HIDPP_LONG: u8 = 0x11;

// Device index for receiver itself
const HIDPP_RECEIVER: u8 = 0xFF;

// Register access sub-IDs
const REG_READ_SHORT: u8 = 0x81;
const REG_WRITE_SHORT: u8 = 0x80;
const REG_WRITE_LONG: u8 = 0x82;
const REG_READ_LONG: u8 = 0x83;

// Registers
const REG_NOTIFICATIONS: u8 = 0x00;
#[allow(dead_code)]
const REG_CONNECTION_STATE: u8 = 0x02;
const REG_PAIRING: u8 = 0xB2;
const REG_PAIRING_INFO: u8 = 0xB5;
const REG_BOLT_DISCOVERY: u8 = 0xC0;
const REG_BOLT_PAIRING: u8 = 0xC1;

// Notification sub-IDs
const NOTIF_DEVICE_CONNECT: u8 = 0x41;
const NOTIF_PAIRING_LOCK: u8 = 0x4A;
const NOTIF_BOLT_PASSKEY_REQUEST: u8 = 0x4D;
const NOTIF_BOLT_PASSKEY_PRESSED: u8 = 0x4E;
const NOTIF_BOLT_DEVICE_DISCOVERY: u8 = 0x4F;
const NOTIF_BOLT_DISCOVERY_STATUS: u8 = 0x53;
const NOTIF_BOLT_PAIRING_STATUS: u8 = 0x54;

// Pairing actions (Unifying)
const PAIR_ACTION_OPEN: u8 = 0x01;
const PAIR_ACTION_CLOSE: u8 = 0x02;
const PAIR_ACTION_UNPAIR: u8 = 0x03;

// Bolt pairing actions
const BOLT_PAIR_ACTION_PAIR: u8 = 0x01;
const BOLT_PAIR_ACTION_CANCEL: u8 = 0x02;
const BOLT_PAIR_ACTION_UNPAIR: u8 = 0x03;

const READ_TIMEOUT_MS: i32 = 500;

fn receiver_name(pid: u16) -> Option<&'static str> {
    RECEIVER_PIDS
        .iter()
        .find(|(p, _)| *p == pid)
        .map(|(_, name)| *name)
}

fn is_bolt(pid: u16) -> bool {
    pid == 0xC548
}

#[derive(Parser, Debug)]
#[command(
    name = "logicli",
    version,
    about = "List, pair, and unpair Logitech USB receivers (Unifying, Nano, Bolt) via HID++"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// List connected Logitech receivers and their paired devices
    List,
    /// Pair a new device (discovers devices and pairs them)
    Pair {
        /// HID path of the receiver; omit to pick the first one found
        #[arg(long)]
        path: Option<String>,
        /// Discovery/pairing window timeout in seconds
        #[arg(long, default_value_t = 30)]
        timeout: u8,
        /// Use reduced entropy for mouse pairing (4 button presses instead of 10)
        #[arg(long)]
        short_auth: bool,
    },
    /// Unpair a device from a slot (1-6)
    Unpair {
        /// Slot number to remove
        slot: u8,
        /// HID path of the receiver; omit to pick the first one found
        #[arg(long)]
        path: Option<String>,
    },
    /// Show version, commit SHA, and build info
    Version,
    /// Debug: dump raw register reads from the receiver
    Debug {
        /// HID path of the receiver; omit to pick the first one found
        #[arg(long)]
        path: Option<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::List => {
            let api = HidApi::new().context("initialising hidapi")?;
            cmd_list(&api)
        }
        Command::Pair {
            path,
            timeout,
            short_auth,
        } => {
            let api = HidApi::new().context("initialising hidapi")?;
            cmd_pair(&api, path.as_deref(), timeout, short_auth)
        }
        Command::Unpair { slot, path } => {
            let api = HidApi::new().context("initialising hidapi")?;
            cmd_unpair(&api, slot, path.as_deref())
        }
        Command::Version => {
            cmd_version();
            Ok(())
        }
        Command::Debug { path } => {
            let api = HidApi::new().context("initialising hidapi")?;
            cmd_debug(&api, path.as_deref())
        }
    }
}

fn cmd_version() {
    let version = option_env!("LOGICLI_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"));
    let commit = option_env!("LOGICLI_COMMIT").unwrap_or("unknown");
    let build_date = option_env!("LOGICLI_BUILD_DATE").unwrap_or("unknown");
    println!("logicli {version}");
    println!("commit:  {commit}");
    println!("built:   {build_date}");
}

// ─── HID interface discovery ────────────────────────────────────────────────

fn hidpp_interfaces(api: &HidApi) -> Vec<&DeviceInfo> {
    let mut seen = std::collections::HashSet::new();
    api.device_list()
        .filter(|d| d.vendor_id() == LOGITECH_VID)
        .filter(|d| receiver_name(d.product_id()).is_some())
        .filter(|d| d.usage_page() == 0xFF00)
        .filter(|d| seen.insert(d.path().to_owned()))
        .collect()
}

fn pick_receiver<'a>(api: &'a HidApi, path: Option<&str>) -> Result<&'a DeviceInfo> {
    let mut candidates = hidpp_interfaces(api);
    if candidates.is_empty() {
        bail!("no Logitech receiver HID++ interface found");
    }
    if let Some(p) = path {
        candidates
            .into_iter()
            .find(|d| d.path().to_string_lossy() == p)
            .with_context(|| format!("no receiver with HID path {p}"))
    } else {
        Ok(candidates.remove(0))
    }
}

// ─── Low-level HID++ I/O ────────────────────────────────────────────────────

/// Write a short (7-byte) HID++ message. Returns the first matching response.
fn hidpp_write_short(dev: &HidDevice, req: &[u8; 7]) -> Result<Vec<u8>> {
    dev.write(req).context("writing HID++ short request")?;
    let want_sub = req[2];
    let want_reg = req[3];
    let deadline = Instant::now() + Duration::from_millis(2000);
    let mut buf = [0u8; 64];
    while Instant::now() < deadline {
        let n = match dev.read_timeout(&mut buf, READ_TIMEOUT_MS) {
            Ok(0) => continue,
            Ok(n) => n,
            Err(e) => return Err(e).context("reading HID++ response"),
        };
        let resp = buf[..n].to_vec();
        // Error reply: [0x10, dev, 0x8F, sub_id_echo, reg_echo, error_code, 0x00]
        if resp.len() >= 6 && resp[2] == 0x8F && resp[3] == want_sub && resp[4] == want_reg {
            return Ok(resp);
        }
        // Success reply matches sub_id and register
        if resp.len() >= 4 && resp[2] == want_sub && resp[3] == want_reg {
            return Ok(resp);
        }
    }
    bail!(
        "timed out waiting for HID++ short response (sub=0x{:02X} reg=0x{:02X})",
        want_sub,
        want_reg
    );
}

/// Write a long (20-byte) HID++ message. Returns the first matching response.
fn hidpp_write_long(dev: &HidDevice, req: &[u8; 20]) -> Result<Vec<u8>> {
    dev.write(req).context("writing HID++ long request")?;
    let want_sub = req[2];
    let want_reg = req[3];
    let deadline = Instant::now() + Duration::from_millis(2000);
    let mut buf = [0u8; 64];
    while Instant::now() < deadline {
        let n = match dev.read_timeout(&mut buf, READ_TIMEOUT_MS) {
            Ok(0) => continue,
            Ok(n) => n,
            Err(e) => return Err(e).context("reading HID++ response"),
        };
        let resp = buf[..n].to_vec();
        // Error reply
        if resp.len() >= 6 && resp[2] == 0x8F && resp[3] == want_sub && resp[4] == want_reg {
            return Ok(resp);
        }
        // Success
        if resp.len() >= 4 && resp[2] == want_sub && resp[3] == want_reg {
            return Ok(resp);
        }
    }
    bail!(
        "timed out waiting for HID++ long response (sub=0x{:02X} reg=0x{:02X})",
        want_sub,
        want_reg
    );
}

/// Read one raw HID report with timeout. Returns None on timeout/zero-read.
fn hidpp_read_raw(dev: &HidDevice, timeout_ms: i32) -> Result<Option<Vec<u8>>> {
    let mut buf = [0u8; 64];
    let n = dev
        .read_timeout(&mut buf, timeout_ms)
        .context("reading HID")?;
    if n == 0 {
        Ok(None)
    } else {
        Ok(Some(buf[..n].to_vec()))
    }
}

fn is_error(resp: &[u8]) -> bool {
    resp.len() >= 3 && resp[2] == 0x8F
}

fn error_code_name(code: u8) -> &'static str {
    match code {
        0x01 => "INVALID_SUB_ID",
        0x02 => "INVALID_ADDRESS",
        0x03 => "INVALID_VALUE",
        0x04 => "CONNECTION_FAILED",
        0x05 => "TOO_MANY_DEVICES",
        0x06 => "ALREADY_EXISTS",
        0x07 => "BUSY",
        0x08 => "UNKNOWN_DEVICE",
        0x09 => "RESOURCE_ERROR",
        0x0A => "REQUEST_UNAVAILABLE",
        0x0B => "UNSUPPORTED_PARAM",
        0x0C => "WRONG_PIN_CODE",
        _ => "UNKNOWN",
    }
}

fn format_resp(resp: &[u8]) -> String {
    resp.iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

// ─── Register helpers ───────────────────────────────────────────────────────

fn read_notification_flags(dev: &HidDevice) -> Result<[u8; 3]> {
    let req: [u8; 7] = [
        HIDPP_SHORT,
        HIDPP_RECEIVER,
        REG_READ_SHORT,
        REG_NOTIFICATIONS,
        0x00,
        0x00,
        0x00,
    ];
    let resp = hidpp_write_short(dev, &req)?;
    if is_error(&resp) {
        bail!("failed to read notification flags: {}", format_resp(&resp));
    }
    Ok([resp[4], resp[5], resp[6]])
}

fn write_notification_flags(dev: &HidDevice, flags: [u8; 3]) -> Result<()> {
    let req: [u8; 7] = [
        HIDPP_SHORT,
        HIDPP_RECEIVER,
        REG_WRITE_SHORT,
        REG_NOTIFICATIONS,
        flags[0],
        flags[1],
        flags[2],
    ];
    let resp = hidpp_write_short(dev, &req)?;
    if is_error(&resp) {
        bail!("failed to write notification flags: {}", format_resp(&resp));
    }
    Ok(())
}

/// Enable wireless + software_present notifications; return previous flags
fn enable_notifications(dev: &HidDevice) -> Result<[u8; 3]> {
    let old_flags = read_notification_flags(dev)?;
    let mut new_flags = old_flags;
    new_flags[1] |= 0x01; // WIRELESS
    new_flags[1] |= 0x08; // SOFTWARE_PRESENT
    if new_flags != old_flags {
        write_notification_flags(dev, new_flags)?;
    }
    Ok(old_flags)
}

fn describe_device_type(t: u8) -> &'static str {
    match t {
        0x01 => "keyboard",
        0x02 => "mouse",
        0x03 => "numpad",
        0x04 => "presenter",
        0x07 => "remote",
        0x08 => "trackball",
        0x09 => "touchpad",
        _ => "other",
    }
}

// ─── Unifying device info ───────────────────────────────────────────────────

fn unifying_read_device_name(dev: &HidDevice, slot: u8) -> Result<String> {
    let sub_reg = 0x40 + (slot - 1);
    let mut req = [0u8; 20];
    req[0] = HIDPP_LONG;
    req[1] = HIDPP_RECEIVER;
    req[2] = REG_READ_LONG;
    req[3] = REG_PAIRING_INFO;
    req[4] = sub_reg;
    let resp = hidpp_write_long(dev, &req)?;
    if is_error(&resp) {
        return Ok("<unknown>".to_string());
    }
    if resp.len() >= 7 {
        let name_len = resp[5] as usize;
        let end = std::cmp::min(6 + name_len, resp.len());
        Ok(String::from_utf8_lossy(&resp[6..end]).to_string())
    } else {
        Ok("<unknown>".to_string())
    }
}

fn unifying_read_pairing_info(dev: &HidDevice, slot: u8) -> Result<Option<(u8, String)>> {
    let sub_reg = 0x20 + (slot - 1);
    let mut req = [0u8; 20];
    req[0] = HIDPP_LONG;
    req[1] = HIDPP_RECEIVER;
    req[2] = REG_READ_LONG;
    req[3] = REG_PAIRING_INFO;
    req[4] = sub_reg;
    let resp = hidpp_write_long(dev, &req)?;
    if is_error(&resp) {
        return Ok(None);
    }
    if resp.len() >= 12 {
        let device_type = resp[11] & 0x0F;
        let wpid = format!("{:02X}{:02X}", resp[8], resp[7]);
        Ok(Some((device_type, wpid)))
    } else {
        Ok(None)
    }
}

// ─── Bolt device info ───────────────────────────────────────────────────────

fn bolt_read_device_name(dev: &HidDevice, slot: u8) -> Result<String> {
    // Bolt uses GET_LONG_REGISTER (0x83) on register 0xB5 with sub-register 0x60+slot
    // IMPORTANT: The request is sent as a SHORT message (7 bytes), the response is LONG (20 bytes)
    let sub_reg = 0x60 + slot;
    let req: [u8; 7] = [
        HIDPP_SHORT,
        HIDPP_RECEIVER,
        REG_READ_LONG,
        REG_PAIRING_INFO,
        sub_reg,
        0x01,
        0x00,
    ];
    let resp = hidpp_write_short(dev, &req)?;
    if is_error(&resp) {
        bail!("device name register not supported");
    }
    // Response: [0x11, 0xFF, 0x83, 0xB5, sub_reg, ...name_data...]
    // In Solaar: codename = resp[3 : 3 + min(14, resp[2])]  (offset from data start at byte 4)
    // So: name_len = resp[4+2] = resp[6], name starts at resp[4+3] = resp[7]
    if resp.len() >= 8 {
        let name_len = std::cmp::min(resp[6] as usize, 14);
        let end = std::cmp::min(7 + name_len, resp.len());
        let name = String::from_utf8_lossy(&resp[7..end]).trim().to_string();
        if name.is_empty() {
            bail!("empty device name");
        }
        Ok(name)
    } else {
        bail!("response too short for device name");
    }
}

fn bolt_read_pairing_info(dev: &HidDevice, slot: u8) -> Result<Option<(u8, String)>> {
    // Bolt uses GET_LONG_REGISTER (0x83) on register 0xB5 with sub-register 0x50+slot
    // IMPORTANT: The request is sent as a SHORT message (7 bytes), the response is LONG (20 bytes)
    let sub_reg = 0x50 + slot;
    let req: [u8; 7] = [
        HIDPP_SHORT,
        HIDPP_RECEIVER,
        REG_READ_LONG,
        REG_PAIRING_INFO,
        sub_reg,
        0x00,
        0x00,
    ];
    let resp = hidpp_write_short(dev, &req)?;
    if is_error(&resp) {
        return Ok(None);
    }
    // Response: [0x11, 0xFF, 0x83, 0xB5, sub_reg, device_type_byte, wpid_lo, wpid_hi, serial...]
    // In Solaar: kind = resp[1] & 0x0F, wpid = resp[3:4]+resp[2:3]
    // Adjusting offsets for raw response: data starts at byte 4 (after 0x11, 0xFF, 0x83, 0xB5)
    // So: device_type = resp[5] & 0x0F, wpid = format(resp[7], resp[6])
    if resp.len() >= 8 {
        let device_type = resp[5] & 0x0F;
        let wpid = format!("{:02X}{:02X}", resp[7], resp[6]);
        Ok(Some((device_type, wpid)))
    } else {
        Ok(None)
    }
}

// ─── cmd_list ───────────────────────────────────────────────────────────────

fn cmd_list(api: &HidApi) -> Result<()> {
    let receivers = hidpp_interfaces(api);
    if receivers.is_empty() {
        println!("No Logitech receivers found.");
        let others: Vec<_> = api
            .device_list()
            .filter(|d| d.vendor_id() == LOGITECH_VID)
            .collect();
        if !others.is_empty() {
            println!("\nOther Logitech HID interfaces present:");
            for d in others {
                println!(
                    "  PID 0x{:04X}  {}  ({})",
                    d.product_id(),
                    d.product_string().unwrap_or(""),
                    d.path().to_string_lossy()
                );
            }
        }
        return Ok(());
    }

    for info in receivers {
        let name = receiver_name(info.product_id()).unwrap_or("Unknown");
        let pid = info.product_id();
        println!(
            "Receiver: {name}  (VID 0x{:04X}  PID 0x{pid:04X})",
            info.vendor_id()
        );
        println!("  Path: {}", info.path().to_string_lossy());
        if let Some(s) = info.product_string() {
            println!("  Product: {s}");
        }

        match info.open_device(api) {
            Ok(dev) => {
                let _ = enable_notifications(&dev);
                if is_bolt(pid) {
                    list_bolt_devices(&dev);
                } else {
                    list_unifying_devices(&dev);
                }
            }
            Err(e) => {
                println!("  <cannot open: {e}>");
                println!(
                    "  (on macOS, allow this terminal under System Settings →\n   Privacy & Security → Input Monitoring)"
                );
            }
        }
        println!();
    }
    Ok(())
}

fn list_unifying_devices(dev: &HidDevice) {
    let mut found = false;
    for slot in 1u8..=6 {
        if let Ok(Some((dtype, wpid))) = unifying_read_pairing_info(dev, slot) {
            let name = unifying_read_device_name(dev, slot).unwrap_or_else(|_| "?".into());
            if !found {
                println!("  Paired devices:");
                found = true;
            }
            println!(
                "    slot {slot}: {} \"{name}\" (WPID {wpid})",
                describe_device_type(dtype)
            );
        }
    }
    if !found {
        println!("  Paired devices: (none)");
    }
}

fn list_bolt_devices(dev: &HidDevice) {
    // Read device count from register 0x02
    let req: [u8; 7] = [
        HIDPP_SHORT,
        HIDPP_RECEIVER,
        REG_READ_SHORT,
        0x02,
        0x00,
        0x00,
        0x00,
    ];
    let count = match hidpp_write_short(dev, &req) {
        Ok(resp) if !is_error(&resp) && resp.len() >= 6 => resp[5],
        _ => {
            println!("  Paired devices: (unknown - could not query)");
            return;
        }
    };

    if count == 0 {
        println!("  Paired devices: (none)");
        return;
    }

    println!("  Paired devices ({count}):");
    for slot in 1..=count {
        let name = bolt_read_device_name(dev, slot).unwrap_or_else(|_| "<unknown>".to_string());
        let type_str = match bolt_read_pairing_info(dev, slot) {
            Ok(Some((kind, wpid))) => {
                format!("{} [WPID {}]", describe_device_type(kind), wpid)
            }
            _ => "device".to_string(),
        };
        println!("    Slot {slot}: {name} ({type_str})");
    }
}

// ─── cmd_pair ───────────────────────────────────────────────────────────────

fn cmd_pair(api: &HidApi, path: Option<&str>, timeout: u8, short_auth: bool) -> Result<()> {
    let info = pick_receiver(api, path)?;
    let pid = info.product_id();
    let rname = receiver_name(pid).unwrap_or("receiver");

    let dev = info
        .open_device(api)
        .context("opening receiver HID interface")?;

    println!("=== Pairing on {rname} (PID 0x{pid:04X}) ===\n");

    if is_bolt(pid) {
        cmd_pair_bolt(&dev, timeout, short_auth)
    } else {
        cmd_pair_unifying(&dev, pid, timeout)
    }
}

fn cmd_pair_unifying(dev: &HidDevice, _pid: u16, timeout: u8) -> Result<()> {
    // Step 1: Enable notifications
    println!("[1/4] Enabling wireless notifications...");
    let old_flags = enable_notifications(dev).context("enabling notifications")?;
    println!("      Done.");

    // Step 2: Open pairing lock
    println!("[2/4] Opening pairing lock (timeout: {timeout}s)...");
    let req: [u8; 7] = [
        HIDPP_SHORT,
        HIDPP_RECEIVER,
        REG_WRITE_SHORT,
        REG_PAIRING,
        PAIR_ACTION_OPEN,
        0x00,
        timeout,
    ];
    let resp = hidpp_write_short(dev, &req)?;
    if is_error(&resp) {
        let ecode = if resp.len() >= 6 { resp[5] } else { 0 };
        let _ = write_notification_flags(dev, old_flags);
        bail!(
            "Receiver rejected open-pairing-lock: error 0x{ecode:02X} ({})\n  Response: {}",
            error_code_name(ecode),
            format_resp(&resp)
        );
    }
    println!("      Pairing lock OPEN.");
    println!("\n>>> Power-cycle your device now (turn OFF then ON) <<<\n");

    // Step 3: Wait for device notification
    println!("[3/4] Waiting for device...");
    let pair_deadline = Instant::now() + Duration::from_secs(timeout as u64 + 5);
    let mut paired_slot: Option<u8> = None;
    let mut lock_open = true;

    while Instant::now() < pair_deadline && lock_open {
        let msg = match hidpp_read_raw(dev, 1000)? {
            Some(m) => m,
            None => continue,
        };
        if msg.len() < 5 {
            continue;
        }

        let device_idx = msg[1];
        let sub_id = msg[2];

        if sub_id == NOTIF_PAIRING_LOCK && device_idx == HIDPP_RECEIVER {
            let lock_status = msg[3] & 0x01;
            let pair_error = msg[4];
            if lock_status == 0 {
                lock_open = false;
                if pair_error != 0 {
                    let err_msg = match pair_error {
                        0x01 => "device timeout",
                        0x02 => "device not supported",
                        0x03 => "too many devices",
                        0x06 => "sequence timeout",
                        _ => "unknown error",
                    };
                    println!("      Pairing FAILED: {err_msg} (0x{pair_error:02X})");
                } else {
                    println!("      Pairing lock closed (success).");
                }
            }
            continue;
        }

        if sub_id == NOTIF_DEVICE_CONNECT {
            let slot = device_idx;
            let device_kind = msg[4] & 0x0F;
            let wpid = if msg.len() >= 7 {
                format!("{:02X}{:02X}", msg[6], msg[5])
            } else {
                "????".into()
            };
            println!(
                "      DEVICE CONNECTED: slot {slot}, {} (WPID {wpid})",
                describe_device_type(device_kind)
            );
            paired_slot = Some(slot);
            std::thread::sleep(Duration::from_millis(100));
            if let Ok(name) = unifying_read_device_name(dev, slot) {
                println!("      Name: \"{name}\"");
            }
        }
    }

    // Step 4: Report
    println!("\n[4/4] Result:");
    if let Some(slot) = paired_slot {
        let name = unifying_read_device_name(dev, slot).unwrap_or_else(|_| "?".into());
        let type_str = unifying_read_pairing_info(dev, slot)
            .ok()
            .flatten()
            .map(|(t, _)| describe_device_type(t))
            .unwrap_or("device");
        println!("      SUCCESS! Paired {type_str} \"{name}\" in slot {slot}.");
    } else if !lock_open {
        println!("      Pairing ended without detecting a new device.");
    } else {
        println!("      TIMEOUT - no device connected.");
        let close_req: [u8; 7] = [
            HIDPP_SHORT,
            HIDPP_RECEIVER,
            REG_WRITE_SHORT,
            REG_PAIRING,
            PAIR_ACTION_CLOSE,
            0x00,
            0x00,
        ];
        let _ = hidpp_write_short(dev, &close_req);
    }

    let _ = write_notification_flags(dev, old_flags);
    Ok(())
}

// ─── Bolt pairing ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct BoltDiscoveredDevice {
    counter: u16,
    device_kind: u8,
    address: [u8; 6],
    authentication: u8,
    name: String,
}

fn cmd_pair_bolt(dev: &HidDevice, timeout: u8, short_auth: bool) -> Result<()> {
    // Step 1: Enable notifications
    println!("[1/5] Enabling notifications...");
    let old_flags = enable_notifications(dev).context("enabling notifications")?;
    println!("      Done.");

    // Step 2: Start Bolt discovery
    println!("[2/5] Starting Bolt device discovery (timeout: {timeout}s)...");
    let req: [u8; 7] = [
        HIDPP_SHORT,
        HIDPP_RECEIVER,
        REG_WRITE_SHORT,
        REG_BOLT_DISCOVERY,
        timeout,
        0x01,
        0x00,
    ];
    let resp = hidpp_write_short(dev, &req)?;
    if is_error(&resp) {
        let ecode = if resp.len() >= 6 { resp[5] } else { 0 };
        let _ = write_notification_flags(dev, old_flags);
        bail!(
            "Receiver rejected Bolt discovery start: error 0x{ecode:02X} ({})\n  Response: {}",
            error_code_name(ecode),
            format_resp(&resp)
        );
    }
    println!("      Discovery started. Turn on your device or press its Connect button.\n");

    // Step 3: Collect discovered devices (deduplicate by BT address)
    println!("[3/5] Scanning for nearby devices...");
    let mut discovered: Vec<BoltDiscoveredDevice> = Vec::new();
    let disc_deadline = Instant::now() + Duration::from_secs(timeout as u64 + 5);
    let mut discovery_done = false;
    // We'll stop discovery early once we have at least one fully-identified device
    // and a short grace period has passed to catch additional devices
    let mut first_device_time: Option<Instant> = None;
    let grace_period = Duration::from_secs(5); // wait 5s after first device for others

    while Instant::now() < disc_deadline && !discovery_done {
        // If we have a complete device and grace period elapsed, stop early
        if let Some(ft) = first_device_time
            && Instant::now() > ft + grace_period
        {
            println!("      Discovery grace period ended.");
            // Cancel discovery
            let cancel_req: [u8; 7] = [
                HIDPP_SHORT,
                HIDPP_RECEIVER,
                REG_WRITE_SHORT,
                REG_BOLT_DISCOVERY,
                0x00,
                0x02,
                0x00,
            ];
            let _ = hidpp_write_short(dev, &cancel_req);
            break;
        }

        let msg = match hidpp_read_raw(dev, 1000)? {
            Some(m) => m,
            None => continue,
        };
        if msg.len() < 5 {
            continue;
        }
        let sub_id = msg[2];

        // Discovery notification (0x4F) - long message
        if sub_id == NOTIF_BOLT_DEVICE_DISCOVERY && msg.len() >= 16 {
            let counter = msg[3] as u16 + (msg[4] as u16) * 256;
            let part = msg[5];

            if part == 0 && msg.len() >= 18 {
                // Part 0: device info
                let device_kind = msg[7];
                let mut address = [0u8; 6];
                address.copy_from_slice(&msg[10..16]);
                let auth_byte = if msg.len() > 18 { msg[18] } else { 0 };

                // Deduplicate by address
                let existing = discovered.iter_mut().find(|d| d.address == address);
                if let Some(d) = existing {
                    d.device_kind = device_kind;
                    d.authentication = auth_byte;
                    d.counter = counter;
                } else {
                    discovered.push(BoltDiscoveredDevice {
                        counter,
                        device_kind,
                        address,
                        authentication: auth_byte,
                        name: String::new(),
                    });
                }
            } else if part == 1 {
                // Part 1: device name
                let name_len = msg[6] as usize;
                let end = std::cmp::min(7 + name_len, msg.len());
                let name = String::from_utf8_lossy(&msg[7..end]).to_string();

                // Find device by counter to associate name
                if let Some(idx) = discovered.iter().position(|d| d.counter == counter)
                    && discovered[idx].name.is_empty()
                {
                    discovered[idx].name = name.clone();
                    // Print on first name discovery only
                    let d = &discovered[idx];
                    println!(
                        "      [{}] {} \"{}\" (addr {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X})",
                        idx + 1,
                        describe_device_type(d.device_kind),
                        d.name,
                        d.address[0],
                        d.address[1],
                        d.address[2],
                        d.address[3],
                        d.address[4],
                        d.address[5],
                    );
                    // Start grace timer on first complete device
                    if first_device_time.is_none() {
                        first_device_time = Some(Instant::now());
                    }
                }
            }
            continue;
        }

        // Discovery status notification (0x53)
        if sub_id == NOTIF_BOLT_DISCOVERY_STATUS {
            let status = msg[3];
            if status != 0 {
                discovery_done = true;
                let error = msg[4];
                if error != 0 && discovered.is_empty() {
                    println!("      Discovery ended (error 0x{error:02X}: device timeout)");
                } else {
                    println!("      Discovery complete.");
                }
            }
            continue;
        }
    }

    if discovered.is_empty() {
        let _ = write_notification_flags(dev, old_flags);
        bail!("No devices discovered. Make sure the device is in pairing mode.");
    }

    // Auto-select: if only one device, pick it; otherwise pick the first one
    let target = if discovered.len() == 1 {
        println!(
            "\n      Auto-selecting: {} \"{}\"",
            describe_device_type(discovered[0].device_kind),
            discovered[0].name
        );
        discovered[0].clone()
    } else {
        println!(
            "\n      Found {} devices. Selecting the first one.",
            discovered.len()
        );
        for (i, d) in discovered.iter().enumerate() {
            println!(
                "        [{}] {} \"{}\"",
                i + 1,
                describe_device_type(d.device_kind),
                d.name
            );
        }
        discovered[0].clone()
    };

    println!(
        "\n[4/5] Pairing with {} \"{}\"...",
        describe_device_type(target.device_kind),
        target.name
    );

    // Send pair command via long register 0xC1
    // [0x11, 0xFF, 0x82, 0xC1, action, slot, address[6], auth, entropy, ...]
    // Entropy = bits of passkey randomness:
    //   Keyboards: 20 bits (6 digit passkey typed on the device)
    //   Mice default: 10 bits (10 left/right button presses)
    //   Mice --short-auth: 4 bits (4 button presses — faster, less secure)
    let entropy: u8 = if target.device_kind == 0x01 {
        20
    } else if short_auth {
        4
    } else {
        10
    };
    let mut pair_req = [0u8; 20];
    pair_req[0] = HIDPP_LONG;
    pair_req[1] = HIDPP_RECEIVER;
    pair_req[2] = REG_WRITE_LONG;
    pair_req[3] = REG_BOLT_PAIRING;
    pair_req[4] = BOLT_PAIR_ACTION_PAIR;
    pair_req[5] = 0x00; // slot = auto-assign
    pair_req[6..12].copy_from_slice(&target.address);
    pair_req[12] = target.authentication;
    pair_req[13] = entropy;

    let resp = hidpp_write_long(dev, &pair_req)?;
    if is_error(&resp) {
        let ecode = if resp.len() >= 6 { resp[5] } else { 0 };
        let _ = write_notification_flags(dev, old_flags);
        bail!(
            "Receiver rejected Bolt pair command: error 0x{ecode:02X} ({})\n  Response: {}",
            error_code_name(ecode),
            format_resp(&resp)
        );
    }
    println!("      Pair command accepted. Waiting for authentication...");

    // Step 5: Wait for pairing status / passkey
    let pair_deadline = Instant::now() + Duration::from_secs(60);
    let mut paired_slot: Option<u8> = None;
    let mut passkey_shown = false;

    while Instant::now() < pair_deadline {
        let msg = match hidpp_read_raw(dev, 1000)? {
            Some(m) => m,
            None => continue,
        };
        if msg.len() < 5 {
            continue;
        }
        let sub_id = msg[2];

        // Passkey request (0x4D)
        if sub_id == NOTIF_BOLT_PASSKEY_REQUEST {
            let passkey_end = std::cmp::min(10, msg.len());
            let passkey = String::from_utf8_lossy(&msg[4..passkey_end])
                .trim_end_matches('\0')
                .to_string();
            if !passkey.is_empty() && !passkey_shown {
                println!("\n      ╔══════════════════════════════════════╗");
                if target.authentication & 0x01 != 0 {
                    // Keyboard-style: type digits + Enter
                    println!("      ║  PASSKEY: {passkey:<26}  ║");
                    println!("      ╚══════════════════════════════════════╝");
                    println!("      Type this passkey on the keyboard, then press ENTER.\n");
                } else {
                    // Mouse-style: left/right button presses based on binary passkey
                    let passkey_num: u32 = passkey.parse().unwrap_or(0);
                    let bits: String = (0..entropy)
                        .rev()
                        .map(|i| {
                            if passkey_num & (1 << i) != 0 {
                                '1'
                            } else {
                                '0'
                            }
                        })
                        .collect();
                    let sequence: Vec<&str> = bits
                        .chars()
                        .map(|b| if b == '1' { "RIGHT" } else { "LEFT" })
                        .collect();
                    println!("      ║  BUTTON SEQUENCE:                    ║");
                    println!("      ╚══════════════════════════════════════╝");
                    println!("      Press buttons in this order:");
                    for (i, btn) in sequence.iter().enumerate() {
                        println!("        {:2}. {btn}", i + 1);
                    }
                    println!("      Then press LEFT + RIGHT simultaneously.\n");
                }
                use std::io::Write;
                let _ = std::io::stdout().flush();
                passkey_shown = true;
            }
            continue;
        }

        // Passkey pressed (0x4E)
        if sub_id == NOTIF_BOLT_PASSKEY_PRESSED {
            print!("*");
            use std::io::Write;
            let _ = std::io::stdout().flush();
            continue;
        }

        // Pairing status (0x54)
        if sub_id == NOTIF_BOLT_PAIRING_STATUS {
            let status = msg[3];
            let error = msg[4];
            eprintln!("\n      [DEBUG] Pairing status: msg={}", format_resp(&msg));
            if status == 0x02 && error == 0 {
                // Success!
                let new_slot = if msg.len() > 11 { msg[11] } else { 0 };
                paired_slot = Some(new_slot);
                break;
            } else if status == 0x00 {
                // Pairing in progress (lock open)
                println!("\n      Pairing in progress...");
                continue;
            } else {
                // Failure
                let _ = write_notification_flags(dev, old_flags);
                let err_msg = match error {
                    0x00 => "success (unexpected with non-zero status)",
                    0x01 => "device timeout",
                    0x02 => "pairing failed (wrong passkey?)",
                    _ => "unknown error",
                };
                bail!("Bolt pairing failed: {err_msg} (status=0x{status:02X} error=0x{error:02X})");
            }
        }
    }

    // Result
    println!("\n[5/5] Result:");
    if let Some(slot) = paired_slot {
        let name = bolt_read_device_name(dev, slot).unwrap_or_else(|_| target.name.clone());
        let type_str = describe_device_type(target.device_kind);
        println!("      SUCCESS! Paired {type_str} \"{name}\" in slot {slot}.");
    } else {
        println!("      TIMEOUT - pairing did not complete within the window.");
        // Cancel pairing
        let mut cancel_req = [0u8; 20];
        cancel_req[0] = HIDPP_LONG;
        cancel_req[1] = HIDPP_RECEIVER;
        cancel_req[2] = REG_WRITE_LONG;
        cancel_req[3] = REG_BOLT_PAIRING;
        cancel_req[4] = BOLT_PAIR_ACTION_CANCEL;
        let _ = hidpp_write_long(dev, &cancel_req);
    }

    let _ = write_notification_flags(dev, old_flags);
    Ok(())
}

// ─── cmd_unpair ─────────────────────────────────────────────────────────────

fn cmd_unpair(api: &HidApi, slot: u8, path: Option<&str>) -> Result<()> {
    if !(1..=6).contains(&slot) {
        bail!("slot must be between 1 and 6 (got {slot})");
    }
    let info = pick_receiver(api, path)?;
    let pid = info.product_id();
    let rname = receiver_name(pid).unwrap_or("receiver");

    let dev = info
        .open_device(api)
        .context("opening receiver HID interface")?;

    println!("=== Unpairing slot {slot} from {rname} ===\n");

    if is_bolt(pid) {
        cmd_unpair_bolt(&dev, slot)
    } else {
        cmd_unpair_unifying(&dev, slot)
    }
}

fn cmd_unpair_unifying(dev: &HidDevice, slot: u8) -> Result<()> {
    // Show current device in slot
    if let Ok(Some((dtype, wpid))) = unifying_read_pairing_info(dev, slot) {
        let name = unifying_read_device_name(dev, slot).unwrap_or_else(|_| "?".into());
        println!(
            "  Current: {} \"{name}\" (WPID {wpid})",
            describe_device_type(dtype)
        );
    } else {
        println!("  WARNING: slot {slot} appears empty.");
    }

    let req: [u8; 7] = [
        HIDPP_SHORT,
        HIDPP_RECEIVER,
        REG_WRITE_SHORT,
        REG_PAIRING,
        PAIR_ACTION_UNPAIR,
        slot,
        0x00,
    ];
    let resp = hidpp_write_short(dev, &req)?;
    if is_error(&resp) {
        let ecode = if resp.len() >= 6 { resp[5] } else { 0 };
        bail!(
            "Unpair rejected: error 0x{ecode:02X} ({})\n  Response: {}",
            error_code_name(ecode),
            format_resp(&resp)
        );
    }
    println!("\n  Unpaired slot {slot} successfully.");
    Ok(())
}

fn cmd_unpair_bolt(dev: &HidDevice, slot: u8) -> Result<()> {
    // Show current device in slot
    if let Ok(Some((dtype, wpid))) = bolt_read_pairing_info(dev, slot) {
        let name = bolt_read_device_name(dev, slot).unwrap_or_else(|_| "?".into());
        println!(
            "  Current: {} \"{name}\" (WPID {wpid})",
            describe_device_type(dtype)
        );
    } else {
        println!("  WARNING: slot {slot} appears empty.");
    }

    // Bolt unpair: write long register 0xC1 with action 0x03
    let mut req = [0u8; 20];
    req[0] = HIDPP_LONG;
    req[1] = HIDPP_RECEIVER;
    req[2] = REG_WRITE_LONG;
    req[3] = REG_BOLT_PAIRING;
    req[4] = BOLT_PAIR_ACTION_UNPAIR;
    req[5] = slot;

    let resp = hidpp_write_long(dev, &req)?;
    if is_error(&resp) {
        let ecode = if resp.len() >= 6 { resp[5] } else { 0 };
        bail!(
            "Bolt unpair rejected: error 0x{ecode:02X} ({})\n  Response: {}",
            error_code_name(ecode),
            format_resp(&resp)
        );
    }
    println!("\n  Unpaired slot {slot} successfully.");
    Ok(())
}

// ─── cmd_debug ──────────────────────────────────────────────────────────────

fn cmd_debug(api: &HidApi, path: Option<&str>) -> Result<()> {
    // First, dump ALL Logitech HID interfaces
    println!("=== All Logitech HID interfaces ===\n");
    for d in api.device_list().filter(|d| d.vendor_id() == LOGITECH_VID) {
        println!(
            "  PID=0x{:04X}  usage_page=0x{:04X}  usage=0x{:04X}  iface={}",
            d.product_id(),
            d.usage_page(),
            d.usage(),
            d.interface_number()
        );
        println!(
            "    path={}\n    product={:?}",
            d.path().to_string_lossy(),
            d.product_string()
        );
    }

    let info = pick_receiver(api, path)?;
    let pid = info.product_id();
    let rname = receiver_name(pid).unwrap_or("receiver");

    let dev = info.open_device(api).context("opening receiver")?;

    println!("\n=== Debug: {rname} (PID 0x{pid:04X}) ===\n");

    // Read notification flags
    println!("--- Notification flags (reg 0x00) ---");
    match read_notification_flags(&dev) {
        Ok(f) => println!("  flags: {:02X} {:02X} {:02X}", f[0], f[1], f[2]),
        Err(e) => println!("  error: {e}"),
    }

    // Try reg 0x02 (connection state) via short read
    println!("\n--- Connection state (reg 0x02 short read) ---");
    let req02: [u8; 7] = [
        HIDPP_SHORT,
        HIDPP_RECEIVER,
        REG_READ_SHORT,
        0x02,
        0x00,
        0x00,
        0x00,
    ];
    match hidpp_write_short(&dev, &req02) {
        Ok(resp) => println!("  resp: {}", format_resp(&resp)),
        Err(e) => println!("  error: {e}"),
    }

    // Try to trigger connection notifications via reg 0x02 write
    println!("\n--- Triggering connection state notifications (write 0x02) ---");
    let req02w: [u8; 7] = [
        HIDPP_SHORT,
        HIDPP_RECEIVER,
        REG_WRITE_SHORT,
        0x02,
        0x02,
        0x00,
        0x00,
    ];
    match hidpp_write_short(&dev, &req02w) {
        Ok(resp) => {
            println!("  write resp: {}", format_resp(&resp));
            // Now read all incoming notifications for 3 seconds
            println!("  Listening for notifications (3s)...");
            let deadline = Instant::now() + Duration::from_secs(3);
            while Instant::now() < deadline {
                if let Ok(Some(msg)) = hidpp_read_raw(&dev, 500) {
                    println!("    notif: {}", format_resp(&msg));
                }
            }
        }
        Err(e) => println!("  error: {e}"),
    }

    // Try Bolt pairing info sub-registers (using SHORT request for GET_LONG_REGISTER)
    println!("\n--- Bolt pairing info (reg 0xB5, sub 0x50-0x52, 0x61-0x62) ---");
    for sub_reg in [0x50u8, 0x51, 0x52, 0x61, 0x62] {
        let req: [u8; 7] = [
            HIDPP_SHORT,
            HIDPP_RECEIVER,
            REG_READ_LONG,
            REG_PAIRING_INFO,
            sub_reg,
            0x01,
            0x00,
        ];
        match hidpp_write_short(&dev, &req) {
            Ok(resp) => {
                let err = if is_error(&resp) { " [ERR]" } else { " [OK]" };
                println!("  0x{sub_reg:02X}:{err} {}", format_resp(&resp));
            }
            Err(e) => println!("  0x{sub_reg:02X}: {e}"),
        }
    }

    Ok(())
}
