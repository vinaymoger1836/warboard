//! Operations, reports and replay (P7): the Rising Sun golden (G13) run end
//! to end, report totals against the raw events, replay hashes.

use std::sync::Arc;
use wb_sim::Sim;

fn fixtures() -> (Arc<wb_content::ContentDb>, [u8; 32]) {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scenarios/fixtures");
    let set = wb_content::load_dir(std::path::Path::new(dir)).expect("fixtures load");
    let db = wb_content::compile(&set);
    let (header, _) = wb_content::encode(&db, &set.manifest.content_version);
    (Arc::new(db), header.content_hash)
}

fn rising_sun() -> Sim {
    let (db, hash) = fixtures();
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scenarios/golden/rising-sun.yaml");
    let yaml = std::fs::read_to_string(path).expect("scenario");
    Sim::from_yaml(&yaml, db, hash, None).expect("builds")
}

#[test]
fn debug_picture() {
    let mut sim = rising_sun();
    for t in [600u64, 1200, 2400] { sim.run_ticks(t - sim.clock().tick); println!("tick {t}");
    let pics = sim.world().resource::<wb_sim::picture::SidePictures>();
    for c in pics.sides[0].contacts.values() {
        println!("{:?} {:?} {:?} lost={} q={:?} est={:?} src={:?}", c.id, c.domain, c.level, c.lost, c.quality, c.est.map(|e| (e.at.lat_deg(), e.at.lon_deg(), e.a_m)), c.sources.iter().map(|s| s.observer).collect::<Vec<_>>());
    }
    }
}
