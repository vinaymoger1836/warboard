//! Validated source → [`ContentDb`]: SI conversion, string interning and
//! typed indices. Pure and deterministic: the same content always gives the
//! same database, byte for byte once encoded.

use crate::compiled::{self as c, ContentDb, StrId};
use crate::load::ContentSet;
use crate::source::{self as s, Entry, Kind};
use std::collections::BTreeMap;
use wb_core::units::{deg_to_rad, h_to_s, km_to_m, kmh_to_mps, kt_to_mps, min_to_s, nm_to_m};

/// kg/h → kg/s.
fn kg_h_to_kg_s(v: f64) -> f64 {
    v / 3_600.0
}

/// kg/min → kg/s.
fn kg_min_to_kg_s(v: f64) -> f64 {
    v / 60.0
}

/// Strings in first-use order; the order is fixed by the compile order.
#[derive(Default)]
struct Strings {
    map: BTreeMap<String, u32>,
    list: Vec<String>,
}

impl Strings {
    fn id(&mut self, s: &str) -> StrId {
        if let Some(&i) = self.map.get(s) {
            return StrId(i);
        }
        let i = u32::try_from(self.list.len()).expect("fewer than 2^32 strings");
        self.map.insert(s.to_owned(), i);
        self.list.push(s.to_owned());
        StrId(i)
    }

    fn ids(&mut self, v: &[String]) -> Vec<StrId> {
        v.iter().map(|x| self.id(x)).collect()
    }
}

/// Position of each id within its kind's list (lists are sorted by id).
struct Indices(BTreeMap<String, u32>);

impl Indices {
    fn get(&self, id: &str) -> u32 {
        *self
            .0
            .get(id)
            .unwrap_or_else(|| panic!("validated reference `{id}`"))
    }
}

struct Compiler<'a> {
    st: Strings,
    ix: BTreeMap<Kind, Indices>,
    set: &'a ContentSet,
}

/// Compiles a validated content set.
pub fn compile(set: &ContentSet) -> ContentDb {
    let mut ix = BTreeMap::new();
    for kind in Kind::ALL {
        let ids = set
            .entries
            .iter()
            .filter(|e| e.kind == kind)
            .enumerate()
            .map(|(i, e)| (e.id.clone(), u32::try_from(i).expect("small")))
            .collect();
        ix.insert(kind, Indices(ids));
    }
    let mut cx = Compiler {
        st: Strings::default(),
        ix,
        set,
    };
    let mut db = ContentDb {
        strings: Vec::new(),
        countries: Vec::new(),
        platforms: Vec::new(),
        systems: Vec::new(),
        sensors: Vec::new(),
        weapons: Vec::new(),
        ew_suites: Vec::new(),
        countermeasures: Vec::new(),
        datalinks: Vec::new(),
        loadouts: Vec::new(),
        bases: Vec::new(),
        inventories: Vec::new(),
        doctrines: Vec::new(),
        realism: Vec::new(),
    };
    // Entries are sorted by id, so each list comes out sorted by id.
    for e in &set.entries {
        let custom = e.custom;
        match &e.entry {
            Entry::Country(x) => db.countries.push(cx.country(x, custom)),
            Entry::Platform(x) => db.platforms.push(cx.platform(x, custom)),
            Entry::System(x) => db.systems.push(cx.system(x, custom)),
            Entry::Sensor(x) => db.sensors.push(cx.sensor(x, custom)),
            Entry::Weapon(x) => db.weapons.push(cx.weapon(x, custom)),
            Entry::EwSuite(x) => db.ew_suites.push(cx.ew_suite(x, custom)),
            Entry::Countermeasure(x) => db.countermeasures.push(cx.countermeasure(x, custom)),
            Entry::Datalink(x) => db.datalinks.push(cx.datalink(x, custom)),
            Entry::Loadout(x) => db.loadouts.push(cx.loadout(x, custom)),
            Entry::Base(x) => db.bases.push(cx.base(x, custom)),
            Entry::Inventory(x) => db.inventories.push(cx.inventory(x, custom)),
            Entry::Doctrine(x) => db.doctrines.push(cx.doctrine(x, custom)),
            Entry::Realism(x) => db.realism.push(cx.realism(x, custom)),
        }
    }
    db.strings = cx.st.list;
    db
}

impl Compiler<'_> {
    fn ix(&self, kind: Kind, id: &str) -> u32 {
        self.ix[&kind].get(id)
    }

    fn provenance(&mut self, p: &s::Provenance, custom: bool) -> c::Provenance {
        c::Provenance {
            confidence: p.confidence,
            custom,
            estimates: self.st.ids(&p.estimates),
            sources: p
                .sources
                .iter()
                .map(|x| c::Source {
                    title: self.st.id(&x.title),
                    url: self.st.id(&x.url),
                    accessed: self.st.id(&x.accessed),
                })
                .collect(),
            notes: p.notes.as_deref().map(|n| self.st.id(n)),
        }
    }

    fn country(&mut self, x: &s::Country, custom: bool) -> c::Country {
        c::Country {
            id: self.st.id(&x.id),
            name: self.st.id(&x.name),
            iso2: self.st.id(&x.iso2),
            iso3: self.st.id(&x.iso3),
            flag: self.st.id(&x.flag),
            doctrine: c::DoctrineIx(self.ix(Kind::Doctrine, &x.doctrine)),
            datalinks: x
                .datalinks
                .iter()
                .map(|d| c::DatalinkIx(self.ix(Kind::Datalink, d)))
                .collect(),
            provenance: self.provenance(&x.provenance, custom),
        }
    }

    fn platform(&mut self, x: &s::Platform, custom: bool) -> c::Platform {
        let m = &x.mobility;
        c::Platform {
            id: self.st.id(&x.id),
            name: self.st.id(&x.name),
            category: x.category,
            domain: x.category.domain(),
            target_class: x.target_class,
            crew: x.crew,
            mobility: c::Mobility {
                cruise_speed_mach: m.cruise_speed_mach,
                max_speed_mach: m.max_speed_mach,
                ceiling_m: m.ceiling_m,
                climb_rate_mps: m.climb_rate_mps,
                sustained_g: m.sustained_g,
                min_runway_m: m.min_runway_m,
                cruise_speed_mps: m.cruise_speed_kt.map(kt_to_mps),
                max_speed_mps: m.max_speed_kt.map(kt_to_mps),
                turn_radius_m: m.turn_radius_m,
                road_speed_mps: m.road_speed_kmh.map(kmh_to_mps),
                offroad_speed_mps: m.offroad_speed_kmh.map(kmh_to_mps),
            },
            signature: c::Signature {
                rcs_m2: x.signature.rcs_m2.clone(),
                external_stores_rcs_multiplier: x
                    .signature
                    .external_stores_rcs_multiplier
                    .unwrap_or(1.0),
                ir: x.signature.ir.as_ref().map(|i| c::IrSignature {
                    cruise: i.cruise,
                    afterburner: i.afterburner,
                }),
                acoustic_db: x.signature.acoustic_db,
                visual_size_m: x.signature.visual_size_m,
            },
            fuel: x.fuel.as_ref().map(|f| c::Fuel {
                internal_kg: f.internal_kg,
                burn_kg_s: f.burn_kg_h.as_ref().map(|b| c::FuelBurn {
                    cruise: kg_h_to_kg_s(b.cruise),
                    military: b.military.map(kg_h_to_kg_s),
                    afterburner: b.afterburner.map(kg_h_to_kg_s),
                }),
                range_m: f.range_nm.map(nm_to_m),
                aar: f.aar,
            }),
            tanker: x.tanker.as_ref().map(|t| c::Tanker {
                offload_kg: t.offload_kg,
                transfer_rate_kg_s: kg_min_to_kg_s(t.transfer_rate_kg_min),
                systems: t.systems.clone(),
            }),
            stations: x
                .stations
                .iter()
                .map(|st| c::Station {
                    id: self.st.id(&st.id),
                    kind: st.kind,
                    categories: self.st.ids(&st.categories),
                    max_kg: st.max_kg,
                    capacity: st.capacity,
                })
                .collect(),
            sensors: x
                .sensors
                .iter()
                .map(|m| c::SensorMount {
                    sensor: c::SensorIx(self.ix(Kind::Sensor, m.sensor())),
                    height_m: m.height_m().unwrap_or(0.0),
                })
                .collect(),
            ew: x
                .ew
                .iter()
                .map(|e| c::EwSuiteIx(self.ix(Kind::EwSuite, e)))
                .collect(),
            countermeasures: x
                .countermeasures
                .iter()
                .map(|(id, n)| (c::CountermeasureIx(self.ix(Kind::Countermeasure, id)), *n))
                .collect(),
            datalinks: x
                .datalinks
                .iter()
                .map(|d| c::DatalinkIx(self.ix(Kind::Datalink, d)))
                .collect(),
            durability: match &x.durability {
                s::platform::Durability::Aircraft(k) => c::Durability::Aircraft {
                    p_kill_on_hit: k.p_kill_on_hit,
                },
                s::platform::Durability::Ship(d) => c::Durability::Ship {
                    damage_points: d.damage_points,
                },
                s::platform::Durability::Ground(d) => c::Durability::Ground {
                    damage_points: d.damage_points,
                },
            },
            turnaround_s: x.turnaround_min.map(min_to_s),
            provenance: self.provenance(&x.provenance, custom),
        }
    }

    fn system(&mut self, x: &s::System, custom: bool) -> c::System {
        c::System {
            id: self.st.id(&x.id),
            name: self.st.id(&x.name),
            deploy_as: x.deploy_as,
            setup_s: min_to_s(x.setup_min),
            teardown_s: x.teardown_min.map(min_to_s),
            components: x
                .components
                .iter()
                .map(|k| c::SystemComponent {
                    platform: c::PlatformIx(self.ix(Kind::Platform, &k.platform)),
                    count: k.count,
                    role: k.role,
                    magazine: k
                        .magazine
                        .iter()
                        .map(|(w, n)| (c::WeaponIx(self.ix(Kind::Weapon, w)), *n))
                        .collect(),
                })
                .collect(),
            emcon_default: x.emcon_default,
            iads_capable: x.iads_capable,
            provenance: self.provenance(&x.provenance, custom),
        }
    }

    fn sensor(&mut self, x: &s::Sensor, custom: bool) -> c::Sensor {
        c::Sensor {
            id: self.st.id(&x.id),
            name: self.st.id(&x.name),
            sensor_type: x.sensor_type,
            role: x.role.clone(),
            radar: x.radar.as_ref().map(|r| c::Radar {
                band: r.band,
                r_ref_m: km_to_m(r.r_ref_km),
                sigma_ref_m2: r.sigma_ref_m2,
                max_range_m: r.max_range_km.map(km_to_m),
                scan_period_s: r.scan_period_s,
                field_of_regard_rad: deg_to_rad(r.field_of_regard_deg),
                clutter_rejection: r.clutter_rejection,
                pulse_doppler: r.pulse_doppler,
                eccm_db: r.eccm_db,
                erp_class_dbw: r.erp_class_dbw,
                fire_channels: r.fire_channels,
            }),
            esm: x.esm.as_ref().map(|e| c::Esm {
                bands: e.bands.clone(),
                r_ref_m: km_to_m(e.r_ref_km),
                erp_ref_dbw: e.erp_ref_dbw,
                bearing_sigma_rad: deg_to_rad(e.bearing_sigma_deg),
                field_of_regard_rad: deg_to_rad(e.field_of_regard_deg),
            }),
            irst: x.irst.as_ref().map(|i| c::Irst {
                r_ref_m: km_to_m(i.r_ref_km),
                i_ref: i.i_ref,
                field_of_regard_rad: deg_to_rad(i.field_of_regard_deg),
                scan_period_s: i.scan_period_s,
            }),
            eo_ir: x.eo_ir.as_ref().map(|e| c::EoIr {
                max_range_m: km_to_m(e.max_range_km),
                gsd_at_10km_m: e.gsd_at_10km_m,
                night_capable: e.night_capable,
                field_of_regard_rad: deg_to_rad(e.field_of_regard_deg),
            }),
            sonar: x.sonar.as_ref().map(|so| c::Sonar {
                passive_r_ref_m: km_to_m(so.passive_r_ref_km),
                noise_ref_db: so.noise_ref_db,
                active_range_m: so.active_range_km.map(km_to_m),
            }),
            provenance: self.provenance(&x.provenance, custom),
        }
    }

    fn weapon(&mut self, x: &s::Weapon, custom: bool) -> c::Weapon {
        let p = &x.profile;
        c::Weapon {
            id: self.st.id(&x.id),
            name: self.st.id(&x.name),
            category: x.category,
            targets: x.targets.clone(),
            target_class: x.target_class,
            profile: c::FlightProfile {
                kind: p.kind,
                boost_s: p.boost_s,
                sustain_s: p.sustain_s,
                max_speed_mach: p.max_speed_mach,
                coast_tau_s: p.coast_tau_s.clone(),
                min_speed_mach: p.min_speed_mach,
                max_range_m: km_to_m(p.max_range_km),
                min_range_m: km_to_m(p.min_range_km),
                max_g: p.max_g,
                cruise_alt_m: p.cruise_alt_m,
                terminal_alt_m: p.terminal_alt_m,
            },
            guidance: c::Guidance {
                midcourse: x.guidance.midcourse.clone(),
                terminal: x.guidance.terminal,
                terminal_secondary: x.guidance.terminal_secondary.clone(),
                seeker_range_m: x.guidance.seeker_range_km.map(km_to_m),
                seeker_eccm: x.guidance.seeker_eccm,
            },
            warhead_kg: x.warhead_kg,
            warhead: x.warhead,
            pk_base: x.pk_base.clone(),
            countermeasure_resistance: x.countermeasure_resistance.clone(),
            cep_m: x.cep_m,
            lethal_radius_m: x.lethal_radius_m,
            mass_kg: x.mass_kg,
            station_category: self.st.ids(&x.station_category),
            internal_carriage: x.internal_carriage,
            rounds_per_cell: x.rounds_per_cell,
            engage_on_remote: x.engage_on_remote,
            envelope: x.envelope.clone(),
            provenance: self.provenance(&x.provenance, custom),
        }
    }

    fn ew_suite(&mut self, x: &s::EwSuite, custom: bool) -> c::EwSuite {
        c::EwSuite {
            id: self.st.id(&x.id),
            name: self.st.id(&x.name),
            ew_type: x.ew_type,
            bands: x.bands.clone(),
            j_db: x.j_db,
            max_range_m: km_to_m(x.max_range_km),
            simultaneous_targets: x.simultaneous_targets,
            field_of_regard_rad: deg_to_rad(x.field_of_regard_deg),
            provenance: self.provenance(&x.provenance, custom),
        }
    }

    fn countermeasure(&mut self, x: &s::Countermeasure, custom: bool) -> c::Countermeasure {
        c::Countermeasure {
            id: self.st.id(&x.id),
            name: self.st.id(&x.name),
            cm_type: x.cm_type,
            effect: x.effect,
            defeats: x.defeats.clone(),
            mass_kg: x.mass_kg,
            provenance: self.provenance(&x.provenance, custom),
        }
    }

    fn datalink(&mut self, x: &s::Datalink, custom: bool) -> c::Datalink {
        c::Datalink {
            id: self.st.id(&x.id),
            name: self.st.id(&x.name),
            latency_s: x.latency_s,
            capacity_tracks: x.capacity_tracks,
            range_m: km_to_m(x.range_km),
            engage_on_remote: x.engage_on_remote,
            jam_resistance: x.jam_resistance,
            provenance: self.provenance(&x.provenance, custom),
        }
    }

    fn loadout(&mut self, x: &s::Loadout, custom: bool) -> c::Loadout {
        let platform = self
            .set
            .entries
            .iter()
            .find_map(|e| match &e.entry {
                Entry::Platform(p) if p.id == x.platform => Some(p),
                _ => None,
            })
            .expect("validated platform");
        let items = x
            .items
            .iter()
            .map(|it| c::LoadoutItem {
                station: platform
                    .stations
                    .iter()
                    .position(|st| st.id == it.station)
                    .and_then(|i| u32::try_from(i).ok())
                    .expect("validated station"),
                weapon: c::WeaponIx(self.ix(Kind::Weapon, &it.weapon)),
                count: it.count,
            })
            .collect();
        c::Loadout {
            id: self.st.id(&x.id),
            name: self.st.id(&x.name),
            platform: c::PlatformIx(self.ix(Kind::Platform, &x.platform)),
            roles: self.st.ids(&x.roles),
            items,
            provenance: self.provenance(&x.provenance, custom),
        }
    }

    fn base(&mut self, x: &s::Base, custom: bool) -> c::Base {
        c::Base {
            id: self.st.id(&x.id),
            name: self.st.id(&x.name),
            base_type: x.base_type,
            country: c::CountryIx(self.ix(Kind::Country, &x.country)),
            icao: x.icao.as_deref().map(|i| self.st.id(i)),
            lat_rad: deg_to_rad(x.lat_deg),
            lon_rad: deg_to_rad(x.lon_deg),
            elevation_m: x.elevation_m,
            runway_length_m: x.runway_length_m,
            aircraft_capacity: x.aircraft_capacity,
            ship_berths: x.ship_berths,
            provenance: self.provenance(&x.provenance, custom),
        }
    }

    fn force(&self, id: &str) -> c::ForceIx {
        if self.ix[&Kind::Platform].0.contains_key(id) {
            c::ForceIx::Platform(c::PlatformIx(self.ix(Kind::Platform, id)))
        } else {
            c::ForceIx::System(c::SystemIx(self.ix(Kind::System, id)))
        }
    }

    fn inventory(&mut self, x: &s::Inventory, custom: bool) -> c::Inventory {
        c::Inventory {
            id: self.st.id(&x.id),
            country: c::CountryIx(self.ix(Kind::Country, &x.country)),
            as_of: self.st.id(&x.as_of),
            platforms: x
                .platforms
                .iter()
                .map(|(id, n)| (self.force(id), *n))
                .collect(),
            munitions: x
                .munitions
                .iter()
                .map(|(id, n)| (c::WeaponIx(self.ix(Kind::Weapon, id)), *n))
                .collect(),
            basing: x
                .basing
                .iter()
                .map(|b| c::Basing {
                    force: self.force(&b.platform),
                    base: c::BaseIx(self.ix(Kind::Base, &b.base)),
                    count: b.count,
                })
                .collect(),
            provenance: self.provenance(&x.provenance, custom),
        }
    }

    fn doctrine(&mut self, x: &s::Doctrine, custom: bool) -> c::Doctrine {
        c::Doctrine {
            id: self.st.id(&x.id),
            name: self.st.id(&x.name),
            aggression: x.aggression,
            risk_tolerance: x.risk_tolerance,
            emcon_discipline: x.emcon_discipline,
            emcon_default: x.emcon_default,
            missile_reserve_fraction: x.missile_reserve_fraction,
            launch_doctrine: x.launch_doctrine,
            shots_per_target: x.shots_per_target,
            cap_aircraft: x.cap_aircraft,
            cap_rotation_s: min_to_s(x.cap_rotation_min),
            reaction_time_s: x.reaction_time_s,
            iads_layers: self.st.ids(&x.iads_layers),
            strike_package: self.st.ids(&x.strike_package),
            provenance: self.provenance(&x.provenance, custom),
        }
    }

    fn realism(&mut self, x: &s::Realism, custom: bool) -> c::Realism {
        let p = &x.passive;
        let f = &x.fusion;
        let d = &x.damage;
        c::Realism {
            id: self.st.id(&x.id),
            name: self.st.id(&x.name),
            time: x.time.clone(),
            earth: x.earth.clone(),
            radar: x.radar.clone(),
            passive: c::PassiveRealism {
                esm_bearing_sigma_default_rad: deg_to_rad(p.esm_bearing_sigma_default_deg),
                irst_afterburner_factor: p.irst_afterburner_factor,
                irst_aspect_factor: p.irst_aspect_factor.clone(),
                eo_gsd_type_m: p.eo_gsd_type_m,
                eo_gsd_class_m: p.eo_gsd_class_m,
                visual_range_m: p
                    .visual_range_km
                    .iter()
                    .map(|(k, v)| (*k, km_to_m(*v)))
                    .collect(),
                visual_night_factor: p.visual_night_factor,
            },
            satellites: c::SatelliteRealism {
                eo_min_sun_elevation_rad: deg_to_rad(x.satellites.eo_min_sun_elevation_deg),
                downlink_delay_default_s: min_to_s(x.satellites.downlink_delay_default_min),
            },
            fusion: c::FusionRealism {
                age_out_s: f
                    .age_out_min
                    .iter()
                    .map(|(k, v)| (*k, min_to_s(*v)))
                    .collect(),
                fixed_sites_kept: f.fixed_sites_kept,
                ellipse_growth_mps: f
                    .ellipse_growth_kt
                    .iter()
                    .map(|(k, v)| (*k, kt_to_mps(*v)))
                    .collect(),
                datalink_latency_default_s: f.datalink_latency_default_s,
                c2_report_delay_s: f.c2_report_delay_s,
            },
            ew: x.ew.clone(),
            weapons: x.weapons.clone(),
            endgame: x.endgame.clone(),
            tewa: c::TewaRealism {
                ciws_layer_min_m: km_to_m(x.tewa.ciws_layer_min_km),
                ciws_layer_max_m: km_to_m(x.tewa.ciws_layer_max_km),
                soft_kill_window_s: x.tewa.soft_kill_window_s,
                kill_assessment_s: x.tewa.kill_assessment_s,
                iads_autonomous_reaction_factor: x.tewa.iads_autonomous_reaction_factor,
            },
            damage: c::DamageRealism {
                ship_out_of_action_fraction: d.ship_out_of_action_fraction,
                ship_sink_delay_s: min_to_s(d.ship_sink_delay_min),
                warhead_multiplier: d.warhead_multiplier.clone(),
                runway_repair_min_s: h_to_s(d.runway_repair_min_h),
                runway_repair_max_s: h_to_s(d.runway_repair_max_h),
            },
            logistics: x.logistics.clone(),
            ground: x.ground.clone(),
            provenance: self.provenance(&x.provenance, custom),
        }
    }
}
