
<div align="center">

# FableFlight

### An Open-World Flight Simulator Built in Pure Rust

Explore the skies with real-world weather, live air traffic, and an immersive open-world flying experience.

*Created with Claude Fable 5.2 & Opus 5.5*

<br>

[![Discord](https://img.shields.io/badge/Discord-5865F2?style=for-the-badge&logo=discord&logoColor=white)](https://discord.gg/9akYzMRBc6)
[![Rust](https://img.shields.io/badge/Rust-CE422B?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org/)
<br>
</div>
<div align="center">
  <video
    src="https://github.com/user-attachments/assets/4f8833da-13c0-4d20-a50b-c0acf2d88df1"
    width="800"
    controls>
  </video>
</div>

# Compiling
* Install rust
```
cargo build --release
```

running
```
cargo run --release
```

# Language
The interface is in English or French. It follows the system language, can be changed in Settings (start screen or pause menu), and is remembered. For one run: `cargo run --release -- --lang fr`.

# Joysticks, pedals and key bindings
Any joystick, throttle, rudder pedals or wheel can be used: in Bindings (pause menu, or Settings on the start screen), bind each axis by moving it the way that means "more" (nose up, right roll, right rudder, full throttle, brake pressed) and each button to any action. Each axis has an Invert switch and a dead zone. Presets set a device up in one click; the Turtle Beach VelocityOne Flightstick and rudder pedals get theirs on their own the first time they are plugged in. Keyboard keys can be moved too. Everything is kept in `bindings.json` in the config folder.

# Aircraft
The Cessna 172RG, the Robin DR 400/180 Régent, the 747, the F/A-18, the F-4, the F-22, Concorde, the SR-71 and Starship. The Robin is the French wooden four-seater and glider tug: bubble canopy, the cranked Jodel wing, an all-moving tailplane, fixed gear and 60° flaps (`--aircraft dr400`); 6 slides its canopy open and shut. Like the Cessna it is built in code (`src/aircraft/dr400`), in a made-up classic Robin livery.

# Fuel and engine start
The engines burn fuel as they work (the Cessna some 30 l/h at 75% power, the 747 about 10 t/h in the cruise) and the fuel weighs: full tanks fly heavier than empty ones. Choose how much to take on the start screen (Flight); the HUD, the flight panel (fuel, flow, endurance) and the cockpit gauges show what is left, with LOW FUEL / NO FUEL warnings, and a dry engine stops. Start on a runway with the engine off ("cold start", or `--cold`) and start it by hand (I magnetos, Page Up mixture, hold O for the starter; on the jets I is the fuel controls and O engages the starters) or with F4, which also shuts it down. A windmilling engine with fuel and ignition catches again in the air; shut down on the ground, compression stops it within seconds.

# Mass and balance
The fuel and the payload (crew, passengers, baggage: set on the start screen) make the aircraft heavier or lighter and move its centre of gravity, shown against its limits in % of the mean aerodynamic chord (start screen, flight panel; CG LIMIT on the HUD). Loaded aft, the nose is lighter and quicker to answer and a gust takes longer to settle; forward, it is stable but nose-heavy and the elevator runs out sooner (a harder flare). A Cessna with four aboard and full baggage is out of its aft limit, as the real one is.

# Electrics and failures
A battery behind the master switch (Ins) and an alternator (the jets' generators) that charges it once the engine turns fast enough: the starter, the radios, the electric gauges and the lights need power, the engine itself runs on its magnetos. The pause menu's Failures page sets failures by hand or at random (about every N minutes): engine, alternator, flat battery, vacuum pump (the attitude and heading gyros run down), blocked pitot (the airspeed indicator turns into an altimeter) or static port (altimeter and VSI freeze), fuel leak. The flight panel shows the volts, battery and current, and the HUD warns LOW VOLTS and VACUUM.

# In-flight map

A round minimap in the lower-left corner, stacked above the info panel, follows the aircraft; ` (backquote, the tilde key) shows or hides it. Choose **Satellite** or **Map** (a light road map), and **North** to keep north at the top or **Heading** to keep the nose pointing up. The controls, hints and status messages follow the game language (English/French). Place names use the corresponding translation when supplied by the map data, falling back to the local name; changing language also updates already loaded labels. City, town and village names stay upright on both backgrounds, with smaller places appearing as you zoom in and overlapping labels hidden. Use **+ / -** or the mouse wheel over the map to zoom. The compact toolbar sits under the map: click the orientation/background buttons to toggle them, or **x** to collapse it to a **Map** button. Drag the knob on the upper-right of the rim to resize it (240-640 logical pixels across, shrunk to fit between the HUD panels); the size is remembered and can also be set in the pause menu Settings. The map follows HUD visibility and remains visible beside the flight panel and temporarily hides while the pause menu is open. Visibility, background, orientation and zoom are remembered.

Runways are highlighted in cyan using the loaded 3D airfield geometry when available, falling back to database endpoint coordinates for distant airfields. Near a loaded airfield, the active runway gets orange arrival arrows and a corridor on the map. In flight on its arrival side, orange gates show a visual 3-degree descent path toward the runway aiming point. This optional visual aid uses the game traffic runway selection (or wind-based selection), not an ATC clearance or terrain-clearance calculation. Toggle it in pause Settings; the choice is remembered.

The minimap shares the terrain imagery and OpenFreeMap vector data caches. It loads tiles in the background, with at most one imagery and one vector request in flight, 64 satellite textures and 32 road-map tiles in memory. The road map is rasterized once per tile on a worker, not every frame, and does not request satellite images. Neither mode renders the 3D world a second time. Lower-resolution satellite tiles stand in while detail loads. Offline it uses cached data; missing imagery is indicated, and place names require cached vector tiles. The map is unavailable beyond 85 degrees latitude. Map and label data: [OpenFreeMap](https://openfreemap.org/), [OpenMapTiles](https://openmaptiles.org/), [OpenStreetMap contributors](https://www.openstreetmap.org/copyright).

# Radio navigation
VORs and NDBs from [OurAirports](https://ourairports.com/data/) (public domain), and an ILS on every paved runway end of 1500 m or more at medium and large airports. Tune NAV 1 (VOR or ILS) and the ADF on the flight panel's Radio page (Tab); J shows the instruments: a course deviation indicator with TO/FROM, the glide slope on an ILS and DME, and an ADF needle. [ and ] turn the OBS. Starting on a final approach tunes the runway's ILS. The altimeter is set to the QNH (from the METAR when the weather is real) unless you set it yourself; X sets it back. Courses are true: the world has no magnetic variation. `assets/mknav.py` rebuilds `assets/navaids.tsv.gz` from OurAirports' `navaids.csv`.

# More aircraft: FlightGear's collection
Click the aircraft picture on the start screen (or **All aircraft**) to see every aircraft you can fly as a gallery; **Get more…** opens [FlightGear's aircraft collection](https://wiki.flightgear.org/FGAddon) (its 2020 catalogue, about 650 aircraft). Pick one to read its files from FGAddon: its flight model, author, the license its JSBSim file states (else FGAddon's GNU GPL v2 or later), and what it becomes here; the ones that can't be flown are hidden once read (tick **Show all**). **Download** takes its package from a FlightGear mirror and unpacks it, without sounds, scripts and splash screens, into `~/.local/share/fablegame/aircraft/<folder>/` (with a `source.json` recording the source, revision and license), and it joins the list. Nothing from FlightGear ships with the game.

A downloaded aircraft is **drawn with its own 3D model** (src/fgmodel.rs): the AC3D meshes and textures (PNG, JPEG, SGI .rgb) its model file names, with sub-models placed by their offsets. Its animations become our moving parts where we drive the same thing: control surfaces, flaps, slats, spoilers, gear retraction, steering, oleo compression, wheels and propellers. Others are taken at rest, and select animations hide what is off at rest or swap propeller blades for their blur. It sits on its own landing gear (the JSBSim contact points), sees from its pilot's view, and gets its picture from its previews.

It is not flown by JSBSim or YASim: its flight model is *fitted* to ours (src/hangar.rs). Its mass, inertia, wing, lift and drag curves, flaps, engines and fuel go into a `Spec` built on the closest of our types, which lends its instruments, sounds and damage rules: propeller aircraft the Cessna's, fighters and anything with afterburners the Hornet's, other jets of 25 t or more the 747's. A JSBSim aircraft's curves are read from its own aerodynamic tables. A YASim aircraft (most of the catalogue) has none: YASim builds it from its geometry and solves for the lift and drag that fly the approach and cruise its file gives, and src/yasim.rs does the same for a polar, with its mass spread over the airframe around its ballast, engines and fuel for the centre of gravity and inertia. Helicopters and autogyros, rockets, gliders, balloons and electric aircraft can't be flown yet.

**A word of caution:** the collection's aircraft are made by FlightGear's community and vary a lot. Some are not of high quality (rough models, odd files), and since each is fitted to our flight model rather than flown by its own, some don't fly realistically here. From the command line: `--fg-install SR22T` downloads, `--aircraft fg:sr22t` flies it, `--fg-survey [TEXT]` reads catalogue entries (those named JSBSim, or matching TEXT) and prints which can be flown, `--jsbsim-fit FILE...` prints what JSBSim (or YASim) files become, and `--fly-check KEY` flies any aircraft in an empty world (take-off, top speed, stall).

# Comparing with a JSBSim flight model
`cargo run --release -- --jsbsim-compare path/to/c172p.xml [--aircraft 747]` reads a JSBSim aircraft file (the XML flight models JSBSim and FlightGear use; none ship with the game) and prints how its aerodynamics compare with one of ours: geometry, mass and inertia, the lift and drag curves clean and with flaps, stall, best glide, the roll rate at full aileron, and the `Spec` values that would match its curves, as a start for tuning by hand.

# Live air traffic
Real aircraft around you, as reported by ADS-B receivers (data from [adsb.lol](https://adsb.lol), ODbL), fly in the sky with tags (callsign, type, altitude, speed, distance); the flight panel (Tab) lists the nearest. It can be switched off in the pause menu's Settings.

# Missiles
The F/A-18C carries its two wingtip Sidewinders. Press **3** to set Master Arm, **4** to find and lock a target (press it again to step to the next one), and **5** to fire. On a gamepad, hold **Y** and press **left** on the d-pad to arm, **Y** + **right** to lock or cycle, and release **Y** on its own at a locked target to fire. The seeker looks about 45° either side of the nose for airborne aircraft — the real ADS-B traffic and airport departures and arrivals — and needs a few seconds to build a lock; the HUD shows `TRACK` until it is solid, then `LOCK`. Armed with no lock, firing says `NO LOCK`; with Master Arm off it says `MASTER ARM OFF`. The missile leaves the rail, boosts, follows the target's heat and detonates on a proximity fuse. The rail stays, the empty wingtip is visible, and the second missile is still there. The keys can be moved on the Bindings page.

# Real weather
The weather follows the real conditions where you fly: the nearest METAR within 60 km ([aviationweather.gov](https://aviationweather.gov)) sets the wind, gusts, visibility, cloud layers and thunderstorms; with no airport near, [Open-Meteo](https://open-meteo.com)'s model stands in. It is looked up again every 10 minutes or 30 km, and the sky eases into each report. Switch it off in the weather settings (start screen, flight panel or F3) to set the weather by hand.

The clock can follow real time too ("Real time" above the day sliders): the sun and the date are those where you fly, now (solar time, so the local hour can differ from the legal one by up to an hour or so). Pressing T / Y to set the time by hand leaves real time.

Settings (language, sound, camera, HUD, anti-aliasing) are kept in `settings.json` in the config folder (`~/.config/fablegame` on Linux).

# AI copilot (MCP)
The simulator can host an AI copilot through the [Model Context Protocol](https://modelcontextprotocol.io). Build the game and the small stdio bridge, run the game normally, and point your AI client at `target/release/fableflight-mcp`:

```sh
cargo build --release
./target/release/fablegame
```

For OpenCode, add a local server to `opencode.json` (or use `opencode mcp add fableflight -- /absolute/path/to/fableflight-mcp`):

```jsonc
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "servers": {
      "fableflight": {
        "type": "local",
        "command": ["/absolute/path/to/fableflight-mcp"]
      }
    }
  }
}
```

For Claude Code: `claude mcp add fableflight /absolute/path/to/fableflight-mcp`. Claude Desktop uses the same command in its `mcpServers` configuration. The bridge attaches to the running game over `127.0.0.1:42761`; start a flight, then ask the AI to read the aircraft state, tune radios, set flaps or weather, or take a screenshot. `--mcp-port PORT` changes the game's port and `FABLEFLIGHT_MCP_PORT` tells the bridge where to look. `--no-mcp` disables the service.

The copilot can read the aircraft, navigation, weather, traffic and airports, and can set the throttle, flaps, gear, speedbrake, parking brake, mixture, trim, engine state, radios, weather, time and the aircraft's position. It cannot move the stick: attitude stays with the human pilot. See [docs/mcp.md](docs/mcp.md) for the full tool list, units and examples.

# Contributions
Please contribute anything if you want to! Improvements to flight sim realism, graphics, optimization, UI, and especially plane models are needed! (((=

I personally recommend using Opus 5.5 high for most tasks. If you don't have a routine for conserving tokens a general rule of thumb is to give models specific tasks (e.g. decompose the cockpit and plane body into different prompts) and keep conversations to one prompt, starting a new chat if refinement is needed.
