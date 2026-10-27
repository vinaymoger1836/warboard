//! Fixture tests: the good set loads; every file in `fixtures/bad/` fails with
//! the message in its `# expect:` line.
//!
//! A bad fixture holds one or more files, each starting with
//! `# file: <path in the content folder>`. They are added to the good set.

use std::path::{Path, PathBuf};
use wb_content::source::{Entry, platform::SensorMount};
use wb_content::{SourceFile, load_files};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn read_tree(root: &Path, dir: &Path, out: &mut Vec<SourceFile>) {
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            read_tree(root, &p, out);
        } else {
            out.push(SourceFile {
                path: p
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
                text: std::fs::read_to_string(&p).unwrap(),
            });
        }
    }
}

fn good_files() -> Vec<SourceFile> {
    let root = fixtures().join("good");
    let mut files = Vec::new();
    read_tree(&root, &root, &mut files);
    let realism = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/realism/default.yaml");
    files.push(SourceFile {
        path: "realism/default.yaml".into(),
        text: std::fs::read_to_string(realism).unwrap(),
    });
    files
}

/// Splits a bad fixture into its files and expected messages.
fn parse_bad(text: &str) -> (Vec<SourceFile>, Vec<String>) {
    let mut files: Vec<SourceFile> = Vec::new();
    let mut expect = Vec::new();
    for line in text.lines() {
        if let Some(e) = line.strip_prefix("# expect: ") {
            expect.push(e.to_owned());
        } else if let Some(p) = line.strip_prefix("# file: ") {
            files.push(SourceFile {
                path: p.trim().to_owned(),
                text: String::new(),
            });
        } else if let Some(f) = files.last_mut() {
            f.text.push_str(line);
            f.text.push('\n');
        }
    }
    (files, expect)
}

#[test]
fn good_fixtures_load_and_resolve_extends() {
    let set = load_files("content", good_files()).unwrap_or_else(|errors| {
        panic!(
            "good fixtures failed:\n{}",
            errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        )
    });
    let x = set
        .entries
        .iter()
        .find(|e| e.id == "test-fighter-x")
        .expect("custom entry loaded");
    assert!(x.custom);
    let Entry::Platform(p) = &x.entry else {
        panic!("not a platform")
    };
    // Overridden.
    assert_eq!(p.mobility.max_speed_mach, Some(1.8));
    assert_eq!(p.stations[1].max_kg, Some(1200.0));
    assert_eq!(p.name, "Test fighter X (custom)");
    // Inherited.
    assert_eq!(p.mobility.ceiling_m, Some(15_000.0));
    assert_eq!(p.sensors, vec![SensorMount::Id("test-radar".into())]);
    assert_eq!(p.turnaround_min, Some(60.0));
    // Compiles and round-trips.
    let db = wb_content::compile(&set);
    let (_, bytes) = wb_content::encode(&db, "0.0.1");
    assert_eq!(wb_content::decode(&bytes).unwrap().1, db);
}

#[test]
fn bad_fixtures_fail_with_expected_message() {
    let dir = fixtures().join("bad");
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    names.sort();
    assert!(names.len() >= 20, "expected the full set of bad fixtures");
    let mut failures = Vec::new();
    for path in names {
        let text = std::fs::read_to_string(&path).unwrap();
        let (bad, expect) = parse_bad(&text);
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            !bad.is_empty() && !expect.is_empty(),
            "{name}: needs `# file:` and `# expect:` lines"
        );
        let mut files = good_files();
        files.extend(bad);
        match load_files("content", files) {
            Ok(_) => failures.push(format!("{name}: loaded without errors")),
            Err(errors) => {
                let shown: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
                for e in &expect {
                    if !shown.iter().any(|s| s.contains(e.as_str())) {
                        failures.push(format!(
                            "{name}: expected `{e}`\n  got:\n    {}",
                            shown.join("\n    ")
                        ));
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
