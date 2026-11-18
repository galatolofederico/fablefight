# FableFight — Tasks

Source plan: [PLAN.md](PLAN.md)

**Verification commands**

```sh
cargo check
cargo test
cargo test --test shaders
cargo test --test mcp
cargo run --release -- --fly-check fa18
cargo run --release -- --fly-check f4
cargo run --release -- --fly-check f22
```

## How to use this file

- Work through phases in order; later phases depend on earlier ones.
- Read the linked section of `PLAN.md` before starting a phase.
- Write the tests in the same phase as the code they cover; tests are deliverables.
- A phase is complete only when every checkbox is checked and its verification commands pass.
- Keep the project runnable at every phase boundary: `cargo run --release -- --aircraft fa18` must always start a flight.
- Update `PLAN.md` when implementation forces a design change.
- `AGENTS.md` requires a model at or above Claude Opus 5.5 / GPT-6 Astra / Claude Fable capability for this work. Do not split phases to route around that requirement.

## Phase 1 — Identity, roster and controls

**Goal:** The product is renamed, the menu offers exactly the three fighters, the controls match the agreed layout, the arm sequence is gone, and flaps are off the stick.
**Depends on:** none
**Plan:** `PLAN.md` § Approach, § Interfaces, configuration, and CLI
**Files:** `README.md`, `src/aircraft/mod.rs`, `src/bind.rs`, `src/main.rs`, `src/menu.rs`, `src/weapons.rs`

### Implementation

- [x] 1.1 In `README.md`, retitle the document to FableFight and add the credit line "FableFight's design and plan were drafted with DeepSeek V4.1 Flash" directly beside the existing "Created with Claude Fable 5.2 & Opus 5.5" line at line 10.
- [x] 1.2 In `README.md`, update the aircraft list, the missiles section and the controls summary so they describe three fighters, six stores, cannon, flares and the new gamepad layout.
- [x] 1.3 In `src/aircraft/mod.rs`, shrink `BUILTIN` to `[&Spec; 3]` ordered Hornet, Phantom, Raptor; keep the `c172`, `dr400`, `b747`, `concorde`, `sr71` and `starship` module declarations intact.
- [x] 1.4 In `src/aircraft/mod.rs`, add a comment above the retired modules recording the three shared dependencies (`src/plane.rs:615-620`, `src/render.rs:1483`, `src/hangar.rs:887-894`) so a future reader does not delete them.
- [x] 1.5 In `src/bind.rs`, add the actions `fire_cannon` (`Digit6`), `deploy_flares` (`Digit7`) and `padlock` (`KeyJ`) with English and French labels.
- [x] 1.6 In `src/bind.rs`, remove `flaps_down`/`flaps_up` from the three joystick presets while leaving both actions bindable by hand.
- [x] 1.7 In `src/main.rs`, rewrite the hardcoded gamepad layout: LB fires missiles, RB fires the cannon, D-pad up toggles afterburner, D-pad down deploys flares, D-pad left/right cycle targets, Y pads the camera to the target, LT/RT stay rudder, A/B throttle, X brakes, L3 gear.
- [x] 1.8 In `src/main.rs`, delete the Y + d-pad master-arm and cycle chords and the fire-on-Y-release behaviour.
- [x] 1.9 In `src/main.rs`, replace the hardcoded `boost = false` at lines 1786-1787 with persisted afterburner toggle state driven by the D-pad up action.
- [x] 1.10 In `src/weapons.rs`, default `Weapons::armed` to `true` on construction and reset so no keypress is required to fire; keep `Systems::powered()` as the only gate.
- [x] 1.11 In `src/menu.rs`, update `CONTROLS`, `PAD_FLYING` and `PAD_MENUS` to describe the new layout and drop the flap gamepad rows.
- [x] 1.12 In `src/main.rs`, remove the loading-tip line that teaches flaps on the gamepad and add tips for the cannon, missiles and flares.

### Tests

- [x] 1.13 Add a unit test in `src/aircraft/mod.rs` asserting `BUILTIN` has exactly three entries with first keys `fa18`, `f4`, `f22`.
- [x] 1.14 Add a unit test in `src/aircraft/mod.rs` asserting the fighters keep `Spec::id` 2, 3 and 4, and that ids 0, 1, 5, 6, 7 and 8 are unused by `BUILTIN`.
- [x] 1.15 Add a unit test in `src/bind.rs` asserting `fire_cannon`, `fire_missile`, `deploy_flares` and `padlock` exist in `ACTIONS` with the documented default keys.
- [x] 1.16 Add a unit test in `src/weapons.rs` asserting a freshly constructed `Weapons` is `armed`, and that `try_fire` succeeds with a lock without any prior arm toggle.
- [x] 1.17 Add a unit test in `src/bind.rs` asserting that no joystick preset maps a button to `flaps_down` or `flaps_up`.

### Verification

- [x] Run `cargo check`; it exits 0.
- [x] Run `cargo test`; it exits 0 and all tests pass, including the five tests added above.
- [x] Run `cargo run --release -- --aircraft c172`; it warns about the unknown key and starts the Hornet instead of panicking.
- [x] Run `cargo run --release -- --aircraft fa18`; the menu lists only Hornet, Phantom and Raptor.
- [ ] Manually verify on a gamepad that LB, RB, Y and the four d-pad directions behave as specified and that no chord remains.
- [ ] Manually verify that `F` and `V` still move the flaps and that flap aerodynamics are unchanged at low speed.
- [x] Confirm no new skipped, quarantined or failing tests.

**Done when:** the menu offers three fighters, the layout matches the table in `PLAN.md`, and no arming step is required to fire.

## Phase 2 — Bandit aircraft and AI

**Goal:** AI bandits exist as real aircraft flying the shared flight model, can be damaged by blasts, collide with the player, and can be shot with the existing missile system.
**Depends on:** Phase 1
**Plan:** `PLAN.md` § Architecture and data flow, § State model
**Files:** `src/bandits.rs` (new), `src/ai.rs` (new), `src/plane.rs`, `src/damage.rs`, `src/weapons.rs`, `src/main.rs`

### Implementation

- [x] 2.1 Create `src/bandits.rs` with the `Bandit` and `Bandits` structures from `PLAN.md` § State model, plus `Bandits::spawn(spec)`, `Bandits::step(h, &World, &Plane)` and `Bandits::remove_gone()`.
- [x] 2.2 In `src/bandits.rs`, share one `airframe::Model` per type via the existing `Game.hangar` cache; never build a model per bandit.
- [x] 2.3 In `src/ai.rs`, implement `Pilot::controls` producing `plane::Controls` from the bandit state and the player's last known position, with states `Search`, `Pursue`, `Attack`, `Defend` and `Extend`.
- [x] 2.4 In `src/ai.rs`, implement the sensor model: the player is visible only inside the bandit's forward radar cone and range, or while one of its missiles is tracking.
- [x] 2.5 In `src/ai.rs`, enforce the energy rule: no commanded angle of attack above the type's stall angle, and a mandatory `Extend` state when specific energy falls below a threshold.
- [x] 2.6 In `src/ai.rs`, add the `Skill` parameters (aim error, reaction delay, energy aggressiveness) with an "average pilot" default and document each in a comment.
- [x] 2.7 In `src/plane.rs`, add `Plane::blast(at, power) -> Option<usize>` and the hull-sphere accessor used by collision and guns.
- [x] 2.8 In `src/damage.rs`, add the body-point-to-group mapping helper that `Plane::blast` uses, driven by `Spec::groups`.
- [x] 2.9 In `src/weapons.rs`, add `TargetKey::Bandit(u32)` and accept bandit targets without disturbing the existing live and traffic variants.
- [x] 2.10 In `src/main.rs`, step bandits and feed their targets to `Weapons` inside the existing 1/180 s fixed-step loop.
- [x] 2.11 In `src/main.rs`, emit one `render::Inst` and one `airframe::Input` pose per bandit, using `Plane::pose` exactly as the player aircraft does.
- [x] 2.12 In `src/main.rs`, detect player-to-bandit collision using swept segments between previous and current positions; destroy both aircraft and record the event.
- [x] 2.13 In `src/bandits.rs`, keep wrecked bandits as particle fireball plus smoke trail only, then remove them after a fixed dwell time; do not allocate a second `fluid` volume.

### Tests

- [x] 2.14 Add a unit test in `src/ai.rs` asserting a bandit outside its sensor cone reports no target and enters `Search`.
- [x] 2.15 Add a unit test in `src/ai.rs` asserting commanded angle of attack never exceeds the type's stall angle across 1000 simulated steps.
- [x] 2.16 Add a unit test in `src/plane.rs` asserting `blast` returns the expected group for points in the fuselage, each wing and the tail.
- [x] 2.17 Add a unit test in `src/bandits.rs` asserting that destroying a bandit and calling `remove_gone` frees the slot and never reuses the index.
- [x] 2.18 Add an integration test in `tests/` that steps the player and one bandit headlessly, fires a missile, and asserts the bandit is destroyed and a kill is recorded.
- [x] 2.19 Add a unit test in `src/bandits.rs` asserting a swept collision is detected at maximum closing speed where a point test would miss.

### Verification

- [x] Run `cargo check`; it exits 0.
- [x] Run `cargo test`; it exits 0 and all tests pass, including the six added above.
- [ ] Run `cargo run --release -- --aircraft fa18`; a bandit appears, manoeuvres, and can be locked and destroyed with a missile.
- [ ] Manually verify the bandit stalls when slow and high, and overshoots when it turns late.
- [ ] Manually verify that flying into a bandit destroys both aircraft.
- [ ] Confirm the frame time with three aircraft in view has not regressed noticeably.
- [x] Confirm no new skipped, quarantined or failing tests.

**Done when:** a real AI aircraft engages the player, takes damage from blasts, can be shot down, and can be collided with.

## Phase 3 — Cannon and gunsight

**Goal:** The player and the bandits have working cannon with real projectiles, tracers, ammunition limits, and a lead-computing sight.
**Depends on:** Phase 2
**Plan:** `PLAN.md` § Interfaces, configuration, and CLI, § Failure modes and edge cases
**Files:** `src/guns.rs` (new), `src/main.rs`, `src/plane.rs`, `src/audio.rs`, `src/aircraft/mod.rs`, `src/aircraft/fa18/mod.rs`, `src/aircraft/f4/mod.rs`, `src/aircraft/f22/mod.rs`

### Implementation

- [x] 3.1 Create `src/guns.rs` with `Round`, `Guns`, `Guns::step(h, ...)` and `Guns::fire(shooter, origin, direction)`, using the 6000 rpm fractional accumulation described in `PLAN.md`.
- [x] 3.2 In `src/guns.rs`, implement swept-segment hit detection against the hull spheres of every bandit and of the player.
- [x] 3.3 In `src/guns.rs`, apply hits through `Plane::blast` with an energy scaled by the 20 mm round, not by the existing missile warhead power.
- [x] 3.4 In `src/aircraft/mod.rs`, add `Spec::gun` carrying magazine size, rate and dispersion, and populate it for the three fighters from researched references recorded in a comment.
- [x] 3.5 In `src/main.rs`, wire `fire_cannon` to a held trigger, cap live rounds at 2000, and stop firing and stop consuming ammunition at the magazine limit.
- [x] 3.6 In `src/main.rs`, emit tracer particles from the muzzle using the existing particle ring buffer, and a muzzle flash light.
- [x] 3.7 In `src/audio.rs`, add gunfire voices with a rate-limited per-shot click so 100 rounds/s does not swamp the mixer.
- [x] 3.8 In `src/main.rs`, draw a lead-computing pipper on the HUD for the locked target, computed from target range and closure.
- [x] 3.9 In `src/ai.rs`, allow bandits to fire the cannon inside a range gate using the existing `Skill::aim_error` rather than perfect lead.
- [x] 3.10 In `src/main.rs`, show remaining cannon rounds and a "GUNS DRY" HUD state.

### Tests

- [x] 3.11 Add a unit test in `src/guns.rs` asserting the round count fired over one second is within one round of 100.
- [x] 3.12 Add a unit test in `src/guns.rs` asserting the magazine depletes to zero and that further trigger presses spawn no rounds.
- [x] 3.13 Add a unit test in `src/guns.rs` asserting a round travelling 5.7 m per step still hits a 0.6 m target placed between the endpoints, proving the swept test.
- [x] 3.14 Add a unit test in `src/guns.rs` asserting a grazing pass outside the hull radius registers no hit.
- [x] 3.15 Add a unit test in `src/main.rs` asserting the pipper position differs from the aim point when the target has lateral velocity and coincides when closure is zero.
- [x] 3.16 Add a unit test in `src/ai.rs` asserting a bandit does not fire outside its range gate.

### Verification

- [x] Run `cargo check`; it exits 0.
- [x] Run `cargo test`; it exits 0 and all tests pass, including the six added above.
- [ ] Run `cargo run --release -- --aircraft fa18`; the cannon fires, tracers are visible, rounds damage a bandit, and the pipper leads a crossing target.
- [ ] Manually fire the full magazine against a manoeuvring bandit and confirm the dry state and that no rounds spawn afterwards.
- [ ] Confirm audio does not clip or stutter at maximum rate of fire.
- [x] Confirm no new skipped, quarantined or failing tests.

**Done when:** cannon fire is a real projectile weapon with finite ammunition, a visible pipper, and hits that remove structure.

## Phase 4 — Missile loadouts and countermeasures

**Goal:** Each fighter carries six visible missiles on correct stations, seeks them with aspect dependence, has a usable minimum range, and can decoy incoming missiles with flares.
**Depends on:** Phase 2
**Plan:** `PLAN.md` § Interfaces, configuration, and CLI, § State model, § File changes
**Files:** `assets/blender/fa18.py`, `assets/blender/f4.py`, `assets/blender/f22.py`, `assets/fa18.bin.gz`, `assets/f4.bin.gz`, `assets/f22.bin.gz`, `src/aircraft/fa18/mod.rs`, `src/aircraft/f4/mod.rs`, `src/aircraft/f22/mod.rs`, `src/weapons.rs`, `src/plane.rs`, `docs/fablefight-limitations.md` (new)

### Implementation

- [x] 4.1 Read the rebuild instructions in each of `fa18.py`, `f4.py` and `f22.py` before running them; record in a comment that a rebuild overwrites manual `.blend` edits.
- [x] 4.2 In `assets/blender/fa18.py`, replace the two wingtip-only stores with six stations: two tip rails and two pylon pairs, with store geometry on each.
- [x] 4.3 In `assets/blender/f4.py`, add pylons and six stores where the real aircraft carries them, with reference-derived placement.
- [x] 4.4 In `assets/blender/f22.py`, add six stores including the main weapon-bay positions, replacing the painted-on bay doors with real geometry where the stores protrude.
- [x] 4.5 Extend `EXPECTED` in all three scripts with the new piece names and rebuild the `.blend` and `.bin.gz` for each type.
- [x] 4.6 In `src/aircraft/{fa18,f4,f22}/mod.rs`, add the six `MissileStation` entries with the exported piece names, and assign each piece to its wing or fuselage group.
- [x] 4.7 In `src/aircraft/mod.rs`, make `Spec::missiles()` return the per-type table for all three fighters.
- [x] 4.8 In `src/weapons.rs`, make lock range depend on target aspect: long from the rear hemisphere, short from the nose, with the break points recorded in `docs/fablefight-limitations.md`.
- [x] 4.9 In `src/weapons.rs`, refuse a launch inside the minimum range and surface a distinct refusal message.
- [x] 4.10 In `src/weapons.rs`, implement flare dispensing on `deploy_flares`: rearward, up to 30 per aircraft, with per-step decoy rolls that rise near the target's tail and fall off with range.
- [x] 4.11 In `src/plane.rs` and `src/weapons.rs`, make station load state contribute to `Plane::mass()` so a full jet flies heavier and firing changes trim.
- [x] 4.12 Create `docs/fablefight-limitations.md` recording every deliberate approximation: magazine sizes and their sources, missile aspect ranges, minimum range, flare count and decoy probability, and the F-4E flare configuration chosen.

### Tests

- [x] 4.13 Add a unit test in `src/aircraft/mod.rs` asserting each fighter returns exactly six missile stations.
- [x] 4.14 Add a unit test in `src/weapons.rs` asserting lock range from the rear exceeds lock range head-on by the documented ratio.
- [x] 4.15 Add a unit test in `src/weapons.rs` asserting a launch inside the minimum range is refused with the documented reason.
- [x] 4.16 Add a unit test in `src/weapons.rs` asserting flare count never exceeds 30 and that dispensing with zero remaining is a no-op.
- [x] 4.17 Add a unit test in `src/weapons.rs` asserting decoy probability is bounded to 0..1 across the full range and aspect envelope and is monotonic in range.
- [x] 4.18 Add a unit test in `src/plane.rs` asserting mass drops by the documented store mass after each launch.
- [x] 4.19 Add a test in `src/aircraft/fa18/mod.rs` (and equivalents for `f4`, `f22`) asserting every station piece name exists in the loaded model.

### Verification

- [x] Run `cargo check`; it exits 0.
- [x] Run `cargo test`; it exits 0 and all tests pass, including the seven added above.
- [x] Run `cargo test --test shaders`; it exits 0.
- [ ] Run `cargo run --release -- --aircraft fa18`, `--aircraft f4` and `--aircraft f22`; each shows six loaded stores in the correct positions.
- [ ] Visually inspect each aircraft's stores from front, side, top and rear exterior views and from the cockpit, checking for clipping with gear, doors and control surfaces.
- [ ] Fire all six missiles on each type and confirm the correct store disappears each time.
- [ ] Verify by flying that a rear-aspect lock is achievable far beyond head-on lock range, and that a 500 m crossing shot is refused.
- [ ] Verify that flares dispense visually and can seduce a tracking missile.
- [x] Confirm no new skipped, quarantined or failing tests.

**Done when:** all three jets carry six correctly placed missiles, seekers behave differently by aspect, and flares work.

## Phase 5 — Arena loop and scoring

**Goal:** FableFight plays as an arena: escalating waves, death and respawn, a kills/deaths tally, a kill feed, and a HUD target designator.
**Depends on:** Phases 2-4
**Plan:** `PLAN.md` § State model (Arena), § File changes (`src/arena.rs`)
**Files:** `src/arena.rs` (new), `src/main.rs`, `src/menu.rs`, `src/weapons.rs`

### Implementation

- [x] 5.1 Create `src/arena.rs` with the `Arena` structure and `Arena::step(h, &mut Bandits, &PlayerState)`.
- [x] 5.2 In `src/arena.rs`, implement endless waves that each put up the configured simultaneous bandit count (two by default, one to four on the start screen).
- [x] 5.3 In `src/arena.rs`, implement player death, an 8 s timer, and an airborne respawn with a full magazine and full flare load.
- [x] 5.4 In `src/arena.rs`, implement the kills and deaths tally and the kill feed entries `SPLASH`, `GUN`, `MID-AIR`, each attributed by weapon.
- [x] 5.5 In `src/main.rs`, draw the kill feed and the tally on the HUD with an age-based fade.
- [x] 5.6 In `src/main.rs`, draw the target designator: diamond, range, closure, and off-screen threat arrows for bandits outside the view.
- [x] 5.7 In `src/menu.rs`, add a dogfight entry that spawns the player airborne a few miles out at altitude with bandits five nautical miles ahead inside a configurable combat zone, keeping the current world and live traffic.
- [x] 5.8 In `src/weapons.rs`, restrict target collection to bandits while in dogfight mode so live ADS-B and airport traffic cannot be locked.

### Tests

- [x] 5.9 Add a unit test in `src/arena.rs` asserting the wave sequence never puts more than two bandits alive at once.
- [x] 5.10 Add a unit test in `src/arena.rs` asserting the respawn timer is exactly 8 s and restores full stores and flares.
- [x] 5.11 Add a unit test in `src/arena.rs` asserting kills and deaths accumulate independently and that a mid-air collision counts one of each.
- [x] 5.12 Add a unit test in `src/arena.rs` asserting kill-feed entries expire after their documented lifetime.
- [x] 5.13 Add a unit test in `src/weapons.rs` asserting that dogfight mode produces no live or traffic targets even when both are present in the world.
- [x] 5.14 Add an integration test in `tests/` covering spawn, one kill, one player death and respawn in a headless arena run.

### Verification

- [x] Run `cargo check`; it exits 0.
- [x] Run `cargo test`; it exits 0 and all tests pass, including the six added above.
- [ ] Run `cargo run --release -- --aircraft fa18`; entering the dogfight mode spawns airborne with bandits ahead.
- [ ] Manually fight through three waves and confirm each wave's two aircraft, the cap, the kill feed and the tally.
- [ ] Manually die and confirm the 8 s respawn with full stores.
- [ ] Confirm that in dogfight mode no ADS-B or airport traffic can be locked or hit.
- [ ] Confirm the frame time holds with two bandits, tracers and missile trails on screen.
- [x] Confirm no new skipped, quarantined or failing tests.

**Done when:** a complete arena loop runs end to end from spawn to death to respawn with working scoring.

## Phase 6 — Presentation, audio and acceptance

**Goal:** Bandits are visually identifiable and audible, the cockpit radar reflects the real fight, and the full acceptance criteria pass.
**Depends on:** Phases 1-5
**Plan:** `PLAN.md` § Approach, § Acceptance criteria
**Files:** `assets/blender/fa18_paint.py`, `assets/blender/f4_paint.py`, `assets/blender/f22_paint.py`, `assets/fa18_paint.jpg`, `assets/f4_paint.jpg`, `assets/f22_paint.jpg`, `src/shaders/plane.wgsl`, `src/shaders/pit18.wgsl`, `src/audio.rs`, `src/main.rs`, `docs/fablefight-limitations.md`

### Implementation

- [x] 6.1 In the three paint scripts, paint an aggressor scheme for enemy aircraft and regenerate the paint atlases; identify it in the limitations document as the fictional enemy livery.
- [x] 6.2 In `src/shaders/plane.wgsl`, add the livery branch that selects the aggressor paint for a bandit while the player keeps the existing scheme.
- [x] 6.3 In `src/main.rs`, pass an enemy flag into the per-instance tint so the shader can distinguish player from bandit.
- [x] 6.4 In `src/audio.rs`, add bandit engine voices with distance gain and stereo pan, capped at the four nearest, plus flare, launch-warning and RWR tones.
- [x] 6.5 In `src/shaders/pit18.wgsl`, drive the radar page from real bandit positions by bearing and range, and drive the RWR from real threats instead of the hardcoded three.
- [x] 6.6 In `src/main.rs`, feed the radar and RWR uniforms from the arena and weapons state each frame.
- [x] 6.7 Update `docs/fablefight-limitations.md` with the audio model, the radar and RWR simplifications, the aggressor livery, and everything not verified against references.
- [x] 6.8 Walk every acceptance criterion in `PLAN.md` and record the result of each, noting any that could not be checked and why.

### Tests

- [x] 6.9 Add a test in `src/audio.rs` asserting the bandit voice count never exceeds four regardless of how many bandits exist.
- [x] 6.10 Add a unit test in `src/main.rs` asserting radar contact bearing and range match the world-space values for a known geometry.
- [x] 6.11 Add a unit test in `src/main.rs` asserting the RWR lists exactly the bandits currently painting the player.
- [x] 6.12 Add a shader test case in `tests/shaders.rs` covering the new aggressor livery branch and the rewritten radar page.

### Verification

- [x] Run `cargo check`; it exits 0.
- [x] Run `cargo test`; it exits 0 and all tests pass, including the four added above.
- [x] Run `cargo test --test shaders`; it exits 0.
- [x] Run `cargo test --test mcp`; it exits 0.
- [x] Run `cargo run --release -- --fly-check fa18`, `--fly-check f4` and `--fly-check f22`; record the figures and compare against pre-change values, reporting any difference beyond noise.
- [ ] Visually inspect all three aircraft from several exterior angles and from the cockpit: stores, livery, clipping, instrument readability.
- [ ] Fly a full arena session on each of the three jets and confirm bandit audibility, radar and RWR behaviour, and kill-feed accuracy.
- [x] Report every acceptance criterion as pass, fail or unverified, with the reason.
- [x] Confirm no new skipped, quarantined or failing tests.

**Done when:** every acceptance criterion in `PLAN.md` is checked and the full verification suite passes.
