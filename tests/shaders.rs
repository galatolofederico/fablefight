//! Parses and validates every shader without needing a GPU.
fn check(name: &str, src: &str) -> bool {
    match naga::front::wgsl::parse_str(src) {
        Err(e) => {
            eprintln!("--- {name}\n{}", e.emit_to_string(src));
            false
        }
        Ok(m) => match naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all()).validate(&m) {
            Err(e) => {
                eprintln!("--- {name}\n{}", e.emit_to_string(src));
                false
            }
            Ok(_) => true,
        },
    }
}

/// A minimal string table and the S_ names the deck and cockpit pages use, so
/// plane.wgsl can be validated without the binary's generated table.
fn string_stubs() -> String {
    let mut out = String::from("const STRS = array<vec4u, 2>(vec4u(0u, 0u, 0u, 0u), vec4u(0u, 0u, 0u, 0u));\n");
    let mut names = std::collections::BTreeSet::new();
    for src in [include_str!("../src/shaders/deck747.wgsl"), include_str!("../src/shaders/pit18.wgsl")] {
        for tok in src.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
            if tok.len() > 2 && tok.starts_with("S_") {
                names.insert(tok);
            }
        }
    }
    for n in names {
        out += &format!("const {n}: u32 = 1u;\n");
    }
    out
}

#[test]
fn aggressor_livery_and_radar_pages_validate() {
    let common = include_str!("../src/shaders/common.wgsl");
    let plane = include_str!("../src/shaders/plane.wgsl");
    let pit = include_str!("../src/shaders/pit18.wgsl");
    // The branches this phase adds: the enemy atlas offset in the paint, and
    // the radar page and RWR driven by the real contacts instead of hashes.
    assert!(plane.contains("fn enemy()") && plane.contains("fn enemy_layer()"));
    assert!(plane.contains("let el = enemy_layer();"));
    assert!(pit.contains("RD.contacts[i]"));
    assert!(!pit.contains("let threats = array<vec3f, 3>"));
    let src = format!("{common}\n{plane}\n{}\n{}\n{pit}", string_stubs(), include_str!("../src/shaders/deck747.wgsl"));
    let m = naga::front::wgsl::parse_str(&src).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&src)));
    if let Err(e) = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all()).validate(&m) {
        panic!("{}", e.emit_to_string(&src));
    }
}

#[test]
fn shaders_validate() {
    let common = include_str!("../src/shaders/common.wgsl");
    // Modules that draw the sky get sky.wgsl too, as in render.rs.
    let sky = include_str!("../src/shaders/sky.wgsl");
    let mut ok = check("noise3d", include_str!("../src/shaders/noise3d.wgsl"));
    ok &= check("resolve", include_str!("../src/shaders/resolve.wgsl"));
    ok &= check("detail", include_str!("../src/shaders/detail.wgsl"));
    ok &= check("ocean", include_str!("../src/shaders/ocean.wgsl"));
    ok &= check("shore", include_str!("../src/shaders/shore.wgsl"));
    for (name, src) in [
        ("lut", include_str!("../src/shaders/lut.wgsl")),
        ("terrain", include_str!("../src/shaders/terrain.wgsl")),
        ("photo", include_str!("../src/shaders/photo.wgsl")),
        ("post", include_str!("../src/shaders/post.wgsl")),
        ("particles_sim", include_str!("../src/shaders/particles_sim.wgsl")),
        ("particles_draw", include_str!("../src/shaders/particles_draw.wgsl")),
        ("roads", include_str!("../src/shaders/roads.wgsl")),
        ("trees", include_str!("../src/shaders/trees.wgsl")),
        ("vol", include_str!("../src/shaders/vol.wgsl")),
        ("ao", include_str!("../src/shaders/ao.wgsl")),
        ("taa", include_str!("../src/shaders/taa.wgsl")),
        ("airfield", include_str!("../src/shaders/airfield.wgsl")),
        ("lights", include_str!("../src/shaders/lights.wgsl")),
    ] {
        ok &= check(name, &format!("{common}\n{src}"));
    }
    // As in render.rs, buildings and instanced solids share the facade shading.
    let facade = include_str!("../src/shaders/facade.wgsl");
    for (name, src) in [("buildings", include_str!("../src/shaders/buildings.wgsl")), ("inst", include_str!("../src/shaders/inst.wgsl"))] {
        ok &= check(name, &format!("{common}\n{facade}\n{src}"));
    }
    for (name, src) in [("composite", include_str!("../src/shaders/composite.wgsl")), ("clouds", include_str!("../src/shaders/clouds.wgsl"))] {
        ok &= check(name, &format!("{common}\n{sky}\n{src}"));
    }
    // plane.wgsl takes the 747 deck's string table: checked in b747::deck's tests.
    assert!(ok, "shader errors above");
}
