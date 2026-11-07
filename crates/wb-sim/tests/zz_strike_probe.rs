// TEMP probe, deleted after use.
use std::sync::Arc;
use wb_sim::Sim;

#[test]
fn probe() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scenarios/fixtures");
    let set = wb_content::load_dir(std::path::Path::new(dir)).unwrap();
    let db = wb_content::compile(&set);
    let (header, _) = wb_content::encode(&db, &set.manifest.content_version);
    let yaml = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../scenarios/hosted-test.yaml")).unwrap();
    let mut geo = wb_geo::Geo::new();
    for d in ["world", "europe"] {
        geo.add_pack(wb_geo::Pack::open_dir(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../build/packs").join(d)).unwrap()).unwrap();
    }
    let mut sim = Sim::from_yaml(&yaml, Arc::new(db), header.content_hash, Some(std::rc::Rc::new(geo))).unwrap();
    for _ in 0..70 { sim.step(); }
    let t = sim.clock().tick;
    let order: wb_sim::orders::Order = serde_json::from_str(&format!(r#"{{"tick":{t},"side":0,"type":"AssignTask","units":[10],"task":{{"kind":"strike","contact":2,"weapon":"test-asm","count":3}}}}"#)).unwrap();
    println!("submit: {:?}", sim.submit(&order));
    let mut seen = sim.events().len();
    for _ in 0..30000 {
        sim.step();
        let all = sim.events();
        if all.len() < seen { seen = 0; }
        let new: Vec<_> = all[seen..].to_vec();
        seen = all.len();
        for ev in &new {
            let s = serde_json::to_string(ev).unwrap();
            if (s.contains("\"sides\":1") || s.contains("\"sides\":3")) && !s.contains("AutoSlow") && !s.contains("ContactCount") && (s.contains("Weapon") || s.contains("Contact") || s.contains("Task") || s.contains("Damage") || s.contains("Destroyed")) {
                println!("{}", &s[..s.len().min(260)]);
            }
        }
        if sim.clock().tick > 30000 { break; }
    }
}
