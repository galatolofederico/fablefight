# FableFight — Dogfight Simulator Conversion

**Status:** Draft
**Date:** 2026-11-17
**Related:** [TASKS.md](TASKS.md)

## Goal

Convert FableFlight from an open-world flight simulator into **FableFight**: a visual-range air-combat arena flown with the existing flight model, over the existing real terrain and streaming world.

The player flies one of three fighters, engages escalating waves of AI bandits with cannon, heat-seeking missiles and flares, and takes structural damage that degrades the airframe rather than draining a health bar. Real ADS-B traffic and airport traffic remain in the world as ambience but are no longer shootable. The airframes and flight physics stay referenced and sourced to the standard in `AGENTS.md`; the combat systems are tuned for playability and their approximations are documented rather than presented as accurate.

## Non-goals

- Multiplayer or any network play.
- Beyond-visual-range combat, radar-guided missiles, track-while-scan, datalink.
- Chaff, ECM, or any electronic-warfare model.
- Campaigns, missions, briefings, or scripted objectives.
- Progression, unlocks, currency, or persistent statistics.
- Clickable or interactive cockpit controls.
- Full component-level systems simulation (hydraulics, electrics, pressurisation).
- Contrails and vapour modelling.
- More than one simultaneous GPU fire/smoke simulation volume.
- Renaming the Cargo package or binary (`fablegame`); the product name changes, the crate name does not.
- Full decoupling of the retired civil-aircraft modules from shared code. Deferred to a later branch.

## Current state

The project is a single-aircraft simulator. `Game` owns exactly one `Plane` and one `airframe::Model` (`src/main.rs:749-971`), and `Game::set_aircraft` swaps them (`src/main.rs:1193-1262`). Every other dynamic object lives in its own dedicated vector rather than a shared entity system: cars, airport traffic (`src/traffic.rs`), live ADS-B aircraft (`src/live.rs`), missiles (`src/weapons.rs`), shed parts (`src/damage.rs`), and one crash wreck (`src/crash.rs`).

**Aircraft roster.** `src/aircraft/mod.rs:10-24` declares nine built-in modules and a fixed `BUILTIN: [&Spec; 9]`, ordered Cessna 172RG, DR 400, 747-400, F/A-18C, F-4E, F-22A, Starship, Concorde, SR-71A. Only three are fighters. `Spec::id` (0-8) is the shader "kind" and branches throughout `src/shaders/plane.wgsl`; ids must not be renumbered.

**Retirement is not a clean delete.** Three shared dependencies keep civil aircraft modules alive:

- `src/plane.rs:615-620` calls `crate::aircraft::c172::kinematics` for gear animation on every aircraft type.
- `src/render.rs:1483` embeds `crate::aircraft::b747::deck::strings_wgsl()` into the object shader source.
- `src/hangar.rs:887-894` uses the Cessna, Hornet and 747 specs as stand-in bodies when fitting downloaded aircraft.

**Weapons.** `src/weapons.rs` implements one system: two wingtip IR missiles on the Hornet only. `Spec::missiles()` is hardcoded to the first key `"fa18"` (`src/aircraft/mod.rs:532-537`). A missile has a 2.8 s boost at 170 m/s², quadratic drag `k = 0.00006`, 1000 m/s speed cap, 32 g turn limit, 65° gimbal, 32 s life, and a proximity fuse at target half-span + 12 m. Lock range is 10 km + 1.5 km per size class, capped at 16 km.

**No aspect dependence, no minimum range, no decoys.** The seeker reads target position and size only. Flares do not exist. The missile motor has no energy-bleed behaviour against a manoeuvring target.

**No cannon of any kind.** No bullets, dispersion, ammunition, tracer, gun audio, or gun reticle.

**Damage.** `damage::Damage` (`src/damage.rs:92-137`) is a progressive structural model: wing stations, tears, folded wings, tail, hull batteredness, control-surface survival, engine health, gear, oil, windshield, vibration, fatigue, flutter, flap stress. `Plane::wrecked()` is a predicate, not a threshold. Crucially, a missile blast near the player only sets `dmg.vibe` within 800 m (`src/main.rs:2859-2871`); no combat weapon can currently damage the player.

**Targeting.** `weapons::Target` is built from live ADS-B aircraft and airport traffic movers (`src/main.rs:3387-3400`, `src/live.rs:171-198`, `src/traffic.rs:413-431`). `Target` has no affiliation field. HUD symbology is an egui diamond with range and closure (`src/main.rs:5850-5874`). The Hornet cockpit radar page and RWR are decorative shader code with synthetic contacts (`src/shaders/pit18.wgsl:85-120`, `:173-188`).

**Input.** Keys are rebindable through `src/bind.rs` (`ACTIONS`, 49 rebindable actions). Joystick and gamepad buttons bind to actions per device. A separate hardcoded gamepad layout (`src/main.rs:1669-1881`, `:2389-2469`) applies only to a device with nothing bound in Bindings. On that layout the analog triggers are rudder (`yaw = RT − LT`, `src/main.rs:1782`), the bumpers are flaps, and master arm / target cycle / fire are a Y + d-pad chord with fire on Y release (`src/main.rs:2419-2435`, `:1864-1880`). Afterburner has no gamepad binding at all; `src/main.rs:1786-1787` hardcodes `boost = false`.

**Audio.** One global `Params` snapshot built from the player aircraft and camera (`src/main.rs:3107-3191`). Airport traffic and live aircraft have no engine sound. There is no emitter/listener model.

**Effects.** A shared GPU particle ring buffer (`src/render.rs:411-426`, `:2491-2510`) and a single anchored 40×40×48 fire/smoke grid (`src/fluid.rs`).

**No entity, AI, scenario, score, or networking system exists.** Searches for teams, coalitions, factions, enemies, and AI aircraft find nothing gameplay-related.

## Approach

Build the fight in six phases, each independently playable and verifiable:

1. **Identity, roster and controls** — shrink the roster, rename the product, remap the controls, remove the arm sequence, take flaps off the stick.
2. **Bandit aircraft and AI** — introduce a multi-aircraft entity container and an AI controller that flies the real `Plane` flight model.
3. **Cannon and gunsight** — real projectiles with swept collision and a lead-computing sight.
4. **Missile loadouts and countermeasures** — six stores per jet with real geometry, aspect-dependent seekers, a minimum range, and decoy logic.
5. **Arena loop and scoring** — escalating waves, respawn, kills/deaths tally, kill feed, target designator.
6. **Presentation, audio and acceptance** — aggressor liveries, emitter-lite audio, the optional radar scope, and a full acceptance pass.

Bandits come before weapons because weapons need something to shoot; the existing missile system is repointed at bandits in Phase 2 and gives the fight a working opening move before the cannon exists.

The rejected alternative was a simplified kinematic bandit model. It is cheaper, but an enemy that cannot trade energy, stall, or overshoot makes every engagement a scripting exercise, and the flight model is the only asset this project cannot replace.

The second rejected alternative was deleting the civil aircraft outright. Three shared dependencies make that a refactor of infrastructure disguised as a deletion; it would put every combat bug behind two possible causes.

## Technical design

### Architecture and data flow

```
                 ┌───────────────────────┐
                 │ Game (src/main.rs)    │
                 │  plane: Plane         │
                 │  bandits: Bandits     │
                 │  guns: Guns           │
                 │  weapons: Weapons     │
                 │  arena: Arena         │
                 └───────────┬───────────┘
                             │ fixed step h = 1/180 s
        ┌────────────────────┼────────────────────┐
        │                    │                    │
  Plane::step          Bandits::step         Guns::step
  (player)             ├ ai::Pilot::controls  (swept segments)
                       └ Plane::step          Weapons::step
                                              (missiles, flares)
                             │
                        Arena::step (waves, respawn, score)
                             │
                     render instances + HUD + audio
```

The fixed-step loop at `src/main.rs:3394-3404` already runs `plane.step`, `weapons.step` and `gauges.update` at 1/180 s. Bandits and guns are inserted into that same loop so all entities share one time base.

**Bandit.** A bandit owns its own `Plane`, its own `Damage` (via `Plane.dmg`), and an AI pilot. It shares immutable `Spec` data and a cached `airframe::Model` with any other aircraft of its type.

**Sensors.** The AI sees the player only through a simulated sensor: inside a forward radar cone and range, or while one of its missiles is tracking the player. Losing sight puts it into a search state. This is the fairness contract, not a difficulty option.

**Rendering.** Bandit instances are emitted into the same `render::Inst` list the player aircraft uses (`src/main.rs:4215-4222`). Each needs its model transform plus an `airframe::Input` pose computed by `Plane::pose`, exactly as the player's is.

### State model

No database, no migrations. The new persistent-in-memory structures:

```rust
// src/bandits.rs (new)
pub struct Bandit {
    pub spec: &'static aircraft::Spec,
    pub plane: plane::Plane,      // owns dmg: damage::Damage
    pub ai: ai::Pilot,
    pub alive: bool,
    pub gone: bool,               // wreck finished, ready to remove
    pub kill_credit: Option<KillBy>,
}
pub struct Bandits {
    pub items: Vec<Bandit>,
    prev_pos: Vec<glam::Vec3>,    // for swept collision between steps
}

// src/ai.rs (new)
pub struct Pilot {
    pub skill: Skill,             // aim_error, reaction, energy_use
    pub target_seen: bool,
    pub state: AiState,           // Search, Pursue, Attack, Defend, Extend
    last_seen: Option<glam::Vec3>,
    reaction_timer: f32,
}
impl Pilot { pub fn controls(&mut self, me: &Plane, seen: Option<TargetView>, h: f32) -> plane::Controls }

// src/guns.rs (new)
pub struct Round { pub pos: glam::Vec3, pub vel: glam::Vec3, pub age: f32 }
pub struct Guns {
    pub rounds: Vec<Round>,
    pub ammo: [u32; 2],           // player, then bandit index + 1
    pub trigger: bool,
    reload_acc: f32,
}

// src/weapons.rs (modify)
pub struct Flare { pub pos: glam::Vec3, pub vel: glam::Vec3, pub age: f32, pub life: f32 }

// src/arena.rs (new)
pub struct Arena {
    pub wave: u32,
    pub kills: u32,
    pub deaths: u32,
    pub respawn_in: Option<f32>,
    pub feed: Vec<(f32, String)>, // (age, text)
}
```

`weapons::TargetKey` gains a `Bandit(u32)` variant so missiles can be aimed at AI aircraft without disturbing the existing live/traffic variants, which are retained for free flight.

### Interfaces, configuration, and CLI

**Aircraft.** `Spec::missiles()` keeps its signature but returns per-type data. New `Spec::stores()` returns the station table for a type.

```rust
// src/aircraft/mod.rs
pub fn missiles(&self) -> &'static [MissileStation] {
    if self.keys.contains(&"fa18") { fa18::MISSILES }
    else if self.keys.contains(&"f4") { f4::MISSILES }
    else if self.keys.contains(&"f22") { f22::MISSILES }
    else { &[] }
}
```

**Damage.** Add one entry point on `Plane` so weapons never poke at `Damage` fields directly:

```rust
// src/plane.rs
/// Inject blast energy at a body-frame point. Returns the structural group hit.
pub fn blast(&mut self, at: glam::Vec3, power: f32) -> Option<usize>;
```

`power` is the existing warhead strength (`src/weapons.rs:558-560`). The mapping from point to group uses `Spec::groups`.

**Guns.** `Guns::step(h, &mut [Bandit], &mut Plane, &mut Arena)` advances rounds, tests swept segments against bandit hull spheres and the player hull, and consumes ammunition. The fire rate is 6000 rpm, so `reload_acc` accumulates fractional rounds per step (100 rounds/s ÷ 180 steps/s).

**Bindings.** New actions in `src/bind.rs::ACTIONS`:

| id | default key | gamepad |
|---|---|---|
| `fire_cannon` | `Digit6` | RB |
| `fire_missile` | `Digit5` (unchanged) | LB |
| `deploy_flares` | `Digit7` | D-pad down |
| `cycle_target` | `Digit4` (unchanged) | D-pad left / right |
| `afterburner` | `Space` (unchanged) | D-pad up |
| `padlock` | `KeyJ` | Y |

`fire_missile`, `cycle_target` and `afterburner` already exist as actions and only need gamepad bindings plus the removal of the Y chord.

**CLI.** No new flags. Existing `--aircraft <key>`, `--fly-check <key>` and `--jsbsim-compare` keep working against the shrunken registry. `--aircraft c172` falls back to `BUILTIN[0]`, which becomes the Hornet, and prints the usual unknown-key warning.

**Settings.** `src/settings.rs` gains no new persisted keys. Dogfight mode is entered from the existing menu rather than remembered across launches.

### File changes

| Path | Action | Details |
|---|---|---|
| `README.md` | modify | Retitle to FableFight; add the credit line "FableFight's design and plan were drafted with DeepSeek V4.1 Flash" beside the existing "Created with Claude Fable 5.2 & Opus 5.5"; update the missiles section and the key list. |
| `src/aircraft/mod.rs` | modify | `BUILTIN` shrinks to Hornet, Phantom, Raptor; `missiles()` becomes per-type; add `stores()`. |
| `src/aircraft/fa18/mod.rs` | modify | Six `MissileStation` entries; piece names for new pylons. |
| `src/aircraft/f4/mod.rs` | modify | Six `MissileStation` entries; piece names for new pylon and store geometry. |
| `src/aircraft/f22/mod.rs` | modify | Six `MissileStation` entries; bay and pylon piece names. |
| `assets/blender/fa18.py` | modify | Model six loaded stores; extend `EXPECTED`. |
| `assets/blender/f4.py` | modify | Model pylons and six stores; extend `EXPECTED`. |
| `assets/blender/f22.py` | modify | Model bay-mounted and pylon stores; extend `EXPECTED`. |
| `assets/fa18.bin.gz` | regenerate | Exported from the rebuilt `.blend`. |
| `assets/f4.bin.gz` | regenerate | Exported from the rebuilt `.blend`. |
| `assets/f22.bin.gz` | regenerate | Exported from the rebuilt `.blend`. |
| `src/bind.rs` | modify | New actions; remove flap button presets from the joystick presets. |
| `src/main.rs` | modify | Gamepad layout; remove the Y chord; wire guns, flares, bandits, arena; HUD pipper, designator, kill feed. |
| `src/menu.rs` | modify | `CONTROLS`, `PAD_FLYING` and `PAD_MENUS` tables; dogfight entry. |
| `src/plane.rs` | modify | `Plane::blast`; expose hull spheres for collision. |
| `src/weapons.rs` | modify | `TargetKey::Bandit`; aspect-dependent lock range; minimum range; flare dispensing and decoy rolls; `armed` defaults true. |
| `src/guns.rs` | create (new) | Projectile list, swept hit detection, ammunition, tracer emitters. |
| `src/bandits.rs` | create (new) | Bandit container, model sharing, stepping, wreck removal. |
| `src/ai.rs` | create (new) | Pilot controller, sensor model, skill parameters. |
| `src/arena.rs` | create (new) | Waves, respawn, score, kill feed. |
| `src/damage.rs` | modify | Blast-to-group mapping helper used by `Plane::blast`. |
| `src/audio.rs` | modify | Gun, bandit engine, flare, launch-warning and RWR voices. |
| `src/shaders/pit18.wgsl` | modify | Radar scope driven by world state; RWR from real threats. |
| `src/shaders/plane.wgsl` | modify | Aggressor livery branch per fighter kind. |
| `assets/blender/fa18_paint.py`, `assets/blender/f4_paint.py`, `assets/blender/f22_paint.py` | modify | Aggressor paint scheme per type. |
| `docs/fablefight-limitations.md` | create (new) | Every deliberate approximation, with the number used and why. |

Test files follow the repository's existing convention: unit tests inline in `#[cfg(test)]` modules; integration tests in `tests/`.

### Failure modes and edge cases

- **Empty bandit list.** The arena must idle safely with zero bandits: no panics on empty `max_by`, no HUD division by zero.
- **Bandit dies mid-lock.** A missile tracking a removed bandit must drop its target and continue ballistically rather than dereference a dead index.
- **Respawn during a missile's flight.** Bandit ids are never reused within a wave; a new bandit gets a fresh index so stale `TargetKey::Bandit` values cannot alias.
- **Ammunition exhaustion.** Firing with zero rounds must not spawn projectiles, must not play gun audio, and must show a HUD state.
- **Flare dispensing with no missiles tracking.** Allowed but pointless; must not allocate per press.
- **Player death vs. bandit death in the same step.** Arena resolves both before respawn timers are armed.
- **Mid-air collision at high closure.** Segment test between previous and current positions on both aircraft, not a point test.
- **Retired aircraft keys on the command line.** `--aircraft c172` must warn and fall back, never panic.
- **No gamepad bound.** The hardcoded layout must remain dormant exactly as today (`src/main.rs:1707-1709`).
- **Bandit model not yet built.** Requesting a bandit type whose model is still on the build thread must defer spawning that bandit, not block the frame.
- **Audio voice exhaustion.** Extra bandit engines must be capped, with the nearest N audible.

### Compatibility, rollout, security, and performance

- **Shader ids are load-bearing.** `Spec::id` 0-8 map to shader branches. Retirement leaves gaps (0 Cessna, 1 747, 5 Starship, 6 Concorde, 7 SR-71, 8 DR 400 unused). Reusing an id for a new aircraft would silently paint the wrong livery; new types must take ids ≥ 9.
- **Save compatibility.** `bindings.json` persists action ids; unknown ids must be ignored on load rather than rejected. `settings.json` shape is unchanged.
- **No security surface added.** The MCP loopback bridge (`src/mcp/rpc.rs`) is untouched.
- **Performance budget.** Three aircraft at 180 Hz is arithmetic, but rendering, particles and audio are the real costs. Targets: no regression in frame time with three aircraft in view; no more than four bandit engine voices; no more than ~2000 live rounds (about 20 s of continuous fire from one gun).
- **Memory.** Bandit models share one `airframe::Model` per type through the existing `Game.hangar` cache (`src/main.rs:1193-1262`); never build a model per bandit.

## Testing strategy

Unit tests live inline in the modules they cover, matching `src/weapons.rs:584-652` and `src/aircraft/fa18/mod.rs:520-526`. Integration tests live in `tests/`.

- **Unit:** gun rate accumulation and ammunition depletion; swept-segment hit detection including a grazing miss and a tunnelling case; blast-to-group mapping; aspect-dependent lock range; minimum-range refusal; flare decoy probability bounds; AI sensor visibility cone; arena escalation and respawn timers.
- **Integration:** a headless two-aircraft step loop proving a bandit can be hit and destroyed, and that the player takes structural damage from a nearby blast.
- **End-to-end:** a scripted run (existing `--crash-at`-style switch or a new `--dogfight-at`) that spawns a bandit, fires, and asserts a kill is recorded.
- **Shaders:** `tests/shaders.rs` already validates WGSL via `naga`; every shader edit must keep it green.
- **Manual:** fly each of the three jets, verify control mapping on a real gamepad, verify bandit visibility and AI behaviour from the cockpit, and inspect each new store geometry from exterior angles.

Exact commands:

```sh
cargo check
cargo test
cargo test --test shaders
cargo test --test mcp
cargo run --release -- --fly-check fa18
cargo run --release -- --fly-check f4
cargo run --release -- --fly-check f22
```

## Phase overview

| Phase | Title | Outcome | Depends on |
|---|---|---|---|
| 1 | Identity, roster and controls | Roster shrunk to three fighters, product renamed, controls remapped, arm sequence removed, flaps off the stick | none |
| 2 | Bandit aircraft and AI | Bandits exist as real aircraft under AI control, can be damaged, and can be shot with the existing missile system | Phase 1 |
| 3 | Cannon and gunsight | Cannon fires real projectiles; player has a lead-computing sight; AI can shoot too | Phase 2 |
| 4 | Missile loadouts and countermeasures | Six stores per jet with real geometry, aspect-dependent seekers, minimum range, flares, store mass | Phase 2 |
| 5 | Arena loop and scoring | Escalating waves, respawn, kills/deaths tally, kill feed, HUD target designator | Phases 2-4 |
| 6 | Presentation, audio and acceptance | Aggressor liveries, emitter audio, radar scope, and a full acceptance pass | Phases 1-5 |

## Risks and mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| Removing civil aircraft breaks shared dependencies | Build failure across `plane.rs`, `render.rs`, `hangar.rs` | Keep the modules compiling; only unregister them. The decoupling refactor is explicitly deferred. |
| Renumbering `Spec::id` | Silent wrong liveries and geometry across all aircraft | Never reassign an existing id; new types take ids ≥ 9. Add a test asserting the three fighter ids. |
| Bandit AI never feels fair | The whole arena fails regardless of weapon quality | Fix the sensor model and the "respects energy" rule first; tune only aim error, reaction delay and energy aggressiveness. |
| Blender procedural rebuild overwrites manual edits | Lost modelling work | Read each script's rebuild instructions before running (`AGENTS.md`); keep source and exported assets consistent. |
| Blender work for stores on three airframes is larger than expected | Phase 4 slips | Phase 4 can ship with generic store geometry on correct pylons, then be refined per type; the plan's acceptance criterion is station count and placement, not surface detail. |
| Swept-segment collision misses at high closure | Rounds pass through bandits | Test explicit tunnelling cases at 180 Hz and maximum muzzle velocity. |
| Three aircraft plus particles plus audio exceed frame budget | Unplayable | Cap live rounds and audio voices; profile before adding a fourth bandit. |
| Live traffic ghosting through the fight | Immersion break | Accepted and documented: no collision with traffic or live aircraft. Optionally suppress live traffic inside the arena volume. |
| Downloaded aircraft re-enter the menu as non-fighters | "Fighters only" rule violated in spirit | Downloaded types are selectable for free flight only; the dogfight entry lists the three built-ins. |

## Acceptance criteria

- [x] `BUILTIN` contains exactly the F/A-18C, F-4E and F-22A; the Cessna, DR 400, 747, Concorde, SR-71A and Starship do not appear in the menu or the hangar gallery.
- [x] `cargo check`, `cargo test`, `cargo test --test shaders` and `cargo test --test mcp` all pass.
- [x] `--fly-check fa18`, `--fly-check f4` and `--fly-check f22` produce the documented handling figures with no regression against pre-change values beyond noise.
- [ ] On a gamepad, LT/RT control rudder, LB fires missiles, RB fires the cannon, D-pad up toggles afterburner, D-pad down dispenses flares, D-pad left/right cycle targets, Y pads the camera to the target.
- [ ] Flaps are not bound to any joystick or gamepad button; keyboard `F`/`V` still move them, and flap aerodynamics are unchanged.
- [x] No arming keypress is required to fire; weapons are live whenever the aircraft has electrical power and stores.
- [x] Each fighter carries six missiles with visible, correctly placed store geometry on it at spawn; each launch removes the correct store.
- [x] A missile fired from the rear locks at long range; the same missile fired head-on locks only at short range; inside roughly 600 m a crossing-target shot misses.
- [x] Flares dispense rearward, are limited to 30 per aircraft, and can seduce a tracking missile.
- [ ] A bandit flies the full flight model: it stalls if mishandled, overshoots if it turns too hard, and can be out-energised.
- [x] A bandit that loses sight of the player searches instead of tracking it perfectly.
- [ ] Cannon rounds are real projectiles with visible tracers; hits remove structure from the bandit and can tear off parts.
- [ ] A missile blast near the player causes real structural damage, and a fatal blast produces the existing crash behaviour.
- [x] Mid-air collision with a bandit destroys both aircraft and reports `MID-AIR`.
- [x] Every wave puts up the configured simultaneous bandit count (two by default); the player respawns after 8 s with a full magazine; kills and deaths are tallied.
- [x] Nothing in the arena can target live ADS-B traffic or airport traffic.
- [x] `docs/fablefight-limitations.md` lists every deliberate approximation with its value and rationale.

## Assumptions and open questions

- **Unverified:** the exact M61 magazine sizes per type, dispersion figures, and muzzle velocity. All three types carried M61-derived 20 mm cannon in reality, but the counts must be researched against references before being written into `Spec` data. Do not guess.
- **Unverified:** the F-4E flare dispenser fit. Most of its service life had no CMDS; the loadout must be chosen from a specific documented configuration and stated in the limitations document.
- **Unverified:** pylon station geometry and store placement for all three airframes. Requires reference drawings or photographs per type.
- **Assumption:** the player's dogfight entry point is a menu item rather than a CLI flag. Confirm during Phase 5.
- **Assumption:** downloaded FlightGear aircraft remain available for free flight only. Veto if the hangar should be removed entirely.
- **Open:** whether stores affect drag as well as mass. Phase 4 connects mass; drag is deferred unless it proves noticeable.
- **Open:** the radar scope in Phase 6 is optional and may be dropped if schedule pressure appears; the HUD designator is the mandatory piece.
- **Resolved:** the arena's steady state is endless waves of the start screen's simultaneous bandit count (two by default, one to four); every wave starts with that many in the air.
- **Constraint from `AGENTS.md`:** implementation requires a model at or above Claude Opus 5.5 / GPT-6 Astra / Claude Fable capability. This plan was drafted by DeepSeek V4.1 Flash and must be executed by a model meeting that bar.

## References

- `AGENTS.md` — model requirements, research standards, cockpit and exterior realism, flight-behaviour validation, verification duties.
- `README.md` — current product description and the existing model-capability guidance at line 122.
- `src/aircraft/mod.rs` — `Spec`, `MissileStation`, `BUILTIN`, `missiles()`.
- `src/weapons.rs` — missile system, seeker, lock, arm sequence.
- `src/damage.rs` — structural damage model and `Spec::groups`.
- `src/plane.rs` — flight model, `Plane::step`, `wrecked()`, gear kinematics call.
- `src/bind.rs` — rebindable actions, gamepad presets.
- `src/main.rs` — fixed-step loop, gamepad layout, HUD, audio params.
- `src/fluid.rs` — single fire/smoke simulation volume.
- `docs/mcp.md` — existing MCP copilot bridge, untouched by this plan.
