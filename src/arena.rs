//! The arena: waves of bandits, the player's deaths and respawns, the score
//! and the kill feed.
//!
//! The fight is continuous in the world but organised in waves. The first
//! waves grow one aircraft at a time up to the configured number of bandits;
//! no more than that number is in the air at once, so the rest of a wave waits
//! for a slot when the others are still flying. The waves never end: this is
//! the arena's steady state.
//!
//! A dogfight is fought inside a circular [`Zone`] around the start point:
//! bandits are put into the air inside it and turn back when they near its
//! edge, and a player who stays outside it is damaged and then destroyed.
//!
//! Nothing here flies an aeroplane. `main.rs` steps the player, the bandits fly
//! their own `Plane` through `bandits::Bandits::step`, and the arena only
//! decides when a bandit appears, what the score is, when the player's eight
//! seconds are up and what the kill feed says. It asks for the things it
//! cannot do itself (put a bandit in the air, replace the player's aircraft)
//! with [`Event`]s rather than reaching into the game.
use crate::aircraft::Spec;
use crate::airframe::Piece;
use crate::bandits::{Bandits, KillBy};
use crate::guns::Guns;
use crate::t;
use crate::weapons::Weapons;
use glam::DVec3;

/// How long the player waits before a replacement aircraft comes in (s).
pub const RESPAWN_DELAY: f64 = 8.0;

/// How long a kill-feed line stays up (s). It is at full strength until its
/// last [`FEED_FADE`] seconds, then fades to nothing.
pub const FEED_LIFE: f32 = 6.0;
/// How long a kill-feed line fades over before it is dropped (s).
pub const FEED_FADE: f32 = 1.5;

/// The most bandits in the air at once, when the caller does not ask for a
/// different number: two.
pub const MAX_ALIVE: usize = 2;

/// The most bandits the start screen can ask the arena to put in the air.
pub const MAX_BANDITS: usize = 4;

/// Seconds outside the combat zone before structural damage begins.
pub const ZONE_GRACE: f64 = 60.0;

/// Hull damage per second once [`ZONE_GRACE`] is over. The hull fails at 0.6
/// (`Plane::wrecked`), so about twenty seconds of damage follow the grace.
pub const ZONE_DAMAGE: f32 = 0.03;

/// What one step of a zone breach does to the airframe.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ZoneHull {
    /// Still inside the grace, or the call is not a breach at all.
    Hold,
    /// The hull is taking structural damage; this is its new value.
    Damaged(f32),
    /// The hull has given way: the aircraft must break up now.
    Failed,
}

/// The hull result of one step spent outside the zone. `outside` is the time
/// spent outside before this step (s), `h` the step length.
pub fn zone_hull(hull: f32, outside: f64, h: f64) -> ZoneHull {
    if outside <= ZONE_GRACE {
        return ZoneHull::Hold;
    }
    let next = hull + ZONE_DAMAGE * h as f32;
    if next >= 0.6 {
        ZoneHull::Failed
    } else {
        ZoneHull::Damaged(next)
    }
}

/// The first waves grow one, two, three aircraft and then hold at three; a
/// larger configured enemy count lets the fourth join at wave four.
const WAVE_CAP: u32 = 3;
/// The most kill-feed lines kept; old ones fall off the bottom of the list.
const FEED_MAX: usize = 6;

/// Where a wave appears: ahead of the player at the briefed set-up range of
/// five nautical miles, 300 m above it, and one nautical mile to either side
/// so two bandits do not share a point. A spawned bandit is brought inside the
/// combat zone if the zone is smaller than that.
const SPAWN_AHEAD: f64 = 5.0 * 1852.0;
const SPAWN_UP: f64 = 300.0;
const SPAWN_SIDE: f64 = 1852.0;

/// A circular combat zone: a horizontal circle on the planet, open above and
/// below. Distances are measured along the ground, not through the air, so
/// climbing does not take an aircraft out of the fight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zone {
    /// The centre of the circle (anything with the right latitude and
    /// longitude; its height only sets the plane the circle is measured in).
    pub centre: DVec3,
    /// The radius along the ground (m).
    pub radius: f64,
}

impl Zone {
    pub fn new(centre: DVec3, radius: f64) -> Self {
        Self { centre, radius: radius.max(500.0) }
    }

    /// Horizontal distance from the centre (m).
    pub fn distance(&self, pos: DVec3) -> f64 {
        let up = self.centre.normalize_or_zero();
        let d = pos - self.centre;
        (d - up * d.dot(up)).length()
    }

    /// Is the position inside the circle, whatever its height?
    pub fn contains(&self, pos: DVec3) -> bool {
        self.distance(pos) <= self.radius
    }

    /// `pos` brought horizontally inside `fraction` of the radius, keeping its
    /// height. Used for spawns and for where a replacement aircraft comes back.
    pub fn clamp(&self, pos: DVec3, fraction: f64) -> DVec3 {
        let up = self.centre.normalize_or_zero();
        let off = pos - self.centre;
        let flat = off - up * off.dot(up);
        let limit = self.radius * fraction.clamp(0.0, 1.0);
        if flat.length_squared() <= limit * limit {
            return pos;
        }
        self.centre + flat.normalize_or_zero() * limit + up * off.dot(up)
    }
}

/// One line of the kill feed and how long it has been up.
#[derive(Clone, Debug, PartialEq)]
pub struct FeedEntry {
    /// Seconds since the line appeared.
    pub age: f32,
    pub text: String,
}

impl FeedEntry {
    /// How strongly the line is drawn, 0..1: full strength, then a linear fade
    /// over its last [`FEED_FADE`] seconds.
    pub fn alpha(&self) -> f32 {
        ((FEED_LIFE - self.age) / FEED_FADE).clamp(0.0, 1.0)
    }
}

/// What the arena needs to know of the player for one step.
#[derive(Clone, Copy, Debug)]
pub struct PlayerState {
    /// Where the aircraft is: the next wave appears relative to it.
    pub pos: DVec3,
    /// Which way its nose points, flattened along the local horizon.
    pub forward: DVec3,
    /// The aircraft is wrecked or has crashed.
    pub destroyed: bool,
    /// What brought it down, when the caller knows. A ground impact is
    /// nobody's kill; a collision or a bandit's gun is worth a feed line.
    pub cause: Option<KillBy>,
}

/// What the caller must do after a step. The arena does not own the player's
/// aircraft, nor a bandit's model or cannon, so it asks instead of reaching
/// into the game.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// Put a bandit in the air here, flying along `dir` at its type's cruise.
    Spawn { pos: DVec3, dir: DVec3 },
    /// The player's eight seconds are up: a fresh aircraft is due.
    Respawn,
    /// A new wave has begun.
    Wave(u32),
}

/// The arena's whole state: the wave being fought, the score, the respawn
/// timer and the kill feed.
pub struct Arena {
    /// The wave being fought, 1-based; 0 before the first has begun.
    pub wave: u32,
    /// Bandits the player has brought down.
    pub kills: u32,
    /// Times the player's aircraft has been destroyed.
    pub deaths: u32,
    /// Seconds left before the replacement aircraft, while the player is down.
    pub respawn_in: Option<f64>,
    /// The kill feed, oldest first.
    pub feed: Vec<FeedEntry>,
    /// Bandits in the current wave that have not appeared yet.
    left: u32,
    /// The circular battle zone, while a dogfight is running.
    zone: Option<Zone>,
    /// The most bandits in the air at once (the player's choice, 1..=MAX_BANDITS).
    max_alive: usize,
    /// Which bandits' kill credits have been counted, by their slots in
    /// `Bandits` (which are never reused).
    credited: Vec<bool>,
    /// A wave has begun; before that the arena is inert (free flight).
    started: bool,
    /// A replacement aircraft has been asked for but has not reported back;
    /// the same destroyed `PlayerState` must not count as another death.
    waiting: bool,
    /// The next bandit's lateral offset, so a wave spreads out across the sky.
    slot: u32,
}

impl Default for Arena {
    fn default() -> Self {
        Self::new()
    }
}

impl Arena {
    pub fn new() -> Self {
        Self {
            wave: 0,
            kills: 0,
            deaths: 0,
            respawn_in: None,
            feed: Vec::new(),
            left: 0,
            zone: None,
            max_alive: MAX_ALIVE,
            credited: Vec::new(),
            started: false,
            waiting: false,
            slot: 0,
        }
    }

    /// Enter dogfight mode inside `zone`, with up to `enemies` bandits in the
    /// air at once (1..=MAX_BANDITS). Wave one is due at the next step.
    pub fn begin_in(&mut self, zone: Zone, enemies: usize) {
        self.zone = Some(zone);
        self.max_alive = enemies.clamp(1, MAX_BANDITS);
        self.begin();
    }

    /// Enter dogfight mode: wave one is due at the next step. The zone and
    /// enemy count already set on the arena are kept.
    pub fn begin(&mut self) {
        let (zone, max_alive) = (self.zone, self.max_alive);
        self.reset();
        self.zone = zone;
        self.max_alive = max_alive;
        self.started = true;
        self.wave = 1;
        self.left = quota(1, self.max_alive);
    }

    /// The running fight's zone, if it has one.
    pub fn zone(&self) -> Option<Zone> {
        self.zone
    }

    /// Back to dormant: no wave, no score, no feed and no zone.
    pub fn reset(&mut self) {
        self.wave = 0;
        self.kills = 0;
        self.deaths = 0;
        self.respawn_in = None;
        self.feed.clear();
        self.left = 0;
        self.zone = None;
        self.credited.clear();
        self.started = false;
        self.waiting = false;
        self.slot = 0;
    }

    /// The caller has put a fresh aircraft in the air: the death timer is done
    /// with and the same aircraft's loss cannot be counted twice.
    pub fn respawned(&mut self) {
        self.respawn_in = None;
        self.waiting = false;
    }

    /// One step of the arena's clock. `bandits` is read for the living and the
    /// fallen (their credits and labels) and tidied of finished wrecks; the
    /// events tell the caller what to put in the air.
    pub fn step(&mut self, h: f64, bandits: &mut Bandits, player: &PlayerState) -> Vec<Event> {
        let mut events = Vec::new();
        self.age_feed(h);
        if !self.started {
            return events;
        }
        bandits.remove_gone();
        self.count_kills(bandits);

        // The player's death. It is counted once, when the aircraft is first
        // seen destroyed; the latch keeps the same wreck from being counted
        // again on the steps before its replacement arrives.
        if !player.destroyed {
            self.waiting = false;
        }
        let down_now = player.destroyed && !self.waiting && self.respawn_in.is_none();
        if down_now {
            self.deaths += 1;
            self.respawn_in = Some(RESPAWN_DELAY);
            // A collision already has its line from the bandit's credit: the
            // same aircraft going down twice must not read as two events.
            if let Some(by) = player.cause.filter(|by| *by != KillBy::Collision) {
                self.feed_by(by, t!("YOU", "VOUS"));
            }
        }
        // The countdown. The step that sees the death does not also eat into
        // the eight seconds, so the timer is exactly `RESPAWN_DELAY` when it
        // is armed.
        if !down_now {
            if let Some(left) = self.respawn_in {
                let left = left - h;
                if left <= 0.0 {
                    self.respawn_in = None;
                    self.waiting = true;
                    events.push(Event::Respawn);
                } else {
                    self.respawn_in = Some(left);
                }
            }
        }

        // Waves: no new one while the player is out of the fight. A wave is
        // over when all its bandits have appeared and none is still flying.
        if self.respawn_in.is_none() && !self.waiting {
            let alive = bandits.alive_count();
            if self.left == 0 && alive == 0 {
                self.wave += 1;
                self.left = quota(self.wave, self.max_alive);
                events.push(Event::Wave(self.wave));
            }
            let mut room = self.max_alive.saturating_sub(alive);
            while self.left > 0 && room > 0 {
                let pos = spawn_point(player, self.slot, self.zone);
                self.slot += 1;
                self.left -= 1;
                room -= 1;
                // Head-on: from the spawn point towards the player, even when
                // the zone has pulled the point in to one side.
                let dir = (player.pos - pos).normalize_or_zero();
                let dir = if dir.length_squared() > 0.5 { dir } else { -ahead(player) };
                events.push(Event::Spawn { pos, dir });
            }
        }
        events
    }

    /// Drop the kill-feed lines that have outlived their fade and age the rest.
    fn age_feed(&mut self, h: f64) {
        for e in &mut self.feed {
            e.age += h as f32;
        }
        self.feed.retain(|e| e.age < FEED_LIFE);
    }

    /// Count a bandit's credit once, when it goes down, and say so in the feed
    /// by the weapon that did it.
    fn count_kills(&mut self, bandits: &Bandits) {
        if self.credited.len() < bandits.items.len() {
            self.credited.resize(bandits.items.len(), false);
        }
        for (i, slot) in bandits.items.iter().enumerate() {
            if self.credited[i] {
                continue;
            }
            let Some(b) = slot else { continue };
            let Some(by) = b.kill_credit else { continue };
            self.credited[i] = true;
            self.kills += 1;
            self.feed_by(by, &b.label);
        }
    }

    /// A line in the feed for a kill, named after the weapon that made it.
    fn feed_by(&mut self, by: KillBy, who: &str) {
        let text = match by {
            KillBy::Missile => t!("SPLASH  {}", "ABATTU  {}", who).to_string(),
            KillBy::Gun => t!("GUN  {}", "CANON  {}", who).to_string(),
            KillBy::Collision => t!("MID-AIR  {}", "COLLISION EN VOL  {}", who).to_string(),
            KillBy::Zone => t!("LEFT THE ZONE  {}", "SORTI DE LA ZONE  {}", who).to_string(),
        };
        self.push_feed(text);
    }

    fn push_feed(&mut self, text: String) {
        self.feed.push(FeedEntry { age: 0.0, text });
        // A long fight must not grow the HUD without bound: the oldest line
        // falls off the top.
        while self.feed.len() > FEED_MAX {
            self.feed.remove(0);
        }
    }
}

/// A respawned aircraft comes back with its full load: every station loaded,
/// the flare dispenser full and the cannon's belt full. `main.rs` calls this
/// when the arena asks for a respawn and when a flight starts.
pub fn rearm(spec: &'static Spec, pieces: &[Piece], weapons: &mut Weapons, guns: &mut Guns) {
    weapons.reset(spec, pieces);
    guns.reset(spec);
}

/// A new wave begins with full stores: every missile station loaded again,
/// the flare dispenser full and the cannon's belt full. Anything already in
/// the air keeps flying, unlike [`rearm`]. `main.rs` calls this on
/// [`Event::Wave`].
pub fn recharge(weapons: &mut Weapons, guns: &mut Guns) {
    weapons.recharge();
    guns.recharge();
}

/// How many bandits a wave holds: one, then two, then three, and one more for
/// each extra bandit the caller allows in the air (up to `MAX_BANDITS`). The
/// waves keep coming; the live cap is the configured enemy count.
fn quota(wave: u32, cap: usize) -> u32 {
    wave.clamp(1, WAVE_CAP.max(cap.max(1) as u32))
}

/// The player's nose along the local horizon, or its full forward vector when
/// it is pointing straight up or down.
fn ahead(player: &PlayerState) -> DVec3 {
    let up = player.pos.normalize_or_zero();
    let fwd = player.forward - up * player.forward.dot(up);
    // (The flattened vector is a little shorter than one whenever the nose is
    // pitched; only a nearly vertical nose falls back to the unflattened one.)
    if fwd.length_squared() < 1.0e-6 {
        player.forward.normalize_or_zero()
    } else {
        fwd.normalize()
    }
}

/// Where a bandit of a wave appears: five nautical miles ahead of the player,
/// 300 m above it, with a lateral offset that alternates right, left, none.
/// Inside a combat zone, the point is brought in to 72% of its radius.
fn spawn_point(player: &PlayerState, slot: u32, zone: Option<Zone>) -> DVec3 {
    let up = player.pos.normalize_or_zero();
    let fwd = ahead(player);
    let right = fwd.cross(up).normalize_or_zero();
    let side = match slot % 4 {
        1 => 1.0,
        2 => -1.0,
        _ => 0.0,
    };
    let p = player.pos + fwd * SPAWN_AHEAD + right * (side * SPAWN_SIDE) + up * SPAWN_UP;
    match zone {
        Some(z) => z.clamp(p, 0.72),
        None => p,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aircraft::fa18;
    use crate::geo;

    fn player_at(alt: f64) -> PlayerState {
        let pos = geo::lla_to_pos(0.0, 0.0, alt);
        let (_, north, _) = geo::enu(pos);
        PlayerState { pos, forward: north, destroyed: false, cause: None }
    }

    /// Apply the arena's spawn requests to a bandit container, as `main.rs`
    /// does, and report whether a respawn was asked for.
    fn apply(events: Vec<Event>, bandits: &mut Bandits, spec: &'static Spec) -> bool {
        let mut respawn = false;
        for ev in events {
            match ev {
                Event::Spawn { pos, dir } => {
                    bandits.spawn(spec, pos, dir, spec.cruise);
                }
                Event::Respawn => respawn = true,
                Event::Wave(_) => {}
            }
        }
        respawn
    }

    #[test]
    fn waves_never_put_more_than_two_bandits_in_the_air() {
        let spec = &fa18::SPEC;
        let mut bandits = Bandits::new();
        let mut arena = Arena::new();
        let player = player_at(3_000.0);
        arena.begin();
        let h = 1.0 / 180.0;
        let mut wave = 1u32;
        // One entry per wave started; wave one begins without an event.
        let mut per_wave = vec![0u32];
        let mut most = 0;
        for _ in 0..600 {
            for ev in arena.step(h, &mut bandits, &player) {
                match ev {
                    Event::Wave(n) => {
                        assert_eq!(n, wave + 1, "waves must follow one another");
                        wave = n;
                        per_wave.push(0);
                    }
                    Event::Spawn { pos, dir } => {
                        bandits.spawn(spec, pos, dir, spec.cruise);
                        *per_wave.last_mut().unwrap() += 1;
                    }
                    Event::Respawn => panic!("the player was never hit"),
                }
            }
            most = most.max(bandits.alive_count());
            // Work through the wave one bandit at a time.
            let down = bandits.alive().next().map(|(i, _)| i);
            if let Some(i) = down {
                bandits.destroy(i, KillBy::Missile);
            }
            bandits.remove_gone();
        }
        assert!(most <= MAX_ALIVE, "{most} bandits were in the air at once");
        assert!(wave >= 4, "only reached wave {wave}");
        let done = &per_wave[..per_wave.len() - 1];
        assert!(done.len() >= 3);
        assert_eq!(done[0], 1, "wave one is a single bandit");
        assert_eq!(done[1], 2, "wave two is two bandits");
        for (k, n) in done.iter().enumerate().skip(2) {
            assert_eq!(*n, 3, "wave {} was {n} bandits", k + 1);
        }
    }

    #[test]
    fn a_death_arms_exactly_eight_seconds_and_the_respawn_rearms_the_aircraft() {
        let spec = &fa18::SPEC;
        let mut bandits = Bandits::new();
        let mut arena = Arena::new();
        let player = player_at(3_000.0);
        arena.begin();
        let h = 1.0 / 180.0;
        let down = PlayerState { destroyed: true, cause: Some(KillBy::Gun), ..player };
        assert!(!apply(arena.step(h, &mut bandits, &down), &mut bandits, spec));
        assert_eq!(arena.deaths, 1);
        assert_eq!(arena.respawn_in, Some(RESPAWN_DELAY), "the timer is armed at eight seconds");
        assert!((RESPAWN_DELAY - 8.0).abs() < 1e-12);
        // 1,439 more steps: still down. The 1,440th (eight seconds) asks for a
        // replacement, and the steps after it do not count another death.
        let steps = (RESPAWN_DELAY / h).round() as usize;
        for _ in 0..steps - 1 {
            assert!(!apply(arena.step(h, &mut bandits, &down), &mut bandits, spec));
        }
        let mut respawned = 0;
        for _ in 0..3 {
            if apply(arena.step(h, &mut bandits, &down), &mut bandits, spec) {
                respawned += 1;
            }
        }
        assert_eq!(respawned, 1, "the replacement came back once, after eight seconds");
        assert_eq!(arena.respawn_in, None);
        assert_eq!(arena.deaths, 1, "the same wreck was counted twice");

        // The replacement has its full stores and flares, and a full belt.
        let (mut w, mut g) = (Weapons::new(spec, &[]), Guns::new(3));
        w.stations[0].loaded = false;
        w.flares_left = 2;
        g.ammo = 3;
        rearm(spec, &[], &mut w, &mut g);
        assert!(w.stations.iter().all(|s| s.loaded), "not every station reloaded");
        assert_eq!(w.flares_left, 30, "the dispenser is not full");
        assert_eq!(g.ammo, g.magazine(), "the belt is not full");
    }

    #[test]
    fn kills_and_deaths_accumulate_independently_and_a_collision_counts_both() {
        let spec = &fa18::SPEC;
        let mut bandits = Bandits::new();
        let mut arena = Arena::new();
        let player = player_at(3_000.0);
        arena.begin();
        let h = 1.0 / 180.0;
        apply(arena.step(h, &mut bandits, &player), &mut bandits, spec);
        assert_eq!(bandits.alive_count(), 1, "wave one is one bandit");
        // The first bandit goes down to a missile.
        let (i, _) = bandits.alive().next().unwrap();
        bandits.destroy(i, KillBy::Missile);
        arena.step(h, &mut bandits, &player);
        assert_eq!((arena.kills, arena.deaths), (1, 0));
        assert_eq!(arena.feed.len(), 1, "one kill, one line");
        // A collision takes the player with it: one kill and one death, and
        // the collision already has its line from the bandit's credit.
        let down = PlayerState { destroyed: true, cause: Some(KillBy::Collision), ..player };
        arena.step(h, &mut bandits, &down);
        assert_eq!((arena.kills, arena.deaths), (1, 1));
        assert_eq!(arena.feed.len(), 1, "the collision was reported twice");
        // A gun kill of another bandit adds only to the kills.
        let j = bandits.spawn(spec, player.pos + ahead(&player) * 2_000.0, -ahead(&player), spec.cruise);
        bandits.destroy(j, KillBy::Gun);
        arena.step(h, &mut bandits, &down);
        assert_eq!((arena.kills, arena.deaths), (2, 1));
        assert_eq!(arena.feed.len(), 2, "the gun kill has no line");
    }

    #[test]
    fn kill_feed_lines_expire_after_their_life() {
        let spec = &fa18::SPEC;
        let mut bandits = Bandits::new();
        let mut arena = Arena::new();
        let player = player_at(3_000.0);
        arena.begin();
        let h = 1.0 / 180.0;
        let i = bandits.spawn(spec, player.pos + ahead(&player) * 5_000.0, -ahead(&player), spec.cruise);
        bandits.destroy(i, KillBy::Gun);
        arena.step(h, &mut bandits, &player);
        assert_eq!(arena.feed.len(), 1);
        // It stays up until its life is nearly over, and by then it has faded.
        for _ in 0..((FEED_LIFE - 0.2) / h as f32) as usize {
            arena.step(h, &mut bandits, &player);
        }
        assert_eq!(arena.feed.len(), 1, "the line went early");
        let a = arena.feed[0].alpha();
        assert!(a > 0.0 && a < 1.0, "the line did not fade: alpha {a}");
        for _ in 0..((0.5 / h) as usize) {
            arena.step(h, &mut bandits, &player);
        }
        assert!(arena.feed.is_empty(), "the line never expired");
        // The feed is bounded, however long the fight goes on.
        for k in 0..(FEED_MAX as u32 + 10) {
            let j = bandits.spawn(spec, player.pos + ahead(&player) * 5_000.0, -ahead(&player), spec.cruise);
            bandits.destroy(j, KillBy::Missile);
            arena.step(h, &mut bandits, &player);
            let _ = k;
        }
        assert!(arena.feed.len() <= FEED_MAX, "{} lines in the feed", arena.feed.len());
    }

    #[test]
    fn the_next_wave_appears_ahead_inside_the_zone() {
        let mut bandits = Bandits::new();
        let mut arena = Arena::new();
        let player = player_at(3_000.0);
        arena.begin();
        let events = arena.step(1.0 / 180.0, &mut bandits, &player);
        let (pos, dir) = events
            .iter()
            .find_map(|e| match e {
                Event::Spawn { pos, dir } => Some((*pos, *dir)),
                _ => None,
            })
            .expect("the first bandit was not put up");
        let d = pos - player.pos;
        let up = player.pos.normalize();
        let along = d.dot(ahead(&player));
        assert!((along - SPAWN_AHEAD).abs() < 1.0, "spawned {along:.0} m ahead");
        assert!((d.dot(up) - SPAWN_UP).abs() < 1.0, "spawned at the wrong height");
        // The bandit is pointed at the player, so the AI has a contact at once.
        let to_player = (player.pos - pos).normalize();
        assert!(dir.dot(to_player) > 0.999, "the spawn heading is {:.1}° off the player", dir.angle_between(to_player).to_degrees());
        assert_eq!(bandits.items.len(), 0, "the arena spawns through its caller");
    }

    #[test]
    fn a_nose_up_player_still_spawns_bandits_level_ahead() {
        // The player's nose is pitched up in normal flight; the setup point is
        // measured along the horizon, not along the nose.
        let mut player = player_at(3_000.0);
        let up = player.pos.normalize();
        let (_, north, _) = geo::enu(player.pos);
        player.forward = (north + up * 0.25).normalize();
        let p = spawn_point(&player, 0, None);
        let d = p - player.pos;
        assert!((d.dot(up) - SPAWN_UP).abs() < 1.0, "spawned {:.1} m off the level", d.dot(up) - SPAWN_UP);
        assert!((d.dot(ahead(&player)) - SPAWN_AHEAD).abs() < 1.0);
    }

    #[test]
    fn a_small_zone_clamps_the_spawn_inside_its_edge() {
        let mut bandits = Bandits::new();
        let mut arena = Arena::new();
        let player = player_at(3_000.0);
        let zone = Zone::new(player.pos, 4_000.0);
        arena.begin_in(zone, 2);
        let events = arena.step(1.0 / 180.0, &mut bandits, &player);
        let pos = events
            .iter()
            .find_map(|e| match e {
                Event::Spawn { pos, .. } => Some(*pos),
                _ => None,
            })
            .expect("the first bandit was not put up");
        assert!(zone.contains(pos), "spawned {:.0} m from a 4 km centre", zone.distance(pos));
        assert!(zone.distance(pos) <= 0.72 * zone.radius + 1.0, "not brought inside the spawn ring");
    }

    #[test]
    fn the_enemy_count_caps_the_aircraft_in_the_air() {
        let spec = &fa18::SPEC;
        let mut bandits = Bandits::new();
        let mut arena = Arena::new();
        let player = player_at(3_000.0);
        arena.begin_in(Zone::new(player.pos, 20_000.0), 4);
        let h = 1.0 / 180.0;
        let mut most = 0;
        for _ in 0..600 {
            for ev in arena.step(h, &mut bandits, &player) {
                if let Event::Spawn { pos, dir } = ev {
                    bandits.spawn(spec, pos, dir, spec.cruise);
                }
            }
            most = most.max(bandits.alive_count());
            let down = bandits.alive().next().map(|(i, _)| i);
            if let Some(i) = down {
                bandits.destroy(i, KillBy::Missile);
            }
            bandits.remove_gone();
        }
        assert_eq!(most, 4, "the chosen four bandits were never all in the air");
    }

    #[test]
    fn a_minute_outside_is_harmless_then_the_hull_fails() {
        let h = 1.0 / 180.0;
        let mut hull = 0.0f32;
        let mut t = 0.0;
        // The whole grace leaves the hull untouched.
        for _ in 0..(ZONE_GRACE / h) as usize {
            t += h;
            match zone_hull(hull, t, h) {
                ZoneHull::Hold => {}
                ZoneHull::Damaged(v) => hull = v,
                ZoneHull::Failed => panic!("the hull failed inside the grace"),
            }
        }
        assert_eq!(hull, 0.0);
        // After it, damage accumulates step by step and the hull fails after
        // about 0.6 / ZONE_DAMAGE seconds.
        let mut failed_at = None;
        for _ in 0..(30.0 / h) as usize {
            t += h;
            match zone_hull(hull, t, h) {
                ZoneHull::Hold => panic!("damage stopped after the grace"),
                ZoneHull::Damaged(v) => hull = v,
                ZoneHull::Failed => {
                    failed_at = Some(t);
                    break;
                }
            }
        }
        let failed_at = failed_at.expect("the hull never failed");
        let expected = ZONE_GRACE + (0.6 / ZONE_DAMAGE) as f64;
        assert!((failed_at - expected).abs() < 0.1, "failed at {failed_at:.2} s, expected {expected:.2}");
    }
}
