use super::{condition, specialize};
use crate::ast::{Cond, Location, Node, Os, Part, Shell, Statement, Target, Word};
use crate::parser::parse;

const TARGETS: [Target; 4] = [
    Target {
        shell: Shell::Bash,
        os: Os::Linux,
    },
    Target {
        shell: Shell::Bash,
        os: Os::Darwin,
    },
    Target {
        shell: Shell::Fish,
        os: Os::Linux,
    },
    Target {
        shell: Shell::Powershell,
        os: Os::Windows,
    },
];
const ANSWERS: [Result<bool, ()>; 3] = [Ok(false), Ok(true), Err(())];
type Answers = [Result<bool, ()>; 2];

// Independent interpreter: no specialization rules, just ordered evaluation.
fn evaluate<'a>(
    cond: &'a Cond,
    target: Target,
    answers: Answers,
    events: &mut Vec<Event<'a>>,
) -> Result<bool, &'a Cond> {
    let name = match cond {
        Cond::Constant(value) => return Ok(*value),
        Cond::Shell(shell) => return Ok(*shell == target.shell),
        Cond::Os(os) => return Ok(*os == target.os),
        Cond::Not(inner) => return evaluate(inner, target, answers, events).map(|value| !value),
        Cond::And(left, right) => {
            return Ok(evaluate(left, target, answers, events)?
                && evaluate(right, target, answers, events)?);
        }
        Cond::Or(left, right) => {
            return Ok(evaluate(left, target, answers, events)?
                || evaluate(right, target, answers, events)?);
        }
        Cond::Env(name) => name.as_str(),
        Cond::Have(word) | Cond::Exists(word) => {
            word.literal().expect("oracle uses literal query names")
        }
    };
    events.push(Event::Query(cond));
    let index = match name {
        "a" => 0,
        "b" => 1,
        _ => panic!("unknown oracle query"),
    };
    answers[index].map_err(|()| cond)
}

#[derive(Debug, PartialEq, Eq)]
enum Event<'a> {
    Query(&'a Cond),
    Effect(&'a Node),
}

#[derive(Debug, PartialEq, Eq)]
struct Observation<'a> {
    result: Result<(), &'a Cond>,
    events: Vec<Event<'a>>,
}

fn observe(nodes: &[Node], target: Target, answers: Answers) -> Observation<'_> {
    fn run<'a>(
        nodes: &'a [Node],
        target: Target,
        answers: Answers,
        events: &mut Vec<Event<'a>>,
    ) -> Result<(), &'a Cond> {
        for node in nodes {
            match &node.kind {
                Statement::If(branch) => {
                    let mut chosen = &branch.otherwise;
                    for (cond, body) in &branch.branches {
                        if evaluate(cond, target, answers, events)? {
                            chosen = body;
                            break;
                        }
                    }
                    run(chosen, target, answers, events)?;
                }
                Statement::Raw { shell, .. } if *shell != target.shell => {}
                _ => events.push(Event::Effect(node)), // Observe opaque statements without executing them.
            }
        }
        Ok(())
    }
    let mut events = Vec::new();
    let result = run(nodes, target, answers, &mut events);
    Observation { result, events }
}

fn normalized(cond: &Cond) -> bool {
    match cond {
        Cond::Shell(_) | Cond::Os(_) => false,
        Cond::Not(inner) => {
            normalized(inner) && !matches!(**inner, Cond::Constant(_) | Cond::Not(_))
        }
        Cond::And(left, right) => {
            normalized(left)
                && normalized(right)
                && !matches!(**left, Cond::Constant(_))
                && !matches!(**right, Cond::Constant(true))
        }
        Cond::Or(left, right) => {
            normalized(left)
                && normalized(right)
                && !matches!(**left, Cond::Constant(_))
                && !matches!(**right, Cond::Constant(false))
        }
        _ => true,
    }
}

fn normalized_nodes(nodes: &[Node], target: Target) -> bool {
    nodes.iter().all(|node| match &node.kind {
        Statement::If(branch) => {
            !branch.branches.is_empty()
                && branch.branches.iter().all(|(cond, body)| {
                    !matches!(cond, Cond::Constant(_))
                        && normalized(cond)
                        && normalized_nodes(body, target)
                })
                && normalized_nodes(&branch.otherwise, target)
        }
        Statement::Raw { shell, .. } => *shell == target.shell,
        _ => true,
    })
}

// Arena indexes avoid cloning generated expression trees, even in the tests.
#[derive(Clone, Copy)]
enum Expr {
    False,
    True,
    A,
    B,
    Shell,
    Os,
    Not(usize),
    And(usize, usize),
    Or(usize, usize),
}

fn lower(arena: &[Expr], index: usize) -> Cond {
    match arena[index] {
        Expr::False => Cond::Constant(false),
        Expr::True => Cond::Constant(true),
        Expr::A => Cond::Env("a".into()),
        Expr::B => Cond::Env("b".into()),
        Expr::Shell => Cond::Shell(Shell::Bash),
        Expr::Os => Cond::Os(Os::Linux),
        Expr::Not(inner) => Cond::Not(Box::new(lower(arena, inner))),
        Expr::And(left, right) => {
            Cond::And(Box::new(lower(arena, left)), Box::new(lower(arena, right)))
        }
        Expr::Or(left, right) => {
            Cond::Or(Box::new(lower(arena, left)), Box::new(lower(arena, right)))
        }
    }
}

#[test]
fn bounded_trees_preserve_results_errors_and_query_order() {
    let mut arena = vec![
        Expr::False,
        Expr::True,
        Expr::A,
        Expr::B,
        Expr::Shell,
        Expr::Os,
    ];
    let mut levels = vec![Vec::new(), (0..arena.len()).collect::<Vec<_>>()];
    for size in 2..=7 {
        let mut level = Vec::new();
        let mut add = |expr| {
            level.push(arena.len());
            arena.push(expr);
        };
        for &inner in &levels[size - 1] {
            add(Expr::Not(inner));
        }
        for left_size in 1..size - 1 {
            for &left in &levels[left_size] {
                for &right in &levels[size - left_size - 1] {
                    add(Expr::And(left, right));
                    add(Expr::Or(left, right));
                }
            }
        }
        levels.push(level);
    }
    assert_eq!(arena.len(), 90_690);
    let mut comparisons = 0;
    for index in 0..arena.len() {
        for target in TARGETS {
            let original = lower(&arena, index);
            let specialized = condition(lower(&arena, index), target);
            assert!(normalized(&specialized), "{specialized:?}");
            let mut before = Vec::new();
            let mut after = Vec::new();
            for a in ANSWERS {
                for b in ANSWERS {
                    before.clear();
                    after.clear();
                    assert_eq!(
                        evaluate(&original, target, [a, b], &mut before),
                        evaluate(&specialized, target, [a, b], &mut after)
                    );
                    assert_eq!(before, after);
                    comparisons += 1;
                }
            }
            assert_eq!(condition(original, target), condition(specialized, target));
        }
    }
    assert_eq!(comparisons, 3_264_840);
}

#[test]
fn bounded_branch_chains_preserve_prior_effects_failures_and_first_match() {
    const CONDITIONS: [&str; 10] = [
        "false",
        "true",
        "env a",
        "env b",
        "shell bash",
        "os linux",
        "not env a",
        "env a and false",
        "env a or true",
        "env a and os linux",
    ];
    let mut comparisons = 0;
    for length in 0..=3u32 {
        for encoding in 0..10usize.pow(length) {
            for empty in [false, true] {
                for has_else in [false, true] {
                    let mut source = String::from("set BEFORE kept\n");
                    let mut code = encoding;
                    for branch in 0..length {
                        source.push_str(if branch == 0 { "if " } else { "elif " });
                        source.push_str(CONDITIONS[code % 10]);
                        code /= 10;
                        source.push('\n');
                        if !empty {
                            source.push_str(&format!("set BODY{branch} selected\n"));
                        }
                    }
                    if has_else {
                        if length > 0 {
                            source.push_str("else\n");
                        }
                        source.push_str("set FALLBACK selected\n");
                    }
                    if length > 0 {
                        source.push_str("end\n");
                    }
                    source.push_str("set AFTER kept\n");
                    let original = parse(&source).unwrap();
                    for target in TARGETS {
                        let specialized = specialize(parse(&source).unwrap(), target);
                        assert!(normalized_nodes(&specialized, target), "{source}");
                        for a in ANSWERS {
                            for b in ANSWERS {
                                assert_eq!(
                                    observe(&original, target, [a, b]),
                                    observe(&specialized, target, [a, b]),
                                    "{source}"
                                );
                                comparisons += 1;
                            }
                        }
                        assert_eq!(
                            specialize(parse(&source).unwrap(), target),
                            specialize(specialized, target)
                        );
                    }
                }
            }
        }
    }
    assert_eq!(comparisons, 159_984);
}

#[test]
fn absorbing_right_operands_and_empty_bodies_do_not_swallow_failure() {
    for leaf in ["env a", "have a", "exists a"] {
        for expression in [format!("{leaf} and false"), format!("{leaf} or true")] {
            let source = format!("set BEFORE kept\nif {expression}\nend\nset AFTER kept");
            let original = parse(&source).unwrap();
            let specialized = specialize(parse(&source).unwrap(), TARGETS[0]);
            assert_eq!(specialized.len(), 3);
            let outcome = observe(&specialized, TARGETS[0], [Err(()), Ok(true)]);
            let Statement::If(branch) = &original[1].kind else {
                panic!()
            };
            let (Cond::And(query, _) | Cond::Or(query, _)) = &branch.branches[0].0 else {
                panic!()
            };
            assert_eq!(outcome.result, Err(&**query));
            assert_eq!(
                outcome.events,
                [Event::Effect(&original[0]), Event::Query(query)]
            );
            // Replacing the condition by its predictable truth value loses this error.
            assert_ne!(outcome.result, Ok(()));
        }
    }
}

#[test]
fn inlining_preserves_locations_and_all_opaque_values() {
    let source = r#"if shell bash
set a yes
alias wrapper = tool "" "${env:a}" --key="hello # users"
init tool "${home}" "${shell}"
path+ relative
unset OLD
raw bash <<CODE
# shell comment
end
' unmatched quote
CODE
if env a
  if false
    set DEAD no
  elif env b
    set INNER yes
  end
elif shell bash
  set FALLBACK yes
elif env b
  set DEAD no
end
else
set DEAD no
end
"#;
    let original = parse(source).unwrap();
    let specialized = specialize(parse(source).unwrap(), TARGETS[0]);
    let Statement::If(outer) = &original[0].kind else {
        panic!()
    };
    assert_eq!(specialized[..6], outer.branches[0].1[..6]);
    let Statement::If(inner) = &specialized[6].kind else {
        panic!()
    };
    assert_eq!(inner.branches.len(), 1);
    assert_eq!(inner.otherwise.len(), 1);
    assert!(normalized_nodes(&specialized, TARGETS[0]));
    for a in ANSWERS {
        for b in ANSWERS {
            assert_eq!(
                observe(&original, TARGETS[0], [a, b]),
                observe(&specialized, TARGETS[0], [a, b])
            );
        }
    }
}

#[test]
fn raw_selection_keeps_exact_payload_and_never_evaluates_words() {
    let source = "set VALUE \"${env:UNKNOWN}\"\r\nraw bash <<B\r\n'${not_dsl}\r\nB\r\nraw powershell <<P\r\n' unclosed\r\nP";
    for target in TARGETS {
        let original = parse(source).unwrap();
        let specialized = specialize(parse(source).unwrap(), target);
        assert_eq!(original[0], specialized[0]);
        assert!(normalized_nodes(&specialized, target));
        assert_eq!(
            observe(&original, target, [Ok(false); 2]),
            observe(&specialized, target, [Ok(false); 2])
        );
    }
    // The word is data, not a compile-time environment lookup or global replacement.
    let word = Word {
        at: Location { line: 1, column: 1 },
        parts: vec![Part::Home, Part::Env("MISSING".into()), Part::TargetShell],
    };
    let original = Cond::Exists(word);
    let unchanged = condition(
        Cond::Exists(Word {
            at: Location { line: 1, column: 1 },
            parts: vec![Part::Home, Part::Env("MISSING".into()), Part::TargetShell],
        }),
        TARGETS[0],
    );
    assert_eq!(original, unchanged);
}
