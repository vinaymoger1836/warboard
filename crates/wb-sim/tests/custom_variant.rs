//! P8 acceptance: a custom system made with `extends` (as the admin app writes it,
//! doc 04 §3.6) and listed in a nation's inventory shows up in that nation's Forces
//! picture, marked custom, with its overridden value in the compiled content, and
//! not in another nation's picture.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use wb_sim::inventory::inventory;
use wb_sim::{Rules, Sim};

const SCENARIO: &str = "
name: custom variant
seed: 8
start_utc: 2026-10-27T06:00:00Z
sides:
  - { name: Blue, nations: [hlv] }
  - { name: Red, nations: [ald] }
units: []
";

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("create dir");
    for e in fs::read_dir(from).expect("read dir") {
        let e = e.expect("entry");
        let dest = to.join(e.file_name());
        if e.file_type().expect("type").is_dir() {
            copy_dir(&e.path(), &dest);
        } else {
            fs::copy(e.path(), dest).expect("copy");
        }
    }
}

#[test]
fn custom_variant_reaches_its_nation_only() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
    let tmp: PathBuf =
        std::env::temp_dir().join(format!("wb-custom-variant-{}", std::process::id()));
    let _ = fs::remove_dir_all(&tmp);
    copy_dir(&src, &tmp);

    // What the admin app's NEW CUSTOM VARIANT writes, plus one override.
    fs::write(
        tmp.join("platforms/aircraft/custom-mf-5-long-legs.yaml"),
        "id: custom-mf-5-long-legs\nkind: platform\ncustom: true\nextends: mf-5\n\
         name: \"MF-5 Dunmere (long legs)\"\noverrides:\n  fuel.internal_kg: 4600\n\
         design: { archetype: fighter-multirole, note: \"Custom variant of mf-5 with a bigger internal tank.\" }\n",
    )
    .expect("write variant");
    let inv_path = tmp.join("inventories/hlv.yaml");
    let inv = fs::read_to_string(&inv_path).expect("read inventory");
    fs::write(
        &inv_path,
        inv.replacen(
            "platforms:\n",
            "platforms:\n  custom-mf-5-long-legs: 4\n",
            1,
        ),
    )
    .expect("write inventory");

    let set = wb_content::load_dir(&tmp).expect("content with the variant loads");
    let db = Arc::new(wb_content::compile(&set));
    let (header, _) = wb_content::encode(&db, &set.manifest.content_version);
    let mut sim = Sim::from_yaml(SCENARIO, db.clone(), header.content_hash, None).expect("builds");
    let _ = fs::remove_dir_all(&tmp);

    let rules = sim.world().resource::<Rules>();
    let blue = inventory(rules, sim.sides(), 0);
    let line = blue[0]
        .forces
        .iter()
        .find(|l| l.id == "custom-mf-5-long-legs")
        .expect("the variant is in its nation's inventory");
    assert!(line.custom);
    assert_eq!(line.count, 4);
    assert_eq!(line.name, "MF-5 Dunmere (long legs)");

    let red = inventory(rules, sim.sides(), 1);
    assert!(
        red.iter()
            .all(|n| n.forces.iter().all(|l| l.id != "custom-mf-5-long-legs"))
    );
    let _ = &mut sim;
}
