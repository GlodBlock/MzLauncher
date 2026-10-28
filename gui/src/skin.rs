use std::collections::HashMap;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use eframe::egui;

#[derive(Clone)]
pub struct SkinTexture {
    pub handle: egui::TextureHandle,
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
    fallback: Option<egui::TextureHandle>,
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

    pub fn texture(&mut self, ctx: &egui::Context, id: &str) -> Option<SkinTexture> {
        let entry = self.entries.get_mut(id)?;
        if entry.texture.is_none() {
            let image = entry.image.take()?;
            let height = image.height() as u32;
            let handle = ctx.load_texture(
                format!("account-skin-{id}"),
                image,
                egui::TextureOptions::NEAREST,
            );
            entry.texture = Some(SkinTexture {
                handle,
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

    pub fn fallback(&mut self, ctx: &egui::Context) -> SkinTexture {
        let handle = self
            .fallback
            .get_or_insert_with(|| {
                let mut image =
                    egui::ColorImage::new([64, 64], egui::Color32::from_rgb(123, 145, 174));
                for y in 0..64 {
                    for x in 0..64 {
                        if (x / 8 + y / 8) % 2 == 0 {
                            image[(x, y)] = egui::Color32::from_rgb(143, 165, 194);
                        }
                    }
                }
                ctx.load_texture(
                    "account-skin-fallback",
                    image,
                    egui::TextureOptions::NEAREST,
                )
            })
            .clone();
        SkinTexture {
            handle,
            model: "classic".into(),
            height: 64,
        }
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

pub fn show_character(ui: &mut egui::Ui, size: egui::Vec2, skin: &SkinTexture, time: f64) {
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let mut faces = character_faces(&skin.model, skin.height, time);
    let yaw = 0.42 + (time as f32 * 0.35).sin() * 0.12;
    let pitch = -0.08;
    for face in &mut faces {
        for point in &mut face.points {
            *point = point.rotate_y(yaw).rotate_x(pitch);
        }
    }
    faces.sort_by(|a, b| {
        let depth_a = a.points.iter().map(|point| point.z).sum::<f32>();
        let depth_b = b.points.iter().map(|point| point.z).sum::<f32>();
        depth_a.total_cmp(&depth_b)
    });

    let scale = (rect.width() / 25.0).min(rect.height() / 38.0);
    let mut mesh = egui::Mesh::with_texture(skin.handle.id());
    for face in faces {
        let base = mesh.vertices.len() as u32;
        let color = shade_color(face.shade);
        let uvs = uv_corners(face.uv, skin.height as f32);
        for (point, uv) in face.points.into_iter().zip(uvs) {
            let perspective = 64.0 / (64.0 - point.z);
            mesh.vertices.push(egui::epaint::Vertex {
                pos: egui::pos2(
                    point.x * scale * perspective,
                    -point.y * scale * perspective,
                ),
                uv,
                color,
            });
        }
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    mesh.translate(rect.center() - mesh.calc_bounds().center());
    ui.painter().add(egui::Shape::mesh(mesh));
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

fn shade_color(shade: f32) -> egui::Color32 {
    let channel = (255.0 * shade).round().clamp(0.0, 255.0) as u8;
    egui::Color32::from_rgb(channel, channel, channel)
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
