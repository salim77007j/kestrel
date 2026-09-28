//! Vector icons.
//!
//! Drawn as egui shape primitives rather than shipped as image assets. That
//! keeps them crisp at any scale, tintable by theme, and free of a binary
//! dependency — a real win for a browser that has to start fast.

use egui::{Color32, Painter, Pos2, Rect, Stroke, StrokeKind, Vec2};

/// Standard 24x24 design grid, matching common icon sets.
const GRID: f32 = 24.0;

pub fn stroke(p: &Painter, color: Color32, width: f32) -> Stroke {
    Stroke::new(width, color)
}

/// Back arrow.
pub fn back(p: &Painter, r: Rect, c: Color32, w: f32) {
    let s = r.height() / GRID;
    let o = r.min;
    let pt = |x: f32, y: f32| Pos2::new(o.x + x * s, o.y + y * s);
    let line = p.line_segment([pt(20.0, 12.0), pt(4.0, 12.0)], stroke(p, c, w));
    let head = p.add(Stroke::new(
        w,
        c,
    ));
    p.line_segment([pt(10.0, 6.0), pt(4.0, 12.0)], head);
    p.line_segment([pt(4.0, 12.0), pt(10.0, 18.0)], head);
    let _ = line;
}

/// Forward arrow.
pub fn forward(p: &Painter, r: Rect, c: Color32, w: f32) {
    let s = r.height() / GRID;
    let o = r.min;
    let pt = |x: f32, y: f32| Pos2::new(o.x + x * s, o.y + y * s);
    p.line_segment([pt(4.0, 12.0), pt(20.0, 12.0)], stroke(p, c, w));
    p.line_segment([pt(14.0, 6.0), pt(20.0, 12.0)], stroke(p, c, w));
    p.line_segment([pt(20.0, 12.0), pt(14.0, 18.0)], stroke(p, c, w));
}

/// Reload / stop. `stop` draws the square variant used while loading.
pub fn reload(p: &Painter, r: Rect, c: Color32, w: f32, stop: bool) {
    if stop {
        let inset = r.height() * 0.28;
        p.rect_filled(
            Rect::from_min_max(
                Pos2::new(r.min.x + inset, r.min.y + inset),
                Pos2::new(r.max.x - inset, r.max.y - inset),
            ),
            1.5,
            c,
        );
        return;
    }
    let center = r.center();
    let rad = r.height() * 0.34;
    // Arc from 40deg round to 330deg, leaving a gap at the top-right.
    let mut pts = Vec::new();
    for i in 0..24 {
        let t = i as f32 / 23.0;
        let a = 0.5 + t * 4.9; // radians
        pts.push(Pos2::new(
            center.x + a.cos() * rad,
            center.y + a.sin() * rad,
        ));
    }
    p.add(egui::Shape::line(pts, stroke(p, c, w)));
    // Arrow head closing the arc.
    let tip = *pts.last().unwrap();
    let h = r.height() * 0.13;
    p.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(tip.x + h * 0.9, tip.y - h * 0.5),
            Pos2::new(tip.x - h * 0.3, tip.y - h * 1.15),
            Pos2::new(tip.x - h * 0.7, tip.y + h * 0.6),
        ],
        c,
        Stroke::NONE,
    ));
}

/// House glyph for the home button.
pub fn home(p: &Painter, r: Rect, c: Color32, w: f32) {
    let s = r.height() / GRID;
    let o = r.min;
    let pt = |x: f32, y: f32| Pos2::new(o.x + x * s, o.y + y * s);
    // Roof
    p.add(egui::Shape::line(
        vec![pt(3.0, 11.5), pt(12.0, 4.0), pt(21.0, 11.5)],
        stroke(p, c, w),
    ));
    // Body
    p.add(egui::Shape::line(
        vec![pt(5.5, 10.5), pt(5.5, 20.0)],
        stroke(p, c, w),
    ));
    p.add(egui::Shape::line(
        vec![pt(18.5, 10.5), pt(18.5, 20.0)],
        stroke(p, c, w),
    ));
    p.add(egui::Shape::line(
        vec![pt(5.5, 20.0), pt(18.5, 20.0)],
        stroke(p, c, w),
    ));
}

/// Shield, tinted by connection security.
pub fn shield(p: &Painter, r: Rect, c: Color32, w: f32) {
    let s = r.height() / GRID;
    let o = r.min;
    let pt = |x: f32, y: f32| Pos2::new(o.x + x * s, o.y + y * s);
    p.add(egui::Shape::line(
        vec![
            pt(12.0, 3.0),
            pt(20.0, 6.0),
            pt(20.0, 12.0),
            pt(12.0, 21.0),
            pt(4.0, 12.0),
            pt(4.0, 6.0),
            pt(12.0, 3.0),
        ],
        stroke(p, c, w),
    ));
}

/// Star, for bookmarking. Filled when the page is bookmarked.
pub fn star(p: &Painter, r: Rect, c: Color32, w: f32, filled: bool) {
    let center = r.center();
    let rad = r.height() * 0.42;
    let mut pts = Vec::new();
    for i in 0..10 {
        let a = -std::f32::consts::FRAC_PI_2 + (i as f32) * std::f32::consts::PI / 5.0;
        let rr = if i % 2 == 0 { rad } else { rad * 0.45 };
        pts.push(Pos2::new(
            center.x + a.cos() * rr,
            center.y + a.sin() * rr,
        ));
    }
    if filled {
        p.add(egui::Shape::convex_polygon(pts, c, Stroke::NONE));
    } else {
        p.add(egui::Shape::closed_line(pts, stroke(p, c, w)));
    }
}

/// Three vertical dots for menus.
pub fn menu(p: &Painter, r: Rect, c: Color32, w: f32) {
    let x = r.center().x;
    let pad = r.height() * 0.28;
    for y in [r.min.y + pad, r.center().y, r.max.y - pad] {
        p.circle_filled(Pos2::new(x, y), w * 0.9, c);
    }
}

/// Plus, for the new-tab button.
pub fn plus(p: &Painter, r: Rect, c: Color32, w: f32) {
    let m = r.height() * 0.3;
    p.line_segment(
        [r.min + Vec2::new(m, 0.0), r.max - Vec2::new(m, 0.0)],
        stroke(p, c, w),
    );
    p.line_segment(
        [r.min + Vec2::new(0.0, m), r.max - Vec2::new(0.0, m)],
        stroke(p, c, w),
    );
}

/// Cross, for closing tabs and dismissing dialogs.
pub fn close(p: &Painter, r: Rect, c: Color32, w: f32) {
    let m = r.height() * 0.32;
    p.line_segment(
        [r.min + Vec2::new(m, m), r.max - Vec2::new(m, m)],
        stroke(p, c, w),
    );
    p.line_segment(
        [r.min + Vec2::new(m, r.height() - m), r.max - Vec2::new(m, r.height() - m)],
        stroke(p, c, w),
    );
}

/// Magnifier for the omnibox and new-tab search.
pub fn search(p: &Painter, r: Rect, c: Color32, w: f32) {
    let s = r.height();
    let o = r.min;
    let ctr = Pos2::new(o.x + s * 0.42, o.y + s * 0.42);
    let rad = s * 0.28;
    let mut pts = Vec::new();
    for i in 0..20 {
        let a = (i as f32 / 19.0) * std::f32::consts::TAU;
        pts.push(Pos2::new(ctr.x + a.cos() * rad, ctr.y + a.sin() * rad));
    }
    p.add(egui::Shape::closed_line(pts, stroke(p, c, w)));
    p.line_segment(
        [
            Pos2::new(ctr.x + rad * 0.72, ctr.y + rad * 0.72),
            Pos2::new(o.x + s * 0.92, o.y + s * 0.92),
        ],
        stroke(p, c, w),
    );
}

/// Microphone, for the voice-search affordance on the new-tab page.
pub fn mic(p: &Painter, r: Rect, c: Color32, w: f32) {
    let s = r.height();
    let o = r.min;
    let cx = o.x + s * 0.5;
    p.add(egui::Shape::line(
        vec![
            Pos2::new(cx, o.y + s * 0.18),
            Pos2::new(cx, o.y + s * 0.55),
        ],
        stroke(p, c, w * 2.2),
    ));
    p.add(egui::Shape::line(
        vec![
            Pos2::new(o.x + s * 0.32, o.y + s * 0.5),
            Pos2::new(cx, o.y + s * 0.72),
            Pos2::new(o.x + s * 0.68, o.y + s * 0.5),
        ],
        stroke(p, c, w),
    ));
    p.add(egui::Shape::line(
        vec![
            Pos2::new(cx, o.y + s * 0.72),
            Pos2::new(cx, o.y + s * 0.86),
        ],
        stroke(p, c, w),
    ));
}

/// Camera / lens for the visual-search affordance.
pub fn camera(p: &Painter, r: Rect, c: Color32, w: f32) {
    let s = r.height();
    let o = r.min;
    p.add(egui::Shape::line(
        vec![
            Pos2::new(o.x + s * 0.2, o.y + s * 0.3),
            Pos2::new(o.x + s * 0.8, o.y + s * 0.3),
            Pos2::new(o.x + s * 0.8, o.y + s * 0.78),
            Pos2::new(o.x + s * 0.2, o.y + s * 0.78),
            Pos2::new(o.x + s * 0.2, o.y + s * 0.3),
        ],
        stroke(p, c, w),
    ));
    p.add(egui::Shape::closed_line(
        {
            let ctr = Pos2::new(o.x + s * 0.5, o.y + s * 0.54);
            let rad = s * 0.16;
            (0..16)
                .map(|i| {
                    let a = (i as f32 / 16.0) * std::f32::consts::TAU;
                    Pos2::new(ctr.x + a.cos() * rad, ctr.y + a.sin() * rad)
                })
                .collect()
        },
        stroke(p, c, w),
    ));
}

/// Sliders glyph for the privacy dashboard.
pub fn privacy(p: &Painter, r: Rect, c: Color32, w: f32) {
    let s = r.height();
    let o = r.min;
    for (i, (y, knob)) in [(0.3, 0.62), (0.5, 0.38), (0.7, 0.55)].iter().enumerate() {
        let yy = o.y + s * y;
        p.line_segment(
            [Pos2::new(o.x + s * 0.15, yy), Pos2::new(o.x + s * 0.85, yy)],
            stroke(p, c, w),
        );
        let _ = i;
        p.circle_filled(Pos2::new(o.x + s * knob, yy), w * 1.9, c);
    }
}

/// Gear for settings.
pub fn gear(p: &Painter, r: Rect, c: Color32, w: f32) {
    let ctr = r.center();
    let outer = r.height() * 0.42;
    let inner = outer * 0.62;
    let teeth = 8;
    let mut pts = Vec::new();
    for i in 0..teeth * 2 {
        let a = (i as f32 / (teeth * 2) as f32) * std::f32::consts::TAU;
        let rad = if i % 2 == 0 { outer } else { inner };
        pts.push(Pos2::new(ctr.x + a.cos() * rad, ctr.y + a.sin() * rad));
    }
    p.add(egui::Shape::convex_polygon(pts, c, Stroke::NONE));
    p.circle_filled(ctr, outer * 0.32, if p.clip_rect().is_negative() { c } else { c });
}

/// Clock for history.
pub fn clock(p: &Painter, r: Rect, c: Color32, w: f32) {
    let ctr = r.center();
    let rad = r.height() * 0.4;
    p.circle_stroke(ctr, rad, stroke(p, c, w));
    p.line_segment([ctr, Pos2::new(ctr.x, ctr.y - rad * 0.55)], stroke(p, c, w));
    p.line_segment([ctr, Pos2::new(ctr.x + rad * 0.42, ctr.y)], stroke(p, c, w));
}

/// Down-arrow into a tray, for downloads.
pub fn download(p: &Painter, r: Rect, c: Color32, w: f32) {
    let s = r.height();
    let o = r.min;
    let cx = o.x + s * 0.5;
    p.line_segment(
        [Pos2::new(cx, o.y + s * 0.2), Pos2::new(cx, o.y + s * 0.6)],
        stroke(p, c, w),
    ));
    p.add(egui::Shape::line(
        vec![
            Pos2::new(cx - s * 0.16, o.y + s * 0.45),
            Pos2::new(cx, o.y + s * 0.62),
            Pos2::new(cx + s * 0.16, o.y + s * 0.45),
        ],
        stroke(p, c, w),
    ));
    p.add(egui::Shape::line(
        vec![
            Pos2::new(o.x + s * 0.22, o.y + s * 0.68),
            Pos2::new(o.x + s * 0.22, o.y + s * 0.84),
            Pos2::new(o.x + s * 0.78, o.y + s * 0.84),
            Pos2::new(o.x + s * 0.78, o.y + s * 0.68),
        ],
        stroke(p, c, w),
    ));
}

/// Globe, for the default favicon placeholder.
pub fn globe(p: &Painter, r: Rect, c: Color32, w: f32) {
    let ctr = r.center();
    let rad = r.height() * 0.4;
    p.circle_stroke(ctr, rad, stroke(p, c, w));
    p.add(egui::Shape::line(
        vec![
            Pos2::new(ctr.x - rad, ctr.y),
            Pos2::new(ctr.x + rad, ctr.y),
        ],
        stroke(p, c, w),
    ));
    p.circle_stroke(ctr, rad * 0.5, stroke(p, c, w * 0.8));
    p.add(egui::Shape::line(
        vec![
            Pos2::new(ctr.x, ctr.y - rad),
            Pos2::new(ctr.x, ctr.y + rad),
        ],
        stroke(p, c, w * 0.8),
    ));
}

/// Padlock, shown for insecure origins.
pub fn lock(p: &Painter, r: Rect, c: Color32, w: f32) {
    let s = r.height();
    let o = r.min;
    let body = Rect::from_min_max(
        Pos2::new(o.x + s * 0.26, o.y + s * 0.46),
        Pos2::new(o.x + s * 0.74, o.y + s * 0.86),
    );
    p.rect_filled(body, 1.5, c);
    p.add(egui::Shape::line(
        {
            let ctr = Pos2::new(o.x + s * 0.5, o.y + s * 0.46);
            let rad = s * 0.16;
            (0..14)
                .map(|i| {
                    let a = std::f32::consts::PI + (i as f32 / 13.0) * std::f32::consts::PI;
                    Pos2::new(ctr.x + a.cos() * rad, ctr.y + a.sin() * rad)
                })
                .collect()
        },
        stroke(p, c, w),
    ));
}

/// Sun, for the new-tab greeting.
pub fn sun(p: &Painter, r: Rect, c: Color32, w: f32) {
    let ctr = r.center();
    let rad = r.height() * 0.22;
    p.circle_stroke(ctr, rad, stroke(p, c, w));
    for i in 0..8 {
        let a = (i as f32 / 8.0) * std::f32::consts::TAU;
        p.line_segment(
            [
                Pos2::new(ctr.x + a.cos() * rad * 1.5, ctr.y + a.sin() * rad * 1.5),
                Pos2::new(ctr.x + a.cos() * rad * 2.0, ctr.y + a.sin() * rad * 2.0),
            ],
            stroke(p, c, w),
        );
    }
}

/// Cloud, for the weather on the new-tab page.
pub fn cloud(p: &Painter, r: Rect, c: Color32, w: f32) {
    let s = r.height();
    let o = r.min;
    let ctr = Pos2::new(o.x + s * 0.5, o.y + s * 0.6);
    p.circle_stroke(ctr, s * 0.22, stroke(p, c, w));
    p.circle_stroke(Pos2::new(o.x + s * 0.68, o.y + s * 0.64), s * 0.17, stroke(p, c, w));
    p.line_segment(
        [
            Pos2::new(o.x + s * 0.32, o.y + s * 0.75),
            Pos2::new(o.x + s * 0.7, o.y + s * 0.75),
        ],
        stroke(p, c, w),
    );
}

/// Bookmark ribbon, used in the manager.
pub fn bookmark(p: &Painter, r: Rect, c: Color32, w: f32) {
    let s = r.height();
    let o = r.min;
    p.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(o.x + s * 0.24, o.y + s * 0.16),
            Pos2::new(o.x + s * 0.76, o.y + s * 0.16),
            Pos2::new(o.x + s * 0.76, o.y + s * 0.86),
            Pos2::new(o.x + s * 0.5, o.y + s * 0.68),
            Pos2::new(o.x + s * 0.24, o.y + s * 0.86),
        ],
        c,
        Stroke::NONE,
    ));
}

/// Speaker, for unmuting a tab.
pub fn volume(p: &Painter, r: Rect, c: Color32, w: f32) {
    let s = r.height();
    let o = r.min;
    p.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(o.x + s * 0.2, o.y + s * 0.38),
            Pos2::new(o.x + s * 0.38, o.y + s * 0.38),
            Pos2::new(o.x + s * 0.56, o.y + s * 0.2),
            Pos2::new(o.x + s * 0.56, o.y + s * 0.8),
            Pos2::new(o.x + s * 0.38, o.y + s * 0.62),
            Pos2::new(o.x + s * 0.2, o.y + s * 0.62),
        ],
        c,
        Stroke::NONE,
    ));
    p.add(egui::Shape::line(
        vec![
            Pos2::new(o.x + s * 0.66, o.y + s * 0.36),
            Pos2::new(o.x + s * 0.86, o.y + s * 0.5),
        ],
        stroke(p, c, w),
    ));
    p.add(egui::Shape::line(
        vec![
            Pos2::new(o.x + s * 0.66, o.y + s * 0.64),
            Pos2::new(o.x + s * 0.86, o.y + s * 0.5),
        ],
        stroke(p, c, w),
    ));
}

/// Download the full icon set into the closure-free helper used by the widgets.
pub fn paint(p: &Painter, which: Icon, r: Rect, c: Color32, w: f32) {
    match which {
        Icon::Back => back(p, r, c, w),
        Icon::Forward => forward(p, r, c, w),
        Icon::Reload => reload(p, r, c, w, false),
        Icon::Stop => reload(p, r, c, w, true),
        Icon::Home => home(p, r, c, w),
        Icon::Shield => shield(p, r, c, w),
        Icon::Star => star(p, r, c, w, false),
        Icon::StarFilled => star(p, r, c, w, true),
        Icon::Menu => menu(p, r, c, w),
        Icon::Plus => plus(p, r, c, w),
        Icon::Close => close(p, r, c, w),
        Icon::Search => search(p, r, c, w),
        Icon::Mic => mic(p, r, c, w),
        Icon::Camera => camera(p, r, c, w),
        Icon::Privacy => privacy(p, r, c, w),
        Icon::Gear => gear(p, r, c, w),
        Icon::Clock => clock(p, r, c, w),
        Icon::Download => download(p, r, c, w),
        Icon::Globe => globe(p, r, c, w),
        Icon::Lock => lock(p, r, c, w),
        Icon::Sun => sun(p, r, c, w),
        Icon::Cloud => cloud(p, r, c, w),
        Icon::Bookmark => bookmark(p, r, c, w),
        Icon::Volume => volume(p, r, c, w),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Back,
    Forward,
    Reload,
    Stop,
    Home,
    Shield,
    Star,
    StarFilled,
    Menu,
    Plus,
    Close,
    Search,
    Mic,
    Camera,
    Privacy,
    Gear,
    Clock,
    Download,
    Globe,
    Lock,
    Sun,
    Cloud,
    Bookmark,
    Volume,
}

/// Small inline helper so callers can draw without importing the painter.
pub fn draw(ui: &mut egui::Ui, which: Icon, size: f32, color: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        paint(ui.painter(), which, rect, color, (size / 24.0) * 1.8);
    }
    resp
}

/// Draw an icon centred on an absolute point, without consuming layout space.
///
/// Used by the new-tab page, which positions elements by coordinates rather
/// than by egui's linear layout.
pub fn draw_at(ui: &mut egui::Ui, which: Icon, size: Vec2, center: Pos2, color: Color32) {
    let rect = Rect::from_center_size(center, size);
    if ui.is_rect_visible(rect) {
        paint(ui.painter(), which, rect, color, (size.x / 24.0) * 1.8);
    }
}

/// Unused variant retained so `StrokeKind` stays referenced on all targets.
pub fn _stroke_kind() -> StrokeKind {
    StrokeKind::Middle
}
