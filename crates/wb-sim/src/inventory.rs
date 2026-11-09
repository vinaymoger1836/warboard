//! `inventory()` for the Forces panel (doc 06 §7, doc 03 §4): the content
//! inventories of one side's own nations. Content only, never the war state,
//! and never another side's nations (CLAUDE.md rule 1).

use serde::Serialize;
use wb_content::compiled::{Design, ForceIx};

use crate::rules::Rules;
use crate::world::Sides;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct NationInventory {
    /// Nation id upper-case, `ALD`.
    pub nation: String,
    pub name: String,
    /// Inventory note.
    pub note: String,
    pub forces: Vec<InventoryLine>,
    pub munitions: Vec<InventoryLine>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct InventoryLine {
    /// Content id, `mf-4`.
    pub id: String,
    pub name: String,
    /// Class archetype name, empty when the entry names none.
    pub class_name: String,
    /// `aircraft/fighter`, `system/site`, `aam`.
    pub category: String,
    pub count: u32,
    /// A user-made variant (`custom: true`, doc 04 §3.6).
    pub custom: bool,
    /// Number at preset bases in the basing table (forces only).
    pub based: u32,
}

fn enum_name<T: Serialize>(v: &T) -> String {
    serde_yaml::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Inventories of the nations on `side`, in the side's nation order.
pub fn inventory(rules: &Rules, sides: &Sides, side: u8) -> Vec<NationInventory> {
    let db = &rules.db;
    let class = |d: &Design| d.class_name.map(|c| db.str(c).to_owned()).unwrap_or_default();
    let Some(s) = sides.sides.get(usize::from(side)) else {
        return Vec::new();
    };
    s.nations
        .iter()
        .filter_map(|code| {
            let (nix, nation) = db
                .nations
                .iter()
                .enumerate()
                .find(|(_, n)| db.str(n.id) == code)?;
            let inv = db.inventories.iter().find(|i| i.nation.0 as usize == nix);
            let forces = inv
                .map(|inv| {
                    inv.platforms
                        .iter()
                        .map(|(f, count)| {
                            let based = inv
                                .basing
                                .iter()
                                .filter(|b| b.force == *f)
                                .map(|b| b.count)
                                .sum();
                            let (id, name, design, category) = match f {
                                ForceIx::Platform(p) => {
                                    let p = &db.platforms[p.0 as usize];
                                    (p.id, p.name, &p.design, enum_name(&p.category))
                                }
                                ForceIx::System(x) => {
                                    let x = &db.systems[x.0 as usize];
                                    (
                                        x.id,
                                        x.name,
                                        &x.design,
                                        format!("system/{}", enum_name(&x.deploy_as)),
                                    )
                                }
                            };
                            InventoryLine {
                                id: db.str(id).to_owned(),
                                name: db.str(name).to_owned(),
                                class_name: class(design),
                                category,
                                count: *count,
                                custom: design.custom,
                                based,
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            let munitions = inv
                .map(|inv| {
                    inv.munitions
                        .iter()
                        .map(|(w, count)| {
                            let w = &db.weapons[w.0 as usize];
                            InventoryLine {
                                id: db.str(w.id).to_owned(),
                                name: db.str(w.name).to_owned(),
                                class_name: class(&w.design),
                                category: enum_name(&w.category),
                                count: *count,
                                custom: w.design.custom,
                                based: 0,
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(NationInventory {
                nation: code.to_uppercase(),
                name: db.str(nation.name).to_owned(),
                note: inv.map(|i| db.str(i.design.note).to_owned()).unwrap_or_default(),
                forces,
                munitions,
            })
        })
        .collect()
}
