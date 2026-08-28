# EL15 GUI Design

## Layout

```
┌────────────────────────────────────────────────────────────────────┐
│ [LOAD: OFF]  [BT: Connected]  Fan: 0/5  Mode: CC  FW: HW:2.0 SW:1.7  OK │
└────────────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────┬─────────────────────┐
│ Voltage (V)                                  │ Run Time             │
│ 7.9357 V                                     │ 00:00:00             │
│                                              ├─────────────────────┤
│ Current (A)                                  │ Temp                 │
│ 0.00000 A                                    │ 28.49 °C             │
│                                              ├─────────────────────┤
│ Power (W)                                    │ Set Current          │
│ 0.00000 W                                    │ [0.300] A [Set]      │
│                                              ├─────────────────────┤
│                                              │ (CAP/DCR params here)│
└──────────────────────────────────────────────┴─────────────────────┘

┌────────────────────────────────────────────────────────────────────┐
│ [CC] [CV] [CR] [CP]   [CAP] [DCR]    Output: OFF   [Enable Load]   │
└────────────────────────────────────────────────────────────────────┘

┌────────────────────────────────────────────────────────────────────┐
│ Chart (V/I/P)                                                [Hide]  │
└────────────────────────────────────────────────────────────────────┘

┌────────────────────────────────────────────────────────────────────┐
│ Samples: 847  last: V=7.9357 I=0.0000 P=0.0000  [Clear] [Export]   │
└────────────────────────────────────────────────────────────────────┘

┌────────────────────────────────────────────────────────────────────┐
│ Bluetooth: [EL15_BLE_D7OFD ▼] [Scan] [Disconnect] Connected        │
│                                                [Settings] [Flash FW]│
└────────────────────────────────────────────────────────────────────┘
```

## Sections

### 1. Status Bar
- Load state (ON/OFF badge, color-coded green/gray)
- Bluetooth connection status badge
- Fan speed (0–5)
- Current mode name (translated label `label.mode`)
- Firmware version: shown as `HW:X.Y SW:X.Y` after device connects; `---` when disconnected (label `label.dev_versions`)
- OK/ERR/warning indicator (right-aligned)

### 2. Measurement Area
Left side — three large measurement cards with colored borders:
- **Voltage** (green): `X.XXXX V`
- **Current** (red): `X.XXXXX A`
- **Power** (purple): `X.XXXXX W`

Right side — three stacked info cells (mode-dependent):

| Mode | Cell 1 | Cell 2 | Cell 3 |
|------|--------|--------|--------|
| CC/CV/CR/CP | Run Time | Temp (°C) | **Editable setpoint** |
| CAP | Run Time | Capacity (Ah) | Energy (Wh) |
│ DCR | Run Time | Temp (°C) | Resistance (mΩ) |

### 3. Mode/Output Row
- Mode buttons: CC, CV, CR, CP, (spacer), CAP, DCR
- Each button has a translated tooltip (e.g. "Constant Current (CC)")
- Active mode is highlighted blue
- Buttons disabled when BT device not connected
- Output status text (OFF/ON, color-coded)
- Enable/Disable Load button (orange when OFF for visibility, green when ON)

### 4. Battery Measurement Parameters Panel
Located in the right column, below the info cards. Hidden for CC/CV/CR/CP modes.

**CAP mode (Capacity Test):**

Only one CAP parameter is reachable over Bluetooth. The BLE protocol has a dedicated opcode for
the discharge current (write `0x05`, read `0x0A`) but **no command at all** for the cutoff voltage
or the timer — those are front-panel settings. See `docs/BT_PROTOCOL.md`.

- Line 1: **Discharge current** input (A, range 0–12, i.e. 0–12000 mA) + "Set" button. This is
  sent to the device with opcode `0x05`. "Set" is disabled while no device is connected or the
  value is out of range.
- Line 2: a static note that Cutoff / Timer are set on the device and are not available over
  Bluetooth.
- Line 3 (temporary removed): **"On device: N.NNN A (NNNN mA)"** — the value read back with `0x0A` after each write.
  On its own line, not beside the input, so the editable value and the device's value are not
  confused. The device stores milliamps, so the readback is quantised (5.0 comes back as
  5.0000010) and must never be compared for exact equality. Shows "—" until the device answers.
- The discharge current is **not** present in the status packet, so it is requested explicitly on
  entering CAP mode and after every write.

*Previously* this panel offered editable Timer, Cutoff voltage, Chemistry and Cells controls. None
of them was ever transmitted to the device, and no BLE command exists that could transmit them —
they silently did nothing. Those editors are commented out in `battery_params_panel` (kept, not
deleted, so they can be restored if a firmware revision exposes the parameters), and their settings
fields are retained as local notes.

**DCR mode (DC Internal Resistance Test):**

Like the CAP cutoff, the DCR test currents and timer have **no BLE command** — they are front-panel
settings (manual §3.4.2 "DCR Params"). Unlike the CAP cutoff, they are *readable*: the DCR status
packet carries both test currents.

- Line 1: a static note that the test currents and timer are set on the device and are not
  available over Bluetooth.
- Line 2: read-only **"On device: I1: 20 mA   I2: 1000 mA"**, taken from status bytes 15..19 and
  19..23 (Amps on the wire, shown in mA). Shows "—" until the first status packet arrives.

Both mode panels use the same order — actionable controls, then the device-only notice, then the
read-back values — and share one i18n key for the notice text (`label.device_only_hint`), which is
therefore named after neither mode.

*Previously* this panel offered editable I1 / I2 / Timer inputs. None was ever transmitted, and no
command exists that could transmit them. Those editors are commented out in `battery_params_panel`
(kept, not deleted) and their settings fields are retained as local notes.

### 5. Chart
- V/I/P graph with per-trace toggles (V, I, P colored buttons)
- Layout modes: Combined (overlaid), Split ↕ (vertical stacked), Split ↔ (horizontal side-by-side)
- Chart fills all remaining vertical space in the window; resize by dragging the window border
- Window size is persisted between sessions (saved in settings)
- Minimum window size: 400×400 px
- Combined mode: voltage scale on left axis, current and power scales on right axis (color-coded)
- **X axis is time-based**, not sample-index based. Points are positioned by timestamp within an
  explicit time domain, and the axis is labelled underneath:
  - Roll: labels are relative to now (`-1h02m`, `-2m05s`, `-45s`, `0`)
  - Infinite: labels are wall-clock (`HH:MM:SS`)
  - Full labels are drawn when the plot is at least 320 px wide; narrower plots are labelled at
    their ends only. Split ↕ labels the bottom sub-chart only (all sub-charts share one domain).
- Traces are **min/max decimated** to at most two points per horizontal pixel. The full time span
  is always drawn — there is no cap on how many samples a plot may cover — and single-sample
  transients survive decimation.
- A pause longer than 5 poll intervals (minimum 1.5 s) **breaks the polyline**, so a disconnect or
  a paused log renders as a gap instead of a straight line. An isolated reading between two gaps is
  drawn as a dot.
- Time mode controls (bottom toolbar row, left-aligned):
  - **Mode toggle button** shows current mode: `⟳ Roll` or `∞ Infinite`; click to switch.
    Switching is a **view-only** change: it never adds to or removes from the sample buffer, so it
    is safe to toggle mid-run.
  - **Roll mode**: the last N seconds; time window input + "Set" button are shown. The domain is the
    window itself, so a partly-filled window draws on the right-hand portion of the canvas rather
    than stretching to fill it. The window is clamped to the history retention setting.
  - **Infinite mode**: everything retained in the buffer since the last Clear; the window input and
    "Set" are hidden.
  - **Clear button** (both modes): sets the graph's view epoch to now. Samples before the epoch are
    hidden from the graph in **both** modes, but are **not** deleted — a CSV export still contains
    them. Pressing Clear is the only action that hides recorded data from the graph.
  - **Buffer label**: wall-clock span currently held in the buffer, so a window wider than the
    available history is visibly explained rather than silently ignored.
- Hide/show toggle button
- The graph and CSV export read the **same** buffer. Retention (Settings → Application → History
  retention, default 86400 s = 24 h) bounds both together: the graph can never show something an
  export would miss, and vice versa. Clearing the samples buffer also resets the graph's view epoch.
  The 24 h default is sized for long CAP runs such as a car battery discharge.

### 6. Samples Panel
- Sample count
- Last sample summary (V/I/P values)
- Clear button (deletes all samples — unlike the chart's Clear, this discards data that would
  otherwise be exportable; it also resets the chart's view epoch)
- Export button (saves CSV with columns: timestamp, voltage, current, power, resistance, mode)

### 7. Connection Panel
- Bluetooth device picker dropdown
- Scan button
- Connect/Disconnect button
- Connection status badge
- Settings button
- Flash FW button

## Setpoint Behavior

- Editable text field in the right panel (third cell for basic modes)
- Press Enter or "Set" button to apply
- Setpoint is automatically sent to device when:
  - Mode is switched (stored default for new mode is sent)
  - Load is toggled ON (current setpoint sent before load enable)
- **Safety:** If load is ON and new value differs from current by >10×,
  a confirmation dialog appears before applying.

### Setpoint Ranges

| Mode | Label | Unit | Range |
|------|-------|------|-------|
| CC | Set Current | A | 0.000–12.000 |
| CV | Set Voltage | V | 0.100–60.000 |
| CR | Set Resistance | Ω | 0.1–7500.0 |
| CP | Set Power | W | 0.00–150.00 |
| CAP | Discharge current | A | 0.000–12.000 (device range 0–12000 mA) |
| DCR | Current | mA | 20–12000 |

### Setpoint Validation
- Out-of-range values are highlighted with a red border around the setpoint block.
- The "Set" button is disabled when the value is out of range.
- The Load ON button is also disabled when the setpoint is invalid.
- The valid range hint is shown in the setpoint label.

## Window Title
- Shows app name and version from Cargo.toml: "EL15 Electronic Load Controller vX.Y.Z"

## Application Icon
- Embedded PNG icon (256×256) loaded at startup for Linux/Windows window icon.
- macOS uses .icns file in the .app bundle (AppIcon.icns).
- Windows uses embedded .ico resource for taskbar/explorer.

## Settings Page

Responsive card-based layout. On wide windows (≥720px), cards are arranged in two columns. On narrow windows, they collapse to a single column.

### Cards

1. **Application**
   - Theme (dropdown: Light/Dark)
   - Language (dropdown)
   - Poll interval (dropdown: 50/100/200/500/1000/2000 ms; helper text below)
   - Auto-connect to first EL15 (toggle)
   - History retention (text input + "Set"; seconds, clamped to 60–86400, default 86400 = 24 h;
     helper text below). Bounds the shared sample buffer used by both the graph and CSV export.
     Shrinking it trims the buffer immediately and pulls the Roll window in with it.
     A hard backstop of 500 000 samples (~32 MB) also applies: it sits just above 24 h at the
     default 200 ms poll, so a *faster* poll reaches the ceiling first and retains proportionally
     less wall-clock time (at 50 ms, roughly 7 h).

2. **SCPI Server**
   - Enable SCPI server (toggle)
   - SCPI port (text input; helper text below)

3. **Maintenance**
   - Description text
   - "Open Firmware Update" button → opens the dedicated Firmware Update page

4. **About**
   - App name + version
   - Repository link (GitHub button opens browser)

Footer: "Settings are saved automatically."

Close button at the bottom.

## Firmware Update Page

Dedicated secondary page accessed from Settings > Maintenance > "Open Firmware Update".

### Layout (top → bottom)

1. **Title**: "Firmware Update"
2. **Upgrade steps card** (bordered box with numbered instructions)
3. **DFU connection status** (bordered box):
   - "DFU device: Ready" (green) — when a DFU USB device is detected
   - "DFU device: Not detected" (amber) — when no DFU device found
   - Polled every 2 seconds while on this page
4. **Firmware file**: label showing selected filename or "No file selected"
5. **Select Firmware File** button
6. **Start Upgrade** / **Stop** buttons
7. **Helper text** (when Start Upgrade is disabled):
   - No file + no DFU: "Select a firmware file and connect the device in DFU mode before starting the upgrade."
   - No file only: "Select a firmware file before starting the upgrade."
   - No DFU only: "Start Upgrade is disabled until a DFU device is detected."
8. **Progress bar**
9. **Status text** (progress %, error, or completion message)
10. **Close** button

### Start Upgrade Button Rules

Disabled unless ALL of:
- A firmware file is selected
- A DFU device is detected and ready
- No firmware upgrade is currently running

### Confirmation Dialog

Before flashing begins, a confirmation dialog appears:
- Warning that firmware update may interrupt the device
- Device must be in DFU mode
- USB/power must not be disconnected
- Buttons: Cancel / Continue

Only after "Continue" does flashing actually begin.

### Stop Button

Enabled only while an upgrade is running.

## Disconnection Detection
- When a BLE poll command fails (device powered off), the app detects disconnection.
- The DeviceEvent::Disconnected stream event also triggers cleanup.
- UI resets to disconnected state (clears status, firmware version, device handle).

## Verbose Logging
- `--verbose-ble`: enables debug logging for BLE device search and communication.
- `--verbose-gui`: enables debug logging for GUI message processing.

## Colors

| Element | Color |
|---------|-------|
| Voltage | Green (#33D95A) |
| Current | Red (#F24D4D) |
| Power | Purple (#B266F2) |
| Load ON | Green (#33C759) |
| Load OFF | Gray (#737380) |
| Active mode button | Blue (#3399F2) |
