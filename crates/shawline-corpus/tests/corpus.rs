use std::{env, fs};

use shawline_corpus::{STAGES, programs, root};

#[test]
fn corpus() {
    let bless = env::var("SHAWLINE_BLESS").unwrap_or_default();
    let blessing = |stage: &str| bless.split(',').any(|b| b == stage || b == "all");
    let mut failures = Vec::new();
    let mut checked = 0;

    for program in programs(&root()) {
        let src = fs::read_to_string(&program).unwrap();
        for stage in STAGES {
            let expected_path = program.with_extension(stage.name);
            let stored = fs::read_to_string(&expected_path).ok();
            if blessing(stage.name) {
                if stored.is_some() || !stage.selective {
                    fs::write(&expected_path, (stage.run)(&src)).unwrap();
                }
                continue;
            }
            let expected = stored.or_else(|| stage.fallback.and_then(|f| f(&program, &src)));
            let Some(expected) = expected else { continue };
            checked += 1;
            let actual = (stage.run)(&src);
            if actual != expected {
                let name = expected_path.file_name().unwrap().to_string_lossy();
                failures.push(format!("── {name}\nexpected:\n{expected}\nactual:\n{actual}"));
            }
        }
    }

    assert!(checked > 0 || !bless.is_empty(), "no corpus expectations found under {:?}", root());
    assert!(
        failures.is_empty(),
        "{} of {checked} corpus checks failed\n\n{}",
        failures.len(),
        failures.join("\n")
    );
}
