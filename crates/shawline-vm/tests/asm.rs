//! `.shawasm`: the assembler, the disassembler and their round trip.

use shawline_grid::Pos;
use shawline_vm::{assemble, disassemble, run};

/// Canonical text: every instruction, every kind of constant, spans present
/// and absent, and a forward call. The disassembler must reproduce it exactly.
const CANONICAL: &str = "\
fn 𐑥𐑱𐑯 params 0 results 2 registers 6
  const k0 -21
  const k1 «𐑣𐑦 ; 𐑞𐑺»
  const k2 nil
  const k3 (1 [«𐑨» (2 3) nil])
  const r0 k0
  move r1 r0
  call 𐑛𐑳𐑚𐑩𐑤 r1 1 r2 at 3:14
  prim 𐑜 r3,r4 r5 at 2:0
  jne r2 k0 6 at 0:9
  tail 𐑛𐑳𐑚𐑩𐑤 r0 1
  jump 7
  nomatch at 8:1
end
fn 𐑛𐑳𐑚𐑩𐑤 params 1 results 2 registers 2
  prim 𐑐 r1 r0,r0
  ret r0 2
end
main 𐑥𐑱𐑯
";

#[test]
fn round_trip() {
    let module = assemble(CANONICAL).unwrap();
    assert_eq!(disassemble(&module), CANONICAL);
    assert_eq!(assemble(&disassemble(&module)).unwrap(), module);
}

#[test]
fn layout_and_the_doc_example() {
    // The example in bytecode.rs, plus a main: comments, blank lines,
    // indentation, and space-separated prim registers.
    let src = "
; doubles 21
fn 𐑛𐑳𐑚𐑩𐑤 params 1 results 1 registers 2
  const k0 21                ; consts: an integer, «text», or nil
  prim 𐑐 r1 r0 r0            ; prim <letter> <dst>[,<dst>] <src>[,<src>]
  ret r1 1                   ; ret <base> <count>
end

fn 𐑥𐑱𐑯 params 0 results 1 registers 2
const k0 21
const r0 k0
call 𐑛𐑳𐑚𐑩𐑤 r0 1 r1
ret r1 1
end
main 𐑥𐑱𐑯";
    let module = assemble(src).unwrap();
    assert_eq!(run(&module).unwrap()[0].to_string(), "42");
    assert!(disassemble(&module).contains("  prim 𐑐 r1 r0,r0\n"));
}

#[test]
fn errors_point_at_the_offending_token() {
    let wrap = |body: &str| {
        format!("fn 𐑥𐑱𐑯 params 0 results 1 registers 2\n{body}\n  ret r0 1\nend\nmain 𐑥𐑱𐑯\n")
    };
    let cases = [
        (wrap("  mov r0 r1"), Pos::new(1, 2)),
        (wrap("  move r0 r9"), Pos::new(1, 2)),
        (wrap("  move r0 x1"), Pos::new(1, 10)),
        (wrap("  move r0 r1 r1"), Pos::new(1, 13)),
        (wrap("  move r0"), Pos::new(1, 9)),
        (wrap("  const r0 k0"), Pos::new(1, 2)),
        (wrap("  const k1 1"), Pos::new(1, 8)),
        (wrap("  const k0 «𐑨"), Pos::new(1, 11)),
        (wrap("  const k0 +1"), Pos::new(1, 11)),
        (wrap("  prim 𐑐 r0 r1"), Pos::new(1, 9)),
        (wrap("  prim 𐑣 r0 r1,r1"), Pos::new(1, 7)),
        (wrap("  call 𐑯𐑴 r0 0 r0"), Pos::new(1, 7)),
        (wrap("  jump 9"), Pos::new(1, 2)),
        (wrap("  nomatch at 1"), Pos::new(1, 13)),
        (wrap("  ret r0 2"), Pos::new(1, 2)),
        ("fn 𐑥𐑱𐑯 params 0 results 0 registers 0\n  jump 0\n".into(), Pos::new(0, 0)),
        ("fn 𐑥𐑱𐑯 params 0 results 0 registers 0\nend\nmain 𐑥𐑱𐑯\n".into(), Pos::new(0, 0)),
        ("fn 𐑥𐑱𐑯 params 0 results 0 registers 0\n  jump 0\nend\n".into(), Pos::new(3, 0)),
        ("fn 𐑥𐑱𐑯 params 0 results 0 registers 0\n  jump 0\nend\nmain 𐑨\n".into(), Pos::new(3, 5)),
        ("fn 𐑥𐑱𐑯 params 1 results 0 registers 1\n  jump 0\nend\nmain 𐑥𐑱𐑯\n".into(), Pos::new(0, 0)),
        ("  jump 0\n".into(), Pos::new(0, 2)),
    ];
    for (src, at) in cases {
        let d = assemble(&src).expect_err(&src);
        assert_eq!((d.at, d.code), (at, "A001"), "{src}{d}");
    }
}

#[test]
fn constants_nest_at_most_ten_thousand_deep() {
    let nested = |depth: usize| {
        let value = format!("{}1{}", "[".repeat(depth), "]".repeat(depth));
        let body = format!("  const k0 {value}\n  const r0 k0\n  ret r0 1\nend\n");
        format!("fn 𐑥𐑱𐑯 params 0 results 1 registers 1\n{body}main 𐑥𐑱𐑯\n")
    };
    let m = assemble(&nested(10_000)).unwrap();
    assert!(disassemble(&m).contains(&format!("{}1]", "[".repeat(10_000))));
    let d = assemble(&nested(10_001)).unwrap_err();
    assert_eq!((d.at, d.code), (Pos::new(1, 10_011), "A001"));
}

#[test]
fn unused_primitive_slots_must_be_zero() {
    use shawline_vm::bytecode::{Function, Instr, Module, Prim};
    let f = Function {
        name: "𐑥𐑱𐑯".into(),
        params: 0,
        results: 1,
        registers: 2,
        consts: vec![],
        code: vec![
            Instr::Prim { prim: Prim::Show, dst: [0, 1], src: [0, 0] },
            Instr::Return { base: 0, count: 1 },
        ],
        spans: vec![Pos::new(2, 2), Pos::new(0, 0)],
    };
    let d = run(&Module { functions: vec![f], main: 0 }).unwrap_err();
    assert_eq!((d.at, d.code), (Pos::new(2, 2), "A001"));
}
