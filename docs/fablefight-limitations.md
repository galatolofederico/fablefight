# FableFight — deliberate limitations and approximations

This file records every deliberate approximation in the FableFight weapon
system, with the figure used and why. It is a working list, not a claim that
the aircraft are fully realistic: appearance, a single speed check or a passing
test does not establish that.

## Rebuilding the aircraft assets

The three fighter exteriors are procedural Blender rebuilds:

```sh
blender -b --factory-startup --python assets/blender/fa18.py -- assets
blender -b --factory-startup --python assets/blender/f4.py -- assets
blender -b --factory-startup --python assets/blender/f22.py -- assets
```

Each script states at its head that this **throws away anything edited by hand
in the `.blend`**. To keep hand edits, open the saved `.blend` and run the
same script with `--export-only` (after saving it). The `.blend` and `.bin.gz`
in this repository were rebuilt together with the stores below, so the source
assets and the exported assets are consistent.

## Stores and loadouts

The three fighters carry six visible missiles. The station tables live in
`src/aircraft/{fa18,f4,f22}/mod.rs`; the geometry is in the three Blender
scripts.

| Type | Six-store fit | Station order |
|---|---|---|
| F/A-18C Hornet | 2 × AIM-9X on the wingtip rails, 4 × AIM-120C-5 on the four underwing pylons (two each side) | tips first, then outboard pylons, then inboard |
| F-4E Phantom II | 4 × AIM-7E-2 Sparrows in the fuselage's forward and aft wells, 1 × AIM-9J on each inboard wing pylon | left forward, right forward, left aft, right aft, then the Sidewinders |
| F-22A Raptor | 4 × AIM-120C-5 in the main ventral bay, 1 × AIM-9M in each side bay | main bay 1–4, then left and right side bays |

### Where the positions come from

- **Hornet.** The wingtip rails are at the model's 5.75 m tip; the four
  underwing pylons are the two each side already in `wings()` at 2.35 m
  (inboard) and 3.55 m (outboard) from the centreline. They correspond to the
  real aircraft's stations 3 and 2, with the tips as stations 1 and 9. No exact
  span dimension was available, so the positions follow the model's existing
  pylon geometry rather than a quoted drawing. Sources:
  <https://en.wikipedia.org/wiki/McDonnell_Douglas_F/A-18_Hornet> (armament and
  nine hardpoints).
- **Phantom.** The 2 + 2 Sparrow arrangement and the inboard-pylon Sidewinder
  rails follow Heatblur's F-4E station chart
  (<https://f4.manuals.heatblur.se/stores/overview.html>): stations 3/4 are
  the forward Sparrow wells, 6/7 the aft wells, 2/8 the inboard pylons.
  The fore-and-aft positions are **estimates** scaled from an underside
  photograph of an F-4E carrying four Sparrows and two Sidewinders
  (<https://commons.wikimedia.org/wiki/File:F-4E_Phantom_II_armed_with_missiles_in_1985.JPEG>)
  against the model's 19.2 m length: forward wells about 5.7 m behind the
  nose, aft wells about 10.5 m. The photograph is a perspective view, not a
  drawing, so the placement is good to about ±0.15 m.
- **Raptor.** The main bay is about 0.90 m wide and 3.9 m long
  (<https://www.f-16.net/forum/viewtopic.php?t=8164>, a measured bay; the
  same thread gives the AMRAAM's boxed carriage width). The side bays are
  outboard of the inlets, after GlobalSecurity's description
  (<https://www.globalsecurity.org/military/systems/aircraft/f-22-weapons.htm>).
  The exact fore-and-aft and vertical positions are **estimates** from the
  bay size and the model's belly line, ±0.10 m.

### Store masses

| Store | Mass with a full motor | Source |
|---|---|---|
| AIM-9X | 85.3 kg (188 lb) | <https://en.wikipedia.org/wiki/AIM-9_Sidewinder> |
| AIM-9M / AIM-9J | 85.3 kg / 76.9 kg | same page (variant table) |
| AIM-120C-5 | 161.5 kg (356 lb) | <https://en.wikipedia.org/wiki/AIM-120_AMRAAM> |
| AIM-7E | 197 kg (435 lb) | <https://en.wikipedia.org/wiki/AIM-7_Sparrow> (variant table) |

Full loads: Hornet 816.6 kg, Phantom 941.8 kg, Raptor 816.6 kg. The launcher
rails, pylons and ejector racks are **not** included, and the stores are
treated as being at the reference centre of gravity. A full jet therefore
flies heavier, and firing a missile changes its mass and trim, but the small
CG movement from an individual station is not modelled.

## Guns

The cannon data is from the earlier cannon work and is repeated here because
it is part of the same weapon-loadout picture. All three fire the same
6000 rounds/minute; the magazines differ:

| Type | Gun | Rounds | Sources |
|---|---|---|---|
| F/A-18C | M61A1 | 578 | <https://en.wikipedia.org/wiki/McDonnell_Douglas_F/A-18_Hornet> |
| F-4E | M61A1 | 640 | <https://en.wikipedia.org/wiki/McDonnell_Douglas_F-4_Phantom_II> |
| F-22A | M61A2 | 480 | <https://en.wikipedia.org/wiki/Lockheed_Martin_F-22_Raptor> |

The rate and the 8 mrad diameter 80% dispersion circle are General Dynamics'
figures for the M61 family
(<https://www.gdots.com/armaments/aircraft-guns-gun-systems/m61a1/>).

## Seeker, lock range and minimum range

The game has one infrared-seeker model for every missile. Lock range is
computed from the target's size and **aspect**, where aspect is the angle at
the target between its own nose (its velocity) and the line back to the
shooter:

| Aspect at the target | Range multiplier |
|---|---|
| 0°–30° (head-on) | 0.5 × nominal |
| 30°–90° | linear 0.5 → 1.0 |
| 90°–150° | linear 1.0 → 2.5 |
| 150°–180° (dead astern) | 2.5 × nominal |

The nominal range is 10 km + 1.5 km × size class, so a fighter-sized target
reaches about 32.5 km from dead astern and 6.5 km head-on: a rear-aspect lock is
exactly **5:1** over head-on. These are game values chosen to make aspect
matter, **not** published AIM-9/AIM-120 seeker ranges. A target with no
velocity counts as abeam (nominal range).

The seeker will lock from 150 m out. A **launch inside 600 m is refused**
(distinct "TOO CLOSE" message): the motor and fuse would not arm in time, and
a crossing target leaves the seeker's gimbal almost at once. The documented
AIM-9 minimum launch range is about 0.5 km; 600 m is a deliberately
conservative game figure, so the acceptance check's 500 m crossing shot is
refused.

## Flares

- Capacity: **30 flares per aircraft**, released **two per press**. The F-4E's
  two AN/ALE-40 dispensers on pylons 2/8 carry 60 chaff and 30 flares between
  them (<https://f4.manuals.heatblur.se/stores/overview.html>); the game models
  the 30 flares and no chaff, and gives the same capacity to the Hornet and
  Raptor even though their real dispensers and loads differ. The F-4E
  configuration chosen is therefore the 30-flare ALE-40 fit.
- A flare is ejected backwards and down, burns for 4.5 s, and falls behind.
- A missile's seeker rolls once per 1/180 s step for every flare. The chance
  per step is at most **0.6 %** (point blank, dead astern, flare within 0 m of
  the tail) and falls to zero at 2.5 km; it is further scaled by how close the
  flare is to the target (zero beyond 400 m) and by the cosine of the angle
  between the target's tail and the flare. The chance is bounded to 0..1.
  A seduced missile drops its aircraft target and guides on the flare; it
  detonates within 18 m of it, or flies on blind when the flare burns out.
- Visuals: each flare is a single emissive particle with the flare's ejection
  velocity, not a simulated burning body, and there is no flare light. The
  simulator currently has **no enemy missile launches and no AI flares**, so
  in normal play the seduction path is exercised by the unit test rather than
  by a bandit shooting at the player. That is a missing feature, not a claim
  that decoying works in a full dogfight.

## Other deliberate approximations

- **One guidance law for four missile types.** The AIM-7, AIM-120 and AIM-9
  all use the same boost-and-coast body, the same 32 g fin limit and the same
  IR seeker. The real AIM-7 and AIM-120 are radar-guided, with mid-course
  updates and much longer effective ranges; none of that is modelled. The
  difference between them is currently the model on the rail, the station's
  mass and the name.
- **Missile flight performance** (2.8 s boost, 170 m/s², drag coefficient
  6 × 10⁻⁵, 32 s life, 1000 m/s speed limit) is tuned for play, not matched
  to any one missile's published envelope.
- **F-22 bays.** The main bay doors and side-bay doors are modelled **open**,
  with the AMRAAMs lowered on their trapezes and the Sidewinders extended
  outboard. The real Raptor flies clean with the doors shut; the stores are
  shown because the phase requires six visible missiles. The bay interiors
  are not cut into the fuselage; the door panels and trapezes frame stores
  that sit below the skin.
- **F-4 semi-recessed Sparrows.** The fuselage wells are not cut out. Each
  Sparrow's centre sits 9 cm below the belly line, so the lower body and the
  two lower fins show; the upper pair of fins is omitted because it would
  otherwise break through the skin.
- **F-4 loadout.** The real F-4E can carry four Sparrows and up to four
  Sidewinders (two per inboard pylon); six were chosen to match the phase's
  six-store requirement: four Sparrows and two Sidewinders.
- **Hornet loadout.** 2 × AIM-9X + 4 × AIM-120C-5 is one real six-missile
  fit; the fuselage and centreline stations are empty, and no external fuel
  tanks are carried.
- **Raptor loadout.** 4 × AMRAAM + 2 × Sidewinder is the classic fit; the
  later six-AMRAAM "Sidekick" main-bay load and the external pylons are not
  modelled.
- **Wing damage.** A wing store is lost with the wing it hangs from; fuselage
  stores (the Phantom's Sparrows, the Raptor's bay missiles) survive a wing
  loss. This is a game rule, not a structural analysis.
- **Pylons and launchers** are static geometry with no jettison, no sway
  braces, no umbilical or coolant connections, and no mass. Empty rails stay
  on the aircraft after firing.
- **Firing order** is the first loaded station in the table, not the real
  aircraft's weapon-select logic or clearance-driven sequence.

## The enemy livery

Bandits wear a **fictional aggressor scheme**, not a real unit's or a real
exercise's: the player's paint turned to a dark blue-grey, with red wingtips
and a red band behind the radome. The three paint scripts
(`assets/blender/{fa18,f4,f22}_paint.py`) derive it from the player's pictures
and bake both sets into the same atlas, the player's views first and the
aggressor's after them; `plane.wgsl` picks the second set for a bandit
(`tint.w < 0`). The red bands are placed by model station, not traced from a
photograph. Nothing about the scheme is claimed to be historical.

## The combat zone

- A dogfight is fought inside a circular zone centred on the chosen start
  point. Its radius (**3–60 km**, default 20 km) and the number of bandits in
  the air at once (**1–4**, default 2) are chosen on the start screen. The same
  circle is drawn in red on the globe before starting and on the in-flight
  minimap.
- The boundary is a horizontal circle: altitude does not count, so climbing
  does not take an aircraft out of the fight.
- Bandits turn back for the centre at **80%** of the radius and do not resume
  the chase until they are back inside **55%**, so the AI fight stays within
  the circle. This is a game leash, not a real rules-of-engagement model.
- The player is warned while outside. After **60 s** outside, the hull takes
  `ZONE_DAMAGE` (0.03/s) of structural damage, about twenty seconds from an
  undamaged hull to the 0.6 failure threshold; a wing then tears off and the
  aircraft is lost. The clock resets as soon as the aircraft is back inside.
  The same grace and damage rule applies to a bandit dragged outside. This is a
  game rule, not a structural analysis.
- Spawns are brought inside 72% of the radius. With a very small zone the
  bandits appear closer than the usual five nautical miles.
- The zone arithmetic, spawn clamping, bandit turn-back and the grace/damage
  curve are unit-tested; the HUD warning, the actual wing break-up and a flown
  exit were **not checked** in the simulator.

## Audio model

- The player's aircraft keeps its recorded fighter loops. A bandit's engines
  are a cheap synthesised turbofan voice, because four more copies of the
  recorded loops would be too expensive; they are audible at distance and
  panned by bearing, and only the **four nearest** are played (a fixed voice
  budget). They do not reproduce the type's real engine sound.
- The level falls with distance as `500 / (d + 180)` (capped at 1.4), which is
  a game curve, not an inverse-square measurement.
- The flare dispenser has a thump and hiss. The RWR has a warble while any
  bandit's radar is painting the player, and a separate launch-warning warble
  is wired but **never sounds in play**: no AI aircraft carries or launches a
  missile yet.
- Nothing in the audio was compared against measurements or recordings of a
  real F/A-18, F-4 or F-22; the aircraft sounds remain game sounds.

## Radar and RWR

- The Hornet's radar page (the right DDI) and the RWR are drawn from the real
  bandit positions: bearing relative to the player's nose and slant range.
  The scope is fixed at **40 nm and ±60°**, the RWR's rings at **20 km**, and
  at most **four** contacts are drawn, the nearest. There is no scan latency,
  no fading track history, no altitude or closure readout on the page, and no
  terrain masking or jamming.
- The RWR shows only the bandits whose AI radar currently holds the player:
  the same forward cone and 12 km range that the AI itself uses to see the
  player (the fairness sensor in `src/ai.rs`). It is not an emission-mode or
  threat-library model, and it does not show search radars that have not
  detected the player.
- The radar page and RWR are still decorative in the sense that they do not
  change what the player can lock: the missile seeker has its own aspect and
  range rules, documented above.

## Acceptance walkthrough

Every criterion in `plans/fablefight/PLAN.md` was walked in phase 6. The
result of each is recorded here; the boxes in `PLAN.md` are ticked only for
the criteria that were actually verified.

| Criterion | Result |
|---|---|
| `BUILTIN` is exactly the Hornet, Phantom and Raptor; the civil types are absent from the menu and hangar gallery | **Pass**: `aircraft::tests::builtin_lists_exactly_the_three_fighters`; the menu and hangar gallery list `aircraft::list()`, which is `BUILTIN` plus downloaded types only. |
| `cargo check`, `cargo test`, the shader tests and the MCP tests pass | **Pass**: all four were run in this phase; see the verification status below. |
| The three fly-checks have no regression beyond noise | **Pass**: the pre- and post-change figures are identical to the printed precision; see below. |
| The gamepad layout (rudder, missiles, cannon, afterburner, flares, target cycle, padlock) | **Unverified**: needs a real gamepad and a display. The bindings and the hardcoded layout are compile-checked, but no pad was available. |
| Flaps off every joystick/gamepad button while `F`/`V` still work, with unchanged flap aerodynamics | **Partly verified**: `bind::tests::joystick_presets_leave_flaps_to_the_keyboard` asserts no preset maps flaps and that `F`/`V` remain the keys. The aerodynamic side was **not measured**; no flight-model code was changed, but that is not a measurement. |
| No arming keypress is required to fire | **Pass**: `weapons::tests::a_fresh_aircraft_needs_no_arming_step` (and the default is `armed = true`). |
| Six visible, correctly placed stores; each launch removes the right one | **Pass for geometry and logic**: the station tables and model piece names are unit-tested for all three types, launch hides the fired station's piece, and offscreen Blender previews from front, side, top, under and rear show six stores on each airframe. Not checked in the running simulator. |
| Rear-aspect lock is long, head-on short, and a crossing shot inside ~600 m is refused | **Pass**: `weapons::tests::rear_aspect_locks_five_times_as_far_as_head_on` and `weapons::tests::a_launch_inside_the_minimum_range_is_refused` cover the documented 5:1 ratio and the 600 m refusal. |
| Flares dispense rearward, are limited to 30, and can seduce a tracking missile | **Pass**: `flares_never_exceed_the_carry_and_run_out_cleanly`, `decoy_chance_is_bounded_and_falls_with_range` and `a_flare_behind_the_target_can_seduce_a_seeker`. The visual ejection and a live dogfight were not flown. |
| A bandit flies the full flight model, stalls if mishandled, overshoots and can be out-energised | **Unverified**: the AI's angle-of-attack and energy limits are unit-tested, but the stall/overshoot/out-energy behaviour needs a flown engagement. |
| A bandit that loses sight searches instead of tracking perfectly | **Pass**: `ai::tests::a_player_outside_the_radar_cone_is_not_seen`. |
| Cannon rounds are real projectiles with visible tracers; hits remove structure and can tear parts off | **Unverified**: the projectile, swept-hit, ammunition and structural-damage tests pass, but the tracer visuals were not checked in the simulator. |
| A missile blast near the player does real structural damage; a fatal blast crashes the aircraft | **Fail / unverified**: no missile is ever aimed at the player (AI aircraft carry no missiles), and `main.rs` handles a nearby `weapons::Event::Blast` by shaking the airframe only; it does not call `Plane::blast` on the player. The fatal-blast crash path was therefore not exercised. |
| A mid-air collision destroys both aircraft and reports `MID-AIR` | **Pass**: `arena::tests::kills_and_deaths_accumulate_independently_and_a_collision_counts_both` and the arena integration test. |
| Waves reach the configured number of simultaneous bandits (two by default; one to four on the start screen); respawn is 8 s with full stores; kills and deaths are tallied | **Pass**: `arena::tests::waves_never_put_more_than_two_bandits_in_the_air`, `arena::tests::the_enemy_count_caps_the_aircraft_in_the_air`, `a_death_arms_exactly_eight_seconds_and_the_respawn_rearms_the_aircraft` and `tests/arena_loop.rs`. |
| The dogfight stays inside a configurable circular combat zone; the player outside it for more than a minute is damaged and then destroyed | **Partly verified**: the arena clamps spawns (`a_small_zone_clamps_the_spawn_inside_its_edge`), bandits turn back (`bandits::tests::a_bandit_turns_back_at_the_combat_zone_edge`), and the grace/damage curve is tested (`arena::tests::a_minute_outside_is_harmless_then_the_hull_fails`); the HUD warning and a flown exit were not checked. |
| Nothing in the arena can target live ADS-B or airport traffic | **Pass**: `weapons::tests::dogfight_mode_leaves_live_and_airport_traffic_unshootable`. |
| `docs/fablefight-limitations.md` lists every deliberate approximation | **Pass**: this document, updated in this phase. |

## Verification status

Checked in phase 6: `cargo check`, `cargo test`, `cargo test --test shaders`,
`cargo test --test mcp`, and the three release `--fly-check` runs before and
after the change. The fly-check figures are unchanged (Hornet 587 kt and 113
kt stall; Phantom 595 kt and 141 kt; Raptor 658 kt and 103 kt, with the same
take-off and wing notes). New unit tests cover the four-voice bandit cap, the
radar bearing/range, the RWR threat list, and the aggressor/radar shader
compiles. Offscreen Blender previews were rendered from front, three-quarter,
side, top, under and rear for all three types, and the enemy halves of the
three paint atlases were inspected.

Not checked: a real flight in the simulator, the cockpit view (the radar page
and RWR were only shader-validated, not looked at in the aircraft), firing all
six missiles in the game, gamepad behaviour, a live arena session on each jet,
the flare dispenser's visuals, bandit audibility in the mix, and the tracer
visuals. Those need a display, a gamepad and a pilot; the corresponding boxes
in `plans/fablefight/TASKS.md` are left unchecked.
