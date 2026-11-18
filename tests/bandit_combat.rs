//! A headless dogfight, end to end: the shipped flight model, the bandit's AI
//! and the missile seeker.
//!
//! The game is a binary crate (the library carries only the MCP bridge), so
//! its modules are compiled into this test with `include!`: nothing here opens
//! a window, touches the GPU or reads the network (the world is flat and
//! offline). The included modules also bring their unit tests into this
//! target; they run again here, on the same code the game ships.
include!("../src/main.rs");

use airframe::{Anim, Mesh, Piece, Role, Vis};

fn sidewinder(name: &'static str, x: f32) -> Piece {
    Piece {
        name,
        group: airframe::G_LWING,
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

#[test]
fn a_missile_shot_down_a_bandit_and_the_kill_is_recorded() {
    let dir = std::env::temp_dir().join("fablegame-bandit-combat-test");
    let src = std::sync::Arc::new(sources::Sources::new(fetch::Fetcher::new(dir, true), String::new()));
    let mut world = world::World::new(sources::Loader::new(src, 0), 14);
    world.flat = Some(0.0);

    let spec = &aircraft::fa18::SPEC;
    let mut player = Plane::spawn(spec, 37.6f64.to_radians(), -122.37f64.to_radians(), 4_000.0, 0.0, spec.cruise);
    let up = player.pos.normalize();
    let fwd = (player.forward() - up * player.forward().dot(up)).normalize();
    let mut bandits = bandits::Bandits::new();
    let id = bandits.spawn(spec, player.pos + fwd * 4_000.0, fwd, spec.cruise);
    assert_eq!(bandits.alive_count(), 1);
    // This test is about the missile's kill path, not the bandit's decoy
    // defence: empty its dispenser so the one shot has to be flown.
    bandits.items[id].as_mut().unwrap().flares_left = 0;

    // The two wingtip missiles, as the model names them.
    let pieces = vec![sidewinder("left sidewinder", -5.9), sidewinder("right sidewinder", 5.9)];
    let mut weapons = weapons::Weapons::new(spec, &pieces);
    assert!(weapons.has_weapons());

    let dt = 1.0 / 180.0;
    let pilot = plane::Controls { throttle: 0.6, ..Default::default() };
    let mut targets = Vec::new();
    let (mut fired, mut killed) = (false, false);
    for _ in 0..(40.0 / dt) as usize {
        player.step(dt, &pilot, &world);
        bandits.step(dt, &world, &player, &weapons.missiles, &weapons.flares);
        targets.clear();
        bandits.append_targets(&mut targets);
        if targets.is_empty() {
            break;
        }
        // Acquire the bandit, wait for the seeker, then shoot: the same path
        // the player's cycle and fire keys drive.
        if weapons.target.is_none() {
            weapons.request_cycle();
        }
        if weapons.lock >= 1.0 && !fired {
            weapons.request_fire();
        }
        for ev in weapons.step_with_flares(dt, &mut player, &targets, &world, true, &bandits.flares) {
            match ev {
                weapons::Event::Fired { .. } => fired = true,
                weapons::Event::Blast { pos, power, target: Some(weapons::TargetKey::Bandit(b)), .. } => {
                    killed |= bandits.hit(b, pos, power, bandits::KillBy::Missile);
                }
                _ => {}
            }
        }
        if killed {
            break;
        }
    }

    assert!(fired, "the missile was never fired");
    assert!(killed, "the missile did not destroy the bandit");
    assert_eq!(bandits.kills, 1, "exactly one kill is recorded");
    let b = bandits.get(id).unwrap();
    assert_eq!(b.kill_credit, Some(bandits::KillBy::Missile));
    assert!(!b.alive);
}

#[test]
fn an_arena_bandit_closes_on_a_straight_flying_player() {
    let dir = std::env::temp_dir().join("fablegame-bandit-approach-test");
    let src = std::sync::Arc::new(sources::Sources::new(fetch::Fetcher::new(dir, true), String::new()));
    let mut world = world::World::new(sources::Loader::new(src, 0), 14);
    world.flat = Some(0.0);

    let spec = &aircraft::fa18::SPEC;
    let mut player = Plane::spawn(spec, 37.6f64.to_radians(), -122.37f64.to_radians(), 3_000.0, 0.0, spec.cruise);
    let mut bandits = bandits::Bandits::new();
    let mut arena = arena::Arena::new();
    let zone = arena::Zone::new(player.pos, 20_000.0);
    arena.begin_in(zone, 2);
    bandits.zone = Some(zone);
    let state = |p: &Plane| arena::PlayerState { pos: p.pos, forward: p.forward(), destroyed: false, cause: None };
    let apply = |events: Vec<arena::Event>, bandits: &mut bandits::Bandits| {
        for ev in events {
            if let arena::Event::Spawn { pos, dir } = ev {
                bandits.spawn(spec, pos, dir, spec.cruise);
            }
        }
    };
    apply(arena.step(1.0 / 180.0, &mut bandits, &state(&player)), &mut bandits);
    let controls = plane::Controls { throttle: 0.7, ..Default::default() };
    let h = 1.0 / 180.0;
    let mut closest = f64::MAX;
    for _ in 0..(45.0 / h) as usize {
        player.step(h, &controls, &world);
        bandits.step(h, &world, &player, &[], &[]);
        apply(arena.step(h, &mut bandits, &state(&player)), &mut bandits);
        for (_, b) in bandits.alive() {
            closest = closest.min((b.plane.pos - player.pos).length());
        }
    }
    assert!(closest < 2_000.0, "the bandit never came into view: closest {closest:.0} m");
}

