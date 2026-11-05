//! Target adapters consume the same specialized semantic tree.
use crate::ast::*;
mod powershell;
mod unix;

pub fn generate(nodes: Vec<Node>, target: Target) -> Result<String, String> {
    validate(&nodes, target.shell)?;
    let nodes = crate::prune::specialize(nodes, target);
    Ok(match target.shell {
        Shell::Powershell => powershell::generate(&nodes, target),
        _ => unix::generate(&nodes, target),
    })
}

fn validate(nodes: &[Node], shell: Shell) -> Result<(), String> {
    for node in nodes {
        match &node.kind {
            Statement::Alias { name, .. } => {
                let reserved = match shell {
                    Shell::Powershell => powershell::reserved_alias(name),
                    _ => unix::reserved_alias(name, shell),
                };
                if reserved {
                    let backend = if shell == Shell::Powershell {
                        "PowerShell"
                    } else {
                        shell.name()
                    };
                    return Err(format!(
                        "line {}, column {}: reserved {backend} alias name {name:?}",
                        node.at.line, node.at.column
                    ));
                }
            }
            Statement::If(branch) => {
                for (_, body) in &branch.branches {
                    validate(body, shell)?;
                }
                validate(&branch.otherwise, shell)?;
            }
            _ => {}
        }
    }
    Ok(())
}
