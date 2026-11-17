//! The start screen's aircraft window, and the hangar it opens: every
//! aircraft you can fly as a gallery of cards, and the JSBSim collection to
//! download more from (see hangar.rs), each with the license its files state.
use crate::aircraft::{self, Spec};
use crate::hangar::{self, Install, LicenseFrom, State, Status};
use crate::i18n::{t, tr};
use crate::menu::{self, ACCENT, DIM, TEXT};
use egui::{Align2, Color32, RichText, Stroke};

/// Which page of the hangar is showing.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Tab {
    Mine,
    Download,
}

const OK: Color32 = Color32::from_rgb(120, 214, 140);
const BAD: Color32 = Color32::from_rgb(255, 128, 96);

impl crate::Game {
    /// An aircraft's picture (shared by types drawn with the same body).
    pub(crate) fn still(&mut self, ctx: &egui::Context, spec: &Spec) -> egui::TextureHandle {
        let key = spec.still.as_ptr() as usize;
        self.stills
            .entry(key)
            .or_insert_with(|| {
                let img = image::load_from_memory(spec.still).expect("aircraft still").to_rgba8();
                let size = [img.width() as usize, img.height() as usize];
                ctx.load_texture(format!("aircraft {key}"), egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()), egui::TextureOptions::LINEAR)
            })
            .clone()
    }

    /// Top left of the start screen: the chosen aircraft, arrows to the
    /// others in the title bar, its picture (click: the hangar) and a line on
    /// it. Returns its rectangle.
    pub(crate) fn aircraft_window(&mut self, ctx: &egui::Context, w: f32, m: f32) -> egui::Rect {
        let list = aircraft::list();
        let n = list.len();
        let mut choose: Option<usize> = None;
        let mut open: Option<Tab> = None;
        let k = self.menu.aircraft.min(n - 1);
        let spec = list[k];
        let tex = self.still(ctx, spec);
        let air = egui::Area::new(egui::Id::unique("menu_air")).anchor(Align2::LEFT_TOP, [m, m]).show(ctx, |ui| {
            ui.set_width(w);
            ui.spacing_mut().item_spacing.y = 0.0;
            let top = egui::CornerRadius { nw: 12, ne: 12, sw: 0, se: 0 };
            let bottom = egui::CornerRadius { nw: 0, ne: 0, sw: 12, se: 12 };
            egui::Frame::new().fill(menu::PANEL).corner_radius(top).inner_margin(egui::Margin { left: 10, right: 10, top: 12, bottom: 10 }).show(ui, |ui| {
                ui.set_width(w - 20.0);
                ui.horizontal(|ui| {
                    let arrow = |ui: &mut egui::Ui, turn: f32| {
                        let r = ui.add(egui::Button::new("").min_size(egui::vec2(34.0, 34.0)).fill(Color32::from_white_alpha(8)).corner_radius(8.0));
                        menu::chevron(ui.painter(), r.rect.center(), 6.0, turn, if r.hovered() || r.has_focus() { Color32::WHITE } else { DIM });
                        r
                    };
                    if arrow(ui, 2.0).on_hover_text(t!("Previous aircraft", "Avion précédent")).clicked() {
                        choose = Some((k + n - 1) % n);
                    }
                    let name_w = ui.available_width() - 34.0 - ui.spacing().item_spacing.x;
                    ui.allocate_ui_with_layout(egui::vec2(name_w, 34.0), egui::Layout::top_down(egui::Align::Center), |ui| {
                        ui.label(RichText::new(t!("AIRCRAFT  {} / {}", "AVION  {} / {}", k + 1, n)).size(10.5).strong().color(DIM).extra_letter_spacing(1.5));
                        ui.label(RichText::new(spec.name).size(18.0).strong().color(Color32::WHITE));
                    });
                    if arrow(ui, 0.0).on_hover_text(t!("Next aircraft", "Avion suivant")).clicked() {
                        choose = Some((k + 1) % n);
                    }
                });
            });
            // The picture: click it for the hangar.
            let (pic, resp) = ui.allocate_exact_size(egui::vec2(w, 196.0), egui::Sense::click());
            ui.painter().rect_filled(pic, 0.0, menu::PANEL);
            egui::Image::new(&tex).paint_at(ui, pic);
            if spec.origin.is_some() {
                badge(ui.painter(), pic.left_top() + egui::vec2(12.0, 8.0), if spec.ext.is_some() { "FlightGear" } else { "JSBSim" });
            }
            if resp.hovered() {
                ui.painter().rect_filled(pic, 0.0, Color32::from_black_alpha(90));
                let p = ui.painter();
                p.text(pic.center(), Align2::CENTER_CENTER, t!("See all aircraft", "Voir tous les avions"), egui::FontId::proportional(16.0), Color32::WHITE);
            }
            if resp.clicked() {
                open = Some(Tab::Mine);
            }
            egui::Frame::new().fill(menu::PANEL).corner_radius(bottom).inner_margin(egui::Margin { left: 16, right: 16, top: 10, bottom: 12 }).show(ui, |ui| {
                ui.set_width(w - 32.0);
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.label(RichText::new(tr(spec.blurb)).size(12.0).color(DIM));
                if let Some(o) = spec.origin {
                    let by = if o.author.is_empty() { String::new() } else { t!("  ·  by {}", "  ·  par {}", o.author) };
                    ui.label(RichText::new(t!("License: {}{}", "Licence : {}{}", o.license, by)).size(11.0).color(DIM.gamma_multiply(0.8)));
                }
                ui.add_space(2.0);
                ui.columns(2, |c| {
                    if menu::wide_button(&mut c[0], t!("All aircraft", "Tous les avions"), crate::padui::Hint::None, false, true).clicked() {
                        open = Some(Tab::Mine);
                    }
                    if menu::wide_button(&mut c[1], t!("Get more…", "En obtenir…"), crate::padui::Hint::None, false, true).clicked() {
                        open = Some(Tab::Download);
                    }
                });
            });
        });
        let r = air.response.rect;
        ctx.layer_painter(air.response.layer_id).rect_stroke(r, 12.0, Stroke::new(1.0, Color32::from_white_alpha(22)), egui::StrokeKind::Inside);
        if let Some(k) = choose {
            self.menu.aircraft = k;
        }
        if let Some(t) = open {
            self.open_hangar(t);
        }
        r
    }

    pub(crate) fn open_hangar(&mut self, tab: Tab) {
        self.menu.hangar = Some(tab);
        if tab == Tab::Download {
            hangar::open();
        }
    }

    /// The hangar, over the whole start screen while it is open.
    pub(crate) fn hangar_window(&mut self, ctx: &egui::Context) {
        let Some(tab) = self.menu.hangar else { return };
        let screen = ctx.content_rect();
        // A dim backdrop; a click on it closes the hangar.
        let back = egui::Area::new(egui::Id::unique("hangar_back")).order(egui::Order::Middle).fixed_pos(screen.min).show(ctx, |ui| {
            let (r, resp) = ui.allocate_exact_size(screen.size(), egui::Sense::click());
            ui.painter().rect_filled(r, 0.0, Color32::from_black_alpha(150));
            resp
        });
        if back.inner.clicked() {
            self.menu.hangar = None;
            return;
        }
        let w = (screen.width() - 48.0).min(1080.0);
        let h = (screen.height() - 64.0).min(820.0);
        let mut close = false;
        let mut next_tab = tab;
        egui::Area::new(egui::Id::unique("hangar")).order(egui::Order::Foreground).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            menu::panel().inner_margin(egui::Margin::symmetric(22, 18)).show(ui, |ui| {
                ui.set_width(w - 44.0);
                ui.set_height(h - 36.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(t!("Aircraft", "Avions")).size(22.0).strong().color(Color32::WHITE));
                    ui.add_space(18.0);
                    let n = aircraft::list().len();
                    for (tb, label) in [(Tab::Mine, t!("Your aircraft ({})", "Vos avions ({})", n)), (Tab::Download, t!("Download more", "En télécharger").to_string())] {
                        let on = tab == tb;
                        let b = egui::Button::new(RichText::new(label).size(14.0).color(if on { menu::ON_ACCENT } else { TEXT })).fill(if on { ACCENT } else { Color32::from_white_alpha(10) }).corner_radius(16.0).min_size(egui::vec2(0.0, 30.0));
                        if ui.add(b).clicked() {
                            next_tab = tb;
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(egui::Button::new(RichText::new(t!("Close", "Fermer")).size(13.0)).min_size(egui::vec2(64.0, 28.0))).clicked() {
                            close = true;
                        }
                    });
                });
                ui.add_space(4.0);
                let r = ui.available_rect_before_wrap();
                ui.painter().hline(r.x_range(), r.top(), Stroke::new(1.0, Color32::from_white_alpha(18)));
                ui.add_space(12.0);
                match tab {
                    Tab::Mine => {
                        if let Some(c) = self.gallery(ui, ctx) {
                            match c {
                                Some(k) => {
                                    self.menu.aircraft = k;
                                    close = true;
                                }
                                None => next_tab = Tab::Download,
                            }
                        }
                    }
                    Tab::Download => {
                        if let Some(k) = self.downloads(ui) {
                            self.menu.aircraft = k;
                            close = true;
                        }
                    }
                }
                crate::padui::follow_focus(ui);
            });
        });
        if close {
            self.menu.hangar = None;
        } else if next_tab != tab {
            self.open_hangar(next_tab);
        }
    }

    /// Every aircraft as a card. Some(Some(k)): fly aircraft k; Some(None): get more.
    fn gallery(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) -> Option<Option<usize>> {
        let list = aircraft::list();
        let mut out = None;
        let gap = 14.0;
        let avail = ui.available_width();
        let cols = ((avail + gap) / (250.0 + gap)).floor().max(1.0) as usize;
        let cw = (avail - gap * (cols as f32 - 1.0)) / cols as f32;
        let ph = cw * 196.0 / 340.0;
        let ch = ph + 62.0;
        let texs: Vec<_> = list.iter().map(|s| self.still(ctx, s)).collect();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let n = list.len() + 1;
            for row in 0..n.div_ceil(cols) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for c in 0..cols {
                        let k = row * cols + c;
                        if k > list.len() {
                            break;
                        }
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(cw, ch), egui::Sense::click());
                        let lit = resp.hovered() || resp.has_focus();
                        let p = ui.painter();
                        if k == list.len() {
                            // The last card: get more.
                            p.rect_filled(rect, 10.0, Color32::from_white_alpha(if lit { 14 } else { 5 }));
                            p.rect_stroke(rect.shrink(1.0), 10.0, Stroke::new(1.5, if lit { ACCENT } else { Color32::from_white_alpha(40) }), egui::StrokeKind::Inside);
                            // a plus sign
                            let (c, r, col) = (rect.center() - egui::vec2(0.0, 24.0), 14.0, if lit { ACCENT } else { DIM });
                            p.line_segment([c - egui::vec2(r, 0.0), c + egui::vec2(r, 0.0)], Stroke::new(3.0, col));
                            p.line_segment([c - egui::vec2(0.0, r), c + egui::vec2(0.0, r)], Stroke::new(3.0, col));
                            p.text(rect.center() + egui::vec2(0.0, 14.0), Align2::CENTER_CENTER, t!("Download more aircraft", "Télécharger d'autres avions"), egui::FontId::proportional(15.0), if lit { Color32::WHITE } else { TEXT });
                            p.text(rect.center() + egui::vec2(0.0, 36.0), Align2::CENTER_CENTER, t!("from FlightGear's collection", "de la collection de FlightGear"), egui::FontId::proportional(12.0), DIM);
                            if resp.clicked() {
                                out = Some(None);
                            }
                            continue;
                        }
                        let spec = list[k];
                        let chosen = k == self.menu.aircraft;
                        p.rect_filled(rect, 10.0, Color32::from_white_alpha(if lit { 16 } else { 7 }));
                        let pic = egui::Rect::from_min_size(rect.min, egui::vec2(cw, ph));
                        egui::Image::new(&texs[k]).corner_radius(egui::CornerRadius { nw: 10, ne: 10, sw: 0, se: 0 }).paint_at(ui, pic);
                        let p = ui.painter();
                        if spec.origin.is_some() {
                            badge(p, pic.left_top() + egui::vec2(10.0, 8.0), if spec.ext.is_some() { "FlightGear" } else { "JSBSim" });
                        }
                        let tx = rect.left() + 12.0;
                        p.text(egui::pos2(tx, pic.bottom() + 18.0), Align2::LEFT_CENTER, spec.name, egui::FontId::proportional(16.0), if chosen { ACCENT } else { Color32::WHITE });
                        let sub = match spec.origin {
                            Some(o) if spec.ext.is_some() => t!("Downloaded  ·  {}", "Téléchargé  ·  {}", o.license),
                            Some(o) => t!("Drawn as the {}  ·  {}", "Dessiné comme le {}  ·  {}", o.body, o.license),
                            None => t!("Built in", "Inclus").to_string(),
                        };
                        let sub_g = p.layout(sub, egui::FontId::proportional(11.5), DIM, cw - 24.0);
                        p.galley(egui::pos2(tx, pic.bottom() + 32.0), sub_g, DIM);
                        if chosen {
                            p.rect_stroke(rect, 10.0, Stroke::new(2.0, ACCENT), egui::StrokeKind::Inside);
                            p.text(pic.right_top() + egui::vec2(-10.0, 12.0), Align2::RIGHT_CENTER, t!("SELECTED", "CHOISI"), egui::FontId::proportional(10.5), ACCENT);
                        } else if lit {
                            p.rect_stroke(rect, 10.0, Stroke::new(1.0, ACCENT.gamma_multiply(0.6)), egui::StrokeKind::Inside);
                        }
                        let resp = resp.on_hover_text(tr(spec.blurb));
                        if resp.clicked() {
                            out = Some(Some(k));
                        }
                    }
                });
                ui.add_space(gap);
            }
        });
        out
    }

    /// The JSBSim collection: a list on the left, the chosen one's details,
    /// license and download button on the right. Some(k): fly aircraft k.
    fn downloads(&mut self, ui: &mut egui::Ui) -> Option<usize> {
        let mut fly = None;
        let link = |ui: &mut egui::Ui, text: &str, url: &str| {
            ui.hyperlink_to(RichText::new(text).size(12.0), url);
        };
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.label(RichText::new(t!("Aircraft from", "Avions de")).size(12.0).color(DIM));
            link(ui, t!("FlightGear's collection (FGAddon)", "la collection de FlightGear (FGAddon)"), &hangar::repo_url());
            ui.label(RichText::new(t!("— each with its own 3D model. Its flight model (JSBSim or YASim) is fitted to ours. Each keeps the license its files state. They are made by FlightGear's community: some are not of high quality, and some don't fly realistically here.", "— chacun avec son propre modèle 3D. Son modèle de vol (JSBSim ou YASim) est adapté au nôtre. Chacun garde la licence indiquée dans ses fichiers. Ils sont faits par la communauté de FlightGear : certains ne sont pas de grande qualité, et certains ne volent pas de façon réaliste ici.")).size(12.0).color(DIM));
        });
        ui.add_space(8.0);
        let (status, entries) = {
            let c = hangar::catalogue();
            (c.status.clone(), c.entries.clone())
        };
        match status {
            Status::Idle | Status::Listing => {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new());
                    ui.label(RichText::new(t!("Reading FlightGear's aircraft catalogue…", "Lecture du catalogue d'avions de FlightGear…")).color(TEXT));
                });
                ui.ctx().request_repaint();
                return None;
            }
            Status::Failed(e) => {
                ui.label(RichText::new(t!("Could not get the catalogue: {}", "Impossible d'obtenir le catalogue : {}", e)).color(BAD));
                if ui.button(t!("Try again", "Réessayer")).clicked() {
                    hangar::catalogue().status = Status::Idle;
                    hangar::open();
                }
                return None;
            }
            Status::Ready => {}
        }
        if hangar::reading() > 0 || entries.iter().any(|e| matches!(e.install, Install::Downloading(..) | Install::Unpacking)) {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
        let flyable = |e: &hangar::Entry| matches!(&e.state, State::Ready(i) if i.fit.is_ok());
        if self.menu.hangar_sel.as_ref().is_none_or(|s| !entries.iter().any(|e| &e.folder == s)) {
            self.menu.hangar_sel = entries.iter().find(|e| flyable(e)).or(entries.iter().find(|e| hangar::named_fdm(e) == Some(true))).or(entries.first()).map(|e| e.folder.clone());
        }
        let left_w = 300.0;
        let full = ui.available_rect_before_wrap();
        let h = full.height();
        let left = egui::Rect::from_min_size(full.min, egui::vec2(left_w, h));
        let right = egui::Rect::from_min_max(egui::pos2(left.right() + 14.0, full.top()), full.max);
        let column = |r: egui::Rect| egui::UiBuilder::new().max_rect(r).layout(egui::Layout::top_down(egui::Align::Min));
        ui.allocate_rect(full, egui::Sense::hover());
        {
            // The list.
            ui.scope_builder(column(left), |ui| {
                ui.set_width(left_w);
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.menu.hangar_find).hint_text(t!("Search", "Rechercher")).desired_width(left_w - 120.0).margin(egui::vec2(8.0, 5.0)));
                    ui.checkbox(&mut self.menu.hangar_all, RichText::new(t!("Show all", "Tout voir")).size(12.0)).on_hover_text(t!("Also list the ones that can't be flown here (helicopters, rockets, gliders…)", "Lister aussi ceux qui ne peuvent pas voler ici (hélicoptères, fusées, planeurs…)"));
                });
                let line = t!("{} aircraft · pick one to see if it flies here", "{} avions · choisissez-en un pour voir s'il vole ici", entries.len());
                ui.label(RichText::new(line).size(11.5).color(DIM));
                ui.add_space(4.0);
                let find = self.menu.hangar_find.to_lowercase();
                egui::ScrollArea::vertical().id_salt("hangar_list").auto_shrink([false, false]).max_height(ui.available_height()).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    for e in &entries {
                        let name = match &e.state {
                            State::Ready(i) => i.name.clone(),
                            _ if !e.name.is_empty() => e.name.clone(),
                            _ => e.folder.clone(),
                        };
                        let can = flyable(e);
                        let failed = matches!(e.state, State::Failed(_)) || matches!(&e.state, State::Ready(i) if i.fit.is_err());
                        if failed && !self.menu.hangar_all && e.install != Install::Installed {
                            continue;
                        }
                        if !find.is_empty() && !name.to_lowercase().contains(&find) && !e.folder.to_lowercase().contains(&find) {
                            continue;
                        }
                        let sel = self.menu.hangar_sel.as_deref() == Some(e.folder.as_str());
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 40.0), egui::Sense::click());
                        let lit = resp.hovered() || resp.has_focus();
                        let p = ui.painter();
                        p.rect_filled(rect, 7.0, if sel { ACCENT.gamma_multiply(0.18) } else { Color32::from_white_alpha(if lit { 14 } else { 5 }) });
                        if sel {
                            p.rect_stroke(rect, 7.0, Stroke::new(1.0, ACCENT.gamma_multiply(0.7)), egui::StrokeKind::Inside);
                        }
                        let name_col = if can || matches!(e.state, State::Pending) { Color32::WHITE } else { DIM };
                        p.text(rect.left_center() + egui::vec2(10.0, -7.0), Align2::LEFT_CENTER, &name, egui::FontId::proportional(14.0), name_col);
                        let sub = match &e.state {
                            State::Ready(i) => match &i.fit {
                                Ok(s) => format!("{} · {}", s.kind, i.license),
                                Err(_) => format!("{} · {}", t!("can't be flown here", "ne peut pas voler ici"), i.license),
                            },
                            State::Pending => match hangar::named_fdm(e) {
                                Some(true) => format!("JSBSim · {}", crate::bytes(e.bytes())),
                                Some(false) => format!("YASim · {}", crate::bytes(e.bytes())),
                                None => crate::bytes(e.bytes()),
                            },
                            State::Failed(_) => t!("couldn't be read", "illisible").to_string(),
                        };
                        let sub: String = sub.chars().take(44).collect();
                        p.text(rect.left_center() + egui::vec2(10.0, 9.0), Align2::LEFT_CENTER, sub, egui::FontId::proportional(11.0), DIM);
                        let (tag, col) = match &e.install {
                            Install::Installed => (t!("installed", "installé").to_string(), OK),
                            Install::Downloading(a, b) => (format!("{:.0}%", *a as f64 * 100.0 / (*b).max(1) as f64), ACCENT),
                            Install::Unpacking => (t!("unpacking", "décompression").to_string(), ACCENT),
                            Install::Failed(_) => ("!".to_string(), BAD),
                            Install::No => (String::new(), DIM),
                        };
                        p.text(rect.right_top() + egui::vec2(-10.0, 13.0), Align2::RIGHT_CENTER, tag, egui::FontId::proportional(11.0), col);
                        if resp.clicked() {
                            self.menu.hangar_sel = Some(e.folder.clone());
                        }
                    }
                });
            });
            // The details.
            if let Some(e) = self.menu.hangar_sel.as_ref().and_then(|s| entries.iter().find(|e| &e.folder == s)) {
                ui.scope_builder(column(right), |ui| {
                    egui::Frame::new().fill(Color32::from_white_alpha(6)).corner_radius(10.0).inner_margin(18.0).show(ui, |ui| {
                        ui.set_width(right.width() - 36.0);
                        ui.set_height(h - 36.0);
                        egui::ScrollArea::vertical().id_salt("hangar_detail").auto_shrink([false, false]).show(ui, |ui| {
                            ui.set_width(right.width() - 36.0);
                            fly = self.detail(ui, e);
                        });
                    });
                });
            }
        }
        fly
    }

    /// One entry of the collection in full. Some(k): fly aircraft k.
    fn detail(&mut self, ui: &mut egui::Ui, e: &hangar::Entry) -> Option<usize> {
        let mut fly = None;
        hangar::want(&e.folder);
        let info = match &e.state {
            State::Ready(i) => Some(i),
            State::Pending => {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new());
                    ui.label(RichText::new(t!("Reading {}…", "Lecture de {}…", if e.name.is_empty() { &e.folder } else { &e.name })).color(TEXT));
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
                });
                None
            }
            State::Failed(err) => {
                ui.label(RichText::new(&e.folder).size(22.0).strong().color(Color32::WHITE));
                ui.label(RichText::new(t!("Couldn't read it: {}", "Lecture impossible : {}", err)).color(BAD));
                None
            }
        };
        let Some(i) = info else { return None };
        ui.label(RichText::new(&i.name).size(24.0).strong().color(Color32::WHITE));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.label(RichText::new(format!("{}  ·  {}", e.folder, crate::bytes(e.bytes()))).size(12.0).color(DIM));
            ui.hyperlink_to(RichText::new(t!("view its files", "voir ses fichiers")).size(12.0), hangar::folder_url(&e.folder));
        });
        if !i.author.is_empty() {
            ui.label(RichText::new(t!("By {}", "Par {}", i.author)).size(13.0).color(TEXT));
        }
        if !i.description.is_empty() {
            ui.label(RichText::new(&i.description).size(13.0).color(TEXT));
        }
        ui.add_space(10.0);

        // The license.
        menu::heading(ui, t!("LICENSE", "LICENCE"));
        egui::Frame::new().fill(Color32::from_white_alpha(7)).corner_radius(8.0).inner_margin(12.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            if i.license_url.is_empty() {
                ui.label(RichText::new(&i.license).size(15.0).strong().color(Color32::WHITE));
            } else {
                ui.hyperlink_to(RichText::new(&i.license).size(15.0).strong(), &i.license_url);
            }
            let from = match i.license_from {
                LicenseFrom::Header => t!("as stated in the aircraft file's header", "indiquée dans l'en-tête du fichier de l'avion").to_string(),
                LicenseFrom::Comment => t!("as stated in the aircraft file's comments", "indiquée dans les commentaires du fichier de l'avion").to_string(),
                LicenseFrom::Repository => t!("the file states no license of its own: it is covered by the license of FlightGear's collection", "le fichier n'indique pas de licence propre : la licence de la collection de FlightGear s'applique").to_string(),
            };
            ui.label(RichText::new(from).size(12.0).color(DIM));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(RichText::new(t!("FlightGear's collection:", "Collection de FlightGear :")).size(12.0).color(DIM));
                ui.hyperlink_to(RichText::new(hangar::REPO_LICENSE).size(12.0), hangar::REPO_LICENSE_URL);
            });
            if !i.notes.is_empty() {
                ui.add_space(4.0);
                ui.label(RichText::new(t!("Notes from its authors", "Notes de ses auteurs")).size(11.5).strong().color(DIM));
                let notes: String = i.notes.chars().take(900).collect();
                ui.label(RichText::new(notes).size(11.5).color(DIM));
            }
        });
        ui.add_space(10.0);

        // What it becomes here.
        menu::heading(ui, t!("IN FABLE FIGHT", "DANS FABLE FIGHT"));
        egui::Frame::new().fill(Color32::from_white_alpha(7)).corner_radius(8.0).inner_margin(12.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            match &i.fit {
                Ok(s) => {
                    egui::Grid::new("fit").num_columns(4).spacing([14.0, 4.0]).show(ui, |ui| {
                        let cell = |ui: &mut egui::Ui, k: &str, v: String| {
                            ui.label(RichText::new(k).size(12.0).color(DIM));
                            ui.label(RichText::new(v).size(13.0).color(Color32::WHITE));
                        };
                        cell(ui, t!("Type", "Type"), s.kind.clone());
                        cell(ui, t!("Engines", "Moteurs"), s.power.clone());
                        ui.end_row();
                        cell(ui, t!("Mass", "Masse"), format!("{:.0} kg", s.mass_kg));
                        cell(ui, t!("Span", "Envergure"), format!("{:.1} m", s.span_m));
                        ui.end_row();
                        cell(ui, t!("Stall", "Décrochage"), format!("{:.0} kt", s.stall_kt));
                        cell(ui, t!("Top speed", "Vitesse max"), t!("about {:.0} kt", "environ {:.0} kt", s.vmax_kt));
                        ui.end_row();
                    });
                    ui.add_space(4.0);
                    let how = if s.own_model {
                        t!("Flies with our flight model, fitted to its {1} lift, drag, mass and engines. Drawn with its own 3D model, on its own landing gear; instruments and sounds from the {0}.", "Vole avec notre modèle de vol, ajusté à sa portance, sa traînée, sa masse et ses moteurs {1}. Dessiné avec son propre modèle 3D, sur son propre train ; instruments et sons du {0}.", s.body, s.fdm)
                    } else {
                        t!("Flies with our flight model, fitted to its {1} lift, drag, mass and engines. Drawn as the {0} (body, cockpit, gear and paint).", "Vole avec notre modèle de vol, ajusté à sa portance, sa traînée, sa masse et ses moteurs {1}. Dessiné comme le {0} (fuselage, cockpit, train et peinture).", s.body, s.fdm)
                    };
                    ui.label(RichText::new(how).size(12.0).color(DIM));
                }
                Err(why) => {
                    ui.label(RichText::new(t!("Can't be flown here yet", "Ne peut pas encore voler ici")).size(14.0).strong().color(BAD));
                    ui.label(RichText::new(why).size(12.0).color(DIM));
                }
            }
        });
        ui.add_space(14.0);

        // Download, fly, remove.
        let key = hangar::key(&e.folder);
        let can = i.fit.is_ok();
        match &e.install {
            Install::No | Install::Failed(_) => {
                if let Install::Failed(err) = &e.install {
                    ui.label(RichText::new(t!("Download failed: {}", "Échec du téléchargement : {}", err)).size(12.0).color(BAD));
                }
                let label = t!("Download  ·  {}", "Télécharger  ·  {}", crate::bytes(e.bytes()));
                if menu::wide_button(ui, &label, crate::padui::Hint::None, true, can).clicked() {
                    hangar::install(&e.folder);
                }
                ui.label(RichText::new(t!("Its package comes straight from a FlightGear mirror to {} under {}.", "Son paquet vient directement d'un miroir FlightGear dans {} sous {}.", hangar::install_dir().join(&e.folder).display(), i.license)).size(11.0).color(DIM));
            }
            Install::Downloading(a, b) => {
                ui.add(egui::ProgressBar::new(*a as f32 / (*b).max(1) as f32).text(t!("Downloading… {} / {}", "Téléchargement… {} / {}", crate::bytes(*a), crate::bytes(*b))).desired_height(30.0));
            }
            Install::Unpacking => {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new());
                    ui.label(RichText::new(t!("Unpacking and reading its model…", "Décompression et lecture de son modèle…")).color(TEXT));
                });
            }
            Install::Installed => {
                let spec = aircraft::list().into_iter().position(|s| s.keys.first() == Some(&key.as_str()));
                match spec {
                    Some(k) => {
                        if menu::wide_button(ui, t!("Fly this aircraft", "Piloter cet avion"), crate::padui::Hint::None, true, true).clicked() {
                            fly = Some(k);
                        }
                    }
                    None => {
                        ui.label(RichText::new(t!("Downloaded, but it doesn't load here.", "Téléchargé, mais ne se charge pas ici.")).size(12.0).color(BAD));
                    }
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(t!("Downloaded", "Téléchargé")).size(12.5).color(OK));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(RichText::new(t!("Remove", "Supprimer")).size(12.5)).clicked() {
                            let chosen = aircraft::nth(self.menu.aircraft).keys.first().copied();
                            if let Err(err) = hangar::remove(&e.folder) {
                                log::warn!("removing {}: {err}", e.folder);
                            }
                            // Keep the same aircraft chosen if it is still there.
                            self.menu.aircraft = chosen.and_then(aircraft::by_key).map(aircraft::index_of).unwrap_or(0);
                        }
                    });
                });
            }
        }
        fly
    }
}

/// A small label on a picture.
fn badge(p: &egui::Painter, at: egui::Pos2, text: &str) {
    let g = p.layout_no_wrap(text.to_string(), egui::FontId::proportional(10.5), menu::ON_ACCENT);
    let r = egui::Rect::from_min_size(at, g.size() + egui::vec2(12.0, 4.0));
    p.rect_filled(r, 4.0, ACCENT);
    p.galley(r.min + egui::vec2(6.0, 2.0), g, menu::ON_ACCENT);
}
