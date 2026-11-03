//! Rules of engagement (doc 03 §7, P5): the effective ROE of a unit, the
//! gates, territory, the approval queue and border crossings.
//!
//! - **Inheritance:** unit override → group override → side default. A war
//!   with no ROE in its scenario starts every side at Charlie with
//!   self-defence on.
//! - **Gates** (offensive engagements; self-defence and ordered targets are
//!   handled by `engage`): Alpha fires at any hostile; Bravo fires when the
//!   contact's *estimated* position and its error ellipse (×
//!   `roe.territory_sigma`) are in own coalition territory, or when it
//!   launched at our forces within `return_fire_memory_s`; Charlie fires at
//!   nothing. A blocked opportunity becomes an approval request.
//! - **Territory (ADR-030):** the packs' `territory_owner` when packs are
//!   installed, else the scenario's `test_territory` polygons, else none.
//! - **Approval queue:** one open request per (group, contact); one approval
//!   allows one engagement; a request expires when the contact is expected
//!   to leave the envelope.

use bevy_ecs::prelude::*;
use core::f64::consts::FRAC_PI_4;
use serde::{Deserialize, Serialize};
use wb_content::compiled::{CountryIx, WeaponIx};
use wb_core::geo::{LatLon, destination_point};
use wb_core::math::{atan2, cos, sin, sqrt};
use wb_core::units::rad_to_deg;
use wb_core::{ContactId, UnitId};

use crate::components::{GroupMember, Roe, RoeSetting};
use crate::events::{Decision, EventKind, SlowCause, emit};
use crate::picture::{Contact, ContactIdentity, SidePictures, TypeRef};
use crate::rules::Rules;
use crate::sensors::GeoRes;
use crate::weapons::{Combat, LaunchReason};
use crate::world::{Sides, entity};

/// One test territory polygon (ADR-030): `[lon, lat]` degrees.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TerritoryArea {
    pub country: CountryIx,
    pub polygon: Vec<[f64; 2]>,
}

/// Scenario `test_territory`, saved with the war like `test_terrain`.
#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TestTerritory(pub Vec<TerritoryArea>);

/// Ray casting on lon/lat degrees.
fn inside(poly: &[[f64; 2]], lon: f64, lat: f64) -> bool {
    let mut c = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + n - 1) % n]);
        if (a[1] > lat) != (b[1] > lat) && lon < (b[0] - a[0]) * (lat - a[1]) / (b[1] - a[1]) + a[0]
        {
            c = !c;
        }
    }
    c
}

/// The country whose territory (land, 12 nm, airspace) holds `p`.
pub fn owner(world: &World, rules: &Rules, p: LatLon) -> Option<CountryIx> {
    if let Some(geo) = world.get_non_send::<GeoRes>().and_then(|g| g.0.clone())
        && let Ok(Some(code)) = geo.territory_owner(p)
        && let Some(i) = rules
            .db
            .countries
            .iter()
            .position(|c| rules.db.str(c.id) == code.as_str())
    {
        return Some(CountryIx(i as u32));
    }
    let (lon, lat) = (rad_to_deg(p.lon), rad_to_deg(p.lat));
    world.get_resource::<TestTerritory>().and_then(|t| {
        t.0.iter()
            .find(|a| inside(&a.polygon, lon, lat))
            .map(|a| a.country)
    })
}

/// Whether `p` is territory of `side`'s coalition.
pub fn own_territory(world: &World, rules: &Rules, side: u8, p: LatLon) -> bool {
    owner(world, rules, p).is_some_and(|c| {
        world
            .resource::<Sides>()
            .country_ix
            .get(usize::from(side))
            .is_some_and(|cs| cs.binary_search(&c).is_ok())
    })
}

/// Where the effective ROE of a unit comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum RoeSource {
    Side,
    Group,
    Unit,
}

/// Effective ROE and self-defence of a unit (or a group), with the source
/// of the ROE.
pub fn effective(world: &World, side: u8, unit: UnitId) -> (Roe, RoeSource, bool) {
    let sides = world.resource::<Sides>();
    let s = &sides.sides[usize::from(side)];
    let mut roe = (s.roe, RoeSource::Side);
    let mut sd = s.self_defence;
    let e = entity(world, unit);
    let group = e
        .and_then(|e| world.get::<GroupMember>(e))
        .and_then(|m| entity(world, m.group));
    for (ent, src) in [(group, RoeSource::Group), (e, RoeSource::Unit)] {
        if let Some(r) = ent.and_then(|x| world.get::<RoeSetting>(x)) {
            if let Some(v) = r.roe {
                roe = (v, src);
            }
            if let Some(v) = r.self_defence {
                sd = v;
            }
        }
    }
    (roe.0, roe.1, sd)
}

/// Whether a contact's estimate and its error ellipse, scaled to
/// `roe.territory_sigma`, lie in `side`'s territory (centre and eight points
/// on the ellipse).
fn surely_inside(world: &World, rules: &Rules, side: u8, c: &Contact, now: u64) -> bool {
    let (Some(e), Some((a, b, rot))) = (c.est, c.ellipse(rules, now)) else {
        return false;
    };
    let k = rules.realism().roe.territory_sigma;
    own_territory(world, rules, side, e.at)
        && (0..8).all(|i| {
            let t = f64::from(i) * FRAC_PI_4;
            let (x, y) = (k * a * cos(t), k * b * sin(t));
            let p = destination_point(e.at, rot + atan2(y, x), sqrt(x * x + y * y));
            own_territory(world, rules, side, p)
        })
}

/// What ROE says about an offensive shot at a hostile contact.
pub enum Gate {
    Fire(LaunchReason),
    Ask,
}

pub fn gate(world: &World, rules: &Rules, side: u8, unit: UnitId, c: &Contact, now: u64) -> Gate {
    match effective(world, side, unit).0 {
        Roe::Alpha => Gate::Fire(LaunchReason::WeaponsFree),
        Roe::Bravo => {
            if surely_inside(world, rules, side, c, now) {
                return Gate::Fire(LaunchReason::Territory);
            }
            let memory_ms = (rules.realism().roe.return_fire_memory_s * 1_000.0) as u64;
            let acted = world
                .resource::<Combat>()
                .hostile_acts
                .get(&(side, c.id))
                .is_some_and(|t| now.saturating_sub(*t) <= memory_ms);
            if acted {
                Gate::Fire(LaunchReason::ReturnFire)
            } else {
                Gate::Ask
            }
        }
        Roe::Charlie => Gate::Ask,
    }
}

/// An engagement waiting for the player.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RoeRequest {
    pub id: u32,
    pub side: u8,
    pub group: UnitId,
    pub shooter: UnitId,
    pub contact: ContactId,
    pub weapon: WeaponIx,
    pub pk: f64,
    pub expires_ms: u64,
    pub approved: bool,
}

/// An open request as the Approvals tab shows it: own units, the side's
/// own contact and its label, never the truth behind it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ApprovalView {
    pub request: u32,
    pub group: u32,
    pub group_name: String,
    pub contact: u32,
    /// Type short name at type level, class name at class level, empty below.
    pub contact_type: String,
    pub weapon_type: String,
    pub pk: f64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub expires_ms: u64,
    pub approved: bool,
}

/// The side's open requests, oldest first.
pub fn approvals(world: &World, side: u8) -> Vec<ApprovalView> {
    use wb_content::source::realism::ContactLevel;
    let rules = world.resource::<Rules>();
    let pic = world
        .resource::<SidePictures>()
        .sides
        .get(usize::from(side));
    world
        .resource::<RoeRequests>()
        .open
        .iter()
        .filter(|r| r.side == side)
        .map(|r| {
            let contact_type = pic
                .and_then(|p| p.get(r.contact))
                .map(|c| {
                    if c.level >= ContactLevel::Type {
                        c.type_ref.short_name(rules).to_owned()
                    } else if c.level >= ContactLevel::Class {
                        c.type_ref.class_name(rules)
                    } else {
                        String::new()
                    }
                })
                .unwrap_or_default();
            let group_name = entity(world, r.group)
                .and_then(|e| world.get::<crate::components::Identity>(e))
                .map(|i| i.name.clone())
                .unwrap_or_default();
            ApprovalView {
                request: r.id,
                group: r.group.0,
                group_name,
                contact: r.contact.0,
                contact_type,
                weapon_type: rules
                    .db
                    .str(rules.db.weapons[r.weapon.0 as usize].name)
                    .to_owned(),
                pk: r.pk,
                expires_ms: r.expires_ms,
                approved: r.approved,
            }
        })
        .collect()
}

/// The approval queue (saved).
#[derive(Resource, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RoeRequests {
    pub next: u32,
    pub open: Vec<RoeRequest>,
}

/// Raises a request unless one is open for the group and contact.
#[allow(clippy::too_many_arguments)]
pub fn ask(
    world: &mut World,
    rules: &Rules,
    side: u8,
    group: UnitId,
    shooter: UnitId,
    contact: ContactId,
    weapon: WeaponIx,
    pk: f64,
    expires_ms: u64,
) {
    let q = world.resource::<RoeRequests>();
    if q.open
        .iter()
        .any(|r| r.side == side && r.group == group && r.contact == contact)
    {
        return;
    }
    let id = {
        let mut q = world.resource_mut::<RoeRequests>();
        q.next += 1;
        let id = q.next;
        q.open.push(RoeRequest {
            id,
            side,
            group,
            shooter,
            contact,
            weapon,
            pk,
            expires_ms,
            approved: false,
        });
        id
    };
    let w = &rules.db.weapons[weapon.0 as usize];
    emit(
        world,
        side,
        EventKind::RoeRequest {
            request: id,
            group,
            contact,
            weapon_type: rules.db.str(w.id).to_owned(),
            pk,
            expires_ms,
        },
    );
    emit(
        world,
        side,
        EventKind::AutoSlow {
            reason: format!("Approval requested to engage C{}", contact.0),
            cause: SlowCause::RoeRequest,
        },
    );
}

/// An approved request for this group and contact, taken (one approval =
/// one engagement).
pub fn take_approval(world: &mut World, side: u8, group: UnitId, contact: ContactId) -> bool {
    let mut q = world.resource_mut::<RoeRequests>();
    let Some(i) = q
        .open
        .iter()
        .position(|r| r.side == side && r.group == group && r.contact == contact && r.approved)
    else {
        return false;
    };
    q.open.remove(i);
    true
}

/// Applies a player's decision (the order checked the request is open).
pub fn decide(world: &mut World, side: u8, request: u32, approve: bool) {
    let mut q = world.resource_mut::<RoeRequests>();
    let Some(i) = q
        .open
        .iter()
        .position(|r| r.id == request && r.side == side)
    else {
        return;
    };
    if approve {
        q.open[i].approved = true;
    } else {
        q.open.remove(i);
    }
    emit(
        world,
        side,
        EventKind::RoeDecision {
            request,
            decision: if approve {
                Decision::Approved
            } else {
                Decision::Denied
            },
        },
    );
}

/// Drops requests whose window closed.
pub fn expire(world: &mut World, now: u64) {
    let gone: Vec<(u8, u32)> = {
        let mut q = world.resource_mut::<RoeRequests>();
        let gone = q
            .open
            .iter()
            .filter(|r| r.expires_ms <= now)
            .map(|r| (r.side, r.id))
            .collect();
        q.open.retain(|r| r.expires_ms > now);
        gone
    };
    for (side, request) in gone {
        emit(
            world,
            side,
            EventKind::RoeDecision {
                request,
                decision: Decision::Expired,
            },
        );
    }
}

/// `BorderCrossing` when a hostile contact's estimate enters or leaves own
/// territory (not on its first sighting).
pub fn borders(world: &mut World, rules: &Rules) {
    let n = world.resource::<Sides>().len();
    let mut changes = Vec::new();
    let mut now_inside = Vec::new();
    for side in 0..n as u8 {
        let pics = world.resource::<SidePictures>();
        let Some(pic) = pics.sides.get(usize::from(side)) else {
            continue;
        };
        for c in pic.contacts.values() {
            if c.identity != ContactIdentity::Hostile || matches!(c.type_ref, TypeRef::Weapon(_)) {
                continue;
            }
            let Some(e) = c.est else { continue };
            now_inside.push((side, c.id, own_territory(world, rules, side, e.at)));
        }
    }
    {
        let mut combat = world.resource_mut::<Combat>();
        let mut fresh = std::collections::BTreeMap::new();
        for (side, id, inside) in now_inside {
            if let Some(prev) = combat.inside.get(&(side, id))
                && *prev != inside
            {
                changes.push((side, id, inside));
            }
            fresh.insert((side, id), inside);
        }
        combat.inside = fresh;
    }
    for (side, contact, inbound) in changes {
        emit(world, side, EventKind::BorderCrossing { contact, inbound });
    }
}
