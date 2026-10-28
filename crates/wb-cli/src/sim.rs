//! Shared set-up for the commands that run the engine: content, packs,
//! scenarios.

use anyhow::Context;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use wb_content::ContentDb;
use wb_geo::Geo;
use wb_sim::{Scenario, Sim};

/// A content pack in memory and its hash.
pub struct Content {
    pub db: Arc<ContentDb>,
    pub hash: [u8; 32],
}

/// Loads content from a `.wbpack` file, or compiles a content directory the
/// same way `wb content build` does (same bytes, same hash).
pub fn content(path: &Path) -> anyhow::Result<Content> {
    let bytes = if path.extension().is_some_and(|e| e == "wbpack") {
        std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?
    } else {
        let set = crate::load_content(path)?;
        let db = wb_content::compile(&set);
        wb_content::encode(&db, &set.manifest.content_version).1
    };
    let (header, db) = wb_content::decode(&bytes)
        .with_context(|| format!("content pack {} is not valid", path.display()))?;
    Ok(Content {
        db: Arc::new(db),
        hash: header.content_hash,
    })
}

pub fn geo(packs: &[PathBuf]) -> anyhow::Result<Option<Rc<Geo>>> {
    if packs.is_empty() {
        return Ok(None);
    }
    Ok(Some(Rc::new(crate::geo::open(packs)?)))
}

pub fn read_scenario(path: &Path) -> anyhow::Result<Scenario> {
    let yaml = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read scenario {}", path.display()))?;
    Scenario::from_yaml(&yaml).with_context(|| format!("cannot load scenario {}", path.display()))
}

pub fn build(scenario: &Scenario, c: &Content, geo: Option<Rc<Geo>>) -> anyhow::Result<Sim> {
    Sim::new(scenario, c.db.clone(), c.hash, geo).context("cannot build the scenario")
}
