# Agent instructions for FableFlight

These instructions apply throughout this repository.

## Learn from the existing aircraft

Read the closest existing aircraft before adding or substantially changing one. Reuse the project's integration patterns while verifying real-world accuracy independently; existing values are not authoritative aircraft specifications.

- `src/aircraft/mod.rs`: aircraft registry and shared `Spec`, including geometry, mass, engines, gear, handling, cameras, and sound.
- `src/aircraft/c172/` and `src/aircraft/dr400/`: aircraft and cabins built in Rust with the toolkit in `src/airframe.rs`.
- `src/aircraft/fa18/` and `assets/blender/fa18.py`: Blender exterior geometry, exported moving pieces, and a Rust cockpit. Other Blender aircraft have their own scripts under `assets/blender/`.
- `src/aircraft/b747/deck.rs` and `src/aircraft/fa18/pit.rs`: detailed cockpit layouts, labels, and live instruments.
- `src/shaders/plane.wgsl` and the relevant cockpit shaders: materials, liveries, surface detail, and instrument rendering.
- `src/plane.rs` and `src/systems.rs`: shared flight physics and aircraft systems.

## Research and dimensions

- Identify the exact aircraft variant and configuration being reproduced. Keep its geometry, cockpit, engines, weights, and performance consistent with that variant.
- Look up real references before modeling or tuning. Prefer manufacturer specifications, pilot operating handbooks, flight manuals, technical drawings, and clear photographs of the correct variant. Cross-check conflicting figures.
- Record source URLs and the relevant figures in aircraft comments or accompanying documentation. State units, configuration, and any assumptions. Mark estimates and unavailable data honestly; do not invent precision or claim unverified accuracy.
- Build at real scale. Match length, wingspan, height, wing area, sweep, dihedral, fuselage cross-sections, tail geometry, engine locations, landing gear, and wheel sizes to documented measurements.
- Use metres for geometry and the existing units documented by `Spec`. The game body frame is +X right, +Y up, -Z forward, with the origin at the reference centre of gravity. Check coordinate conversions when exporting from Blender.
- Keep rendered geometry, collision/contact points, gear locations, hinges, pilot eye position, and aerodynamic dimensions aligned.

## Cockpit realism

- Reproduce the actual cockpit's proportions and layout using photographs and manuals: panel shapes, instrument positions, consoles, seats, controls, pedals, canopy or windows, and pilot sightlines.
- Use realistic materials, labels, fasteners, bezels, switches, lighting, and display symbology. Instruments must be readable from the normal pilot view.
- Drive implemented gauges, displays, annunciators, and animated controls from the appropriate simulated state. Document any controls or systems that remain decorative or simplified.
- Check the cockpit from the seated pilot's view and exterior views for scale, visibility, clipping, and fit inside the airframe.

## Exterior realism

- Match the real aircraft's silhouette and construction, including intakes, exhausts, doors, access panels, control-surface gaps, gear wells, lights, and antennas.
- Include realistic panel seams, rivet rows, bolts, screws, and other fasteners where the actual aircraft has them. Match their placement and scale; respect differences between metal, composite, wood, and fabric construction. The DR400's wood-and-fabric surfaces, for example, should not receive generic metal rivet patterns.
- Use appropriate paint, glass, rubber, fabric, and metal finishes. Base wear, markings, and liveries on references; identify fictional liveries as fictional.
- Give moving parts correct pivots, axes, travel, and timing. Check the full movement of gear, doors, flaps, control surfaces, propellers, and canopies for interference and clipping.
- Preserve exporter piece names, material identifiers, and animation/damage mappings. Read each Blender script's rebuild instructions before using it; a procedural rebuild can overwrite manual `.blend` edits. Keep source assets and exported assets consistent.

## Flight behavior

- Tune each aircraft to its real behavior using documented performance, not just another aircraft's settings. A copied `Spec` is only a starting point.
- Match mass, payload, fuel capacity and consumption, centre-of-gravity range, inertia, power or thrust, drag, lift, and configuration effects to the selected variant.
- Validate stall speeds, takeoff and landing performance, climb, cruise, maximum speed, approach behavior, and handling against references at comparable weight, altitude, atmosphere, power, and flap/gear configuration. Distinguish indicated airspeed, true airspeed, groundspeed, and Mach.
- Reproduce the aircraft's characteristic stability, control response, trim, stall behavior, and engine response within the simulator's capabilities. Keep gear, flap, structural, and engine limits grounded in references.
- Do not change shared physics merely to make one aircraft hit a target if it breaks the others. Check affected existing aircraft whenever shared flight code changes.
- Document simulator limitations and remaining discrepancies. Do not describe an aircraft as fully realistic based only on its appearance or a single speed check.

## Verification

- For aircraft code changes, run `cargo check` and the relevant tests; for shared physics changes, include the flight-model tests. Run `cargo test --test shaders` when changing shaders, plus relevant cockpit shader tests.
- For flight changes, use `cargo run --release -- --fly-check <key>` as an initial performance check. Where suitable reference files are available, use `--jsbsim-compare <file> --aircraft <key>` to compare flight-model data. These checks supplement flight testing and real-world references.
- For visual changes, inspect the aircraft in the simulator or rendered previews from several exterior angles and from the cockpit. Check dimensions, surface detail, materials, instrument behavior, and moving parts.
- Report what was checked, the reference targets and measured results where applicable, and any checks that could not be completed. Documentation-only changes do not require a build or flight test.
