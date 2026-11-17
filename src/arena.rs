//! The arena: waves of bandits, the player's deaths and respawns, the score
//! and the kill feed.
//!
//! The fight is continuous in the world but organised in waves. The first
//! three waves are one, two and three bandits; from the third wave on every
//! wave is three aircraft, with no more than [`MAX_ALIVE`] in the air at once,
//! so a bandit of a three-aircraft wave waits for a slot when the other two are
//! still flying. The waves never end: this is the arena's steady state.
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

/// The most bandits in the air at once. The third bandit of a three-aircraft
/// wave joins when one of the first two goes down.
pub const MAX_ALIVE: usize = 2;

/// The largest wave: one, then two, then three aircraft for ever after.
const WAVE_CAP: u32 = 3;
/// The most kill-feed lines kept; old ones fall off the bottom of the list.
const FEED_MAX: usize = 6;

/// Where a wave appears: ahead of the player at the briefed set-up range of
/// eight nautical miles, 300 m above it, and one nautical mile to either side
/// so two bandits do not share a point.
const SPAWN_AHEAD: f64 = 8.0 * 1852.0;
const SPAWN_UP: f64 = 300.0;
const SPAWN_SIDE: f64 = 1852.0;

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
            credited: Vec::new(),
            started: false,
            waiting: false,
            slot: 0,
        }
    }

    /// Enter dogfight mode: wave one is due at the next step.
    pub fn begin(&mut self) {
        self.reset();
        self.started = true;
        self.wave = 1;
        self.left = quota(1);
    }

    /// Back to dormant: no wave, no score, no feed.
    pub fn reset(&mut self) {
        self.wave = 0;
        self.kills = 0;
        self.deaths = 0;
        self.respawn_in = None;
        self.feed.clear();
        self.left = 0;
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
                self.left = quota(self.wave);
                events.push(Event::Wave(self.wave));
            }
            let mut room = MAX_ALIVE.saturating_sub(alive);
            while self.left > 0 && room > 0 {
                let pos = spawn_point(player, self.slot);
                self.slot += 1;
                self.left -= 1;
                room -= 1;
                events.push(Event::Spawn { pos, dir: -ahead(player) });
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

/// How many bandits a wave holds: one, two, three, then three for ever. The
/// waves keep coming; only the sky is capped (`MAX_ALIVE`).
fn quota(wave: u32) -> u32 {
    wave.clamp(1, WAVE_CAP)
}

/// The player's nose along the local horizon, or its full forward vector when
/// it is pointing straight up or down.
fn ahead(player: &PlayerState) -> DVec3 {
    let up = player.pos.normalize_or_zero();
    let fwd = player.forward - up * player.forward.dot(up);
    if fwd.length_squared() < 1.0 {
        player.forward.normalize_or_zero()
    } else {
        fwd.normalize()
    }
}

/// Where a bandit of a wave appears: eight nautical miles ahead of the player,
/// 300 m above it, with a lateral offset that alternates right, left, none.
fn spawn_point(player: &PlayerState, slot: u32) -> DVec3 {
    let up = player.pos.normalize_or_zero();
    let fwd = ahead(player);
    let right = fwd.cross(up).normalize_or_zero();
    let side = match slot % 4 {
        1 => 1.0,
        2 => -1.0,
        _ => 0.0,
    };
    player.pos + fwd * SPAWN_AHEAD + right * (side * SPAWN_SIDE) + up * SPAWN_UP
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
    fn the_next_wave_appears_eight_nautical_miles_ahead() {
        let mut bandits = Bandits::new();
        let mut arena = Arena::new();
        let player = player_at(3_000.0);
        arena.begin();
        let events = arena.step(1.0 / 180.0, &mut bandits, &player);
        let pos = events
            .iter()
            .find_map(|e| match e {
                Event::Spawn { pos, .. } => Some(*pos),
                _ => None,
            })
            .expect("the first bandit was not put up");
        let d = pos - player.pos;
        let up = player.pos.normalize();
        let along = d.dot(ahead(&player));
        assert!((along - 8.0 * 1852.0).abs() < 1.0, "spawned {along:.0} m ahead");
        assert!((d.dot(up) - SPAWN_UP).abs() < 1.0, "spawned at the wrong height");
        assert_eq!(bandits.items.len(), 0, "the arena spawns through its caller");
    }
}
