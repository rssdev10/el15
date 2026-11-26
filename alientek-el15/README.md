# alientek-el15

Unofficial Rust client for the **ALIENTEK EL15** programmable DC electronic load over
Bluetooth LE. Not affiliated with or endorsed by ALIENTEK.

- Command frames: init, poll, load on/off, panel lock, mode (CC, CV, CR, CP, CAP, DCR),
  setpoint, and the CAP discharge current, each with its checksum.
- Parsers for the 28-byte status notification (voltage, current, power, temperature,
  setpoint, capacity, energy, DCR result, fan, load and lock state, protection warnings),
  the firmware version and the CAP discharge current.
- Scanning, connecting, and a `Device` handle that sends commands and turns notifications
  into `DeviceEvent`s.

Cargo feature `ble` (default) brings Bluetooth LE through `btleplug`. With
`default-features = false` the crate builds without a Bluetooth stack or async runtime and
keeps the command frames, the checksum and the reply parsers, for use over your own link.
Feature `serde` adds `Serialize`/`Deserialize` to the status and version types.

The protocol was reverse-engineered from the Bluetooth traffic of ALIENTEK's official
Android app, ATK-xTOOL, and cross-checked against the Python project DM40GUI. Verified
against an EL15 with hardware 2.0 and firmware 1.7. The full description, with the evidence
for each field, is in
[docs/BT_PROTOCOL.md](https://github.com/rssdev10/el15/blob/main/docs/BT_PROTOCOL.md).

## Example

Print a few readings from the first EL15 in range (read only: the load stays as it is):

```rust,no_run
use alientek_el15::{scan_devices, Device, DeviceEvent};
use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let info = scan_devices(None).await?.into_iter().next().ok_or("no EL15 found")?;
    let (device, mut events) = Device::connect(&info).await?;
    // Without the handshake the device reports zeros for temperature and setpoint.
    device.init_handshake().await?;

    for _ in 0..5 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        device.poll().await?;
        while let Some(event) = events.next().await {
            if let DeviceEvent::Status(s) = event {
                println!("{:.3} V  {:.3} A  {:.1} °C", s.voltage, s.current, s.temperature);
                break;
            }
        }
    }

    device.disconnect().await?;
    Ok(())
}
```

A complete version is in [examples/status.rs](examples/status.rs).

The device drops commands written back-to-back: keep at least `INTER_COMMAND_GAP` (120 ms)
between writes, and use `Device::send_sequence` for bursts.

## Using local sources

To try changes to this crate in another project before they are published, point the
dependency at a checkout:

```toml
[dependencies]
alientek-el15 = { path = "../el15/alientek-el15" }
```

or keep the crates.io dependency and override it in the project's `Cargo.toml`:

```toml
[patch.crates-io]
alientek-el15 = { path = "../el15/alientek-el15" }
```

## Safety

The EL15 sinks up to 150 W from the source under test. A wrong mode or setpoint can
over-discharge a battery or overload a power supply. The device's own protections are the
last line of defence, not a substitute for correct settings. Only hardware 2.0 with firmware
1.7 has been tested; other firmware may differ.

## Companion app

The [repository](https://github.com/rssdev10/el15) also holds `el15`, a GUI, CLI and
SCPI server built on this crate, released as binaries.
