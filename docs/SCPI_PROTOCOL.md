# SCPI / DL3000 Emulation

`el15-scpi` exposes a TCP raw-socket SCPI server (default port **5555**, the
same port Rigol's LXI raw service uses). The implementation is in
[el15-scpi/src/handlers.rs](../el15-scpi/src/handlers.rs) and is informed by
the *RIGOL DL3000 Series Programmable DC Electronic Load Programming Manual*
(`docs/DL3000_ProgrammingManual_EN.pdf`, not redistributed).

## Framing

- Plain TCP, line-oriented (`\n` or `\r\n`).
- Multiple commands per line allowed when separated by `;`.
- Queries (`...?`) reply on a single line.

## Implemented command surface

| SCPI                              | Direction | EL15 mapping                                   |
| --------------------------------- | --------- | ---------------------------------------------- |
| `*IDN?`                           | Q         | `RIGOL TECHNOLOGIES,DL3021A,EL15-BRIDGE,01.00` |
| `*RST`, `*CLS`, `*OPC?`           | W/Q       | NOP / `1`                                      |
| `SYST:ERR?`                       | Q         | `0,"No error"` (placeholder)                   |
| `SYST:VERS?`                      | Q         | `1999.0`                                       |
| `SOUR:FUNC {CURR\|VOLT\|RES\|POW}`| W         | `AF 07 03 03 01 <mode>`                        |
| `SOUR:FUNC?`                      | Q         | Last known mode                                 |
| `SOUR:CURR <A>` / `SOUR:CURR?`    | W/Q       | CC mode + setpoint                             |
| `SOUR:VOLT <V>` / `SOUR:VOLT?`    | W/Q       | CV mode + setpoint                             |
| `SOUR:RES  <Ω>` / `SOUR:RES?`     | W/Q       | CR mode + setpoint                             |
| `SOUR:POW  <W>` / `SOUR:POW?`     | W/Q       | CP mode + setpoint                             |
| `INP {ON\|OFF\|1\|0}` / `INP?`    | W/Q       | `AF 07 03 09 01 04 / 00`                       |
| `MEAS:VOLT?`                      | Q         | Latest status `voltage`                        |
| `MEAS:CURR?`                      | Q         | Latest status `current`                        |
| `MEAS:POW?`                       | Q         | Latest `voltage * current`                     |
| `MEAS:RES?`                       | Q         | `voltage / current`                            |
| `SYST:TEMP?` / `SYST:FAN?`        | Q         | Status temperature / fan speed                 |

Unknown queries return `-113,"Undefined header"` (SCPI standard).
Unknown writes are silent NOPs (matching DL3000 behaviour).

## Logging

When `--log <file>` is passed (or `Settings → Log SCPI to file` in GUI), every
request and reply is written with a local-timezone timestamp:

```
[2026-04-29 12:34:56.123 +03:00] 192.168.1.10:54321 <-- Q   MEAS:VOLT                reply=0.000000
[2026-04-29 12:34:56.124 +03:00] 192.168.1.10:54321 --> Q   MEAS:VOLT                reply=0.000000
```

## Test

```bash
# Terminal 1
cargo run --release -p el15-app -- --no-gui --port 5555 -v

# Terminal 2
cargo run --release -p scpi-test -- --port 5555
```

`scripts/scpi-test` walks through the typical command sequence used to drive a
DL3000 from a remote test harness.
