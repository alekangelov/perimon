#![windows_subsystem = "windows"]

mod windows_ffi;
mod scanner;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{channel, Sender};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Color32, CornerRadius, Margin, Pos2, Rect, RichText, Stroke, StrokeKind, Ui, Vec2,
};
use notify_rust::Notification;
use scanner::{AllDevicesStatus, BluetoothDeviceStatus};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
};

struct SharedState {
    status: Option<AllDevicesStatus>,
    last_scanned: Option<Instant>,
    is_scanning: bool,
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

// -----------------------------------------------------------------------------
// Tray Icon RGBA Bitmap Generator (32x32 clean battery icon with transparency)
// -----------------------------------------------------------------------------

fn create_tray_icon() -> Icon {
    let rgba = include_bytes!("../tray_icon_32.rgba").to_vec();
    Icon::from_rgba(rgba, 32, 32).unwrap()
}

fn notify_low_battery(device_name: &str, percentage: u8) {
    let _ = Notification::new()
        .summary("⚠️ Low Peripheral Battery")
        .body(&format!(
            "{} is down to {}%! Please connect to charger.",
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

        if let Ok(data) = std::fs::read(r"C:\Windows\Fonts\segoeuib.ttf") {
            fonts.font_data.insert(
                "segoe_ui_bold".to_owned(),
                Arc::new(egui::FontData::from_owned(data)),
            );
        }

        if has_segoe {
            cc.egui_ctx.set_fonts(fonts);
        }

        // Configure sleek dark theme visuals
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(12, 14, 18);
        visuals.window_fill = Color32::from_rgb(12, 14, 18);
        visuals.extreme_bg_color = Color32::from_rgb(8, 9, 12);
        visuals.faint_bg_color = Color32::from_rgb(16, 19, 26);
        visuals.override_text_color = Some(Color32::from_rgb(241, 245, 249));

        visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(18, 22, 30);
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, Color32::from_rgb(148, 163, 184));
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(24, 29, 40);
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, Color32::from_rgb(203, 213, 225));
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(34, 42, 58);
        visuals.widgets.active.bg_fill = Color32::from_rgb(44, 54, 76);

        cc.egui_ctx.set_visuals(visuals);

        let state = Arc::new(Mutex::new(SharedState {
            status: None,
            last_scanned: None,
            is_scanning: true,
        }));

        let (trigger_tx, trigger_rx) = channel::<()>();

        // Build System Tray Context Menu
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

        thread::spawn(move || {
            loop {
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

                match trigger_rx.recv_timeout(Duration::from_secs(5)) {
                    Ok(_) => {}
                    Err(_) => {}
                }
            }
        });

        // Load App Icon Texture for Header Logo
        let icon_bytes = include_bytes!("../app_icon_128.rgba");
        let icon_img = egui::ColorImage::from_rgba_unmultiplied([128, 128], icon_bytes);
        let app_icon_texture = cc.egui_ctx.load_texture("app_icon", icon_img, egui::TextureOptions::LINEAR);

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
// Helper: Clean & Deduplicate Bluetooth Device Names
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
    let mut map: std::collections::HashMap<String, BluetoothDeviceStatus> = std::collections::HashMap::new();

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
        b.connected.cmp(&a.connected).then_with(|| a.device.cmp(&b.device))
    });
    list
}

// -----------------------------------------------------------------------------
// -----------------------------------------------------------------------------
// Drawing: Battery Pill Widget (adapts to active vs faded inactive state)
// -----------------------------------------------------------------------------

fn draw_battery_pill(ui: &mut Ui, percentage: u8, is_charging: bool, is_active: bool) {
    let desired_size = Vec2::new(40.0, 19.0);
    let (rect, _resp) = ui.allocate_exact_size(desired_size, egui::Sense::hover());
    let painter = ui.painter();

    let level_color = if !is_active {
        Color32::from_rgb(68, 76, 92)
    } else if is_charging {
        Color32::from_rgb(56, 189, 248)
    } else if percentage >= 50 {
        Color32::from_rgb(34, 197, 94)
    } else if percentage >= 20 {
        Color32::from_rgb(245, 158, 11)
    } else {
        Color32::from_rgb(239, 68, 68)
    };

    let body_w = desired_size.x - 4.5;
    let body_rect = Rect::from_min_size(rect.min, Vec2::new(body_w, desired_size.y));

    // Outer shell & terminal nipple cap
    let (shell_bg, shell_stroke, cap_fill) = if is_active {
        (
            Color32::from_rgb(16, 20, 28),
            Color32::from_rgb(55, 64, 85),
            Color32::from_rgb(60, 70, 92),
        )
    } else {
        (
            Color32::from_rgb(13, 15, 20),
            Color32::from_rgb(30, 36, 48),
            Color32::from_rgb(38, 44, 58),
        )
    };

    painter.rect(
        body_rect,
        CornerRadius::same(5),
        shell_bg,
        Stroke::new(1.2, shell_stroke),
        StrokeKind::Inside,
    );

    let cap_rect = Rect::from_min_size(
        Pos2::new(body_rect.max.x + 0.5, body_rect.min.y + (desired_size.y - 8.0) / 2.0),
        Vec2::new(2.5, 8.0),
    );
    painter.rect_filled(cap_rect, CornerRadius::same(1), cap_fill);

    // Proportional interior fill
    let margin = 2.0;
    let max_fill_w = (body_w - (margin * 2.0)).max(0.0);
    let fill_w = (max_fill_w * (percentage as f32 / 100.0)).clamp(2.0, max_fill_w);
    let inner_h = desired_size.y - (margin * 2.0);

    let fill_rect = Rect::from_min_size(
        Pos2::new(body_rect.min.x + margin, body_rect.min.y + margin),
        Vec2::new(fill_w, inner_h),
    );
    painter.rect_filled(fill_rect, CornerRadius::same(3), level_color);
}

// -----------------------------------------------------------------------------
// Component: Sleek Device Row (faded / dimmed when inactive or disconnected)
// -----------------------------------------------------------------------------

fn row_card_frame(is_active: bool) -> egui::Frame {
    if is_active {
        egui::Frame::NONE
            .fill(Color32::from_rgb(19, 23, 32))
            .stroke(Stroke::new(1.0, Color32::from_rgb(34, 40, 56)))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(Margin::symmetric(16, 12))
    } else {
        egui::Frame::NONE
            .fill(Color32::from_rgb(13, 15, 20))
            .stroke(Stroke::new(1.0, Color32::from_rgb(22, 26, 36)))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(Margin::symmetric(16, 12))
    }
}

fn render_device_row(
    ui: &mut Ui,
    icon: &str,
    name: &str,
    subtitle: &str,
    battery: Option<u8>,
    is_charging: bool,
    status_text: Option<&str>,
    tooltip: Option<&str>,
    is_active: bool,
) {
    row_card_frame(is_active).show(ui, |ui| {
        ui.horizontal(|ui| {
            // 1. Icon Badge (smoothly faded when inactive)
            let (badge_bg, badge_stroke) = if is_active {
                (Color32::from_rgb(27, 33, 46), Color32::from_rgb(44, 54, 76))
            } else {
                (Color32::from_rgb(16, 18, 24), Color32::from_rgb(24, 28, 38))
            };

            egui::Frame::NONE
                .fill(badge_bg)
                .stroke(Stroke::new(1.0, badge_stroke))
                .corner_radius(CornerRadius::same(9))
                .inner_margin(Margin::same(7))
                .show(ui, |ui| {
                    let mut icon_text = RichText::new(icon).size(16.0);
                    if !is_active {
                        icon_text = icon_text.color(Color32::from_rgba_unmultiplied(180, 190, 205, 110));
                    }
                    ui.label(icon_text);
                });

            ui.add_space(14.0);

            // 2. Name & Subtitle Stack (faded text colors when inactive)
            let (name_color, sub_color) = if is_active {
                (Color32::from_rgb(248, 250, 252), Color32::from_rgb(130, 142, 162))
            } else {
                (Color32::from_rgb(112, 122, 140), Color32::from_rgb(72, 80, 94))
            };

            let right_column_width = 110.0;
            let left_width = (ui.available_width() - right_column_width).max(130.0);

            ui.allocate_ui_with_layout(
                Vec2::new(left_width, ui.available_height()),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    let name_label = ui.label(
                        RichText::new(name)
                            .size(14.0)
                            .strong()
                            .color(name_color),
                    );
                    if let Some(tip) = tooltip {
                        name_label.on_hover_text(tip);
                    }
                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(subtitle)
                            .size(11.5)
                            .color(sub_color),
                    );
                },
            );

            // 3. Right Column: Battery & Status
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(bat) = battery {
                    let color = if !is_active {
                        Color32::from_rgb(95, 105, 122)
                    } else if is_charging {
                        Color32::from_rgb(56, 189, 248)
                    } else if bat >= 50 {
                        Color32::from_rgb(241, 245, 249)
                    } else if bat >= 20 {
                        Color32::from_rgb(245, 158, 11)
                    } else {
                        Color32::from_rgb(239, 68, 68)
                    };

                    ui.label(
                        RichText::new(format!("{}%", bat))
                            .size(15.5)
                            .strong()
                            .color(color),
                    );

                    ui.add_space(8.0);

                    if is_charging && is_active {
                        ui.label(RichText::new("⚡").color(Color32::from_rgb(56, 189, 248)).size(13.0));
                        ui.add_space(4.0);
                    }

                    draw_battery_pill(ui, bat, is_charging, is_active);
                } else if let Some(txt) = status_text {
                    let (pill_bg, pill_stroke, pill_color) = if is_active {
                        (
                            Color32::from_rgb(26, 32, 44),
                            Color32::from_rgb(42, 52, 72),
                            Color32::from_rgb(150, 165, 188),
                        )
                    } else {
                        (
                            Color32::from_rgb(16, 19, 25),
                            Color32::from_rgb(26, 31, 42),
                            Color32::from_rgb(82, 92, 110),
                        )
                    };

                    egui::Frame::NONE
                        .fill(pill_bg)
                        .stroke(Stroke::new(1.0, pill_stroke))
                        .corner_radius(CornerRadius::same(7))
                        .inner_margin(Margin::symmetric(10, 4))
                        .show(ui, |ui| {
                            let resp = ui.label(
                                RichText::new(txt)
                                    .size(11.5)
                                    .color(pill_color),
                            );
                            if let Some(tip) = tooltip {
                                resp.on_hover_text(tip);
                            }
                        });
                }
            });
        });
    });
}

// -----------------------------------------------------------------------------
// Main App UI
// -----------------------------------------------------------------------------

impl eframe::App for BatteryApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        ui.ctx().request_repaint_after(Duration::from_secs(1));

        // 1. Handle Minimize-To-Tray on Window Close button click
        if ui.input(|i| i.viewport().close_requested()) {
            if !self.quitting {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Visible(false));
                self.window_visible = false;
            }
        }

        // 2. Handle Tray Icon Click Events (Left-click to toggle/show window)
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click { button, button_state, .. } = event {
                if button == MouseButton::Left && button_state == MouseButtonState::Up {
                    self.window_visible = !self.window_visible;
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Visible(self.window_visible));
                    if self.window_visible {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                }
            }
        }

        // 3. Handle Tray Context Menu Events
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

        let state_guard = self.state.lock().unwrap();
        let is_scanning = state_guard.is_scanning;
        let last_scanned = state_guard.last_scanned;
        let status_opt = state_guard.status.clone();
        drop(state_guard);

        // 4. Low Battery Notification Monitor (Trigger native toast alert when <= 20%)
        if let Some(ref status) = status_opt {
            // Check HyperX
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

            // Check Incott
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

            // Check Bluetooth Devices
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

        // 5. Custom Frameless Window Shell
        let window_frame = egui::Frame::NONE
            .fill(Color32::from_rgb(12, 14, 18))
            .stroke(Stroke::new(1.0, Color32::from_rgb(34, 40, 56)))
            .corner_radius(CornerRadius::same(12));

        window_frame.show(ui, |ui| {
            // --- CUSTOM TITLEBAR (30px, drag-to-move, sleek minimize & close buttons) ---
            ui.horizontal(|ui| {
                ui.add_space(10.0);

                // Small Squircle Logo
                let (mini_icon_rect, _) = ui.allocate_exact_size(Vec2::new(16.0, 16.0), egui::Sense::hover());
                let uv = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0));
                ui.painter().image(self.app_icon_texture.id(), mini_icon_rect, uv, Color32::WHITE);
                ui.add_space(6.0);
                ui.label(RichText::new("perimon").size(12.0).strong().color(Color32::from_rgb(150, 162, 182)));

                // Draggable Titlebar Middle Space
                let remaining_w = (ui.available_width() - 68.0).max(10.0);
                let (_, drag_resp) = ui.allocate_exact_size(Vec2::new(remaining_w, 28.0), egui::Sense::click_and_drag());
                if drag_resp.drag_started() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }

                // Window Control Buttons (Minimize & Close to tray)
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Close Button
                    let close_btn = egui::Button::new(RichText::new("✕").size(11.0).color(Color32::from_rgb(203, 213, 225)))
                        .fill(Color32::TRANSPARENT)
                        .stroke(Stroke::NONE)
                        .corner_radius(CornerRadius::same(5))
                        .min_size(Vec2::new(26.0, 22.0));
                    let close_resp = ui.add(close_btn);
                    if close_resp.hovered() {
                        ui.painter().rect_filled(close_resp.rect, CornerRadius::same(5), Color32::from_rgb(225, 29, 72));
                        ui.painter().text(
                            close_resp.rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "✕",
                            egui::FontId::proportional(11.0),
                            Color32::WHITE,
                        );
                    }
                    if close_resp.clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Visible(false));
                        self.window_visible = false;
                    }

                    ui.add_space(2.0);

                    // Minimize Button
                    let min_btn = egui::Button::new(RichText::new("─").size(12.0).color(Color32::from_rgb(180, 190, 205)))
                        .fill(Color32::TRANSPARENT)
                        .stroke(Stroke::NONE)
                        .corner_radius(CornerRadius::same(5))
                        .min_size(Vec2::new(26.0, 22.0));
                    let min_resp = ui.add(min_btn);
                    if min_resp.hovered() {
                        ui.painter().rect_filled(min_resp.rect, CornerRadius::same(5), Color32::from_rgb(28, 34, 48));
                        ui.painter().text(
                            min_resp.rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "─",
                            egui::FontId::proportional(12.0),
                            Color32::from_rgb(220, 230, 245),
                        );
                    }
                    if min_resp.clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                    }
                });
            });

            ui.add_space(2.0);
            ui.separator();
            ui.add_space(2.0);

            // --- MAIN DASHBOARD CONTENT ---
            let content_frame = egui::Frame::NONE
                .fill(Color32::from_rgb(12, 14, 18))
                .inner_margin(Margin::symmetric(18, 12));

            content_frame.show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // --- HEADER ---
                        ui.horizontal(|ui| {
                            // Minimalist Black Squircle App Logo
                            let (icon_rect, _) = ui.allocate_exact_size(Vec2::new(38.0, 38.0), egui::Sense::hover());
                            let uv = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0));
                            ui.painter().image(
                                self.app_icon_texture.id(),
                                icon_rect,
                                uv,
                                Color32::WHITE,
                            );

                            ui.add_space(10.0);

                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("perimon")
                                            .size(17.5)
                                            .strong()
                                            .color(Color32::WHITE),
                                    );
                                    ui.add_space(6.0);
                                    ui.label(
                                        RichText::new("● Live")
                                            .size(11.5)
                                            .color(Color32::from_rgb(34, 197, 94))
                                            .strong(),
                                    );
                                });

                                ui.add_space(2.0);

                                let subtext = match last_scanned {
                                    Some(t) => {
                                        let elapsed = t.elapsed().as_secs();
                                        if is_scanning {
                                            "Scanning hardware...".to_string()
                                        } else if elapsed == 0 {
                                            "Updated just now".to_string()
                                        } else {
                                            format!("Updated {}s ago", elapsed)
                                        }
                                    }
                                    None => "Connecting...".to_string(),
                                };
                                ui.label(RichText::new(subtext).size(11.5).color(Color32::from_rgb(120, 132, 152)));
                            });

                            // Dragging on empty space in header also moves window
                            let header_drag_w = (ui.available_width() - 85.0).max(0.0);
                            let (_, header_drag_resp) = ui.allocate_exact_size(Vec2::new(header_drag_w, 36.0), egui::Sense::click_and_drag());
                            if header_drag_resp.drag_started() {
                                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                            }

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let btn_text = if is_scanning { "..." } else { "↻ Refresh" };
                                let btn = egui::Button::new(RichText::new(btn_text).size(12.0).strong())
                                    .fill(Color32::from_rgb(26, 32, 45))
                                    .stroke(Stroke::new(1.0, Color32::from_rgb(44, 54, 76)))
                                    .corner_radius(CornerRadius::same(7))
                                    .min_size(Vec2::new(74.0, 28.0));

                                if ui.add_enabled(!is_scanning, btn).clicked() {
                                    let _ = self.trigger_tx.send(());
                                }
                            });
                        });

                        ui.add_space(14.0);
                        ui.separator();
                        ui.add_space(12.0);

                    // --- DEVICE LIST ---
                    match status_opt {
                        Some(status) => {
                            // 1. HyperX Headset
                            let is_hx_active = status.hyperx.connected;
                            let hx_sub = if let Some(mv) = status.hyperx.voltage_mv {
                                format!("2.4GHz Wireless • {:.2}V", mv as f32 / 1000.0)
                            } else if status.hyperx.dongle_connected {
                                "2.4GHz Dongle Online".to_string()
                            } else {
                                "Dongle Not Found".to_string()
                            };

                            let hx_status = if !is_hx_active {
                                if status.hyperx.dongle_connected {
                                    Some("○ Asleep")
                                } else {
                                    Some("○ Offline")
                                }
                            } else {
                                None
                            };

                            render_device_row(
                                ui,
                                "🎧",
                                "HyperX Cloud III",
                                &hx_sub,
                                status.hyperx.battery,
                                false,
                                hx_status,
                                None,
                                is_hx_active,
                            );
                            ui.add_space(10.0);

                            // 2. Incott G23 v2 Mouse
                            let is_mouse_active = status.incott.connected;
                            let mut mouse_sub_parts = vec!["2.4GHz Wireless"];
                            let dpi_str;
                            if let Some(dpi) = status.incott.dpi {
                                dpi_str = format!("{} DPI", dpi);
                                mouse_sub_parts.push(&dpi_str);
                            }
                            let hz_str;
                            if let Some(hz) = status.incott.polling_rate {
                                hz_str = format!("{} Hz", hz);
                                mouse_sub_parts.push(&hz_str);
                            }
                            let mouse_sub = mouse_sub_parts.join(" • ");

                            let mouse_status = if !is_mouse_active {
                                if status.incott.dongle_connected {
                                    Some("○ Standby")
                                } else {
                                    Some("○ Offline")
                                }
                            } else {
                                None
                            };

                            render_device_row(
                                ui,
                                "🖱️",
                                "Incott G23 v2",
                                &mouse_sub,
                                status.incott.battery,
                                status.incott.charging,
                                mouse_status,
                                None,
                                is_mouse_active,
                            );
                            ui.add_space(10.0);

                            // 3. Keychron Keyboard
                            let is_kc_active = status.keychron.connected;
                            let kc_sub = if is_kc_active {
                                format!("Connected • {}", status.keychron.connection_type)
                            } else {
                                "Not Connected".to_string()
                            };

                            let (kc_bat, kc_status, kc_tip) = if let Some(bat) = status.keychron.battery {
                                (Some(bat), None, None)
                            } else if is_kc_active {
                                (
                                    None,
                                    Some("2.4G Link"),
                                    Some("Keychron 2.4G dongles do not report battery to Windows.\nTip: Press Fn + B for physical LED level, or switch to Bluetooth for live percentage."),
                                )
                            } else {
                                (None, Some("○ Offline"), None)
                            };

                            render_device_row(
                                ui,
                                "⌨️",
                                "Keychron Keyboard",
                                &kc_sub,
                                kc_bat,
                                false,
                                kc_status,
                                kc_tip,
                                is_kc_active,
                            );
                            ui.add_space(10.0);

                            // 4. Other Bluetooth Devices
                            let bt_devices = deduplicate_bluetooth_devices(&status.other_devices);
                            for dev in bt_devices {
                                let is_bt_active = dev.connected;
                                let icon = if dev.device.to_lowercase().contains("head")
                                    || dev.device.to_lowercase().contains("moondrop")
                                    || dev.device.to_lowercase().contains("ath")
                                    || dev.device.to_lowercase().contains("ear")
                                {
                                    "🎧"
                                } else if dev.device.to_lowercase().contains("mouse")
                                    || dev.device.to_lowercase().contains("master")
                                {
                                    "🖱️"
                                } else {
                                    "📶"
                                };

                                let sub = if is_bt_active {
                                    "Bluetooth Connected".to_string()
                                } else {
                                    "Bluetooth • Disconnected".to_string()
                                };

                                let (bat, st) = if is_bt_active {
                                    (Some(dev.battery), None)
                                } else {
                                    (None, Some("○ Offline"))
                                };

                                render_device_row(
                                    ui,
                                    icon,
                                    &dev.device,
                                    &sub,
                                    bat,
                                    false,
                                    st,
                                    None,
                                    is_bt_active,
                                );
                                ui.add_space(10.0);
                            }
                        }
                        None => {
                            ui.vertical_centered(|ui| {
                                ui.add_space(50.0);
                                ui.spinner();
                                ui.add_space(12.0);
                                ui.label(
                                    RichText::new("Querying peripherals...")
                                        .size(13.5)
                                        .color(Color32::from_rgb(148, 163, 184)),
                                );
                            });
                        }
                    }

                    ui.add_space(10.0);
                });
            });
        });
    }
}

fn main() -> eframe::Result {
    // Single-instance check: ensure ONLY one instance can EVER run at a time
    let mutex_name: Vec<u16> = "perimon_instance_lock\0"
        .encode_utf16()
        .collect();
    let _h_mutex = unsafe {
        windows_ffi::CreateMutexW(std::ptr::null_mut(), 1, mutex_name.as_ptr())
    };
    if unsafe { windows_ffi::GetLastError() } == windows_ffi::ERROR_ALREADY_EXISTS {
        // Another instance is already running! Exit silently so duplicates never happen
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
            .with_inner_size([420.0, 560.0])
            .with_min_inner_size([360.0, 420.0])
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
