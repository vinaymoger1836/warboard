//! Loading `content/**/*.yaml`: parse, check kind and folder, resolve
//! `extends` + `overrides`, read the typed entry.

use crate::source::{
    Base, Countermeasure, Country, Datalink, Doctrine, Entry, EntryMeta, EwSuite, Inventory, Kind,
    Loadout, Manifest, Platform, Realism, Sensor, System, Weapon,
};
use serde::de::DeserializeOwned;
use serde_yaml::{Mapping, Value};
use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

/// One problem in the content, with the file and field it is in.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ContentError {
    /// File as shown to the user, e.g. `content/weapons/aim-120d.yaml`.
    pub file: String,
    /// Field path, e.g. `profile.max_range_km` or `stations[0].max_kg`; empty
    /// for the whole file.
    pub path: String,
    pub message: String,
}

impl fmt::Display for ContentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            write!(f, "{}: {}", self.file, self.message)
        } else {
            write!(f, "{}: {}: {}", self.file, self.path, self.message)
        }
    }
}

/// A content file: path relative to the content root with `/` separators.
#[derive(Debug, Clone)]
pub struct SourceFile {
    pub path: String,
    pub text: String,
}

/// An entry after `extends` and `overrides`.
#[derive(Debug, Clone)]
pub struct LoadedEntry {
    /// Display path of the file.
    pub file: String,
    /// Path relative to the content root.
    pub rel_path: String,
    pub id: String,
    pub kind: Kind,
    pub custom: bool,
    /// Resolved YAML without meta keys (used for estimates paths and
    /// unit checks).
    pub value: Value,
    pub entry: Entry,
}

/// Every entry of a content folder, sorted by id.
#[derive(Debug, Clone)]
pub struct ContentSet {
    pub manifest: Manifest,
    pub entries: Vec<LoadedEntry>,
}

const MANIFEST: &str = "manifest.yaml";
const META_KEYS: [&str; 4] = ["kind", "custom", "extends", "overrides"];

/// Reads every `*.yaml` under `dir` and loads it. Errors carry paths that
/// start with `dir` as given.
pub fn load_dir(dir: &Path) -> Result<ContentSet, Vec<ContentError>> {
    let label = dir.to_string_lossy().replace('\\', "/");
    let label = label.trim_end_matches('/').to_owned();
    let mut files = Vec::new();
    let mut errors = Vec::new();
    collect_yaml(dir, dir, &mut files, &mut errors);
    if !errors.is_empty() {
        return Err(errors);
    }
    load_files(&label, files)
}

fn collect_yaml(
    root: &Path,
    dir: &Path,
    files: &mut Vec<SourceFile>,
    errors: &mut Vec<ContentError>,
) {
    let io_err = |p: &Path, e: std::io::Error| ContentError {
        file: p.to_string_lossy().replace('\\', "/"),
        path: String::new(),
        message: format!("cannot read: {e}"),
    };
    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(e) => return errors.push(io_err(dir, e)),
    };
    let mut paths: Vec<_> = read.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            collect_yaml(root, &p, files, errors);
        } else if p.extension().is_some_and(|e| e == "yaml" || e == "yml") {
            let rel = p
                .strip_prefix(root)
                .unwrap_or(&p)
                .to_string_lossy()
                .replace('\\', "/");
            match std::fs::read_to_string(&p) {
                Ok(text) => files.push(SourceFile { path: rel, text }),
                Err(e) => errors.push(io_err(&p, e)),
            }
        }
    }
}

/// A parsed file before resolution.
struct Raw {
    file: String,
    rel_path: String,
    id: String,
    meta: EntryMeta,
    /// Own keys without meta keys.
    own: Mapping,
}

/// Loads, resolves and validates a set of files. `root_label` prefixes file
/// paths in errors (e.g. `content`).
pub fn load_files(
    root_label: &str,
    mut files: Vec<SourceFile>,
) -> Result<ContentSet, Vec<ContentError>> {
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let display = |rel: &str| {
        if root_label.is_empty() {
            rel.to_owned()
        } else {
            format!("{root_label}/{rel}")
        }
    };
    let mut errors = Vec::new();
    let mut manifest = None;
    let mut raws: BTreeMap<String, Raw> = BTreeMap::new();

    for f in &files {
        let file = display(&f.path);
        let err = |path: &str, message: String| ContentError {
            file: file.clone(),
            path: path.to_owned(),
            message,
        };
        if f.path == MANIFEST {
            match parse_yaml(&f.text)
                .map_err(|e| (String::new(), e))
                .and_then(from_value::<Manifest>)
            {
                Ok(m) => manifest = Some(m),
                Err((path, msg)) => errors.push(err(&path, msg)),
            }
            continue;
        }
        let folder = f.path.split('/').next().unwrap_or_default();
        let Some(folder_kind) = Kind::from_folder(folder).filter(|_| f.path.contains('/')) else {
            errors.push(err(
                "",
                format!(
                    "is not in a content folder (expected one of: {})",
                    Kind::ALL.map(|k| k.folder()).join(", ")
                ),
            ));
            continue;
        };
        let value = match parse_yaml(&f.text) {
            Ok(v) => v,
            Err(e) => {
                errors.push(err("", e));
                continue;
            }
        };
        let Value::Mapping(map) = value else {
            errors.push(err(
                "",
                "must be a YAML mapping (one entry per file)".into(),
            ));
            continue;
        };
        let meta = match from_value::<EntryMeta>(Value::Mapping(map.clone())) {
            Ok(m) => m,
            Err((path, msg)) => {
                errors.push(err(&path, msg));
                continue;
            }
        };
        if meta.kind != folder_kind {
            errors.push(err(
                "kind",
                format!(
                    "is `{}` but the file is in `{}/` (expected `{}`)",
                    meta.kind.as_str(),
                    folder,
                    folder_kind.as_str()
                ),
            ));
            continue;
        }
        let id = match map.get("id") {
            Some(Value::String(s)) => s.clone(),
            Some(_) => {
                errors.push(err("id", "must be a string".into()));
                continue;
            }
            None => {
                errors.push(err("id", "is missing".into()));
                continue;
            }
        };
        if !valid_id(&id) {
            errors.push(err(
                "id",
                format!("`{id}` must be lower-case letters, digits and hyphens, starting with a letter or digit"),
            ));
            continue;
        }
        if let Some(other) = raws.get(&id) {
            errors.push(err(
                "id",
                format!("`{id}` is already used by {}", other.file),
            ));
            continue;
        }
        if !map.contains_key("provenance") {
            errors.push(err(
                "provenance",
                "is missing; every entry needs sources and a confidence (doc 04 §4)".into(),
            ));
            continue;
        }
        let mut own = map;
        for k in META_KEYS {
            own.remove(k);
        }
        raws.insert(
            id.clone(),
            Raw {
                file,
                rel_path: f.path.clone(),
                id,
                meta,
                own,
            },
        );
    }

    if manifest.is_none() && !errors.iter().any(|e| e.file == display(MANIFEST)) {
        errors.push(ContentError {
            file: display(MANIFEST),
            path: String::new(),
            message: "is missing; it must give `content_version`".into(),
        });
    }

    let mut resolved: BTreeMap<String, Option<Mapping>> = BTreeMap::new();
    for id in raws.keys() {
        resolve(id, &raws, &mut resolved, &mut Vec::new(), &mut errors);
    }

    let mut entries = Vec::new();
    for (id, raw) in &raws {
        let Some(Some(map)) = resolved.get(id) else {
            continue;
        };
        let value = Value::Mapping(map.clone());
        match typed(raw.meta.kind, value.clone()) {
            Ok(entry) => entries.push(LoadedEntry {
                file: raw.file.clone(),
                rel_path: raw.rel_path.clone(),
                id: raw.id.clone(),
                kind: raw.meta.kind,
                custom: raw.meta.custom,
                value,
                entry,
            }),
            Err((path, message)) => errors.push(ContentError {
                file: raw.file.clone(),
                path,
                message,
            }),
        }
    }

    if errors.is_empty() {
        let set = ContentSet {
            manifest: manifest.expect("checked above"),
            entries,
        };
        errors = crate::validate::validate(&set);
        if errors.is_empty() {
            return Ok(set);
        }
    }
    errors.sort();
    errors.dedup();
    Err(errors)
}

/// Resolves one entry (memoised). `None` in `done` marks a failed entry.
fn resolve(
    id: &str,
    raws: &BTreeMap<String, Raw>,
    done: &mut BTreeMap<String, Option<Mapping>>,
    stack: &mut Vec<String>,
    errors: &mut Vec<ContentError>,
) -> Option<Mapping> {
    if let Some(r) = done.get(id) {
        return r.clone();
    }
    let raw = &raws[id];
    let err = |path: String, message: String| ContentError {
        file: raw.file.clone(),
        path,
        message,
    };
    let mut merged = match &raw.meta.extends {
        None => raw.own.clone(),
        Some(parent) => {
            if let Some(pos) = stack.iter().position(|s| s == parent) {
                let mut cycle = stack[pos..].to_vec();
                cycle.push(id.to_owned());
                cycle.push(parent.clone());
                // Reported once, on the entry that closes the cycle.
                errors.push(err(
                    "extends".into(),
                    format!("cycle: {}", cycle.join(" → ")),
                ));
                done.insert(id.to_owned(), None);
                return None;
            }
            let Some(p) = raws.get(parent) else {
                errors.push(err("extends".into(), format!("unknown entry `{parent}`")));
                done.insert(id.to_owned(), None);
                return None;
            };
            if p.meta.kind != raw.meta.kind {
                errors.push(err(
                    "extends".into(),
                    format!(
                        "`{parent}` is a {}, not a {}",
                        p.meta.kind.as_str(),
                        raw.meta.kind.as_str()
                    ),
                ));
                done.insert(id.to_owned(), None);
                return None;
            }
            stack.push(id.to_owned());
            let base = resolve(parent, raws, done, stack, errors);
            stack.pop();
            let Some(mut base) = base else {
                done.insert(id.to_owned(), None);
                return None;
            };
            for (k, v) in &raw.own {
                base.insert(k.clone(), v.clone());
            }
            base
        }
    };
    let mut ok = true;
    for (path, v) in &raw.meta.overrides {
        let mut root = Value::Mapping(merged);
        match get_path_mut(&mut root, path) {
            Some(slot) => *slot = v.clone(),
            None => {
                ok = false;
                errors.push(err(
                    format!("overrides.{path}"),
                    "no such field to override".into(),
                ));
            }
        }
        let Value::Mapping(m) = root else {
            unreachable!()
        };
        merged = m;
    }
    let result = ok.then_some(merged);
    done.insert(id.to_owned(), result.clone());
    result
}

fn parse_yaml(text: &str) -> Result<Value, String> {
    serde_yaml::from_str(text).map_err(|e| format!("invalid YAML: {e}"))
}

/// Deserializes with the failing field's path.
fn from_value<T: DeserializeOwned>(value: Value) -> Result<T, (String, String)> {
    serde_path_to_error::deserialize(value).map_err(|e| {
        let path = e.path().to_string();
        let path = if path == "." { String::new() } else { path };
        (path, e.into_inner().to_string())
    })
}

fn typed(kind: Kind, v: Value) -> Result<Entry, (String, String)> {
    Ok(match kind {
        Kind::Country => Entry::Country(from_value::<Country>(v)?),
        Kind::Platform => Entry::Platform(from_value::<Platform>(v)?),
        Kind::System => Entry::System(from_value::<System>(v)?),
        Kind::Sensor => Entry::Sensor(from_value::<Sensor>(v)?),
        Kind::Weapon => Entry::Weapon(from_value::<Weapon>(v)?),
        Kind::EwSuite => Entry::EwSuite(from_value::<EwSuite>(v)?),
        Kind::Countermeasure => Entry::Countermeasure(from_value::<Countermeasure>(v)?),
        Kind::Datalink => Entry::Datalink(from_value::<Datalink>(v)?),
        Kind::Loadout => Entry::Loadout(from_value::<Loadout>(v)?),
        Kind::Base => Entry::Base(from_value::<Base>(v)?),
        Kind::Inventory => Entry::Inventory(from_value::<Inventory>(v)?),
        Kind::Doctrine => Entry::Doctrine(from_value::<Doctrine>(v)?),
        Kind::Realism => Entry::Realism(Box::new(from_value::<Realism>(v)?)),
    })
}

pub(crate) fn valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    !b.is_empty()
        && b[0].is_ascii_alphanumeric()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// Splits `a.b[0].c` or `a.b.0.c` into segments.
fn segments(path: &str) -> Vec<String> {
    path.replace('[', ".")
        .replace(']', "")
        .split('.')
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The value at a field path, if it exists.
pub fn get_path<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cur = v;
    for seg in segments(path) {
        cur = match cur {
            Value::Mapping(m) => m.get(seg.as_str())?,
            Value::Sequence(s) => s.get(seg.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(cur)
}

fn get_path_mut<'a>(v: &'a mut Value, path: &str) -> Option<&'a mut Value> {
    let segs = segments(path);
    if segs.is_empty() {
        return None;
    }
    let mut cur = v;
    for seg in segs {
        cur = match cur {
            Value::Mapping(m) => m.get_mut(seg.as_str())?,
            Value::Sequence(s) => s.get_mut(seg.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(cur)
}
