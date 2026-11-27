//! Dependency cycles: which files can reach one another by following imports.

use std::collections::HashMap;

use super::client::Db;

/// The groups of files in `project` that are tangled in dependency cycles.
/// Within a group every file can reach every other by following dependencies,
/// so any two of them lie on a cycle. A file that only leads into a group, or
/// that a group only leads out to, is not in it.
///
/// It reports groups and not the cycles themselves because cycles cannot be
/// listed in bounded time: fifteen files that all import each other hold
/// millions of them.
///
/// Each group has at least two files and is sorted by path, and the groups
/// are sorted, so the answer is the same on every call.
pub fn find_cycles(db: &Db, project: &str) -> Result<Vec<Vec<String>>, String> {
    let edges: Vec<(String, String)> = db.read("Failed to read the import graph", |conn| {
        // Only dependencies: a Rust `mod child;` and the child's `use super::…`
        // are not a cycle.
        conn.prepare(
            "SELECT source_path, target_path FROM imports
             WHERE project = ?1 AND is_dependency",
        )?
        .query_map([project], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect()
    })?;

    Ok(mutually_reachable_groups(&edges))
}

/// The strongly connected components of more than one node, by Kosaraju's
/// two passes: order the nodes by when a depth-first search finishes them,
/// then, latest first, collect what reaches each node against the edges.
fn mutually_reachable_groups(edges: &[(String, String)]) -> Vec<Vec<String>> {
    let mut ids: HashMap<&str, usize> = HashMap::new();
    let mut names: Vec<&str> = Vec::new();
    for (source, target) in edges {
        for name in [source.as_str(), target.as_str()] {
            ids.entry(name).or_insert_with(|| {
                names.push(name);
                names.len() - 1
            });
        }
    }

    let mut forward: Vec<Vec<usize>> = vec![Vec::new(); names.len()];
    let mut backward: Vec<Vec<usize>> = vec![Vec::new(); names.len()];
    for (source, target) in edges {
        let (source, target) = (ids[source.as_str()], ids[target.as_str()]);
        forward[source].push(target);
        backward[target].push(source);
    }

    let mut groups: Vec<Vec<String>> = Vec::new();
    let mut grouped = vec![false; names.len()];
    for &start in finish_order(&forward).iter().rev() {
        if grouped[start] {
            continue;
        }
        grouped[start] = true;
        let mut members = Vec::new();
        let mut pending = vec![start];
        while let Some(node) = pending.pop() {
            members.push(names[node].to_string());
            for &previous in &backward[node] {
                if !grouped[previous] {
                    grouped[previous] = true;
                    pending.push(previous);
                }
            }
        }
        if members.len() > 1 {
            members.sort();
            groups.push(members);
        }
    }
    groups.sort();
    groups
}

/// The nodes in the order a depth-first search finishes with them. It keeps
/// its own stack, because an import chain can be deeper than the call stack.
fn finish_order(graph: &[Vec<usize>]) -> Vec<usize> {
    let mut seen = vec![false; graph.len()];
    let mut order = Vec::with_capacity(graph.len());
    for start in 0..graph.len() {
        if seen[start] {
            continue;
        }
        seen[start] = true;
        // Each entry is a node and how many of its neighbours have been taken.
        let mut path = vec![(start, 0)];
        while let Some((node, taken)) = path.pop() {
            match graph[node].get(taken) {
                Some(&next) => {
                    path.push((node, taken + 1));
                    if !seen[next] {
                        seen[next] = true;
                        path.push((next, 0));
                    }
                }
                None => order.push(node),
            }
        }
    }
    order
}

#[cfg(test)]
#[path = "cycles_tests.rs"]
mod tests;
