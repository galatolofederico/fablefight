//! The arena, end to end: a wave appears, both bandits are shot down, the player
//! is killed, and comes back eight seconds later with its stores full.
//!
//! The game is a binary crate (the library carries only the MCP bridge), so
//! its modules are compiled into this test with `include!`: nothing here opens
//! a window, touches the GPU or reads the network (the world is flat and
//! offline). The included modules also bring their unit tests into this
//! target; they run again here, on the same code the game ships.
include!("../src/main.rs");

/// Apply the arena's spawn requests and report whether it asked for the
/// player's replacement, exactly as `Game::arena_step` does.
fn apply(events: Vec<arena::Event>, bandits: &mut bandits::Bandits, guns: &mut guns::Guns) -> bool {
    let spec = &aircraft::fa18::SPEC;
    let mut respawn = false;
    for ev in events {
        match ev {
            arena::Event::Spawn { pos, dir } => {
                let i = bandits.spawn(spec, pos, dir, spec.cruise);
                guns.bandit_spawned(i as u32, spec);
            }
            arena::Event::Wave(_) => {}
            arena::Event::Respawn => respawn = true,
        }
    }
    respawn
}

#[test]
fn a_wave_a_kill_a_death_and_a_respawn() {
    let dir = std::env::temp_dir().join("fablegame-arena-loop-test");
    let src = std::sync::Arc::new(sources::Sources::new(fetch::Fetcher::new(dir, true), String::new()));
    let mut world = world::World::new(sources::Loader::new(src, 0), 14);
    world.flat = Some(0.0);

    let spec = &aircraft::fa18::SPEC;
    let mut player = Plane::spawn(spec, 37.6f64.to_radians(), -122.37f64.to_radians(), 3_000.0, 0.0, spec.cruise);
    let mut bandits = bandits::Bandits::new();
    let mut guns = guns::Guns::new(9);
    guns.reset(spec);
    let mut arena = arena::Arena::new();
    let state = |p: &Plane, destroyed: bool, cause: Option<bandits::KillBy>| arena::PlayerState {
        pos: p.pos,
        forward: p.forward(),
        destroyed,
        cause,
    };
    let player_state = |p: &Plane| state(p, false, None);

    arena.begin();
    let h = 1.0 / 180.0;
    let mut respawns = 0;
    respawns += apply(arena.step(h, &mut bandits, &player_state(&player)), &mut bandits, &mut guns) as u32;
    assert_eq!(respawns, 0);
    assert_eq!(arena.wave, 1);
    assert_eq!(bandits.alive_count(), 2, "wave one is the configured two bandits");

    // Fly the first two seconds with both aircraft alive.
    let controls = plane::Controls { throttle: 0.7, ..Default::default() };
    for _ in 0..(2.0 / h) as usize {
        player.step(h, &controls, &world);
        bandits.step(h, &world, &player);
        respawns += apply(arena.step(h, &mut bandits, &player_state(&player)), &mut bandits, &mut guns) as u32;
    }
    assert_eq!(bandits.alive_count(), 2, "more than the configured number joined wave one");

    // The missile's blast path brings both bandits down.
    let down: Vec<_> = bandits.alive().map(|(i, b)| (i, b.plane.pos)).collect();
    assert_eq!(down.len(), 2);
    for (i, at) in down {
        assert!(bandits.hit(i as u32, at, 5.0, bandits::KillBy::Missile));
    }
    respawns += apply(arena.step(h, &mut bandits, &player_state(&player)), &mut bandits, &mut guns) as u32;
    assert_eq!(arena.kills, 2, "the kills were not recorded");
    assert!(
        arena.feed.iter().any(|e| e.text.contains("SPLASH") || e.text.contains("ABATTU")),
        "the kill is not in the feed: {:?}",
        arena.feed
    );
    // The next wave is due, and it is two aircraft, both in the air at once.
    assert_eq!(arena.wave, 2);
    assert_eq!(bandits.alive_count(), 2, "wave two did not put up two bandits");

    // The player's aircraft is destroyed by a bandit's cannon: one death,
    // eight seconds on the clock.
    let dead = state(&player, true, Some(bandits::KillBy::Gun));
    respawns += apply(arena.step(h, &mut bandits, &dead), &mut bandits, &mut guns) as u32;
    assert_eq!(arena.deaths, 1, "the death was not recorded");
    assert_eq!(arena.respawn_in, Some(arena::RESPAWN_DELAY), "the timer is not eight seconds");
    assert!(arena.feed.iter().any(|e| e.text.contains("GUN") || e.text.contains("CANON")));

    // While the player is down the bandits keep flying.
    let before = bandits.alive().next().map(|(_, b)| b.plane.pos);
    for _ in 0..(arena::RESPAWN_DELAY / h) as usize + 4 {
        bandits.step(h, &world, &player);
        respawns += apply(arena.step(h, &mut bandits, &dead), &mut bandits, &mut guns) as u32;
    }
    assert_eq!(respawns, 1, "the player did not come back exactly once");
    assert_eq!(arena.deaths, 1, "the same wreck was counted twice");
    assert_eq!(arena.kills, 2);
    assert_eq!(arena.respawn_in, None);
    if let (Some(before), Some((_, after))) = (before, bandits.alive().next()) {
        assert!((after.plane.pos - before).length() > 500.0, "the fight froze while the player was down");
    }

    // The replacement has full stores, a full dispenser and a full belt.
    let mut weapons = weapons::Weapons::new(spec, &[]);
    let mut guns = guns::Guns::new(11);
    guns.reset(spec);
    weapons.stations[0].loaded = false;
    weapons.flares_left = 0;
    guns.ammo = 0;
    arena::rearm(spec, &[], &mut weapons, &mut guns);
    assert!(weapons.stations.iter().all(|s| s.loaded), "not every station reloaded");
    assert_eq!(weapons.flares_left, 30, "the dispenser is not full");
    assert_eq!(guns.ammo, guns.magazine(), "the belt is not full");

    // A fresh aircraft in the air is not another death, and the arena carries
    // on with the fight that was already running.
    let player = Plane::spawn(spec, 37.6f64.to_radians(), -122.37f64.to_radians(), 3_000.0, 0.0, spec.cruise);
    respawns += apply(arena.step(h, &mut bandits, &player_state(&player)), &mut bandits, &mut guns) as u32;
    assert_eq!(respawns, 1, "a live aircraft asked for another respawn");
    assert_eq!((arena.kills, arena.deaths), (2, 1), "the tally moved after the respawn");
    assert_eq!(arena.wave, 2, "the respawn restarted the waves");
}
