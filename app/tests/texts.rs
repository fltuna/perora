//! Every text the windows ask for by its key is in the language files. The core's own texts (the
//! notices, the menu) are checked in `perora_core::language`; the windows ask for theirs by key,
//! as `t("main.account.signIn")`, and a key misspelt there is shown as it is, in place of the text
//! (owner, 2026-11-07: 「てすとにしておこうか」).
//!
//! A key is told by its look: a string in the code, of letters, digits, `_` and dots, beginning
//! with a part that keys of the reference language begin with (`main`, `preview`, `tour`, …).
//! Keys made up as the program runs (`format!("main.output.format.{}", …)`) are not seen here.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use perora_core::language::Languages;

/// Where the windows' code is: the application's, the editor's, the shared parts'.
const SOURCES: [&str; 3] = ["src", "../crates/editor/src", "../crates/ui/src"];

fn rust_files(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, found);
        } else if path.extension().is_some_and(|ending| ending == "rs") {
            found.push(path);
        }
    }
}

/// The strings in a line of code (what is between one quote and the next: enough for keys, which
/// hold no quotes themselves).
fn strings(line: &str) -> impl Iterator<Item = &str> {
    line.split('"').skip(1).step_by(2)
}

#[test]
fn every_key_the_windows_ask_for_is_in_the_languages() {
    let english = Languages::load(None).by_code("en").unwrap();
    let reference: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(include_str!("../../lang/en.json")).unwrap();
    let beginnings: BTreeSet<&str> = reference
        .keys()
        .filter_map(|key| key.split('.').next())
        .collect();
    let looks_like_a_key = |text: &str| {
        text.contains('.')
            && !text.ends_with('.')
            && text
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
            && text
                .split('.')
                .next()
                .is_some_and(|first| beginnings.contains(first))
    };

    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for source in SOURCES {
        rust_files(&here.join(source), &mut files);
    }
    assert!(!files.is_empty(), "no code found under {}", here.display());

    let mut asked = 0;
    let mut missing: BTreeMap<String, String> = BTreeMap::new();
    for file in &files {
        let code = std::fs::read_to_string(file).unwrap();
        for (number, line) in code.lines().enumerate() {
            for key in strings(line).filter(|text| looks_like_a_key(text)) {
                asked += 1;
                // (A language has no text for a key when it gives the key back.)
                if english.text(key, &[]) == key {
                    missing
                        .entry(key.to_owned())
                        .or_insert_with(|| format!("{}:{}", file.display(), number + 1));
                }
            }
        }
    }
    // (The rule above finds the keys: were it to find none, it would check nothing.)
    assert!(asked > 100, "only {asked} keys found: is the rule still right?");
    assert!(
        missing.is_empty(),
        "keys the windows ask for that the language files do not have:\n{}",
        missing
            .iter()
            .map(|(key, at)| format!("  {key} ({at})"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
