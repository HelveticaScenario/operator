//! Graph cycle detection via [`petgraph::algo::tarjan_scc`].
//!
//! Returns `ProcessingMode` per module ID. Modules in a strongly-connected
//! component with >1 member, or a single node with a self-loop, are assigned
//! `Sample` mode. All others get `Block` mode.
//!
//! Cable extraction lives elsewhere — callers walk deserialized params via
//! the [`modular_core::types::CollectCables`] trait and pass the resulting
//! adjacency map here. Keeping graph_analysis ignorant of params shape means
//! the cable schema lives in exactly one place: each type's own `CollectCables`
//! impl.

#![allow(dead_code)]

use modular_core::types::ProcessingMode;
use petgraph::algo::tarjan_scc;
use petgraph::graphmap::DiGraphMap;
use std::collections::HashMap;

/// Classify each module as `Block` or `Sample` based on adjacency.
///
/// `adjacency[consumer]` is the list of producer module IDs `consumer` reads
/// from (one entry per cable; duplicates are tolerated). Producer IDs that
/// are not also keys in `adjacency` are inserted as orphan `Block` nodes.
pub fn classify_modules(
    adjacency: &HashMap<String, Vec<String>>,
) -> HashMap<String, ProcessingMode> {
    let mut g: DiGraphMap<&str, ()> = DiGraphMap::new();

    for (consumer, producers) in adjacency {
        g.add_node(consumer.as_str());
        for producer in producers {
            // `add_edge` inserts both endpoints if missing; safe even when the
            // producer isn't a key in `adjacency` (e.g. a dangling cable).
            g.add_edge(consumer.as_str(), producer.as_str(), ());
        }
    }

    let mut result = HashMap::new();
    for scc in tarjan_scc(&g) {
        // A 1-node SCC is cyclic iff the node has an edge to itself.
        let cyclic = scc.len() > 1 || (scc.len() == 1 && g.contains_edge(scc[0], scc[0]));
        let mode = if cyclic {
            ProcessingMode::Sample
        } else {
            ProcessingMode::Block
        };
        for id in scc {
            result.insert(id.to_string(), mode);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use modular_core::types::ProcessingMode;

    fn adj(edges: &[(&str, &[&str])]) -> HashMap<String, Vec<String>> {
        edges
            .iter()
            .map(|(consumer, producers)| {
                (
                    consumer.to_string(),
                    producers.iter().map(|p| p.to_string()).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn no_cycle_is_block_mode() {
        // A -> B -> C (consumer C reads B; consumer B reads A)
        let modes = classify_modules(&adj(&[
            ("A", &[]),
            ("B", &["A"]),
            ("C", &["B"]),
        ]));
        assert_eq!(modes["A"], ProcessingMode::Block);
        assert_eq!(modes["B"], ProcessingMode::Block);
        assert_eq!(modes["C"], ProcessingMode::Block);
    }

    #[test]
    fn two_node_cycle_is_sample_mode() {
        // A <-> B
        let modes = classify_modules(&adj(&[("A", &["B"]), ("B", &["A"])]));
        assert_eq!(modes["A"], ProcessingMode::Sample);
        assert_eq!(modes["B"], ProcessingMode::Sample);
    }

    #[test]
    fn self_loop_is_sample_mode() {
        let modes = classify_modules(&adj(&[("A", &["A"])]));
        assert_eq!(modes["A"], ProcessingMode::Sample);
    }

    #[test]
    fn three_node_cycle_is_sample_mode() {
        // A → B → C → A (no shorter back-edges)
        let modes = classify_modules(&adj(&[
            ("A", &["C"]),
            ("B", &["A"]),
            ("C", &["B"]),
        ]));
        assert_eq!(modes["A"], ProcessingMode::Sample);
        assert_eq!(modes["B"], ProcessingMode::Sample);
        assert_eq!(modes["C"], ProcessingMode::Sample);
    }

    #[test]
    fn cycle_plus_independent_node() {
        let modes = classify_modules(&adj(&[
            ("A", &["B"]),
            ("B", &["A"]),
            ("C", &[]),
        ]));
        assert_eq!(modes["A"], ProcessingMode::Sample);
        assert_eq!(modes["B"], ProcessingMode::Sample);
        assert_eq!(modes["C"], ProcessingMode::Block);
    }

    #[test]
    fn dangling_producer_is_inserted_as_block() {
        // Consumer references a module ID that isn't a key — still treated as
        // a node so it gets a mode entry.
        let modes = classify_modules(&adj(&[("A", &["MISSING"])]));
        assert_eq!(modes["A"], ProcessingMode::Block);
        assert_eq!(modes["MISSING"], ProcessingMode::Block);
    }

    #[test]
    fn duplicate_producer_edges_collapse() {
        // Two cables from B reading A — adjacency contains "A" twice. Should
        // not affect classification.
        let modes = classify_modules(&adj(&[("B", &["A", "A"])]));
        assert_eq!(modes["A"], ProcessingMode::Block);
        assert_eq!(modes["B"], ProcessingMode::Block);
    }
}
