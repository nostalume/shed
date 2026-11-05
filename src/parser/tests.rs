use crate::ast::{Cond, Location, Os, Part, PathDir, Shell, Statement, Target, Word};
use crate::parser::parse;

fn literal(word: &Word) -> &str {
    word.literal().expect("expected a literal word")
}

#[test]
fn declarations_keep_values_and_argument_boundaries() {
    let source = r##"
set EDITOR nvim
unset OLD
path+ bin
path- 'C:\tools\'
alias gs = git status --short
init tool "" --config="hello # users" "${env:FLAGS}"
"##;
    let nodes = parse(source).unwrap();
    assert_eq!(nodes.len(), 6);
    assert_eq!(nodes[0].at, Location { line: 2, column: 1 });
    let Statement::Set { name, value } = &nodes[0].kind else {
        panic!()
    };
    assert_eq!(name, "EDITOR");
    assert_eq!(literal(value), "nvim");
    assert!(matches!(&nodes[1].kind, Statement::Unset(name) if name == "OLD"));
    let Statement::Path { value, direction } = &nodes[2].kind else {
        panic!()
    };
    assert_eq!(*direction, PathDir::Prepend);
    assert_eq!(literal(value), "bin"); // no compiler-directory anchoring
    let Statement::Path { value, direction } = &nodes[3].kind else {
        panic!()
    };
    assert_eq!(*direction, PathDir::Append);
    assert_eq!(literal(value), "C:\\tools\\");
    let Statement::Alias { name, command } = &nodes[4].kind else {
        panic!()
    };
    assert_eq!(name, "gs");
    assert_eq!(literal(&command.program), "git");
    assert_eq!(
        command.args.iter().map(literal).collect::<Vec<_>>(),
        ["status", "--short"]
    );
    let Statement::Init(command) = &nodes[5].kind else {
        panic!()
    };
    assert_eq!(literal(&command.program), "tool");
    assert_eq!(command.args.len(), 3);
    assert_eq!(literal(&command.args[0]), "");
    assert_eq!(literal(&command.args[1]), "--config=hello # users");
    assert_eq!(command.args[2].parts, [Part::Env("FLAGS".into())]);
}

#[test]
fn words_preserve_expansion_types_and_literal_quotes() {
    let nodes = parse(
        r##"set VALUE pre' literal '"${env:ROOT}/${home}/${shell}"post
set LITERAL '${env:OTHER}'
set ESCAPED "\${env:OTHER}\n\r\t\\\""
set EMPTY ""
"##,
    )
    .unwrap();
    let Statement::Set { value, .. } = &nodes[0].kind else {
        panic!()
    };
    assert_eq!(
        value.parts,
        [
            Part::Literal("pre literal ".into()),
            Part::Env("ROOT".into()),
            Part::Literal("/".into()),
            Part::Home,
            Part::Literal("/".into()),
            Part::TargetShell,
            Part::Literal("post".into()),
        ]
    );
    let Statement::Set { value, .. } = &nodes[1].kind else {
        panic!()
    };
    assert_eq!(literal(value), "${env:OTHER}");
    let Statement::Set { value, .. } = &nodes[2].kind else {
        panic!()
    };
    assert_eq!(literal(value), "${env:OTHER}\n\r\t\\\"");
    let Statement::Set { value, .. } = &nodes[3].kind else {
        panic!()
    };
    assert_eq!(literal(value), "");
}

#[test]
fn comments_only_start_at_whitespace_boundaries() {
    let nodes = parse(
        r##"
# ignored ' " ${unknown}
set A abc#def # discarded "
set B "hello # users"
set C "x"#suffix
set D ''#suffix
"##,
    )
    .unwrap();
    let values = nodes
        .iter()
        .map(|node| {
            let Statement::Set { value, .. } = &node.kind else {
                panic!()
            };
            literal(value)
        })
        .collect::<Vec<_>>();
    assert_eq!(values, ["abc#def", "hello # users", "x#suffix", "#suffix"]);
}

#[test]
fn precedence_grouping_and_operator_named_operands_are_structural() {
    let nodes = parse("if have a or have b and not have c\nend\nif (have a or have b) and have or\nend\nif have a and have and and have b\nend").unwrap();
    let Statement::If(first) = &nodes[0].kind else {
        panic!()
    };
    let Cond::Or(_, right) = &first.branches[0].0 else {
        panic!()
    };
    assert!(matches!(&**right, Cond::And(_, rhs) if matches!(&**rhs, Cond::Not(_))));
    let Statement::If(second) = &nodes[1].kind else {
        panic!()
    };
    let Cond::And(left, right) = &second.branches[0].0 else {
        panic!()
    };
    assert!(matches!(&**left, Cond::Or(_, _)));
    assert!(matches!(&**right, Cond::Have(word) if literal(word) == "or"));
    let Statement::If(third) = &nodes[2].kind else {
        panic!()
    };
    assert!(
        matches!(&third.branches[0].0, Cond::And(lhs, _) if matches!(&**lhs, Cond::And(_, rhs) if matches!(&**rhs, Cond::Have(word) if literal(word) == "and")))
    );
}

#[test]
fn branches_keep_order_empty_bodies_and_nested_statements() {
    let nodes = parse("if false\nelif shell powershell\n if env ENABLED\n  set A yes\n else\n end\nelif os darwin\nelse\n unset A\nend").unwrap();
    let Statement::If(branch) = &nodes[0].kind else {
        panic!()
    };
    assert_eq!(branch.branches.len(), 3);
    assert!(matches!(branch.branches[0].0, Cond::Constant(false)));
    assert!(branch.branches[0].1.is_empty());
    assert!(matches!(
        branch.branches[1].0,
        Cond::Shell(Shell::Powershell)
    ));
    assert!(matches!(branch.branches[2].0, Cond::Os(Os::Darwin)));
    let Statement::If(inner) = &branch.branches[1].1[0].kind else {
        panic!()
    };
    assert!(matches!(&inner.branches[0].0, Cond::Env(name) if name == "ENABLED"));
    assert!(inner.otherwise.is_empty());
    assert_eq!(branch.otherwise.len(), 1);
}

#[test]
fn raw_payload_is_opaque_and_preserves_original_line_endings() {
    for newline in ["\n", "\r\n"] {
        let body = format!(
            "  # shell comment{newline}end{newline}' unfinished quote{newline}$HOME ${{unknown}}{newline}"
        );
        let source = format!(
            "if false{newline}  raw  powershell <<CODE # comment{newline}{body}CODE{newline}end{newline}"
        );
        let nodes = parse(&source).unwrap();
        let Statement::If(branch) = &nodes[0].kind else {
            panic!()
        };
        let Statement::Raw {
            shell,
            body: actual,
        } = &branch.branches[0].1[0].kind
        else {
            panic!()
        };
        assert_eq!(*shell, Shell::Powershell);
        assert_eq!(*actual, body);
    }
    let nodes = parse("raw bash <<END\nEND").unwrap();
    assert!(matches!(&nodes[0].kind, Statement::Raw { body, .. } if body.is_empty()));
}

#[test]
fn malformed_or_incomplete_source_is_rejected_even_in_dead_branches() {
    for source in [
        "set A hello world",
        "set 1BAD x",
        "unset",
        "unset A B",
        "path+",
        "path- one two",
        "alias gs git status",
        "alias -gs = git",
        "alias gs =",
        "init",
        "set A $HOME",
        "set A \"$(date)\"",
        "set A \"\\q\"",
        "set A \"${unknown}\"",
        "set A \"${env:1BAD}\"",
        "set A \"${home\"",
        "set A \"unfinished",
        "if true\nend garbage",
        "if true\nelse garbage\nend",
        "if true\nelse\nelse\nend",
        "if true\nelse\nelif true\nend",
        "if true",
        "end",
        "else",
        "elif true",
        "if (true\nend",
        "if true)\nend",
        "if (true)#not_a_comment\nend",
        "if \"true\"\nend",
        "if have a garbage\nend",
        "if os plan9\nend",
        "if shell pwsh\nend",
        "if env \"bad key\"\nend",
        "raw bash <<END\n END\n",
        "raw bash <<END",
        "raw other <<END\nEND",
        "raw bash <<END extra\nEND",
        "raw bash <<1BAD\n1BAD",
        "if false\nunknown statement\nend",
        "if false\nset A \"${unknown}\"\nend",
        "if false\nraw other <<END\nEND\nend",
        "init tool | source",
        "set A '\0'",
        "raw bash <<END\na\0b\nEND",
    ] {
        assert!(parse(source).is_err(), "unexpectedly accepted: {source:?}");
    }
}

#[test]
fn locations_count_unicode_characters_and_preserve_original_lines() {
    let error = parse("# comment\n  set X 中$bad").unwrap_err();
    assert_eq!(
        error.at,
        Location {
            line: 2,
            column: 10
        }
    );
    assert!(error.to_string().contains("line 2, column 10"));
    let error = parse("\n  if true\n").unwrap_err();
    assert_eq!(error.at, Location { line: 2, column: 3 });
    let error = parse("# comment\n中\0").unwrap_err();
    assert_eq!(error.at, Location { line: 2, column: 2 });
}

#[test]
fn targets_use_canonical_names_and_the_approved_matrix() {
    assert!(Shell::parse("pwsh").is_none());
    assert!(Os::parse("macos").is_none());
    for shell in [Shell::Bash, Shell::Zsh, Shell::Fish, Shell::Powershell] {
        for os in [Os::Linux, Os::Darwin, Os::Windows] {
            assert_eq!(
                Target::new(shell, os).is_ok(),
                os != Os::Windows || shell == Shell::Powershell
            );
        }
    }
}

#[test]
fn literals_roundtrip_without_becoming_expansions_or_extra_words() {
    fn check(value: &str) {
        let mut quoted = String::from("\"");
        for ch in value.chars() {
            quoted.push_str(match ch {
                '\\' => "\\\\",
                '"' => "\\\"",
                '$' => "\\$",
                '\n' => "\\n",
                '\r' => "\\r",
                '\t' => "\\t",
                _ => {
                    quoted.push(ch);
                    continue;
                }
            });
        }
        quoted.push('"');
        let nodes = parse(&format!("init tool {quoted}")).unwrap();
        let Statement::Init(command) = &nodes[0].kind else {
            panic!()
        };
        assert_eq!(command.args.len(), 1);
        assert_eq!(literal(&command.args[0]), value);
    }
    const ALPHABET: [char; 24] = [
        'a', ' ', '#', '\'', '"', '$', '\\', '\n', '\t', ';', '(', ')', '>', '中', '\r', '😀', '‘',
        '’', '‚', '‛', '“', '”', '„', '‟',
    ];
    check("");
    for a in ALPHABET {
        check(&a.to_string());
        for b in ALPHABET {
            check(&format!("{a}{b}"));
            for c in ALPHABET {
                check(&format!("{a}{b}{c}"));
            }
        }
    }
}
