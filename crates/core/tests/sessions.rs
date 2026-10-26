//! Recorded sessions in tests/fixtures/sessions/, each with the findings
//! checked by hand in a `.expect.json` next to it. A finding that isn't
//! expected is a false positive; an expected one that's gone is a miss.
//! Either fails the test, so a rule change shows what it does to them.

use std::collections::BTreeMap;
use std::path::Path;

use flowbuddy_core::session;

#[test]
fn recorded_sessions_find_what_was_checked() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/sessions");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no sessions in {}", dir.display());
    let mut total: BTreeMap<String, session::Score> = BTreeMap::new();
    let mut wrong = Vec::new();
    for file in &files {
        let entries = session::parse(&std::fs::read_to_string(file).unwrap())
            .unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        let found = session::counts(&session::replay(&entries));
        let expect_file = file.with_extension("expect.json");
        let expected: BTreeMap<String, u32> = serde_json::from_str(
            &std::fs::read_to_string(&expect_file)
                .unwrap_or_else(|_| panic!("{} has no {}", file.display(), expect_file.display())),
        )
        .unwrap();
        for (rule, s) in session::score(&expected, &found) {
            if s.extra + s.missed > 0 {
                let name = file.file_name().unwrap().to_string_lossy();
                wrong.push(format!(
                    "{name}: {rule}: {} extra, {} missed",
                    s.extra, s.missed
                ));
            }
            let t = total.entry(rule).or_default();
            t.hits += s.hits;
            t.extra += s.extra;
            t.missed += s.missed;
        }
    }
    eprintln!(
        "per rule over {} sessions (right / extra / missed):",
        files.len()
    );
    for (rule, s) in &total {
        eprintln!("  {rule:<28} {} / {} / {}", s.hits, s.extra, s.missed);
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
