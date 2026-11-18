//! The bandits' pilots.
//!
//! A bandit flies the same `Plane` the player does: the AI only moves the
//! stick, the rudder and the throttle through `plane::Controls`, so it trades
//! energy, stalls and overshoots with its type's real behaviour. It sees the
//! player through a simulated radar (a forward cone and a range), through a
//! missile that is already tracking, or through a missile-approach warning
//! when a player's missile is closing on it. It never asks the wing for more
//! angle of attack than it can give.
//!
//! The pilot fights with the whole aircraft: it presses for guns inside the
//! cannon envelope, takes heat-seeking missile shots when the seeker geometry
//! is good, breaks and dispenses flares when a missile is behind it, and
//! extends to rebuild energy instead of staggering through a turn. The missile
//! and flare bodies themselves live in `weapons.rs`; this module only decides
//! when to use them.
use crate::crash::Rng;
use crate::geo;
use crate::plane::{self, Plane};
use crate::weapons;
use glam::DVec3;

/// The radar's reach (m) and half-angle off the nose (rad): the player is
/// visible inside both, or while one of the bandit's missiles is tracking.
const RADAR_RANGE: f64 = 12_000.0;
const RADAR_CONE: f64 = 60.0f64.to_radians();
/// The range and angle off the nose at which the pilot commits to an attack.
const ATTACK_RANGE: f64 = 3_000.0;
const ATTACK_CONE: f64 = 30.0f64.to_radians();
/// The cannon's gate: the range it is worth firing at (m), and the angle off
/// the nose the target must be inside (rad). Closer than the minimum the
/// target fills the sight and the burst is behind it before it arrives.
const GUN_MIN: f64 = 200.0;
const GUN_MAX: f64 = 1_200.0;
const GUN_CONE: f64 = 5.0f64.to_radians();
/// Inside this range, with the player in front of it and pointing at the
/// bandit, the pilot breaks instead of pressing on. A player closer than the
/// hard gate is treated as a threat even before its nose comes round.
const THREAT_RANGE: f64 = 3_500.0;
const THREAT_CONE: f64 = 30.0f64.to_radians();
const HARD_THREAT_RANGE: f64 = 1_500.0;
/// A missile this close is detected by the approach warning and reacted to;
/// inside the evade range the pilot stops attacking and defends.
const MISSILE_SENSE_RANGE: f64 = 4_500.0;
const MISSILE_EVADE_RANGE: f64 = 4_000.0;
/// Flares are worth burning only inside this range, and only for a seeker
/// approaching from behind where the flare will sit in the exhaust wake.
const FLARE_RANGE: f64 = 2_800.0;
/// The missile envelope the pilot will shoot inside: the seeker's own range
/// (from `weapons::lock_range`) is narrowed by skill and capped for the game's
/// sight picture. Inside the minimum the warhead would not arm.
const MISSILE_MIN_RANGE: f64 = 700.0;
const MISSILE_MAX_RANGE: f64 = 6_000.0;
const MISSILE_CONE: f64 = 35.0f64.to_radians();
/// The line-of-sight rate above which the target is crossing too fast for a
/// worthwhile shot (rad/s).
const MISSILE_RATE_MAX: f64 = 0.35;
/// Specific energy is kinetic energy height, `v² / 2g`. Below the energy height
/// of `1.25 Vs` the pilot must extend; above `1.45 Vs` it may fight again.
const ENERGY_FLOOR: f64 = 1.25;
const ENERGY_EXIT: f64 = 1.45;
const G0: f64 = 9.80665;

/// What the pilot is doing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AiState {
    /// Nothing on the radar: patrol and sweep, or fly to where the player was.
    Search,
    /// The player is on the radar but outside attack range.
    Pursue,
    /// Close and in front: track and press the attack.
    Attack,
    /// The player is behind and pointing this way: break.
    Defend,
    /// A missile is closing on the aircraft: beam it, burn and flare.
    Evade,
    /// Too slow to fight: dive away and rebuild energy.
    Extend,
}

/// An incoming missile as the pilot's approach warning sees it: where it is,
/// which way it is going and how far off. The AI does not know who fired it,
/// only that it is coming.
#[derive(Clone, Copy, Debug)]
pub struct Threat {
    pub pos: DVec3,
    pub vel: DVec3,
    pub range: f64,
}

/// What the pilot knows of the player: where it is and how it is moving. No
/// other knowledge of the player is available to the AI.
#[derive(Clone, Copy, Debug)]
pub struct TargetView {
    pub pos: DVec3,
    pub vel: DVec3,
    /// range from the bandit (m)
    pub range: f64,
    /// angle off the bandit's nose (rad)
    pub angle: f64,
}

/// How good the pilot is.
#[derive(Clone, Copy, Debug)]
pub struct Skill {
    /// Aim error: the RMS angular offset (rad) between its tracking solution
    /// and the perfect lead point. An average pilot holds about two-thirds of
    /// a degree; a good one a quarter, a poor one over two.
    pub aim_error: f64,
    /// Reaction delay (s): how long a new sighting, or a change from pursuing
    /// to attacking, takes to alter what the pilot is doing.
    pub reaction: f64,
    /// Energy aggressiveness (0 cautious .. 1 aggressive): how willingly it
    /// uses the afterburner and how hard it is prepared to spend speed in a
    /// turn. The average pilot sits at 0.5.
    pub energy_use: f64,
    /// Missile aggressiveness (0 waits for a sure thing .. 1 takes the first
    /// useful envelope): how much of the seeker's range it uses and how
    /// quickly it follows one shot with another.
    pub missile_use: f64,
}

impl Skill {
    /// The default opponent: a competent pilot. It reacts in about a third of
    /// a second, aims within a degree, spends energy when the fight asks for
    /// it and will take a missile shot when the seeker geometry is sound.
    pub fn average() -> Self {
        Self { aim_error: 0.010, reaction: 0.35, energy_use: 0.58, missile_use: 0.65 }
    }
}

impl Default for Skill {
    fn default() -> Self {
        Self::average()
    }
}

/// One AI pilot. It owns only the decision-making; the aircraft is a `Plane`.
pub struct Pilot {
    pub skill: Skill,
    /// The player was inside the sensor cone this step.
    pub target_seen: bool,
    pub state: AiState,
    /// What it saw this step (where the player is and how it is moving), for
    /// the trigger: the gun's gate is judged on the same sighting the stick
    /// follows.
    view: Option<TargetView>,
    /// Where the player was last seen, for a search towards the last contact.
    last_seen: Option<DVec3>,
    /// Seconds left before the pilot reacts to what it has just seen.
    reaction_timer: f64,
    /// Where it is aiming away from the perfect lead solution (world frame, m),
    /// and the time until that offset is drawn again.
    aim: DVec3,
    aim_timer: f64,
    /// Seconds flown, for the search weave.
    t: f64,
    /// Below the energy floor: extend until above its exit.
    extending: bool,
    /// The missile this step, when one is closing.
    threat: Option<Threat>,
    /// Seconds before another missile may be shot, and before the dispenser
    /// may fire again. The initial missile delay staggers a formation so a
    /// wave does not salvo on one frame.
    missile_cd: f64,
    flare_cd: f64,
    rng: Rng,
}

impl Pilot {
    pub fn new(skill: Skill, seed: u64) -> Self {
        let mut rng = Rng(seed | 1);
        let first_shot = 2.0 + rng.range(0.0, 4.0) as f64;
        Self {
            skill,
            target_seen: false,
            state: AiState::Search,
            view: None,
            last_seen: None,
            reaction_timer: 0.0,
            aim: DVec3::ZERO,
            aim_timer: 0.0,
            t: 0.0,
            extending: false,
            threat: None,
            missile_cd: first_shot,
            flare_cd: 0.0,
            rng,
        }
    }

    /// Look for the player. `tracking` is true while one of the bandit's own
    /// missiles holds the player: the seeker keeps the contact even when the
    /// player leaves the radar cone.
    pub fn sense(&mut self, me: &Plane, player: &Plane, tracking: bool) -> Option<TargetView> {
        let d = player.pos - me.pos;
        let range = d.length();
        let angle = if range > 1.0e-3 { me.forward().angle_between(d / range) } else { 0.0 };
        let seen = tracking || (range <= RADAR_RANGE && angle <= RADAR_CONE);
        if seen {
            if !self.target_seen {
                self.reaction_timer = self.skill.reaction;
            }
            self.target_seen = true;
            self.last_seen = Some(player.pos);
            Some(TargetView { pos: player.pos, vel: player.vel, range, angle })
        } else {
            self.target_seen = false;
            None
        }
    }

    /// The stick, rudder and throttle for this step. `threat` is the nearest
    /// missile the approach warning has found closing on this aircraft.
    pub fn controls(&mut self, me: &Plane, seen: Option<TargetView>, threat: Option<Threat>, h: f64) -> plane::Controls {
        self.t += h;
        self.view = seen;
        self.reaction_timer = (self.reaction_timer - h).max(0.0);
        self.aim_timer -= h;
        self.missile_cd = (self.missile_cd - h).max(0.0);
        self.flare_cd = (self.flare_cd - h).max(0.0);
        self.threat = threat.filter(|t| t.range <= MISSILE_SENSE_RANGE && incoming(me, t));

        // The energy rule: specific energy below the floor is a mandatory
        // extension, with hysteresis so it does not flicker on the boundary.
        let v = me.airspeed().length();
        let vs = stall_speed(me);
        let energy = v * v / (2.0 * G0);
        let floor = (ENERGY_FLOOR * vs).powi(2) / (2.0 * G0);
        let exit = (ENERGY_EXIT * vs).powi(2) / (2.0 * G0);
        if energy < floor {
            self.extending = true;
        } else if energy > exit {
            self.extending = false;
        }

        // A missile is the most urgent thing in the sky: it beats the energy
        // rule and the reaction delay. The pilot only has seconds.
        if self.threat.is_some_and(|t| t.range <= MISSILE_EVADE_RANGE) {
            self.state = AiState::Evade;
            self.reaction_timer = self.reaction_timer.min(h);
        } else if self.extending {
            self.state = AiState::Extend;
        } else if self.reaction_timer > 0.0 {
            // Still reacting: keep doing whatever it was doing.
        } else if let Some(t) = seen {
            let los = (t.pos - me.pos).normalize_or_zero();
            let behind = me.forward().dot(los) < 0.25;
            let incoming = t.vel.normalize_or_zero().dot((me.pos - t.pos).normalize_or_zero());
            let pointed = incoming > THREAT_CONE.cos();
            self.state = if behind && t.range < THREAT_RANGE && (pointed || t.range < HARD_THREAT_RANGE) {
                AiState::Defend
            } else if t.range <= ATTACK_RANGE && t.angle <= ATTACK_CONE {
                AiState::Attack
            } else {
                AiState::Pursue
            };
        } else {
            self.state = AiState::Search;
        }

        let up = me.pos.normalize();
        let c = match self.state {
            AiState::Search => self.search(me, up),
            AiState::Pursue => self.pursue(me, seen, false),
            AiState::Attack => self.pursue(me, seen, true),
            AiState::Defend => self.defend(me, seen, up),
            AiState::Evade => self.evade(me, self.threat, up),
            AiState::Extend => self.extend(me, seen, up),
        };
        limit_pitch(me, c)
    }

    /// Is a missile shot on this step? Called by `Bandits` after `controls`,
    /// with the same sighting and the pilot's remaining store count. A good
    /// shot needs the target inside the seeker's aspect-dependent range, near
    /// enough to the nose that the seeker can hold it, a line-of-sight rate
    /// the missile can follow, and no missile already coming the other way.
    pub fn missile_shot(&mut self, me: &Plane, seen: Option<TargetView>, have: u32) -> bool {
        if have == 0 || self.missile_cd > 0.0 || self.threat.is_some() {
            return false;
        }
        if !matches!(self.state, AiState::Pursue | AiState::Attack) {
            return false;
        }
        let Some(t) = seen else { return false };
        if t.range < MISSILE_MIN_RANGE || t.angle > MISSILE_CONE {
            return false;
        }
        // Seeker range grows towards the target's tail, as the live missile's
        // does. A cautious pilot only takes the nearer part of the envelope.
        // A target with no velocity counts as abeam, as it does in the seeker.
        let to_shooter = (me.pos - t.pos).normalize_or_zero();
        let aspect = if t.vel.length_squared() < 1.0 {
            std::f64::consts::FRAC_PI_2
        } else {
            t.vel.normalize().angle_between(to_shooter)
        };
        let usable = weapons::Weapons::lock_range(1, aspect) * (0.50 + 0.35 * self.skill.missile_use);
        if t.range > usable.min(MISSILE_MAX_RANGE) {
            return false;
        }
        // How fast the line of sight is swinging: a beam shot at a hard
        // crossing target is a missile wasted.
        let los = (t.pos - me.pos).normalize_or_zero();
        let rel = t.vel - me.vel;
        let rate = (rel - los * rel.dot(los)).length() / t.range.max(1.0);
        if rate > MISSILE_RATE_MAX {
            return false;
        }
        // Do not fire from below the energy floor: the aircraft cannot support
        // the shot while it is staggering.
        if me.airspeed().length() < 1.2 * stall_speed(me) {
            return false;
        }
        // A short pause between shots, and a longer one for a cautious pilot.
        self.missile_cd = 5.5 + 4.0 * (1.0 - self.skill.missile_use) + self.rng.range(0.0, 1.5) as f64;
        true
    }

    /// Should the dispenser fire this step? Only a seeker coming up behind,
    /// inside flare range and still closing, can be led into the exhaust wake.
    pub fn flare_ready(&mut self, me: &Plane, threat: Option<Threat>) -> bool {
        if self.flare_cd > 0.0 {
            return false;
        }
        let Some(t) = threat else { return false };
        let los = (t.pos - me.pos).normalize_or_zero();
        if t.range > FLARE_RANGE || t.range < 120.0 || closing_speed(me, &t) < 20.0 {
            return false;
        }
        // Only a missile behind the wing can be decoyed by a flare dropped
        // into the aircraft's own wake.
        if los.dot(me.forward()) > 0.15 {
            return false;
        }
        self.flare_cd = 0.55;
        true
    }

    /// Is the cannon's trigger pulled? Only in the attack, inside the gun's
    /// range gate, with the target near the nose. The nose already carries the
    /// pilot's aim error (see `pursue`), and `guns` adds it to the burst's
    /// dispersion, so no burst is the perfect lead.
    pub fn trigger(&self) -> bool {
        let Some(t) = self.view else { return false };
        self.state == AiState::Attack && (GUN_MIN..=GUN_MAX).contains(&t.range) && t.angle <= GUN_CONE
    }

    /// Patrol, or fly towards where the player was last seen.
    fn search(&mut self, me: &Plane, up: DVec3) -> plane::Controls {
        // A target already on the radar is being reacted to: keep the patrol
        // until the reaction delay has run out.
        if !self.target_seen {
            if let Some(p) = self.last_seen {
                let d = p - me.pos;
                if d.length() > 600.0 {
                    return plane::Controls { throttle: 0.9, ..steer(me, d) };
                }
                // At the last contact: fly a circle round it, looking around,
                // until the player comes back into the radar cone. The tangent
                // (rather than the bearing) keeps the turn going the same way.
                let tangent = up.cross(d).normalize_or_zero();
                return plane::Controls { throttle: 0.85, ..steer(me, tangent + d.normalize_or_zero() * 0.2 + up * 0.05) };
            }
        }
        // Hold the height, wings level, and weave a little so the radar sweeps.
        let vs = me.vel.dot(up);
        let level = me.right().dot(up);
        plane::Controls {
            pitch: (-vs * 0.02 - me.omega.x * 0.3).clamp(-0.5, 0.5),
            roll: (level * 2.5 + (self.t * 0.25).sin() * 0.2 + me.omega.z * 0.5).clamp(-0.5, 0.5),
            throttle: 0.8,
            ..Default::default()
        }
    }

    /// Point at the lead point (attack) or at the player (pursuit).
    fn pursue(&mut self, me: &Plane, seen: Option<TargetView>, attack: bool) -> plane::Controls {
        let Some(t) = seen else {
            return self.search(me, me.pos.normalize());
        };
        let up = me.pos.normalize();
        // A head-on pass is not a collision course: offset the aim above the
        // target so the two aircraft cross with height between them, as real
        // fighters do. Inside the last few hundred metres, pull up rather than
        // flying through the target's canopy.
        let head_on = t.vel.length_squared() > 1.0 && t.vel.normalize().dot(me.forward()) < -0.5;
        if t.range < 500.0 && head_on {
            let mut c = steer(me, me.forward() + up * 0.55);
            c.throttle = 1.0;
            return c;
        }
        // A tail chase that has closed to a few hundred metres is a collision
        // risk too: pull up rather than flying through the target.
        if t.range < 250.0 {
            let mut c = steer(me, me.forward() + up * 0.35);
            c.throttle = 1.0;
            return c;
        }
        // Too close to track: fly through and let the geometry open again
        // (a late turn would only overshoot and bleed energy).
        if t.range < 400.0 {
            return plane::Controls { throttle: 1.0, ..Default::default() };
        }
        if self.aim_timer <= 0.0 {
            // A fresh aim error, square to the line of sight and scaled by
            // the range, so it is a constant angular error rather than a
            // constant miss distance.
            let los = (t.pos - me.pos).normalize_or_zero();
            let mut d = self.rng.dir().as_dvec3();
            d -= los * d.dot(los);
            self.aim = d.normalize_or_zero() * (t.range * self.skill.aim_error);
            self.aim_timer = 0.8;
        }
        let los = (t.pos - me.pos).normalize_or_zero();
        let closing = (me.airspeed().length() - t.vel.dot(los)).max(80.0);
        let t_go = (t.range / closing).clamp(0.0, 8.0);
        let mut aim = t.pos + t.vel * t_go + self.aim;
        if t.range < 1_200.0 && head_on {
            // Enough vertical separation to clear a fast closure, tapering to
            // a definite pull-up at the last moment.
            aim += up * (t.range * 0.22).clamp(60.0, 280.0);
        }
        let mut c = steer(me, aim - me.pos);
        if !attack {
            // A pursuit follows at a sustainable load; the attack presses.
            c.pitch *= 0.85;
        }
        c.throttle = 1.0;
        // A chasing aircraft uses the burner; an aircraft with the target in
        // its sights saves it for the turn in which the shot will be taken.
        c.boost = self.skill.energy_use > 0.35
            && ((attack && t.range > 2_200.0) || (!attack && t.range > 1_200.0));
        c
    }

    /// Break across the player's nose, to make the shot as hard as possible.
    fn defend(&mut self, me: &Plane, seen: Option<TargetView>, up: DVec3) -> plane::Controls {
        let Some(t) = seen else {
            return self.search(me, up);
        };
        let away = (me.pos - t.pos).normalize_or_zero();
        let mut side = away.cross(up).normalize_or_zero();
        if side.length_squared() < 0.1 {
            side = me.right();
        }
        // Break the way the nose already points, so the turn gets going, and
        // change the vertical component as the fight goes on so the break is
        // not a predictable circle.
        let side = if me.forward().dot(side) < me.forward().dot(-side) { -side } else { side };
        let jink = 0.10 + 0.10 * (self.t * 1.7).sin();
        let mut c = steer(me, side + up * jink);
        c.throttle = 1.0;
        c.boost = self.skill.energy_use > 0.4 || t.range < 2_000.0;
        c
    }

    /// Put the missile on the beam: turn across its line of sight, pull hard
    /// and keep the speed up. Flares go out separately (`flare_ready`).
    fn evade(&mut self, me: &Plane, threat: Option<Threat>, up: DVec3) -> plane::Controls {
        let Some(t) = threat else {
            return self.search(me, up);
        };
        let los = (t.pos - me.pos).normalize_or_zero();
        // The beam is perpendicular to the missile's line of sight. Turn the
        // way the nose is already going, so the break starts immediately.
        let mut beam = los.cross(up).normalize_or_zero();
        if beam.length_squared() < 0.1 {
            beam = me.right();
        }
        if me.forward().dot(beam) < 0.0 {
            beam = -beam;
        }
        // A hard, slightly descending break keeps the speed up and makes the
        // missile's lead solution change. Very close in, level the break so
        // the wing can give everything to the turn.
        let dive = if t.range < 800.0 { 0.04 } else { 0.16 };
        let mut c = steer(me, beam - up * dive);
        c.throttle = 1.0;
        c.boost = true;
        c
    }

    /// Run away downhill, trading height for speed until above the exit energy.
    fn extend(&mut self, me: &Plane, seen: Option<TargetView>, up: DVec3) -> plane::Controls {
        let away = match seen {
            Some(t) => (me.pos - t.pos).normalize_or_zero(),
            None => me.forward(),
        };
        let horiz = (away - up * away.dot(up)).normalize_or_zero();
        let mut c = steer(me, horiz - up * 0.12);
        c.throttle = 1.0;
        c.boost = true;
        c
    }
}

/// How fast the missile is closing (m/s): positive when the range is shrinking.
fn closing_speed(me: &Plane, t: &Threat) -> f64 {
    let los = (t.pos - me.pos).normalize_or_zero();
    (me.vel - t.vel).dot(los)
}

/// Is the missile closing on the aircraft? Distance must be shrinking along
/// the line between them, not merely a missile flying past.
fn incoming(me: &Plane, t: &Threat) -> bool {
    closing_speed(me, t) > 10.0
}

/// Point the aircraft at `point` and hold full dry power: the turn back to a
/// combat zone's centre. The pilot's normal state is overridden for as long as
/// the boundary needs it, so the fight cannot draw a bandit out of its arena.
pub fn return_to(me: &Plane, point: DVec3) -> plane::Controls {
    let mut c = steer(me, point - me.pos);
    c.throttle = 1.0;
    c.boost = false;
    limit_pitch(me, c)
}

/// Bank towards the target's side of the nose, then pull it into the vertical
/// plane where the wing's lift can turn the flight path. A target behind the
/// tail rolls hard into the shorter turn.
fn steer(me: &Plane, desired: DVec3) -> plane::Controls {
    let w_up = me.pos.normalize();
    let d = desired.normalize_or_zero();
    if d.length_squared() < 0.5 {
        return plane::Controls::default();
    }
    let local = me.rot.inverse() * d;
    let ias = me.ias().max(40.0);
    // The bank the target asks for: a target 30 degrees off the nose wants
    // about 45 degrees of bank. Roll towards it against the actual bank.
    let az = local.x.atan2(-local.z);
    let bank_want = (az * 2.0).clamp(-1.2, 1.2);
    let bank = (-me.right().dot(w_up)).atan2(me.up().dot(w_up));
    let mut roll = ((bank_want - bank) * 1.0 + me.omega.z * 1.2).clamp(-1.0, 1.0);
    // The same stick pulls far more G at 250 m/s than at 100: go gently at
    // speed, and let `limit_pitch` cap the load as well as the stall angle.
    let slow = (160.0 / ias).powi(2).min(1.0).max(0.35);
    let mut pitch = ((local.y * 6.0 - me.omega.x * 0.35) * slow).clamp(-1.0, 1.0);
    if local.z > 0.0 && local.x.abs() < 0.15 {
        // Behind the tail: roll hard into a turn and pull.
        roll = if me.up().dot(w_up) >= 0.0 { 1.0 } else { -1.0 };
        pitch = (0.7 * slow).max(0.3);
    }
    plane::Controls { pitch, roll, ..Default::default() }
}

/// The speed at which the wing stalls at this weight and height (m/s TAS).
fn stall_speed(me: &Plane) -> f64 {
    let (_, _, h) = geo::pos_to_lla(me.pos);
    let rho = plane::density(h).max(0.01);
    let cl = (me.spec.cl0 + me.spec.cl_a * me.stall_alpha()).max(0.2);
    (2.0 * me.mass() * G0 / (rho * me.spec.wing_area * cl)).sqrt()
}

/// The most pitch the wing can take: solve `Plane::step`'s stick-to-angle
/// mapping with the type's stall angle as the ceiling, and no more load than
/// the airframe is cleared for, including the CG's effect on how much angle of
/// attack a stick deflection buys.
fn limit_pitch(me: &Plane, mut c: plane::Controls) -> plane::Controls {
    let s = me.spec;
    let shift = me.cg() - s.cg[0];
    let margin = 0.15 - shift;
    let stiff = (0.4 + 0.6 * margin / 0.15).clamp(0.15, 1.8);
    let auth = (1.0 + 1.5 * shift).clamp(0.5, 1.4);
    let sens = (1.0 / stiff).min(2.5);
    let per_stick = s.handling.alpha_ctl * auth * sens;
    let (_, _, h) = geo::pos_to_lla(me.pos);
    let v = me.airspeed().length();
    let q = 0.5 * plane::density(h) * v * v;
    let n = (s.g_limit as f64 * 0.85).max(1.5);
    let alpha_g = (n * me.mass() * G0 / (q * s.wing_area).max(1.0) - s.cl0) / s.cl_a;
    let limit = (me.stall_alpha() * 0.95).min(alpha_g).max(me.trim);
    let max = ((limit - me.trim) / per_stick.max(1.0e-6)).clamp(-1.0, 1.0);
    c.pitch = c.pitch.min(max);
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aircraft::fa18;

    fn empty_world() -> crate::world::World {
        let dir = std::env::temp_dir().join("fablegame-ai-tests");
        let src = std::sync::Arc::new(crate::sources::Sources::new(crate::fetch::Fetcher::new(dir, true), String::new()));
        let mut w = crate::world::World::new(crate::sources::Loader::new(src, 0), 14);
        w.flat = Some(0.0);
        w
    }

    #[test]
    fn a_player_outside_the_radar_cone_is_not_seen() {
        let spec = &fa18::SPEC;
        let me = Plane::spawn(spec, 0.0, 0.0, 3_000.0, 0.0, spec.cruise);
        // Behind the bandit (its nose is -Z, so the player is at +Z).
        let (lat, lon, alt) = geo::pos_to_lla(me.pos - me.forward() * 3_000.0);
        let player = Plane::spawn(spec, lat, lon, alt, 0.0, spec.cruise);
        let mut pilot = Pilot::new(Skill::average(), 7);
        let seen = pilot.sense(&me, &player, false);
        assert!(seen.is_none(), "the player behind the tail is not on the radar");
        assert!(!pilot.target_seen);
        pilot.controls(&me, seen, None, 1.0 / 180.0);
        assert_eq!(pilot.state, AiState::Search);
    }

    #[test]
    fn commanded_angle_of_attack_never_exceeds_the_stall_angle() {
        for spec in [&fa18::SPEC, &crate::aircraft::f4::SPEC, &crate::aircraft::f22::SPEC] {
            let mut me = Plane::spawn(spec, 0.0, 0.0, 5_000.0, 0.0, spec.cruise);
            let world = empty_world();
            let mut pilot = Pilot::new(Skill::average(), 11);
            let h = 1.0 / 180.0;
            // A target close and above, so the pilot pulls as hard as it can.
            let up = me.pos.normalize();
            let target = TargetView {
                pos: me.pos + me.forward() * 800.0 + up * 800.0,
                vel: DVec3::ZERO,
                range: 1_131.0,
                angle: 45.0f64.to_radians(),
            };
            for step in 0..1_000 {
                let c = pilot.controls(&me, Some(target), None, h);
                let hd = &spec.handling;
                // The same mapping Plane::step uses for the commanded alpha.
                let shift = me.cg() - spec.cg[0];
                let margin = 0.15 - shift;
                let stiff = (0.4 + 0.6 * margin / 0.15).clamp(0.15, 1.8);
                let auth = (1.0 + 1.5 * shift).clamp(0.5, 1.4);
                let sens = (1.0 / stiff).min(2.5);
                let alpha_cmd = me.trim + c.pitch * hd.alpha_ctl * auth * sens;
                assert!(alpha_cmd <= spec.stall_alpha + 1.0e-9, "{} step {step}: commanded {alpha_cmd:.4} rad, stall {:.4} rad", spec.name, spec.stall_alpha);
                me.step(h, &c, &world);
            }
        }
    }

    #[test]
    fn a_slow_aircraft_extend_is_mandatory() {
        let spec = &fa18::SPEC;
        let mut me = Plane::spawn(spec, 0.0, 0.0, 6_000.0, 0.0, 80.0);
        me.throttle = 0.0;
        let world = empty_world();
        let mut pilot = Pilot::new(Skill::average(), 3);
        let h = 1.0 / 180.0;
        for _ in 0..30 {
            let c = pilot.controls(&me, None, None, h);
            me.step(h, &c, &world);
        }
        assert_eq!(pilot.state, AiState::Extend, "below the energy floor the pilot must extend");
    }

    #[test]
    fn a_bandit_does_not_fire_outside_its_range_gate() {
        let spec = &fa18::SPEC;
        let me = Plane::spawn(spec, 0.0, 0.0, 3_000.0, 0.0, spec.cruise);
        // In the attack, looking at a target `range` ahead (`angle` off the nose).
        let ready = |range: f64, angle: f64| {
            let mut p = Pilot::new(Skill::average(), 9);
            p.state = AiState::Attack;
            p.view = Some(TargetView { pos: me.pos + me.forward() * range, vel: me.vel, range, angle });
            p
        };
        assert!(ready(600.0, 0.0).trigger(), "600 m dead ahead is a shot");
        assert!(!ready(3_000.0, 0.0).trigger(), "3 km is outside the gun's range");
        assert!(!ready(100.0, 0.0).trigger(), "100 m is inside the minimum range");
        assert!(!ready(600.0, 20f64.to_radians()).trigger(), "20 degrees off the nose is not a shot");
        // Out of the attack, it holds fire even with the sight on.
        let mut p = ready(600.0, 0.0);
        p.state = AiState::Pursue;
        assert!(!p.trigger());
        // And with no sighting at all.
        let mut p = Pilot::new(Skill::average(), 9);
        p.state = AiState::Attack;
        assert!(!p.trigger());
    }

    #[test]
    fn a_visible_player_is_picked_up_after_the_reaction_delay() {
        let spec = &fa18::SPEC;
        let me = Plane::spawn(spec, 0.0, 0.0, 5_000.0, 0.0, spec.cruise);
        let (lat, lon, alt) = geo::pos_to_lla(me.pos + me.forward() * 5_000.0);
        let player = Plane::spawn(spec, lat, lon, alt, 0.0, spec.cruise);
        let mut pilot = Pilot::new(Skill { reaction: 0.4, ..Skill::average() }, 5);
        let h = 1.0 / 180.0;
        let view = pilot.sense(&me, &player, false);
        assert!(view.is_some());
        pilot.controls(&me, view, None, h);
        assert_eq!(pilot.state, AiState::Search, "it has not reacted yet");
        for _ in 0..(0.5 / h) as usize {
            let view = pilot.sense(&me, &player, false);
            pilot.controls(&me, view, None, h);
        }
        assert_eq!(pilot.state, AiState::Pursue);
    }

    #[test]
    fn a_missile_shot_needs_a_sensible_seeker_envelope() {
        let spec = &fa18::SPEC;
        let me = Plane::spawn(spec, 0.0, 0.0, 4_000.0, 0.0, spec.cruise);
        // A target two kilometres ahead, running the same way: a rear-aspect
        // shot in the middle of the envelope.
        let rear = TargetView { pos: me.pos + me.forward() * 2_000.0, vel: me.vel, range: 2_000.0, angle: 0.0 };
        let mut pilot = Pilot::new(Skill::average(), 21);
        pilot.state = AiState::Attack;
        pilot.missile_cd = 0.0;
        assert!(pilot.missile_shot(&me, Some(rear), 2), "a 2 km tail chase is a shot");
        assert!(!pilot.missile_shot(&me, Some(rear), 2), "the launcher must not salvo on the next step");
        // Well outside the cap, off the nose, or crossing beam-on: hold fire.
        for (range, angle, vel) in [
            (9_000.0, 0.0, me.vel),
            (2_000.0, 80.0f64.to_radians(), me.vel),
            (800.0, 0.0, me.right() * 400.0),
        ] {
            let t = TargetView { pos: me.pos + me.forward() * range, vel, range, angle };
            let mut pilot = Pilot::new(Skill::average(), 22);
            pilot.state = AiState::Attack;
            pilot.missile_cd = 0.0;
            assert!(!pilot.missile_shot(&me, Some(t), 2), "range {range}, angle {angle}: the shot was taken");
        }
        // With no store, or while a missile is already inbound: no shot.
        let mut pilot = Pilot::new(Skill::average(), 23);
        pilot.state = AiState::Attack;
        pilot.missile_cd = 0.0;
        assert!(!pilot.missile_shot(&me, Some(rear), 0));
        let incoming = Threat { pos: me.pos - me.forward() * 2_000.0, vel: me.forward() * 800.0, range: 2_000.0 };
        pilot = Pilot::new(Skill::average(), 24);
        pilot.state = AiState::Attack;
        pilot.missile_cd = 0.0;
        pilot.threat = Some(incoming);
        assert!(!pilot.missile_shot(&me, Some(rear), 2));
    }

    #[test]
    fn an_incoming_missile_makes_the_pilot_evade_and_flare() {
        let spec = &fa18::SPEC;
        let me = Plane::spawn(spec, 0.0, 0.0, 4_000.0, 0.0, spec.cruise);
        let behind = Threat { pos: me.pos - me.forward() * 2_000.0, vel: me.forward() * 800.0, range: 2_000.0 };
        let mut pilot = Pilot::new(Skill::average(), 31);
        pilot.controls(&me, None, Some(behind), 1.0 / 180.0);
        assert_eq!(pilot.state, AiState::Evade, "a closing missile must override the patrol");
        assert!(pilot.flare_ready(&me, Some(behind)), "a seeker behind must draw flares");
        assert!(!pilot.flare_ready(&me, Some(behind)), "the dispenser must not empty in one step");
        // A seeker in front cannot be led into the aircraft's exhaust wake.
        let ahead = Threat { pos: me.pos + me.forward() * 2_000.0, vel: -me.forward() * 800.0, range: 2_000.0 };
        let mut pilot = Pilot::new(Skill::average(), 32);
        assert!(!pilot.flare_ready(&me, Some(ahead)), "flares ahead of the nose are wasted");
        pilot.controls(&me, None, Some(ahead), 1.0 / 180.0);
        assert_eq!(pilot.state, AiState::Evade, "a head-on missile is still a threat");
        // A missile opening the range is not chased.
        let away = Threat { pos: me.pos - me.forward() * 2_000.0, vel: -me.forward() * 800.0, range: 2_000.0 };
        let mut pilot = Pilot::new(Skill::average(), 33);
        pilot.controls(&me, None, Some(away), 1.0 / 180.0);
        assert_ne!(pilot.state, AiState::Evade);
    }

    #[test]
    fn the_search_patrol_is_stable_and_does_not_stall() {
        let spec = &fa18::SPEC;
        let mut me = Plane::spawn(spec, 0.0, 0.0, 5_000.0, 0.0, spec.cruise);
        let mut pilot = Pilot::new(Skill::average(), 13);
        let h = 1.0 / 180.0;
        let world = empty_world();
        let start = geo::pos_to_lla(me.pos).2;
        for _ in 0..(60.0 / h) as usize {
            let c = pilot.controls(&me, None, None, h);
            me.step(h, &c, &world);
        }
        let end = geo::pos_to_lla(me.pos).2;
        assert!((end - start).abs() < 400.0, "the patrol wandered {:.0} m in height", end - start);
        assert!(!me.stalled);
    }
}
