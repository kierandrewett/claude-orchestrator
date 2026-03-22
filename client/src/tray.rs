use std::sync::{Arc, Mutex};
use tray_icon::{
    menu::{Menu, MenuId, MenuItem, PredefinedMenuItem},
    Icon, TrayIcon, TrayIconBuilder,
};

// Claude icon extracted from claude.ai favicon — 32×32 raw RGBA
const ICON_RGBA: &[u8] = include_bytes!("../assets/claude-icon-32.rgba");
const ICON_SIZE: u32 = 32;

#[derive(Debug, Clone, Default)]
pub struct TrayState {
    pub connected: bool,
    pub hostname: Option<String>,
    pub active_sessions: usize,
    pub dashboard_url: String,
}

pub struct Tray {
    icon: TrayIcon,
    state_item: MenuItem,
    sessions_item: MenuItem,
    open_item: MenuItem,
    restart_item: MenuItem,
    quit_item: MenuItem,
    last_count: usize,
    last_connected: bool,
}

impl Tray {
    pub fn new(_state: Arc<Mutex<TrayState>>) -> anyhow::Result<Self> {
        #[cfg(target_os = "linux")]
        gtk::init()?;

        let icon = make_icon(0, false);

        let state_item = MenuItem::new("Disconnected", false, None);
        let sessions_item = MenuItem::new("No active sessions", false, None);
        let open_item = MenuItem::new("Open Dashboard", true, None);
        let restart_item = MenuItem::new("Restart Orchestrator", true, None);
        let quit_item = MenuItem::new("Quit", true, None);

        let menu = Menu::new();
        menu.append_items(&[
            &state_item,
            &PredefinedMenuItem::separator(),
            &sessions_item,
            &PredefinedMenuItem::separator(),
            &open_item,
            &restart_item,
            &PredefinedMenuItem::separator(),
            &quit_item,
        ])?;

        let tooltip = format!(
            "Claude Client — {}",
            std::env::var("SERVER_URL").unwrap_or_else(|_| "not configured".to_string()),
        );

        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(tooltip)
            .with_icon(icon)
            .build()?;

        Ok(Self {
            icon: tray,
            state_item,
            sessions_item,
            open_item,
            restart_item,
            quit_item,
            last_count: 0,
            last_connected: false,
        })
    }

    pub fn update(&mut self, state: &TrayState) {
        // Status line
        let status = if state.connected {
            format!(
                "Connected — {}",
                state.hostname.as_deref().unwrap_or("homelab")
            )
        } else {
            "Disconnected".to_string()
        };
        let _ = self.state_item.set_text(&status);

        // Sessions line
        let sessions_text = match state.active_sessions {
            0 => "No active sessions".to_string(),
            1 => "1 active session".to_string(),
            n => format!("{n} active sessions"),
        };
        let _ = self.sessions_item.set_text(&sessions_text);

        // Update icon when count or connection state changes
        if state.active_sessions != self.last_count || state.connected != self.last_connected {
            self.last_count = state.active_sessions;
            self.last_connected = state.connected;
            let _ = self
                .icon
                .set_icon(Some(make_icon(state.active_sessions, state.connected)));
        }
    }

    pub fn open_id(&self) -> MenuId {
        self.open_item.id().clone()
    }
    pub fn restart_id(&self) -> MenuId {
        self.restart_item.id().clone()
    }
    pub fn quit_id(&self) -> MenuId {
        self.quit_item.id().clone()
    }
}

// ── Icon rendering ────────────────────────────────────────────────────────────

/// Build the tray icon, optionally greyscaled and/or with a session-count badge.
fn make_icon(count: usize, connected: bool) -> Icon {
    let mut rgba = ICON_RGBA.to_vec();

    if !connected {
        greyscale(&mut rgba);
    }

    if count > 0 {
        draw_badge(&mut rgba, ICON_SIZE, ICON_SIZE, count);
    }

    Icon::from_rgba(rgba, ICON_SIZE, ICON_SIZE).expect("valid icon")
}

/// Desaturate all pixels to greyscale (luminance-weighted).
fn greyscale(pixels: &mut Vec<u8>) {
    for chunk in pixels.chunks_exact_mut(4) {
        let r = chunk[0] as f32;
        let g = chunk[1] as f32;
        let b = chunk[2] as f32;
        let luma = (0.299 * r + 0.587 * g + 0.114 * b) as u8;
        chunk[0] = luma;
        chunk[1] = luma;
        chunk[2] = luma;
        // alpha unchanged
    }
}

/// Draw a red pill badge with a digit (or "+") in the bottom-right corner.
fn draw_badge(pixels: &mut Vec<u8>, w: u32, h: u32, count: usize) {
    let label: &[u8] = match count {
        1 => b"1",
        2 => b"2",
        3 => b"3",
        4 => b"4",
        5 => b"5",
        6 => b"6",
        7 => b"7",
        8 => b"8",
        9 => b"9",
        _ => b"+",
    };

    // Badge is 16×16 px, flush to bottom-right corner.
    let badge_size: i32 = 16;
    let bx = w as i32 - badge_size;
    let by = h as i32 - badge_size;
    let r = badge_size / 2 - 1; // radius slightly inside the bounding box
    let cx = bx + badge_size / 2;
    let cy = by + badge_size / 2;

    // Filled red circle
    for py in 0..h as i32 {
        for px in 0..w as i32 {
            let dx = px - cx;
            let dy = py - cy;
            if dx * dx + dy * dy <= r * r {
                set_pixel(pixels, w, px as u32, py as u32, [220, 38, 38, 255]);
            }
        }
    }

    // Digit rendered at 2× scale (each font pixel → 2×2 block) for readability.
    // Base font is 3 wide × 5 tall; scaled glyph is 6 wide × 10 tall.
    let glyph = digit_glyph(label[0]);
    let scale: i32 = 2;
    let gx = cx - (3 * scale) / 2; // left edge, horizontally centred
    let gy = cy - (5 * scale) / 2; // top edge, vertically centred
    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..3i32 {
            if (bits >> (2 - col)) & 1 == 1 {
                for dy in 0..scale {
                    for dx in 0..scale {
                        let px = gx + col * scale + dx;
                        let py = gy + row as i32 * scale + dy;
                        if px >= 0 && py >= 0 && px < w as i32 && py < h as i32 {
                            set_pixel(pixels, w, px as u32, py as u32, [255, 255, 255, 255]);
                        }
                    }
                }
            }
        }
    }
}

#[inline]
fn set_pixel(pixels: &mut [u8], w: u32, x: u32, y: u32, rgba: [u8; 4]) {
    let idx = ((y * w + x) * 4) as usize;
    pixels[idx..idx + 4].copy_from_slice(&rgba);
}

/// 3-wide × 5-tall bitmap glyphs. Each u8 is one row; bits 2-0 = columns left→right.
fn digit_glyph(ch: u8) -> [u8; 5] {
    match ch {
        b'0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        b'1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        b'2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        b'3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        b'4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        b'5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        b'6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        b'7' => [0b111, 0b001, 0b001, 0b001, 0b001],
        b'8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        b'9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        b'+' => [0b010, 0b010, 0b111, 0b010, 0b010],
        _ => [0b000; 5],
    }
}
