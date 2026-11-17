#![allow(non_snake_case, non_camel_case_types, dead_code)]

use std::ffi::c_void;

pub type HANDLE = *mut c_void;
pub type HDEVINFO = *mut c_void;
pub type BOOL = i32;
pub type DWORD = u32;
pub type WORD = u16;
pub type BYTE = u8;
pub type ULONG = u32;
pub type USHORT = u16;

pub const INVALID_HANDLE_VALUE: HANDLE = -1isize as HANDLE;
pub const GENERIC_READ: DWORD = 0x80000000;
pub const GENERIC_WRITE: DWORD = 0x40000000;
pub const FILE_SHARE_READ: DWORD = 0x00000001;
pub const FILE_SHARE_WRITE: DWORD = 0x00000002;
pub const OPEN_EXISTING: DWORD = 3;
pub const FILE_FLAG_OVERLAPPED: DWORD = 0x40000000;

pub const DIGCF_PRESENT: DWORD = 0x00000002;
pub const DIGCF_ALLCLASSES: DWORD = 0x00000004;
pub const DIGCF_DEVICEINTERFACE: DWORD = 0x00000010;

pub const WAIT_OBJECT_0: DWORD = 0;
pub const WAIT_TIMEOUT: DWORD = 258;
pub const ERROR_ALREADY_EXISTS: DWORD = 183;

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct GUID {
    pub data1: DWORD,
    pub data2: WORD,
    pub data3: WORD,
    pub data4: [BYTE; 8],
}

impl GUID {
    pub const fn new(d1: DWORD, d2: WORD, d3: WORD, d4: [BYTE; 8]) -> Self {
        Self { data1: d1, data2: d2, data3: d3, data4: d4 }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct DEVPROPKEY {
    pub fmtid: GUID,
    pub pid: ULONG,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SP_DEVINFO_DATA {
    pub cb_size: DWORD,
    pub class_guid: GUID,
    pub dev_inst: DWORD,
    pub reserved: usize,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct SP_DEVICE_INTERFACE_DATA {
    pub cb_size: DWORD,
    pub interface_class_guid: GUID,
    pub flags: DWORD,
    pub reserved: usize,
}

#[repr(C)]
pub struct SP_DEVICE_INTERFACE_DETAIL_DATA_W {
    pub cb_size: DWORD,
    pub device_path: [u16; 1],
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct HIDD_ATTRIBUTES {
    pub size: DWORD,
    pub vendor_id: WORD,
    pub product_id: WORD,
    pub version_number: WORD,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct HIDP_CAPS {
    pub usage: USHORT,
    pub usage_page: USHORT,
    pub input_report_byte_length: USHORT,
    pub output_report_byte_length: USHORT,
    pub feature_report_byte_length: USHORT,
    pub reserved: [USHORT; 17],
    pub number_link_collection_nodes: USHORT,
    pub number_input_button_caps: USHORT,
    pub number_input_value_caps: USHORT,
    pub number_input_data_indices: USHORT,
    pub number_output_button_caps: USHORT,
    pub number_output_value_caps: USHORT,
    pub number_output_data_indices: USHORT,
    pub number_feature_button_caps: USHORT,
    pub number_feature_value_caps: USHORT,
    pub number_feature_data_indices: USHORT,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct OVERLAPPED {
    pub internal: usize,
    pub internal_high: usize,
    pub offset: DWORD,
    pub offset_high: DWORD,
    pub h_event: HANDLE,
}

impl Default for OVERLAPPED {
    fn default() -> Self {
        Self {
            internal: 0,
            internal_high: 0,
            offset: 0,
            offset_high: 0,
            h_event: std::ptr::null_mut(),
        }
    }
}

#[link(name = "hid")]
unsafe extern "system" {
    pub fn HidD_GetHidGuid(guid: *mut GUID);
    pub fn HidD_GetAttributes(handle: HANDLE, attr: *mut HIDD_ATTRIBUTES) -> BOOL;
    pub fn HidD_GetPreparsedData(handle: HANDLE, preparsed: *mut *mut c_void) -> BOOL;
    pub fn HidD_FreePreparsedData(preparsed: *mut c_void) -> BOOL;
    pub fn HidP_GetCaps(preparsed: *mut c_void, caps: *mut HIDP_CAPS) -> i32;
    pub fn HidD_SetOutputReport(handle: HANDLE, report: *const u8, len: ULONG) -> BOOL;
    pub fn HidD_SetFeature(handle: HANDLE, report: *const u8, len: ULONG) -> BOOL;
    pub fn HidD_GetFeature(handle: HANDLE, report: *mut u8, len: ULONG) -> BOOL;
    pub fn HidD_GetProductString(handle: HANDLE, buffer: *mut u16, buffer_len: ULONG) -> BOOL;
}

#[link(name = "setupapi")]
unsafe extern "system" {
    pub fn SetupDiGetClassDevsW(
        class_guid: *const GUID,
        enumerator: *const u16,
        hwnd_parent: HANDLE,
        flags: DWORD,
    ) -> HDEVINFO;
    pub fn SetupDiEnumDeviceInfo(
        device_info_set: HDEVINFO,
        member_index: DWORD,
        device_info_data: *mut SP_DEVINFO_DATA,
    ) -> BOOL;
    pub fn SetupDiEnumDeviceInterfaces(
        device_info_set: HDEVINFO,
        device_info_data: *mut SP_DEVINFO_DATA,
        interface_class_guid: *const GUID,
        member_index: DWORD,
        device_interface_data: *mut SP_DEVICE_INTERFACE_DATA,
    ) -> BOOL;
    pub fn SetupDiGetDeviceInterfaceDetailW(
        device_info_set: HDEVINFO,
        device_info_data: *mut SP_DEVICE_INTERFACE_DATA,
        device_interface_detail_data: *mut u8,
        device_interface_detail_data_size: DWORD,
        required_size: *mut DWORD,
        device_info_data_out: *mut SP_DEVINFO_DATA,
    ) -> BOOL;
    pub fn SetupDiGetDevicePropertyW(
        device_info_set: HDEVINFO,
        device_info_data: *const SP_DEVINFO_DATA,
        property_key: *const DEVPROPKEY,
        property_type: *mut ULONG,
        property_buffer: *mut u8,
        property_buffer_size: DWORD,
        required_size: *mut DWORD,
        flags: DWORD,
    ) -> BOOL;
    pub fn SetupDiDestroyDeviceInfoList(device_info_set: HDEVINFO) -> BOOL;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    pub fn CreateFileW(
        file_name: *const u16,
        desired_access: DWORD,
        share_mode: DWORD,
        security_attributes: *mut c_void,
        creation_disposition: DWORD,
        flags_and_attributes: DWORD,
        template_file: HANDLE,
    ) -> HANDLE;
    pub fn CloseHandle(handle: HANDLE) -> BOOL;
    pub fn ReadFile(
        handle: HANDLE,
        buffer: *mut u8,
        number_of_bytes_to_read: DWORD,
        number_of_bytes_read: *mut DWORD,
        overlapped: *mut OVERLAPPED,
    ) -> BOOL;
    pub fn WriteFile(
        handle: HANDLE,
        buffer: *const u8,
        number_of_bytes_to_write: DWORD,
        number_of_bytes_written: *mut DWORD,
        overlapped: *mut OVERLAPPED,
    ) -> BOOL;
    pub fn CreateEventW(
        event_attributes: *mut c_void,
        manual_reset: BOOL,
        initial_state: BOOL,
        name: *const u16,
    ) -> HANDLE;
    pub fn WaitForSingleObject(handle: HANDLE, milliseconds: DWORD) -> DWORD;
    pub fn GetOverlappedResult(
        handle: HANDLE,
        overlapped: *mut OVERLAPPED,
        number_of_bytes_transferred: *mut DWORD,
        wait: BOOL,
    ) -> BOOL;
    pub fn CancelIo(handle: HANDLE) -> BOOL;
    pub fn GetLastError() -> DWORD;
    pub fn CreateMutexW(
        mutex_attributes: *mut c_void,
        initial_owner: BOOL,
        name: *const u16,
    ) -> HANDLE;
}
