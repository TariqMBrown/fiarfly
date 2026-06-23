//! ROI editor panel — interactive polygon drawing with frame preview.

use eframe::egui;
use fiarfly_core::roi::{Roi, RoiSet};
use crate::state::{ActivePanel, AppState, DialogRequest, WorkerHandle, WorkerOutput};

// Predefined ROI colors (RGB), cycling.
const ROI_COLORS: &[[u8; 3]] = &[
    [255, 100, 100],
    [100, 220, 100],
    [100, 180, 255],
    [255, 200,  50],
    [220, 100, 255],
    [255, 150,  50],
    [100, 230, 200],
    [255, 120, 200],
];

/// Drawing tool mode.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum DrawMode {
    #[default]
    Draw,
    Pan,
    Select,
    Delete,
    /// Click to stamp a circular ROI of fixed radius centred on the cursor.
    StampCircle,
    /// Click to stamp a square ROI of fixed half-side centred on the cursor.
    StampSquare,
}

/// Per-frame draw state stored in egui memory.
#[derive(Clone)]
pub struct DrawState {
    pub mode: DrawMode,
    /// Vertices of polygon currently being drawn (image pixel coords, x=col y=row).
    pub in_progress: Vec<[f32; 2]>,
    pub selected: Option<usize>,
    /// ROI fill opacity.
    pub opacity: f32,
    /// Zoom multiplier (1.0 = fit-to-canvas).
    pub zoom: f32,
    /// Pan offset in screen pixels applied after centering.
    pub pan: egui::Vec2,
    /// Canvas rotation in degrees (counter-clockwise, around canvas center).
    pub rotation_deg: f32,
    /// Whether to show ROI labels on the canvas.
    pub show_labels: bool,
    /// Font size for ROI labels.
    pub label_font_size: f32,

    // --- Stamp mode parameters ---
    /// Radius of stamped circles in image pixels.
    pub stamp_radius_px: f32,
    /// Half-side of stamped squares in image pixels.
    pub stamp_half_side_px: f32,
    /// Number of vertices used to approximate a stamped circle.
    pub stamp_circle_vertices: usize,

    // --- Vertex drag (Select mode) ---
    /// `(roi_idx, vertex_idx)` of the vertex currently being dragged.
    pub dragging_vertex: Option<(usize, usize)>,
}

impl Default for DrawState {
    fn default() -> Self {
        Self {
            mode: DrawMode::Draw,
            in_progress: Vec::new(),
            selected: None,
            opacity: 0.25,
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
            rotation_deg: 0.0,
            show_labels: true,
            label_font_size: 10.0,
            stamp_radius_px: 8.0,
            stamp_half_side_px: 8.0,
            stamp_circle_vertices: 24,
            dragging_vertex: None,
        }
    }
}

pub fn show(ui: &mut egui::Ui, state: &mut AppState) {
    let mut draw_state = ui.memory_mut(|mem| {
        mem.data
            .get_temp_mut_or_insert_with::<DrawState>(egui::Id::new("draw_state"), DrawState::default)
            .clone()
    });

    // ── Heading + toolbar row 1 ──────────────────────────────────────────────
    ui.heading("ROI Editor");
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        ui.label("Tool:");
        ui.radio_value(&mut draw_state.mode, DrawMode::Draw,        "✏ Draw");
        ui.radio_value(&mut draw_state.mode, DrawMode::StampCircle, "● Circle")
            .on_hover_text("Click to stamp a circle of fixed radius centred on the cursor");
        ui.radio_value(&mut draw_state.mode, DrawMode::StampSquare, "■ Square")
            .on_hover_text("Click to stamp a square of fixed size centred on the cursor");
        ui.radio_value(&mut draw_state.mode, DrawMode::Pan,         "✋ Pan");
        ui.radio_value(&mut draw_state.mode, DrawMode::Select,      "↖ Select")
            .on_hover_text("Click an ROI to select; drag vertex handles to reshape");
        ui.radio_value(&mut draw_state.mode, DrawMode::Delete,      "✕ Delete");
        ui.separator();
        if !state.roi_show_projection {
            let n = state.n_frames().saturating_sub(1);
            // Play / Pause button.
            let icon = if state.playing { "⏸" } else { "▶" };
            if ui.button(icon).on_hover_text(if state.playing { "Pause" } else { "Play" }).clicked() {
                state.playing = !state.playing;
                if !state.playing {
                    state.last_frame_time = None;
                }
            }
            ui.label("Frame:");
            let old = state.current_frame;
            ui.add(egui::Slider::new(&mut state.current_frame, 0..=n));
            if state.current_frame != old {
                state.roi_texture_key = String::new(); // frame changed → reload texture
                if state.playing {
                    state.playing = false;
                    state.last_frame_time = None;
                }
            }
        }
    });

    // ── Stamp settings (only shown in stamp modes) ────────────────────────
    if matches!(draw_state.mode, DrawMode::StampCircle | DrawMode::StampSquare) {
        ui.horizontal(|ui| {
            match draw_state.mode {
                DrawMode::StampCircle => {
                    ui.label("Radius (px):");
                    ui.add(
                        egui::DragValue::new(&mut draw_state.stamp_radius_px)
                            .speed(0.25)
                            .range(1.0..=200.0),
                    );
                    ui.label("Vertices:");
                    ui.add(
                        egui::DragValue::new(&mut draw_state.stamp_circle_vertices)
                            .speed(1.0)
                            .range(6..=64),
                    )
                    .on_hover_text(
                        "How many polygon vertices approximate the circle. \
                         More vertices = smoother but slower; each vertex is \
                         independently draggable in Select mode.",
                    );
                }
                DrawMode::StampSquare => {
                    ui.label("Half-side (px):");
                    ui.add(
                        egui::DragValue::new(&mut draw_state.stamp_half_side_px)
                            .speed(0.25)
                            .range(1.0..=200.0),
                    );
                    ui.label(egui::RichText::new(
                        "Tip: switch to Select to drag any corner and warp into a rectangle.",
                    ).small().weak());
                }
                _ => {}
            }
        });
    }

    // ── Toolbar row 2: projection, LUT, zoom ────────────────────────────────
    ui.horizontal(|ui| {
        // Projection toggle
        let proj_btn_label = if state.roi_show_projection {
            "📽 Projection"
        } else {
            "📷 Live frame"
        };
        if ui.button(proj_btn_label).on_hover_text("Toggle between current frame and mean projection").clicked() {
            state.roi_show_projection = !state.roi_show_projection;
            state.roi_texture_key = String::new();
        }

        if state.projection_mean.is_none() {
            if state.tiff_reader.is_some() && state.worker.is_none() {
                if ui.button("Compute Projection").clicked() {
                    spawn_projection(state);
                }
            } else if state.worker.is_some() {
                ui.add(
                    egui::ProgressBar::new(state.progress)
                        .desired_width(100.0)
                        .animate(true),
                );
            }
        } else {
            ui.label(egui::RichText::new("✓ proj").small().weak());
        }

        ui.separator();

        // LUT sliders. Coupled so roi_lut_min can never exceed roi_lut_max
        // (min>max would later panic inside f32::clamp when building the LUT).
        ui.label("Min:");
        let old_min = state.roi_lut_min;
        ui.add(egui::Slider::new(
            &mut state.roi_lut_min,
            0.0..=(state.roi_lut_max - 0.001).max(0.0),
        ).step_by(0.01));
        ui.label("Max:");
        let old_max = state.roi_lut_max;
        ui.add(egui::Slider::new(
            &mut state.roi_lut_max,
            (state.roi_lut_min + 0.001).min(1.0)..=1.0,
        ).step_by(0.01));
        if state.roi_lut_min != old_min || state.roi_lut_max != old_max {
            state.roi_texture_key = String::new(); // LUT changed → reload texture
        }
        if ui.small_button("⟳").on_hover_text("Reset LUT to full range").clicked() {
            state.roi_lut_min = 0.0;
            state.roi_lut_max = 1.0;
            state.roi_texture_key = String::new();
        }

        ui.separator();

        // Zoom buttons
        ui.label("Zoom:");
        if ui.small_button("−").clicked() {
            draw_state.zoom = (draw_state.zoom / 1.4).max(0.1);
        }
        ui.label(format!("{:.1}×", draw_state.zoom));
        if ui.small_button("+").clicked() {
            draw_state.zoom = (draw_state.zoom * 1.4).min(40.0);
        }
        if ui.small_button("Fit").clicked() {
            draw_state.zoom        = 1.0;
            draw_state.pan         = egui::Vec2::ZERO;
            draw_state.rotation_deg = 0.0;
        }

        ui.separator();

        // Rotation
        ui.label("Rotate:");
        if ui.small_button("↺").on_hover_text("Rotate −90°").clicked() {
            draw_state.rotation_deg = (draw_state.rotation_deg - 90.0).rem_euclid(360.0);
            state.roi_texture_key = String::new();
        }
        ui.add(
            egui::DragValue::new(&mut draw_state.rotation_deg)
                .range(0.0..=359.9)
                .speed(0.5)
                .suffix("°"),
        );
        if ui.small_button("↻").on_hover_text("Rotate +90°").clicked() {
            draw_state.rotation_deg = (draw_state.rotation_deg + 90.0).rem_euclid(360.0);
            state.roi_texture_key = String::new();
        }
    });

    ui.add_space(4.0);

    // ── Main layout: canvas (left) + sidebar (right) ─────────────────────────
    let available  = ui.available_size();
    let sidebar_w  = 210.0;
    let canvas_w   = (available.x - sidebar_w - 12.0).max(100.0);
    let canvas_h   = (available.y - 56.0).max(100.0); // leave room for nav row

    ui.horizontal(|ui| {
        // ── Canvas ────────────────────────────────────────────────────────────
        ui.vertical(|ui| {
            let (canvas_rect, response) = ui.allocate_exact_size(
                egui::vec2(canvas_w, canvas_h),
                egui::Sense::click_and_drag(),
            );
            let painter = ui.painter_at(canvas_rect);

            // Dark background
            painter.rect_filled(canvas_rect, 0.0, egui::Color32::from_gray(28));

            if let Some((img_h, img_w)) = state.image_shape() {
                // ── Texture upload (cached by key) ────────────────────────────
                let desired_key = if state.roi_show_projection && state.projection_mean.is_some() {
                    format!("proj:{:.3}:{:.3}", state.roi_lut_min, state.roi_lut_max)
                } else {
                    format!("f{}:{:.3}:{:.3}", state.current_frame, state.roi_lut_min, state.roi_lut_max)
                };

                if state.roi_texture.is_none() || state.roi_texture_key != desired_key {
                    // Defensive: sort the LUT bounds so `clamp` can never panic
                    // even if state.roi_lut_min/max are set out-of-order by a
                    // non-UI code path.
                    let (lut_lo, lut_hi) = if state.roi_lut_min <= state.roi_lut_max {
                        (state.roi_lut_min, state.roi_lut_max)
                    } else {
                        (state.roi_lut_max, state.roi_lut_min)
                    };
                    let lut_range = (lut_hi - lut_lo).max(1e-6);

                    let to_u8 = |v: f32| -> u8 {
                        ((v.clamp(lut_lo, lut_hi) - lut_lo) / lut_range * 255.0) as u8
                    };

                    let pixels_opt: Option<Vec<u8>> =
                        if state.roi_show_projection {
                            state.projection_mean.as_ref().map(|p| p.iter().map(|&v| to_u8(v)).collect())
                        } else {
                            state.tiff_reader.as_ref().and_then(|r| {
                                r.get_frame(state.current_frame).ok().map(|f| f.iter().map(|&v| to_u8(v)).collect())
                            })
                        };

                    if let Some(pixels) = pixels_opt {
                        let img = egui::ColorImage::from_gray([img_w, img_h], &pixels);
                        let tex = ui.ctx().load_texture("roi_canvas", img, egui::TextureOptions::LINEAR);
                        state.roi_texture     = Some(tex);
                        state.roi_texture_key = desired_key;
                    }
                }

                // ── Scroll-wheel zoom centered on cursor ──────────────────────
                let scroll_y = ui.input(|i| i.raw_scroll_delta.y);
                if response.hovered() && scroll_y != 0.0 {
                    let factor = if scroll_y > 0.0 { 1.12_f32 } else { 1.0 / 1.12 };
                    let new_zoom = (draw_state.zoom * factor).clamp(0.1, 40.0);

                    // Zoom around the hovered pixel so that the image point under
                    // the cursor stays fixed.
                    if let Some(hover) = response.hover_pos() {
                        let fit_x = canvas_w / img_w as f32;
                        let fit_y = canvas_h / img_h as f32;
                        let fit   = fit_x.min(fit_y);

                        let old_scale = fit * draw_state.zoom;
                        let canvas_ctr = canvas_rect.center();
                        let old_origin = canvas_ctr
                            - egui::vec2(img_w as f32 * old_scale * 0.5,
                                         img_h as f32 * old_scale * 0.5)
                            + draw_state.pan;

                        // Image coordinate under cursor
                        let img_x = (hover.x - old_origin.x) / old_scale;
                        let img_y = (hover.y - old_origin.y) / old_scale;

                        let new_scale  = fit * new_zoom;
                        let new_origin = egui::pos2(
                            hover.x - img_x * new_scale,
                            hover.y - img_y * new_scale,
                        );
                        let base_origin = canvas_ctr
                            - egui::vec2(img_w as f32 * new_scale * 0.5,
                                         img_h as f32 * new_scale * 0.5);
                        draw_state.pan  = new_origin - base_origin;
                        draw_state.zoom = new_zoom;
                    } else {
                        draw_state.zoom = new_zoom;
                    }
                }

                // ── Pan: left-drag in Pan mode, middle-drag, or right-drag ──────
                if draw_state.mode == DrawMode::Pan && response.dragged_by(egui::PointerButton::Primary)
                    || response.dragged_by(egui::PointerButton::Middle)
                    || response.dragged_by(egui::PointerButton::Secondary)
                {
                    draw_state.pan += response.drag_delta();
                }

                // ── Set cursor based on active tool ───────────────────────────
                if response.hovered() {
                    match draw_state.mode {
                        DrawMode::Pan => {
                            let is_dragging = response.dragged_by(egui::PointerButton::Primary);
                            ui.ctx().set_cursor_icon(if is_dragging {
                                egui::CursorIcon::Grabbing
                            } else {
                                egui::CursorIcon::Grab
                            });
                        }
                        DrawMode::Delete => {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        DrawMode::Select => {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
                        }
                        DrawMode::Draw => {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                        }
                        DrawMode::StampCircle | DrawMode::StampSquare => {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                        }
                    }
                }

                // ── Compute transform (scale + pan + rotation) ────────────────
                let fit_scale = {
                    let fx = canvas_w / img_w as f32;
                    let fy = canvas_h / img_h as f32;
                    fx.min(fy)
                };
                let scale      = fit_scale * draw_state.zoom;
                let canvas_ctr = canvas_rect.center();
                let img_origin = canvas_ctr
                    - egui::vec2(img_w as f32 * scale * 0.5, img_h as f32 * scale * 0.5)
                    + draw_state.pan;

                let angle = draw_state.rotation_deg.to_radians();
                let cos_a = angle.cos();
                let sin_a = angle.sin();

                // Rotate a screen-space point around canvas_ctr.
                let rotate_pt = |p: egui::Pos2| -> egui::Pos2 {
                    let dx = p.x - canvas_ctr.x;
                    let dy = p.y - canvas_ctr.y;
                    egui::pos2(
                        canvas_ctr.x + dx * cos_a - dy * sin_a,
                        canvas_ctr.y + dx * sin_a + dy * cos_a,
                    )
                };
                // Inverse rotation (for screen → image coord).
                let unrotate_pt = |p: egui::Pos2| -> egui::Pos2 {
                    let dx = p.x - canvas_ctr.x;
                    let dy = p.y - canvas_ctr.y;
                    egui::pos2(
                        canvas_ctr.x + dx * cos_a + dy * sin_a,
                        canvas_ctr.y - dx * sin_a + dy * cos_a,
                    )
                };

                let img_to_screen = |p: [f32; 2]| -> egui::Pos2 {
                    let raw = egui::pos2(img_origin.x + p[0] * scale,
                                         img_origin.y + p[1] * scale);
                    rotate_pt(raw)
                };
                let screen_to_img = |sp: egui::Pos2| -> [f32; 2] {
                    let unrot = unrotate_pt(sp);
                    [(unrot.x - img_origin.x) / scale, (unrot.y - img_origin.y) / scale]
                };

                // ── Draw image texture (rotated quad via mesh) ────────────────
                if let Some(tex) = &state.roi_texture {
                    // Compute the four rotated corners of the image rect.
                    let tl = rotate_pt(img_origin);
                    let tr = rotate_pt(egui::pos2(
                        img_origin.x + img_w as f32 * scale, img_origin.y));
                    let bl = rotate_pt(egui::pos2(
                        img_origin.x, img_origin.y + img_h as f32 * scale));
                    let br = rotate_pt(egui::pos2(
                        img_origin.x + img_w as f32 * scale,
                        img_origin.y + img_h as f32 * scale));

                    let mut mesh = egui::Mesh::with_texture(tex.id());
                    mesh.vertices.push(egui::epaint::Vertex { pos: tl, uv: egui::pos2(0.0, 0.0), color: egui::Color32::WHITE });
                    mesh.vertices.push(egui::epaint::Vertex { pos: tr, uv: egui::pos2(1.0, 0.0), color: egui::Color32::WHITE });
                    mesh.vertices.push(egui::epaint::Vertex { pos: bl, uv: egui::pos2(0.0, 1.0), color: egui::Color32::WHITE });
                    mesh.vertices.push(egui::epaint::Vertex { pos: br, uv: egui::pos2(1.0, 1.0), color: egui::Color32::WHITE });
                    mesh.indices.extend_from_slice(&[0, 1, 2, 1, 3, 2]);
                    painter.add(egui::Shape::mesh(mesh));
                }

                // ── Draw completed ROIs ───────────────────────────────────────
                if let Some(roi_set) = &state.roi_set {
                    for (i, roi) in roi_set.rois.iter().enumerate() {
                        if roi.vertices.len() < 3 {
                            continue;
                        }
                        let color = egui::Color32::from_rgb(roi.color[0], roi.color[1], roi.color[2]);
                        let fill  = egui::Color32::from_rgba_unmultiplied(
                            roi.color[0], roi.color[1], roi.color[2],
                            (draw_state.opacity * 255.0) as u8,
                        );
                        let pts: Vec<egui::Pos2> =
                            roi.vertices.iter().map(|&v| img_to_screen(v)).collect();

                        // Triangulate with earcutr so concave polygons render correctly.
                        {
                            let coords: Vec<f64> = pts.iter()
                                .flat_map(|p| [p.x as f64, p.y as f64])
                                .collect();
                            let indices = earcutr::earcut(&coords, &[], 2)
                                .unwrap_or_default();
                            if !indices.is_empty() {
                                let mut mesh = egui::Mesh::default();
                                let uv = egui::pos2(0.0, 0.0);
                                for &p in &pts {
                                    mesh.vertices.push(egui::epaint::Vertex { pos: p, uv, color: fill });
                                }
                                for &i in &indices {
                                    mesh.indices.push(i as u32);
                                }
                                painter.add(egui::Shape::mesh(mesh));
                            }
                        }
                        // Outline stroke (drawn separately so it follows the polygon edge).
                        painter.add(egui::Shape::closed_line(
                            pts.clone(),
                            egui::Stroke::new(1.5, color),
                        ));

                        // Label at centroid
                        if draw_state.show_labels {
                            let c = roi.centroid();
                            let label_pos = img_to_screen(c);
                            painter.text(
                                label_pos,
                                egui::Align2::CENTER_CENTER,
                                &roi.label,
                                egui::FontId::proportional(draw_state.label_font_size),
                                egui::Color32::WHITE,
                            );
                        }

                        // Vertex handles when selected (draggable in Select mode).
                        if draw_state.selected == Some(i) {
                            let cursor = response.hover_pos();
                            for (vi, p) in pts.iter().enumerate() {
                                let active = draw_state.dragging_vertex == Some((i, vi));
                                let near = !active
                                    && draw_state.mode == DrawMode::Select
                                    && cursor.map(|c| p.distance(c) < 10.0).unwrap_or(false);
                                let (fill, stroke_color) = if active {
                                    (egui::Color32::YELLOW, egui::Color32::WHITE)
                                } else if near {
                                    (egui::Color32::from_rgb(255, 220, 120), egui::Color32::WHITE)
                                } else {
                                    (egui::Color32::from_rgba_unmultiplied(255, 255, 255, 200),
                                     egui::Color32::from_gray(20))
                                };
                                painter.circle_filled(*p, 4.0, fill);
                                painter.circle_stroke(
                                    *p, 4.0, egui::Stroke::new(1.0, stroke_color),
                                );
                            }
                        }
                    }
                }

                // ── Draw in-progress polygon ──────────────────────────────────
                if !draw_state.in_progress.is_empty() {
                    let pts: Vec<egui::Pos2> =
                        draw_state.in_progress.iter().map(|&p| img_to_screen(p)).collect();

                    // Edges
                    for w in pts.windows(2) {
                        painter.line_segment([w[0], w[1]], egui::Stroke::new(1.5, egui::Color32::YELLOW));
                    }
                    // Closing preview line to cursor
                    if let Some(cursor) = response.hover_pos() {
                        if canvas_rect.contains(cursor) {
                            let last = *pts.last().unwrap();
                            painter.line_segment(
                                [last, cursor],
                                egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(255, 255, 0, 120)),
                            );
                        }
                    }
                    // Vertex dots.
                    // First vertex: larger yellow = close target.
                    // Other vertices: smaller; turn red when cursor is within
                    // delete range (10 px) so users know clicking will remove it.
                    let cursor_pos = response.hover_pos();
                    for (i, &p) in pts.iter().enumerate() {
                        if i == 0 {
                            painter.circle_filled(p, 6.0, egui::Color32::YELLOW);
                        } else {
                            let near = cursor_pos
                                .map(|c| p.distance(c) < 10.0)
                                .unwrap_or(false);
                            let color = if near {
                                egui::Color32::from_rgb(255, 80, 80)
                            } else {
                                egui::Color32::YELLOW
                            };
                            painter.circle_filled(p, 4.0, color);
                        }
                    }
                }

                // ── Stamp preview at cursor ──────────────────────────────────
                if matches!(draw_state.mode, DrawMode::StampCircle | DrawMode::StampSquare) {
                    if let Some(cursor) = response.hover_pos() {
                        if canvas_rect.contains(cursor) {
                            let centre_img = screen_to_img(cursor);
                            let preview_pts: Vec<egui::Pos2> = match draw_state.mode {
                                DrawMode::StampCircle => circle_vertices(
                                    centre_img,
                                    draw_state.stamp_radius_px,
                                    draw_state.stamp_circle_vertices,
                                ),
                                DrawMode::StampSquare => {
                                    square_vertices(centre_img, draw_state.stamp_half_side_px)
                                }
                                _ => Vec::new(),
                            }
                            .into_iter()
                            .map(img_to_screen)
                            .collect();
                            if !preview_pts.is_empty() {
                                painter.add(egui::Shape::closed_line(
                                    preview_pts,
                                    egui::Stroke::new(
                                        1.5,
                                        egui::Color32::from_rgba_unmultiplied(255, 255, 0, 220),
                                    ),
                                ));
                                painter.circle_filled(
                                    cursor,
                                    2.0,
                                    egui::Color32::YELLOW,
                                );
                            }
                        }
                    }
                }

                // ── Crosshair hint while drawing (not in Pan mode) ───────────
                if draw_state.mode == DrawMode::Draw {
                    if let Some(cursor) = response.hover_pos() {
                        if canvas_rect.contains(cursor) {
                            let c = egui::Color32::from_rgba_unmultiplied(255, 255, 255, 60);
                            painter.line_segment(
                                [egui::pos2(canvas_rect.left(), cursor.y), egui::pos2(canvas_rect.right(), cursor.y)],
                                egui::Stroke::new(0.5, c),
                            );
                            painter.line_segment(
                                [egui::pos2(cursor.x, canvas_rect.top()), egui::pos2(cursor.x, canvas_rect.bottom())],
                                egui::Stroke::new(0.5, c),
                            );
                        }
                    }
                }

                // ── Vertex drag in Select mode ────────────────────────────────
                // On primary drag-start near a vertex of the selected ROI,
                // capture (roi_idx, vertex_idx); on subsequent drag updates,
                // move that vertex to the new image-pixel position.
                if draw_state.mode == DrawMode::Select {
                    if response.drag_started_by(egui::PointerButton::Primary) {
                        if let Some(cursor) = response.interact_pointer_pos() {
                            if let (Some(roi_set), Some(sel)) =
                                (state.roi_set.as_ref(), draw_state.selected)
                            {
                                if let Some(roi) = roi_set.rois.get(sel) {
                                    let mut best: Option<(usize, f32)> = None;
                                    for (vi, v) in roi.vertices.iter().enumerate() {
                                        let s = img_to_screen(*v);
                                        let d = s.distance(cursor);
                                        if d < 10.0 && best.is_none_or(|(_, bd)| d < bd) {
                                            best = Some((vi, d));
                                        }
                                    }
                                    if let Some((vi, _)) = best {
                                        draw_state.dragging_vertex = Some((sel, vi));
                                    }
                                }
                            }
                        }
                    }
                    if let Some((roi_idx, v_idx)) = draw_state.dragging_vertex {
                        if response.dragged_by(egui::PointerButton::Primary) {
                            if let Some(cursor) = response.interact_pointer_pos() {
                                if let Some(roi_set) = state.roi_set.as_mut() {
                                    if let Some(roi) = roi_set.rois.get_mut(roi_idx) {
                                        if let Some(v) = roi.vertices.get_mut(v_idx) {
                                            *v = screen_to_img(cursor);
                                        }
                                    }
                                }
                            }
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                        } else if !response.dragged() {
                            draw_state.dragging_vertex = None;
                        }
                    }
                } else {
                    draw_state.dragging_vertex = None;
                }

                // ── Input: left click (not in Pan mode, not while dragging a vertex) ──
                if draw_state.mode != DrawMode::Pan
                    && draw_state.dragging_vertex.is_none()
                    && response.clicked_by(egui::PointerButton::Primary) {
                    if let Some(click) = response.interact_pointer_pos() {
                        match draw_state.mode {
                            DrawMode::Draw => {
                                let n_verts = draw_state.in_progress.len();

                                // Close polygon: click near the first vertex (≥3 pts placed).
                                let close = n_verts >= 3 && {
                                    let first = img_to_screen(draw_state.in_progress[0]);
                                    first.distance(click) < 12.0
                                };

                                // Delete vertex: click near any already-placed vertex
                                // other than the first (Illustrator-style undo of last point).
                                // We check from the last vertex backwards so the most
                                // recently placed point wins when vertices are close together.
                                let delete_idx = if !close {
                                    (1..n_verts).rev().find(|&i| {
                                        img_to_screen(draw_state.in_progress[i])
                                            .distance(click) < 10.0
                                    })
                                } else {
                                    None
                                };

                                if close {
                                    let verts = std::mem::take(&mut draw_state.in_progress);
                                    let n_existing = state.roi_set.as_ref()
                                        .map(|r| r.rois.len()).unwrap_or(0);
                                    let color  = ROI_COLORS[n_existing % ROI_COLORS.len()];
                                    let n_v    = verts.len();
                                    let new_roi = Roi {
                                        id: String::new(),
                                        label: format!("ROI {}", n_existing + 1),
                                        group: None,
                                        color,
                                        vertices: verts,
                                    };
                                    let (h, w) = state.image_shape().unwrap_or((0, 0));
                                    let roi_set = state.roi_set.get_or_insert_with(|| {
                                        RoiSet::new("default", h, w)
                                    });
                                    roi_set.add_roi(new_roi);
                                    state.log(format!(
                                        "Added ROI {} ({n_v} vertices).", n_existing + 1
                                    ));
                                } else if let Some(idx) = delete_idx {
                                    draw_state.in_progress.remove(idx);
                                } else {
                                    draw_state.in_progress.push(screen_to_img(click));
                                }
                            }
                            DrawMode::Select => {
                                if let Some(roi_set) = &state.roi_set {
                                    let ip = screen_to_img(click);
                                    draw_state.selected = roi_set.rois.iter().position(|r| {
                                        r.contains_point(ip[0], ip[1])
                                    });
                                }
                            }
                            DrawMode::Delete => {
                                if let Some(roi_set) = &mut state.roi_set {
                                    let ip = screen_to_img(click);
                                    if let Some(i) = roi_set.rois.iter().position(|r| {
                                        r.contains_point(ip[0], ip[1])
                                    }) {
                                        let label = roi_set.rois[i].label.clone();
                                        roi_set.rois.remove(i);
                                        if draw_state.selected == Some(i) {
                                            draw_state.selected = None;
                                        }
                                        state.log(format!("Deleted {label}."));
                                    }
                                }
                            }
                            DrawMode::StampCircle => {
                                let centre = screen_to_img(click);
                                let verts = circle_vertices(
                                    centre,
                                    draw_state.stamp_radius_px,
                                    draw_state.stamp_circle_vertices,
                                );
                                stamp_roi(state, verts, "circle");
                            }
                            DrawMode::StampSquare => {
                                let centre = screen_to_img(click);
                                let verts = square_vertices(centre, draw_state.stamp_half_side_px);
                                stamp_roi(state, verts, "square");
                            }
                            DrawMode::Pan => {} // handled by drag above
                        }
                    }
                }

                // Right-click or Escape cancels in-progress polygon
                if (response.secondary_clicked()
                    || ui.input(|i| i.key_pressed(egui::Key::Escape)))
                    && !draw_state.in_progress.is_empty() {
                        draw_state.in_progress.clear();
                        state.log("ROI drawing cancelled.");
                    }

                // Pixel coords tooltip
                if let Some(hover) = response.hover_pos() {
                    if canvas_rect.contains(hover) {
                        let ip = screen_to_img(hover);
                        let x = ip[0].round() as i32;
                        let y = ip[1].round() as i32;
                        if x >= 0 && y >= 0 && (x as usize) < img_w && (y as usize) < img_h {
                            painter.text(
                                canvas_rect.right_bottom() - egui::vec2(4.0, 4.0),
                                egui::Align2::RIGHT_BOTTOM,
                                format!("x={x} y={y}"),
                                egui::FontId::monospace(10.0),
                                egui::Color32::from_rgba_unmultiplied(200, 200, 200, 180),
                            );
                        }
                    }
                }
            } else {
                // No file loaded
                painter.text(
                    canvas_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Open a TIFF file to begin drawing ROIs.",
                    egui::FontId::proportional(14.0),
                    egui::Color32::GRAY,
                );
            }
        });

        // ── Sidebar ───────────────────────────────────────────────────────────
        ui.vertical(|ui| {
            ui.set_width(sidebar_w);
            ui.label(egui::RichText::new("ROI LIST").strong());
            ui.separator();

            let list_h = (canvas_h - 130.0).max(40.0);
            egui::ScrollArea::vertical().max_height(list_h).show(ui, |ui| {
                if let Some(roi_set) = &mut state.roi_set {
                    let mut to_delete: Option<usize> = None;
                    for (i, roi) in roi_set.rois.iter_mut().enumerate() {
                        let is_sel = draw_state.selected == Some(i);
                        ui.horizontal(|ui| {
                            // Color swatch — opens color wheel on click.
                            let mut rgba = egui::Color32::from_rgb(
                                roi.color[0], roi.color[1], roi.color[2],
                            );
                            if ui.color_edit_button_srgba(&mut rgba).changed() {
                                roi.color = [rgba.r(), rgba.g(), rgba.b()];
                                // Invalidate canvas texture so ROI overlay redraws.
                                state.roi_texture_key = String::new();
                            }
                            let r = ui.selectable_label(is_sel, &roi.label);
                            if r.clicked() {
                                draw_state.selected = Some(i);
                            }
                            if ui.add(egui::Button::new(
                                egui::RichText::new("✕").color(egui::Color32::from_rgb(210, 60, 60))
                            ).small()).clicked() {
                                to_delete = Some(i);
                            }
                        });
                    }
                    if let Some(i) = to_delete {
                        let label = roi_set.rois[i].label.clone();
                        roi_set.rois.remove(i);
                        if draw_state.selected == Some(i) {
                            draw_state.selected = None;
                        }
                        state.log(format!("Deleted {label}."));
                    }
                } else {
                    ui.label(egui::RichText::new("(no ROIs yet)").weak());
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(
                        "Click on canvas to place vertices.\nClick near the first vertex to close."
                    ).small().weak());
                }
            });

            ui.add_space(4.0);
            ui.label("Opacity:");
            ui.add(egui::Slider::new(&mut draw_state.opacity, 0.05..=0.60));

            ui.add_space(4.0);
            ui.checkbox(&mut draw_state.show_labels, "Show labels");
            if draw_state.show_labels {
                ui.horizontal(|ui| {
                    ui.label("Label size:");
                    ui.add(egui::Slider::new(&mut draw_state.label_font_size, 6.0..=24.0).step_by(1.0));
                });
            }

            ui.add_space(8.0);
            ui.separator();
            ui.label(egui::RichText::new("ROI SETS").strong());

            if ui.button("Save ROI Set…").clicked() {
                state.pending_dialog = Some(DialogRequest::SaveRoiSet);
            }

            if ui.button("Load ROI Set…").clicked() {
                state.pending_dialog = Some(DialogRequest::LoadRoiSet);
            }

            if ui.button("Clear All ROIs").clicked() {
                if let Some(roi_set) = &mut state.roi_set {
                    roi_set.rois.clear();
                }
                draw_state.in_progress.clear();
                draw_state.selected = None;
            }
        });
    });

    // ── Navigation ────────────────────────────────────────────────────────────
    ui.horizontal(|ui| {
        if ui.button("← Back").clicked() {
            state.active_panel = ActivePanel::MotionCorrection;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let has_rois = state.roi_set.as_ref().map(|r| !r.rois.is_empty()).unwrap_or(false);
            if ui.add_enabled(has_rois, egui::Button::new("Continue to Traces →")).clicked() {
                state.active_panel = ActivePanel::SignalViewer;
            }
        });
    });

    // Persist draw_state
    ui.memory_mut(|mem| {
        *mem.data.get_temp_mut_or_insert_with::<DrawState>(
            egui::Id::new("draw_state"), DrawState::default,
        ) = draw_state;
    });
}

// ── Background worker: compute mean + max projection ─────────────────────────

fn spawn_projection(state: &mut AppState) {
    let path = match &state.source_path {
        Some(p) => p.clone(),
        None => {
            state.log("No file loaded.");
            return;
        }
    };

    let (progress_tx, progress_rx) = std::sync::mpsc::channel::<f32>();
    let (done_tx, done_rx)         = std::sync::mpsc::channel();

    std::thread::spawn(move || {
        let result: Result<WorkerOutput, fiarfly_core::FiarflyError> = (|| {
            let _ = progress_tx.send(0.0);
            let reader = fiarfly_core::io::TiffReader::open(&path)?;
            let n = reader.num_frames;
            let (h, w) = (reader.height, reader.width);

            // Accumulate in f64 to avoid precision loss
            let mut sum_buf = vec![0.0_f64; h * w];
            let mut max_buf = vec![f32::NEG_INFINITY; h * w];

            for i in 0..n {
                let frame = reader.get_frame(i)?;
                for (j, &v) in frame.iter().enumerate() {
                    sum_buf[j] += v as f64;
                    if v > max_buf[j] {
                        max_buf[j] = v;
                    }
                }
                if i % 20 == 0 {
                    let _ = progress_tx.send(i as f32 / n as f32);
                }
            }

            let mean_data: Vec<f32> = sum_buf.iter().map(|&s| (s / n as f64) as f32).collect();
            let mean = fiarfly_core::Frame::from_shape_vec((h, w), mean_data)
                .map_err(|_| fiarfly_core::FiarflyError::DimensionMismatch("projection shape".into()))?;
            let max = fiarfly_core::Frame::from_shape_vec((h, w), max_buf)
                .map_err(|_| fiarfly_core::FiarflyError::DimensionMismatch("projection shape".into()))?;

            Ok(WorkerOutput::ProjectionComputed { mean, max })
        })();
        let _ = done_tx.send(result);
    });

    state.worker   = Some(WorkerHandle { progress_rx, done_rx });
    state.progress = 0.0;
    state.log("Computing mean/max projection…");
}

// ──────────────────────────────────────────────────────────────────────────
// Stamp helpers (PR9) — generate polygon vertices for click-to-stamp shapes.
// ──────────────────────────────────────────────────────────────────────────

/// `n` evenly-spaced points on a circle of `radius` around `centre`,
/// in image-pixel coordinates `[x, y]`. The polygon is returned open
/// (first vertex not repeated at the end), matching the rest of the ROI
/// pipeline's convention.
fn circle_vertices(centre: [f32; 2], radius: f32, n: usize) -> Vec<[f32; 2]> {
    let n = n.max(3);
    let r = radius.max(0.5);
    (0..n)
        .map(|i| {
            let theta = (i as f32 / n as f32) * std::f32::consts::TAU;
            [
                centre[0] + r * theta.cos(),
                centre[1] + r * theta.sin(),
            ]
        })
        .collect()
}

/// Four corners of an axis-aligned square of `2 * half_side` per side,
/// centred on `centre`, in clockwise order.
fn square_vertices(centre: [f32; 2], half_side: f32) -> Vec<[f32; 2]> {
    let h = half_side.max(0.5);
    vec![
        [centre[0] - h, centre[1] - h],
        [centre[0] + h, centre[1] - h],
        [centre[0] + h, centre[1] + h],
        [centre[0] - h, centre[1] + h],
    ]
}

/// Append a stamped ROI to the active ROI set, generating an id, label,
/// and color exactly the way the freehand-draw path does.
fn stamp_roi(state: &mut AppState, vertices: Vec<[f32; 2]>, kind: &str) {
    let n_existing = state
        .roi_set
        .as_ref()
        .map(|r| r.rois.len())
        .unwrap_or(0);
    let color = ROI_COLORS[n_existing % ROI_COLORS.len()];
    let new_roi = Roi {
        id: String::new(),
        label: format!("ROI {}", n_existing + 1),
        group: None,
        color,
        vertices,
    };
    let (h, w) = state.image_shape().unwrap_or((0, 0));
    let roi_set = state
        .roi_set
        .get_or_insert_with(|| RoiSet::new("default", h, w));
    roi_set.add_roi(new_roi);
    state.log(format!("Stamped {kind} ROI {}.", n_existing + 1));
}

#[cfg(test)]
mod stamp_tests {
    use super::*;

    #[test]
    fn circle_has_requested_vertex_count() {
        let v = circle_vertices([10.0, 10.0], 5.0, 16);
        assert_eq!(v.len(), 16);
        // First vertex on the +x axis from centre.
        assert!((v[0][0] - 15.0).abs() < 1e-3);
        assert!((v[0][1] - 10.0).abs() < 1e-3);
    }

    #[test]
    fn circle_vertices_are_on_radius() {
        let cx = 20.0_f32;
        let cy = 30.0_f32;
        let r = 7.5_f32;
        let v = circle_vertices([cx, cy], r, 24);
        for [x, y] in v {
            let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
            assert!((d - r).abs() < 1e-3, "vertex distance was {d}, expected {r}");
        }
    }

    #[test]
    fn circle_clamps_low_vertex_count() {
        let v = circle_vertices([0.0, 0.0], 1.0, 1);
        assert!(v.len() >= 3, "circle must have at least 3 vertices, got {}", v.len());
    }

    #[test]
    fn square_corners_match_half_side() {
        let v = square_vertices([5.0, 5.0], 2.0);
        assert_eq!(v.len(), 4);
        assert_eq!(v, vec![[3.0, 3.0], [7.0, 3.0], [7.0, 7.0], [3.0, 7.0]]);
    }
}
