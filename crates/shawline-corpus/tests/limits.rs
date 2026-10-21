//! Limits too large to keep as corpus files: these sheets are generated.

/// `·𐑚𐑦𐑜` with `inputs` unused inputs, beside a main that gives 1.
fn wide(inputs: usize) -> String {
    let word = |i: usize| -> String {
        [i / (48 * 48), i / 48 % 48, i % 48].iter().map(|&k| char::from_u32(0x10450 + k as u32).unwrap()).collect()
    };
    let mut src = String::from("╭─ ·𐑚𐑦𐑜 ──╮\n");
    for i in 0..inputs {
        src += &format!("▶ {}     │\n", word(i));
    }
    src + "╰─────────╯\n\n╭─ ·𐑥𐑱𐑯 ──────╮\n│             │\n│ 1 ──────────▶ 𐑬\n│             │\n╰─────────────╯\n"
}

/// A frame holds 65,535 registers (SPEC §15). A definition that fills one
/// compiles, even uncalled; one more input is `C007` at its namer dot.
#[test]
fn frame_limit() {
    let fits = shawline_cli::check(&wide(65_535)).unwrap();
    assert!(!fits.has_errors());
    let values = shawline_cli::run(fits.program.as_ref().unwrap()).unwrap();
    assert_eq!(values.iter().map(|v| v.to_string()).collect::<Vec<_>>(), ["1"]);

    let over = shawline_cli::check(&wide(65_536)).unwrap();
    let errors: Vec<String> =
        over.diagnostics.iter().filter(|d| !d.is_warning()).map(|d| format!("{} {}", d.at, d.code)).collect();
    assert_eq!(errors, ["0:3 C007"]);
}
