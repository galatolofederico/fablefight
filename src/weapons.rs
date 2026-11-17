//! Air-to-air missiles and countermeasures.
//!
//! The player's weapon system lives here: master arm, the seeker and its
//! lock, the six stations on the three fighters, the missiles themselves once
//! they have left the rails, and the flare dispenser. The flight model and the
//! rest of the aircraft know nothing about them; `main.rs` feeds in the target
//! list each fixed step and turns the events into sound, particles and damage.
//! Targets are live ADS-B aircraft, airport traffic, or the AI bandits
//! (`TargetKey::Bandit`). In dogfight mode (`Weapons::dogfight`) only the AI
//! aircraft can be locked: the traffic stays in the world as ambience.
//!
//! A missile is a simple boost-and-coast body: it leaves its station with the
//! aircraft's velocity, burns for a couple of seconds, then drags down while
//! its seeker follows the target's heat. Guidance is lead pursuit with a
//! finite turn rate, so a hard break can defeat it; a proximity fuse ends the
//! flight when it gets close enough. The seeker sees a target's tailpipe much
//! farther off than its nose, and will not launch inside the minimum range.
//! Flares leave the dispenser backwards; each step they may seduce a missile
//! that is close behind the target.

use crate::aircraft::{MissileStation, Spec};
use crate::airframe::{Piece, G_LWING, G_RWING};
use crate::geo;
use crate::world::World;
use glam::{DQuat, DVec3};
use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};

/// The seeker's cone, off the aircraft's nose (rad).
const LOCK_CONE: f64 = FRAC_PI_4;
/// How long a lock takes to build (s), and how quickly it falls away when the
/// target leaves the cone or range.
const LOCK_TIME: f64 = 1.1;
const LOCK_DECAY: f64 = 2.5;
/// Lock range: a base distance plus a little for each size class of target,
/// then scaled by the target's aspect: head-on (0°) is half the nominal
/// range, abeam (90°) the nominal, and dead astern (150° and beyond) two and
/// a half times it. A rear-aspect lock therefore reaches five times as far as
/// a head-on one. The break points are recorded in
/// `docs/fablefight-limitations.md`.
const LOCK_BASE: f64 = 10_000.0;
const LOCK_SIZE: f64 = 1_500.0;
const LOCK_NOSE: f64 = 0.5;
const LOCK_REAR: f64 = 2.5;
const ASPECT_NOSE: f64 = 30.0 * PI / 180.0;
const ASPECT_TAIL: f64 = 150.0 * PI / 180.0;
/// The closest a target can be and still be locked (m).
const MIN_RANGE: f64 = 150.0;
/// The closest a target can be and still be launched at (m): inside this the
/// motor and fuse would not arm before the missile reached it, and a crossing
/// target leaves the seeker's gimbal almost at once. A 500 m shot is refused.
const MIN_LAUNCH: f64 = 600.0;
/// How long a missile flies before it gives up (s).
const MISSILE_LIFE: f64 = 32.0;
/// The motor (s), its acceleration (m/s²), and the drag coefficient at speed
/// (a = k v²).  The latter is tuned so a missile burns out around Mach 1.5 and
/// then coasts down towards the target.
const BOOST_TIME: f64 = 2.8;
const BOOST_ACC: f64 = 170.0;
const DRAG_K: f64 = 0.00006;
/// The most lateral acceleration the fins can make (g), and the seeker's
/// gimbal limit (rad).  Past the limit the lock is lost.
const MAX_G: f64 = 32.0;
const SEEKER_GIMBAL: f64 = 65.0f64.to_radians();
/// How close a missile must pass to the target's centre to detonate (m).
const FUSE_MARGIN: f64 = 12.0;
const G0: f64 = 9.80665;
/// The dispenser: flares carried, released per press, and how long each burns
/// (s). The F-4E's two AN/ALE-40s carry 30 flares between them; the game gives
/// the same 30 to every fighter (see docs/fablefight-limitations.md).
const FLARES_MAX: u32 = 30;
const FLARE_SALVO: u32 = 2;
const FLARE_LIFE: f64 = 4.5;
/// How near the target (m) a flare must be, how far a missile's seeker can be
/// fooled by one (m), the most chance per step at point-blank dead astern,
/// and the fuse radius on a seduced flare (m).
const FLARE_NEAR: f64 = 400.0;
const FLARE_REACH: f64 = 2_500.0;
const FLARE_DECOY: f64 = 0.006;
const DECOY_FUSE: f64 = 18.0;
const RNG_SEED: u64 = 0x9E37_79B9_7F4A_7C15;

/// What an aircraft is, as far as a missile is concerned.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TargetKey {
    /// A live ADS-B aircraft, by its ICAO address.
    Live(String),
    /// A moving aircraft at an airport: which airfield, and the mover's id.
    Airport { apt: u32, mover: u32 },
    /// An AI aircraft, by its stable slot in `bandits::Bandits` (never reused).
    Bandit(u32),
}

/// One target a seeker may lock onto.
#[derive(Clone, Debug)]
pub struct Target {
    pub key: TargetKey,
    pub pos: DVec3,
    pub vel: DVec3,
    /// Its size class (0 light single .. 4 wide-body), which sets how hot it
    /// looks to the seeker and how big a fuse circle it needs.
    pub size: u8,
    pub label: String,
}

/// The locked target as the HUD wants it.
#[derive(Clone, Debug)]
pub struct TargetView {
    pub pos: DVec3,
    pub label: String,
    pub range: f64,
    pub closure: f64,
}

/// Why a weapon request was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    NotArmed,
    NoPower,
    NoLock,
    NoMissiles,
    NoWeapons,
    NoTarget,
    /// The target is closer than the missile's minimum launch range.
    TooClose,
    /// The flare dispenser is empty.
    NoFlares,
}

/// Something the weapon system did, for `main.rs` to turn into a flash, a
/// sound or particles.
#[derive(Clone, Debug)]
pub enum Event {
    /// The seeker has finished building a lock.
    Locked,
    /// The target went out of the seeker's cone/range, or disappeared.
    Lost,
    /// A missile left a station.
    Fired { station: usize },
    /// A request could not be carried out.
    Refused(Refusal),
    /// A flare left the dispenser.
    Flare { pos: DVec3, vel: DVec3 },
    /// A missile detonated.  `target` is set when it was the target that
    /// ended the flight (a hit), not the ground or a timeout.
    Blast {
        pos: DVec3,
        power: f32,
        size: f32,
        target: Option<TargetKey>,
    },
}

/// A burning flare: ejected backwards, it drags down and away from the aircraft
/// while it burns.
#[derive(Clone, Debug)]
pub struct Flare {
    pub id: u64,
    pub pos: DVec3,
    pub vel: DVec3,
    pub age: f64,
    pub life: f64,
}

/// One hardpoint: what it is, the model piece that is the loaded missile, and
/// where that piece's middle is in the body frame.
pub struct Station {
    pub spec: MissileStation,
    pub piece: Option<usize>,
    pub point: DVec3,
    pub loaded: bool,
}

/// A missile in flight.  Body frame is the aircraft's: the nose is -Z.
#[derive(Clone)]
pub struct Missile {
    pub station: usize,
    pub pos: DVec3,
    pub vel: DVec3,
    pub rot: DQuat,
    pub age: f64,
    /// How hard the motor is burning, 0 after burnout (for the smoke trail).
    pub burn: f32,
    target: Option<TargetKey>,
    /// The flare it has been seduced by, while that flare is alight.
    decoy: Option<u64>,
    alive: bool,
}

pub struct Weapons {
    pub stations: Vec<Station>,
    pub armed: bool,
    /// Dogfight mode: only the AI aircraft are shootable. Live ADS-B and
    /// airport traffic are in the world as ambience and can neither be locked
    /// nor hit, whatever the caller puts in the target list.
    pub dogfight: bool,
    /// How complete the lock is, 0..1.
    pub lock: f64,
    pub target: Option<TargetKey>,
    pub target_view: Option<TargetView>,
    pub missiles: Vec<Missile>,
    /// The burning flares in the air.
    pub flares: Vec<Flare>,
    /// Flares left in the dispensers.
    pub flares_left: u32,
    /// The seeker's tone: 0 silent, 0.1 searching, up to 1 locked.
    pub seeker_tone: f32,
    cycle: bool,
    fire: bool,
    flares_requested: bool,
    /// A lock event has been sent for the current target.
    locked: bool,
    /// The next flare's serial (never reused) and the decoy dice's state.
    next_flare: u64,
    rng: u64,
}

impl Weapons {
    pub fn new(spec: &'static Spec, pieces: &[Piece]) -> Self {
        let mut w = Self {
            stations: Vec::new(),
            armed: true,
            dogfight: false,
            lock: 0.0,
            target: None,
            target_view: None,
            missiles: Vec::new(),
            flares: Vec::new(),
            flares_left: 0,
            seeker_tone: 0.0,
            cycle: false,
            fire: false,
            flares_requested: false,
            locked: false,
            next_flare: 0,
            rng: RNG_SEED,
        };
        w.reset(spec, pieces);
        w
    }

    /// Back to a fresh aircraft: missiles loaded, flares stowed, weapons live,
    /// no lock.
    pub fn reset(&mut self, spec: &'static Spec, pieces: &[Piece]) {
        self.stations.clear();
        for st in spec.missiles() {
            let piece = pieces.iter().position(|p| p.name == st.piece);
            let point = piece
                .map(|i| pieces[i].pivot.as_dvec3())
                .unwrap_or(DVec3::ZERO);
            self.stations.push(Station {
                spec: *st,
                piece,
                point,
                loaded: true,
            });
        }
        self.armed = true;
        self.lock = 0.0;
        self.target = None;
        self.target_view = None;
        self.missiles.clear();
        self.flares.clear();
        self.flares_left = if self.stations.is_empty() { 0 } else { FLARES_MAX };
        self.seeker_tone = 0.0;
        self.cycle = false;
        self.fire = false;
        self.flares_requested = false;
        self.locked = false;
        self.next_flare = 0;
        self.rng = RNG_SEED;
    }

    pub fn has_weapons(&self) -> bool {
        !self.stations.is_empty()
    }

    /// May the seeker use this target? In dogfight mode only the AI aircraft:
    /// live ADS-B and airport traffic are ambience, not targets.
    pub fn shootable(&self, key: &TargetKey) -> bool {
        !self.dogfight || matches!(key, TargetKey::Bandit(_))
    }

    pub fn station(&self, i: usize) -> Option<&Station> {
        self.stations.get(i)
    }

    pub fn loaded(&self, i: usize) -> bool {
        self.stations.get(i).is_some_and(|s| s.loaded)
    }

    pub fn toggle_arm(&mut self) -> bool {
        self.armed = !self.armed;
        if !self.armed {
            self.clear_lock();
        }
        self.armed
    }

    pub fn request_cycle(&mut self) {
        self.cycle = true;
    }

    pub fn request_fire(&mut self) {
        self.fire = true;
    }

    pub fn request_flares(&mut self) {
        self.flares_requested = true;
    }

    /// Is the loaded missile of this model piece gone (and so hidden)?
    pub fn hides_piece(&self, name: &str) -> bool {
        self.stations
            .iter()
            .any(|s| s.piece.is_some() && s.spec.piece == name && !s.loaded)
    }

    /// Fly the seeker, the flares and every missile on.  `dt` is the fixed
    /// physics step; `powered` is the aircraft's electrical bus.
    pub fn step(
        &mut self,
        dt: f64,
        plane: &mut crate::plane::Plane,
        targets: &[Target],
        world: &World,
        powered: bool,
    ) -> Vec<Event> {
        let mut events = Vec::new();
        // In dogfight mode a target list may still hold the traffic the world
        // draws around the fight; none of it is shootable. Filter before the
        // seeker or any missile sees it, so live ADS-B and airport traffic can
        // never be locked or hit by mistake.
        let picky: Vec<Target>;
        let targets: &[Target] = if self.dogfight {
            picky = targets.iter().filter(|t| self.shootable(&t.key)).cloned().collect();
            &picky
        } else {
            targets
        };
        let can_seek = self.armed && powered && !self.stations.is_empty();
        if !can_seek {
            self.clear_lock();
            self.seeker_tone = 0.0;
        }

        if std::mem::take(&mut self.cycle) {
            if !self.armed {
                events.push(Event::Refused(Refusal::NotArmed));
            } else if !powered {
                events.push(Event::Refused(Refusal::NoPower));
            } else if self.stations.is_empty() {
                events.push(Event::Refused(Refusal::NoWeapons));
            } else if !self.cycle_target(plane, targets) {
                events.push(Event::Refused(Refusal::NoTarget));
            }
        }

        if can_seek {
            if let Some(key) = self.target.clone() {
                if let Some(t) = targets.iter().find(|t| t.key == key) {
                    let (range, angle) = Self::aim(plane, t);
                    let aspect = Self::aspect(plane, t);
                    if range >= MIN_RANGE && range <= Self::lock_range(t.size, aspect) && angle <= LOCK_CONE
                    {
                        self.lock = (self.lock + dt / LOCK_TIME).min(1.0);
                        self.set_view(t, plane);
                        if self.lock >= 1.0 && !self.locked {
                            self.locked = true;
                            events.push(Event::Locked);
                        }
                    } else {
                        self.lock = (self.lock - dt * LOCK_DECAY).max(0.0);
                        self.set_view(t, plane);
                        if self.lock <= 0.0 {
                            self.target = None;
                            self.target_view = None;
                            self.locked = false;
                            events.push(Event::Lost);
                        }
                    }
                } else {
                    self.target = None;
                    self.lock = 0.0;
                    self.target_view = None;
                    self.locked = false;
                    events.push(Event::Lost);
                }
            } else {
                self.lock = 0.0;
                self.target_view = None;
                self.locked = false;
            }
        }

        if std::mem::take(&mut self.flares_requested) {
            if self.stations.is_empty() {
                events.push(Event::Refused(Refusal::NoWeapons));
            } else if !powered {
                events.push(Event::Refused(Refusal::NoPower));
            } else {
                events.extend(self.dispense(plane));
            }
        }
        self.drift_flares(dt);

        if std::mem::take(&mut self.fire) {
            events.extend(self.try_fire(plane, powered));
        }

        self.seduce(targets);
        for m in &mut self.missiles {
            m.step(dt, targets, &self.flares, world, &mut events);
        }
        self.missiles.retain(|m| m.alive);

        self.seeker_tone = if can_seek {
            if self.target_view.is_some() {
                (0.25 + 0.75 * self.lock) as f32
            } else {
                0.1
            }
        } else {
            0.0
        };
        events
    }

    /// One press of the dispenser: a small salvo backwards and down, with one
    /// event per flare for the visuals.
    fn dispense(&mut self, plane: &crate::plane::Plane) -> Vec<Event> {
        if self.flares_left == 0 {
            return vec![Event::Refused(Refusal::NoFlares)];
        }
        let n = FLARE_SALVO.min(self.flares_left);
        let mut events = Vec::with_capacity(n as usize);
        for _ in 0..n {
            // Left and right dispensers alternate, as the real ones would.
            let side = if self.next_flare % 2 == 0 { -1.0 } else { 1.0 };
            let pos = plane.model_pos() + plane.rot * DVec3::new(side * 0.35, -0.55, 1.6);
            let vel = plane.vel + plane.rot * DVec3::new(side * 6.0, -5.0, 16.0);
            self.flares.push(Flare { id: self.next_flare, pos, vel, age: 0.0, life: FLARE_LIFE });
            self.next_flare += 1;
            self.flares_left -= 1;
            events.push(Event::Flare { pos, vel });
        }
        events
    }

    /// Let the burning flares fall behind and burn out.
    fn drift_flares(&mut self, dt: f64) {
        for f in &mut self.flares {
            f.age += dt;
            f.vel -= f.pos.normalize_or_zero() * G0 * dt;
            // A flare is a bluff body: it slows quickly against the air.
            f.vel *= (1.0 - 0.35 * dt).max(0.0);
            f.pos += f.vel * dt;
        }
        self.flares.retain(|f| f.age < f.life);
    }

    /// Roll each missile's seeker against every flare: close behind the
    /// target's tail is what fools it, and farther away is less and less
    /// likely.
    fn seduce(&mut self, targets: &[Target]) {
        if self.flares.is_empty() || self.missiles.is_empty() {
            return;
        }
        let flares: Vec<(u64, DVec3)> = self.flares.iter().map(|f| (f.id, f.pos)).collect();
        let mut rng = self.rng;
        for m in &mut self.missiles {
            if m.decoy.is_some() {
                continue;
            }
            let Some(t) = m.target.as_ref().and_then(|k| targets.iter().find(|t| &t.key == k)) else {
                continue;
            };
            let range = (t.pos - m.pos).length();
            for &(id, pos) in &flares {
                let near = 1.0 - ((pos - t.pos).length() / FLARE_NEAR).clamp(0.0, 1.0);
                let chance = decoy_chance(range, behind_factor(t, pos)) * near;
                if chance > 0.0 && next_random(&mut rng) < chance {
                    m.decoy = Some(id);
                    m.target = None;
                    break;
                }
            }
        }
        self.rng = rng;
    }

    /// Lock range for a target (m): its size class sets the nominal range, and
    /// its aspect at the seeker scales it (see `aspect_factor`). `aspect` is
    /// the angle at the target between its nose and the line back to us: 0
    /// head-on, PI dead astern.
    fn lock_range(size: u8, aspect: f64) -> f64 {
        (LOCK_BASE + LOCK_SIZE * size.min(4) as f64) * aspect_factor(aspect)
    }

    /// Range and angle off the nose to a target.
    fn aim(plane: &crate::plane::Plane, t: &Target) -> (f64, f64) {
        let d = t.pos - plane.pos;
        (
            d.length(),
            plane.forward().angle_between(d.normalize_or_zero()),
        )
    }

    /// How the target presents itself to the seeker: the angle at the target
    /// between its own nose (its velocity) and the line back to our aircraft.
    /// A target with no velocity has no tailpipe to aim at, so it counts as
    /// abeam and gets the nominal range.
    fn aspect(plane: &crate::plane::Plane, t: &Target) -> f64 {
        if t.vel.length_squared() < 1.0 {
            return FRAC_PI_2;
        }
        let to_shooter = plane.pos - t.pos;
        if to_shooter.length_squared() < 1.0 {
            return FRAC_PI_2;
        }
        t.vel.normalize().angle_between(to_shooter.normalize())
    }

    fn set_view(&mut self, t: &Target, plane: &crate::plane::Plane) {
        let d = t.pos - plane.pos;
        let range = d.length();
        let los = d.normalize_or_zero();
        self.target_view = Some(TargetView {
            pos: t.pos,
            label: t.label.clone(),
            range,
            closure: (t.vel - plane.vel).dot(los),
        });
    }

    fn clear_lock(&mut self) {
        self.target = None;
        self.target_view = None;
        self.lock = 0.0;
        self.locked = false;
    }

    fn cycle_target(&mut self, plane: &crate::plane::Plane, targets: &[Target]) -> bool {
        let mut cand: Vec<(f64, f64, usize)> = Vec::new();
        for (i, t) in targets.iter().enumerate() {
            let (range, angle) = Self::aim(plane, t);
            let aspect = Self::aspect(plane, t);
            if range >= MIN_RANGE && range <= Self::lock_range(t.size, aspect) && angle <= LOCK_CONE {
                cand.push((angle, range, i));
            }
        }
        if cand.is_empty() {
            self.clear_lock();
            return false;
        }
        cand.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        let current = self
            .target
            .as_ref()
            .and_then(|k| cand.iter().position(|(_, _, i)| &targets[*i].key == k));
        let pick = current.map_or(0, |i| if cand.len() > 1 { (i + 1) % cand.len() } else { i });
        let t = &targets[cand[pick].2];
        let key = t.key.clone();
        if self.target.as_ref() != Some(&key) {
            self.lock = 0.0;
            self.locked = false;
        }
        self.target = Some(key);
        self.set_view(t, plane);
        true
    }

    fn station_ok(&self, plane: &crate::plane::Plane, i: usize) -> bool {
        let Some(s) = self.stations.get(i) else {
            return false;
        };
        // A wing that has been torn away takes its stores with it; a fuselage
        // store does not care what happened to a wing.
        let on_wing = match s.spec.group {
            G_LWING | G_RWING => plane.dmg.wing[s.spec.side] as f64 + 0.5 >= s.point.x.abs(),
            _ => true,
        };
        s.loaded && s.piece.is_some() && on_wing
    }

    /// The launch path shared by the fire key and the tests.
    pub(crate) fn try_fire(&mut self, plane: &mut crate::plane::Plane, powered: bool) -> Vec<Event> {
        if !self.armed {
            return vec![Event::Refused(Refusal::NotArmed)];
        }
        if !powered {
            return vec![Event::Refused(Refusal::NoPower)];
        }
        if self.stations.is_empty() {
            return vec![Event::Refused(Refusal::NoWeapons)];
        }
        if self.lock < 1.0 || self.target.is_none() {
            return vec![Event::Refused(Refusal::NoLock)];
        }
        let Some(i) = (0..self.stations.len()).find(|i| self.station_ok(plane, *i)) else {
            return vec![Event::Refused(Refusal::NoMissiles)];
        };
        if self.target_view.as_ref().is_some_and(|v| v.range < MIN_LAUNCH) {
            return vec![Event::Refused(Refusal::TooClose)];
        }
        let st = &self.stations[i];
        let pos = plane.model_pos() + plane.rot * st.point;
        let vel = plane.vel + plane.forward() * 12.0;
        let rot = plane.rot;
        let target = self.target.clone();
        let mass = st.spec.mass as f64;
        self.missiles.push(Missile {
            station: i,
            pos,
            vel,
            rot,
            age: 0.0,
            burn: 0.0,
            target,
            decoy: None,
            alive: true,
        });
        self.stations[i].loaded = false;
        // The empty launcher stays on the aircraft; only the missile's mass goes.
        plane.stores = (plane.stores - mass).max(0.0);
        vec![Event::Fired { station: i }]
    }
}

/// How much of its nominal lock range the seeker gets at a target aspect:
/// half head-on, the nominal abeam, two and a half times dead astern, linear
/// between the break points at 30°, 90° and 150°.
fn aspect_factor(aspect: f64) -> f64 {
    let a = aspect.abs().min(PI);
    if a <= ASPECT_NOSE {
        LOCK_NOSE
    } else if a <= FRAC_PI_2 {
        LOCK_NOSE + (1.0 - LOCK_NOSE) * (a - ASPECT_NOSE) / (FRAC_PI_2 - ASPECT_NOSE)
    } else if a <= ASPECT_TAIL {
        1.0 + (LOCK_REAR - 1.0) * (a - FRAC_PI_2) / (ASPECT_TAIL - FRAC_PI_2)
    } else {
        LOCK_REAR
    }
}

/// How much a flare lies behind the target's own tail (1 dead astern, 0 ahead
/// of its wing): only a flare in the target's exhaust wake can fool an IR
/// seeker into following it.
fn behind_factor(t: &Target, flare: DVec3) -> f64 {
    let back = -t.vel.normalize_or_zero();
    let to_flare = (flare - t.pos).normalize_or_zero();
    back.dot(to_flare).max(0.0)
}

/// Chance per step that a seeker tracking a target at `range` (m) transfers to
/// a flare `behind` the target's tail. It falls with range and reaches zero at
/// `FLARE_REACH`; `behind` is 0..1. Bounded to 0..1 for any argument.
fn decoy_chance(range: f64, behind: f64) -> f64 {
    let near = 1.0 - (range / FLARE_REACH).clamp(0.0, 1.0);
    (FLARE_DECOY * near * near * behind.clamp(0.0, 1.0)).clamp(0.0, 1.0)
}

/// A deterministic xorshift, so a replay of the same shots rolls the same dice.
fn next_random(state: &mut u64) -> f64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    (*state >> 11) as f64 / (1u64 << 53) as f64
}

impl Missile {
    fn step(&mut self, dt: f64, targets: &[Target], flares: &[Flare], world: &World, events: &mut Vec<Event>) {
        self.age += dt;
        self.burn = if self.age < BOOST_TIME {
            (1.0 - self.age / BOOST_TIME) as f32
        } else {
            0.0
        };
        if self.age >= MISSILE_LIFE {
            events.push(Event::Blast {
                pos: self.pos,
                power: 0.4,
                size: 0.5,
                target: None,
            });
            self.alive = false;
            return;
        }

        if let Some(f) = self.decoy.and_then(|id| flares.iter().find(|f| f.id == id)) {
            // Seduced: the seeker follows the flare, and the fuse fires on it.
            self.fly_at(dt, f.pos, f.vel);
            if (f.pos - self.pos).length() <= DECOY_FUSE {
                events.push(Event::Blast {
                    pos: self.pos,
                    power: 0.35,
                    size: 0.2,
                    target: None,
                });
                self.alive = false;
                return;
            }
        } else {
            // (The flare it was following has burnt out: it flies on blind.)
            if self.decoy.take().is_some() {
                self.target = None;
            }
            let tgt = self
                .target
                .as_ref()
                .and_then(|k| targets.iter().find(|t| &t.key == k));
            self.fly(dt, tgt);

            if let Some(t) = tgt {
                let d = (t.pos - self.pos).length();
                let radius = target_radius(t.size);
                if d <= radius + FUSE_MARGIN {
                    events.push(Event::Blast {
                        pos: self.pos,
                        power: blast_power(t.size),
                        size: t.size as f32,
                        target: Some(t.key.clone()),
                    });
                    self.alive = false;
                    return;
                }
            }
        }

        // The ground (or the sea) ends it, too.
        let (lat, lon, alt) = geo::pos_to_lla(self.pos);
        if let Some(g) = world.ground(lat, lon) {
            if alt <= g.h + 0.6 {
                events.push(Event::Blast {
                    pos: geo::lla_to_pos(lat, lon, g.h),
                    power: 0.8,
                    size: 0.6,
                    target: None,
                });
                self.alive = false;
            }
        } else if alt < 0.0 {
            events.push(Event::Blast {
                pos: self.pos,
                power: 0.6,
                size: 0.5,
                target: None,
            });
            self.alive = false;
        }
    }

    /// One step of guidance and motion, without the hit and ground checks.
    fn fly(&mut self, dt: f64, tgt: Option<&Target>) {
        if let Some(t) = tgt {
            self.fly_at(dt, t.pos, t.vel);
        } else {
            self.coast(dt);
        }
    }

    /// Turn towards a point (a target's or a flare's centre) and fly on.
    fn fly_at(&mut self, dt: f64, pos: DVec3, vel: DVec3) {
        if !self.steer(dt, pos, vel) {
            // The point has slid out of the seeker's gimbal: the missile flies
            // on, but it is no longer guiding. (The fuse still gets its chance
            // this step.)
            self.target = None;
            self.decoy = None;
        }
        self.coast(dt);
    }

    /// The body's motion this step: motor, drag, gravity and the speed limit.
    fn coast(&mut self, dt: f64) {
        let speed = self.vel.length();
        let mut acc = self.vel.normalize_or_zero()
            * if self.age < BOOST_TIME {
                BOOST_ACC
            } else {
                0.0
            };
        if speed > 1.0 {
            acc -= self.vel / speed * (DRAG_K * speed * speed);
        }
        acc -= self.pos.normalize() * G0;
        self.vel += acc * dt;
        let s = self.vel.length();
        if s > 1_000.0 {
            self.vel *= 1_000.0 / s;
        }
        self.pos += self.vel * dt;
        // The model follows the flight path (the missile is symmetric, so its
        // roll does not matter).
        if self.vel.length_squared() > 1.0 {
            self.rot = DQuat::from_rotation_arc(DVec3::NEG_Z, self.vel.normalize());
        }
    }

    /// Turn the flight path towards the lead point of a target (or, with a
    /// flare, its current position). The fins bend the velocity itself, not
    /// just the model, so this works after burnout too. Returns false when the
    /// aim point is outside the seeker's gimbal.
    fn steer(&mut self, dt: f64, pos: DVec3, vel: DVec3) -> bool {
        let los = pos - self.pos;
        let dist = los.length();
        let los_hat = los.normalize_or_zero();
        let speed = self.vel.length();
        let angle = self.vel.normalize_or_zero().angle_between(los_hat);
        if angle > SEEKER_GIMBAL || dist <= 1.0 || speed <= 1.0 {
            return false;
        }
        // Lead the target: where it will be when the missile can get there,
        // solved a few times because the range changes too. A flare hardly
        // moves, and its own drift is all the lead it needs.
        let closing = (speed + vel.dot(los_hat)).max(150.0);
        let mut t_go = dist / closing;
        for _ in 0..3 {
            t_go = (pos + vel * t_go - self.pos).length() / closing;
        }
        let aim = pos + vel * t_go.clamp(0.0, 15.0);
        let desired = (aim - self.pos).normalize_or_zero();
        let cur = self.vel / speed;
        let max_rate = MAX_G * G0 / speed.max(100.0);
        let turn = (max_rate * dt).min(cur.angle_between(desired));
        let axis = cur.cross(desired).normalize_or_zero();
        if axis != DVec3::ZERO && turn > 0.0 {
            self.vel = DQuat::from_axis_angle(axis, turn) * self.vel;
        }
        true
    }
}

/// How far from a target's centre its fuse should fire (m): its half span,
/// with a little room to spare.
fn target_radius(size: u8) -> f64 {
    let ty = &crate::airmodels::TYPES[size.min(4) as usize];
    (ty.span as f64 * 0.5).max(5.0)
}

fn blast_power(size: u8) -> f32 {
    (1.0 + 0.25 * size.min(4) as f32).min(2.2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::airframe::{Anim, Mesh, Piece, Role, Vis};
    use glam::Vec3;

    fn sidewinder(name: &'static str, x: f32) -> Piece {
        Piece {
            name,
            group: crate::airframe::G_LWING,
            mesh: Mesh::default(),
            parent: None,
            pivot: Vec3::new(x, 0.0, 1.4),
            axis: Vec3::X,
            anim: Anim::Fixed,
            role: Role::Other,
            vis: Vis::Always,
            glass: false,
            occluder: false,
        }
    }

    fn hornet_with_missiles() -> (crate::plane::Plane, Vec<Piece>) {
        let pieces = vec![
            sidewinder("left sidewinder", -5.9),
            sidewinder("right sidewinder", 5.9),
        ];
        let plane =
            crate::plane::Plane::spawn(&crate::aircraft::fa18::SPEC, 0.0, 0.0, 1_000.0, 0.0, 200.0);
        (plane, pieces)
    }

    #[test]
    fn a_target_in_front_locks_and_fires() {
        let (mut plane, pieces) = hornet_with_missiles();
        let mut w = Weapons::new(&crate::aircraft::fa18::SPEC, &pieces);
        w.armed = true;
        let target = Target {
            key: TargetKey::Live("TEST123".into()),
            pos: plane.pos + plane.forward() * 5_000.0,
            vel: plane.vel,
            size: 2,
            label: "TEST123".into(),
        };
        assert!(w.cycle_target(&plane, std::slice::from_ref(&target)));
        w.lock = 1.0;
        let events = w.try_fire(&mut plane, true);
        assert!(matches!(events.as_slice(), [Event::Fired { station: 0 }]));
        assert!(!w.loaded(0));
        assert_eq!(w.missiles.len(), 1);
    }

    #[test]
    fn safe_aircraft_refuses_to_fire() {
        let (mut plane, pieces) = hornet_with_missiles();
        let mut w = Weapons::new(&crate::aircraft::fa18::SPEC, &pieces);
        w.armed = false;
        w.lock = 1.0;
        let events = w.try_fire(&mut plane, true);
        assert!(matches!(
            events.as_slice(),
            [Event::Refused(Refusal::NotArmed)]
        ));
    }

    #[test]
    fn a_second_shot_uses_the_other_wingtip() {
        let (mut plane, pieces) = hornet_with_missiles();
        let mut w = Weapons::new(&crate::aircraft::fa18::SPEC, &pieces);
        w.armed = true;
        let target = Target {
            key: TargetKey::Live("TEST123".into()),
            pos: plane.pos + plane.forward() * 5_000.0,
            vel: plane.vel,
            size: 2,
            label: "TEST123".into(),
        };
        w.target = Some(target.key.clone());
        w.lock = 1.0;
        assert!(matches!(
            w.try_fire(&mut plane, true).as_slice(),
            [Event::Fired { station: 0 }]
        ));
        assert!(matches!(
            w.try_fire(&mut plane, true).as_slice(),
            [Event::Fired { station: 1 }]
        ));
        assert!(matches!(
            w.try_fire(&mut plane, true).as_slice(),
            [Event::Refused(Refusal::NoMissiles)]
        ));
    }

    #[test]
    fn guided_missile_closes_on_a_target() {
        let start = geo::lla_to_pos(0.0, 0.0, 1_000.0);
        let mut m = Missile {
            station: 0,
            pos: start,
            vel: DVec3::X * 200.0,
            rot: DQuat::from_rotation_arc(DVec3::NEG_Z, DVec3::X),
            age: 0.0,
            burn: 0.0,
            target: None,
            decoy: None,
            alive: true,
        };
        let mut t = Target {
            key: TargetKey::Live("LEAD".into()),
            pos: start + DVec3::new(2_000.0, 0.0, -1_000.0),
            vel: DVec3::NEG_Z * 200.0,
            size: 2,
            label: "LEAD".into(),
        };
        let dt = 1.0 / 180.0;
        let mut closest = (t.pos - m.pos).length();
        for _ in 0..(20.0 / dt) as usize {
            m.age += dt;
            m.fly(dt, Some(&t));
            t.pos += t.vel * dt;
            closest = closest.min((t.pos - m.pos).length());
        }
        assert!(closest < target_radius(2) + FUSE_MARGIN, "missed by {closest:.0} m");
    }

    #[test]
    fn guidance_turns_the_flight_path_after_burnout() {
        let mut m = Missile {
            station: 0,
            pos: DVec3::ZERO,
            vel: DVec3::X * 300.0,
            rot: DQuat::IDENTITY,
            age: 10.0,
            burn: 0.0,
            target: None,
            decoy: None,
            alive: true,
        };
        let target = Target {
            key: TargetKey::Live("TURN".into()),
            pos: DVec3::new(1_000.0, 0.0, -1_000.0),
            vel: DVec3::ZERO,
            size: 2,
            label: "TURN".into(),
        };
        for _ in 0..180 {
            assert!(m.steer(1.0 / 180.0, target.pos, target.vel));
        }
        // It started flying +X; a second of 32 g at 300 m/s should have bent
        // the flight path well towards -Z.
        assert!(m.vel.z < -100.0, "{:?}", m.vel);
    }

    #[test]
    fn lock_range_grows_with_the_target() {
        assert!(Weapons::lock_range(4, FRAC_PI_2) > Weapons::lock_range(0, FRAC_PI_2));
        assert!((Weapons::lock_range(0, FRAC_PI_2) - LOCK_BASE).abs() < 1e-9);
    }

    #[test]
    fn rear_aspect_locks_five_times_as_far_as_head_on() {
        // The documented ratio: half the nominal range from the nose, two and
        // a half times it from the tail.
        for size in 0..=4 {
            let nose = Weapons::lock_range(size, 0.0);
            let rear = Weapons::lock_range(size, PI);
            assert!((rear / nose - LOCK_REAR / LOCK_NOSE).abs() < 1e-9, "size {size}: {rear} / {nose}");
            assert!(rear > 4.0 * nose, "rear {rear} is not four times head-on {nose}");
            // And the break points: half at 30 degrees or less, nominal abeam.
            assert!((Weapons::lock_range(size, ASPECT_NOSE) - LOCK_NOSE * (LOCK_BASE + LOCK_SIZE * size as f64)).abs() < 1e-9);
            assert!((Weapons::lock_range(size, FRAC_PI_2) - (LOCK_BASE + LOCK_SIZE * size as f64)).abs() < 1e-9);
            assert!((Weapons::lock_range(size, ASPECT_TAIL) - LOCK_REAR * (LOCK_BASE + LOCK_SIZE * size as f64)).abs() < 1e-9);
        }
    }

    #[test]
    fn a_launch_inside_the_minimum_range_is_refused() {
        let (mut plane, pieces) = hornet_with_missiles();
        let mut w = Weapons::new(&crate::aircraft::fa18::SPEC, &pieces);
        let target = Target {
            key: TargetKey::Live("CLOSE".into()),
            pos: plane.pos + plane.forward() * (MIN_LAUNCH - 100.0),
            vel: plane.vel,
            size: 2,
            label: "CLOSE".into(),
        };
        assert!(w.cycle_target(&plane, std::slice::from_ref(&target)));
        w.lock = 1.0;
        assert!(matches!(
            w.try_fire(&mut plane, true).as_slice(),
            [Event::Refused(Refusal::TooClose)]
        ));
        assert!(w.loaded(0), "a refused launch keeps its missile");
        assert!(w.missiles.is_empty());
        // A 500 m crossing shot is inside the documented 600 m minimum.
        assert!(500.0 < MIN_LAUNCH);
    }

    #[test]
    fn flares_never_exceed_the_carry_and_run_out_cleanly() {
        let (plane, pieces) = hornet_with_missiles();
        let mut w = Weapons::new(&crate::aircraft::fa18::SPEC, &pieces);
        assert_eq!(w.flares_left, FLARES_MAX);
        let mut dispensed = 0;
        for _ in 0..(FLARES_MAX + 10) {
            let before = w.flares.len();
            let ev = w.dispense(&plane);
            let added = w.flares.len() - before;
            dispensed += added;
            assert_eq!(w.flares.len() as u32 + w.flares_left, FLARES_MAX, "carry plus fired always 30");
            if added > 0 {
                assert!(ev.iter().all(|e| matches!(e, Event::Flare { .. })));
            } else {
                assert!(matches!(ev.as_slice(), [Event::Refused(Refusal::NoFlares)]));
            }
        }
        assert_eq!(dispensed, FLARES_MAX as usize);
        assert_eq!(w.flares_left, 0);
        // With the dispenser empty, pressing again changes nothing.
        let before = w.flares.len();
        assert!(matches!(w.dispense(&plane).as_slice(), [Event::Refused(Refusal::NoFlares)]));
        assert_eq!(w.flares.len(), before);
    }

    #[test]
    fn decoy_chance_is_bounded_and_falls_with_range() {
        let mut last = decoy_chance(0.0, 1.0);
        assert!(last > 0.0 && last <= 1.0);
        for k in 0..=300 {
            let range = k as f64 * 10.0;
            let chance = decoy_chance(range, 1.0);
            assert!((0.0..=1.0).contains(&chance), "range {range}: {chance}");
            assert!(chance <= last + 1e-12, "rises with range at {range}");
            last = chance;
        }
        assert_eq!(decoy_chance(FLARE_REACH, 1.0), 0.0);
        assert_eq!(decoy_chance(FLARE_REACH + 10_000.0, 1.0), 0.0);
        // The whole aspect envelope, and every range, stays in 0..1.
        for a in 0..=180 {
            let behind = (a as f64).to_radians().cos().max(0.0);
            for range in [0.0, 100.0, 800.0, 1_600.0, 2_400.0, 4_000.0] {
                let chance = decoy_chance(range, behind);
                assert!((0.0..=1.0).contains(&chance), "aspect {a}, range {range}: {chance}");
                assert!(chance <= decoy_chance(range, 1.0) + 1e-12);
            }
        }
    }

    #[test]
    fn a_flare_behind_the_target_can_seduce_a_seeker() {
        let (plane, _) = hornet_with_missiles();
        let mut w = Weapons::new(&crate::aircraft::fa18::SPEC, &[]);
        let target = Target {
            key: TargetKey::Bandit(1),
            pos: plane.pos + plane.forward() * 2_000.0,
            vel: plane.forward() * 200.0,
            size: 1,
            label: "BANDIT".into(),
        };
        let missile = Missile {
            station: 0,
            pos: target.pos - target.vel.normalize() * 1_000.0,
            vel: target.vel,
            rot: DQuat::IDENTITY,
            age: 2.0,
            burn: 0.0,
            target: Some(target.key.clone()),
            decoy: None,
            alive: true,
        };
        let flare = |behind: bool| Flare {
            id: 7,
            pos: if behind { target.pos - target.vel.normalize() * 50.0 } else { target.pos + target.vel.normalize() * 50.0 },
            vel: target.vel * 0.8,
            age: 0.0,
            life: FLARE_LIFE,
        };
        // Ahead of the target's nose there is no exhaust wake to follow.
        w.missiles.push(missile.clone());
        w.flares.push(flare(false));
        for _ in 0..2_000 {
            w.seduce(std::slice::from_ref(&target));
        }
        assert!(w.missiles[0].decoy.is_none(), "a flare ahead must not seduce");
        // Dead astern, close to the tail: over a few seconds the dice fall its way.
        w.missiles[0].target = Some(target.key.clone());
        w.flares[0] = flare(true);
        for _ in 0..5_000 {
            w.seduce(std::slice::from_ref(&target));
            if w.missiles[0].decoy.is_some() {
                break;
            }
        }
        assert_eq!(w.missiles[0].decoy, Some(7));
        assert!(w.missiles[0].target.is_none(), "a seduced missile drops its aircraft target");
    }

    #[test]
    fn a_big_aircraft_needs_a_bigger_fuse() {
        assert!(target_radius(4) > target_radius(0));
        assert!(blast_power(4) > blast_power(0));
    }

    #[test]
    fn the_hornet_has_six_stations() {
        let w = Weapons::new(&crate::aircraft::fa18::SPEC, &[]);
        assert_eq!(w.stations.len(), 6);
        assert!(w.has_weapons());
        assert!(w.stations.iter().all(|s| s.loaded));
    }

    #[test]
    fn a_fresh_aircraft_needs_no_arming_step() {
        let (mut plane, pieces) = hornet_with_missiles();
        let mut w = Weapons::new(&crate::aircraft::fa18::SPEC, &pieces);
        assert!(w.armed, "weapons should be live on construction");
        let target = Target {
            key: TargetKey::Live("TEST123".into()),
            pos: plane.pos + plane.forward() * 5_000.0,
            vel: plane.vel,
            size: 2,
            label: "TEST123".into(),
        };
        assert!(w.cycle_target(&plane, std::slice::from_ref(&target)));
        w.lock = 1.0;
        assert!(matches!(
            w.try_fire(&mut plane, true).as_slice(),
            [Event::Fired { station: 0 }]
        ));
        // A reset gives the next flight the same live weapons.
        w.armed = false;
        w.reset(&crate::aircraft::fa18::SPEC, &pieces);
        assert!(w.armed && w.stations.iter().all(|s| s.loaded));
    }

    #[test]
    fn a_bandit_can_be_locked_without_disturbing_the_other_targets() {
        let (mut plane, pieces) = hornet_with_missiles();
        let mut w = Weapons::new(&crate::aircraft::fa18::SPEC, &pieces);
        let targets = [
            Target {
                key: TargetKey::Live("LIVE01".into()),
                pos: plane.pos + plane.forward() * 6_000.0 + plane.right() * 3_000.0,
                vel: plane.vel,
                size: 1,
                label: "LIVE01".into(),
            },
            Target {
                key: TargetKey::Bandit(4),
                pos: plane.pos + plane.forward() * 4_000.0,
                vel: plane.vel,
                size: 1,
                label: "BANDIT 1".into(),
            },
        ];
        // The bandit is nearer the nose, so the first cycle picks it.
        assert!(w.cycle_target(&plane, &targets));
        assert_eq!(w.target, Some(TargetKey::Bandit(4)));
        w.lock = 1.0;
        assert!(matches!(w.try_fire(&mut plane, true).as_slice(), [Event::Fired { station: 0 }]));
        // The live target is still there and lockable.
        assert!(w.cycle_target(&plane, &targets));
        assert_eq!(w.target, Some(TargetKey::Live("LIVE01".into())));
    }

    /// A world with no terrain, as the AI and bandit tests use.
    fn empty_world() -> crate::world::World {
        let dir = std::env::temp_dir().join("fablegame-weapon-tests");
        let src = std::sync::Arc::new(crate::sources::Sources::new(crate::fetch::Fetcher::new(dir, true), String::new()));
        let mut w = crate::world::World::new(crate::sources::Loader::new(src, 0), 14);
        w.flat = Some(0.0);
        w
    }

    #[test]
    fn dogfight_mode_leaves_live_and_airport_traffic_unshootable() {
        let (mut plane, pieces) = hornet_with_missiles();
        let mut w = Weapons::new(&crate::aircraft::fa18::SPEC, &pieces);
        let world = empty_world();
        let targets = [
            Target {
                key: TargetKey::Live("LIVE01".into()),
                pos: plane.pos + plane.forward() * 4_000.0,
                vel: plane.vel,
                size: 2,
                label: "LIVE01".into(),
            },
            Target {
                key: TargetKey::Airport { apt: 1, mover: 7 },
                pos: plane.pos + plane.forward() * 3_000.0,
                vel: DVec3::ZERO,
                size: 2,
                label: "TRAFFIC".into(),
            },
            Target {
                key: TargetKey::Bandit(0),
                pos: plane.pos + plane.forward() * 5_000.0,
                vel: plane.vel,
                size: 1,
                label: "BANDIT 1".into(),
            },
        ];
        w.dogfight = true;
        assert!(w.shootable(&targets[2].key), "a bandit is shootable in dogfight mode");
        assert!(!w.shootable(&targets[0].key), "live traffic is shootable in dogfight mode");
        assert!(!w.shootable(&targets[1].key), "airport traffic is shootable in dogfight mode");
        // Even with both kinds of traffic nearer the nose, the seeker can only
        // pick the bandit.
        w.request_cycle();
        w.step(1.0 / 180.0, &mut plane, &targets, &world, true);
        assert_eq!(w.target, Some(TargetKey::Bandit(0)), "a traffic target was locked");
        // With no bandit on the list there is nothing to lock at all.
        w.clear_lock();
        w.request_cycle();
        let events = w.step(1.0 / 180.0, &mut plane, &targets[..2], &world, true);
        assert!(w.target.is_none(), "traffic was locked in dogfight mode");
        assert_eq!(w.lock, 0.0);
        assert!(matches!(events.as_slice(), [Event::Refused(Refusal::NoTarget)]));
        // Out of dogfight mode the same list finds the traffic again.
        w.dogfight = false;
        w.request_cycle();
        w.step(1.0 / 180.0, &mut plane, &targets, &world, true);
        assert_eq!(w.target, Some(TargetKey::Airport { apt: 1, mover: 7 }));
    }
}
