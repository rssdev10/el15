//! Connects to the first EL15 in range and prints a few status readings.
//!
//! Read only: it never switches the load on, changes the mode or writes a setpoint.
//!
//! `cargo run -p alientek-el15 --example status`

use std::time::Duration;

use alientek_el15::{scan_devices, Device, DeviceEvent, INTER_COMMAND_GAP};
use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let info = scan_devices(None)
        .await?
        .into_iter()
        .next()
        .ok_or("no EL15 found")?;
    println!("connecting to {}", info.display_label());

    let (device, mut events) = Device::connect(&info).await?;
    // Without the handshake the device reports zeros for temperature and setpoint.
    device.init_handshake().await?;

    let mut readings = 0;
    while readings < 5 {
        tokio::time::sleep(INTER_COMMAND_GAP.max(Duration::from_millis(500))).await;
        device.poll().await?;

        // Drain what arrived; stop waiting after a second without a status reply.
        let deadline = tokio::time::sleep(Duration::from_secs(1));
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                event = events.next() => match event {
                    Some(DeviceEvent::FirmwareVersion(v)) => println!("firmware {v}"),
                    Some(DeviceEvent::Status(s)) => {
                        println!(
                            "{:<4} {:7.3} V {:7.3} A {:7.2} W {:5.1} °C load {}",
                            s.mode_name,
                            s.voltage,
                            s.current,
                            s.power,
                            s.temperature,
                            if s.load_on { "ON" } else { "off" },
                        );
                        readings += 1;
                        break;
                    }
                    Some(DeviceEvent::Disconnected) | None => return Err("device disconnected".into()),
                    Some(_) => {}
                },
                _ = &mut deadline => break,
            }
        }
    }

    device.disconnect().await?;
    Ok(())
}
