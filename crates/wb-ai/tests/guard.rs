//! AI-truth guard (P9 acceptance, CLAUDE.md rule 1, ADR-045, ADR-046): `wb-ai`
//! compiles against a picture-only API. The crate's only dependencies are
//! `wb-core` and `serde`, so it cannot name an engine truth type, the umpire
//! view or the test bench: those live in `wb-sim`, which it cannot import. This
//! test pins that dependency list, and checks that the engine's adapter (the one
//! place the AI touches the engine) never reads a contact's hidden truth key or
//! the truth-only tools.

use std::path::Path;

fn read(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Dependency names of the `[dependencies]` table.
fn dependencies(toml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in toml.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            inside = line == "[dependencies]";
            continue;
        }
        if inside && !line.is_empty() && !line.starts_with('#') {
            let name = line.split(['=', '.']).next().unwrap_or("").trim();
            out.push(name.to_owned());
        }
    }
    out.sort();
    out
}

#[test]
fn wb_ai_depends_on_wb_core_and_serde_only() {
    let deps = dependencies(&read("Cargo.toml"));
    assert_eq!(
        deps,
        vec!["serde".to_owned(), "wb-core".to_owned()],
        "wb-ai must not depend on the engine, the content, the packs or the test bench"
    );
    assert!(!read("Cargo.toml").contains("[dev-dependencies]\nwb-sim"));
}

#[test]
fn wb_ai_names_no_engine_or_truth_api() {
    for file in ["src/lib.rs", "src/brain.rs", "src/picture.rs"] {
        // Code lines only: the docs say what the crate cannot see.
        let src: String = read(file)
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for banned in [
            "wb_sim",
            "wb_content",
            "wb_geo",
            "umpire",
            "testbench",
            "truth",
        ] {
            assert!(!src.contains(banned), "{file} mentions `{banned}`");
        }
    }
}

#[test]
fn the_adapter_reads_only_the_sides_picture() {
    let adapter = read("../wb-sim/src/ai.rs");
    // Code lines only (the module docs explain what is not read).
    let code: String = adapter
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    for banned in [
        ".truth",
        "umpire",
        "testbench",
        "by_truth",
        "first_seen",
        "TruthHit",
    ] {
        assert!(!code.contains(banned), "the AI adapter uses `{banned}`");
    }
    // Contacts come from the AI's own side's picture, by its own index.
    assert!(code.contains("pics.sides.get(usize::from(side))"));
}
