//! SCPI smoke-tester / integration test. Connects to a running `el15 --no-gui`
//! SCPI server and exercises all supported modes and queries.
//!
//! Run with:
//!   cargo run --release -p scpi-test -- --port 5555
//!
//! For a full test: start el15 with a connected device first:
//!   ./target/release/el15 --no-gui --port 5555

use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::Parser;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

#[derive(Parser)]
struct Args {
    /// SCPI server host (default: 127.0.0.1).
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    /// SCPI server port.
    #[arg(long, default_value_t = 5555)]
    port: u16,
    /// Only run a specific test group (idn, cc, cv, cr, cp, cap, dcr, all).
    #[arg(long, default_value = "all")]
    test: String,
}

struct ScpiClient {
    tx: tokio::net::tcp::OwnedWriteHalf,
    reader: tokio::io::Lines<BufReader<tokio::net::tcp::OwnedReadHalf>>,
    passed: u32,
    failed: u32,
}

impl ScpiClient {
    async fn connect(addr: &str) -> Result<Self> {
        let stream = TcpStream::connect(addr)
            .await
            .with_context(|| format!("connecting to {addr}"))?;
        let (rx, tx) = stream.into_split();
        let reader = BufReader::new(rx).lines();
        Ok(Self { tx, reader, passed: 0, failed: 0 })
    }

    /// Send a command (no reply expected).
    async fn send(&mut self, cmd: &str) -> Result<()> {
        println!("  > {cmd}");
        self.tx.write_all(cmd.as_bytes()).await?;
        self.tx.write_all(b"\n").await?;
        tokio::time::sleep(Duration::from_millis(30)).await;
        Ok(())
    }

    /// Send a query and return the reply.
    async fn query(&mut self, cmd: &str) -> Result<String> {
        println!("  > {cmd}");
        self.tx.write_all(cmd.as_bytes()).await?;
        self.tx.write_all(b"\n").await?;
        match tokio::time::timeout(Duration::from_secs(3), self.reader.next_line()).await {
            Ok(Ok(Some(line))) => {
                println!("  < {line}");
                Ok(line)
            }
            Ok(Ok(None)) => bail!("server closed connection"),
            Ok(Err(e)) => bail!("io error: {e}"),
            Err(_) => bail!("timeout waiting for reply to: {cmd}"),
        }
    }

    /// Assert a query returns an expected value.
    async fn assert_eq(&mut self, cmd: &str, expected: &str) -> Result<()> {
        let reply = self.query(cmd).await?;
        if reply.trim() == expected {
            println!("    ✓ OK");
            self.passed += 1;
        } else {
            println!("    ✗ FAIL: expected '{}', got '{}'", expected, reply.trim());
            self.failed += 1;
        }
        Ok(())
    }

    /// Assert a query reply contains a substring.
    async fn assert_contains(&mut self, cmd: &str, substring: &str) -> Result<()> {
        let reply = self.query(cmd).await?;
        if reply.contains(substring) {
            println!("    ✓ OK (contains '{substring}')");
            self.passed += 1;
        } else {
            println!("    ✗ FAIL: '{}' not found in '{}'", substring, reply.trim());
            self.failed += 1;
        }
        Ok(())
    }

    /// Assert a query returns a numeric value (parseable as f64).
    async fn assert_numeric(&mut self, cmd: &str) -> Result<f64> {
        let reply = self.query(cmd).await?;
        match reply.trim().parse::<f64>() {
            Ok(v) => {
                println!("    ✓ OK (numeric: {v})");
                self.passed += 1;
                Ok(v)
            }
            Err(_) => {
                println!("    ✗ FAIL: '{}' is not numeric", reply.trim());
                self.failed += 1;
                Ok(0.0)
            }
        }
    }

    fn summary(&self) {
        println!("\n═══════════════════════════════════════");
        println!("  Results: {} passed, {} failed", self.passed, self.failed);
        println!("═══════════════════════════════════════");
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let addr = format!("{}:{}", args.host, args.port);
    println!("Connecting to SCPI server at {addr}...");
    let mut c = ScpiClient::connect(&addr).await?;
    println!("Connected.\n");

    let tests = &args.test;
    let all = tests == "all";

    if all || tests == "idn" {
        test_identity(&mut c).await?;
    }
    if all || tests == "cc" {
        test_cc_mode(&mut c).await?;
    }
    if all || tests == "cv" {
        test_cv_mode(&mut c).await?;
    }
    if all || tests == "cr" {
        test_cr_mode(&mut c).await?;
    }
    if all || tests == "cp" {
        test_cp_mode(&mut c).await?;
    }
    if all || tests == "cap" {
        test_cap_mode(&mut c).await?;
    }
    if all || tests == "dcr" {
        test_dcr_mode(&mut c).await?;
    }
    if all || tests == "status" {
        test_status_queries(&mut c).await?;
    }

    c.summary();
    if c.failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}

async fn test_identity(c: &mut ScpiClient) -> Result<()> {
    println!("── Test: Identity & System ──");
    c.assert_contains("*IDN?", "RIGOL").await?;
    c.assert_eq("SYST:ERR?", "0,\"No error\"").await?;
    c.assert_eq("*OPC?", "1").await?;
    c.assert_contains("SYST:VERS?", "1999").await?;
    println!();
    Ok(())
}

async fn test_cc_mode(c: &mut ScpiClient) -> Result<()> {
    println!("── Test: CC Mode (Constant Current) ──");
    c.send("SOUR:FUNC CURR").await?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    c.assert_eq("SOUR:FUNC?", "CURR").await?;
    c.send("SOUR:CURR 0.500").await?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    c.assert_eq("SOUR:CURR?", "0.500000").await?;
    // Turn on briefly, measure, turn off
    c.send("INP ON").await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    c.assert_numeric("MEAS:VOLT?").await?;
    c.assert_numeric("MEAS:CURR?").await?;
    c.assert_numeric("MEAS:POW?").await?;
    c.send("INP OFF").await?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    c.assert_eq("INP?", "0").await?;
    println!();
    Ok(())
}

async fn test_cv_mode(c: &mut ScpiClient) -> Result<()> {
    println!("── Test: CV Mode (Constant Voltage) ──");
    c.send("SOUR:FUNC VOLT").await?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    c.assert_eq("SOUR:FUNC?", "VOLT").await?;
    c.send("SOUR:VOLT 5.000").await?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    c.assert_eq("SOUR:VOLT?", "5.000000").await?;
    c.send("INP ON").await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    c.assert_numeric("MEAS:VOLT?").await?;
    c.assert_numeric("MEAS:CURR?").await?;
    c.send("INP OFF").await?;
    println!();
    Ok(())
}

async fn test_cr_mode(c: &mut ScpiClient) -> Result<()> {
    println!("── Test: CR Mode (Constant Resistance) ──");
    c.send("SOUR:FUNC RES").await?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    c.assert_eq("SOUR:FUNC?", "RES").await?;
    c.send("SOUR:RES 100.0").await?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    c.assert_eq("SOUR:RES?", "100.000000").await?;
    c.send("INP ON").await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    c.assert_numeric("MEAS:VOLT?").await?;
    c.assert_numeric("MEAS:CURR?").await?;
    c.assert_numeric("MEAS:RES?").await?;
    c.send("INP OFF").await?;
    println!();
    Ok(())
}

async fn test_cp_mode(c: &mut ScpiClient) -> Result<()> {
    println!("── Test: CP Mode (Constant Power) ──");
    c.send("SOUR:FUNC POW").await?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    c.assert_eq("SOUR:FUNC?", "POW").await?;
    c.send("SOUR:POW 1.0").await?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    c.assert_eq("SOUR:POW?", "1.000000").await?;
    c.send("INP ON").await?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    c.assert_numeric("MEAS:VOLT?").await?;
    c.assert_numeric("MEAS:POW?").await?;
    c.send("INP OFF").await?;
    println!();
    Ok(())
}

async fn test_cap_mode(c: &mut ScpiClient) -> Result<()> {
    println!("── Test: CAP Mode (Capacity) ──");
    c.send("SOUR:FUNC CAP").await?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    c.assert_eq("SOUR:FUNC?", "CAP").await?;
    // CAP mode queries
    c.assert_numeric("MEAS:CAP?").await?;
    c.assert_numeric("MEAS:ENER?").await?;
    c.assert_numeric("MEAS:DCHT?").await?;
    c.assert_numeric("MEAS:VOLT?").await?;
    println!();
    Ok(())
}

async fn test_dcr_mode(c: &mut ScpiClient) -> Result<()> {
    println!("── Test: DCR Mode (DC Resistance) ──");
    c.send("SOUR:FUNC DCR").await?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    c.assert_eq("SOUR:FUNC?", "DCR").await?;
    c.assert_numeric("MEAS:DCR?").await?;
    c.assert_numeric("MEAS:VOLT?").await?;
    println!();
    Ok(())
}

async fn test_status_queries(c: &mut ScpiClient) -> Result<()> {
    println!("── Test: Status Queries ──");
    c.assert_numeric("SYST:TEMP?").await?;
    c.assert_numeric("SYST:FAN?").await?;
    c.assert_numeric("MEAS:VOLT?").await?;
    c.assert_numeric("MEAS:CURR?").await?;
    c.assert_numeric("MEAS:POW?").await?;
    println!();
    Ok(())
}
