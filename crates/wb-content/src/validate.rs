//! Cross-entry validation: references, ranges, units, station compatibility,
//! provenance. Collects every error rather than stopping at the first.

use crate::load::{ContentError, ContentSet, LoadedEntry, get_path};
use crate::source::platform::{Durability, StationKind};
use crate::source::world::BaseType;
use crate::source::{
    Domain, Entry, Kind, Platform, Provenance, Seeker, SensorType, Station, Weapon,
};
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
        cx.provenance(e.entry.provenance());
        match &e.entry {
            Entry::Country(c) => {
                cx.reference("doctrine", &c.doctrine, &[Kind::Doctrine]);
                for (i, d) in c.datalinks.iter().enumerate() {
                    cx.reference(&format!("datalinks[{i}]"), d, &[Kind::Datalink]);
                }
                if c.iso2.len() != 2 || !c.iso2.bytes().all(|b| b.is_ascii_uppercase()) {
                    cx.err(
                        "iso2",
                        format!("`{}` must be two upper-case letters", c.iso2),
                    );
                }
                if c.iso3.len() != 3 || !c.iso3.bytes().all(|b| b.is_ascii_uppercase()) {
                    cx.err(
                        "iso3",
                        format!("`{}` must be three upper-case letters", c.iso3),
                    );
                } else if c.id != c.iso3.to_ascii_lowercase() {
                    cx.err(
                        "id",
                        format!(
                            "must be the lower-case iso3 code `{}`",
                            c.iso3.to_ascii_lowercase()
                        ),
                    );
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
                cx.non_empty("bands", w.bands.len());
                cx.positive("max_range_km", w.max_range_km);
                cx.angle("field_of_regard_deg", w.field_of_regard_deg);
                if w.simultaneous_targets == 0 {
                    cx.err("simultaneous_targets", "must be at least 1".into());
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
                cx.reference("country", &b.country, &[Kind::Country]);
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
                    if path.is_empty() && k == "provenance" {
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

    fn provenance(&mut self, p: &Provenance) {
        if p.sources.is_empty() && !self.entry.custom {
            self.err(
                "provenance.sources",
                "is empty; add at least one public source (only `custom: true` entries may have none)".into(),
            );
        }
        for (i, s) in p.sources.iter().enumerate() {
            if s.title.trim().is_empty() {
                self.err(
                    &format!("provenance.sources[{i}].title"),
                    "must not be empty".into(),
                );
            }
            if !(s.url.starts_with("https://") || s.url.starts_with("http://")) {
                self.err(
                    &format!("provenance.sources[{i}].url"),
                    format!("`{}` must be an http(s) URL", s.url),
                );
            }
            if !valid_date(&s.accessed) {
                self.err(
                    &format!("provenance.sources[{i}].accessed"),
                    format!("`{}` must be a date like 2026-10-27", s.accessed),
                );
            }
        }
        for (i, path) in p.estimates.iter().enumerate() {
            if get_path(&self.entry.value, path).is_none() {
                self.err(
                    &format!("provenance.estimates[{i}]"),
                    format!("`{path}` is not a field of this entry"),
                );
            }
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
        if let Some(e) = w.guidance.seeker_eccm {
            self.probability("guidance.seeker_eccm", e);
        }
        let unguided = matches!(w.guidance.terminal, Seeker::GpsIns | Seeker::None);
        if unguided && w.cep_m.is_none() {
            self.err(
                "cep_m",
                "is required for GPS/INS and unguided weapons".into(),
            );
        }
        if !unguided && w.pk_base.is_empty() {
            self.err("pk_base", "is required for guided weapons".into());
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
        self.reference("country", &inv.country, &[Kind::Country]);
        if !valid_date(&inv.as_of) {
            self.err(
                "as_of",
                format!("`{}` must be a date like 2026-10-27", inv.as_of),
            );
        }
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
        self.less(
            "damage.runway_repair_min_h",
            r.damage.runway_repair_min_h,
            "damage.runway_repair_max_h",
            r.damage.runway_repair_max_h,
        );
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
                    "must be positive for real_airfield and real_port sites".into(),
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
    match st.kind {
        StationKind::Vls | StationKind::Launcher => count.div_ceil(w.rounds_per_cell.max(1)),
        StationKind::Internal | StationKind::External => count,
    }
}

/// `YYYY-MM-DD` with a plausible month and day.
fn valid_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let num = |r: std::ops::Range<usize>| s[r].parse::<u32>().ok();
    matches!(
        (num(0..4), num(5..7), num(8..10)),
        (Some(y), Some(m), Some(d)) if y >= 1900 && (1..=12).contains(&m) && (1..=31).contains(&d)
    )
}

/// The YAML spelling of an enum used as a map key.
fn key<T: serde::Serialize>(k: &T) -> String {
    serde_yaml::to_value(k)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}
