use clap::{Parser, ValueEnum};
use std::path::PathBuf;

/// A small, modern, lightweight serial console TUI.
///
/// Connects to a serial port, shows incoming bytes in a scrollable view,
/// and forwards keystrokes to the port. Optionally logs all incoming traffic
/// to a file with timestamps.
#[derive(Parser, Debug, Clone)]
#[command(name = "rustyserial", version, about)]
pub struct Args {
    /// Serial port to open. Examples: /dev/ttyUSB0, /dev/ttyACM0, COM3.
    /// If omitted, lists available ports and exits.
    #[arg(value_name = "PORT")]
    pub port: Option<String>,

    /// Baud rate.
    #[arg(short, long, default_value_t = 115200)]
    pub baud: u32,

    /// Data bits.
    #[arg(long, default_value_t = 8, value_parser = clap::value_parser!(u8).range(5..=8))]
    pub data_bits: u8,

    /// Parity.
    #[arg(long, value_enum, default_value_t = Parity::None)]
    pub parity: Parity,

    /// Stop bits.
    #[arg(long, value_enum, default_value_t = StopBits::One)]
    pub stop_bits: StopBits,

    /// Hardware flow control (RTS/CTS).
    #[arg(long)]
    pub rtscts: bool,

    /// Software flow control (XON/XOFF).
    #[arg(long)]
    pub xonxoff: bool,

    /// Append all received bytes to this file, with timestamps on each line.
    #[arg(short, long, value_name = "PATH")]
    pub log: Option<PathBuf>,

    /// Convert outgoing CR (Enter key) to CRLF.
    #[arg(long)]
    pub crlf: bool,

    /// Echo typed characters locally (off by default; most devices echo).
    #[arg(long)]
    pub local_echo: bool,

    /// Disable automatic reconnect when the device disappears (e.g. reboot).
    #[arg(long = "no-reconnect", action = clap::ArgAction::SetFalse, default_value_t = true)]
    pub reconnect: bool,

    /// Delay in milliseconds between reconnect attempts.
    #[arg(long, default_value_t = 1000, value_name = "MS")]
    pub reconnect_delay_ms: u64,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum Parity {
    None,
    Odd,
    Even,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum StopBits {
    One,
    Two,
}

impl From<Parity> for tokio_serial::Parity {
    fn from(p: Parity) -> Self {
        match p {
            Parity::None => tokio_serial::Parity::None,
            Parity::Odd => tokio_serial::Parity::Odd,
            Parity::Even => tokio_serial::Parity::Even,
        }
    }
}

impl From<StopBits> for tokio_serial::StopBits {
    fn from(s: StopBits) -> Self {
        match s {
            StopBits::One => tokio_serial::StopBits::One,
            StopBits::Two => tokio_serial::StopBits::Two,
        }
    }
}

impl Args {
    pub fn data_bits_serial(&self) -> tokio_serial::DataBits {
        match self.data_bits {
            5 => tokio_serial::DataBits::Five,
            6 => tokio_serial::DataBits::Six,
            7 => tokio_serial::DataBits::Seven,
            _ => tokio_serial::DataBits::Eight,
        }
    }

    pub fn flow_control(&self) -> tokio_serial::FlowControl {
        if self.rtscts {
            tokio_serial::FlowControl::Hardware
        } else if self.xonxoff {
            tokio_serial::FlowControl::Software
        } else {
            tokio_serial::FlowControl::None
        }
    }
}
