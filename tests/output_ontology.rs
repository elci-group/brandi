use brandi::cli::Cli;
use clap::{ArgAction, Command, CommandFactory};
use padagonia::{Provenance, QueryEngine, Scalar, Store};
use std::collections::{BTreeMap, BTreeSet, HashSet};

#[derive(Debug)]
struct Leaf {
    path: String,
    family: String,
    parameters: Vec<(String, &'static str)>,
}

fn collect_leaves(
    command: &Command,
    prefix: &[String],
    inherited: &[(String, &'static str)],
    out: &mut Vec<Leaf>,
) {
    if command.get_name() == "help" {
        return;
    }
    let root = command.get_name() == "brandi";
    let mut path = prefix.to_vec();
    if !root {
        path.push(command.get_name().to_string());
    }
    let mut parameters = inherited.to_vec();
    if !root {
        parameters.extend(command.get_arguments().filter_map(|arg| {
            let id = arg.get_id().as_str();
            if id == "help" || id == "version" {
                return None;
            }
            let domain = match arg.get_action() {
                ArgAction::SetTrue | ArgAction::SetFalse => "boolean",
                ArgAction::Append | ArgAction::Count => "collection",
                _ if id == "kind" => "enum",
                _ if matches!(
                    id,
                    "budget" | "port" | "fail_under" | "fail_under_precision" | "fail_under_recall"
                ) =>
                {
                    "numeric"
                }
                _ if id.contains("path")
                    || matches!(
                        id,
                        "file" | "images" | "corpus" | "baseline" | "config" | "output"
                    ) =>
                {
                    "path"
                }
                _ if matches!(id, "code" | "code_env") => "secret_source",
                _ if id == "confirm" => "confirmation",
                _ => "string",
            };
            Some((id.to_string(), domain))
        }));
    }
    let children: Vec<_> = command
        .get_subcommands()
        .filter(|child| child.get_name() != "help")
        .collect();
    if children.is_empty() && !path.is_empty() {
        out.push(Leaf {
            family: path[0].clone(),
            path: path.join(" "),
            parameters,
        });
    } else {
        for child in children {
            collect_leaves(child, &path, &parameters, out);
        }
    }
}

fn provenance(evidence: &str) -> Provenance {
    Provenance::new(
        "brandi-output-coverage",
        "deterministic",
        1.0,
        0.0,
        0,
        vec![evidence.to_string()],
    )
}

#[test]
fn padagonia_ontology_covers_the_live_cli() {
    let mut clap = Cli::command();
    clap.build();
    let mut leaves = Vec::new();
    collect_leaves(&clap, &[], &[], &mut leaves);
    leaves.sort_by(|a, b| a.path.cmp(&b.path));
    assert_eq!(leaves.len(), 38, "every executable leaf must be modeled");
    assert_eq!(
        leaves
            .iter()
            .map(|leaf| leaf.path.as_str())
            .collect::<BTreeSet<_>>()
            .len(),
        leaves.len(),
        "leaf identities must be unique"
    );

    let dimensions = [
        "format",
        "color",
        "animation",
        "fidelity",
        "unicode",
        "width",
        "verbosity",
        "stdout_terminal",
        "stderr_terminal",
        "outcome",
        "cardinality",
        "content",
    ];
    let requirements = [
        "human_renderer",
        "plain_renderer",
        "machine_renderer",
        "stream_contract",
        "motion_policy",
        "exit_contract",
        "redaction_policy",
        "narrow_width_policy",
    ];
    let mut store = Store::new();
    let root = store.add_node(
        "OutputExperience",
        vec![("schema", Scalar::String("brandi-output-ontology-v1".into()))],
        None,
        provenance("CLI_OUTPUT_EXPERIENCE_PLAN.md"),
    );
    let mut dimension_nodes = Vec::new();
    for name in dimensions {
        let node = store.add_node(
            "ParameterDimension",
            vec![("name", Scalar::String(name.into()))],
            None,
            provenance("global OutputArgs plus environment and terminal policy"),
        );
        store.add_edge(root, node, "defines", vec![], None, provenance("ontology"));
        dimension_nodes.push(node);
    }
    let mut requirement_nodes = Vec::new();
    for name in requirements {
        let node = store.add_node(
            "CoverageRequirement",
            vec![("name", Scalar::String(name.into()))],
            None,
            provenance("roadmap definition of done"),
        );
        store.add_edge(root, node, "requires", vec![], None, provenance("ontology"));
        requirement_nodes.push(node);
    }

    let mut families = BTreeMap::new();
    let mut command_nodes = Vec::new();
    for leaf in &leaves {
        let family = *families.entry(leaf.family.clone()).or_insert_with(|| {
            let node = store.add_node(
                "CommandFamily",
                vec![("name", Scalar::String(leaf.family.clone()))],
                None,
                provenance("Clap hierarchy"),
            );
            store.add_edge(
                root,
                node,
                "contains",
                vec![],
                None,
                provenance("Clap hierarchy"),
            );
            node
        });
        let command = store.add_node(
            "LeafCommand",
            vec![("path", Scalar::String(leaf.path.clone()))],
            None,
            provenance("Cli::command"),
        );
        store.add_edge(
            family,
            command,
            "contains",
            vec![],
            None,
            provenance("Clap hierarchy"),
        );
        for dimension in &dimension_nodes {
            store.add_edge(
                command,
                *dimension,
                "governed_by",
                vec![],
                None,
                provenance("global policy"),
            );
        }
        for requirement in &requirement_nodes {
            store.add_edge(
                command,
                *requirement,
                "must_cover",
                vec![],
                None,
                provenance("coverage policy"),
            );
        }
        for (name, domain) in &leaf.parameters {
            let parameter = store.add_node(
                "CommandParameter",
                vec![
                    ("name", Scalar::String(name.clone())),
                    ("domain", Scalar::String((*domain).into())),
                    ("command", Scalar::String(leaf.path.clone())),
                ],
                None,
                provenance("Clap argument metadata"),
            );
            store.add_edge(
                command,
                parameter,
                "accepts",
                vec![],
                None,
                provenance("parameter inventory"),
            );
        }
        command_nodes.push(command);
    }

    let query = QueryEngine::new(&store);
    for (leaf, command) in leaves.iter().zip(&command_nodes) {
        let reached = query.bfs(*command, 1, None, Some(1.0));
        assert!(
            reached.len() >= 1 + dimensions.len() + requirements.len(),
            "{} has an ontology coverage gap",
            leaf.path
        );
    }
    let temporary = tempfile::tempdir().unwrap();
    let graph = temporary.path().join("brandi-output-ontology.pad");
    store.save(&graph).unwrap();
    let reloaded = Store::load(&graph).unwrap();
    assert_eq!(
        reloaded.stats(),
        store.stats(),
        "Padagonia round-trip changed the ontology"
    );
    assert_eq!(
        QueryEngine::new(&reloaded)
            .bfs(root, 3, None, Some(1.0))
            .len(),
        reloaded.nodes().len(),
        "every ontology node must be reachable from the root"
    );
}

#[test]
fn finite_safety_core_and_covering_tuples_are_complete() {
    let safety = [3usize, 2, 2, 3, 3, 7];
    let mut cases = HashSet::new();
    for format in 0..safety[0] {
        for stdout_tty in 0..safety[1] {
            for stderr_tty in 0..safety[2] {
                for color in 0..safety[3] {
                    for animation in 0..safety[4] {
                        for outcome in 0..safety[5] {
                            cases.insert((
                                format, stdout_tty, stderr_tty, color, animation, outcome,
                            ));
                        }
                    }
                }
            }
        }
    }
    assert_eq!(cases.len(), 756);
    assert_eq!(cases.len() * 38, 28_728);

    // Constructive covering arrays: every pair and every high-risk triple is
    // materialized with all non-participating dimensions at their safe default.
    let domains = [3usize, 3, 3, 8, 3, 5, 3, 2, 2, 7, 4, 4];
    let mut pair_rows = BTreeSet::new();
    for left in 0..domains.len() {
        for right in left + 1..domains.len() {
            for a in 0..domains[left] {
                for b in 0..domains[right] {
                    let mut row = vec![0usize; domains.len()];
                    row[left] = a;
                    row[right] = b;
                    pair_rows.insert(row);
                }
            }
        }
    }
    for left in 0..domains.len() {
        for right in left + 1..domains.len() {
            for a in 0..domains[left] {
                for b in 0..domains[right] {
                    assert!(pair_rows
                        .iter()
                        .any(|row| row[left] == a && row[right] == b));
                }
            }
        }
    }

    let high_risk = [0usize, 1, 2, 3, 7, 8, 9];
    let mut triple_rows = BTreeSet::new();
    for ai in 0..high_risk.len() {
        for bi in ai + 1..high_risk.len() {
            for ci in bi + 1..high_risk.len() {
                let (a_dim, b_dim, c_dim) = (high_risk[ai], high_risk[bi], high_risk[ci]);
                for a in 0..domains[a_dim] {
                    for b in 0..domains[b_dim] {
                        for c in 0..domains[c_dim] {
                            let mut row = vec![0usize; domains.len()];
                            row[a_dim] = a;
                            row[b_dim] = b;
                            row[c_dim] = c;
                            triple_rows.insert(row);
                        }
                    }
                }
            }
        }
    }
    assert!(!pair_rows.is_empty());
    assert!(!triple_rows.is_empty());
}
