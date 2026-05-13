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
- Line 1: Timer enable/disable toggle. Duration input (HH:MM:SS) is visible only when timer is enabled, on the same line as the Timer toggle.
- Line 2: Cutoff voltage input (always visible, range 0.1–60.0 V) + Chemistry type selector (N/A, NiMH/NiCd, NiZn, Li-Ion, LiPo, LiFePO4, Na-Ion) + Cells count combo box (visible only when chemistry is not N/A; allows picking 1–20 from dropdown or typing any value directly).
- When chemistry is selected, cutoff voltage is auto-calculated as (per-cell voltage × number of cells).
- Per-cell cutoff voltages: NiMH/NiCd=1.00V, NiZn=1.20V, Li-Ion=3.00V, LiPo=3.00V, LiFePO4=2.50V, Na-Ion=2.00V.
- Chemistry/cells selections are persisted in settings.

**DCR mode (DC Internal Resistance Test):**
- I1 current (mA, range 20–12000)
- I2 current (mA, range 20–12000)
- Timer (seconds, range 1–99)

### 5. Chart
- V/I/P graph with per-trace toggles (V, I, P colored buttons)
- Layout modes: Combined (overlaid), Split ↕ (vertical stacked), Split ↔ (horizontal side-by-side)
- Chart fills all remaining vertical space in the window; resize by dragging the window border
- Window size is persisted between sessions (saved in settings)
- Minimum window size: 400×400 px
- Combined mode: voltage scale on left axis, current and power scales on right axis (color-coded)
- Time mode controls (bottom toolbar row, left-aligned):
  - **Mode toggle button** shows current mode: `⟳ Roll` or `∞ Infinite`; click to switch
  - **Roll mode**: rolling window of the last N seconds; time window input + "Set" button are shown
  - **Infinite mode**: all data since app start or last Clear; time window input and "Set" are hidden
  - **Clear button** (Infinite mode only): resets graph display start time to now; does NOT delete samples
- Hide/show toggle button
- Graph data is independent of CSV export (export uses all collected samples)

### 6. Samples Panel
- Sample count
- Last sample summary (V/I/P values)
- Clear button (clears all samples)
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
| CAP | Cutoff V | V | 0.1–60.0 |
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
- Theme, Language, Poll interval, Auto-connect toggle
- SCPI Server section: enable/port
- About section: version display, GitHub repository link (opens browser)

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
