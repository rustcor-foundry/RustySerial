# RustySerial

A small, modern serial console TUI. Roughly the lightweight equivalent of
PuTTY's serial mode — open a port, see traffic, type back at it, optionally
log everything to a file. Cross-platform (Linux, macOS, Windows).

Not a terminal emulator. Control sequences are rendered as `^X` rather than
interpreted, so a misbehaving device cannot blank your screen or fight your
TUI for cursor control. If you need a full terminal, pipe through `picocom`
or `screen`. If you need to see what a microcontroller is actually saying,
`RustySerial` is the tool.

## Features

- **Safe-by-default view** — control bytes shown as `^X`; a noisy device can't
  hijack your terminal.
- **Full line settings** — baud, data bits, parity, stop bits, and hardware
  (`--rtscts`) or software (`--xonxoff`) flow control.
- **Survives device reboots** — keeps the session open and auto-reconnects when a
  port disappears during a firmware flash/reset.
- **Logging** — append received bytes with timestamps to a file.
- **Scrollback** — page through history and snap back to the live tail.
- **Send BREAK** — 250 ms break signal on a keystroke.
- **Cross-platform** — Linux, macOS, and Windows.

## Two binaries

| Binary | Purpose |
|---|---|
| `rustyserial` | the serial console TUI (the main tool) |
| `rustyserial-gui` | an optional PuTTY-style **Bevy** setup window that launches the console with your chosen settings |

## Install

```
cargo install --path .
```

## Package (Windows)

Build release binaries and create a distributable zip:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\package-windows.ps1
```

Output:

- `dist/RustySerial-v<version>-windows-x64/`
- `dist/RustySerial-v<version>-windows-x64.zip`

The package contains:

- `rustyserial.exe`
- `rustyserial-gui.exe`
- `README.md`

## Use

```
# List ports
rustyserial

# Connect at 115200 8N1 (the default)
rustyserial /dev/ttyUSB0

# Common variations
rustyserial /dev/ttyACM0 --baud 9600
rustyserial COM3 --baud 115200 --crlf --log session.log
rustyserial /dev/ttyUSB0 --rtscts
```

### GUI launcher (Bevy)

If you prefer a PuTTY-style setup window, run the Bevy launcher:

```bash
# Build everything once
cargo build

# Launch the setup GUI
cargo run --bin rustyserial-gui
```

Use the GUI controls to set port, baud, data bits, parity, stop bits,
flow control, CRLF, local echo, and logging toggle. Press **Connect** to
spawn the main `rustyserial` console with your selected settings.

### Keys

| Key            | Action                                   |
|----------------|------------------------------------------|
| `Ctrl+]`       | Quit                                     |
| `Ctrl+B`       | Send a 250 ms BREAK                      |
| `PgUp` / `PgDn`| Scroll history                           |
| `Esc`          | Snap back to following the live tail     |
| Anything else  | Sent to the device                       |

### Flags

```
--baud <N>          Baud rate (default 115200)
--data-bits <5..8>  Data bits (default 8)
--parity <none|odd|even>
--stop-bits <one|two>
--rtscts            Hardware flow control
--xonxoff           Software flow control
--crlf              Convert Enter (CR) to CRLF on outgoing
--local-echo        Echo what you type into the local view
--log <PATH>        Append received bytes with timestamps to a file
--no-reconnect      Disable auto reconnect after disconnect/reboot
--reconnect-delay-ms <MS>
                    Delay between reconnect attempts (default 1000)
```

### Device Reboot / Disconnect Behavior

When a selected port disappears (for example during firmware flash/reset),
`RustySerial` keeps the terminal running and retries opening the same port.
This allows you to keep one session open while repeatedly rebooting a device.

## What it does not do

- ANSI/VT100 emulation (use a real terminal emulator if you need that)
- SSH, telnet, raw socket — serial only
- Session manager / saved profiles — use shell aliases or a wrapper script
- Hex view, transmit-from-file — clean additions if there's interest

## License

MIT OR Apache-2.0 at your option.
