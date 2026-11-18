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
use crate::ai::{Pilot, Skill};
use crate::aircraft::Spec;
use crate::arena::{zone_hull, Zone, ZoneHull};
use crate::geo;
use crate::plane::Plane;
use crate::weapons::{Target, TargetKey};
use crate::world::World;
use glam::DVec3;

/// How long a wrecked bandit burns and smokes before it is removed (s).
pub const WRECK_DWELL: f64 = 6.0;

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
}

impl Default for Bandits {
    fn default() -> Self {
        Self::new()
    }
}

impl Bandits {
    pub fn new() -> Self {
        Self { items: Vec::new(), zone: None, prev_pos: Vec::new(), kills: 0, deaths: Vec::new() }
    }

    /// A new flight: forget every bandit, including their indices.
    pub fn reset(&mut self) {
        self.items.clear();
        self.zone = None;
        self.prev_pos.clear();
        self.kills = 0;
        self.deaths.clear();
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

    /// Fly every bandit for one fixed step.
    pub fn step(&mut self, h: f64, world: &World, player: &Plane) {
        let Self { items, zone, prev_pos, deaths, .. } = self;
        if prev_pos.len() < items.len() {
            prev_pos.resize(items.len(), DVec3::ZERO);
        }
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
            // (Once bandits carry missiles, this is true while one of them is
            // tracking the player: the seeker keeps the contact alive.)
            let seen = b.ai.sense(&b.plane, player, false);
            let mut c = b.ai.controls(&b.plane, seen, h);
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
            bandits.step(h, &world, &player);
        }
        assert!(!bandits.items[i].as_ref().unwrap().gone);
        for _ in 0..4 {
            bandits.step(h, &world, &player);
        }
        assert!(bandits.items[i].as_ref().unwrap().gone);
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
            bandits.step(h, &world, &player);
            most = most.max(zone.distance(bandits.get(i).unwrap().plane.pos));
        }
        let b = bandits.get(i).unwrap();
        assert!(b.returning, "the pilot did not turn back");
        assert!(most < zone.radius * 1.05, "it flew {most:.0} m from a {:.0} m centre", zone.radius);
        assert!(zone.distance(b.plane.pos) < zone.radius * 0.8, "it never came back inside");
    }
}
