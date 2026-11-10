# ⚡ perimon

[![Rust](https://img.shields.io/badge/rust-2024%20edition-orange.svg)](https://www.rust-lang.org)
[![Platform](https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-blue.svg)](https://microsoft.com/windows)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)
[![GUI](https://img.shields.io/badge/gui-egui%20%2F%20eframe-purple.svg)](https://github.com/emilk/egui)

A sleek, ultra-fast, native Windows peripheral battery monitor written in pure **Rust**. No Electron, no Chromium, no 500MB proprietary bloatware like Razer Synapse or HyperX NGENUITY—just instant, lightweight hardware telemetry in a minimalist dark dashboard with system tray support.

---

## ✨ Features

- **Linear / Apple-Inspired Dark Theme:** Engineered with crisp 18px gutters, compact layout, custom Segoe UI typography, and subtle dark-mode elevation (`#0C0E12` palette).
- **Custom Frameless Window:** Built with a custom window titlebar, rounded corners, subtle border, and drag-to-move support on top bars and headers.
- **Faded Inactive / Sleeping State:** Sleeping mice, powered-off headsets, and disconnected Bluetooth peripherals don't disappear from the dashboard—they smoothly fade into recessed inactive cards (`○ Asleep`, `○ Standby`, `○ Offline`) and light right back up the second they awaken.
- **System Tray Integration:**
  - **Left-Click:** Instantly toggles and focuses the dashboard window.
  - **Right-Click:** Context menu with *Show Window*, *Scan Now*, and *Exit*.
  - **Minimize to Tray:** Closing the window (`X`) minimizes to tray rather than terminating the background telemetry monitor.
- **Low-Battery Toast Alerts:** Native Windows 10/11 toast notifications trigger when any device drops to &le; 20% (with debounce logic so you aren't spammed).
- **Zero Black Console Window:** Compiled with `#![windows_subsystem = "windows"]` so it never flashes or opens an ugly `cmd.exe` prompt on launch.
- **Single-Instance Protection:** Protected by a native Win32 session mutex (`perimon_instance_lock`) so multiple instances can never conflict or duplicate on boot.
- **Embedded Custom Squircle Icon:** High-resolution minimalist black squircle with a crisp white battery & lightning bolt baked directly into the PE `.exe` with `winres`.
- **Tiny Footprint:** Runs on ~25–35 MB RAM with ~0% CPU at idle.

---

## 🎮 Supported Peripherals & Telemetry

| Peripheral | Interface / Hardware ID | Telemetry Extracted | Method |
| :--- | :--- | :--- | :--- |
| **HyperX Cloud III Wireless** | 2.4GHz USB Dongle (`03F0:06BE` / `0951:02CC`) | Live Battery %, Cell Voltage in mV (`3.77V`), Dongle Status, Sleep State | Direct USB HID Report `0x0C` over vendor collection `0x01C0` on `Col05` |
| **Incott G23 v2 Mouse** | 2.4GHz Dongle / Wired (`093A:522C` / `622C`) | Live Battery %, Charging Status, DPI (`400-6400`), Polling Rate (`125-8000Hz`) | Direct USB HID Feature Report `0x09 0x89` over vendor page `0xFF05` |
| **Keychron Keyboards** | 2.4GHz Link Dongle (`3434:D030`) or Bluetooth | 2.4G Link Status or Live BLE Battery % | Windows SetupAPI Device Enumeration & `DEVPKEY_Device_BatteryLevel` |
| **Bluetooth Peripherals** | Windows Bluetooth | Live Battery % (Logitech MX Master, Moondrop, Audio-Technica, AirPods, etc.) | Validates active connection via `DEVPKEY_Device_IsConnected` |

---

## 🛠️ How to Build from Source

### Prerequisites
1. **Windows 10 or 11 (64-bit)**
2. **Rust & Cargo:** Install via [rustup.rs](https://rustup.rs/) (edition 2024 / MSVC toolchain):
   ```powershell
   rustup default stable-x86_64-pc-windows-msvc
   ```
3. **Visual Studio C++ Build Tools:** Ensure the MSVC linker (`link.exe`) and Windows SDK are installed (standard for Rust on Windows).

### Compiling

1. **Clone the repository:**
   ```powershell
   git clone https://github.com/alekangelov/perimon.git
   cd perimon
   ```

2. **Build the optimized Release binary:**
   ```powershell
   cargo build --release
   ```

3. The compiled executable will be generated at:
   ```
   target\release\perimon.exe
   ```

---

## 🚀 Running & Setup

### Quick Run
Double-click **`run.bat`** or launch directly:
```powershell
.\target\release\perimon.exe
```

### Command-Line Arguments
- `perimon.exe` — Launches the dashboard GUI directly.
- `perimon.exe --minimized` (or `--tray`) — Launches silently into the Windows system tray without opening the window (used for Windows boot startup).

### Auto-Start on Boot (Recommended)
To have `perimon` automatically launch on boot into the system tray and place a shortcut on your Desktop:

1. Open PowerShell in the project directory.
2. Run the included setup script:
   ```powershell
   powershell -ExecutionPolicy Bypass -File .\setup_startup.ps1
   ```
This configures:
- `%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\perimon.lnk` with the `--minimized` flag.
- Desktop shortcut with the custom baked-in icon.

---

## 🧠 Technical Deep-Dive

### HyperX Cloud III Telemetry
HyperX Cloud III Wireless headsets communicate through a vendor-specific USB HID interface (`UsagePage: 0x01C0`, `Usage: 0x0001`). `perimon` opens an asynchronous handle to the device interface via `CreateFileW` with `FILE_FLAG_OVERLAPPED`, requests a 64-byte output report payload (`[0x0C, 0x02, 0x03, 0x01, 0x00, 0x06]`), and reads back the response with `WaitForSingleObject` (700ms timeout).
- Byte 6: Live battery percentage (0–100%).
- Bytes 7–8: Cell voltage in millivolts (e.g., 3770 mV &rarr; 3.77 V).
- Zero voltage response: Identifies headset sleep / power-off state.

### Incott G23 v2 Mouse Telemetry
Incott wireless mice (PAW3395 / PAW3950 sensors) communicate with the PixArt dongle over vendor `UsagePage: 0xFF05` with 9-byte feature reports.
- `perimon` dispatches a feature report `{0x09, 0x89}` using `HidD_SetFeature`.
- The mouse responds via `HidD_GetFeature` with command echo `0x89` at byte 1.
- Byte 8 contains the true battery level (values > 100 indicate charging: `val - 128`).
- Byte 2 encodes both the polling rate index (`0x0F` mask) and DPI profile index (`0xF0` mask).

### Bluetooth Telemetry & Inactive Persistence
Windows caches paired Bluetooth device battery telemetry under `DEVPKEY_Device_BatteryLevel` (`{104EA319-6EE2-4701-BD47-8DDBF425BBE5}, 2`). Many monitors incorrectly report old cached battery values even when devices are off. `perimon` queries `DEVPKEY_Device_IsConnected` (`{83da6326-97a6-4088-9453-a1923f573b29}, 15`) to confirm active connection. When offline, devices cleanly transition into a dimmed inactive state rather than disappearing from the UI.

---

## 📦 Project Structure

```
perimon/
├── .github/
│   └── workflows/
│       └── ci.yml         # GitHub Actions Windows CI build & test workflow
├── src/
│   ├── main.rs            # egui desktop app, custom frameless window, tray, notifications
│   ├── scanner.rs         # Direct USB HID & Bluetooth telemetry scanners
│   └── windows_ffi.rs     # Low-level Win32 / SetupAPI / HID FFI bindings
├── app_icon.ico           # Embedded PE application icon
├── app_icon.png           # Master icon asset
├── app_icon_128.rgba      # 128x128 header logo RGBA bitmap
├── tray_icon_32.rgba      # 32x32 crisp tray icon RGBA bitmap
├── build.rs               # Windows resource compiler (winres)
├── Cargo.toml             # Rust package configuration
├── LICENSE                # MIT License
├── README.md              # Documentation
├── run.bat                # Quick launch batch script
└── setup_startup.ps1      # Windows Startup folder & Desktop shortcut installer
```

---

## 📄 License

Distributed under the MIT License. See [`LICENSE`](LICENSE) for more information.
