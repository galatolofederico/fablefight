//! The fighters' cannon.
//!
//! A round is a real projectile: it leaves the muzzle with the aircraft's own
//! velocity added to the gun's, flies straight while it ages out, and every
//! fixed step it is swept from where it was to where it is and tested against
//! the hull spheres of the player and every bandit. A round that crosses a
//! whole airframe between two samples is still caught, as it is for collisions
//! between aircraft (`bandits::swept_hit`).
//!
//! The trigger is a rate, not a stream of events: an M61 fires 6,000 rounds a
//! minute, so a held trigger accumulates 100/180 of a round per 1/180 s step
//! and fires one whenever a whole round is owed. When the belt is empty the
//! gun stops, consumes nothing and spawns nothing.
//!
//! What a hit does goes through `Plane::blast`, the one entry point for
//! damage, at `ROUND_POWER`: a twenty-fifth of a warhead's power rather than
//! a warhead's.
use crate::aircraft::{Gun, Spec};
use crate::bandits::{Bandits, KillBy};
use crate::crash::Rng;
use crate::plane::Plane;
use glam::DVec3;

/// Muzzle velocity (m/s): the M61A1/A2's published 3,380 ft/s with the 20 mm
/// rounds the three fighters fire (General Dynamics Ordnance and Tactical
/// Systems, <https://www.gdots.com/armaments/aircraft-guns-gun-systems/m61a1/>).
pub const MUZZLE_SPEED: f64 = 1_030.0;

/// What one 20 mm round does in `Plane::blast`'s scale, where 1.0 is a light
/// missile's warhead. An M56 HE-I round carries about 10.7 g of explosive
/// against the 3 kg of a WDU-17/B, so its blast is around a 1/280th of a
/// Sidewinder's before the difference between a contact burst and a proximity
/// burst is counted; it is delivered into the structure rather than spread
/// around the airframe, so the game gives it more than that ratio.
/// `Plane::blast` adds 0.70 x power to the hull and 0.6 of it is wrecked, so
/// this value puts a gun kill at about twenty-two rounds through the fuselage.
/// That hit count is an estimate chosen for play, not a measured figure.
pub const ROUND_POWER: f32 = 0.04;

/// The most rounds in the air at once, both sides together: about twenty
/// seconds of the player's continuous fire, the budget in `PLAN.md` §
/// Compatibility, rollout, security, and performance.
pub const MAX_ROUNDS: usize = 2_000;

/// How long a round flies before it is dropped (s): at the muzzle velocity it
/// has covered 2.6 km, past the range an aircraft gun is used at.
const ROUND_LIFE: f64 = 2.5;

/// Who fired a round. A bandit is named by its slot in `bandits::Bandits`,
/// which is never reused, so the name cannot come to mean another aircraft.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shooter {
    Player,
    Bandit(u32),
}

/// A round in flight. Positions and velocities are in the planet frame.
#[derive(Clone, Copy, Debug)]
pub struct Round {
    pub pos: DVec3,
    pub vel: DVec3,
    pub age: f64,
    /// Who fired it: no round can hit the aircraft that fired it.
    pub from: Shooter,
}

/// Something worth a tracer, a flash or a word on the HUD.
#[derive(Clone, Copy, Debug)]
pub enum Event {
    /// A round left a muzzle, along `dir`.
    Fired { by: Shooter, at: DVec3, dir: DVec3 },
    /// A round struck an aircraft at `at`; `kill` when that was the hit that
    /// brought it down.
    Hit { at: DVec3, on: Shooter, kill: bool },
}

/// One gun's belt and how much of the next round is owed.
#[derive(Clone, Copy, Default)]
struct GunState {
    gun: Option<Gun>,
    ammo: u32,
    acc: f64,
}

/// Both sides' cannon: every round in the air, the player's belt, and the
/// rate each gun has accumulated.
pub struct Guns {
    pub rounds: Vec<Round>,
    /// The belt left in the player's cannon (rounds).
    pub ammo: u32,
    /// The player's gun, while its type has one.
    gun: Option<Gun>,
    /// How much of the next round the held trigger has paid for.
    acc: f64,
    /// Each bandit's gun, by its slot in `Bandits` (never reused).
    bandit: Vec<GunState>,
    rng: Rng,
}

impl Guns {
    pub fn new(seed: u64) -> Self {
        Self { rounds: Vec::new(), ammo: 0, gun: None, acc: 0.0, bandit: Vec::new(), rng: Rng(seed | 1) }
    }

    /// Back to a fresh aircraft: belt full, nothing in the air, no bandit guns.
    pub fn reset(&mut self, spec: &'static Spec) {
        self.rounds.clear();
        self.gun = spec.gun().copied();
        self.ammo = self.magazine();
        self.acc = 0.0;
        self.bandit.clear();
    }

    /// Fill the player's belt again, leaving rounds already in the air alone.
    /// `main.rs` calls this at each wave through `arena::recharge`.
    pub fn recharge(&mut self) {
        self.ammo = self.magazine();
    }

    /// Does the player's type carry a cannon at all?
    pub fn has_gun(&self) -> bool {
        self.gun.is_some()
    }

    /// What a full belt holds.
    pub fn magazine(&self) -> u32 {
        self.gun.map_or(0, |g| g.rounds)
    }

    /// Give bandit `i` its gun and a full belt, called when it is spawned.
    pub fn bandit_spawned(&mut self, i: u32, spec: &'static Spec) {
        let gun = spec.gun().copied();
        let st = GunState { gun, ammo: gun.map_or(0, |g| g.rounds), acc: 0.0 };
        if self.bandit.len() <= i as usize {
            self.bandit.resize(i as usize + 1, GunState::default());
        }
        self.bandit[i as usize] = st;
    }

    /// Pull the trigger once for `shooter`: one round if its belt has one.
    /// Rounds inherit the aircraft's own motion (`carrier`) on top of the
    /// muzzle velocity, so they keep up with it out of the barrel.
    pub fn fire(&mut self, shooter: Shooter, origin: DVec3, carrier: DVec3, direction: DVec3) -> bool {
        if self.gun_of(shooter).is_none() || self.rounds.len() >= MAX_ROUNDS {
            return false;
        }
        let belt = match shooter {
            Shooter::Player => &mut self.ammo,
            Shooter::Bandit(i) => match self.bandit.get_mut(i as usize) {
                Some(b) => &mut b.ammo,
                None => return false,
            },
        };
        if *belt == 0 {
            return false;
        }
        *belt -= 1;
        let dir = direction.normalize_or_zero();
        self.rounds.push(Round { pos: origin, vel: carrier + dir * MUZZLE_SPEED, age: 0.0, from: shooter });
        true
    }

    /// Advance the guns and every round by one fixed step: `trigger` is the
    /// player's trigger held, and each living bandit fires on its own pilot's.
    pub fn step(&mut self, h: f64, trigger: bool, plane: &mut Plane, bandits: &mut Bandits) -> Vec<Event> {
        let mut events = Vec::new();
        // The player's gun: the trigger held and a round owed at its rate.
        if trigger && self.has_gun() {
            let origin = plane.model_pos() + plane.rot * (plane.spec.hard[0] * 0.99);
            let radius = self.gun_of(Shooter::Player).map_or(0.0, |g| g.dispersion);
            self.burst(Shooter::Player, h, origin, plane.vel, plane.forward(), radius, &mut events);
        }
        // The bandits fire inside the AI's range gate, their bursts carrying
        // the pilot's own aim error rather than a perfect lead.
        for i in 0..bandits.items.len() {
            let shot = {
                let Some(b) = bandits.get(i) else { continue };
                if !b.alive || !b.ai.trigger() {
                    continue;
                }
                let Some(g) = self.gun_of(Shooter::Bandit(i as u32)) else { continue };
                let origin = b.plane.model_pos() + b.plane.rot * (b.spec.hard[0] * 0.99);
                (origin, b.plane.vel, b.plane.forward(), g.dispersion.max(b.ai.skill.aim_error))
            };
            self.burst(Shooter::Bandit(i as u32), h, shot.0, shot.1, shot.2, shot.3, &mut events);
        }

        let mut live = Vec::with_capacity(self.rounds.len());
        let rounds = std::mem::take(&mut self.rounds);
        for mut r in rounds {
            let from = r.pos;
            r.pos += r.vel * h;
            r.age += h;
            if r.age >= ROUND_LIFE {
                continue;
            }
            if !self.strike(&r, from, r.pos, plane, bandits, &mut events) {
                live.push(r);
            }
        }
        self.rounds = live;
        events
    }

    /// Fire as many rounds as the rate owes this step, up to the belt and the
    /// live-round cap. At 6,000 rounds a minute that is 5/9 of a round per
    /// 1/180 s step, or one round every second step or so.
    fn burst(&mut self, shooter: Shooter, h: f64, origin: DVec3, carrier: DVec3, dir: DVec3, radius: f64, events: &mut Vec<Event>) -> u32 {
        let Some(gun) = self.gun_of(shooter) else { return 0 };
        let mut acc = self.acc_of(shooter) + h * gun.rpm / 60.0;
        let mut n = 0;
        while acc >= 1.0 {
            if self.rounds.len() >= MAX_ROUNDS {
                // Nothing owed while the sky is full: the gun waits.
                acc = 1.0;
                break;
            }
            let aimed = self.spread(dir, radius);
            if !self.fire(shooter, origin, carrier, aimed) {
                // Dry: the trigger does not wind up rounds for later.
                acc = 0.0;
                break;
            }
            acc -= 1.0;
            n += 1;
            events.push(Event::Fired { by: shooter, at: origin, dir: aimed });
        }
        self.set_acc(shooter, acc);
        n
    }

    /// A round's direction: the boresight thrown a little way inside the gun's
    /// dispersion circle (and, for a bandit, a little further by its pilot's
    /// aim error).
    fn spread(&mut self, dir: DVec3, radius: f64) -> DVec3 {
        if radius <= 0.0 {
            return dir;
        }
        let d = self.rng.dir().as_dvec3();
        let mut side = d - dir * d.dot(dir);
        if side.length_squared() < 1.0e-8 {
            side = dir.cross(DVec3::Y);
            if side.length_squared() < 1.0e-8 {
                side = dir.cross(DVec3::X);
            }
        }
        let r = radius * (self.rng.f() as f64).sqrt();
        (dir + side.normalize_or_zero() * r).normalize_or_zero()
    }

    /// Test one round's step against the bandits and then the player. Returns
    /// true when it struck something, in which case it is spent.
    fn strike(&mut self, r: &Round, from: DVec3, to: DVec3, plane: &mut Plane, bandits: &mut Bandits, events: &mut Vec<Event>) -> bool {
        let mut hit = None;
        for i in 0..bandits.items.len() {
            let Some(b) = bandits.get(i) else { continue };
            if !b.alive || r.from == Shooter::Bandit(i as u32) {
                continue;
            }
            let (c, radius) = b.plane.hull_sphere();
            let c = b.plane.model_pos() + b.plane.rot * c.as_dvec3();
            if let Some(t) = segment_sphere_hit(from, to, c, radius as f64) {
                hit = Some((i as u32, from + (to - from) * t));
                break;
            }
        }
        if let Some((i, at)) = hit {
            let kill = bandits.hit(i, at, ROUND_POWER, KillBy::Gun);
            events.push(Event::Hit { at, on: Shooter::Bandit(i), kill });
            return true;
        }
        // A bandit's round can hit the player; the player's own cannot.
        if r.from != Shooter::Player {
            let (c, radius) = plane.hull_sphere();
            let c = plane.model_pos() + plane.rot * c.as_dvec3();
            if let Some(t) = segment_sphere_hit(from, to, c, radius as f64) {
                let at = from + (to - from) * t;
                let body = plane.rot.inverse() * (at - plane.model_pos());
                if plane.blast(body.as_vec3(), ROUND_POWER).is_some() {
                    events.push(Event::Hit { at, on: Shooter::Player, kill: plane.wrecked() });
                }
                return true;
            }
        }
        false
    }

    fn gun_of(&self, shooter: Shooter) -> Option<Gun> {
        match shooter {
            Shooter::Player => self.gun,
            Shooter::Bandit(i) => self.bandit.get(i as usize).and_then(|s| s.gun),
        }
    }

    fn acc_of(&self, shooter: Shooter) -> f64 {
        match shooter {
            Shooter::Player => self.acc,
            Shooter::Bandit(i) => self.bandit.get(i as usize).map_or(0.0, |s| s.acc),
        }
    }

    fn set_acc(&mut self, shooter: Shooter, v: f64) {
        match shooter {
            Shooter::Player => self.acc = v,
            Shooter::Bandit(i) => {
                if let Some(s) = self.bandit.get_mut(i as usize) {
                    s.acc = v;
                }
            }
        }
    }
}

/// Where along the segment `a..b` it first enters the sphere at `c` with
/// radius `r`, as a fraction of the step (0..1); None if it misses. A round
/// covers several metres in one step, so the whole segment is tested: its end
/// points alone would let it pass straight through a small target.
fn segment_sphere_hit(a: DVec3, b: DVec3, c: DVec3, r: f64) -> Option<f64> {
    let d = b - a;
    let m = a - c;
    let (qq, bb, cc) = (d.length_squared(), 2.0 * m.dot(d), m.length_squared() - r * r);
    if qq <= 1.0e-12 {
        return (m.length_squared() <= r * r).then_some(0.0);
    }
    let disc = bb * bb - 4.0 * qq * cc;
    if disc >= 0.0 {
        let root = disc.sqrt();
        let enter = (-bb - root) / (2.0 * qq);
        if (0.0..=1.0).contains(&enter) {
            return Some(enter);
        }
        // Started inside and left: the exit is where it struck.
        let exit = (-bb + root) / (2.0 * qq);
        if (0.0..=1.0).contains(&exit) {
            return Some(exit.max(0.0));
        }
    }
    // Wholly inside the sphere: the closest point of the step.
    let t = (-bb / (2.0 * qq)).clamp(0.0, 1.0);
    ((a + d * t - c).length_squared() <= r * r).then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aircraft::fa18;

    fn hornet() -> Plane {
        Plane::spawn(&fa18::SPEC, 0.0, 0.0, 3_000.0, 0.0, 220.0)
    }

    fn fired(events: &[Event]) -> u32 {
        events.iter().filter(|e| matches!(e, Event::Fired { .. })).count() as u32
    }

    #[test]
    fn a_held_trigger_fires_a_hundred_rounds_a_second() {
        let mut guns = Guns::new(7);
        guns.reset(&fa18::SPEC);
        let mut plane = hornet();
        let mut bandits = Bandits::new();
        let h = 1.0 / 180.0;
        let mut n = 0;
        for _ in 0..180 {
            n += fired(&guns.step(h, true, &mut plane, &mut bandits));
        }
        assert_eq!(guns.magazine(), fa18::GUN.rounds);
        assert!((n as i64 - 100).abs() <= 1, "{n} rounds fired in one second at 6,000 rpm");
    }

    #[test]
    fn the_magazine_runs_dry_and_then_nothing_fires() {
        let mut guns = Guns::new(11);
        guns.reset(&fa18::SPEC);
        let full = guns.magazine();
        let mut plane = hornet();
        let mut bandits = Bandits::new();
        let h = 1.0 / 180.0;
        let mut n = 0;
        // Hold the trigger well past the belt: 578 rounds are 5.78 s at 100/s.
        for _ in 0..(10.0 / h) as usize {
            n += fired(&guns.step(h, true, &mut plane, &mut bandits));
        }
        assert_eq!(n, full, "the belt emptied");
        assert_eq!(guns.ammo, 0);
        // Further presses spawn nothing and consume nothing.
        let mut after = 0;
        for _ in 0..180 {
            after += fired(&guns.step(h, true, &mut plane, &mut bandits));
        }
        assert_eq!(after, 0, "a dry gun fired");
        assert_eq!(guns.ammo, 0);
        assert!(!guns.fire(Shooter::Player, plane.pos, plane.vel, plane.forward()));
    }

    #[test]
    fn recharging_fills_the_belt_and_leaves_rounds_in_the_air() {
        let mut guns = Guns::new(13);
        guns.reset(&fa18::SPEC);
        let mut plane = hornet();
        let mut bandits = Bandits::new();
        let h = 1.0 / 180.0;
        for _ in 0..30 {
            guns.step(h, true, &mut plane, &mut bandits);
        }
        assert!(guns.ammo < guns.magazine(), "the belt did not empty at all");
        let flying = guns.rounds.len();
        assert!(flying > 0, "nothing was in the air to keep");
        guns.recharge();
        assert_eq!(guns.ammo, guns.magazine(), "the belt is not full");
        assert_eq!(guns.rounds.len(), flying, "rounds already in the air were cleared");
    }

    #[test]
    fn a_round_crossing_a_small_target_in_one_step_still_hits_it() {
        // 1,030 m/s at 1/180 s is 5.7 m a step: the target's 0.3 m radius sits
        // between the step's ends, where an end-point test would miss it.
        let a = DVec3::new(0.0, 0.0, 0.0);
        let b = DVec3::new(5.7, 0.0, 0.0);
        let c = DVec3::new(2.85, 0.0, 0.0);
        assert!(segment_sphere_hit(a, b, c, 0.3).is_some());
        assert!((a - c).length() > 0.3 && (b - c).length() > 0.3, "the ends are outside");
    }

    #[test]
    fn a_pass_outside_the_hull_radius_registers_no_hit() {
        let a = DVec3::new(0.0, 0.0, 0.0);
        let b = DVec3::new(5.7, 0.0, 0.0);
        assert!(segment_sphere_hit(a, b, DVec3::new(2.85, 0.31, 0.0), 0.3).is_none(), "a graze outside the radius hit");
        assert!(segment_sphere_hit(a, b, DVec3::new(2.85, 0.29, 0.0), 0.3).is_some(), "a graze inside the radius missed");
    }

    #[test]
    fn rounds_shot_at_a_bandit_tear_it_down() {
        let spec = &fa18::SPEC;
        let mut plane = hornet();
        let mut bandits = Bandits::new();
        let at = plane.pos + plane.forward() * 500.0;
        let i = bandits.spawn(spec, at, plane.forward(), 200.0);
        let mut guns = Guns::new(13);
        guns.reset(spec);
        guns.bandit_spawned(i as u32, spec);
        let h = 1.0 / 180.0;
        for _ in 0..(2.0 / h) as usize {
            guns.step(h, true, &mut plane, &mut bandits);
        }
        assert_eq!(bandits.kills, 1);
        assert!(!bandits.get(i).unwrap().alive);
        assert_eq!(bandits.get(i).unwrap().kill_credit, Some(KillBy::Gun));
    }
}
