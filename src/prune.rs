//! Semantic pruning pass -- eliminates statically unreachable `if` branches.
//!
//! Only `Cond::Shell(s)` is statically foldable: it is always-true when
//! `s == shell` and always-false otherwise. `Cond::Have`, `Cond::Exists`, and
//! `Cond::Env` remain runtime checks; `Cond::Os` is folded for the Rust target.
//!
//! The pass is pure: no I/O, no global state. It runs after `resolve_paths`
//! and before `emit`.

use crate::ast::{Cond, IfNode, Node};

// -- public API ---------------------------------------------------------------

/// Prune a list of nodes for the given `shell` target.
///
/// Each `Node::If` is reduced:
/// - Always-true condition  -> inline `body` (elifs / else dropped).
/// - Always-false condition -> try each elif in order; inline the first
///   always-true elif body, else inline `else_`, else drop.
/// - Unknown condition      -> keep the `IfNode` but recursively prune all
///   sub-lists (`body`, elifs, `else_`).
///
/// Non-`If` nodes pass through unchanged.
#[cfg(test)]
pub fn prune_nodes(nodes: Vec<Node>, shell: &str) -> Vec<Node> {
    prune_nodes_for_target(nodes, shell, None)
}

/// Prune using an explicit output OS. `None` uses the binary's compilation
/// target and preserves the conservative behavior for unknown native targets.
pub fn prune_nodes_for_target(nodes: Vec<Node>, shell: &str, target_os: Option<&str>) -> Vec<Node> {
    // Worst case is identity (all nodes unknown); pre-size to len.
    let mut out = Vec::with_capacity(nodes.len());
    for n in nodes {
        match n {
            Node::If(inode) => out.extend(prune_if(inode, shell, target_os)),
            other => out.push(other),
        }
    }
    out
}

// -- private helpers ----------------------------------------------------------

/// Outcome of statically evaluating a condition for a target shell.
#[derive(Debug, Clone, PartialEq)]
enum CondResult {
    AlwaysTrue,
    AlwaysFalse,
    Unknown(Cond),
}

/// Reduce a `Cond` to a `CondResult` for the given `shell` target.
///
/// Compound conditions (`Not`, `And`, `Or`) are simplified by folding
/// any statically-known sub-conditions first.
fn prune_cond(cond: Cond, shell: &str, target_os: Option<&str>) -> CondResult {
    match cond {
        Cond::Shell(ref name) if name == shell => CondResult::AlwaysTrue,
        Cond::Shell(_) => CondResult::AlwaysFalse,

        // Os: fold statically at compile time via #[cfg(target_os)].
        // If the os string matches the compile-target we know it is always-true;
        // if it is a *known* other OS string it is always-false.
        // Unknown OS names are kept as Unknown so the runtime check fires.
        Cond::Os(ref name) => fold_os(name, target_os),

        // Have, Exists, and Env are runtime checks -- never folded.
        c @ (Cond::Have(_) | Cond::Exists(_) | Cond::Env(_)) => CondResult::Unknown(c),

        Cond::Not(inner) => match prune_cond(*inner, shell, target_os) {
            CondResult::AlwaysTrue => CondResult::AlwaysFalse,
            CondResult::AlwaysFalse => CondResult::AlwaysTrue,
            CondResult::Unknown(c) => CondResult::Unknown(Cond::Not(Box::new(c))),
        },

        Cond::And(lhs, rhs) => fold_and(
            prune_cond(*lhs, shell, target_os),
            prune_cond(*rhs, shell, target_os),
        ),
        Cond::Or(lhs, rhs) => fold_or(
            prune_cond(*lhs, shell, target_os),
            prune_cond(*rhs, shell, target_os),
        ),
    }
}

/// Short-circuit and identity folding for `And`.
fn fold_and(l: CondResult, r: CondResult) -> CondResult {
    match (l, r) {
        (CondResult::AlwaysFalse, _) | (_, CondResult::AlwaysFalse) => CondResult::AlwaysFalse,
        (CondResult::AlwaysTrue, r) => r,
        (l, CondResult::AlwaysTrue) => l,
        (CondResult::Unknown(lc), CondResult::Unknown(rc)) => {
            CondResult::Unknown(Cond::And(Box::new(lc), Box::new(rc)))
        }
    }
}

/// Short-circuit and identity folding for `Or`.
fn fold_or(l: CondResult, r: CondResult) -> CondResult {
    match (l, r) {
        (CondResult::AlwaysTrue, _) | (_, CondResult::AlwaysTrue) => CondResult::AlwaysTrue,
        (CondResult::AlwaysFalse, r) => r,
        (l, CondResult::AlwaysFalse) => l,
        (CondResult::Unknown(lc), CondResult::Unknown(rc)) => {
            CondResult::Unknown(Cond::Or(Box::new(lc), Box::new(rc)))
        }
    }
}

/// Fold `Cond::Os` at compile time using a single `#[cfg]`-chosen constant.
///
/// The known names are "darwin", "linux", and "windows".
/// Any name matching the compile-time target is `AlwaysTrue`;
/// any other *known* name is `AlwaysFalse`; unknown names stay `Unknown`
/// (runtime check emitted).
fn fold_os(name: &str, target_os: Option<&str>) -> CondResult {
    // Chosen once at compile time; at most one branch is active.
    #[cfg(target_os = "macos")]
    const CURRENT_OS: &str = "darwin";
    #[cfg(target_os = "linux")]
    const CURRENT_OS: &str = "linux";
    #[cfg(target_os = "windows")]
    const CURRENT_OS: &str = "windows";
    // Fall-back for any other (FreeBSD, Haiku, …) compilation target.
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    const CURRENT_OS: &str = "";

    const KNOWN: &[&str] = &["darwin", "linux", "windows"];

    let current_os = target_os.or_else(|| (!CURRENT_OS.is_empty()).then_some(CURRENT_OS));
    if current_os == Some(name) {
        CondResult::AlwaysTrue
    } else if current_os.is_some() && KNOWN.contains(&name) {
        CondResult::AlwaysFalse
    } else {
        CondResult::Unknown(Cond::Os(name.to_owned()))
    }
}

/// Reduce an `IfNode` to a list of replacement nodes.
fn prune_if(inode: IfNode, shell: &str, target_os: Option<&str>) -> Vec<Node> {
    let IfNode {
        cond,
        body,
        elifs,
        else_,
    } = inode;
    match prune_cond(cond, shell, target_os) {
        CondResult::AlwaysTrue => prune_nodes_for_target(body, shell, target_os),
        CondResult::AlwaysFalse => prune_false_head(elifs, else_, shell, target_os),
        CondResult::Unknown(kept) => vec![Node::If(IfNode {
            cond: kept,
            body: prune_nodes_for_target(body, shell, target_os),
            elifs: elifs
                .into_iter()
                .map(|(c, b)| (c, prune_nodes_for_target(b, shell, target_os)))
                .collect(),
            else_: prune_nodes_for_target(else_, shell, target_os),
        })],
    }
}

/// Handle an always-false head: walk elifs, fall through to `else_`.
///
/// - always-true elif  -> inline its body immediately.
/// - always-false elif -> skip (also dead).
/// - unknown elif      -> rebuild a new if-node with this elif as the head,
///   remaining elifs preserved, original `else_` kept.
/// - no elifs remain   -> inline `else_`.
fn prune_false_head(
    elifs: Vec<(Cond, Vec<Node>)>,
    else_: Vec<Node>,
    shell: &str,
    target_os: Option<&str>,
) -> Vec<Node> {
    let mut elifs = elifs.into_iter().peekable();
    while let Some((elif_cond, elif_body)) = elifs.next() {
        match prune_cond(elif_cond, shell, target_os) {
            CondResult::AlwaysFalse => continue,
            CondResult::AlwaysTrue => return prune_nodes_for_target(elif_body, shell, target_os),
            CondResult::Unknown(kept) => {
                return vec![Node::If(IfNode {
                    cond: kept,
                    body: prune_nodes_for_target(elif_body, shell, target_os),
                    elifs: elifs
                        .map(|(c, b)| (c, prune_nodes_for_target(b, shell, target_os)))
                        .collect(),
                    else_: prune_nodes_for_target(else_, shell, target_os),
                })];
            }
        }
    }
    prune_nodes_for_target(else_, shell, target_os)
}

// -- tests -------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Cond, IfNode, Node};

    fn set(key: &str) -> Node {
        Node::Set {
            key: key.into(),
            val: "1".into(),
        }
    }

    fn bare_if(cond: Cond, body: Vec<Node>) -> IfNode {
        IfNode {
            cond,
            body,
            elifs: vec![],
            else_: vec![],
        }
    }

    // -- multi-branch if/elif/else chain --------------------------------------

    /// Dead head, multiple dead elifs, first live elif inlined; rest dropped.
    #[test]
    fn multi_elif_chain_first_live_inlined() {
        // if shell fish   -> dead (bash target)
        //   elif shell zsh  -> dead
        //   elif shell bash -> alive -> inline body
        //   elif shell pwsh -> never reached
        //   else            -> never reached
        let nodes = vec![Node::If(IfNode {
            cond: Cond::Shell("fish".into()),
            body: vec![set("FISH")],
            elifs: vec![
                (Cond::Shell("zsh".into()), vec![set("ZSH")]),
                (Cond::Shell("bash".into()), vec![set("BASH")]),
                (Cond::Shell("pwsh".into()), vec![set("PWSH")]),
            ],
            else_: vec![set("OTHER")],
        })];
        let out = prune_nodes(nodes, "bash");
        assert_eq!(out.len(), 1, "expected exactly one inlined node: {:?}", out);
        assert!(
            matches!(&out[0], Node::Set { key, .. } if key == "BASH"),
            "expected Set(BASH), got {:?}",
            out[0]
        );
    }

    /// All branches dead, no else -> empty; else present -> else inlined.
    #[test]
    fn all_branches_dead_empty_or_else() {
        let no_else = vec![Node::If(IfNode {
            cond: Cond::Shell("fish".into()),
            body: vec![set("FISH")],
            elifs: vec![(Cond::Shell("zsh".into()), vec![set("ZSH")])],
            else_: vec![],
        })];
        assert!(prune_nodes(no_else, "bash").is_empty(), "expected empty");

        let with_else = vec![Node::If(IfNode {
            cond: Cond::Shell("fish".into()),
            body: vec![set("FISH")],
            elifs: vec![(Cond::Shell("zsh".into()), vec![set("ZSH")])],
            else_: vec![set("ELSE")],
        })];
        let out = prune_nodes(with_else, "bash");
        assert_eq!(out.len(), 1);
        assert!(
            matches!(&out[0], Node::Set { key, .. } if key == "ELSE"),
            "expected Set(ELSE), got {:?}",
            out[0]
        );
    }

    // -- first unknown elif promoted to new head ------------------------------

    /// Dead head, dead elif, first unknown elif promoted; remaining elif + else preserved.
    #[test]
    fn unknown_elif_promoted_to_new_head_with_tail() {
        // if shell fish   -> dead
        //   elif shell zsh  -> dead
        //   elif have cargo -> unknown -> new head
        //   elif os linux   -> kept as elif on rebuilt node
        //   else ELSE       -> kept on rebuilt node
        let nodes = vec![Node::If(IfNode {
            cond: Cond::Shell("fish".into()),
            body: vec![set("FISH")],
            elifs: vec![
                (Cond::Shell("zsh".into()), vec![set("ZSH")]),
                (Cond::Have("cargo".into()), vec![set("CARGO")]),
                (Cond::Os("linux".into()), vec![set("LINUX")]),
            ],
            else_: vec![set("ELSE")],
        })];
        let out = prune_nodes(nodes, "bash");
        assert_eq!(out.len(), 1, "expected one rebuilt if-node: {:?}", out);
        let Node::If(rebuilt) = &out[0] else {
            panic!("expected If, got {:?}", out[0])
        };
        assert!(
            matches!(&rebuilt.cond, Cond::Have(cmd) if cmd == "cargo"),
            "new head should be Have(cargo), got {:?}",
            rebuilt.cond
        );
        assert_eq!(rebuilt.elifs.len(), 1, "one elif (os linux) should remain");
        assert_eq!(rebuilt.else_.len(), 1, "else should be preserved");
    }

    // -- compound condition folding + recursive pruning -----------------------

    /// Outer unknown guard recurses into body: dead inner shell block dropped,
    /// live sibling preserved. Verifies that pruning reaches nested nodes.
    #[test]
    fn dead_inner_block_pruned_inside_unknown_outer() {
        let outer = Node::If(IfNode {
            cond: Cond::Have("git".into()),
            body: vec![
                Node::If(bare_if(Cond::Shell("fish".into()), vec![set("DEAD")])),
                set("LIVE"),
            ],
            elifs: vec![],
            else_: vec![],
        });
        let out = prune_nodes(vec![outer], "bash");
        assert_eq!(out.len(), 1, "outer if kept");
        let Node::If(outer_node) = &out[0] else {
            panic!("expected outer If")
        };
        assert_eq!(
            outer_node.body.len(),
            1,
            "dead inner if dropped, only set remains"
        );
        assert!(
            matches!(&outer_node.body[0], Node::Set { key, .. } if key == "LIVE"),
            "expected Set(LIVE), got {:?}",
            outer_node.body[0]
        );
    }

    /// Four And/Or combinations with one shell operand: verifies all
    /// short-circuit and identity folding rules in one pass.
    #[test]
    fn compound_and_or_folding() {
        // bash AND cargo -> Have(cargo) kept
        let out = prune_nodes(
            vec![Node::If(bare_if(
                Cond::And(
                    Box::new(Cond::Shell("bash".into())),
                    Box::new(Cond::Have("cargo".into())),
                ),
                vec![set("A")],
            ))],
            "bash",
        );
        assert!(
            matches!(&out[..], [Node::If(n)] if matches!(&n.cond, Cond::Have(c) if c == "cargo")),
            "bash AND cargo should reduce to Have(cargo): {:?}",
            out
        );

        // fish AND cargo -> dropped
        let out = prune_nodes(
            vec![Node::If(bare_if(
                Cond::And(
                    Box::new(Cond::Shell("fish".into())),
                    Box::new(Cond::Have("cargo".into())),
                ),
                vec![set("B")],
            ))],
            "bash",
        );
        assert!(
            out.is_empty(),
            "fish AND cargo for bash should drop: {:?}",
            out
        );

        // fish OR cargo -> Have(cargo) kept
        let out = prune_nodes(
            vec![Node::If(bare_if(
                Cond::Or(
                    Box::new(Cond::Shell("fish".into())),
                    Box::new(Cond::Have("cargo".into())),
                ),
                vec![set("C")],
            ))],
            "bash",
        );
        assert!(
            matches!(&out[..], [Node::If(n)] if matches!(&n.cond, Cond::Have(c) if c == "cargo")),
            "fish OR cargo for bash should reduce to Have(cargo): {:?}",
            out
        );

        // bash OR cargo -> body inlined
        let out = prune_nodes(
            vec![Node::If(bare_if(
                Cond::Or(
                    Box::new(Cond::Shell("bash".into())),
                    Box::new(Cond::Have("cargo".into())),
                ),
                vec![set("D")],
            ))],
            "bash",
        );
        assert!(
            matches!(&out[..], [Node::Set { key, .. }] if key == "D"),
            "bash OR cargo for bash should inline body: {:?}",
            out
        );
    }

    /// `not` folds correctly: not(self-shell)=false, not(other-shell)=true,
    /// not(runtime-cond) stays as Not(Have).
    #[test]
    fn not_folding_self_other_and_runtime() {
        // not shell bash for bash -> false -> dropped
        let dead = prune_nodes(
            vec![Node::If(bare_if(
                Cond::Not(Box::new(Cond::Shell("bash".into()))),
                vec![set("DEAD")],
            ))],
            "bash",
        );
        assert!(dead.is_empty(), "not(self-shell) should drop: {:?}", dead);

        // not shell fish for bash -> true -> inline
        let live = prune_nodes(
            vec![Node::If(bare_if(
                Cond::Not(Box::new(Cond::Shell("fish".into()))),
                vec![set("LIVE")],
            ))],
            "bash",
        );
        assert_eq!(live.len(), 1);
        assert!(
            matches!(&live[0], Node::Set { .. }),
            "not(other-shell) should inline body"
        );

        // not have cargo -> Not(Have) stays unknown
        let unknown = prune_nodes(
            vec![Node::If(bare_if(
                Cond::Not(Box::new(Cond::Have("cargo".into()))),
                vec![set("NO_CARGO")],
            ))],
            "bash",
        );
        assert_eq!(unknown.len(), 1);
        assert!(
            matches!(&unknown[0], Node::If(n) if matches!(&n.cond, Cond::Not(_))),
            "not(have) should remain as Not(Have): {:?}",
            unknown
        );
    }

    // -- Os static folding via #[cfg(target_os)] ---------------------------------

    /// On a Linux compilation target, `os linux` folds to AlwaysTrue (body inlined),
    /// `os darwin` and `os windows` fold to AlwaysFalse (dropped).
    /// This test is compiled on all platforms but asserts the correct result
    /// for the current compile target only.
    #[test]
    fn os_static_fold_linux_target() {
        // os linux -> should be AlwaysTrue on linux, AlwaysFalse on others
        let nodes_linux = vec![Node::If(bare_if(
            Cond::Os("linux".into()),
            vec![set("LINUX")],
        ))];
        let out_linux = prune_nodes(nodes_linux, "bash");

        // os darwin -> should be AlwaysTrue on macos, AlwaysFalse on others
        let nodes_darwin = vec![Node::If(bare_if(
            Cond::Os("darwin".into()),
            vec![set("DARWIN")],
        ))];
        let out_darwin = prune_nodes(nodes_darwin, "bash");

        // os windows -> should be AlwaysTrue on windows, AlwaysFalse on others
        let nodes_win = vec![Node::If(bare_if(
            Cond::Os("windows".into()),
            vec![set("WIN")],
        ))];
        let out_win = prune_nodes(nodes_win, "bash");

        #[cfg(target_os = "linux")]
        {
            assert_eq!(
                out_linux.len(),
                1,
                "os linux should inline on linux: {:?}",
                out_linux
            );
            assert!(
                out_darwin.is_empty(),
                "os darwin should be dropped on linux: {:?}",
                out_darwin
            );
            assert!(
                out_win.is_empty(),
                "os windows should be dropped on linux: {:?}",
                out_win
            );
        }
        #[cfg(target_os = "macos")]
        {
            assert!(
                out_linux.is_empty(),
                "os linux should be dropped on macos: {:?}",
                out_linux
            );
            assert_eq!(
                out_darwin.len(),
                1,
                "os darwin should inline on macos: {:?}",
                out_darwin
            );
            assert!(
                out_win.is_empty(),
                "os windows should be dropped on macos: {:?}",
                out_win
            );
        }
        #[cfg(target_os = "windows")]
        {
            assert!(
                out_linux.is_empty(),
                "os linux should be dropped on windows: {:?}",
                out_linux
            );
            assert!(
                out_darwin.is_empty(),
                "os darwin should be dropped on windows: {:?}",
                out_darwin
            );
            assert_eq!(
                out_win.len(),
                1,
                "os windows should inline on windows: {:?}",
                out_win
            );
        }
    }

    #[test]
    fn explicit_target_os_overrides_compilation_target() {
        let linux = vec![Node::If(bare_if(
            Cond::Os("linux".into()),
            vec![set("LINUX")],
        ))];
        let windows = vec![Node::If(bare_if(
            Cond::Os("windows".into()),
            vec![set("WINDOWS")],
        ))];

        let linux_out = prune_nodes_for_target(linux, "bash", Some("linux"));
        let windows_out = prune_nodes_for_target(windows, "bash", Some("linux"));
        assert!(matches!(&linux_out[..], [Node::Set { key, .. }] if key == "LINUX"));
        assert!(windows_out.is_empty());
    }

    /// Unknown OS names stay as Unknown (runtime check preserved).
    #[test]
    fn os_unknown_name_stays_unknown() {
        let out = prune_nodes(
            vec![Node::If(bare_if(
                Cond::Os("haiku".into()),
                vec![set("HAIKU")],
            ))],
            "bash",
        );
        assert_eq!(out.len(), 1, "unknown os should stay as If node: {:?}", out);
        assert!(
            matches!(&out[0], Node::If(n) if matches!(&n.cond, Cond::Os(_))),
            "should remain as Os cond: {:?}",
            out
        );
    }

    /// `exists` is always runtime-unknown — never folded.
    #[test]
    fn exists_is_never_folded() {
        let out = prune_nodes(
            vec![Node::If(bare_if(
                Cond::Exists("/home/user/.cargo/bin".into()),
                vec![set("CARGO_PATH")],
            ))],
            "bash",
        );
        assert_eq!(out.len(), 1, "exists should stay as If node: {:?}", out);
        assert!(
            matches!(&out[0], Node::If(n) if matches!(&n.cond, Cond::Exists(_))),
            "should remain as Exists cond: {:?}",
            out
        );
    }
}
