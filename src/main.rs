#![windows_subsystem = "windows"]

mod scanner;
mod windows_ffi;

use std::collections::HashSet;
use std::f32::consts::{PI, TAU};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Color32, CornerRadius, Margin, Painter, Pos2, Rect, RichText, Shape, Stroke, StrokeKind,
    TextWrapMode, Ui, Vec2,
};
use notify_rust::Notification;
use scanner::{AllDevicesStatus, BluetoothDeviceStatus};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
};

// -----------------------------------------------------------------------------
// Theme Palette (Neutral Obsidian / Linear-inspired)
// -----------------------------------------------------------------------------

const BG_WINDOW: Color32 = Color32::from_rgb(12, 13, 17);
const BG_CARD: Color32 = Color32::from_rgb(20, 22, 28);
const BG_CARD_HOVER: Color32 = Color32::from_rgb(26, 29, 37);
const BG_CARD_OFFLINE: Color32 = Color32::from_rgb(15, 16, 21);

const BORDER_WINDOW: Color32 = Color32::from_rgb(36, 39, 48);
const BORDER_CARD: Color32 = Color32::from_rgb(32, 35, 44);
const BORDER_CARD_HOVER: Color32 = Color32::from_rgb(50, 55, 68);
const BORDER_OFFLINE: Color32 = Color32::from_rgb(23, 25, 32);
const DIVIDER: Color32 = Color32::from_rgb(26, 28, 36);

const TEXT_PRIMARY: Color32 = Color32::from_rgb(244, 244, 246);
const TEXT_SECONDARY: Color32 = Color32::from_rgb(138, 145, 160);
const TEXT_MUTED: Color32 = Color32::from_rgb(82, 88, 102);

const ACCENT_GREEN: Color32 = Color32::from_rgb(34, 197, 94);
const ACCENT_AMBER: Color32 = Color32::from_rgb(245, 158, 11);
const ACCENT_RED: Color32 = Color32::from_rgb(239, 68, 68);
const ACCENT_CYAN: Color32 = Color32::from_rgb(56, 189, 248);
const ACCENT_INDIGO: Color32 = Color32::from_rgb(129, 140, 248);

// -----------------------------------------------------------------------------
// State & Models
// -----------------------------------------------------------------------------

struct SharedState {
    status: Option<AllDevicesStatus>,
    last_scanned: Option<Instant>,
    is_scanning: bool,
}

#[derive(Clone, Copy)]
enum DeviceKind {
    Headset,
    Mouse,
    Keyboard,
    Bluetooth,
}

struct DeviceRowModel {
    kind: DeviceKind,
    name: String,
    subtitle: String,
    battery: Option<u8>,
    charging: bool,
    status_badge: Option<String>,
    tooltip: Option<String>,
    connected: bool,
}

struct BatteryApp {
    state: Arc<Mutex<SharedState>>,
    trigger_tx: Sender<()>,
    _tray_icon: TrayIcon,
    menu_show_id: muda::MenuId,
    menu_scan_id: muda::MenuId,
    menu_quit_id: muda::MenuId,
    notified_low_devices: HashSet<String>,
    window_visible: bool,
    quitting: bool,
    app_icon_texture: egui::TextureHandle,
}

fn create_tray_icon() -> Icon {
    let rgba = include_bytes!("../tray_icon_32.rgba").to_vec();
    Icon::from_rgba(rgba, 32, 32).unwrap()
}

fn notify_low_battery(device_name: &str, percentage: u8) {
    let _ = Notification::new()
        .summary("Low Peripheral Battery")
        .body(&format!(
            "{} is down to {}%. Please connect to charger.",
            device_name, percentage
        ))
        .appname("perimon")
        .show();
}

impl BatteryApp {
    fn new(cc: &eframe::CreationContext, initial_visible: bool) -> Self {
        // Load Windows Native Segoe UI Fonts
        let mut fonts = egui::FontDefinitions::default();
        let mut has_segoe = false;

        if let Ok(data) = std::fs::read(r"C:\Windows\Fonts\segoeui.ttf") {
            fonts.font_data.insert(
                "segoe_ui".to_owned(),
                Arc::new(egui::FontData::from_owned(data)),
            );
            if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                family.insert(0, "segoe_ui".to_owned());
                has_segoe = true;
            }
        }

        if let Ok(data) = std::fs::read(r"C:\Windows\Fonts\seguisb.ttf") {
            fonts.font_data.insert(
                "segoe_ui_semibold".to_owned(),
                Arc::new(egui::FontData::from_owned(data)),
            );
        }

        if has_segoe {
            cc.egui_ctx.set_fonts(fonts);
        }

        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = BG_WINDOW;
        visuals.window_fill = BG_WINDOW;
        visuals.extreme_bg_color = BG_WINDOW;
        visuals.faint_bg_color = BG_CARD;
        visuals.override_text_color = Some(TEXT_PRIMARY);
        visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(56, 189, 248, 45);
        cc.egui_ctx.set_visuals(visuals);

        let state = Arc::new(Mutex::new(SharedState {
            status: None,
            last_scanned: None,
            is_scanning: true,
        }));

        let (trigger_tx, trigger_rx) = channel::<()>();

        // System Tray Context Menu
        let tray_menu = Menu::new();
        let show_item = MenuItem::new("Show Window", true, None);
        let scan_item = MenuItem::new("Scan Now", true, None);
        let quit_item = MenuItem::new("Exit", true, None);

        let menu_show_id = show_item.id().clone();
        let menu_scan_id = scan_item.id().clone();
        let menu_quit_id = quit_item.id().clone();

        let _ = tray_menu.append(&show_item);
        let _ = tray_menu.append(&scan_item);
        let _ = tray_menu.append(&PredefinedMenuItem::separator());
        let _ = tray_menu.append(&quit_item);

        let tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(tray_menu))
            .with_tooltip("perimon - Peripheral Battery Monitor")
            .with_icon(create_tray_icon())
            .build()
            .expect("Failed to initialize system tray icon");

        // Background hardware scanner thread
        let bg_state = Arc::clone(&state);
        let egui_ctx = cc.egui_ctx.clone();

        thread::spawn(move || loop {
            {
                let mut s = bg_state.lock().unwrap();
                s.is_scanning = true;
            }
            egui_ctx.request_repaint();

            let result = scanner::get_all_devices();

            {
                let mut s = bg_state.lock().unwrap();
                s.status = Some(result);
                s.last_scanned = Some(Instant::now());
                s.is_scanning = false;
            }
            egui_ctx.request_repaint();

            let _ = trigger_rx.recv_timeout(Duration::from_secs(5));
        });

        let icon_bytes = include_bytes!("../app_icon_128.rgba");
        let icon_img = egui::ColorImage::from_rgba_unmultiplied([128, 128], icon_bytes);
        let app_icon_texture =
            cc.egui_ctx
                .load_texture("app_icon", icon_img, egui::TextureOptions::LINEAR);

        Self {
            state,
            trigger_tx,
            _tray_icon: tray_icon,
            menu_show_id,
            menu_scan_id,
            menu_quit_id,
            notified_low_devices: HashSet::new(),
            window_visible: initial_visible,
            quitting: false,
            app_icon_texture,
        }
    }
}

// -----------------------------------------------------------------------------
// Device Name Cleanup & Deduplication
// -----------------------------------------------------------------------------

fn clean_device_name(raw: &str) -> String {
    let mut name = raw.trim().to_string();
    for suffix in &[
        " Hands-Free AG",
        " Hands-Free",
        " Avrcp Transport",
        " Bluetooth Audio",
        " Stereo",
    ] {
        if let Some(pos) = name.find(suffix) {
            name = name[..pos].trim().to_string();
        }
    }
    if name.starts_with("ATH-SR30BT") {
        return "Audio-Technica ATH-SR30BT".to_string();
    }
    name
}

fn deduplicate_bluetooth_devices(devices: &[BluetoothDeviceStatus]) -> Vec<BluetoothDeviceStatus> {
    let mut map: std::collections::HashMap<String, BluetoothDeviceStatus> =
        std::collections::HashMap::new();

    for dev in devices {
        let clean_name = clean_device_name(&dev.device);
        let mut d = dev.clone();
        d.device = clean_name.clone();

        map.entry(clean_name)
            .and_modify(|existing| {
                if d.connected || (!existing.connected && d.battery > 0) {
                    *existing = d.clone();
                }
            })
            .or_insert(d);
    }

    let mut list: Vec<BluetoothDeviceStatus> = map.into_values().collect();
    list.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then_with(|| a.device.cmp(&b.device))
    });
    list
}

fn infer_device_kind(name: &str) -> DeviceKind {
    let lower = name.to_lowercase();
    if lower.contains("head")
        || lower.contains("moondrop")
        || lower.contains("ath")
        || lower.contains("ear")
        || lower.contains("pods")
        || lower.contains("buds")
        || lower.contains("blackshark")
        || lower.contains("cloud")
    {
        DeviceKind::Headset
    } else if lower.contains("mouse")
        || lower.contains("master")
        || lower.contains("viper")
        || lower.contains("deathadder")
        || lower.contains("superlight")
        || lower.contains("g pro")
        || lower.contains("basilisk")
    {
        DeviceKind::Mouse
    } else if lower.contains("key") || lower.contains("board") {
        DeviceKind::Keyboard
    } else {
        DeviceKind::Bluetooth
    }
}

fn build_device_rows(status: &AllDevicesStatus) -> Vec<DeviceRowModel> {
    let mut rows = Vec::new();

    // 1. HyperX Headset
    let is_hx_active = status.hyperx.connected;
    let hx_sub = if let Some(mv) = status.hyperx.voltage_mv {
        format!("2.4 GHz · {:.2} V", mv as f32 / 1000.0)
    } else if status.hyperx.dongle_connected {
        "2.4 GHz Dongle Online".to_string()
    } else {
        "Dongle Not Found".to_string()
    };

    let hx_badge = if !is_hx_active {
        Some(if status.hyperx.dongle_connected {
            "Asleep".to_string()
        } else {
            "Offline".to_string()
        })
    } else {
        None
    };

    rows.push(DeviceRowModel {
        kind: DeviceKind::Headset,
        name: "HyperX Cloud III".to_string(),
        subtitle: hx_sub,
        battery: status.hyperx.battery,
        charging: false,
        status_badge: hx_badge,
        tooltip: None,
        connected: is_hx_active,
    });

    // 2. Incott G23 v2 Mouse
    let is_mouse_active = status.incott.connected;
    let mut mouse_parts = vec!["2.4 GHz".to_string()];
    if let Some(dpi) = status.incott.dpi {
        mouse_parts.push(format!("{} DPI", dpi));
    }
    if let Some(hz) = status.incott.polling_rate {
        mouse_parts.push(format!("{} Hz", hz));
    }

    let mouse_badge = if !is_mouse_active {
        Some(if status.incott.dongle_connected {
            "Standby".to_string()
        } else {
            "Offline".to_string()
        })
    } else {
        None
    };

    rows.push(DeviceRowModel {
        kind: DeviceKind::Mouse,
        name: "Incott G23 v2".to_string(),
        subtitle: mouse_parts.join(" · "),
        battery: status.incott.battery,
        charging: status.incott.charging,
        status_badge: mouse_badge,
        tooltip: None,
        connected: is_mouse_active,
    });

    // 3. Keychron Keyboard
    let is_kc_active = status.keychron.connected;
    let kc_sub = if is_kc_active {
        status.keychron.connection_type.clone()
    } else {
        "Disconnected".to_string()
    };

    let (kc_bat, kc_badge, kc_tip) = if let Some(bat) = status.keychron.battery {
        (Some(bat), None, None)
    } else if is_kc_active {
        (
            None,
            Some("2.4G Link".to_string()),
            Some(
                "Keychron 2.4G dongles do not expose battery over USB HID.\nTip: Press Fn + B for physical LED level, or switch to Bluetooth for live %."
                    .to_string(),
            ),
        )
    } else {
        (None, Some("Offline".to_string()), None)
    };

    rows.push(DeviceRowModel {
        kind: DeviceKind::Keyboard,
        name: "Keychron Keyboard".to_string(),
        subtitle: kc_sub,
        battery: kc_bat,
        charging: false,
        status_badge: kc_badge,
        tooltip: kc_tip,
        connected: is_kc_active,
    });

    // 4. Extra 2.4GHz HID Wireless Devices (Logitech LIGHTSPEED / Razer HyperSpeed)
    for dev in &status.extra_hid_devices {
        rows.push(DeviceRowModel {
            kind: infer_device_kind(&dev.device),
            name: dev.device.clone(),
            subtitle: dev.connection_type.clone(),
            battery: dev.battery,
            charging: dev.charging,
            status_badge: if dev.connected {
                None
            } else {
                Some("Offline".to_string())
            },
            tooltip: None,
            connected: dev.connected,
        });
    }

    // 5. Bluetooth Devices
    for dev in deduplicate_bluetooth_devices(&status.other_devices) {
        let kind = infer_device_kind(&dev.device);

        let sub = if dev.connected {
            "Bluetooth".to_string()
        } else {
            "Bluetooth · Disconnected".to_string()
        };

        let (bat, badge) = if dev.connected {
            (Some(dev.battery), None)
        } else {
            (None, Some("Offline".to_string()))
        };

        rows.push(DeviceRowModel {
            kind,
            name: dev.device,
            subtitle: sub,
            battery: bat,
            charging: false,
            status_badge: badge,
            tooltip: None,
            connected: dev.connected,
        });
    }

    rows
}

// -----------------------------------------------------------------------------
// Crisp Vector Icons (Painter-drawn, DPI-independent)
// -----------------------------------------------------------------------------

fn draw_device_icon(painter: &Painter, center: Pos2, kind: DeviceKind, color: Color32) {
    let stroke = Stroke::new(1.5, color);

    match kind {
        DeviceKind::Headset => {
            // Headband arc
            let r = 6.5;
            let arc_center = Pos2::new(center.x, center.y + 0.5);
            let steps = 12;
            let mut pts = Vec::with_capacity(steps + 1);
            for i in 0..=steps {
                let t = PI + (i as f32 / steps as f32) * PI;
                pts.push(Pos2::new(
                    arc_center.x + r * t.cos(),
                    arc_center.y + r * t.sin(),
                ));
            }
            painter.add(Shape::line(pts, stroke));

            // Left & right earcups
            let left_cup =
                Rect::from_center_size(Pos2::new(center.x - 6.2, center.y + 3.0), Vec2::new(3.6, 6.4));
            let right_cup =
                Rect::from_center_size(Pos2::new(center.x + 6.2, center.y + 3.0), Vec2::new(3.6, 6.4));
            painter.rect_filled(left_cup, CornerRadius::same(2), color);
            painter.rect_filled(right_cup, CornerRadius::same(2), color);
        }
        DeviceKind::Mouse => {
            let body = Rect::from_center_size(center, Vec2::new(10.5, 15.0));
            painter.rect_stroke(body, CornerRadius::same(5), stroke, StrokeKind::Middle);
            // Scroll wheel tick
            painter.line_segment(
                [
                    Pos2::new(center.x, center.y - 4.2),
                    Pos2::new(center.x, center.y - 1.2),
                ],
                Stroke::new(1.6, color),
            );
        }
        DeviceKind::Keyboard => {
            let body = Rect::from_center_size(center, Vec2::new(16.0, 10.5));
            painter.rect_stroke(body, CornerRadius::same(2), stroke, StrokeKind::Middle);
            // Keycap dots (top row)
            for dx in [-4.5, -1.5, 1.5, 4.5] {
                painter.rect_filled(
                    Rect::from_center_size(
                        Pos2::new(center.x + dx, center.y - 1.8),
                        Vec2::new(1.8, 1.6),
                    ),
                    CornerRadius::ZERO,
                    color,
                );
            }
            // Spacebar row
            painter.rect_filled(
                Rect::from_center_size(Pos2::new(center.x, center.y + 2.0), Vec2::new(8.0, 1.6)),
                CornerRadius::ZERO,
                color,
            );
        }
        DeviceKind::Bluetooth => {
            let s = Stroke::new(1.4, color);
            let top = Pos2::new(center.x, center.y - 6.5);
            let bot = Pos2::new(center.x, center.y + 6.5);
            let r_top = Pos2::new(center.x + 4.2, center.y - 3.2);
            let r_bot = Pos2::new(center.x + 4.2, center.y + 3.2);
            let l_top = Pos2::new(center.x - 4.0, center.y - 3.0);
            let l_bot = Pos2::new(center.x - 4.0, center.y + 3.0);

            painter.add(Shape::line(vec![l_top, r_bot, bot, top, r_top, l_bot], s));
        }
    }
}

fn draw_refresh_icon(painter: &Painter, center: Pos2, color: Color32, rotation: f32) {
    let r = 5.2;
    let steps = 14;
    let start_angle = rotation;
    let sweep = TAU * 0.72;
    let mut pts = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
        let a = start_angle + (i as f32 / steps as f32) * sweep;
        pts.push(Pos2::new(center.x + r * a.cos(), center.y + r * a.sin()));
    }
    let end_pt = *pts.last().unwrap();
    let end_angle = start_angle + sweep;
    painter.add(Shape::line(pts, Stroke::new(1.45, color)));

    // Arrowhead at end_pt tangent
    let tangent = Vec2::new(-end_angle.sin(), end_angle.cos());
    let normal = Vec2::new(end_angle.cos(), end_angle.sin());
    let tip = end_pt + tangent * 1.4;
    let wing1 = end_pt - tangent * 2.2 + normal * 2.2;
    let wing2 = end_pt - tangent * 2.2 - normal * 2.2;
    painter.line_segment([wing1, tip], Stroke::new(1.45, color));
    painter.line_segment([wing2, tip], Stroke::new(1.45, color));
}

fn draw_minimize_icon(painter: &Painter, center: Pos2, color: Color32) {
    let y = center.y.round() + 0.5;
    painter.line_segment(
        [Pos2::new(center.x - 4.5, y), Pos2::new(center.x + 4.5, y)],
        Stroke::new(1.4, color),
    );
}

fn draw_close_icon(painter: &Painter, center: Pos2, color: Color32) {
    let d = 4.0;
    let s = Stroke::new(1.45, color);
    painter.line_segment(
        [
            Pos2::new(center.x - d, center.y - d),
            Pos2::new(center.x + d, center.y + d),
        ],
        s,
    );
    painter.line_segment(
        [
            Pos2::new(center.x + d, center.y - d),
            Pos2::new(center.x - d, center.y + d),
        ],
        s,
    );
}

fn draw_charging_bolt(painter: &Painter, center: Pos2, color: Color32) {
    let pts = vec![
        Pos2::new(center.x + 1.0, center.y - 5.5),
        Pos2::new(center.x - 3.2, center.y + 0.5),
        Pos2::new(center.x - 0.3, center.y + 0.5),
        Pos2::new(center.x - 1.2, center.y + 5.5),
        Pos2::new(center.x + 3.2, center.y - 0.5),
        Pos2::new(center.x + 0.3, center.y - 0.5),
    ];
    painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
}

// -----------------------------------------------------------------------------
// Battery Color & Row Rendering
// -----------------------------------------------------------------------------

fn battery_accent(pct: u8, charging: bool) -> Color32 {
    if charging {
        ACCENT_CYAN
    } else if pct >= 50 {
        ACCENT_GREEN
    } else if pct >= 20 {
        ACCENT_AMBER
    } else {
        ACCENT_RED
    }
}

fn render_device_card(ui: &mut Ui, dev: &DeviceRowModel) {
    let is_active = dev.connected;
    let card_h = if is_active { 58.0 } else { 40.0 };
    let desired_size = Vec2::new(ui.available_width(), card_h);
    let (rect, resp) = ui.allocate_exact_size(desired_size, egui::Sense::hover());

    let accent = match dev.battery {
        Some(pct) if is_active => battery_accent(pct, dev.charging),
        _ if is_active => ACCENT_INDIGO,
        _ => TEXT_MUTED,
    };

    let (bg, border) = if !is_active {
        (BG_CARD_OFFLINE, BORDER_OFFLINE)
    } else if resp.hovered() {
        (BG_CARD_HOVER, BORDER_CARD_HOVER)
    } else {
        (BG_CARD, BORDER_CARD)
    };

    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(10),
        bg,
        Stroke::new(1.0, border),
        StrokeKind::Inside,
    );

    // 1. Left Icon Well
    let well_size = if is_active { 34.0 } else { 24.0 };
    let well_rect = Rect::from_center_size(
        Pos2::new(rect.min.x + 13.0 + well_size * 0.5, rect.center().y),
        Vec2::splat(well_size),
    );
    let (well_bg, well_stroke, icon_color) = if is_active {
        (
            Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 22),
            Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 45),
            Color32::from_rgb(235, 238, 245),
        )
    } else {
        (
            Color32::from_rgb(20, 22, 28),
            Color32::from_rgb(28, 31, 39),
            TEXT_MUTED,
        )
    };
    painter.rect(
        well_rect,
        CornerRadius::same(8),
        well_bg,
        Stroke::new(1.0, well_stroke),
        StrokeKind::Inside,
    );
    draw_device_icon(painter, well_rect.center(), dev.kind, icon_color);

    // 2. Right Column Width & Text Column Bounds
    let right_col_w = if dev.battery.is_some() { 68.0 } else { 78.0 };
    let text_left = well_rect.max.x + 12.0;
    let text_right = (rect.max.x - 14.0 - right_col_w - 8.0).max(text_left + 40.0);
    let text_rect = Rect::from_min_max(
        Pos2::new(text_left, rect.min.y + 6.0),
        Pos2::new(text_right, rect.max.y - 6.0),
    );

    // Render Name + Subtitle inside bounded child UI (truncated cleanly, never wraps)
    {
        let mut text_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(text_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        if is_active {
            text_ui.add_space(4.0);
            text_ui.add(
                egui::Label::new(
                    RichText::new(&dev.name)
                        .size(13.5)
                        .strong()
                        .color(TEXT_PRIMARY),
                )
                .wrap_mode(TextWrapMode::Truncate),
            );
            text_ui.add_space(1.0);
            text_ui.add(
                egui::Label::new(
                    RichText::new(&dev.subtitle)
                        .size(11.0)
                        .color(TEXT_SECONDARY),
                )
                .wrap_mode(TextWrapMode::Truncate),
            );
        } else {
            text_ui.add_space(5.0);
            text_ui.add(
                egui::Label::new(
                    RichText::new(&dev.name)
                        .size(12.5)
                        .color(Color32::from_rgb(108, 114, 128)),
                )
                .wrap_mode(TextWrapMode::Truncate),
            );
        }
    }

    // 3. Right Side: Battery % + Progress Bar OR Status Pill
    let painter = ui.painter();
    let right_x = rect.max.x - 14.0;
    if let Some(pct) = dev.battery {
        let pct_text = format!("{}%", pct);
        let pct_color = if !is_active {
            TEXT_MUTED
        } else if dev.charging {
            ACCENT_CYAN
        } else if pct < 20 {
            ACCENT_RED
        } else if pct < 50 {
            ACCENT_AMBER
        } else {
            TEXT_PRIMARY
        };

        let text_y = rect.center().y - 6.0;
        let galley = painter.layout_no_wrap(
            pct_text,
            egui::FontId::proportional(14.5),
            pct_color,
        );
        let text_pos = Pos2::new(right_x - galley.size().x, text_y - galley.size().y * 0.5);

        if dev.charging && is_active {
            draw_charging_bolt(
                painter,
                Pos2::new(text_pos.x - 7.0, text_y),
                ACCENT_CYAN,
            );
        }
        painter.galley(text_pos, galley, pct_color);

        // Sleek horizontal progress bar beneath percentage
        let bar_w = 54.0;
        let bar_h = 4.0;
        let bar_rect = Rect::from_min_size(
            Pos2::new(right_x - bar_w, rect.center().y + 7.0),
            Vec2::new(bar_w, bar_h),
        );
        painter.rect_filled(
            bar_rect,
            CornerRadius::same(2),
            Color32::from_rgb(34, 38, 48),
        );

        let fill_w = (bar_w * (pct as f32 / 100.0)).clamp(3.0, bar_w);
        let fill_rect = Rect::from_min_size(bar_rect.min, Vec2::new(fill_w, bar_h));
        painter.rect_filled(fill_rect, CornerRadius::same(2), accent);
    } else if let Some(ref badge) = dev.status_badge {
        if is_active {
            let pill_bg = Color32::from_rgba_unmultiplied(ACCENT_INDIGO.r(), ACCENT_INDIGO.g(), ACCENT_INDIGO.b(), 20);
            let pill_stroke = Color32::from_rgba_unmultiplied(ACCENT_INDIGO.r(), ACCENT_INDIGO.g(), ACCENT_INDIGO.b(), 48);
            let pill_fg = Color32::from_rgb(165, 180, 252);

            let galley = painter.layout_no_wrap(
                badge.clone(),
                egui::FontId::proportional(11.0),
                pill_fg,
            );
            let pill_w = galley.size().x + 16.0;
            let pill_h = 22.0;
            let pill_rect = Rect::from_min_size(
                Pos2::new(right_x - pill_w, rect.center().y - pill_h * 0.5),
                Vec2::new(pill_w, pill_h),
            );
            painter.rect(
                pill_rect,
                CornerRadius::same(6),
                pill_bg,
                Stroke::new(1.0, pill_stroke),
                StrokeKind::Inside,
            );
            painter.galley(
                Pos2::new(
                    pill_rect.center().x - galley.size().x * 0.5,
                    pill_rect.center().y - galley.size().y * 0.5,
                ),
                galley,
                pill_fg,
            );
        } else {
            painter.text(
                Pos2::new(right_x, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                badge,
                egui::FontId::proportional(11.0),
                TEXT_MUTED,
            );
        }
    }

    if let Some(ref tip) = dev.tooltip {
        resp.on_hover_text(tip);
    }
}

// -----------------------------------------------------------------------------
// Main App UI
// -----------------------------------------------------------------------------

impl eframe::App for BatteryApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let state_guard = self.state.lock().unwrap();
        let is_scanning = state_guard.is_scanning;
        let last_scanned = state_guard.last_scanned;
        let status_opt = state_guard.status.clone();
        drop(state_guard);

        // Smooth animation while scanning, 1s tick otherwise
        if is_scanning {
            ui.ctx().request_repaint_after(Duration::from_millis(33));
        } else {
            ui.ctx().request_repaint_after(Duration::from_secs(1));
        }

        // 1. Minimize-To-Tray on Window Close
        if ui.input(|i| i.viewport().close_requested()) && !self.quitting {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Visible(false));
            self.window_visible = false;
        }

        // 2. Tray Icon Left-Click Toggle
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                self.window_visible = !self.window_visible;
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Visible(self.window_visible));
                if self.window_visible {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Focus);
                }
            }
        }

        // 3. Tray Context Menu
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id == self.menu_show_id {
                self.window_visible = true;
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Focus);
            } else if event.id == self.menu_scan_id {
                let _ = self.trigger_tx.send(());
            } else if event.id == self.menu_quit_id {
                self.quitting = true;
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }

        // 4. Low Battery Toast Alerts
        if let Some(ref status) = status_opt {
            if status.hyperx.connected {
                if let Some(bat) = status.hyperx.battery {
                    let name = "HyperX Cloud III";
                    if bat <= 20 && bat > 0 {
                        if self.notified_low_devices.insert(name.to_string()) {
                            notify_low_battery(name, bat);
                        }
                    } else if bat > 30 {
                        self.notified_low_devices.remove(name);
                    }
                }
            }

            if status.incott.connected {
                if let Some(bat) = status.incott.battery {
                    let name = "Incott G23 v2";
                    if bat <= 20 && bat > 0 && !status.incott.charging {
                        if self.notified_low_devices.insert(name.to_string()) {
                            notify_low_battery(name, bat);
                        }
                    } else if bat > 30 || status.incott.charging {
                        self.notified_low_devices.remove(name);
                    }
                }
            }

            for dev in &status.extra_hid_devices {
                if dev.connected {
                    if let Some(bat) = dev.battery {
                        if bat <= 20 && bat > 0 && !dev.charging {
                            if self.notified_low_devices.insert(dev.device.clone()) {
                                notify_low_battery(&dev.device, bat);
                            }
                        } else if bat > 30 || dev.charging {
                            self.notified_low_devices.remove(&dev.device);
                        }
                    }
                }
            }

            for dev in &status.other_devices {
                if dev.connected {
                    let clean = clean_device_name(&dev.device);
                    if dev.battery <= 20 && dev.battery > 0 {
                        if self.notified_low_devices.insert(clean.clone()) {
                            notify_low_battery(&clean, dev.battery);
                        }
                    } else if dev.battery > 30 {
                        self.notified_low_devices.remove(&clean);
                    }
                }
            }
        }

        // 5. Outer Frameless Window Shell
        let outer_rect = ui.max_rect();
        ui.painter().rect(
            outer_rect,
            CornerRadius::ZERO,
            BG_WINDOW,
            Stroke::new(1.0, BORDER_WINDOW),
            StrokeKind::Inside,
        );

        let content_rect = outer_rect.shrink(1.0);
        let mut root_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(content_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );

        // --- UNIFIED HEADER (Single bar, draggable) ---
        let header_h = 56.0;
        let (header_rect, header_resp) = root_ui.allocate_exact_size(
            Vec2::new(content_rect.width(), header_h),
            egui::Sense::click_and_drag(),
        );
        if header_resp.drag_started() {
            root_ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }

        // Header Left: Logo + Title + Live Subtitle
        let logo_rect = Rect::from_min_size(
            Pos2::new(header_rect.min.x + 16.0, header_rect.center().y - 14.0),
            Vec2::splat(28.0),
        );
        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        root_ui.painter().image(
            self.app_icon_texture.id(),
            logo_rect,
            uv,
            Color32::WHITE,
        );

        let title_pos = Pos2::new(logo_rect.max.x + 10.0, header_rect.center().y - 16.5);
        root_ui.painter().text(
            title_pos,
            egui::Align2::LEFT_TOP,
            "perimon",
            egui::FontId::proportional(14.5),
            TEXT_PRIMARY,
        );

        let (dot_color, status_text) = if is_scanning {
            (ACCENT_CYAN, "Scanning hardware...".to_string())
        } else if let Some(t) = last_scanned {
            let secs = t.elapsed().as_secs();
            let ago = if secs == 0 {
                "Updated just now".to_string()
            } else {
                format!("Updated {}s ago", secs)
            };
            (ACCENT_GREEN, ago)
        } else {
            (ACCENT_AMBER, "Starting...".to_string())
        };

        let sub_y = header_rect.center().y + 10.0;
        root_ui.painter().circle_filled(
            Pos2::new(logo_rect.max.x + 13.5, sub_y),
            3.0,
            dot_color,
        );
        root_ui.painter().text(
            Pos2::new(logo_rect.max.x + 21.0, sub_y),
            egui::Align2::LEFT_CENTER,
            status_text,
            egui::FontId::proportional(11.0),
            TEXT_SECONDARY,
        );

        // Header Right: Refresh, Minimize, Close Icon Buttons
        let btn_size = Vec2::splat(28.0);
        let close_rect = Rect::from_min_size(
            Pos2::new(header_rect.max.x - 14.0 - btn_size.x, header_rect.center().y - 14.0),
            btn_size,
        );
        let min_rect = Rect::from_min_size(
            Pos2::new(close_rect.min.x - 4.0 - btn_size.x, header_rect.center().y - 14.0),
            btn_size,
        );
        let refresh_rect = Rect::from_min_size(
            Pos2::new(min_rect.min.x - 4.0 - btn_size.x, header_rect.center().y - 14.0),
            btn_size,
        );

        let refresh_resp = root_ui
            .interact(refresh_rect, root_ui.scope_id().with("btn_refresh"), egui::Sense::click())
            .on_hover_text("Scan peripherals now");
        if refresh_resp.hovered() && !is_scanning {
            root_ui.painter().rect_filled(refresh_rect, CornerRadius::same(7), BG_CARD_HOVER);
        }
        let rot = if is_scanning {
            (root_ui.input(|i| i.time) * 6.0) as f32
        } else {
            -0.5
        };
        draw_refresh_icon(
            root_ui.painter(),
            refresh_rect.center(),
            if is_scanning {
                ACCENT_CYAN
            } else if refresh_resp.hovered() {
                TEXT_PRIMARY
            } else {
                TEXT_SECONDARY
            },
            rot,
        );
        if refresh_resp.clicked() && !is_scanning {
            let _ = self.trigger_tx.send(());
        }

        let min_resp = root_ui
            .interact(min_rect, root_ui.scope_id().with("btn_min"), egui::Sense::click())
            .on_hover_text("Minimize");
        if min_resp.hovered() {
            root_ui.painter().rect_filled(min_rect, CornerRadius::same(7), BG_CARD_HOVER);
        }
        draw_minimize_icon(
            root_ui.painter(),
            min_rect.center(),
            if min_resp.hovered() { TEXT_PRIMARY } else { TEXT_SECONDARY },
        );
        if min_resp.clicked() {
            root_ui.ctx().send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }

        let close_resp = root_ui
            .interact(close_rect, root_ui.scope_id().with("btn_close"), egui::Sense::click())
            .on_hover_text("Hide to system tray");
        if close_resp.hovered() {
            root_ui.painter().rect_filled(
                close_rect,
                CornerRadius::same(7),
                Color32::from_rgb(225, 29, 72),
            );
        }
        draw_close_icon(
            root_ui.painter(),
            close_rect.center(),
            if close_resp.hovered() { Color32::WHITE } else { TEXT_SECONDARY },
        );
        if close_resp.clicked() {
            root_ui.ctx().send_viewport_cmd(egui::ViewportCommand::Visible(false));
            self.window_visible = false;
        }

        // Top Divider
        let div_y = header_rect.max.y;
        root_ui.painter().line_segment(
            [Pos2::new(content_rect.min.x, div_y), Pos2::new(content_rect.max.x, div_y)],
            Stroke::new(1.0, DIVIDER),
        );

        // --- BODY & FOOTER SPLIT ---
        let footer_h = 30.0;
        let body_rect = Rect::from_min_max(
            Pos2::new(content_rect.min.x, div_y + 1.0),
            Pos2::new(content_rect.max.x, content_rect.max.y - footer_h - 1.0),
        );

        let mut body_ui = root_ui.new_child(
            egui::UiBuilder::new()
                .max_rect(body_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );

        let (active_count, total_count) = if let Some(ref status) = status_opt {
            let rows = build_device_rows(status);
            let total = rows.len();
            let active: Vec<DeviceRowModel> =
                rows.iter().filter(|r| r.connected).map(|r| DeviceRowModel {
                    kind: r.kind,
                    name: r.name.clone(),
                    subtitle: r.subtitle.clone(),
                    battery: r.battery,
                    charging: r.charging,
                    status_badge: r.status_badge.clone(),
                    tooltip: r.tooltip.clone(),
                    connected: r.connected,
                }).collect();
            let active_len = active.len();
            let offline: Vec<DeviceRowModel> =
                rows.into_iter().filter(|r| !r.connected).collect();

            egui::Frame::NONE
                .inner_margin(Margin::symmetric(14, 12))
                .show(&mut body_ui, |ui| {
                    egui::ScrollArea::vertical()
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for (idx, dev) in active.iter().enumerate() {
                                render_device_card(ui, dev);
                                if idx + 1 < active.len() {
                                    ui.add_space(7.0);
                                }
                            }

                            if !offline.is_empty() {
                                if !active.is_empty() {
                                    ui.add_space(14.0);
                                }
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(format!("OFFLINE · {}", offline.len()))
                                            .size(10.5)
                                            .strong()
                                            .color(TEXT_MUTED),
                                    );
                                });
                                ui.add_space(6.0);

                                for (idx, dev) in offline.iter().enumerate() {
                                    render_device_card(ui, dev);
                                    if idx + 1 < offline.len() {
                                        ui.add_space(6.0);
                                    }
                                }
                            }
                        });
                });

            (active_len, total)
        } else {
            body_ui.vertical_centered(|ui| {
                ui.add_space(80.0);
                ui.spinner();
                ui.add_space(10.0);
                ui.label(
                    RichText::new("Scanning peripherals...")
                        .size(13.0)
                        .color(TEXT_SECONDARY),
                );
            });
            (0, 0)
        };

        // --- FOOTER ---
        let foot_div_y = content_rect.max.y - footer_h - 1.0;
        root_ui.painter().line_segment(
            [
                Pos2::new(content_rect.min.x, foot_div_y),
                Pos2::new(content_rect.max.x, foot_div_y),
            ],
            Stroke::new(1.0, DIVIDER),
        );

        let footer_rect = Rect::from_min_max(
            Pos2::new(content_rect.min.x, foot_div_y + 1.0),
            content_rect.max,
        );
        let footer_left = if total_count == 0 {
            "Initializing...".to_string()
        } else {
            format!("{} of {} active · Auto-scan 5s", active_count, total_count)
        };
        root_ui.painter().text(
            Pos2::new(footer_rect.min.x + 16.0, footer_rect.center().y),
            egui::Align2::LEFT_CENTER,
            footer_left,
            egui::FontId::proportional(11.0),
            TEXT_MUTED,
        );
        root_ui.painter().text(
            Pos2::new(footer_rect.max.x - 16.0, footer_rect.center().y),
            egui::Align2::RIGHT_CENTER,
            "v0.1.0",
            egui::FontId::proportional(11.0),
            TEXT_MUTED,
        );
    }
}

fn main() -> eframe::Result {
    // Single-instance check
    let mutex_name: Vec<u16> = "perimon_instance_lock\0".encode_utf16().collect();
    let _h_mutex =
        unsafe { windows_ffi::CreateMutexW(std::ptr::null_mut(), 1, mutex_name.as_ptr()) };
    if unsafe { windows_ffi::GetLastError() } == windows_ffi::ERROR_ALREADY_EXISTS {
        return Ok(());
    }

    let start_minimized = std::env::args().any(|a| a == "--minimized" || a == "--tray");
    let icon_data = egui::IconData {
        rgba: include_bytes!("../tray_icon_32.rgba").to_vec(),
        width: 32,
        height: 32,
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_decorations(false)
            .with_inner_size([410.0, 520.0])
            .with_min_inner_size([360.0, 380.0])
            .with_title("perimon")
            .with_icon(std::sync::Arc::new(icon_data))
            .with_visible(!start_minimized)
            .with_active(!start_minimized),
        ..Default::default()
    };

    eframe::run_native(
        "perimon",
        options,
        Box::new(move |cc| Ok(Box::new(BatteryApp::new(cc, !start_minimized)))),
    )
}
