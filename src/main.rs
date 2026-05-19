use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use hidapi::{DeviceInfo, HidApi, HidDevice};
use std::time::Duration;

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

const HIDPP_SHORT: u8 = 0x10;
const HIDPP_LONG: u8 = 0x11;
const HIDPP_RECEIVER: u8 = 0xFF;
const HIDPP_SET_SHORT: u8 = 0x80;
const HIDPP_GET_LONG: u8 = 0x83;

const REG_PAIRING: u8 = 0xB2;
const REG_PAIRING_INFO: u8 = 0xB5;
const REG_BOLT_PAIRING: u8 = 0xC0;

const READ_TIMEOUT_MS: i32 = 700;

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
    about = "List, pair, and unpair Logitech USB receivers via HID++"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// List connected Logitech receivers (and any paired devices it reports)
    List,
    /// Open the pairing lock on the receiver so a peripheral can be paired
    Pair {
        /// HID path of the receiver; omit to pick the first one found
        #[arg(long)]
        path: Option<String>,
        /// Pairing window timeout in seconds (0 = firmware default)
        #[arg(long, default_value_t = 30)]
        timeout: u8,
    },
    /// Unpair the peripheral in a slot (1..=6)
    Unpair {
        /// Slot number to remove
        slot: u8,
        /// HID path of the receiver; omit to pick the first one found
        #[arg(long)]
        path: Option<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let api = HidApi::new().context("initialising hidapi")?;

    match cli.command {
        Command::List => cmd_list(&api),
        Command::Pair { path, timeout } => cmd_pair(&api, path.as_deref(), timeout),
        Command::Unpair { slot, path } => cmd_unpair(&api, slot, path.as_deref()),
    }
}

fn hidpp_interfaces(api: &HidApi) -> Vec<&DeviceInfo> {
    // macOS surfaces a separate DeviceInfo per HID usage, so the same
    // receiver appears several times. Dedupe by HID path — that's what we'd
    // open() against anyway.
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
        println!(
            "Receiver: {name}  (VID 0x{:04X}  PID 0x{:04X})",
            info.vendor_id(),
            info.product_id()
        );
        println!("  Path        : {}", info.path().to_string_lossy());
        if let Some(s) = info.manufacturer_string() {
            println!("  Manufacturer: {s}");
        }
        if let Some(s) = info.product_string() {
            println!("  Product     : {s}");
        }
        if let Some(s) = info.serial_number() {
            println!("  Serial      : {s}");
        }
        println!(
            "  Release     : {}.{:02}",
            info.release_number() >> 8,
            info.release_number() & 0xFF
        );

        match info.open_device(api) {
            Ok(dev) => match list_paired_slots(&dev, info.product_id()) {
                Ok(slots) if slots.is_empty() => {
                    println!("  Paired slots: (none reported)");
                }
                Ok(slots) => {
                    println!("  Paired slots:");
                    for (idx, label) in slots {
                        println!("    slot {idx}: {label}");
                    }
                }
                Err(e) => {
                    println!("  Paired slots: <query failed: {e}>");
                }
            },
            Err(e) => {
                println!("  Paired slots: <cannot open receiver: {e}>");
                println!(
                    "    (on macOS, allow this terminal under System Settings →\n     Privacy & Security → Input Monitoring, then retry)"
                );
            }
        }
        println!();
    }

    Ok(())
}

fn cmd_pair(api: &HidApi, path: Option<&str>, timeout: u8) -> Result<()> {
    let info = pick_receiver(api, path)?;
    let pid = info.product_id();
    let dev = info
        .open_device(api)
        .context("opening receiver HID interface")?;
    let register = if is_bolt(pid) {
        REG_BOLT_PAIRING
    } else {
        REG_PAIRING
    };
    // sub-action 0x01 = open lock
    let req = [
        HIDPP_SHORT,
        HIDPP_RECEIVER,
        HIDPP_SET_SHORT,
        register,
        0x01,
        timeout,
        0x00,
    ];
    let resp = write_hidpp(&dev, &req)?;
    if is_error(&resp) {
        bail!(
            "receiver rejected open-pairing-lock: {}",
            format_resp(&resp)
        );
    }
    println!(
        "Pairing window opened on {} for {}s. Power-cycle your peripheral now.",
        receiver_name(pid).unwrap_or("receiver"),
        if timeout == 0 { 30 } else { timeout as u32 }
    );
    Ok(())
}

fn cmd_unpair(api: &HidApi, slot: u8, path: Option<&str>) -> Result<()> {
    if !(1..=6).contains(&slot) {
        bail!("slot must be between 1 and 6 (got {slot})");
    }
    let info = pick_receiver(api, path)?;
    let pid = info.product_id();
    let dev = info
        .open_device(api)
        .context("opening receiver HID interface")?;
    let register = if is_bolt(pid) {
        REG_BOLT_PAIRING
    } else {
        REG_PAIRING
    };
    // sub-action 0x03 = unpair the given device index
    let req = [
        HIDPP_SHORT,
        HIDPP_RECEIVER,
        HIDPP_SET_SHORT,
        register,
        0x03,
        slot,
        0x00,
    ];
    let resp = write_hidpp(&dev, &req)?;
    if is_error(&resp) {
        bail!("receiver rejected unpair: {}", format_resp(&resp));
    }
    println!("Unpaired slot {slot}.");
    Ok(())
}

fn list_paired_slots(dev: &HidDevice, pid: u16) -> Result<Vec<(u8, String)>> {
    // The Bolt receiver doesn't expose the legacy 0xB5 register, so skip the
    // scan there rather than printing six "invalid register" errors.
    if is_bolt(pid) {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for slot in 1u8..=6 {
        let req = [
            HIDPP_SHORT,
            HIDPP_RECEIVER,
            HIDPP_GET_LONG,
            REG_PAIRING_INFO,
            0x20 + (slot - 1),
            0x00,
            0x00,
        ];
        let resp = match write_hidpp(dev, &req) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if is_error(&resp) {
            continue;
        }
        if resp.first().copied() == Some(HIDPP_LONG) && resp.len() >= 8 {
            let dev_type = resp[7] & 0x0F;
            out.push((slot, describe_device_type(dev_type).to_string()));
        }
    }
    Ok(out)
}

fn describe_device_type(t: u8) -> &'static str {
    match t {
        0x01 => "keyboard",
        0x02 => "mouse",
        0x03 => "numpad",
        0x04 => "presenter",
        0x08 => "trackball",
        0x09 => "touchpad",
        _ => "other",
    }
}

fn write_hidpp(dev: &HidDevice, req: &[u8]) -> Result<Vec<u8>> {
    dev.write(req).context("writing HID++ request")?;
    let want_reg = req.get(3).copied();
    let deadline = std::time::Instant::now() + Duration::from_millis(READ_TIMEOUT_MS as u64 * 3);
    let mut buf = [0u8; 64];
    while std::time::Instant::now() < deadline {
        let n = match dev.read_timeout(&mut buf, READ_TIMEOUT_MS) {
            Ok(0) => continue,
            Ok(n) => n,
            Err(e) => return Err(e).context("reading HID++ response"),
        };
        let resp = buf[..n].to_vec();
        // Error reply: 10 FF 8F <sub-id-of-request> <reg> <err-code>
        if resp.len() >= 5 && resp[2] == 0x8F && Some(resp[4]) == want_reg {
            return Ok(resp);
        }
        if resp.len() >= 4 && Some(resp[3]) == want_reg {
            return Ok(resp);
        }
    }
    bail!("timed out waiting for HID++ response");
}

fn is_error(resp: &[u8]) -> bool {
    resp.len() >= 3 && resp[2] == 0x8F
}

fn format_resp(resp: &[u8]) -> String {
    let mut s = String::new();
    for b in resp {
        s.push_str(&format!("{b:02X} "));
    }
    s.trim_end().to_string()
}
