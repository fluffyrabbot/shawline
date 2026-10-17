//! The VM, driven by `.shawasm` programs.

use shawline_grid::{Diagnostic, Pos};
use shawline_vm::bytecode::Value;
use shawline_vm::{assemble, equal, run};

fn exec(src: &str) -> Result<Vec<Value>, Diagnostic> {
    run(&assemble(src).unwrap_or_else(|d| panic!("{d}\n{src}")))
}

/// Runs a program and prints its results the way the CLI does.
fn output(src: &str) -> String {
    let values = exec(src).unwrap_or_else(|d| panic!("{d}"));
    values.iter().map(|v| format!("{v}\n")).collect()
}

fn fails(src: &str) -> (Pos, &'static str) {
    let d = exec(src).expect_err("a runtime error");
    (d.at, d.code)
}

const FACTORIAL: &str = "
fn 𐑓𐑨𐑒𐑑 params 1 results 1 registers 4   ; n! = n * (n-1)!, not a tail call
  const k0 0
  const k1 1
  jne r0 k0 3 at 1:4
  const r1 k1
  ret r1 1
  const r1 k1
  prim 𐑚 r2 r0,r1 at 2:5
  call 𐑓𐑨𐑒𐑑 r2 1 r3 at 2:9
  prim 𐑑 r1 r0,r3 at 3:7
  ret r1 1
end
fn 𐑥𐑱𐑯 params 0 results 1 registers 2
  const k0 N
  const r0 k0
  call 𐑓𐑨𐑒𐑑 r0 1 r1
  ret r1 1
end
main 𐑥𐑱𐑯
";

#[test]
fn factorial() {
    assert_eq!(output(&FACTORIAL.replace(" N", " 20")), "2432902008176640000\n");
}

#[test]
fn factorial_overflow_points_into_the_callee() {
    assert_eq!(fails(&FACTORIAL.replace(" N", " 21")), (Pos::new(3, 7), "R001"));
}

const SUM: &str = "
fn 𐑕𐑳𐑥 params 1 results 1 registers 3   ; n + sum(n-1)
  const k0 0
  const k1 1
  jne r0 k0 2
  ret r0 1
  const r1 k1
  prim 𐑚 r1 r0 r1
  call 𐑕𐑳𐑥 r1 1 r2
  prim 𐑐 r1 r0,r2
  ret r1 1
end
fn 𐑥𐑱𐑯 params 0 results 1 registers 2
  const k0 100000
  const r0 k0
  call 𐑕𐑳𐑥 r0 1 r1
  ret r1 1
end
main 𐑥𐑱𐑯
";

#[test]
fn deep_non_tail_recursion() {
    // Each level is a frame; host recursion would overflow the 2 MiB test
    // thread long before 100 000.
    assert_eq!(output(SUM), "5000050000\n");
}

#[test]
fn mutual_recursion() {
    let src = "
fn 𐑰𐑝𐑩𐑯 params 1 results 1 registers 2
  const k0 0
  const k1 1
  jne r0 k0 3
  const r1 k1
  ret r1 1
  const r1 k1
  prim 𐑚 r0 r0,r1
  tail 𐑪𐑛 r0 1
end
fn 𐑪𐑛 params 1 results 1 registers 2
  const k0 0
  const k1 1
  jne r0 k0 3
  const r1 k0
  ret r1 1
  const r1 k1
  prim 𐑚 r0 r0,r1
  tail 𐑰𐑝𐑩𐑯 r0 1
end
fn 𐑥𐑱𐑯 params 0 results 2 registers 4
  const k0 300001
  const k1 7
  const r0 k0
  call 𐑰𐑝𐑩𐑯 r0 1 r2
  const r1 k1
  call 𐑪𐑛 r1 1 r3
  ret r2 2
end
main 𐑥𐑱𐑯
";
    assert_eq!(output(src), "0\n1\n");
}

const SWITCH: &str = "
fn 𐑯𐑱𐑥 params 1 results 1 registers 2   ; 1 → «𐑢𐑳𐑯», «𐑑𐑵» → 2, (1 2) → nil, else no match
  const k0 1
  const k1 «𐑢𐑳𐑯»
  const k2 «𐑑𐑵»
  const k3 2
  const k4 (1 2)
  const k5 nil
  jne r0 k0 3 at 5:2
  const r1 k1
  jump 10
  jne r0 k2 6 at 5:2
  const r1 k3
  jump 10
  jne r0 k4 9 at 5:2
  const r1 k5
  jump 10
  nomatch at 5:2
  ret r1 1
end
fn 𐑥𐑱𐑯 params 0 results 3 registers 6
  const k0 1
  const k1 «𐑑𐑵»
  const k2 2
  const r0 k0
  call 𐑯𐑱𐑥 r0 1 r3
  const r0 k1
  call 𐑯𐑱𐑥 r0 1 r4
  const r0 k0
  const r1 k2
  prim 𐑒 r2 r0,r1
  call 𐑯𐑱𐑥 r2 1 r5
  ret r3 3
end
main 𐑥𐑱𐑯
";

#[test]
fn switch() {
    assert_eq!(output(SWITCH), "𐑢𐑳𐑯\n2\n[]\n");
}

#[test]
fn switch_without_a_matching_arm() {
    let src = SWITCH.replace("const k2 2\n  const r0", "const k2 3\n  const r0");
    assert_eq!(fails(&src), (Pos::new(5, 2), "R005"));
}

/// A main that applies `letter` to the constants `args` and returns the
/// result(s), failing at 7:3.
fn prim(letter: &str, args: &[&str]) -> Result<Vec<Value>, Diagnostic> {
    let outs = if matches!(letter, "𐑜" | "𐑕") { 2 } else { 1 };
    let consts: String =
        args.iter().enumerate().map(|(i, a)| format!("  const k{i} {a}\n")).collect();
    let loads: String = (0..args.len()).map(|i| format!("  const r{i} k{i}\n")).collect();
    let srcs: Vec<String> = (0..args.len()).map(|i| format!("r{i}")).collect();
    let dsts: Vec<String> = (0..outs).map(|i| format!("r{}", 2 + i)).collect();
    exec(&format!(
        "fn 𐑥𐑱𐑯 params 0 results {outs} registers 4\n{consts}{loads}  prim {letter} {} {} at 7:3\n  ret r2 {outs}\nend\nmain 𐑥𐑱𐑯\n",
        dsts.join(","),
        srcs.join(","),
    ))
}

fn ok(letter: &str, args: &[&str]) -> String {
    let values = prim(letter, args).unwrap_or_else(|d| panic!("{letter} {args:?}: {d}"));
    values.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" ")
}

#[test]
fn primitives() {
    let cases: &[(&str, &[&str], &str)] = &[
        ("𐑐", &["2", "-5"], "-3"),
        ("𐑚", &["2", "-5"], "7"),
        ("𐑑", &["-4", "6"], "-24"),
        ("𐑛", &["-7", "2"], "-3"),
        ("𐑛", &["7", "-2"], "-3"),
        ("𐑤", &["1", "2"], "1"),
        ("𐑤", &["2", "2"], "0"),
        ("𐑮", &["3", "2"], "1"),
        ("𐑥", &["3", "-2"], "-2"),
        ("𐑯", &["3", "-2"], "3"),
        ("𐑦", &["(1 «𐑨»)", "(1 «𐑨»)"], "1"),
        ("𐑦", &["[1 2]", "[1 2]"], "1"),
        ("𐑦", &["1", "«1»"], "0"),
        ("𐑦", &["nil", "(1 2)"], "0"),
        ("𐑰", &["[1 2]", "[1 3]"], "1"),
        ("𐑰", &["«𐑨»", "«𐑨»"], "0"),
        ("𐑒", &["1", "«𐑨»"], "(1 «𐑨»)"),
        ("𐑜", &["(1 «𐑨»)"], "1 𐑨"),
        ("𐑟", &["1", "[2 3]"], "[1 2 3]"),
        ("𐑕", &["[1 2 3]"], "1 [2 3]"),
        ("𐑖", &["-42"], "-42"),
        ("𐑠", &["«-17»"], "-17"),
        ("𐑠", &["«007»"], "7"),
        ("𐑠", &["«-9223372036854775808»"], "-9223372036854775808"),
    ];
    for (letter, args, want) in cases {
        assert_eq!(ok(letter, args), *want, "{letter} {args:?}");
    }
    // Show makes Text, not an Int that prints alike.
    assert_eq!(prim("𐑖", &["5"]).unwrap(), [Value::Text("5".into())]);
}

#[test]
fn runtime_errors() {
    let max = "9223372036854775807";
    let min = "-9223372036854775808";
    let cases: &[(&str, &[&str], &str)] = &[
        ("𐑐", &[max, "1"], "R001"),
        ("𐑚", &[min, "1"], "R001"),
        ("𐑑", &[max, "2"], "R001"),
        ("𐑛", &[min, "-1"], "R001"),
        ("𐑠", &["«9223372036854775808»"], "R001"),
        ("𐑛", &["1", "0"], "R002"),
        ("𐑕", &["nil"], "R003"),
        ("𐑐", &["1", "«1»"], "R004"),
        ("𐑤", &["nil", "1"], "R004"),
        ("𐑯", &["1", "(1 2)"], "R004"),
        ("𐑜", &["[1]"], "R004"),
        ("𐑟", &["1", "(1 2)"], "R004"),
        ("𐑕", &["(1 2)"], "R004"),
        ("𐑖", &["«1»"], "R004"),
        ("𐑠", &["1"], "R004"),
        ("𐑠", &["«12𐑨»"], "R006"),
        ("𐑠", &["«-»"], "R006"),
        ("𐑠", &["«+1»"], "R006"),
        ("𐑠", &["«»"], "R006"),
        ("𐑠", &["« 1»"], "R006"),
    ];
    for (letter, args, code) in cases {
        let d = prim(letter, args).expect_err(&format!("{letter} {args:?} fails"));
        assert_eq!((d.at, d.code), (Pos::new(7, 3), *code), "{letter} {args:?}: {d}");
    }
}

const BUILD: &str = "
fn 𐑚𐑦𐑤𐑛 params 2 results 1 registers 3   ; build(n, acc) = build(n-1, n:acc)
  const k0 0
  const k1 1
  jne r0 k0 2
  ret r1 1
  prim 𐑟 r1 r0,r1
  const r2 k1
  prim 𐑚 r0 r0,r2
  tail 𐑚𐑦𐑤𐑛 r0 2
end
fn 𐑥𐑱𐑯 params 0 results 3 registers 6
  const k0 1000000
  const k1 nil
  const k2 0
  const r0 k0
  const r1 k1
  call 𐑚𐑦𐑤𐑛 r0 2 r2
  call 𐑚𐑦𐑤𐑛 r0 2 r3
  TAIL
  prim 𐑦 r4 r2,r3
  ret r2 3
end
main 𐑥𐑱𐑯
";

#[test]
fn long_lists() {
    // Two separately built, equal million-element lists.
    let values = exec(&BUILD.replace("TAIL", "")).unwrap();
    let [a, b, same] = &values[..] else { panic!("three results") };
    assert_eq!(*same, Value::Int(1));
    assert!(equal(a, b));
    let shown = a.to_string();
    assert!(shown.starts_with("[1 2 3 4 "));
    assert!(shown.ends_with(" 999999 1000000]"));
    drop(values);

    // The second list ends in an extra 0, found only at the far end.
    let extra = "const r5 k2\n  const r1 k1\n  prim 𐑟 r1 r5,r1\n  call 𐑚𐑦𐑤𐑛 r0 2 r3";
    let values = exec(&BUILD.replace("call 𐑚𐑦𐑤𐑛 r0 2 r3\n  TAIL", extra)).unwrap();
    assert_eq!(values[2], Value::Int(0));
    assert!(!equal(&values[0], &values[1]));
}

#[test]
fn malformed_bytecode_is_rejected_before_running() {
    use shawline_vm::bytecode::{Function, Instr, Module};
    let f = Function {
        name: "𐑥𐑱𐑯".into(),
        params: 0,
        results: 1,
        registers: 1,
        consts: vec![],
        code: vec![Instr::Move { dst: 0, src: 3 }, Instr::Return { base: 0, count: 1 }],
        spans: vec![Pos::new(4, 4), Pos::new(0, 0)],
    };
    let d = run(&Module { functions: vec![f], main: 0 }).unwrap_err();
    assert_eq!((d.at, d.code), (Pos::new(4, 4), "A001"));
}
