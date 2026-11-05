//! Target specialization that preserves runtime evaluation order and failures.
use crate::ast::{Cond, IfNode, Node, Statement, Target};

pub fn specialize(nodes: Vec<Node>, target: Target) -> Vec<Node> {
    let mut output = Vec::with_capacity(nodes.len());
    for Node { at, kind } in nodes {
        match kind {
            Statement::If(branch) => {
                let branch = branches(branch, target);
                if branch.branches.is_empty() {
                    output.extend(branch.otherwise);
                } else {
                    output.push(Node {
                        at,
                        kind: Statement::If(branch),
                    });
                }
            }
            Statement::Raw { shell, body } => {
                if shell == target.shell {
                    output.push(Node {
                        at,
                        kind: Statement::Raw { shell, body },
                    });
                }
            }
            kind @ (Statement::Set { .. }
            | Statement::Unset(_)
            | Statement::Path { .. }
            | Statement::Alias { .. }
            | Statement::Init(_)) => {
                output.push(Node { at, kind });
            }
        }
    }
    output
}

fn branches(branch: IfNode, target: Target) -> IfNode {
    let mut kept = Vec::with_capacity(branch.branches.len());
    let mut otherwise = branch.otherwise;
    for (cond, body) in branch.branches {
        match condition(cond, target) {
            Cond::Constant(false) => {}
            Cond::Constant(true) => {
                otherwise = body;
                break;
            }
            cond => kept.push((cond, specialize(body, target))),
        }
    }
    IfNode {
        branches: kept,
        otherwise: specialize(otherwise, target),
    }
}

fn condition(cond: Cond, target: Target) -> Cond {
    match cond {
        Cond::Shell(shell) => Cond::Constant(shell == target.shell),
        Cond::Os(os) => Cond::Constant(os == target.os),
        Cond::Not(mut inner) => {
            *inner = condition(*inner, target);
            match *inner {
                Cond::Constant(value) => Cond::Constant(!value),
                Cond::Not(inner) => *inner,
                _ => Cond::Not(inner),
            }
        }
        Cond::And(mut left, mut right) => {
            *left = condition(*left, target);
            match *left {
                Cond::Constant(false) => Cond::Constant(false),
                Cond::Constant(true) => condition(*right, target),
                _ => {
                    *right = condition(*right, target);
                    // A residual left operand must still run, even if right is false.
                    if matches!(*right, Cond::Constant(true)) {
                        *left
                    } else {
                        Cond::And(left, right)
                    }
                }
            }
        }
        Cond::Or(mut left, mut right) => {
            *left = condition(*left, target);
            match *left {
                Cond::Constant(true) => Cond::Constant(true),
                Cond::Constant(false) => condition(*right, target),
                _ => {
                    *right = condition(*right, target);
                    if matches!(*right, Cond::Constant(false)) {
                        *left
                    } else {
                        Cond::Or(left, right)
                    }
                }
            }
        }
        cond @ (Cond::Constant(_) | Cond::Have(_) | Cond::Exists(_) | Cond::Env(_)) => cond,
    }
}

#[cfg(test)]
mod tests;
