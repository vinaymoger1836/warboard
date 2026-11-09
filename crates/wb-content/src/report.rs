//! The content check as data, for the admin app (doc 04 §5, ADR-045):
//! errors, band warnings, and per entry its resolved fields and where its
//! key stats sit in the archetype's bands. Same rules as `wb content check`.

use crate::load::{ContentError, SourceFile, load_files};
use crate::validate::{BandPosition, band_positions};
use serde::Serialize;

/// One problem: file, field path and message.
#[derive(Debug, Clone, Serialize)]
pub struct Problem {
    pub file: String,
    pub path: String,
    pub message: String,
}

impl From<ContentError> for Problem {
    fn from(e: ContentError) -> Self {
        Self {
            file: e.file,
            path: e.path,
            message: e.message,
        }
    }
}

/// One loaded entry.
#[derive(Debug, Clone, Serialize)]
pub struct EntryInfo {
    pub id: String,
    pub kind: &'static str,
    /// Path relative to the content root.
    pub file: String,
    pub custom: bool,
    pub extends: Option<String>,
    pub archetype: Option<String>,
    /// Resolved fields after `extends` and `overrides`, without meta keys.
    pub value: serde_yaml::Value,
    pub bands: Vec<BandPosition>,
}

/// Result of checking a content folder.
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub ok: bool,
    pub content_version: Option<String>,
    pub errors: Vec<Problem>,
    pub warnings: Vec<Problem>,
    /// Empty when there are errors.
    pub entries: Vec<EntryInfo>,
}

/// Loads, resolves and validates `files` (paths relative to the content
/// root, plus `denylist.txt`) and reports the result.
pub fn check(root_label: &str, files: Vec<SourceFile>) -> Report {
    match load_files(root_label, files) {
        Err(errors) => Report {
            ok: false,
            content_version: None,
            errors: errors.into_iter().map(Problem::from).collect(),
            warnings: Vec::new(),
            entries: Vec::new(),
        },
        Ok(set) => {
            let mut bands = band_positions(&set);
            let entries = set
                .entries
                .iter()
                .enumerate()
                .map(|(i, e)| EntryInfo {
                    id: e.id.clone(),
                    kind: e.kind.as_str(),
                    file: e.rel_path.clone(),
                    custom: e.custom,
                    extends: e.extends.clone(),
                    archetype: e.entry.design().and_then(|d| d.archetype.clone()),
                    value: e.value.clone(),
                    bands: bands.remove(&i).unwrap_or_default(),
                })
                .collect();
            Report {
                ok: true,
                content_version: Some(set.manifest.content_version.clone()),
                errors: Vec::new(),
                warnings: set.warnings.iter().cloned().map(Problem::from).collect(),
                entries,
            }
        }
    }
}
