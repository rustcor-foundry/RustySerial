mod buffer;
mod cli;
mod logger;
mod ui;

use anyhow::{Context, Result};
use buffer::LineBuffer;
use clap::Parser;
use cli::Args;
use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyCode, KeyEventKind,
        KeyModifiers,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures::StreamExt;
use logger::Logger;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::{io::Stdout, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_serial::{SerialPort, SerialPortBuilderExt};
use ui::UiState;

/// Quit key. Ctrl+] is the traditional escape for telnet/picocom-style
/// programs and avoids stomping on anything an embedded shell might want.
const QUIT_KEY: char = ']';

const HISTORY_LINES: usize = 5_000;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let Some(port_name) = args.port.clone() else {
        list_ports()?;
        return Ok(());
    };

    let mut logger = match &args.log {
        Some(path) => Some(
            Logger::open(path)
                .await
                .with_context(|| format!("failed to open log file {}", path.display()))?,
        ),
        None => None,
    };

    let mut terminal = setup_terminal()?;
    let result = run(&mut terminal, &args, &port_name, &mut logger).await;
    restore_terminal(&mut terminal)?;
    result
}

async fn run(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    args: &Args,
    port_name: &str,
    logger: &mut Option<Logger>,
) -> Result<()> {
    let mut buffer = LineBuffer::new(HISTORY_LINES);
    let mut events = EventStream::new();
    let mut read_buf = [0u8; 4096];
    let mut bytes_rx: u64 = 0;
    let mut bytes_tx: u64 = 0;
    let mut status = String::new();
    let mut status_clear_at: Option<std::time::Instant> = None;
    let mut scroll_offset: usize = 0;
    let mut port: Option<tokio_serial::SerialStream> = None;
    let reconnect_delay = Duration::from_millis(args.reconnect_delay_ms.max(100));
    let mut reconnect_at = std::time::Instant::now();
    let mut reconnect_attempt: u64 = 0;
    let mut disconnect_count: u64 = 0;

    let log_path_owned = args.log.as_ref().map(|p| p.display().to_string());

    loop {
        if port.is_none() {
            if std::time::Instant::now() >= reconnect_at {
                match open_serial_port(port_name, args) {
                    Ok(new_port) => {
                        port = Some(new_port);
                        if reconnect_attempt > 0 {
                            status = format!(
                                "reconnected to {} after {} attempt(s)",
                                port_name, reconnect_attempt
                            );
                        } else {
                            status = format!("connected to {}", port_name);
                        }
                        status_clear_at = Some(std::time::Instant::now() + Duration::from_secs(2));
                        reconnect_attempt = 0;
                    }
                    Err(e) => {
                        if args.reconnect {
                            reconnect_attempt = reconnect_attempt.saturating_add(1);
                            status = format!(
                                "reconnect #{} failed ({}). retry in {} ms",
                                reconnect_attempt,
                                e,
                                reconnect_delay.as_millis()
                            );
                            reconnect_at = std::time::Instant::now() + reconnect_delay;
                        } else {
                            status = format!("connect failed: {}", e);
                        }
                        status_clear_at = None;
                    }
                }
            }
        }

        let connected = port.is_some();

        // Clear stale status messages.
        if let Some(at) = status_clear_at {
            if std::time::Instant::now() >= at {
                status.clear();
                status_clear_at = None;
            }
        }

        // Draw.
        terminal.draw(|f| {
            let state = UiState {
                port_name,
                baud: args.baud,
                connected,
                bytes_rx,
                bytes_tx,
                reconnect_enabled: args.reconnect,
                reconnect_attempt,
                disconnect_count,
                status: &status,
                log_path: log_path_owned.as_deref(),
                buffer: &buffer,
                scroll_offset,
            };
            ui::draw(f, &state);
        })?;

        // Race: serial input, keyboard, redraw timer.
        tokio::select! {
            // Bytes from the device.
            read = async {
                if let Some(p) = port.as_mut() {
                    p.read(&mut read_buf).await
                } else {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "not connected"))
                }
            } => {
                match read {
                    Ok(0) => {
                        port = None;
                        reconnect_attempt = 0;
                        disconnect_count = disconnect_count.saturating_add(1);
                        status = "device disconnected (EOF)".into();
                        status_clear_at = None;
                        reconnect_at = std::time::Instant::now() + reconnect_delay;
                    }
                    Ok(n) => {
                        let chunk = &read_buf[..n];
                        buffer.push_bytes(chunk);
                        bytes_rx += n as u64;
                        if let Some(l) = logger.as_mut() {
                            if let Err(e) = l.write(chunk).await {
                                status = format!("log write failed: {}", e);
                                status_clear_at = Some(std::time::Instant::now() + Duration::from_secs(5));
                            }
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                        // tokio-serial uses TimedOut for "no data right now" when a
                        // timeout is configured. Just loop and redraw.
                    }
                    Err(e) => {
                        port = None;
                        reconnect_attempt = 0;
                        disconnect_count = disconnect_count.saturating_add(1);
                        if args.reconnect {
                            status = format!("read error: {} (reconnecting)", e);
                            reconnect_at = std::time::Instant::now() + reconnect_delay;
                        } else {
                            status = format!("read error: {}", e);
                        }
                        status_clear_at = None;
                    }
                }
            }

            // Keyboard / resize events.
            maybe_event = events.next() => {
                let Some(event) = maybe_event else { break };
                let event = match event {
                    Ok(event) => event,
                    Err(e) => {
                        // Input stream hiccups should not kill an active reconnecting session.
                        status = format!("input error: {}", e);
                        status_clear_at = Some(std::time::Instant::now() + Duration::from_secs(2));
                        continue;
                    }
                };
                match event {
                    Event::Key(k) if k.kind == KeyEventKind::Press || k.kind == KeyEventKind::Repeat => {
                        // Quit.
                        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char(QUIT_KEY) {
                            break;
                        }

                        // Scrollback.
                        match k.code {
                            KeyCode::PageUp => {
                                scroll_offset = scroll_offset.saturating_add(10);
                                continue;
                            }
                            KeyCode::PageDown => {
                                scroll_offset = scroll_offset.saturating_sub(10);
                                continue;
                            }
                            KeyCode::Esc => {
                                scroll_offset = 0;
                                continue;
                            }
                            _ => {}
                        }

                        // Send-break: Ctrl+B.
                        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('b') {
                            let Some(p) = port.as_mut() else {
                                status = "not connected".into();
                                status_clear_at = Some(std::time::Instant::now() + Duration::from_secs(2));
                                continue;
                            };

                            // 250ms break is plenty for most receivers.
                            if let Err(e) = p.set_break() {
                                status = format!("break failed: {}", e);
                                status_clear_at = Some(std::time::Instant::now() + Duration::from_secs(3));
                            } else {
                                tokio::time::sleep(Duration::from_millis(250)).await;
                                let _ = p.clear_break();
                                status = "break sent".into();
                                status_clear_at = Some(std::time::Instant::now() + Duration::from_secs(2));
                            }
                            continue;
                        }

                        // Translate key → bytes and send.
                        if let Some(bytes) = key_to_bytes(&k, args.crlf) {
                            if let Some(p) = port.as_mut() {
                                let write_result = p.write_all(&bytes).await;
                                if let Err(e) = write_result {
                                    reconnect_attempt = 0;
                                    disconnect_count = disconnect_count.saturating_add(1);
                                    if args.reconnect {
                                        status = format!("write error: {} (reconnecting)", e);
                                        reconnect_at = std::time::Instant::now() + reconnect_delay;
                                    } else {
                                        status = format!("write error: {}", e);
                                    }
                                    status_clear_at = None;
                                    port = None;
                                } else {
                                    bytes_tx += bytes.len() as u64;
                                    if args.local_echo {
                                        buffer.push_bytes(&bytes);
                                    }
                                }
                            } else {
                                status = "not connected".into();
                                status_clear_at = Some(std::time::Instant::now() + Duration::from_secs(2));
                            }
                        }
                    }
                    Event::Resize(_, _) => {} // redraw on next loop
                    _ => {}
                }
            }

            // Repaint cap so the status bar (rx/tx counters, fading status messages)
            // doesn't sit stale when nothing else is happening.
            _ = tokio::time::sleep(Duration::from_millis(100)) => {}
        }
    }

    Ok(())
}

fn open_serial_port(port_name: &str, args: &Args) -> Result<tokio_serial::SerialStream> {
    let mut port = tokio_serial::new(port_name, args.baud)
        .data_bits(args.data_bits_serial())
        .parity(args.parity.into())
        .stop_bits(args.stop_bits.into())
        .flow_control(args.flow_control())
        .timeout(Duration::from_millis(10))
        .open_native_async()
        .with_context(|| format!("failed to open serial port {}", port_name))?;

    // Best-effort: don't hold DTR/RTS asserted in a way that resets some MCUs
    // every time we connect. If the underlying driver doesn't support these,
    // we just continue.
    let _ = port.write_data_terminal_ready(true);
    let _ = port.write_request_to_send(true);
    Ok(port)
}

/// Map a crossterm key event to the byte sequence we send over the wire.
/// Covers printable ASCII, Enter (with optional CRLF), Backspace, Tab, Esc,
/// arrow keys, Home/End, and Ctrl-letter combinations.
fn key_to_bytes(k: &crossterm::event::KeyEvent, crlf: bool) -> Option<Vec<u8>> {
    use KeyCode::*;
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);

    match k.code {
        Char(c) => {
            if ctrl {
                // Ctrl+A..Z → 0x01..0x1A. Other ctrl combos pass through as the literal char.
                let c_lower = c.to_ascii_lowercase();
                if c_lower.is_ascii_alphabetic() {
                    Some(vec![(c_lower as u8) - b'a' + 1])
                } else {
                    let mut buf = [0u8; 4];
                    Some(c.encode_utf8(&mut buf).as_bytes().to_vec())
                }
            } else {
                let mut buf = [0u8; 4];
                Some(c.encode_utf8(&mut buf).as_bytes().to_vec())
            }
        }
        Enter => Some(if crlf {
            b"\r\n".to_vec()
        } else {
            b"\r".to_vec()
        }),
        Backspace => Some(vec![0x7f]),
        Tab => Some(vec![b'\t']),
        Esc => Some(vec![0x1b]),
        // ANSI cursor escape sequences. Most serial-attached shells understand these.
        Up => Some(b"\x1b[A".to_vec()),
        Down => Some(b"\x1b[B".to_vec()),
        Right => Some(b"\x1b[C".to_vec()),
        Left => Some(b"\x1b[D".to_vec()),
        Home => Some(b"\x1b[H".to_vec()),
        End => Some(b"\x1b[F".to_vec()),
        Delete => Some(b"\x1b[3~".to_vec()),
        _ => None,
    }
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let terminal = Terminal::new(CrosstermBackend::new(stdout))?;
    Ok(terminal)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

fn list_ports() -> Result<()> {
    let ports = tokio_serial::available_ports().context("could not enumerate serial ports")?;
    if ports.is_empty() {
        println!("No serial ports detected.");
        return Ok(());
    }
    println!("Available serial ports:");
    for p in ports {
        match p.port_type {
            tokio_serial::SerialPortType::UsbPort(info) => {
                let manuf = info.manufacturer.as_deref().unwrap_or("?");
                let product = info.product.as_deref().unwrap_or("?");
                println!(
                    "  {}  USB {:04x}:{:04x}  {} — {}",
                    p.port_name, info.vid, info.pid, manuf, product
                );
            }
            tokio_serial::SerialPortType::PciPort => println!("  {}  PCI", p.port_name),
            tokio_serial::SerialPortType::BluetoothPort => {
                println!("  {}  Bluetooth", p.port_name)
            }
            tokio_serial::SerialPortType::Unknown => println!("  {}", p.port_name),
        }
    }
    println!();
    println!("Connect with:  rustyserial <PORT> --baud 115200");
    Ok(())
}
