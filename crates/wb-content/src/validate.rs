//! Cross-entry validation: references, ranges, units, station compatibility,
//! design notes and archetypes. Collects every error rather than stopping at
//! the first.

use crate::load::{ContentError, ContentSet, LoadedEntry, get_path};
use crate::source::platform::{Durability, StationKind};
use crate::source::world::{BaseType, PlaceKind};
use crate::source::{Design, Domain, Entry, Kind, Platform, Seeker, SensorType, Station, Weapon};
use serde_yaml::Value;
use std::collections::{BTreeMap, BTreeSet};

/// Field-name suffixes that carry a unit; such numbers must be finite and
/// not negative (doc 04 §1.3).
const UNIT_SUFFIXES: [&str; 15] = [
    "_km", "_m", "_m2", "_kg", "_s", "_ms", "_min", "_h", "_mach", "_kt", "_kmh", "_nm", "_mps",
    "_kg_h", "_kg_min",
];
/// Unit-suffixed fields that may be negative.
const SIGNED_FIELDS: [&str; 1] = ["elevation_m"];

pub(crate) fn validate(set: &ContentSet) -> Vec<ContentError> {
    let index: BTreeMap<&str, &LoadedEntry> =
        set.entries.iter().map(|e| (e.id.as_str(), e)).collect();
    let mut errors = Vec::new();
    for e in &set.entries {
        let mut cx = Cx {
            entry: e,
            index: &index,
            errors: &mut errors,
        };
        cx.units(&e.value, String::new(), false);
        if let Some(d) = e.entry.design() {
            cx.design(d);
        }
        match &e.entry {
            Entry::Nation(n) => {
                cx.reference("doctrine", &n.doctrine, &[Kind::Doctrine]);
                for (i, d) in n.datalinks.iter().enumerate() {
                    cx.reference(&format!("datalinks[{i}]"), d, &[Kind::Datalink]);
                }
                if n.id.len() != 3 || !n.id.bytes().all(|b| b.is_ascii_lowercase()) {
                    cx.err("id", format!("`{}` must be three lower-case letters", n.id));
                }
                cx.colour("colour", &n.colour);
                if !(2..=3).contains(&n.flag.colours.len()) {
                    cx.err("flag.colours", "must list two or three colours".into());
                }
                for (i, c) in n.flag.colours.iter().enumerate() {
                    cx.colour(&format!("flag.colours[{i}]"), c);
                }
            }
            Entry::World(w) => {
                cx.non_empty("nations", w.nations.len());
                let [west, south, east, north] = w.extent;
                if !(-180.0..=180.0).contains(&west)
                    || !(-180.0..=180.0).contains(&east)
                    || !(-90.0..=90.0).contains(&south)
                    || !(-90.0..=90.0).contains(&north)
                    || west >= east
                    || south >= north
                {
                    cx.err(
                        "extent",
                        "must be [west, south, east, north] in degrees with west < east and south < north"
                            .into(),
                    );
                }
                cx.positive("reach_km", w.reach_km);
                let mut seen = BTreeSet::new();
                for (i, n) in w.nations.iter().enumerate() {
                    let path = format!("nations[{i}]");
                    cx.reference(&format!("{path}.nation"), &n.nation, &[Kind::Nation]);
                    if !seen.insert(n.nation.as_str()) {
                        cx.err(
                            &format!("{path}.nation"),
                            format!("`{}` is listed twice", n.nation),
                        );
                    }
                    let [lon, lat] = n.seed_at;
                    if !(-180.0..=180.0).contains(&lon) || !(-90.0..=90.0).contains(&lat) {
                        cx.err(
                            &format!("{path}.seed_at"),
                            "must be [lon, lat] in degrees".into(),
                        );
                    }
                    cx.positive(&format!("{path}.weight"), n.weight);
                }
                for (i, o) in w.border_overrides.iter().enumerate() {
                    if let Some(n) = &o.nation {
                        cx.reference(&format!("border_overrides[{i}].nation"), n, &[Kind::Nation]);
                    }
                    for (j, c) in o.cells.iter().enumerate() {
                        if c.is_empty() || u64::from_str_radix(c, 16).is_err() {
                            cx.err(
                                &format!("border_overrides[{i}].cells[{j}]"),
                                format!("`{c}` is not an H3 cell index in hex"),
                            );
                        }
                    }
                }
            }
            Entry::Places(p) => {
                cx.reference("world", &p.world, &[Kind::World]);
                cx.non_empty("places", p.places.len());
                let mut capitals = BTreeSet::new();
                for (i, place) in p.places.iter().enumerate() {
                    let path = format!("places[{i}]");
                    if place.name.trim().is_empty() {
                        cx.err(&format!("{path}.name"), "must not be empty".into());
                    }
                    let [lon, lat] = place.at;
                    if !(-180.0..=180.0).contains(&lon) || !(-90.0..=90.0).contains(&lat) {
                        cx.err(
                            &format!("{path}.at"),
                            "must be [lon, lat] in degrees".into(),
                        );
                    }
                    let owned = matches!(
                        place.kind,
                        PlaceKind::Nation | PlaceKind::Capital | PlaceKind::City
                    );
                    match (&place.nation, owned) {
                        (Some(n), _) => {
                            cx.reference(&format!("{path}.nation"), n, &[Kind::Nation]);
                        }
                        (None, true) => cx.err(
                            &format!("{path}.nation"),
                            "is required for nation, capital and city labels".into(),
                        ),
                        (None, false) => {}
                    }
                    if place.kind == PlaceKind::Capital
                        && let Some(n) = &place.nation
                        && !capitals.insert(n.as_str())
                    {
                        cx.err(
                            &format!("{path}.kind"),
                            format!("`{n}` already has a capital"),
                        );
                    }
                }
            }
            Entry::Archetype(a) => {
                for (path, [lo, hi]) in &a.bands {
                    if !(lo.is_finite() && hi.is_finite()) || lo > hi {
                        cx.err(
                            &format!("bands.{path}"),
                            format!("must be [min, max] with min <= max (got [{lo}, {hi}])"),
                        );
                    }
                }
            }
            Entry::Platform(p) => cx.platform(p),
            Entry::System(s) => {
                cx.positive("setup_min", s.setup_min);
                if s.components.is_empty() {
                    cx.err("components", "must list at least one component".into());
                }
                for (i, c) in s.components.iter().enumerate() {
                    let path = format!("components[{i}]");
                    if c.count == 0 {
                        cx.err(&format!("{path}.count"), "must be at least 1".into());
                    }
                    let platform = cx.platform_ref(&format!("{path}.platform"), &c.platform);
                    cx.magazine(&path, platform, &c.magazine);
                }
            }
            Entry::Sensor(s) => cx.sensor(s),
            Entry::Weapon(w) => cx.weapon(w),
            Entry::EwSuite(w) => {
                let modes = w.resolved_modes();
                let has_any_mode = modes.self_protect.is_some()
                    || modes.escort.is_some()
                    || modes.stand_off.is_some();
                if !has_any_mode {
                    cx.err("modes", "at least one mode must be configured".into());
                }
                for (name, mode) in [
                    ("self_protect", &modes.self_protect),
                    ("escort", &modes.escort),
                    ("stand_off", &modes.stand_off),
                ] {
                    if let Some(m) = mode {
                        if m.jam_strength < 0.0 || m.jam_strength > 10.0 {
                            cx.err(
                                &format!("modes.{name}.jam_strength"),
                                "must be in 0..=10".into(),
                            );
                        }
                        if let Some(c) = m.coverage_km {
                            cx.positive(&format!("modes.{name}.coverage_km"), c);
                        }
                        if let Some(sec) = m.sector_deg {
                            cx.angle(&format!("modes.{name}.sector_deg"), sec);
                        }
                    }
                }
            }
            Entry::Countermeasure(c) => {
                cx.probability("effect", c.effect);
                cx.non_empty("defeats", c.defeats.len());
            }
            Entry::Datalink(d) => {
                cx.probability("jam_resistance", d.jam_resistance);
                cx.positive("range_km", d.range_km);
                if d.capacity_tracks == 0 {
                    cx.err("capacity_tracks", "must be at least 1".into());
                }
            }
            Entry::Loadout(l) => cx.loadout(l),
            Entry::Base(b) => {
                cx.reference("nation", &b.nation, &[Kind::Nation]);
                if !(-90.0..=90.0).contains(&b.lat_deg) {
                    cx.err(
                        "lat_deg",
                        format!("must be between -90 and 90 (got {})", b.lat_deg),
                    );
                }
                if !(-180.0..=180.0).contains(&b.lon_deg) {
                    cx.err(
                        "lon_deg",
                        format!("must be between -180 and 180 (got {})", b.lon_deg),
                    );
                }
                if b.base_type != BaseType::Port && b.runway_length_m.is_none() {
                    cx.err("runway_length_m", "is required for air bases".into());
                }
            }
            Entry::Inventory(inv) => cx.inventory(inv),
            Entry::Doctrine(d) => {
                cx.probability("aggression", d.aggression);
                cx.probability("risk_tolerance", d.risk_tolerance);
                cx.probability("emcon_discipline", d.emcon_discipline);
                cx.probability("missile_reserve_fraction", d.missile_reserve_fraction);
                if d.shots_per_target == 0 {
                    cx.err("shots_per_target", "must be at least 1".into());
                }
            }
            Entry::Realism(r) => cx.realism(r),
        }
    }
    if !set
        .entries
        .iter()
        .any(|e| e.kind == Kind::Realism && e.id == "default")
    {
        errors.push(ContentError {
            file: "realism/default.yaml".into(),
            path: String::new(),
            message: "is missing; the pack needs the `default` realism profile".into(),
        });
    }
    errors
}

struct Cx<'a, 'b> {
    entry: &'a LoadedEntry,
    index: &'a BTreeMap<&'a str, &'a LoadedEntry>,
    errors: &'b mut Vec<ContentError>,
}

impl<'a> Cx<'a, '_> {
    fn err(&mut self, path: &str, message: String) {
        self.errors.push(ContentError {
            file: self.entry.file.clone(),
            path: path.to_owned(),
            message,
        });
    }

    fn positive(&mut self, path: &str, v: f64) {
        // Negative unit-suffixed values are already reported by `units`.
        let field = path.rsplit('.').next().unwrap_or(path);
        if v < 0.0 && has_unit_suffix(field) {
            return;
        }
        if v.is_finite() && v <= 0.0 {
            self.err(path, format!("must be greater than 0 (got {v})"));
        }
    }

    fn probability(&mut self, path: &str, v: f64) {
        if !(0.0..=1.0).contains(&v) {
            self.err(path, format!("must be between 0 and 1 (got {v})"));
        }
    }

    fn angle(&mut self, path: &str, v: f64) {
        if !(v > 0.0 && v <= 360.0) {
            self.err(
                path,
                format!("must be greater than 0 and at most 360 (got {v})"),
            );
        }
    }

    fn non_empty(&mut self, path: &str, len: usize) {
        if len == 0 {
            self.err(path, "must not be empty".into());
        }
    }

    fn less(&mut self, path_lo: &str, lo: f64, path_hi: &str, hi: f64) {
        if lo >= hi {
            self.err(
                path_lo,
                format!("must be less than {path_hi} ({lo} ≥ {hi})"),
            );
        }
    }

    /// Checks that `id` names an entry of one of `kinds`.
    fn reference(&mut self, path: &str, id: &str, kinds: &[Kind]) -> Option<&'a LoadedEntry> {
        let expected = kinds
            .iter()
            .map(|k| k.as_str())
            .collect::<Vec<_>>()
            .join(" or ");
        match self.index.get(id) {
            None => {
                self.err(path, format!("unknown {expected} `{id}`"));
                None
            }
            Some(e) if !kinds.contains(&e.kind) => {
                self.err(
                    path,
                    format!("`{id}` is a {}, expected a {expected}", e.kind.as_str()),
                );
                None
            }
            Some(e) => Some(e),
        }
    }

    fn platform_ref(&mut self, path: &str, id: &str) -> Option<&'a Platform> {
        match self.reference(path, id, &[Kind::Platform])?.entry {
            Entry::Platform(ref p) => Some(p),
            _ => None,
        }
    }

    fn weapon_ref(&mut self, path: &str, id: &str) -> Option<&'a Weapon> {
        match self.reference(path, id, &[Kind::Weapon])?.entry {
            Entry::Weapon(ref w) => Some(w),
            _ => None,
        }
    }

    /// Unit-suffixed numbers anywhere in the entry are finite and not negative.
    fn units(&mut self, v: &Value, path: String, in_unit_field: bool) {
        match v {
            Value::Number(n) => {
                let f = n.as_f64().unwrap_or(0.0);
                if !f.is_finite() {
                    self.err(&path, format!("must be a finite number (got {f})"));
                } else if in_unit_field && f < 0.0 {
                    self.err(&path, format!("must be zero or positive (got {f})"));
                }
            }
            Value::Mapping(m) => {
                for (k, child) in m {
                    let Some(k) = k.as_str() else { continue };
                    if path.is_empty() && (k == "design" || k == "bands") {
                        continue;
                    }
                    let unit = in_unit_field || has_unit_suffix(k);
                    let p = if path.is_empty() {
                        k.to_owned()
                    } else {
                        format!("{path}.{k}")
                    };
                    self.units(child, p, unit);
                }
            }
            Value::Sequence(s) => {
                for (i, child) in s.iter().enumerate() {
                    self.units(child, format!("{path}[{i}]"), in_unit_field);
                }
            }
            _ => {}
        }
    }

    fn design(&mut self, d: &Design) {
        if d.note.trim().is_empty() {
            self.err(
                "design.note",
                "must not be empty; say what the entry is for".into(),
            );
        }
        let catalogue = matches!(
            self.entry.kind,
            Kind::Platform
                | Kind::System
                | Kind::Sensor
                | Kind::Weapon
                | Kind::EwSuite
                | Kind::Countermeasure
                | Kind::Datalink
        );
        match &d.archetype {
            Some(a) => {
                self.reference("design.archetype", a, &[Kind::Archetype]);
            }
            None if catalogue => self.err(
                "design.archetype",
                "is missing; catalogue entries name their class archetype (doc 04 §4)".into(),
            ),
            None => {}
        }
    }

    /// `#RRGGBB`.
    fn colour(&mut self, path: &str, c: &str) {
        let b = c.as_bytes();
        if b.len() != 7 || b[0] != b'#' || !b[1..].iter().all(u8::is_ascii_hexdigit) {
            self.err(path, format!("`{c}` must be a colour like #5C7FA8"));
        }
    }

    fn platform(&mut self, p: &Platform) {
        let folder = self.entry.rel_path.split('/').nth(1).unwrap_or_default();
        let is_file = !self
            .entry
            .rel_path
            .trim_start_matches("platforms/")
            .contains('/');
        if is_file || folder != p.category.folder() {
            self.err(
                "category",
                format!(
                    "is `{}`, so the file belongs in `platforms/{}/`",
                    p.category.as_str(),
                    p.category.folder()
                ),
            );
        }
        let m = &p.mobility;
        let domain = p.category.domain();
        let require = |cx: &mut Self, field: &str, v: Option<f64>, what: &str| {
            if v.is_none() {
                cx.err(
                    &format!("mobility.{field}"),
                    format!("is required for {what} platforms"),
                );
            }
        };
        match domain {
            Domain::Air => {
                require(self, "cruise_speed_mach", m.cruise_speed_mach, "air");
                require(self, "max_speed_mach", m.max_speed_mach, "air");
                require(self, "ceiling_m", m.ceiling_m, "air");
                if let (Some(c), Some(x)) = (m.cruise_speed_mach, m.max_speed_mach)
                    && c > x
                {
                    self.err(
                        "mobility.cruise_speed_mach",
                        format!("must not exceed mobility.max_speed_mach ({c} > {x})"),
                    );
                }
                match &p.fuel {
                    Some(f) if f.internal_kg.is_some() && f.burn_kg_h.is_some() => {}
                    _ => self.err(
                        "fuel",
                        "internal_kg and burn_kg_h are required for air platforms".into(),
                    ),
                }
            }
            Domain::Surface | Domain::Subsurface => {
                require(self, "max_speed_kt", m.max_speed_kt, "ship and submarine");
                if let (Some(c), Some(x)) = (m.cruise_speed_kt, m.max_speed_kt)
                    && c > x
                {
                    self.err(
                        "mobility.cruise_speed_kt",
                        format!("must not exceed mobility.max_speed_kt ({c} > {x})"),
                    );
                }
            }
            Domain::Ground => require(self, "road_speed_kmh", m.road_speed_kmh, "ground"),
            Domain::Space => {}
        }
        match (&p.orbit, domain) {
            (None, Domain::Space) => self.err("orbit", "is required for satellites".into()),
            (Some(_), d) if d != Domain::Space => {
                self.err("orbit", "is only allowed for satellites".into())
            }
            (Some(o), _) => {
                if let Err(e) = wb_core::orbit::Tle::parse(&o.tle_line1, &o.tle_line2) {
                    self.err(
                        "orbit",
                        format!("is not a usable two-line element set: {e}"),
                    );
                }
            }
            (None, _) => {}
        }
        let rcs = &p.signature.rcs_m2;
        for (aspect, v) in [
            ("front", rcs.front),
            ("side", rcs.side),
            ("rear", rcs.rear),
            ("top", rcs.top),
        ] {
            self.positive(&format!("signature.rcs_m2.{aspect}"), v);
        }
        let model_ok = matches!(
            (&p.durability, domain),
            (Durability::Aircraft { .. }, Domain::Air)
                | (
                    Durability::Ship { .. },
                    Domain::Surface | Domain::Subsurface
                )
                | (Durability::Ground { .. }, Domain::Ground | Domain::Space)
        );
        if !model_ok {
            self.err(
                "durability.model",
                format!("does not fit a {} platform", p.category.as_str()),
            );
        }
        match &p.durability {
            Durability::Aircraft(k) => {
                self.probability("durability.p_kill_on_hit", k.p_kill_on_hit)
            }
            Durability::Ship(d) | Durability::Ground(d) => {
                self.positive("durability.damage_points", d.damage_points)
            }
        }
        if let Some(t) = &p.tanker {
            self.non_empty("tanker.systems", t.systems.len());
            self.positive("tanker.transfer_rate_kg_min", t.transfer_rate_kg_min);
        }
        let mut seen = BTreeSet::new();
        for (i, s) in p.stations.iter().enumerate() {
            let path = format!("stations[{i}]");
            if !seen.insert(s.id.as_str()) {
                self.err(
                    &format!("{path}.id"),
                    format!("`{}` is used by another station", s.id),
                );
            }
            self.categories(&format!("{path}.categories"), &s.categories);
            if s.capacity == 0 {
                self.err(&format!("{path}.capacity"), "must be at least 1".into());
            }
            if let Some(kg) = s.max_kg {
                self.positive(&format!("{path}.max_kg"), kg);
            }
        }
        for (i, s) in p.sensors.iter().enumerate() {
            self.reference(&format!("sensors[{i}]"), s.sensor(), &[Kind::Sensor]);
        }
        for (i, id) in p.ew.iter().enumerate() {
            self.reference(&format!("ew[{i}]"), id, &[Kind::EwSuite]);
        }
        for id in p.countermeasures.keys() {
            self.reference(
                &format!("countermeasures.{id}"),
                id,
                &[Kind::Countermeasure],
            );
        }
        for (i, id) in p.datalinks.iter().enumerate() {
            self.reference(&format!("datalinks[{i}]"), id, &[Kind::Datalink]);
        }
    }

    fn categories(&mut self, path: &str, cats: &[String]) {
        if cats.is_empty() {
            self.err(path, "must list at least one category".into());
        }
        for (i, c) in cats.iter().enumerate() {
            if c.is_empty()
                || !c
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            {
                self.err(
                    &format!("{path}[{i}]"),
                    format!("`{c}` must be lower-case letters, digits and underscores"),
                );
            }
        }
    }

    fn sensor(&mut self, s: &crate::source::Sensor) {
        self.non_empty("role", s.role.len());
        let blocks = [
            (SensorType::Radar, "radar", s.radar.is_some()),
            (SensorType::Esm, "esm", s.esm.is_some()),
            (SensorType::Irst, "irst", s.irst.is_some()),
            (SensorType::EoIr, "eo_ir", s.eo_ir.is_some()),
            (SensorType::Sonar, "sonar", s.sonar.is_some()),
            (SensorType::Visual, "visual", s.visual.is_some()),
            (SensorType::SatellitePayload, "payload", s.payload.is_some()),
        ];
        let type_name = blocks
            .iter()
            .find(|b| b.0 == s.sensor_type)
            .map(|b| b.1)
            .unwrap_or_default();
        for (t, name, present) in blocks {
            if t == s.sensor_type && !present {
                self.err(name, format!("is required when sensor_type is {name}"));
            } else if t != s.sensor_type && present {
                self.err(
                    name,
                    format!("is not allowed when sensor_type is {type_name}"),
                );
            }
        }
        if let Some(r) = &s.radar {
            self.positive("radar.r_ref_km", r.r_ref_km);
            self.positive("radar.sigma_ref_m2", r.sigma_ref_m2);
            self.positive("radar.scan_period_s", r.scan_period_s);
            self.angle("radar.field_of_regard_deg", r.field_of_regard_deg);
            self.probability("radar.clutter_rejection", r.clutter_rejection);
            if r.jam_resistance.is_none() && r.eccm_db.is_none() {
                self.err("radar.jam_resistance", "is required".into());
            }
            if let Some(jr) = r.jam_resistance
                && !(0.0..=10.0).contains(&jr)
            {
                self.err("radar.jam_resistance", "must be in 0..=10".into());
            }
        }
        if let Some(e) = &s.esm {
            self.non_empty("esm.bands", e.bands.len());
            self.positive("esm.r_ref_km", e.r_ref_km);
            self.positive("esm.bearing_sigma_deg", e.bearing_sigma_deg);
            self.angle("esm.field_of_regard_deg", e.field_of_regard_deg);
        }
        if let Some(i) = &s.irst {
            self.positive("irst.r_ref_km", i.r_ref_km);
            self.positive("irst.i_ref", i.i_ref);
            self.positive("irst.scan_period_s", i.scan_period_s);
            self.angle("irst.field_of_regard_deg", i.field_of_regard_deg);
        }
        if let Some(e) = &s.eo_ir {
            self.positive("eo_ir.max_range_km", e.max_range_km);
            self.positive("eo_ir.gsd_at_10km_m", e.gsd_at_10km_m);
            self.angle("eo_ir.field_of_regard_deg", e.field_of_regard_deg);
        }
        if let Some(so) = &s.sonar {
            self.positive("sonar.passive_r_ref_km", so.passive_r_ref_km);
        }
        if let Some(p) = &s.payload {
            use crate::source::PayloadKind;
            self.positive("payload.swath_km", p.swath_km);
            match p.kind {
                PayloadKind::Optical | PayloadKind::Sar => match p.gsd_m {
                    Some(g) => self.positive("payload.gsd_m", g),
                    None => self.err("payload.gsd_m", "is required for imaging payloads".into()),
                },
                PayloadKind::Sigint => {
                    self.non_empty("payload.bands", p.bands.len());
                    if p.location_sigma_km.is_none() {
                        self.err(
                            "payload.location_sigma_km",
                            "is required for SIGINT payloads".into(),
                        );
                    }
                }
            }
        }
    }

    fn weapon(&mut self, w: &Weapon) {
        use crate::source::weapon::ProfileKind;
        self.non_empty("targets", w.targets.len());
        let pr = &w.profile;
        self.positive("profile.max_speed_mach", pr.max_speed_mach);
        self.positive("profile.max_range_km", pr.max_range_km);
        self.less(
            "profile.min_range_km",
            pr.min_range_km,
            "profile.max_range_km",
            pr.max_range_km,
        );
        if let Some(min) = pr.min_speed_mach {
            self.less(
                "profile.min_speed_mach",
                min,
                "profile.max_speed_mach",
                pr.max_speed_mach,
            );
        }
        if pr.kind == ProfileKind::Rocket {
            self.positive("profile.boost_s", pr.boost_s);
            if pr.coast_tau_s.is_none() {
                self.err(
                    "profile.coast_tau_s",
                    "is required for rocket profiles".into(),
                );
            }
            if pr.min_speed_mach.is_none() {
                self.err(
                    "profile.min_speed_mach",
                    "is required for rocket profiles".into(),
                );
            }
        }
        if let Some(e) = &w.envelope {
            self.less(
                "envelope.min_alt_m",
                e.min_alt_m,
                "envelope.max_alt_m",
                e.max_alt_m,
            );
        }
        for (class, pk) in &w.pk_base {
            self.probability(&format!("pk_base.{}", key(class)), *pk);
        }
        for (cm, r) in &w.countermeasure_resistance {
            self.probability(&format!("countermeasure_resistance.{}", key(cm)), *r);
        }
        if let Some(e) = w.guidance.seeker_resistance() {
            self.probability("guidance.seeker_resistance", e);
        }
        let unguided = matches!(w.guidance.terminal, Seeker::SatnavIns | Seeker::None);
        if unguided && w.cep_m.is_none() {
            self.err(
                "cep_m",
                "is required for satellite-navigation/INS and unguided weapons".into(),
            );
        }
        if !unguided && w.pk_base.is_empty() {
            self.err("pk_base", "is required for guided weapons".into());
        }
        if w.signature.is_none() {
            self.err(
                "signature",
                "is required: sensors must see the weapon in flight".into(),
            );
        }
        self.positive("mass_kg", w.mass_kg);
        self.categories("station_category", &w.station_category);
        if w.rounds_per_cell == 0 {
            self.err("rounds_per_cell", "must be at least 1".into());
        }
    }

    fn loadout(&mut self, l: &crate::source::Loadout) {
        let Some(p) = self.platform_ref("platform", &l.platform) else {
            return;
        };
        self.non_empty("items", l.items.len());
        // Station id → (cells used, kg used, path of the last item on it).
        let mut used: BTreeMap<&str, (u32, f64, String)> = BTreeMap::new();
        for (i, it) in l.items.iter().enumerate() {
            let path = format!("items[{i}]");
            let Some(st) = p.stations.iter().find(|s| s.id == it.station) else {
                self.err(
                    &format!("{path}.station"),
                    format!("platform `{}` has no station `{}`", p.id, it.station),
                );
                continue;
            };
            if it.count == 0 {
                self.err(&format!("{path}.count"), "must be at least 1".into());
            }
            let Some(w) = self.weapon_ref(&format!("{path}.weapon"), &it.weapon) else {
                continue;
            };
            if !self.fits(&format!("{path}.weapon"), w, st) {
                continue;
            }
            let u = used
                .entry(st.id.as_str())
                .or_insert((0, 0.0, String::new()));
            u.0 += cells(st, w, it.count);
            u.1 += w.mass_kg * f64::from(it.count);
            u.2 = path;
        }
        for (sid, (cells, kg, path)) in used {
            let st = p
                .stations
                .iter()
                .find(|s| s.id == sid)
                .expect("found above");
            if cells > st.capacity {
                self.err(
                    &format!("{path}.count"),
                    format!(
                        "station `{sid}` holds {}, the loadout needs {cells}",
                        st.capacity
                    ),
                );
            }
            if let Some(max) = st.max_kg
                && kg > max
            {
                self.err(
                    &format!("{path}.count"),
                    format!(
                        "station `{sid}` takes at most {max} kg, the loadout puts {kg} kg on it"
                    ),
                );
            }
        }
    }

    /// Category and carriage compatibility of a weapon with a station.
    fn fits(&mut self, path: &str, w: &Weapon, st: &Station) -> bool {
        if !w.station_category.iter().any(|c| st.categories.contains(c)) {
            self.err(
                path,
                format!(
                    "`{}` (categories {}) does not fit station `{}` (accepts {})",
                    w.id,
                    w.station_category.join(", "),
                    st.id,
                    st.categories.join(", ")
                ),
            );
            return false;
        }
        if st.kind == StationKind::Internal && !w.internal_carriage {
            self.err(
                path,
                format!(
                    "`{}` cannot be carried internally, and station `{}` is an internal bay",
                    w.id, st.id
                ),
            );
            return false;
        }
        true
    }

    /// Per-unit magazine of a system component against the platform's stations.
    fn magazine(&mut self, path: &str, p: Option<&Platform>, mag: &BTreeMap<String, u32>) {
        let mut cells_needed = 0;
        let mut usable: BTreeSet<&str> = BTreeSet::new();
        for (wid, n) in mag {
            let wpath = format!("{path}.magazine.{wid}");
            let Some(w) = self.weapon_ref(&wpath, wid) else {
                continue;
            };
            let Some(p) = p else { continue };
            let fitting: Vec<&Station> = p
                .stations
                .iter()
                .filter(|s| w.station_category.iter().any(|c| s.categories.contains(c)))
                .collect();
            if fitting.is_empty() {
                self.err(
                    &wpath,
                    format!("`{wid}` does not fit any station of `{}`", p.id),
                );
                continue;
            }
            usable.extend(fitting.iter().map(|s| s.id.as_str()));
            cells_needed += cells(fitting[0], w, *n);
        }
        if let Some(p) = p {
            let capacity: u32 = p
                .stations
                .iter()
                .filter(|s| usable.contains(s.id.as_str()))
                .map(|s| s.capacity)
                .sum();
            if cells_needed > capacity {
                self.err(
                    &format!("{path}.magazine"),
                    format!(
                        "needs {cells_needed} cells per `{}` but it has {capacity}",
                        p.id
                    ),
                );
            }
        }
    }

    fn inventory(&mut self, inv: &crate::source::Inventory) {
        self.reference("nation", &inv.nation, &[Kind::Nation]);
        for id in inv.platforms.keys() {
            self.reference(
                &format!("platforms.{id}"),
                id,
                &[Kind::Platform, Kind::System],
            );
        }
        for id in inv.munitions.keys() {
            self.reference(&format!("munitions.{id}"), id, &[Kind::Weapon]);
        }
        let mut based: BTreeMap<&str, u32> = BTreeMap::new();
        for (i, b) in inv.basing.iter().enumerate() {
            self.reference(&format!("basing[{i}].base"), &b.base, &[Kind::Base]);
            if !inv.platforms.contains_key(&b.platform) {
                self.err(
                    &format!("basing[{i}].platform"),
                    format!("`{}` is not in this inventory's platforms", b.platform),
                );
                continue;
            }
            *based.entry(b.platform.as_str()).or_default() += b.count;
        }
        for (id, n) in based {
            let have = inv.platforms[id];
            if n > have {
                self.err(
                    "basing",
                    format!("bases {n} × `{id}` but the inventory has {have}"),
                );
            }
        }
        let mut stored: BTreeMap<&str, u32> = BTreeMap::new();
        for (i, b) in inv.munition_basing.iter().enumerate() {
            let path = format!("munition_basing[{i}]");
            if let Some(e) = self.reference(&format!("{path}.base"), &b.base, &[Kind::Base])
                && let Entry::Base(base) = &e.entry
                && base.nation != inv.nation
            {
                self.err(
                    &format!("{path}.base"),
                    format!(
                        "`{}` belongs to `{}`, not to `{}`",
                        b.base, base.nation, inv.nation
                    ),
                );
            }
            for (wid, n) in &b.munitions {
                if !inv.munitions.contains_key(wid) {
                    self.err(
                        &format!("{path}.munitions.{wid}"),
                        format!("`{wid}` is not in this inventory's munitions"),
                    );
                    continue;
                }
                *stored.entry(wid.as_str()).or_default() += n;
            }
        }
        for (id, n) in stored {
            let have = inv.munitions[id];
            if n > have {
                self.err(
                    "munition_basing",
                    format!("stores {n} × `{id}` but the inventory has {have}"),
                );
            }
        }
    }

    fn realism(&mut self, r: &crate::source::Realism) {
        let t = &r.time;
        if t.tick_fine_ms == 0
            || !t.tick_coarse_ms.is_multiple_of(t.tick_fine_ms)
            || t.tick_coarse_ms <= t.tick_fine_ms
        {
            self.err(
                "time.tick_coarse_ms",
                format!(
                    "must be a larger multiple of time.tick_fine_ms ({} and {})",
                    t.tick_coarse_ms, t.tick_fine_ms
                ),
            );
        }
        self.positive("earth.radius_m", r.earth.radius_m);
        self.positive("radar.pd_steepness_k", r.radar.pd_steepness_k);
        if r.radar.track_confirm_hits > r.radar.track_confirm_window_scans {
            self.err(
                "radar.track_confirm_hits",
                "must not exceed radar.track_confirm_window_scans".into(),
            );
        }
        self.probability(
            "radar.doppler_notch_pd_factor",
            r.radar.doppler_notch_pd_factor,
        );
        self.probability("passive.visual_night_factor", r.passive.visual_night_factor);
        for (seeker, a) in &r.endgame.aspect {
            for (f, v) in [("head_on", a.head_on), ("beam", a.beam), ("tail", a.tail)] {
                self.probability(&format!("endgame.aspect.{}.{f}", key(seeker)), v);
            }
        }
        self.probability("endgame.maneuver_floor", r.endgame.maneuver_floor);
        self.probability(
            "endgame.nominal_speed_fraction",
            r.endgame.nominal_speed_fraction,
        );
        self.probability("endgame.pk_min", r.endgame.pk_min);
        self.probability("endgame.pk_max", r.endgame.pk_max);
        self.less(
            "endgame.pk_min",
            r.endgame.pk_min,
            "endgame.pk_max",
            r.endgame.pk_max,
        );
        self.less(
            "weapons.pn_gain_min",
            r.weapons.pn_gain_min,
            "weapons.pn_gain_max",
            r.weapons.pn_gain_max,
        );
        self.less(
            "weapons.sea_skim_min_alt_m",
            r.weapons.sea_skim_min_alt_m,
            "weapons.sea_skim_max_alt_m",
            r.weapons.sea_skim_max_alt_m,
        );
        self.less(
            "tewa.ciws_layer_min_km",
            r.tewa.ciws_layer_min_km,
            "tewa.ciws_layer_max_km",
            r.tewa.ciws_layer_max_km,
        );
        let d = &r.damage;
        for (k, b) in &d.base_components {
            let path = format!("damage.base_components.{}", key(k));
            self.less(
                &format!("{path}.repair_min_h"),
                b.repair_min_h,
                &format!("{path}.repair_max_h"),
                b.repair_max_h,
            );
            self.positive(&format!("{path}.hardness_kg"), b.hardness_kg);
        }
        if d.base_components.values().map(|b| b.weight).sum::<f64>() <= 0.0 {
            self.err(
                "damage.base_components",
                "needs at least one component with a positive weight".into(),
            );
        }
        if d.ship_hits.values().sum::<f64>() <= 0.0 {
            self.err(
                "damage.ship_hits",
                "needs at least one location with a positive weight".into(),
            );
        }
        self.probability("damage.propulsion_speed_factor", d.propulsion_speed_factor);
        self.probability(
            "damage.munitions_store_loss_fraction",
            d.munitions_store_loss_fraction,
        );
        self.positive("damage.points_per_warhead_kg", d.points_per_warhead_kg);
        let w = &r.weapons;
        for (path, v) in [
            ("weapons.tau_high_alt_m", w.tau_high_alt_m),
            ("weapons.seeker_half_angle_deg", w.seeker_half_angle_deg),
            ("weapons.datalink_update_s", w.datalink_update_s),
            ("weapons.fuze_radius_m", w.fuze_radius_m),
            (
                "weapons.self_destruct_after_lost_s",
                w.self_destruct_after_lost_s,
            ),
            ("weapons.cruise_alt_default_m", w.cruise_alt_default_m),
            ("weapons.max_flight_path_deg", w.max_flight_path_deg),
        ] {
            self.positive(path, v);
        }
        self.probability("weapons.close_fraction", w.close_fraction);
        self.probability(
            "weapons.range_calibration_tolerance",
            w.range_calibration_tolerance,
        );
        {
            use crate::source::WeaponCategory as C;
            for c in [
                C::Aam,
                C::Sam,
                C::Ashm,
                C::Lacm,
                C::Arm,
                C::Agm,
                C::Bomb,
                C::Ballistic,
                C::Rocket,
                C::Shell,
                C::Torpedo,
                C::Loitering,
                C::Decoy,
            ] {
                if !w.calibration.contains_key(&c) {
                    self.err(
                        &format!("weapons.calibration.{}", key(&c)),
                        "is missing; every weapon category needs calibration conditions".into(),
                    );
                }
            }
        }
        for seeker in [
            Seeker::ActiveRadar,
            Seeker::SemiActiveRadar,
            Seeker::Ir,
            Seeker::ImagingIr,
            Seeker::AntiRadiation,
            Seeker::SatnavIns,
            Seeker::Laser,
            Seeker::Command,
            Seeker::None,
        ] {
            if !r.endgame.aspect.contains_key(&seeker) {
                self.err(
                    &format!("endgame.aspect.{}", key(&seeker)),
                    "is missing; every seeker needs aspect factors".into(),
                );
            }
        }
        self.positive(
            "countermeasures.release_range_m",
            r.countermeasures.release_range_m,
        );
        self.positive("roe.territory_sigma", r.roe.territory_sigma);
        self.positive("roe.threat_cpa_m", r.roe.threat_cpa_m);
        self.positive("roe.threat_horizon_s", r.roe.threat_horizon_s);
        self.probability(
            "damage.ship_out_of_action_fraction",
            r.damage.ship_out_of_action_fraction,
        );
        self.probability(
            "logistics.fuel_reserve_fraction",
            r.logistics.fuel_reserve_fraction,
        );
        self.probability("ground.random_variation", r.ground.random_variation);

        let m = &r.movement;
        for (path, v) in [
            ("movement.isa_sea_level_temp_k", m.isa_sea_level_temp_k),
            ("movement.isa_gamma_r", m.isa_gamma_r),
            (
                "movement.aircraft_climb_rate_mps",
                m.aircraft_climb_rate_mps,
            ),
            (
                "movement.aircraft_descent_rate_mps",
                m.aircraft_descent_rate_mps,
            ),
            ("movement.aircraft_accel_mps2", m.aircraft_accel_mps2),
            ("movement.aircraft_cruise_alt_m", m.aircraft_cruise_alt_m),
            ("movement.ship_accel_mps2", m.ship_accel_mps2),
            ("movement.formation_spacing_m", m.formation_spacing_m),
            ("movement.waypoint_radius_m", m.waypoint_radius_m),
        ] {
            self.positive(path, v);
        }
        if m.aircraft_sustained_g <= 1.0 {
            self.err(
                "movement.aircraft_sustained_g",
                format!("must be above 1 to turn, got {}", m.aircraft_sustained_g),
            );
        }
        let bands = &r.fuel.burn_altitude_factor;
        if bands.is_empty() {
            self.err(
                "fuel.burn_altitude_factor",
                "needs at least one band".into(),
            );
        }
        for (i, b) in bands.iter().enumerate() {
            self.positive(&format!("fuel.burn_altitude_factor[{i}].factor"), b.factor);
            if i > 0 && b.below_m <= bands[i - 1].below_m {
                self.err(
                    &format!("fuel.burn_altitude_factor[{i}].below_m"),
                    "bands must be in increasing altitude order".into(),
                );
            }
        }
        self.probability("fuel.descent_burn_fraction", r.fuel.descent_burn_fraction);
        let l = &r.logistics;
        for (path, v) in [
            ("logistics.tanker_join_km", l.tanker_join_km),
            ("logistics.landing_radius_km", l.landing_radius_km),
            ("logistics.pattern_alt_m", l.pattern_alt_m),
            ("logistics.berth_radius_km", l.berth_radius_km),
            ("logistics.turnaround_default_min", l.turnaround_default_min),
        ] {
            self.positive(path, v);
        }
        use crate::source::realism::{BaseKind, SiteRule};
        for kind in [
            BaseKind::AirBase,
            BaseKind::RoadStrip,
            BaseKind::Heliport,
            BaseKind::NavalBase,
            BaseKind::Anchorage,
            BaseKind::ArmyBase,
            BaseKind::SamSite,
            BaseKind::MissileSite,
            BaseKind::Hq,
        ] {
            match r.bases.get(&kind) {
                None => self.err(
                    &format!("bases.{}", key(&kind)),
                    "is missing; every base kind needs a placement rule".into(),
                ),
                Some(b) if b.site != SiteRule::Terrain && b.match_radius_km <= 0.0 => self.err(
                    &format!("bases.{}.match_radius_km", key(&kind)),
                    "must be positive for preset sites".into(),
                ),
                Some(_) => {}
            }
        }
    }
}

/// The field carries a unit and may not be negative.
fn has_unit_suffix(field: &str) -> bool {
    UNIT_SUFFIXES.iter().any(|s| field.ends_with(s)) && !SIGNED_FIELDS.contains(&field)
}

/// Cells (or rounds, for pylons and bays) taken by `count` rounds.
pub(crate) fn cells(st: &Station, w: &Weapon, count: u32) -> u32 {
    crate::compiled::station_cells(st.kind, w.rounds_per_cell, count)
}

/// The YAML spelling of an enum used as a map key.
fn key<T: serde::Serialize>(k: &T) -> String {
    serde_yaml::to_value(k)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Values outside their archetype's bands (doc 04 §4). A band whose path the
/// entry does not have is skipped: archetypes are shared by entries that fill
/// different optional fields.
pub(crate) fn band_warnings(set: &ContentSet) -> Vec<ContentError> {
    let archetypes: BTreeMap<&str, &crate::source::Archetype> = set
        .entries
        .iter()
        .filter_map(|e| match &e.entry {
            Entry::Archetype(a) => Some((e.id.as_str(), a)),
            _ => None,
        })
        .collect();
    let mut out = Vec::new();
    for e in &set.entries {
        let Some(id) = e.entry.design().and_then(|d| d.archetype.as_deref()) else {
            continue;
        };
        let Some(a) = archetypes.get(id) else {
            continue;
        };
        for (path, [lo, hi]) in &a.bands {
            let Some(v) = get_path(&e.value, path).and_then(Value::as_f64) else {
                continue;
            };
            if v < *lo || v > *hi {
                out.push(ContentError {
                    file: e.file.clone(),
                    path: path.clone(),
                    message: format!(
                        "{v} is outside the `{id}` band [{lo}, {hi}]; say why in design.note or move it inside"
                    ),
                });
            }
        }
    }
    out
}
