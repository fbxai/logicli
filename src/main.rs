use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use hidapi::HidApi;

mod backend;

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
    /// List connected receivers and their paired devices
    List,

    /// Discover and pair a new device (keyboard or mouse)
    Pair {
        /// HID path of the receiver; omit to pick the first one found
        #[arg(long)]
        path: Option<String>,
        /// Discovery timeout in seconds (default: 30)
        #[arg(long, default_value = "30")]
        timeout: u8,
        /// Use short authentication (4 button presses for mice instead of 10)
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

    /// Dump raw register reads for debugging (development only)
    Debug {
        /// HID path of the receiver; omit to pick the first one found
        #[arg(long)]
        path: Option<String>,
    },

    /// Print version information
    Version,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Version => {
            cmd_version();
            Ok(())
        }
        Command::List => {
            let api = HidApi::new().context("initialising hidapi")?;
            backend::logitech::cmd_list(&api)
        }
        Command::Pair {
            path,
            timeout,
            short_auth,
        } => {
            let api = HidApi::new().context("initialising hidapi")?;
            backend::logitech::cmd_pair(&api, path.as_deref(), timeout, short_auth)
        }
        Command::Unpair { slot, path } => {
            let api = HidApi::new().context("initialising hidapi")?;
            if !(1..=6).contains(&slot) {
                bail!("slot must be 1-6");
            }
            backend::logitech::cmd_unpair(&api, slot, path.as_deref())
        }
        Command::Debug { path } => {
            let api = HidApi::new().context("initialising hidapi")?;
            backend::logitech::cmd_debug(&api, path.as_deref())
        }
    }
}

fn cmd_version() {
    let version = option_env!("LOGICLI_VERSION").unwrap_or("dev");
    let commit = option_env!("LOGICLI_COMMIT").unwrap_or("unknown");
    let date = option_env!("LOGICLI_BUILD_DATE").unwrap_or("unknown");
    println!("logicli {version}");
    println!("  commit: {commit}");
    println!("  built:  {date}");
}
