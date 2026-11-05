//! Target adapters consume the same specialized semantic tree.
use crate::ast::*;
mod powershell;
mod unix;

pub fn generate(nodes: Vec<Node>, target: Target) -> Result<String, String> {
    if target.shell == Shell::Powershell {
        powershell::validate(&nodes)?;
    } else {
        unix::validate(&nodes, target.shell)?;
    }
    let nodes = crate::prune::specialize(nodes, target);
    Ok(match target.shell {
        Shell::Powershell => powershell::generate(&nodes, target),
        _ => unix::generate(&nodes, target),
    })
}
