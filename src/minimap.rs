//! Satellite/road moving map. Bounded background tile loading; no second scene render.
mod data;

use crate::{geo::{self, TileId}, sources::Sources};
use egui::{Color32, Pos2, Stroke, Vec2};
use std::{collections::HashMap, sync::{Arc, mpsc}, time::{Duration, Instant}};

struct Tile {
    texture: Option<egui::TextureHandle>,
    touched: u64,
    retry: Instant,
}

struct MapTile {
    texture: Option<egui::TextureHandle>,
    places: Vec<data::Place>,
    touched: u64,
    retry: Instant,
}

pub struct Minimap {
    pub visible: bool,
    pub landing_guidance: bool,
    pub heading_up: bool,
    pub zoom: u8,
    pub plan: bool,
    pub size: Vec2,
    /// Distance from the window's bottom edge, kept clear of the info panel below.
    pub bottom: f32,
    /// Lowest point of the HUD panel above, which the map must not reach.
    pub top: f32,
    resize_start: f32,
    maps: HashMap<TileId, MapTile>,
    map_pending: Option<(TileId, mpsc::Receiver<Option<data::Data>>)>,
    heading: f64,
    scroll: f32,
    tiles: HashMap<TileId, Tile>,
    pending: Option<(TileId, u32, mpsc::Receiver<Option<image::RgbaImage>>)>,
    generation: u32,
    frame: u64,
    runway_airports: Vec<u32>,
    runway_center: Option<glam::DVec3>,
}

fn rotate(v: Vec2, angle: f64) -> Vec2 {
    let (s, c) = (angle as f32).sin_cos();
    Vec2::new(c * v.x - s * v.y, s * v.x + c * v.y)
}

/// Whether `r` lies wholly inside the disc.
fn in_disc(r: egui::Rect, center: Pos2, radius: f32) -> bool {
    [r.left_top(), r.right_top(), r.left_bottom(), r.right_bottom()].iter().all(|p| p.distance(center) <= radius)
}

/// Spots labels must leave free: the aircraft, the north mark and the scale bar.
fn reserved(rect: egui::Rect, angle: f64) -> Vec<egui::Rect> {
    let (center, radius) = (rect.center(), rect.width() * 0.5);
    let north = center + rotate(Vec2::new(0.0, -(radius - 14.0)), angle);
    vec![
        egui::Rect::from_center_size(center, Vec2::splat(38.0)),
        egui::Rect::from_center_size(north, Vec2::splat(26.0)),
        egui::Rect::from_center_size(Pos2::new(center.x, rect.bottom() - 32.0), Vec2::new(72.0, 30.0)),
    ]
}

const DISC_SIDES: usize = 96;

fn lerp_vertex(a: &egui::epaint::Vertex, b: &egui::epaint::Vertex, t: f32) -> egui::epaint::Vertex {
    let c = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    egui::epaint::Vertex {
        pos: a.pos + (b.pos - a.pos) * t,
        uv: a.uv + (b.uv - a.uv) * t,
        color: Color32::from_rgba_premultiplied(c(a.color.r(), b.color.r()), c(a.color.g(), b.color.g()), c(a.color.b(), b.color.b()), c(a.color.a(), b.color.a())),
    }
}

/// Cuts a mesh down to a disc (a fine regular polygon), interpolating uv and colour.
fn clip_mesh(mesh: &egui::Mesh, center: Pos2, radius: f32) -> egui::Mesh {
    let inner = radius * (std::f32::consts::PI / DISC_SIDES as f32).cos();
    let mut out = egui::Mesh::with_texture(mesh.texture_id);
    for tri in mesh.indices.chunks_exact(3) {
        let v = [tri[0], tri[1], tri[2]].map(|i| mesh.vertices[i as usize]);
        if v.iter().all(|v| v.pos.distance(center) <= inner) {
            let base = out.vertices.len() as u32;
            out.vertices.extend(v);
            out.indices.extend([base, base + 1, base + 2]);
            continue;
        }
        let bounds = egui::Rect::from_points(&v.map(|v| v.pos));
        if bounds.distance_to_pos(center) > radius { continue; }
        let mut poly = v.to_vec();
        for k in 0..DISC_SIDES {
            let a = k as f32 / DISC_SIDES as f32 * std::f32::consts::TAU;
            let normal = Vec2::new(a.cos(), a.sin());
            let side = |p: Pos2| inner - (p - center).dot(normal);
            let mut next = Vec::with_capacity(poly.len() + 1);
            for i in 0..poly.len() {
                let (p, q) = (&poly[i], &poly[(i + 1) % poly.len()]);
                let (sp, sq) = (side(p.pos), side(q.pos));
                if sp >= 0.0 { next.push(*p); }
                if (sp >= 0.0) != (sq >= 0.0) { next.push(lerp_vertex(p, q, sp / (sp - sq))); }
            }
            poly = next;
            if poly.len() < 3 { break; }
        }
        if poly.len() < 3 { continue; }
        let base = out.vertices.len() as u32;
        for i in 1..poly.len() as u32 - 1 { out.indices.extend([base, base + i, base + i + 1]); }
        out.vertices.extend(poly);
    }
    out
}

fn clip_shape(tessellator: &mut egui::epaint::Tessellator, shape: egui::Shape, center: Pos2, radius: f32, out: &mut Vec<egui::Shape>) {
    let mesh = match shape {
        egui::Shape::Noop => return,
        egui::Shape::Vec(shapes) => {
            for s in shapes { clip_shape(tessellator, s, center, radius, out); }
            return;
        }
        egui::Shape::Mesh(mesh) => clip_mesh(&mesh, center, radius),
        other => {
            let mut mesh = egui::Mesh::default();
            tessellator.tessellate_shape(other, &mut mesh);
            clip_mesh(&mesh, center, radius)
        }
    };
    if !mesh.is_empty() { out.push(egui::Shape::mesh(mesh)); }
}

/// Replaces the shapes painted on `layer` since `start` with copies cut to the disc.
fn clip_to_disc(ctx: &egui::Context, layer: egui::LayerId, start: egui::layers::ShapeIdx, center: Pos2, radius: f32) {
    let font_size = ctx.fonts(|f| f.font_image_size());
    let mut tessellator = egui::epaint::Tessellator::new(ctx.pixels_per_point(), ctx.tessellation_options(|o| *o), font_size, Vec::new());
    ctx.graphics_mut(|g| {
        let list = g.entry(layer);
        let end = list.next_idx().0;
        for i in start.0..end {
            list.mutate_shape(egui::layers::ShapeIdx(i), |clipped| {
                let shape = std::mem::replace(&mut clipped.shape, egui::Shape::Noop);
                let mut meshes = Vec::new();
                clip_shape(&mut tessellator, shape, center, radius, &mut meshes);
                clipped.shape = egui::Shape::Vec(meshes);
            });
        }
    });
}

fn tile_offset(x: f64, center: f64, n: f64) -> f64 {
    (x - center + n * 0.5).rem_euclid(n) - n * 0.5
}

/// Where a latitude and longitude (radians) falls on the disc, taking the
/// nearest copy across the antimeridian and turning it with the map.
fn map_point(lat: f64, lon: f64, cx: f64, cy: f64, n: f64, angle: f64, center: Pos2) -> Pos2 {
    let x = geo::lon_to_merc_x(lon).rem_euclid(1.0) * n;
    let y = geo::lat_to_merc_y(lat) * n;
    center + rotate(Vec2::new((tile_offset(x, cx, n) * 256.0) as f32, ((y - cy) * 256.0) as f32), angle)
}

/// Where a contact is drawn: at its point while that is on the disc,
/// otherwise pinned to the rim in the same direction so it still shows which
/// way to turn.
fn blip_point(at: Pos2, center: Pos2, radius: f32) -> Pos2 {
    let off = at - center;
    let d = off.length();
    if d <= radius || d <= 0.0 {
        at
    } else {
        center + off * (radius / d)
    }
}

fn distance_label(meters: f64, lang: crate::i18n::Lang) -> String {
    let text = if meters >= 1000.0 { format!("{:.1} km", meters / 1000.0) } else { format!("{meters:.0} m") };
    if lang == crate::i18n::Lang::Fr { text.replace('.', ",") } else { text }
}

impl Minimap {
    fn draw_runways(&mut self, world: &crate::world::World, painter: &egui::Painter, rect: egui::Rect, lat: f64, lon: f64, cx: f64, cy: f64, n: f64, angle: f64) {
        // Below this zoom individual runways are too small to distinguish.
        if self.zoom < 9 { return; }
        let db = crate::airports::Db::get();
        let position = geo::lla_to_pos(lat, lon, 0.0);
        if self.runway_center.is_none_or(|center| (position - center).length() > 10_000.0) {
            self.runway_airports = db.near(lat.to_degrees(), lon.to_degrees(), 120_000.0).into_iter().map(|(id, _)| id).collect();
            self.runway_center = Some(position);
        }
        let project = |(la, lo): (f64, f64)| {
            let x = geo::lon_to_merc_x(lo.to_radians()).rem_euclid(1.0) * n;
            let y = geo::lat_to_merc_y(la.to_radians()) * n;
            rect.center() + rotate(Vec2::new((tile_offset(x, cx, n) * 256.0) as f32, ((y - cy) * 256.0) as f32), angle)
        };
        let color = Color32::from_rgb(95, 235, 255);
        let mut occupied = reserved(rect, angle);
        for &id in &self.runway_airports {
            let airport = &db.apts[id as usize];
            // Prefer the exact geometry used by the 3D airfield and approach guide.
            // Some database entries (including EBNM) have no endpoint coordinates.
            let loaded = world.airfield(id).filter(|field| !field.geo.runways.is_empty());
            let runways: Vec<_> = if let Some(field) = loaded {
                field.geo.runways.iter().map(|r| {
                    let endpoints = [r.a, r.b].map(|p| {
                        let (lat, lon) = field.geo.lla(p);
                        (lat.to_degrees(), lon.to_degrees())
                    });
                    (endpoints, &r.ident)
                }).collect()
            } else {
                airport.rwys.iter().filter_map(|r| {
                    let [Some(a), Some(b)] = r.pos else { return None; };
                    Some(([a, b], &r.ident))
                }).collect()
            };
            for ([a, b], ident) in runways {
                let ends = [project(a), project(b)];
                if !egui::Rect::from_two_pos(ends[0], ends[1]).expand(4.0).intersects(rect) { continue; }
                let length = ends[0].distance(ends[1]);
                if length < 2.0 { continue; }
                painter.line_segment(ends, Stroke::new(6.0, Color32::from_black_alpha(220)));
                painter.line_segment(ends, Stroke::new(3.0, color));
                if self.zoom < 11 || length < 18.0 { continue; }
                let direction = (ends[1] - ends[0]) / length;
                let side = Vec2::new(-direction.y, direction.x);
                for end in ends {
                    painter.line_segment([end - side * 4.0, end + side * 4.0], Stroke::new(2.0, color));
                }
                let text = format!("{}  {}/{}", airport.ident, ident[0], ident[1]);
                let galley = painter.layout_no_wrap(text, egui::FontId::proportional(11.0), color);
                let midpoint = ends[0] + (ends[1] - ends[0]) * 0.5;
                let Some(label) = [-1.0, 1.0].into_iter().map(|sign| {
                    egui::Rect::from_center_size(midpoint + Vec2::new(0.0, sign * (galley.size().y * 0.5 + 8.0)), galley.size() + Vec2::new(6.0, 4.0))
                }).find(|r| in_disc(*r, rect.center(), rect.width() * 0.5 - 6.0) && !occupied.iter().any(|other| other.intersects(*r))) else { continue; };
                painter.rect_filled(label, 3.0, Color32::from_black_alpha(225));
                painter.galley(label.min + Vec2::new(3.0, 2.0), galley, color);
                occupied.push(label);
            }
        }
    }

    fn draw_map(&mut self, _ctx: &egui::Context, src: &Arc<Sources>, painter: &egui::Painter, rect: egui::Rect, cx: f64, cy: f64, n: f64, angle: f64) -> usize {
        let z = self.zoom.min(14);
        let factor = 2f64.powi((self.zoom - z) as i32);
        let (mx, my, count) = (cx / factor, cy / factor, (1u32 << z) as i64);
        let mut wanted = Vec::new();
        let mut visible = Vec::new();
        let mut drawn = 0;
        for dy in -2..=2 { for dx in -2..=2 {
            let (x, y) = (mx.floor() as i64 + dx, my.floor() as i64 + dy);
            if y < 0 || y >= count { continue; }
            let id = TileId::new(z, x.rem_euclid(count) as u32, y as u32);
            let offset = Vec2::new(tile_offset(x as f64, mx, count as f64) as f32, (y as f64 - my) as f32);
            let corners = [Vec2::ZERO, Vec2::X, Vec2::splat(1.0), Vec2::Y].map(|v| rect.center() + rotate((offset + v) * (256.0 * factor as f32), angle));
            if !egui::Rect::from_points(&corners).intersects(rect) { continue; }
            visible.push(id);
            wanted.push((dx * dx + dy * dy, id));
            if let Some(tile) = self.maps.get_mut(&id) {
                tile.touched = self.frame;
                if self.plan {
                    if let Some(texture) = &tile.texture {
                        let mut mesh = egui::Mesh::with_texture(texture.id());
                        for (pos, uv) in corners.into_iter().zip([[0.0,0.0],[1.0,0.0],[1.0,1.0],[0.0,1.0]]) {
                            mesh.vertices.push(egui::epaint::Vertex { pos, uv: Pos2::new(uv[0],uv[1]), color: Color32::WHITE });
                        }
                        mesh.indices.extend([0,1,2,0,2,3]);
                        painter.add(mesh);
                        drawn += 1;
                    }
                }
            }
        } }
        if self.map_pending.is_none() {
            wanted.sort_by_key(|(d, _)| *d);
            if let Some((_, id)) = wanted.into_iter().find(|(_, id)| self.maps.get(id).is_none_or(|t| t.texture.is_none() && Instant::now() >= t.retry)) {
                let (tx, rx) = mpsc::channel();
                let src = Arc::clone(src);
                self.map_pending = Some((id, rx));
                std::thread::spawn(move || { let _ = tx.send(data::load(&src, id)); });
            }
        }
        let mut labels: Vec<_> = visible.iter().filter_map(|id| self.maps.get(id)).flat_map(|tile| &tile.places)
            .filter(|p| self.zoom >= p.min_zoom).collect();
        labels.sort_by(|a, b| a.priority.cmp(&b.priority).then(a.name.cmp(&b.name)).then(a.xy[0].total_cmp(&b.xy[0])));
        let mut occupied = reserved(rect, angle);
        let mut names = std::collections::HashSet::new();
        for place in labels {
            let name = place.label(crate::i18n::lang());
            if names.contains(name) { continue; }
            let offset = Vec2::new((tile_offset(place.xy[0] * n, cx, n) * 256.0) as f32, ((place.xy[1] * n - cy) * 256.0) as f32);
            let point = rect.center() + rotate(offset, angle);
            let font = egui::FontId::proportional(if place.priority == 0 { 13.0 } else { 11.0 });
            let galley = painter.layout_no_wrap(name.to_owned(), font, Color32::WHITE);
            // Try above and below so a town underneath the aircraft can still be named.
            let Some(label_rect) = [-1.0, 1.0].into_iter().map(|side| egui::Rect::from_center_size(point + Vec2::new(0.0, side * (galley.size().y * 0.5 + 6.0)), galley.size() + Vec2::new(6.0, 2.0)))
                .find(|r| in_disc(*r, rect.center(), rect.width() * 0.5 - 6.0) && !occupied.iter().any(|other| other.intersects(r.expand(3.0)))) else { continue };
            painter.circle_filled(point, 2.5, Color32::WHITE);
            painter.circle_stroke(point, 2.5, Stroke::new(1.0, Color32::from_gray(40)));
            painter.rect_filled(label_rect, 3.0, Color32::from_black_alpha(185));
            painter.galley(label_rect.min + Vec2::new(3.0, 1.0), galley, Color32::WHITE);
            occupied.push(label_rect);
            names.insert(name);
            if names.len() >= 8 { break; }
        }
        drawn
    }

    pub fn new(prefs: &crate::settings::Settings) -> Self {
        Self { visible: prefs.minimap, landing_guidance: prefs.landing_guidance, heading_up: prefs.minimap_heading_up, zoom: prefs.minimap_zoom,
            size: Vec2::splat(prefs.minimap_size), bottom: 65.0, top: 0.0, resize_start: 0.0, plan: prefs.minimap_plan, maps: HashMap::new(), map_pending: None, heading: 0.0, scroll: 0.0, tiles: HashMap::new(), pending: None, generation: 0, frame: 0, runway_airports: Vec::new(), runway_center: None }
    }

    fn poll(&mut self, ctx: &egui::Context, generation: u32) {
        if let Some((id, rx)) = &self.map_pending {
            match rx.try_recv() {
                Ok(result) => {
                    let (texture, places) = result.map(|d| {
                        let texture = ctx.load_texture(format!("minimap-plan-{id:?}"), egui::ColorImage::from_rgba_unmultiplied([256, 256], d.image.as_raw()), egui::TextureOptions::LINEAR);
                        (Some(texture), d.places)
                    }).unwrap_or_default();
                    self.maps.insert(*id, MapTile { texture, places, touched: self.frame, retry: Instant::now() + Duration::from_secs(30) });
                    self.map_pending = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => { self.map_pending = None; }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        while self.maps.len() > 32 {
            let oldest = *self.maps.iter().min_by_key(|(_, t)| t.touched).unwrap().0;
            self.maps.remove(&oldest);
        }
        if generation != self.generation {
            self.tiles.clear();
            self.generation = generation;
        }
        if let Some((id, gen_id, rx)) = &self.pending {
            match rx.try_recv() {
                Ok(result) => {
                    if *gen_id == generation {
                        let texture = result.map(|img| {
                            ctx.load_texture(format!("minimap-{id:?}-{generation}"),
                                egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()),
                                egui::TextureOptions::LINEAR)
                        });
                        self.tiles.insert(*id, Tile { texture, touched: self.frame, retry: Instant::now() + Duration::from_secs(30) });
                    }
                    self.pending = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => { self.pending = None; }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        while self.tiles.len() > 64 {
            let oldest = *self.tiles.iter().min_by_key(|(_, t)| t.touched).unwrap().0;
            self.tiles.remove(&oldest);
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, src: &Arc<Sources>, lat: f64, lon: f64, heading: Option<f64>, info_open: bool, guide: Option<&crate::approach::Guide>, world: &crate::world::World, zone: Option<(f64, f64, f64)>, contacts: &[(f64, f64)]) {
        self.frame += 1;
        self.poll(ctx, src.google.generation());
        if let Some(h) = heading { self.heading = h; }
        let reserved_width = if info_open { 426.0 } else { 44.0 };
        let limit = (ctx.content_rect().width() - reserved_width).min(ctx.content_rect().bottom() - self.bottom - 44.0 - self.top - 8.0).clamp(120.0, 640.0);
        let diameter = self.size.x.clamp(240.0, 640.0).min(limit);
        egui::Area::new(egui::Id::unique("minimap")).anchor(egui::Align2::LEFT_BOTTOM, [14.0, -self.bottom]).show(ctx, |ui| {
            ui.set_width(diameter);
            ui.vertical_centered(|ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                let (rect, response) = ui.allocate_exact_size(Vec2::splat(diameter), egui::Sense::click_and_drag());
                let (center, radius) = (rect.center(), diameter * 0.5);
                let over_disc = response.hover_pos().is_some_and(|p| p.distance(center) <= radius);
                if over_disc {
                    let scroll = ui.input_mut(|i| {
                        let s = i.smooth_scroll_delta.y;
                        i.smooth_scroll_delta = Vec2::ZERO;
                        s
                    });
                    self.scroll += scroll;
                    let steps = (self.scroll / 40.0).trunc() as i32;
                    self.scroll -= steps as f32 * 40.0;
                    self.zoom = (self.zoom as i32 + steps).clamp(3, 17) as u8;
                } else {
                    self.scroll = 0.0;
                }
                let painter = ui.painter_at(rect);
                // Everything painted from here to the bezel is cut to the disc afterwards.
                let first_shape = ctx.graphics_mut(|g| g.entry(painter.layer_id()).next_idx());
                painter.circle_filled(center, radius, Color32::from_rgb(22, 32, 42));
                let angle = if self.heading_up { -self.heading } else { 0.0 };
                let n = (1u32 << self.zoom) as f64;
                let cx = geo::lon_to_merc_x(lon).rem_euclid(1.0) * n;
                let cy = geo::lat_to_merc_y(lat) * n;
                let supported = lat.abs() <= 85.0_f64.to_radians();
                let mut wanted = Vec::new();
                let mut drawn = 0;
                if supported && !self.plan {
                    for dy in -2..=2 {
                        for dx in -2..=2 {
                            let x = cx.floor() as i64 + dx;
                            let y = cy.floor() as i64 + dy;
                            if y < 0 || y >= n as i64 { continue; }
                            let id = TileId::new(self.zoom, x.rem_euclid(n as i64) as u32, y as u32);
                            let offset = Vec2::new(tile_offset(x as f64, cx, n) as f32, (y as f64 - cy) as f32) * 256.0;
                            let corners = [Vec2::ZERO, Vec2::new(256.0, 0.0), Vec2::splat(256.0), Vec2::new(0.0, 256.0)]
                                .map(|v| rect.center() + rotate(offset + v, angle));
                            if !egui::Rect::from_points(&corners).intersects(rect) { continue; }
                            // Coarse coverage first, then detail; reuse ancestors while zooming.
                            let coarse = id.ancestor(self.zoom.saturating_sub(2)).0;
                            wanted.push((0, coarse));
                            wanted.push((1, id));
                            for z in (0..=self.zoom).rev() {
                                let (a, u, v, scale) = id.ancestor(z);
                                if let Some(tile) = self.tiles.get_mut(&a) {
                                    tile.touched = self.frame;
                                    if let Some(texture) = &tile.texture {
                                        let mut mesh = egui::Mesh::with_texture(texture.id());
                                        let uv = [[u, v], [u + scale, v], [u + scale, v + scale], [u, v + scale]];
                                        for (pos, uv) in corners.into_iter().zip(uv) {
                                            mesh.vertices.push(egui::epaint::Vertex { pos, uv: Pos2::new(uv[0] as f32, uv[1] as f32), color: Color32::WHITE });
                                        }
                                        mesh.indices.extend([0, 1, 2, 0, 2, 3]);
                                        painter.add(mesh);
                                        drawn += 1;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
                wanted.sort_by_key(|(priority, id)| (*priority, (id.x as i64 - (cx / 2f64.powi((self.zoom - id.z) as i32)) as i64).abs()));
                if !self.plan && self.pending.is_none() {
                    if let Some((_, id)) = wanted.into_iter().find(|(_, id)| self.tiles.get(id).is_none_or(|t| t.texture.is_none() && Instant::now() >= t.retry)) {
                        let (tx, rx) = mpsc::channel();
                        let src = Arc::clone(src);
                        self.pending = Some((id, self.generation, rx));
                        std::thread::spawn(move || {
                            let img = src.imagery(id).ok().map(|img| image::imageops::resize(&img, 256, 256, image::imageops::FilterType::Triangle));
                            let _ = tx.send(img);
                        });
                    }
                }
                if supported {
                    let map_drawn = self.draw_map(ctx, src, &painter, rect, cx, cy, n, angle);
                    if self.plan { drawn = map_drawn; }
                    self.draw_runways(world, &painter, rect, lat, lon, cx, cy, n, angle);
                    // The combat zone: a red circle on the map the fight must stay in.
                    if let Some((zlat, zlon, zr)) = zone {
                        let at = map_point(zlat, zlon, cx, cy, n, angle, rect.center());
                        let mpp = 2.0 * std::f64::consts::PI * geo::R * zlat.cos() / (n * 256.0);
                        let rp = (zr / mpp) as f32;
                        if rp >= 2.0 {
                            painter.circle_filled(at, rp, Color32::from_rgba_unmultiplied(255, 60, 45, 18));
                            painter.circle_stroke(at, rp, Stroke::new(1.8, Color32::from_rgb(255, 80, 60)));
                        }
                    }
                    // The bandits in the fight: a red blip each. One beyond the
                    // map's edge is pinned to the rim in its direction, so a
                    // bandit the map cannot reach still shows where it is.
                    for &(blat, blon) in contacts {
                        let at = map_point(blat, blon, cx, cy, n, angle, rect.center());
                        let dot = blip_point(at, rect.center(), radius - 7.0);
                        painter.circle_filled(dot, 5.0, Color32::from_rgb(255, 45, 35));
                        painter.circle_stroke(dot, 5.0, Stroke::new(1.5, Color32::BLACK));
                    }
                    if let Some(guide) = guide { guide.draw_map(&painter, rect, cx, cy, n, angle); }
                }
                if !supported || drawn == 0 {
                    painter.text(rect.center() + Vec2::new(0.0, 32.0), egui::Align2::CENTER_CENTER,
                        if !supported { crate::t!("Map unavailable near poles", "Carte indisponible aux pôles") }
                        else if if self.plan { self.map_pending.is_some() } else { self.pending.is_some() } { crate::t!("Loading map…", "Chargement…") }
                        else { crate::t!("Imagery unavailable", "Images indisponibles") }, egui::FontId::proportional(12.0), Color32::WHITE);
                }
                let arrow = [Vec2::new(0.0, -13.0), Vec2::new(9.0, 10.0), Vec2::new(-9.0, 10.0)];
                let points: Vec<_> = arrow.into_iter().map(|v| rect.center() + rotate(v, self.heading + angle)).collect();
                painter.add(egui::Shape::convex_polygon(points, Color32::from_rgb(255, 210, 70), Stroke::new(2.0, Color32::BLACK)));
                let meters = 2.0 * std::f64::consts::PI * geo::R * lat.cos() / (n * 256.0) * 60.0;
                let a = Pos2::new(center.x - 30.0, rect.bottom() - 26.0);
                painter.line_segment([a, a + Vec2::new(60.0, 0.0)], Stroke::new(4.0, Color32::BLACK));
                painter.line_segment([a, a + Vec2::new(60.0, 0.0)], Stroke::new(2.0, Color32::WHITE));
                let label_rect = egui::Rect::from_center_size(a + Vec2::new(30.0, -13.0), Vec2::new(64.0, 18.0));
                painter.rect_filled(label_rect, 3.0, Color32::from_black_alpha(190));
                painter.text(label_rect.center(), egui::Align2::CENTER_CENTER, distance_label(meters, crate::i18n::lang()), egui::FontId::proportional(12.0), Color32::WHITE);
                clip_to_disc(ctx, painter.layer_id(), first_shape, center, radius);
                // Bezel, with the north mark riding on it.
                painter.circle_stroke(center, radius - 1.5, Stroke::new(3.0, Color32::from_black_alpha(200)));
                painter.circle_stroke(center, radius - 3.5, Stroke::new(1.0, Color32::from_white_alpha(70)));
                let north = center + rotate(Vec2::new(0.0, -(radius - 14.0)), angle);
                painter.circle_filled(north, 10.0, Color32::from_black_alpha(200));
                painter.text(north, egui::Align2::CENTER_CENTER, "N", egui::FontId::proportional(13.0), Color32::WHITE);
                // Resize handle on the rim, upper right; the lower-left stays anchored.
                let knob = center + Vec2::new(1.0, -1.0) * (radius * std::f32::consts::FRAC_1_SQRT_2);
                let resize = ui.interact(egui::Rect::from_center_size(knob, Vec2::splat(18.0)), egui::Id::unique("minimap_resize"), egui::Sense::drag())
                    .on_hover_cursor(egui::CursorIcon::ResizeNeSw)
                    .on_hover_text(crate::t!("Drag to resize the map", "Glisser pour redimensionner la carte"));
                if resize.drag_started() { self.resize_start = diameter; }
                if resize.dragged() {
                    let d = resize.total_drag_delta().unwrap_or_default();
                    let size = (self.resize_start + (d.x - d.y) * 0.5).clamp(240.0f32.min(limit), limit);
                    self.size = Vec2::splat(size);
                }
                let hot = resize.hovered() || resize.dragged();
                painter.circle_filled(knob, 7.0, Color32::from_black_alpha(220));
                painter.circle_stroke(knob, 7.0, Stroke::new(1.5, if hot { Color32::from_rgb(255, 210, 70) } else { Color32::WHITE }));
                // Controls sit under the disc.
                egui::Frame::new().fill(Color32::from_black_alpha(210)).corner_radius(14.0).inner_margin(3.0).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = 3.0;
                    ui.spacing_mut().button_padding = Vec2::new(5.0, 4.0);
                    ui.style_mut().override_font_id = Some(egui::FontId::proportional(12.0));
                    ui.horizontal(|ui| {
                        let button = egui::Button::selectable(self.heading_up, if self.heading_up { crate::t!("Heading", "Cap") } else { crate::t!("North", "Nord") });
                        if ui.add_sized([48.0, 24.0], button).on_hover_text(crate::t!("Toggle north up / heading up", "Basculer nord en haut / cap en haut")).clicked() { self.heading_up = !self.heading_up; }
                        let button = egui::Button::selectable(self.plan, if self.plan { crate::t!("Map", "Plan") } else { "Satellite" });
                        if ui.add_sized([62.0, 24.0], button).on_hover_text(crate::t!("Toggle satellite / map", "Basculer satellite / plan")).clicked() { self.plan = !self.plan; }
                        if ui.add_enabled(self.zoom > 3, egui::Button::new("-").min_size(Vec2::splat(24.0))).on_hover_text(crate::t!("Zoom out", "D\u{00e9}zoomer")).clicked() { self.zoom -= 1; }
                        if ui.add_enabled(self.zoom < 17, egui::Button::new("+").min_size(Vec2::splat(24.0))).on_hover_text(crate::t!("Zoom in", "Zoomer")).clicked() { self.zoom += 1; }
                        if ui.add_sized([24.0, 24.0], egui::Button::new("x")).on_hover_text(crate::t!("Hide map (`)", "Masquer la carte (`)")).clicked() { self.visible = false; }
                    });
                });
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scale_uses_localized_decimal_separator() {
        assert_eq!(distance_label(1500.0, crate::i18n::Lang::Fr), "1,5 km");
        assert_eq!(distance_label(1500.0, crate::i18n::Lang::En), "1.5 km");
        assert_eq!(distance_label(250.0, crate::i18n::Lang::Fr), "250 m");
    }
    #[test]
    fn heading_up_keeps_aircraft_up_and_rotates_east_up() {
        let heading = std::f64::consts::FRAC_PI_2;
        assert!((rotate(Vec2::new(10.0, 0.0), -heading) - Vec2::new(0.0, -10.0)).length() < 1e-5);
        assert!((rotate(Vec2::new(0.0, -10.0), heading - heading) - Vec2::new(0.0, -10.0)).length() < 1e-5);
    }
    #[test]
    fn antimeridian_uses_nearest_copy() {
        assert!((tile_offset(0.0, 255.75, 256.0) - 0.25).abs() < 1e-8);
        assert!((tile_offset(255.0, 0.25, 256.0) + 1.25).abs() < 1e-8);
    }
    #[test]
    fn a_contact_beyond_the_map_is_pinned_to_the_rim() {
        let center = Pos2::new(100.0, 100.0);
        let near = Pos2::new(150.0, 100.0);
        assert_eq!(blip_point(near, center, 60.0), near, "an on-map contact moved");
        let far = blip_point(Pos2::new(400.0, 100.0), center, 60.0);
        assert!((far.distance(center) - 60.0).abs() < 1e-4, "the contact was not pinned to the rim");
        assert!((far.y - 100.0).abs() < 1e-4, "the rim contact left its bearing");
        assert_eq!(blip_point(center, center, 60.0), center);
    }
    #[test]
    fn a_contact_over_the_aircraft_sits_at_the_centre() {
        let (lat, lon) = (50.0_f64.to_radians(), 4.0_f64.to_radians());
        let n = 4096.0;
        let (cx, cy) = (geo::lon_to_merc_x(lon).rem_euclid(1.0) * n, geo::lat_to_merc_y(lat) * n);
        let center = Pos2::new(120.0, 140.0);
        // The player's own position is the centre however the map is turned.
        for angle in [0.0, -1.0, 2.5] {
            let p = map_point(lat, lon, cx, cy, n, angle, center);
            assert!(p.distance(center) < 1e-3, "the contact sat {:.2} px off centre", p.distance(center));
        }
    }
}
