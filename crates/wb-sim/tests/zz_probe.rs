// TEMP probe, deleted after use.
use std::sync::Arc;
use wb_sim::Sim;
use wb_sim::view::ViewEncoder;

#[test]
fn probe() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scenarios/fixtures");
    let set = wb_content::load_dir(std::path::Path::new(dir)).unwrap();
    let db = wb_content::compile(&set);
    let (header, _) = wb_content::encode(&db, &set.manifest.content_version);
    let yaml = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../scenarios/_p6b-ui-scratch.yaml")).unwrap();
    let mut sim = Sim::from_yaml(&yaml, Arc::new(db), header.content_hash, None).unwrap();
    let mut enc = ViewEncoder::new();
    for _ in 0..70 {
        sim.step();
    }
    let _ = enc.encode(sim.world(), 0).to_vec();
}
