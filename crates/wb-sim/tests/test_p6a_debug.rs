use std::path::Path;
use std::sync::Arc;
use wb_sim::{Sim, Scenario};

#[test]
fn test_threat_priority_debug() {
    let scn_path = Path::new("../../scenarios/golden/g10-threat-priority.yaml");
    let scn_text = std::fs::read_to_string(scn_path).expect("read scn");
    let scn: Scenario = serde_yaml::from_str(&scn_text).expect("parse scn");
    let content_path = Path::new("../../scenarios/fixtures");
    let raw = wb_content::load_dir(content_path).expect("load dir");
    let db = wb_content::compile(&raw);
    let mut sim = Sim::new(&scn, Arc::new(db), [0; 32], None).expect("build sim");
    println!("Built sim successfully!");
    let mut last_event_len = 0;
    for tick in 1..=150 {
        sim.run_ticks(1);
        let events = sim.events();
        if events.len() > last_event_len {
            for e in &events[last_event_len..] {
                println!("Tick {}: {:?}", tick, e.kind);
            }
            last_event_len = events.len();
        }
    }
}
