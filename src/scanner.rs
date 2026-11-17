use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Serialize};

use crate::windows_ffi::*;

static KNOWN_BT_DEVICES: Mutex<Option<HashMap<String, BluetoothDeviceStatus>>> = Mutex::new(None);

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct HyperXStatus {
    pub device: String,
    pub connected: bool,
    pub battery: Option<u8>,
    pub voltage_mv: Option<u16>,
    pub status: String,
    pub dongle_connected: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct IncottStatus {
    pub device: String,
    pub connected: bool,
    pub battery: Option<u8>,
    pub charging: bool,
    pub dpi: Option<u32>,
    pub polling_rate: Option<u32>,
    pub status: String,
    pub dongle_connected: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct KeychronStatus {
    pub device: String,
    pub connected: bool,
    pub battery: Option<u8>,
    pub connection_type: String,
    pub status: String,
    pub note: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BluetoothDeviceStatus {
    pub device: String,
    pub connected: bool,
    pub battery: u8,
    pub connection_type: String,
    pub status: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AllDevicesStatus {
    pub timestamp: f64,
    pub hyperx: HyperXStatus,
    pub incott: IncottStatus,
    pub keychron: KeychronStatus,
    pub other_devices: Vec<BluetoothDeviceStatus>,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct HidInterfaceInfo {
    pub vid: u16,
    pub pid: u16,
    pub usage_page: u16,
    pub usage: u16,
    pub in_len: u16,
    pub out_len: u16,
    pub feat_len: u16,
    pub path: String,
}

fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn enumerate_hid_interfaces() -> Vec<HidInterfaceInfo> {
    let mut results = Vec::new();

    unsafe {
        let mut hid_guid = GUID::default();
        HidD_GetHidGuid(&mut hid_guid);

        let hdev = SetupDiGetClassDevsW(
            &hid_guid,
            ptr::null(),
            ptr::null_mut(),
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        );

        if hdev.is_null() || hdev == INVALID_HANDLE_VALUE {
            return results;
        }

        let mut index: DWORD = 0;
        loop {
            let mut idata = SP_DEVICE_INTERFACE_DATA {
                cb_size: std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as DWORD,
                interface_class_guid: GUID::default(),
                flags: 0,
                reserved: 0,
            };

            if SetupDiEnumDeviceInterfaces(hdev, ptr::null_mut(), &hid_guid, index, &mut idata) == 0 {
                break;
            }
            index += 1;

            let mut req_size: DWORD = 0;
            SetupDiGetDeviceInterfaceDetailW(
                hdev,
                &mut idata,
                ptr::null_mut(),
                0,
                &mut req_size,
                ptr::null_mut(),
            );

            if req_size == 0 {
                continue;
            }

            let mut buf = vec![0u8; req_size as usize];
            // cb_size is 8 bytes on 64-bit Windows
            *(buf.as_mut_ptr() as *mut DWORD) = 8;

            if SetupDiGetDeviceInterfaceDetailW(
                hdev,
                &mut idata,
                buf.as_mut_ptr(),
                req_size,
                ptr::null_mut(),
                ptr::null_mut(),
            ) != 0
            {
                let path_slice = std::slice::from_raw_parts(
                    buf.as_ptr().add(4) as *const u16,
                    (req_size as usize - 4) / 2,
                );
                let null_pos = path_slice.iter().position(|&c| c == 0).unwrap_or(path_slice.len());
                let path = String::from_utf16_lossy(&path_slice[..null_pos]);

                let wide_path = to_wide_null(&path);
                let h = CreateFileW(
                    wide_path.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    ptr::null_mut(),
                    OPEN_EXISTING,
                    0,
                    ptr::null_mut(),
                );

                if !h.is_null() && h != INVALID_HANDLE_VALUE {
                    let mut attr = HIDD_ATTRIBUTES {
                        size: std::mem::size_of::<HIDD_ATTRIBUTES>() as DWORD,
                        vendor_id: 0,
                        product_id: 0,
                        version_number: 0,
                    };

                    if HidD_GetAttributes(h, &mut attr) != 0 {
                        let mut preparsed: *mut c_void = ptr::null_mut();
                        let mut caps = HIDP_CAPS::default();

                        if HidD_GetPreparsedData(h, &mut preparsed) != 0 {
                            HidP_GetCaps(preparsed, &mut caps);
                            HidD_FreePreparsedData(preparsed);

                            results.push(HidInterfaceInfo {
                                vid: attr.vendor_id,
                                pid: attr.product_id,
                                usage_page: caps.usage_page,
                                usage: caps.usage,
                                in_len: caps.input_report_byte_length,
                                out_len: caps.output_report_byte_length,
                                feat_len: caps.feature_report_byte_length,
                                path,
                            });
                        }
                    }
                    CloseHandle(h);
                }
            }
        }

        SetupDiDestroyDeviceInfoList(hdev);
    }

    results
}

pub fn read_hyperx_cloud_3(devices: &[HidInterfaceInfo]) -> HyperXStatus {
    let target = devices.iter().find(|d| {
        (d.vid == 0x03F0 || d.vid == 0x0951)
            && (d.pid == 0x06BE || d.pid == 0x02CC)
            && d.usage_page == 0x01C0
            && d.usage == 0x0001
    });

    let target = match target {
        Some(t) => t,
        None => {
            return HyperXStatus {
                device: "HyperX Cloud III Wireless".to_string(),
                connected: false,
                battery: None,
                voltage_mv: None,
                status: "Dongle not found".to_string(),
                dongle_connected: false,
            };
        }
    };

    unsafe {
        let wide_path = to_wide_null(&target.path);
        let h = CreateFileW(
            wide_path.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            ptr::null_mut(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED,
            ptr::null_mut(),
        );

        if h.is_null() || h == INVALID_HANDLE_VALUE {
            return HyperXStatus {
                device: "HyperX Cloud III Wireless".to_string(),
                connected: false,
                battery: None,
                voltage_mv: None,
                status: "Could not open device handle".to_string(),
                dongle_connected: true,
            };
        }

        let mut ov_r = OVERLAPPED {
            h_event: CreateEventW(ptr::null_mut(), 1, 0, ptr::null()),
            ..Default::default()
        };

        let mut in_buf = [0u8; 64];
        let mut read_bytes: DWORD = 0;
        ReadFile(
            h,
            in_buf.as_mut_ptr(),
            64,
            &mut read_bytes,
            &mut ov_r,
        );

        // Command payload: [0x0C, 0x02, 0x03, 0x01, 0x00, 0x06]
        let mut payload = [0u8; 64];
        payload[0] = 0x0C;
        payload[1] = 0x02;
        payload[2] = 0x03;
        payload[3] = 0x01;
        payload[4] = 0x00;
        payload[5] = 0x06;
        HidD_SetOutputReport(h, payload.as_ptr(), 64);

        let res = WaitForSingleObject(ov_r.h_event, 700);
        let mut battery: Option<u8> = None;
        let mut voltage_mv: Option<u16> = None;
        let mut status = "Online".to_string();

        if res == WAIT_OBJECT_0 {
            GetOverlappedResult(h, &mut ov_r, &mut read_bytes, 0);
            if read_bytes >= 9 && in_buf[0] == 0x0C {
                let bat = in_buf[6];
                let mv = ((in_buf[7] as u16) << 8) | (in_buf[8] as u16);
                if (mv == 0 && bat == 0) || bat > 100 {
                    status = "Headset asleep / off".to_string();
                } else {
                    battery = Some(bat);
                    voltage_mv = Some(mv);
                    status = "Connected".to_string();
                }
            }
        } else {
            CancelIo(h);
            status = "Headset asleep / off".to_string();
        }

        CloseHandle(ov_r.h_event);
        CloseHandle(h);

        let is_connected = status == "Connected";
        HyperXStatus {
            device: "HyperX Cloud III Wireless".to_string(),
            connected: is_connected,
            battery: if is_connected { battery } else { None },
            voltage_mv: if is_connected { voltage_mv } else { None },
            status,
            dongle_connected: true,
        }
    }
}

pub fn read_incott_mouse(devices: &[HidInterfaceInfo]) -> IncottStatus {
    let target = devices.iter().find(|d| {
        d.vid == 0x093A
            && (d.pid == 0x522C || d.pid == 0x622C)
            && d.usage_page == 0xFF05
            && d.feat_len == 9
    });

    let target = match target {
        Some(t) => t,
        None => {
            return IncottStatus {
                device: "Incott G23 v2 Mouse".to_string(),
                connected: false,
                battery: None,
                charging: false,
                dpi: None,
                polling_rate: None,
                status: "Dongle not found".to_string(),
                dongle_connected: false,
            };
        }
    };

    let is_wired = target.pid == 0x622C;

    unsafe {
        let wide_path = to_wide_null(&target.path);
        let h = CreateFileW(
            wide_path.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            ptr::null_mut(),
            OPEN_EXISTING,
            0,
            ptr::null_mut(),
        );

        if h.is_null() || h == INVALID_HANDLE_VALUE {
            return IncottStatus {
                device: "Incott G23 v2 Mouse".to_string(),
                connected: false,
                battery: None,
                charging: false,
                dpi: None,
                polling_rate: None,
                status: "Could not open device handle".to_string(),
                dongle_connected: true,
            };
        }

        // Send feature report {0x09, 0x89}
        let mut req = [0u8; 9];
        req[0] = 0x09;
        req[1] = 0x89;
        HidD_SetFeature(h, req.as_ptr(), 9);

        let mut battery: Option<u8> = None;
        let mut charging = is_wired;
        // DPI / polling rate are not reliably encoded in the 0x89 status reply, so don't guess.
        let dpi: Option<u32> = None;
        let polling_rate: Option<u32> = None;

        for _ in 0..12 {
            std::thread::sleep(std::time::Duration::from_millis(40));
            let mut resp = [0u8; 9];
            resp[0] = 0x09;
            if HidD_GetFeature(h, resp.as_mut_ptr(), 9) != 0 {
                // Reply layout: [0]=0x09 report id, [1]=0x89 echo, [2]=battery % (bit 7 = charging)
                if resp[1] == 0x89 && (resp[2] != 0 || resp[3] != 0) {
                    let raw_bat = resp[2];
                    let bat = if raw_bat > 100 { raw_bat & 0x7F } else { raw_bat }.min(100);
                    battery = Some(bat);
                    charging = is_wired || raw_bat > 100;
                    break;
                }
            }
        }

        CloseHandle(h);

        let is_connected = battery.is_some();
        IncottStatus {
            device: "Incott G23 v2 Mouse".to_string(),
            connected: is_connected,
            battery,
            charging,
            dpi,
            polling_rate,
            status: if is_connected {
                "Online".to_string()
            } else {
                "Mouse asleep / standby".to_string()
            },
            dongle_connected: true,
        }
    }
}

pub fn read_keychron_and_bluetooth() -> (KeychronStatus, Vec<BluetoothDeviceStatus>) {
    // DEVPKEY_Device_BatteryLevel: {104EA319-6EE2-4701-BD47-8DDBF425BBE5}, 2
    let key_bat = DEVPROPKEY {
        fmtid: GUID::new(
            0x104EA319,
            0x6EE2,
            0x4701,
            [0xBD, 0x47, 0x8D, 0xDB, 0xF4, 0x25, 0xBB, 0xE5],
        ),
        pid: 2,
    };

    // DEVPKEY_NAME: {b725f130-47ef-101a-a5f1-02608c9eebac}, 10
    let key_name = DEVPROPKEY {
        fmtid: GUID::new(
            0xb725f130,
            0x47ef,
            0x101a,
            [0xa5, 0xf1, 0x02, 0x60, 0x8c, 0x9e, 0xeb, 0xac],
        ),
        pid: 10,
    };

    // DEVPKEY_Device_HardwareIds: {a45c254e-df1c-4efd-8020-67d146a850e0}, 3
    let key_hw = DEVPROPKEY {
        fmtid: GUID::new(
            0xa45c254e,
            0xdf1c,
            0x4efd,
            [0x80, 0x20, 0x67, 0xd1, 0x46, 0xa8, 0x50, 0xe0],
        ),
        pid: 3,
    };

    // DEVPKEY_Device_IsConnected: {83da6326-97a6-4088-9453-a1923f573b29}, 15
    let key_is_connected = DEVPROPKEY {
        fmtid: GUID::new(
            0x83da6326,
            0x97a6,
            0x4088,
            [0x94, 0x53, 0xa1, 0x92, 0x3f, 0x57, 0x3b, 0x29],
        ),
        pid: 15,
    };

    let mut keychron_info: Option<KeychronStatus> = None;
    let mut keychron_dongle_present = false;
    let mut other_devices = Vec::new();

    unsafe {
        let hdev = SetupDiGetClassDevsW(
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
            DIGCF_PRESENT | DIGCF_ALLCLASSES,
        );

        if !hdev.is_null() && hdev != INVALID_HANDLE_VALUE {
            let mut idx: DWORD = 0;
            loop {
                let mut dinfo = SP_DEVINFO_DATA {
                    cb_size: std::mem::size_of::<SP_DEVINFO_DATA>() as DWORD,
                    class_guid: GUID::default(),
                    dev_inst: 0,
                    reserved: 0,
                };

                if SetupDiEnumDeviceInfo(hdev, idx, &mut dinfo) == 0 {
                    break;
                }
                idx += 1;

                // Check Hardware ID for Keychron Link Dongle (VID 0x3434, PID 0xD030)
                let mut hwbuf = [0u16; 512];
                let mut prop_type: ULONG = 0;
                let mut req: DWORD = 0;
                if SetupDiGetDevicePropertyW(
                    hdev,
                    &dinfo,
                    &key_hw,
                    &mut prop_type,
                    hwbuf.as_mut_ptr() as *mut u8,
                    1024,
                    &mut req,
                    0,
                ) != 0
                {
                    let len = hwbuf.iter().position(|&c| c == 0).unwrap_or(hwbuf.len());
                    let hw_str = String::from_utf16_lossy(&hwbuf[..len]).to_uppercase();
                    if hw_str.contains("VID_3434&PID_D030") {
                        keychron_dongle_present = true;
                    }
                }

                // Check Battery Level property
                let mut bat_buf = [0u8; 256];
                if SetupDiGetDevicePropertyW(
                    hdev,
                    &dinfo,
                    &key_bat,
                    &mut prop_type,
                    bat_buf.as_mut_ptr(),
                    256,
                    &mut req,
                    0,
                ) != 0
                {
                    // Check if this device is ACTUALLY connected right now (not just a cached devnode)
                    let mut is_connected = false;
                    let mut conn_buf = [0u8; 16];
                    if SetupDiGetDevicePropertyW(
                        hdev,
                        &dinfo,
                        &key_is_connected,
                        &mut prop_type,
                        conn_buf.as_mut_ptr(),
                        16,
                        &mut req,
                        0,
                    ) != 0
                    {
                        // DEVPROP_TYPE_BOOLEAN: true is non-zero
                        is_connected = conn_buf[0] != 0;
                    }

                    let mut nbuf = [0u16; 256];
                    SetupDiGetDevicePropertyW(
                        hdev,
                        &dinfo,
                        &key_name,
                        &mut prop_type,
                        nbuf.as_mut_ptr() as *mut u8,
                        512,
                        &mut req,
                        0,
                    );
                    let len = nbuf.iter().position(|&c| c == 0).unwrap_or(nbuf.len());
                    let dev_name = String::from_utf16_lossy(&nbuf[..len]);
                    let bat_pct = bat_buf[0];

                    if dev_name.to_lowercase().contains("keychron") {
                        if is_connected || keychron_info.is_none() {
                            keychron_info = Some(KeychronStatus {
                                device: dev_name,
                                connected: is_connected,
                                battery: if is_connected { Some(bat_pct) } else { None },
                                connection_type: "Bluetooth".to_string(),
                                status: if is_connected { "Connected".to_string() } else { "Disconnected".to_string() },
                                note: None,
                            });
                        }
                    } else {
                        other_devices.push(BluetoothDeviceStatus {
                            device: dev_name,
                            connected: is_connected,
                            battery: bat_pct,
                            connection_type: "Bluetooth".to_string(),
                            status: if is_connected { "Connected".to_string() } else { "Disconnected".to_string() },
                        });
                    }
                }
            }

            SetupDiDestroyDeviceInfoList(hdev);
        }
    }

    let final_keychron = match keychron_info {
        Some(kc) => kc,
        None => {
            if keychron_dongle_present {
                KeychronStatus {
                    device: "Keychron Keyboard".to_string(),
                    connected: true,
                    battery: None,
                    connection_type: "2.4GHz Link Dongle".to_string(),
                    status: "Dongle Active (Fn+B for LED level)".to_string(),
                    note: Some(
                        "Keychron 2.4GHz dongles do not report battery to Windows OS. Switch to Bluetooth mode to see live percentage.".to_string(),
                    ),
                }
            } else {
                KeychronStatus {
                    device: "Keychron Keyboard".to_string(),
                    connected: false,
                    battery: None,
                    connection_type: "None".to_string(),
                    status: "Not connected".to_string(),
                    note: None,
                }
            }
        }
    };

    let mut cache_lock = KNOWN_BT_DEVICES.lock().unwrap();
    let map = cache_lock.get_or_insert_with(HashMap::new);

    let mut current_names = std::collections::HashSet::new();
    for dev in other_devices {
        current_names.insert(dev.device.clone());
        map.entry(dev.device.clone())
            .and_modify(|existing| {
                if dev.connected || (!existing.connected && dev.battery > 0) {
                    *existing = dev.clone();
                }
            })
            .or_insert(dev);
    }

    for (name, dev) in map.iter_mut() {
        if !current_names.contains(name) {
            dev.connected = false;
            dev.status = "Disconnected".to_string();
        }
    }

    let mut all_bt: Vec<BluetoothDeviceStatus> = map.values().cloned().collect();
    all_bt.sort_by(|a, b| {
        b.connected.cmp(&a.connected).then_with(|| a.device.cmp(&b.device))
    });

    (final_keychron, all_bt)
}

pub fn get_all_devices() -> AllDevicesStatus {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();

    let hid_devices = enumerate_hid_interfaces();
    let hyperx = read_hyperx_cloud_3(&hid_devices);
    let incott = read_incott_mouse(&hid_devices);
    let (keychron, others) = read_keychron_and_bluetooth();

    AllDevicesStatus {
        timestamp: now,
        hyperx,
        incott,
        keychron,
        other_devices: others,
    }
}
