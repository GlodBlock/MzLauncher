use std::collections::HashMap;
use std::sync::Arc;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use eframe::egui;

#[derive(Clone)]
pub struct SkinTexture {
    pub image: Arc<egui::ColorImage>,
    pub model: String,
    pub height: u32,
}

struct SkinEntry {
    source_url: Option<String>,
    loading: bool,
    failed: bool,
    image: Option<egui::ColorImage>,
    model: String,
    texture: Option<SkinTexture>,
}

#[derive(Default)]
pub struct SkinCache {
    entries: HashMap<String, SkinEntry>,
    fallback: Option<SkinTexture>,
}

impl SkinCache {
    pub fn retain_accounts(&mut self, ids: &[String]) {
        self.entries.retain(|id, _| ids.contains(id));
    }

    pub fn needs_request(&self, id: &str, source_url: &str) -> bool {
        let Some(entry) = self.entries.get(id) else {
            return true;
        };
        if entry.loading || entry.failed {
            return false;
        }
        entry.source_url.as_deref() != Some(source_url)
            || (entry.image.is_none() && entry.texture.is_none())
    }

    pub fn reset_pending(&mut self) {
        for entry in self.entries.values_mut() {
            if entry.loading {
                entry.loading = false;
            }
        }
    }

    pub fn begin_request(&mut self, id: &str, source_url: &str) {
        self.entries.insert(
            id.to_owned(),
            SkinEntry {
                source_url: Some(source_url.to_owned()),
                loading: true,
                failed: false,
                image: None,
                model: "classic".into(),
                texture: None,
            },
        );
    }

    pub fn complete(&mut self, id: &str, png_base64: &str, model: &str) -> Result<(), String> {
        let bytes = STANDARD
            .decode(png_base64)
            .map_err(|error| error.to_string())?;
        let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
            .map_err(|error| error.to_string())?
            .to_rgba8();
        let size = [image.width() as usize, image.height() as usize];
        let image = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
        let entry = self
            .entries
            .get_mut(id)
            .ok_or_else(|| "skin request expired".to_owned())?;
        entry.loading = false;
        entry.failed = false;
        entry.image = Some(image);
        entry.model = model.to_owned();
        entry.texture = None;
        Ok(())
    }

    pub fn fail(&mut self, id: &str) {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.loading = false;
            entry.failed = true;
        }
    }

    pub fn texture(&mut self, id: &str) -> Option<SkinTexture> {
        let entry = self.entries.get_mut(id)?;
        if entry.texture.is_none() {
            let image = Arc::new(entry.image.take()?);
            let height = image.height() as u32;
            entry.texture = Some(SkinTexture {
                image,
                model: entry.model.clone(),
                height,
            });
        }
        entry.texture.clone()
    }

    pub fn is_loading(&self, id: &str) -> bool {
        self.entries.get(id).is_some_and(|entry| entry.loading)
    }

    pub fn is_failed(&self, id: &str) -> bool {
        self.entries.get(id).is_some_and(|entry| entry.failed)
    }

    pub fn retry(&mut self, id: &str) {
        if let Some(entry) = self.entries.get_mut(id) {
            entry.failed = false;
        }
    }

    pub fn invalidate(&mut self, id: &str) {
        self.entries.remove(id);
    }

    pub fn fallback(&mut self) -> SkinTexture {
        if self.fallback.is_none() {
            let mut image = egui::ColorImage::new([64, 64], egui::Color32::from_rgb(123, 145, 174));
            for y in 0..64 {
                for x in 0..64 {
                    if (x / 8 + y / 8) % 2 == 0 {
                        image[(x, y)] = egui::Color32::from_rgb(143, 165, 194);
                    }
                }
            }
            self.fallback = Some(SkinTexture {
                image: Arc::new(image),
                model: "classic".into(),
                height: 64,
            });
        }
        self.fallback.clone().unwrap()
    }
}

#[derive(Clone, Copy, Default)]
struct Vec3 {
    x: f32,
    y: f32,
    z: f32,
}

impl Vec3 {
    fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }

    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }

    fn scale(self, factor: f32) -> Self {
        Self::new(self.x * factor, self.y * factor, self.z * factor)
    }

    fn rotate_x(self, angle: f32) -> Self {
        let (sin, cos) = angle.sin_cos();
        Self::new(
            self.x,
            self.y * cos - self.z * sin,
            self.y * sin + self.z * cos,
        )
    }

    fn rotate_y(self, angle: f32) -> Self {
        let (sin, cos) = angle.sin_cos();
        Self::new(
            self.x * cos + self.z * sin,
            self.y,
            -self.x * sin + self.z * cos,
        )
    }
}

#[derive(Clone, Copy)]
struct UvSet {
    top: [f32; 4],
    bottom: [f32; 4],
    right: [f32; 4],
    front: [f32; 4],
    left: [f32; 4],
    back: [f32; 4],
}

impl UvSet {
    fn cube(u: f32, v: f32, width: f32, height: f32, depth: f32) -> Self {
        Self {
            top: [u + depth, v, u + depth + width, v + depth],
            bottom: [u + depth + width, v, u + depth * 2.0 + width, v + depth],
            right: [u, v + depth, u + depth, v + depth + height],
            front: [u + depth, v + depth, u + depth + width, v + depth + height],
            left: [
                u + depth + width,
                v + depth,
                u + depth * 2.0 + width,
                v + depth + height,
            ],
            back: [
                u + depth * 2.0 + width,
                v + depth,
                u + depth * 2.0 + width * 2.0,
                v + depth + height,
            ],
        }
    }
}

struct ProjectedFace {
    points: [Vec3; 4],
    uv: [f32; 4],
    shade: f32,
}

fn character_faces(model: &str, height: u32, time: f64) -> Vec<ProjectedFace> {
    let mut faces = Vec::with_capacity(72);
    let phase = time as f32 * 4.2;
    let swing = phase.sin() * 0.55;
    let slim = model == "slim";
    let arm_width = if slim { 3.0 } else { 4.0 };
    let legacy = height < 64;

    push_part(
        &mut faces,
        Vec3::new(0.0, 26.0, 0.0),
        Vec3::new(8.0, 8.0, 8.0),
        Vec3::default(),
        0.0,
        UvSet::cube(0.0, 0.0, 8.0, 8.0, 8.0),
        1.0,
    );
    push_part(
        &mut faces,
        Vec3::new(0.0, 26.0, 0.0),
        Vec3::new(8.0, 8.0, 8.0),
        Vec3::default(),
        0.0,
        UvSet::cube(32.0, 0.0, 8.0, 8.0, 8.0),
        1.08,
    );
    push_part(
        &mut faces,
        Vec3::new(0.0, 16.0, 0.0),
        Vec3::new(8.0, 12.0, 4.0),
        Vec3::default(),
        0.0,
        UvSet::cube(16.0, 16.0, 8.0, 12.0, 4.0),
        1.0,
    );
    if !legacy {
        push_part(
            &mut faces,
            Vec3::new(0.0, 16.0, 0.0),
            Vec3::new(8.0, 12.0, 4.0),
            Vec3::default(),
            0.0,
            UvSet::cube(16.0, 32.0, 8.0, 12.0, 4.0),
            1.06,
        );
    }

    let right_arm_uv = UvSet::cube(40.0, 16.0, arm_width, 12.0, 4.0);
    let left_arm_uv = if legacy {
        right_arm_uv
    } else {
        UvSet::cube(32.0, 48.0, arm_width, 12.0, 4.0)
    };
    let arm_offset = 4.0 + arm_width / 2.0;
    push_part(
        &mut faces,
        Vec3::new(-arm_offset, 16.0, 0.0),
        Vec3::new(arm_width, 12.0, 4.0),
        Vec3::new(-arm_offset, 22.0, 0.0),
        swing,
        right_arm_uv,
        1.0,
    );
    push_part(
        &mut faces,
        Vec3::new(arm_offset, 16.0, 0.0),
        Vec3::new(arm_width, 12.0, 4.0),
        Vec3::new(arm_offset, 22.0, 0.0),
        -swing,
        left_arm_uv,
        1.0,
    );
    if !legacy {
        push_part(
            &mut faces,
            Vec3::new(-arm_offset, 16.0, 0.0),
            Vec3::new(arm_width, 12.0, 4.0),
            Vec3::new(-arm_offset, 22.0, 0.0),
            swing,
            UvSet::cube(40.0, 32.0, arm_width, 12.0, 4.0),
            1.08,
        );
        push_part(
            &mut faces,
            Vec3::new(arm_offset, 16.0, 0.0),
            Vec3::new(arm_width, 12.0, 4.0),
            Vec3::new(arm_offset, 22.0, 0.0),
            -swing,
            UvSet::cube(48.0, 48.0, arm_width, 12.0, 4.0),
            1.08,
        );
    }

    let right_leg_uv = UvSet::cube(0.0, 16.0, 4.0, 12.0, 4.0);
    let left_leg_uv = if legacy {
        right_leg_uv
    } else {
        UvSet::cube(16.0, 48.0, 4.0, 12.0, 4.0)
    };
    push_part(
        &mut faces,
        Vec3::new(-2.0, 6.0, 0.0),
        Vec3::new(4.0, 12.0, 4.0),
        Vec3::new(-2.0, 12.0, 0.0),
        -swing,
        right_leg_uv,
        1.0,
    );
    push_part(
        &mut faces,
        Vec3::new(2.0, 6.0, 0.0),
        Vec3::new(4.0, 12.0, 4.0),
        Vec3::new(2.0, 12.0, 0.0),
        swing,
        left_leg_uv,
        1.0,
    );
    if !legacy {
        push_part(
            &mut faces,
            Vec3::new(-2.0, 6.0, 0.0),
            Vec3::new(4.0, 12.0, 4.0),
            Vec3::new(-2.0, 12.0, 0.0),
            -swing,
            UvSet::cube(0.0, 32.0, 4.0, 12.0, 4.0),
            1.07,
        );
        push_part(
            &mut faces,
            Vec3::new(2.0, 6.0, 0.0),
            Vec3::new(4.0, 12.0, 4.0),
            Vec3::new(2.0, 12.0, 0.0),
            swing,
            UvSet::cube(0.0, 48.0, 4.0, 12.0, 4.0),
            1.07,
        );
    }
    faces
}

const CAMERA_DISTANCE: f32 = 64.0;
const RENDER_SUPERSAMPLE: f32 = 2.0;

#[derive(Clone, Copy, Default)]
struct RasterVertex {
    x: f32,
    y: f32,
    inv_w: f32,
    u_over_w: f32,
    v_over_w: f32,
}

#[derive(Clone)]
struct CharacterRender {
    texture: egui::TextureHandle,
    image: egui::ColorImage,
    depth: Vec<f32>,
}

pub fn show_character(ui: &mut egui::Ui, size: egui::Vec2, skin: &SkinTexture, time: f64) {
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let render_scale = ui.ctx().pixels_per_point() * RENDER_SUPERSAMPLE;
    let width = (rect.width() * render_scale).ceil().max(1.0) as usize;
    let height = (rect.height() * render_scale).ceil().max(1.0) as usize;
    let faces = character_faces(&skin.model, skin.height, time);
    let yaw = 0.42 + (time as f32 * 0.35).sin() * 0.12;
    let pitch = -0.08;
    let scale = (rect.width() / 25.0).min(rect.height() / 38.0) * render_scale;

    let mut projected = Vec::with_capacity(faces.len());
    let mut min = egui::vec2(f32::INFINITY, f32::INFINITY);
    let mut max = egui::vec2(f32::NEG_INFINITY, f32::NEG_INFINITY);
    for face in &faces {
        let mut vertices = [RasterVertex::default(); 4];
        let uvs = uv_corners(face.uv, skin.height as f32);
        for (slot, (point, uv)) in vertices.iter_mut().zip(face.points.into_iter().zip(uvs)) {
            let point = point.rotate_y(yaw).rotate_x(pitch);
            let inv_w = 1.0 / (CAMERA_DISTANCE - point.z);
            let perspective = CAMERA_DISTANCE * inv_w;
            let x = point.x * scale * perspective;
            let y = -point.y * scale * perspective;
            min = min.min(egui::vec2(x, y));
            max = max.max(egui::vec2(x, y));
            *slot = RasterVertex {
                x,
                y,
                inv_w,
                u_over_w: uv.x * inv_w,
                v_over_w: uv.y * inv_w,
            };
        }
        let shade = (face.shade * 255.0).round().clamp(0.0, 255.0) as u8;
        projected.push((vertices, shade));
    }
    let offset = egui::vec2(width as f32, height as f32) * 0.5 - (min + max) * 0.5;

    let render_id = ui.id().with("character-render");
    let cached = ui
        .ctx()
        .data_mut(|data| data.get_temp::<CharacterRender>(render_id))
        .is_some();
    if !cached {
        let render = CharacterRender {
            texture: ui.ctx().load_texture(
                "character-render",
                egui::ColorImage::new([1, 1], egui::Color32::TRANSPARENT),
                egui::TextureOptions::LINEAR,
            ),
            image: egui::ColorImage::new([1, 1], egui::Color32::TRANSPARENT),
            depth: vec![0.0],
        };
        ui.ctx()
            .data_mut(|data| data.insert_temp(render_id, render));
    }
    let texture_id = ui.ctx().data_mut(|data| {
        let render = data.get_temp_mut_or_insert_with::<CharacterRender>(render_id, || {
            unreachable!("character render cache initialized above")
        });
        if render.image.size != [width, height] {
            render.image = egui::ColorImage::new([width, height], egui::Color32::TRANSPARENT);
            render.depth = vec![0.0; width * height];
        } else {
            render.image.pixels.fill(egui::Color32::TRANSPARENT);
            render.depth.fill(0.0);
        }
        for (mut vertices, shade) in projected {
            for vertex in &mut vertices {
                vertex.x += offset.x;
                vertex.y += offset.y;
            }
            rasterize_triangle(
                &mut render.image,
                &mut render.depth,
                [vertices[0], vertices[1], vertices[2]],
                shade,
                &skin.image,
            );
            rasterize_triangle(
                &mut render.image,
                &mut render.depth,
                [vertices[0], vertices[2], vertices[3]],
                shade,
                &skin.image,
            );
        }
        render
            .texture
            .set(render.image.clone(), egui::TextureOptions::LINEAR);
        Some(render.texture.id())
    });
    let Some(texture_id) = texture_id else {
        return;
    };
    ui.painter().image(
        texture_id,
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
}

fn rasterize_triangle(
    image: &mut egui::ColorImage,
    depth: &mut [f32],
    [a, b, c]: [RasterVertex; 3],
    shade: u8,
    texture: &egui::ColorImage,
) {
    let edge = |from: RasterVertex, to: RasterVertex, x: f32, y: f32| {
        (x - from.x) * (to.y - from.y) - (y - from.y) * (to.x - from.x)
    };
    let area = edge(a, b, c.x, c.y);
    if area.abs() < f32::EPSILON {
        return;
    }
    let width = image.width();
    let height = image.height();
    let min_x = a.x.min(b.x).min(c.x).floor().max(0.0) as usize;
    let max_x = (a.x.max(b.x).max(c.x).ceil().max(0.0) as usize).min(width - 1);
    let min_y = a.y.min(b.y).min(c.y).floor().max(0.0) as usize;
    let max_y = (a.y.max(b.y).max(c.y).ceil().max(0.0) as usize).min(height - 1);
    let texture_width = texture.width();
    let texture_height = texture.height();
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let weight_a = edge(b, c, px, py) / area;
            let weight_b = edge(c, a, px, py) / area;
            let weight_c = edge(a, b, px, py) / area;
            if weight_a < 0.0 || weight_b < 0.0 || weight_c < 0.0 {
                continue;
            }
            let inv_w = weight_a * a.inv_w + weight_b * b.inv_w + weight_c * c.inv_w;
            if inv_w <= 0.0 {
                continue;
            }
            let index = y * width + x;
            if inv_w <= depth[index] {
                continue;
            }
            let u = (weight_a * a.u_over_w + weight_b * b.u_over_w + weight_c * c.u_over_w) / inv_w;
            let v = (weight_a * a.v_over_w + weight_b * b.v_over_w + weight_c * c.v_over_w) / inv_w;
            let texel_x = ((u * texture_width as f32) as usize).min(texture_width - 1);
            let texel_y = ((v * texture_height as f32) as usize).min(texture_height - 1);
            let sample = texture[(texel_x, texel_y)];
            if sample.a() == 0 {
                continue;
            }
            let shaded = egui::Color32::from_rgba_unmultiplied(
                (sample.r() as u16 * shade as u16 / 255) as u8,
                (sample.g() as u16 * shade as u16 / 255) as u8,
                (sample.b() as u16 * shade as u16 / 255) as u8,
                sample.a(),
            );
            image[(x, y)] = image[(x, y)].blend(shaded);
            if sample.a() > 127 {
                depth[index] = inv_w;
            }
        }
    }
}

fn push_part(
    faces: &mut Vec<ProjectedFace>,
    center: Vec3,
    size: Vec3,
    pivot: Vec3,
    angle: f32,
    uv: UvSet,
    expansion: f32,
) {
    let half = size.scale(0.5 * expansion);
    let transform = |point: Vec3| {
        if angle == 0.0 {
            point
        } else {
            point.sub(pivot).rotate_x(angle).add(pivot)
        }
    };
    let p = |x: f32, y: f32, z: f32| transform(center.add(Vec3::new(x, y, z)));
    let mut push = |points: [Vec3; 4], uv: [f32; 4], shade: f32| {
        faces.push(ProjectedFace { points, uv, shade });
    };
    push(
        [
            p(-half.x, half.y, half.z),
            p(half.x, half.y, half.z),
            p(half.x, -half.y, half.z),
            p(-half.x, -half.y, half.z),
        ],
        uv.front,
        1.0,
    );
    push(
        [
            p(half.x, half.y, -half.z),
            p(-half.x, half.y, -half.z),
            p(-half.x, -half.y, -half.z),
            p(half.x, -half.y, -half.z),
        ],
        uv.back,
        0.65,
    );
    push(
        [
            p(-half.x, half.y, -half.z),
            p(-half.x, half.y, half.z),
            p(-half.x, -half.y, half.z),
            p(-half.x, -half.y, -half.z),
        ],
        uv.left,
        0.76,
    );
    push(
        [
            p(half.x, half.y, half.z),
            p(half.x, half.y, -half.z),
            p(half.x, -half.y, -half.z),
            p(half.x, -half.y, half.z),
        ],
        uv.right,
        0.86,
    );
    push(
        [
            p(-half.x, half.y, -half.z),
            p(-half.x, half.y, half.z),
            p(half.x, half.y, half.z),
            p(half.x, half.y, -half.z),
        ],
        uv.top,
        1.0,
    );
    push(
        [
            p(-half.x, -half.y, half.z),
            p(-half.x, -half.y, -half.z),
            p(half.x, -half.y, -half.z),
            p(half.x, -half.y, half.z),
        ],
        uv.bottom,
        0.55,
    );
}

fn uv_corners(rect: [f32; 4], height: f32) -> [egui::Pos2; 4] {
    [
        egui::pos2(rect[0] / 64.0, rect[1] / height),
        egui::pos2(rect[2] / 64.0, rect[1] / height),
        egui::pos2(rect[2] / 64.0, rect[3] / height),
        egui::pos2(rect[0] / 64.0, rect[3] / height),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contains_front(faces: &[ProjectedFace], uv: UvSet) -> bool {
        faces.iter().any(|face| face.uv == uv.front)
    }

    #[test]
    fn modern_skins_include_all_six_overlay_regions() {
        let faces = character_faces("classic", 64, 0.0);
        assert_eq!(faces.len(), 72);
        for uv in [
            UvSet::cube(32.0, 0.0, 8.0, 8.0, 8.0),
            UvSet::cube(16.0, 32.0, 8.0, 12.0, 4.0),
            UvSet::cube(40.0, 32.0, 4.0, 12.0, 4.0),
            UvSet::cube(48.0, 48.0, 4.0, 12.0, 4.0),
            UvSet::cube(0.0, 32.0, 4.0, 12.0, 4.0),
            UvSet::cube(0.0, 48.0, 4.0, 12.0, 4.0),
        ] {
            assert!(contains_front(&faces, uv));
        }
    }

    #[test]
    fn legacy_skins_include_only_the_hat_overlay() {
        let faces = character_faces("classic", 32, 0.0);
        assert_eq!(faces.len(), 42);
        assert!(contains_front(
            &faces,
            UvSet::cube(32.0, 0.0, 8.0, 8.0, 8.0)
        ));
        assert!(!contains_front(
            &faces,
            UvSet::cube(16.0, 32.0, 8.0, 12.0, 4.0)
        ));
        assert!(!contains_front(
            &faces,
            UvSet::cube(40.0, 32.0, 4.0, 12.0, 4.0)
        ));
    }

    #[test]
    fn depth_buffer_resolves_occlusion_regardless_of_draw_order() {
        let mut texture = egui::ColorImage::new([2, 1], egui::Color32::RED);
        texture[(1, 0)] = egui::Color32::BLUE;

        let quad = |inv_w: f32, u0: f32, u1: f32| {
            let vertex = |x: f32, y: f32, u: f32, v: f32| RasterVertex {
                x,
                y,
                inv_w,
                u_over_w: u * inv_w,
                v_over_w: v * inv_w,
            };
            [
                [
                    vertex(0.0, 0.0, u0, 0.0),
                    vertex(4.0, 0.0, u1, 0.0),
                    vertex(4.0, 4.0, u1, 1.0),
                ],
                [
                    vertex(0.0, 0.0, u0, 0.0),
                    vertex(4.0, 4.0, u1, 1.0),
                    vertex(0.0, 4.0, u0, 1.0),
                ],
            ]
        };
        let near = quad(0.5, 0.0, 0.5);
        let far = quad(0.25, 0.5, 1.0);

        for order in [[near, far], [far, near]] {
            let mut image = egui::ColorImage::new([4, 4], egui::Color32::TRANSPARENT);
            let mut depth = vec![0.0; 16];
            for triangles in order {
                for triangle in triangles {
                    rasterize_triangle(&mut image, &mut depth, triangle, 255, &texture);
                }
            }
            assert_eq!(image[(2, 2)], egui::Color32::RED);
        }
    }

    #[test]
    fn preserves_semi_transparent_skin_pixels() {
        let pixels = image::RgbaImage::from_pixel(64, 64, image::Rgba([12, 34, 56, 128]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(pixels)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let encoded = STANDARD.encode(bytes.into_inner());
        let mut cache = SkinCache::default();
        cache.begin_request("account", "https://example.com/skin.png");
        cache.complete("account", &encoded, "classic").unwrap();
        let entry = cache.entries.get("account").unwrap();
        let pixel = entry.image.as_ref().unwrap().pixels[0];
        assert_eq!(
            pixel,
            egui::Color32::from_rgba_unmultiplied(12, 34, 56, 128)
        );
    }
}
