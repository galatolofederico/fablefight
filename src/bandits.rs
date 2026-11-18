//! The AI aircraft in the fight.
//!
//! A bandit is a real aircraft: it owns its own `Plane` (and so its own
//! damage) and an `ai::Pilot`, and it flies through the same `Plane::step`
//! the player does. It shares its `Spec` and its model with the player's
//! aircraft when it is the same type: the caller looks the model up in the
//! game's hangar cache and never builds one per bandit.
//!
//! A destroyed bandit stops flying and becomes a fireball and a smoke trail
//! for `WRECK_DWELL` seconds, then its slot is freed. Slots are never reused:
//! a new bandit always takes the next index, so a `TargetKey::Bandit` held by
//! a missile in flight can never alias a different aircraft.
use crate::ai::{Pilot, Skill, Threat};
use crate::aircraft::Spec;
use crate::arena::{zone_hull, Zone, ZoneHull};
use crate::airframe::{G_LWING, G_RWING};
use crate::crash::Rng;
use crate::geo;
use crate::plane::Plane;
use crate::weapons::{self, Flare, Missile, Target, TargetKey};
use crate::world::World;
use glam::DVec3;

/// How long a wrecked bandit burns and smokes before it is removed (s).
pub const WRECK_DWELL: f64 = 6.0;

/// How far away a player's missile is considered by the bandit's approach
/// warning (m). The AI then filters for missiles that are actually closing.
const MISSILE_WARNING_RANGE: f64 = 6_000.0;

/// One bandit's missile in flight, with the slot that fired it. The missile
/// outlives the aircraft: a shot already on the way keeps flying after its
/// launcher is destroyed.
pub struct BanditMissile {
    pub owner: usize,
    pub missile: Missile,
}

/// Something a bandit did this step: the caller turns it into a sound, a
/// flash, a smoke trail or damage to the player.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// A missile left a rail. `owner` is the bandit's slot.
    MissileLaunch { owner: usize, pos: DVec3 },
    /// A flare left a dispenser, for the hiss.
    Flare { owner: usize, pos: DVec3 },
    /// A missile detonated. `hit_player` is set when the proximity fuse fired
    /// on the player's aircraft, so the caller applies the warhead.
    Blast { owner: usize, pos: DVec3, power: f32, size: f32, hit_player: bool },
}

/// What brought a bandit down, for the score and the kill feed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KillBy {
    Missile,
    Gun,
    Collision,
    /// The aircraft stayed outside the combat zone and broke up there.
    Zone,
}

/// One AI aircraft.
pub struct Bandit {
    pub spec: &'static Spec,
    pub plane: Plane,
    pub ai: Pilot,
    pub alive: bool,
    /// The wreck has finished burning and is ready to remove.
    pub gone: bool,
    pub kill_credit: Option<KillBy>,
    /// Seconds since it was destroyed.
    pub wreck_t: f64,
    /// What the HUD calls it.
    pub label: String,
    /// Turning back to the combat zone: the fight must not draw it outside.
    pub returning: bool,
    /// Seconds spent outside the combat zone.
    pub zone_out: f64,
    /// The infrared stations this type can fire, as indices into
    /// `spec.missiles()`, and which of them are still loaded. The game models
    /// every missile as an infrared seeker, so a bandit uses its Sidewinder
    /// stations (wingtips or side bays); the radar missiles stay on the rails.
    pub missile_stations: Vec<usize>,
    loaded: Vec<bool>,
    /// Flares left and the next serial, so its flares never alias another
    /// aircraft's.
    pub flares_left: u32,
    next_flare: u64,
}

impl Bandit {
    /// Missiles still on the rails.
    pub fn missiles_left(&self) -> u32 {
        self.loaded.iter().filter(|l| **l).count() as u32
    }

    /// Take the next loaded missile station, marking it empty.
    fn take_station(&mut self) -> Option<usize> {
        let k = self.loaded.iter().position(|l| *l)?;
        self.loaded[k] = false;
        self.missile_stations.get(k).copied()
    }

    /// Is this model piece a missile this bandit has already fired?
    pub fn hides_piece(&self, name: &str) -> bool {
        self.missile_stations.iter().zip(self.loaded.iter()).any(|(&st, &loaded)| {
            !loaded && self.spec.missiles().get(st).is_some_and(|s| s.piece == name)
        })
    }

    /// Where a station's missile leaves the aircraft (body frame). Wing stores
    /// use the type's left or right wing hardpoint; a side-bay store leaves
    /// from its own side of the fuselage.
    fn station_point(spec: &Spec, station: usize) -> DVec3 {
        let Some(st) = spec.missiles().get(station) else { return DVec3::ZERO };
        match st.group {
            G_LWING => spec.hard[1],
            G_RWING => spec.hard[2],
            // A side-bay store (the Raptor) leaves from its own side of the
            // fuselage, not from the centreline.
            _ => DVec3::new(if st.side == 0 { -1.2 } else { 1.2 }, -0.6, 0.0),
        }
    }
}

/// The bandits in one flight.
pub struct Bandits {
    /// A removed bandit leaves a `None` here. `spawn` always appends, so an
    /// index is never reused (a missile cannot lock a new bandit by mistake).
    pub items: Vec<Option<Bandit>>,
    /// The arena whose boundary the pilots must respect, while one is running.
    pub zone: Option<Zone>,
    /// Where each bandit was at the start of the step (planet frame), for the
    /// swept collision test.
    prev_pos: Vec<DVec3>,
    /// Bandits destroyed by the player this flight.
    pub kills: u32,
    /// Fireballs for the caller to draw this frame: position and power.
    pub deaths: Vec<(DVec3, f32)>,
    /// Every bandit missile in the air, whoever fired it and whether or not
    /// that aircraft is still flying.
    pub missiles: Vec<BanditMissile>,
    /// Every bandit flare burning in the air. The player's missile seeker is
    /// shown these by `main.rs`, so a bandit's flares can decoy it.
    pub flares: Vec<Flare>,
    /// Launches, flares and detonations from this step.
    pub events: Vec<Event>,
    rng: Rng,
}

impl Default for Bandits {
    fn default() -> Self {
        Self::new()
    }
}

impl Bandits {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            zone: None,
            prev_pos: Vec::new(),
            kills: 0,
            deaths: Vec::new(),
            missiles: Vec::new(),
            flares: Vec::new(),
            events: Vec::new(),
            rng: Rng(0x51ED_2C3D_9A17_004B),
        }
    }

    /// A new flight: forget every bandit, including their indices.
    pub fn reset(&mut self) {
        self.items.clear();
        self.zone = None;
        self.prev_pos.clear();
        self.kills = 0;
        self.deaths.clear();
        self.missiles.clear();
        self.flares.clear();
        self.events.clear();
        self.rng = Rng(0x51ED_2C3D_9A17_004B);
    }

    /// Clear what is in the air without touching the aircraft: a respawn
    /// starts a fresh engagement, and a missile still looking for the old
    /// wreck must not follow the replacement.
    pub fn clear_ordnance(&mut self) {
        self.missiles.clear();
        self.flares.clear();
        self.events.clear();
    }

    /// Put one bandit in the air at `pos`, flying along `dir` at `speed`.
    /// Returns its index in `items`.
    pub fn spawn(&mut self, spec: &'static Spec, pos: DVec3, dir: DVec3, speed: f64) -> usize {
        let (lat, lon, alt) = geo::pos_to_lla(pos);
        let (e, n, _) = geo::enu(pos);
        let heading = e.dot(dir).atan2(n.dot(dir));
        let mut plane = Plane::spawn(spec, lat, lon, alt, heading, speed.max(60.0));
        plane.throttle = 0.85;
        let seed = (pos.x.to_bits() ^ pos.y.to_bits() ^ pos.z.to_bits() ^ self.items.len() as u64) | 1;
        let label = format!("BANDIT {}", self.items.len() + 1);
        // The infrared stations: the type's Sidewinder rails (wingtips on the
        // Hornet and Phantom, side bays on the Raptor). If a downloaded type
        // has no name we recognise, its first two stations are used.
        let all = spec.missiles();
        let mut missile_stations: Vec<usize> = all
            .iter()
            .enumerate()
            .filter(|(_, st)| {
                let piece = st.piece.to_ascii_lowercase();
                let name = st.name.to_ascii_lowercase();
                piece.contains("sidewinder") || name.contains("sidewinder")
            })
            .map(|(i, _)| i)
            .collect();
        if missile_stations.is_empty() {
            missile_stations = (0..all.len()).take(2).collect();
        }
        let loaded = vec![true; missile_stations.len()];
        self.items.push(Some(Bandit {
            spec,
            plane,
            ai: Pilot::new(Skill::average(), seed),
            alive: true,
            gone: false,
            kill_credit: None,
            wreck_t: 0.0,
            label,
            returning: false,
            zone_out: 0.0,
            missile_stations,
            loaded,
            flares_left: weapons::FLARES_MAX,
            next_flare: (self.items.len() as u64 + 1) << 8,
        }));
        self.prev_pos.push(pos);
        self.items.len() - 1
    }

    pub fn alive(&self) -> impl Iterator<Item = (usize, &Bandit)> + '_ {
        self.items.iter().enumerate().filter_map(|(i, b)| b.as_ref().filter(|b| b.alive).map(|b| (i, b)))
    }

    pub fn alive_count(&self) -> usize {
        self.alive().count()
    }

    pub fn get(&self, i: usize) -> Option<&Bandit> {
        self.items.get(i).and_then(Option::as_ref)
    }

    /// The bandits the player's seeker may lock: the living ones, with their
    /// positions from the last step.
    pub fn append_targets(&self, out: &mut Vec<Target>) {
        for (i, b) in self.alive() {
            out.push(Target {
                key: TargetKey::Bandit(i as u32),
                pos: b.plane.pos,
                vel: b.plane.vel,
                // A fighter as the traffic model's size classes see it: the
                // business-jet class is the closest span and heat signature.
                size: 1,
                label: b.label.clone(),
            });
        }
    }

    /// Fly every bandit for one fixed step, and every bandit missile and flare
    /// in the air. `player_missiles` and `player_flares` are the player's own
    /// ordnance: bandits see a player's missile on their approach warning and
    /// can decoy it with their flares, just as the player can decoy a bandit's.
    pub fn step(
        &mut self,
        h: f64,
        world: &World,
        player: &Plane,
        player_missiles: &[Missile],
        player_flares: &[Flare],
    ) {
        let Self { items, zone, prev_pos, deaths, missiles, flares, events, rng, .. } = self;
        if prev_pos.len() < items.len() {
            prev_pos.resize(items.len(), DVec3::ZERO);
        }
        // The player as the bandit missiles' seeker sees it: a fighter-sized
        // heat source with an exhaust plume.
        let targets = [Target {
            key: TargetKey::Player,
            pos: player.pos,
            vel: player.vel,
            size: 1,
            label: "YOU".into(),
        }];
        for (i, slot) in items.iter_mut().enumerate() {
            let Some(b) = slot else { continue };
            prev_pos[i] = b.plane.pos;
            if !b.alive {
                // No second flight model and no fluid volume for a wreck: it
                // falls ballistically while the caller draws the smoke.
                b.wreck_t += h;
                if b.wreck_t >= WRECK_DWELL {
                    b.gone = true;
                } else {
                    let up = b.plane.pos.normalize();
                    b.plane.vel -= up * 9.81 * h;
                    b.plane.pos += b.plane.vel * h;
                }
                continue;
            }
            // The bandits fly in the player's air mass: same mean wind and
            // turbulence model (each airframe makes its own gusts from it).
            b.plane.wind = player.wind;
            b.plane.air = player.air;
            // The approach warning: the nearest player missile actually
            // guiding on this aircraft. A missile that has been seduced by a
            // flare is no longer a threat.
            let mut threat: Option<Threat> = None;
            for m in player_missiles {
                if m.is_decoyed() || !matches!(m.target_key(), Some(TargetKey::Bandit(k)) if *k as usize == i) {
                    continue;
                }
                let range = (m.pos - b.plane.pos).length();
                if range > MISSILE_WARNING_RANGE {
                    continue;
                }
                if threat.is_none_or(|t| range < t.range) {
                    threat = Some(Threat { pos: m.pos, vel: m.vel, range });
                }
            }
            // A bandit's own missile keeps the player on its radar even when
            // the seeker has slid out of the forward cone.
            let tracking = missiles.iter().any(|bm| {
                bm.owner == i && bm.missile.target_key() == Some(&TargetKey::Player) && !bm.missile.is_decoyed()
            });
            let seen = b.ai.sense(&b.plane, player, tracking);
            let mut c = b.ai.controls(&b.plane, seen, threat, h);
            // The fight is kept inside its circle: a pilot that reaches the
            // 80% ring turns back for the centre, whatever it was doing, and
            // does not resume the chase until it is well inside again.
            if let Some(z) = *zone {
                let d = z.distance(b.plane.pos);
                if d >= z.radius * 0.80 {
                    b.returning = true;
                } else if d <= z.radius * 0.55 {
                    b.returning = false;
                }
                if b.returning {
                    c = crate::ai::return_to(&b.plane, z.centre);
                }
            } else {
                b.returning = false;
            }
            if let Some(imp) = b.plane.step(h, &c, world) {
                b.alive = false;
                b.wreck_t = 0.0;
                // Flying into the ground is no one's kill.
                deaths.push((imp.pos, 1.1));
            } else if b.plane.wrecked() {
                b.alive = false;
                b.wreck_t = 0.0;
                deaths.push((b.plane.pos, 1.0));
            }
            // The zone's own rule applies to the AI too, if a pilot is ever
            // forced outside: the clock is wiped on return, and the hull is
            // damaged after the grace until it breaks up.
            if b.alive {
                if let Some(z) = *zone {
                    if z.contains(b.plane.pos) {
                        b.zone_out = 0.0;
                    } else {
                        b.zone_out += h;
                        match zone_hull(b.plane.dmg.hull, b.zone_out, h) {
                            ZoneHull::Hold => {}
                            ZoneHull::Damaged(hull) => {
                                b.plane.dmg.hull = hull;
                                b.plane.dmg.vibe = b.plane.dmg.vibe.max(0.6);
                                b.plane.dmg.smoke = b.plane.dmg.smoke.max(0.25);
                            }
                            ZoneHull::Failed => {
                                b.plane.break_up("LEFT THE COMBAT ZONE");
                                b.alive = false;
                                b.wreck_t = 0.0;
                                deaths.push((b.plane.pos, 1.0));
                            }
                        }
                    }
                } else {
                    b.zone_out = 0.0;
                }
            }
            if b.alive {
                // A missile behind the wing is answered with the dispenser
                // first: it is the cheaper answer and it buys the turn time.
                if b.flares_left > 0 && b.ai.flare_ready(&b.plane, threat) {
                    let n = weapons::FLARE_SALVO.min(b.flares_left);
                    for _ in 0..n {
                        let side = if b.next_flare % 2 == 0 { -1.0 } else { 1.0 };
                        let f = weapons::eject_flare(&b.plane, b.next_flare, side);
                        events.push(Event::Flare { owner: i, pos: f.pos });
                        flares.push(f);
                        b.next_flare += 1;
                        b.flares_left -= 1;
                    }
                }
                // A good seeker solution, and no missile already inbound.
                if !b.returning && b.ai.missile_shot(&b.plane, seen, b.missiles_left()) {
                    if let Some(station) = b.take_station() {
                        let point = Bandit::station_point(b.spec, station);
                        let pos = b.plane.model_pos() + b.plane.rot * point;
                        let vel = b.plane.vel + b.plane.forward() * 12.0;
                        let shot = Missile::launch(station, pos, vel, b.plane.rot, Some(TargetKey::Player));
                        events.push(Event::MissileLaunch { owner: i, pos });
                        missiles.push(BanditMissile { owner: i, missile: shot });
                    }
                }
            }
            // The player's effect queues (HUD notes, shed pieces, sparks and
            // splashes) are drawn by `main.rs` for the player only; do not let
            // a bandit's fill up over a long flight.
            b.plane.notes.clear();
            b.plane.shed.clear();
            b.plane.tree_hits.clear();
            b.plane.sparks.clear();
            b.plane.splashes.clear();
            b.plane.dirt.clear();
        }

        weapons::drift_flares(flares, h);
        // Both sides' flares are in the sky. A bandit's flare near the player
        // can fool the player's missile; a player's flare near the player can
        // fool a bandit's.
        let mut all_flares: Vec<Flare> = flares.iter().cloned().collect();
        all_flares.extend(player_flares.iter().cloned());
        let mut dice = Rng(rng.0);
        for bm in missiles.iter_mut() {
            weapons::roll_decoy(&mut bm.missile, &all_flares, &targets, &mut dice.0);
        }
        rng.0 = dice.0;
        let mut blasts = Vec::new();
        for bm in missiles.iter_mut() {
            blasts.clear();
            bm.missile.step(h, &targets, &all_flares, world, &mut blasts);
            for ev in blasts.drain(..) {
                if let weapons::Event::Blast { pos, power, size, target } = ev {
                    events.push(Event::Blast {
                        owner: bm.owner,
                        pos,
                        power,
                        size,
                        hit_player: target == Some(TargetKey::Player),
                    });
                }
            }
        }
        missiles.retain(|bm| bm.missile.is_alive());
    }

    /// The nearest bandit missile still guiding on the player (m), for the
    /// launch-warning tone and the HUD.
    pub fn incoming_on_player(&self, player: &Plane) -> Option<f64> {
        self.missiles
            .iter()
            .filter(|bm| !bm.missile.is_decoyed() && bm.missile.target_key() == Some(&TargetKey::Player))
            .map(|bm| (bm.missile.pos - player.pos).length())
            .min_by(f64::total_cmp)
    }

    /// Inject a blast at a body-frame point of bandit `i`; returns true when
    /// that hit destroyed it and credited `by`.
    pub fn hit(&mut self, i: u32, at: DVec3, power: f32, by: KillBy) -> bool {
        let Some(Some(b)) = self.items.get_mut(i as usize) else { return false };
        if !b.alive {
            return false;
        }
        let body = b.plane.rot.inverse() * (at - b.plane.model_pos());
        b.plane.blast(body.as_vec3(), power);
        if !b.plane.wrecked() {
            return false;
        }
        self.destroy(i as usize, by)
    }

    /// Mark bandit `i` destroyed and record the credit. Returns false if it
    /// was already down.
    pub fn destroy(&mut self, i: usize, by: KillBy) -> bool {
        let Self { items, kills, .. } = self;
        let Some(Some(b)) = items.get_mut(i) else { return false };
        if !b.alive {
            return false;
        }
        b.alive = false;
        b.wreck_t = 0.0;
        b.kill_credit.get_or_insert(by);
        *kills += 1;
        true
    }

    /// Did the player and any living bandit touch during this step? Both are
    /// swept from where they were to where they are, so a fast head-on pass
    /// cannot slip between two point samples.
    pub fn swept_collision(&self, player_prev: DVec3, player_now: DVec3, player_radius: f64) -> Option<usize> {
        for (i, b) in self.alive() {
            let (centre, radius) = b.plane.hull_sphere();
            let centre = centre.as_dvec3();
            let was = self.prev_pos.get(i).copied().unwrap_or(b.plane.pos) + b.plane.rot * centre;
            let now = b.plane.model_pos() + b.plane.rot * centre;
            if swept_hit(player_prev, player_now, player_radius, was, now, radius as f64) {
                return Some(i);
            }
        }
        None
    }

    /// Free the slots of wrecks that have finished burning. Indices stay put.
    pub fn remove_gone(&mut self) {
        for slot in &mut self.items {
            if slot.as_ref().is_some_and(|b| b.gone) {
                *slot = None;
            }
        }
    }
}

/// Do two hull spheres, each carried along a segment, touch during a step?
/// The closest approach of the two swept segments is compared with the sum of
/// the radii. A point test at the end positions misses a fast crossing.
pub fn swept_hit(a0: DVec3, a1: DVec3, ra: f64, b0: DVec3, b1: DVec3, rb: f64) -> bool {
    closest_segment_distance_sq(a0, a1, b0, b1) <= (ra + rb).powi(2)
}

/// Squared closest distance between segments `p1..q1` and `p2..q2`
/// (Ericson, Real-Time Collision Detection, §5.1.9).
fn closest_segment_distance_sq(p1: DVec3, q1: DVec3, p2: DVec3, q2: DVec3) -> f64 {
    let d1 = q1 - p1;
    let d2 = q2 - p2;
    let r = p1 - p2;
    let a = d1.length_squared();
    let e = d2.length_squared();
    let f = d2.dot(r);
    let (mut s, mut t);
    if a <= 1.0e-9 && e <= 1.0e-9 {
        return r.length_squared();
    }
    if a <= 1.0e-9 {
        s = 0.0;
        t = (f / e).clamp(0.0, 1.0);
    } else {
        let c = d1.dot(r);
        if e <= 1.0e-9 {
            t = 0.0;
            s = (-c / a).clamp(0.0, 1.0);
        } else {
            let b = d1.dot(d2);
            let denom = a * e - b * b;
            s = if denom > 1.0e-9 { ((b * f - c * e) / denom).clamp(0.0, 1.0) } else { 0.0 };
            t = (b * s + f) / e;
            if t < 0.0 {
                t = 0.0;
                s = (-c / a).clamp(0.0, 1.0);
            } else if t > 1.0 {
                t = 1.0;
                s = ((b - c) / a).clamp(0.0, 1.0);
            }
        }
    }
    let c1 = p1 + d1 * s;
    let c2 = p2 + d2 * t;
    (c1 - c2).length_squared()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aircraft::fa18;

    fn at(alt: f64) -> DVec3 {
        geo::lla_to_pos(0.0, 0.0, alt)
    }

    #[test]
    fn destroying_a_bandit_frees_its_slot_and_never_reuses_the_index() {
        let spec = &fa18::SPEC;
        let mut bandits = Bandits::new();
        let first = bandits.spawn(spec, at(2_000.0), DVec3::X, 200.0);
        assert_eq!(first, 0);
        let pos = bandits.get(first).unwrap().plane.pos;
        assert!(bandits.hit(first as u32, pos, 5.0, KillBy::Missile));
        assert_eq!(bandits.kills, 1);
        assert_eq!(bandits.get(first).unwrap().kill_credit, Some(KillBy::Missile));
        // Let the wreck finish, then free the slot.
        bandits.items[first].as_mut().unwrap().gone = true;
        bandits.remove_gone();
        assert!(bandits.items[first].is_none(), "the slot is free");
        // The next bandit takes a fresh index: index 0 stays empty.
        let second = bandits.spawn(spec, at(2_100.0), DVec3::X, 200.0);
        assert_eq!(second, 1);
        assert!(bandits.items[0].is_none());
        assert_eq!(bandits.items.len(), 2);
    }

    #[test]
    fn a_fast_crossing_is_caught_by_the_swept_test_where_points_miss() {
        // Two aircraft crossing at 1000 m/s each, the missile's top speed:
        // in one 1/180 s step each moves 5.6 m and they pass through each
        // other. Their end positions are 3.9 m apart, outside the 2 m sum of
        // two small hull radii, so a point test misses.
        let dt = 1.0 / 180.0;
        let v = 1_000.0 * dt;
        let a0 = DVec3::new(-0.5 * v, 0.0, 0.0);
        let a1 = DVec3::new(0.5 * v, 0.0, 0.0);
        let b0 = DVec3::new(0.0, 0.0, -0.5 * v);
        let b1 = DVec3::new(0.0, 0.0, 0.5 * v);
        assert!((a1 - b1).length() > 2.0, "the end points are outside the radii");
        assert!(swept_hit(a0, a1, 1.0, b0, b1, 1.0));
        // A pass five metres to one side misses.
        assert!(!swept_hit(a0, a1, 1.0, b0 + DVec3::Y * 5.0, b1 + DVec3::Y * 5.0, 1.0));
    }

    #[test]
    fn a_wreck_burns_for_the_dwell_then_is_gone() {
        let spec = &fa18::SPEC;
        let mut bandits = Bandits::new();
        let i = bandits.spawn(spec, at(2_000.0), DVec3::X, 200.0);
        bandits.destroy(i, KillBy::Missile);
        let dir = std::env::temp_dir().join("fablegame-bandit-tests");
        let src = std::sync::Arc::new(crate::sources::Sources::new(crate::fetch::Fetcher::new(dir, true), String::new()));
        let mut world = crate::world::World::new(crate::sources::Loader::new(src, 0), 14);
        world.flat = Some(0.0);
        let player = Plane::spawn(spec, 0.0, 0.0, 2_000.0, 0.0, 200.0);
        let h = 1.0 / 180.0;
        for _ in 0..(WRECK_DWELL / h) as usize - 2 {
            bandits.step(h, &world, &player, &[], &[]);
        }
        assert!(!bandits.items[i].as_ref().unwrap().gone);
        for _ in 0..4 {
            bandits.step(h, &world, &player, &[], &[]);
        }
        assert!(bandits.items[i].as_ref().unwrap().gone);
    }

    fn empty_world(name: &str) -> crate::world::World {
        let dir = std::env::temp_dir().join(name);
        let src = std::sync::Arc::new(crate::sources::Sources::new(crate::fetch::Fetcher::new(dir, true), String::new()));
        let mut w = crate::world::World::new(crate::sources::Loader::new(src, 0), 14);
        w.flat = Some(0.0);
        w
    }

    #[test]
    fn a_bandit_launches_its_ir_missile_and_the_warhead_reaches_the_player() {
        let spec = &fa18::SPEC;
        let world = empty_world("fablegame-bandit-missile-tests");
        let mut player = Plane::spawn(spec, 0.0, 0.0, 3_000.0, 0.0, spec.cruise);
        // A stationary target makes the geometry deterministic; the missile
        // still has to fly the whole envelope to the proximity fuse.
        player.vel = DVec3::ZERO;
        let mut bandits = Bandits::new();
        let i = bandits.spawn(spec, player.pos + player.forward() * 2_500.0, -player.forward(), spec.cruise);
        assert_eq!(bandits.get(i).unwrap().missiles_left(), 2, "the Hornet's two Sidewinder rails");
        let h = 1.0 / 180.0;
        let mut launched = false;
        let mut hit = false;
        for _ in 0..(25.0 / h) as usize {
            bandits.step(h, &world, &player, &[], &[]);
            for ev in bandits.events.drain(..) {
                match ev {
                    Event::MissileLaunch { owner, .. } => {
                        assert_eq!(owner, i);
                        launched = true;
                    }
                    Event::Blast { hit_player: true, .. } => hit = true,
                    _ => {}
                }
            }
            if hit {
                break;
            }
        }
        assert!(launched, "the bandit never took a missile shot");
        assert!(hit, "the missile never fused on the player");
        let b = bandits.get(i).unwrap();
        assert_eq!(b.missiles_left(), 1, "the fired station is still loaded");
        assert!(b.hides_piece("left sidewinder") || b.hides_piece("right sidewinder"), "the fired missile is still on the wing");
    }

    #[test]
    fn a_missile_on_the_tail_draws_the_bandits_flares() {
        let spec = &fa18::SPEC;
        let world = empty_world("fablegame-bandit-flare-tests");
        let player = Plane::spawn(spec, 0.0, 0.0, 3_000.0, 0.0, spec.cruise);
        let mut bandits = Bandits::new();
        let i = bandits.spawn(spec, player.pos + player.forward() * 5_000.0, -player.forward(), spec.cruise);
        let b = bandits.get(i).unwrap();
        let pos = b.plane.pos - b.plane.forward() * 2_000.0;
        let vel = b.plane.forward() * 800.0;
        let threat = Missile::launch(0, pos, vel, b.plane.rot, Some(TargetKey::Bandit(i as u32)));
        let h = 1.0 / 180.0;
        bandits.step(h, &world, &player, std::slice::from_ref(&threat), &[]);
        let b = bandits.get(i).unwrap();
        assert!(b.flares_left < weapons::FLARES_MAX, "the dispenser did not fire");
        assert!(!bandits.flares.is_empty(), "no flare is in the air");
        assert!(bandits.events.iter().any(|e| matches!(e, Event::Flare { owner, .. } if *owner == i)));
        // The pilot must also be defending, not flying on as if nothing had
        // happened.
        assert_eq!(b.ai.state, crate::ai::AiState::Evade);
    }

    #[test]
    fn a_bandit_presses_a_turning_player_and_gets_a_missile_away() {
        let spec = &fa18::SPEC;
        let world = empty_world("fablegame-bandit-press-tests");
        let mut player = Plane::spawn(spec, 0.0, 0.0, 3_500.0, 0.0, spec.cruise);
        let mut bandits = Bandits::new();
        let i = bandits.spawn(spec, player.pos + player.forward() * 4_000.0, -player.forward(), spec.cruise);
        let h = 1.0 / 180.0;
        let mut launched = 0;
        for step in 0..(35.0 / h) as usize {
            // A crude but real evasive target: full power and a hard,
            // reversing turn every eight seconds. The bandit has to keep the
            // contact, turn with it and take its chance.
            let roll = if (step / (8.0 / h) as usize) % 2 == 0 { 0.8 } else { -0.8 };
            let c = crate::plane::Controls { pitch: 0.45, roll, throttle: 1.0, ..Default::default() };
            assert!(player.step(h, &c, &world).is_none(), "the test player flew into the ground");
            bandits.step(h, &world, &player, &[], &[]);
            for ev in bandits.events.drain(..) {
                if matches!(ev, Event::MissileLaunch { .. }) {
                    launched += 1;
                }
            }
            if launched > 0 {
                break;
            }
        }
        assert!(launched > 0, "the bandit never took a shot at a turning target");
        assert!(bandits.get(i).unwrap().missiles_left() < 2);
    }

    #[test]
    fn a_bandit_missile_can_hit_a_turning_player() {
        let spec = &fa18::SPEC;
        let world = empty_world("fablegame-bandit-hit-tests");
        let mut player = Plane::spawn(spec, 0.0, 0.0, 3_500.0, 0.0, spec.cruise);
        let mut bandits = Bandits::new();
        bandits.spawn(spec, player.pos + player.forward() * 3_000.0, -player.forward(), spec.cruise);
        let h = 1.0 / 180.0;
        let (mut launched, mut hits) = (0, 0);
        for step in 0..(30.0 / h) as usize {
            let roll = if (step / (6.0 / h) as usize) % 2 == 0 { 0.6 } else { -0.6 };
            let c = crate::plane::Controls { pitch: 0.35, roll, throttle: 1.0, ..Default::default() };
            player.step(h, &c, &world);
            bandits.step(h, &world, &player, &[], &[]);
            for ev in bandits.events.drain(..) {
                match ev {
                    Event::MissileLaunch { .. } => launched += 1,
                    Event::Blast { hit_player: true, .. } => hits += 1,
                    _ => {}
                }
            }
        }
        assert!(launched > 0, "the bandit never shot");
        assert!(hits > 0, "two guided shots and neither reached a turning, non-flaring target");
    }

    #[test]
    fn a_head_on_pass_does_not_end_in_a_ram() {
        let spec = &fa18::SPEC;
        let world = empty_world("fablegame-bandit-merge-tests");
        let mut player = Plane::spawn(spec, 0.0, 0.0, 3_000.0, 0.0, spec.cruise);
        let mut bandits = Bandits::new();
        bandits.spawn(spec, player.pos + player.forward() * 2_000.0, -player.forward(), spec.cruise);
        let h = 1.0 / 180.0;
        let straight = crate::plane::Controls { throttle: 0.8, ..Default::default() };
        let mut closest = f64::MAX;
        for _ in 0..(12.0 / h) as usize {
            player.step(h, &straight, &world);
            bandits.step(h, &world, &player, &[], &[]);
            for (_, b) in bandits.alive() {
                closest = closest.min((b.plane.pos - player.pos).length());
            }
        }
        assert!(closest > 15.0, "the bandit passed within {closest:.1} m: a ram");
    }

    #[test]
    fn a_bandit_turns_back_at_the_combat_zone_edge() {
        let spec = &fa18::SPEC;
        let dir = std::env::temp_dir().join("fablegame-bandit-zone-tests");
        let src = std::sync::Arc::new(crate::sources::Sources::new(crate::fetch::Fetcher::new(dir, true), String::new()));
        let mut world = crate::world::World::new(crate::sources::Loader::new(src, 0), 14);
        world.flat = Some(0.0);
        let player = Plane::spawn(spec, 0.0, 0.0, 3_000.0, 0.0, spec.cruise);
        let (east, _, _) = geo::enu(player.pos);
        let zone = Zone::new(player.pos, 8_000.0);
        let mut bandits = Bandits::new();
        bandits.zone = Some(zone);
        // Near the 80% ring and flying straight at it.
        let i = bandits.spawn(spec, player.pos + east * 6_600.0, east, spec.cruise);
        let h = 1.0 / 180.0;
        let mut most = 0.0f64;
        for _ in 0..(30.0 / h) as usize {
            bandits.step(h, &world, &player, &[], &[]);
            most = most.max(zone.distance(bandits.get(i).unwrap().plane.pos));
        }
        let b = bandits.get(i).unwrap();
        assert!(b.returning, "the pilot did not turn back");
        assert!(most < zone.radius * 1.05, "it flew {most:.0} m from a {:.0} m centre", zone.radius);
        assert!(zone.distance(b.plane.pos) < zone.radius * 0.8, "it never came back inside");
    }
}
