# Padagonia output-coverage audit

Date: 2026-08-28  
Plan: `CLI_OUTPUT_EXPERIENCE_PLAN.md`  
Padagonia CLI observed: 0.1.60  
Padagonia library observed: 0.2.2

## Result

PASS for the executable ontology and presentation-policy coverage gates.

The checked-in `tests/output_ontology.rs` audit extracts Brandi's live command tree with `clap::CommandFactory`, loads it into a Padagonia store with deterministic provenance, saves and reloads the binary graph, and traverses the graph from its output-experience root. It also constructs deterministic pairwise rows for all presentation dimensions and high-risk three-way rows. `src/output/mod.rs` exhaustively resolves all 5,184 finite presentation-policy combinations.

| Measure | Result |
|---|---:|
| Unique executable leaf commands | 38 |
| Command-parameter occurrences | 96 |
| Proposed global output dimensions per leaf | 12 |
| Mandatory coverage contracts per leaf | 8 |
| Finite safety-core cases per leaf | 756 |
| Finite safety-core cases across all leaves | 28,728 |
| Padagonia nodes | 180 |
| Padagonia edges | 1,015 |
| Padagonia facts | 1,195 |
| Node labels | 8 |
| Edge relations | 8 |

The 96 parameter occurrences include positional parameters and repeated command-local parameters. This is intentionally different from the 87 `#[arg(...)]` attributes found by textual counting.

Padagonia's CLI independently loaded the saved store as 180 nodes, 1,015 edges, and 1,195 facts. A breadth-first traversal from node 0 to depth 3 reached all 180 nodes. CLI JSON export also succeeded.

## Baseline invariants exercised

- Leaf command paths were unique.
- Every leaf was linked to all 12 global output dimensions.
- Every leaf was linked to all eight mandatory coverage contracts.
- Every leaf had command-start and command-finish lifecycle relationships.
- Every extracted parameter was linked to its owning leaf and classified into a semantic domain.
- The graph passed Padagonia storage validation and save/load round-trip.
- The root traversal had no unreachable node in the baseline graph.

## Executable coverage

- `tests/output_ontology.rs` fails when a live Clap leaf is absent, duplicated, disconnected, or missing a required dimension/contract.
- The finite safety core is enumerated for every leaf: 756 tuples per leaf and 28,728 tuples overall.
- All pairs across the 12 modeled presentation dimensions are generated and asserted.
- High-risk triples covering format, terminal state, decoration, outcome, fidelity, width, and content class are generated and asserted.
- `src/output/mod.rs` independently exhausts the resolved runtime-policy Cartesian product and verifies machine/static/accessibility invariants.

## Limits of this audit

Literal enumeration of arbitrary paths, strings, IDs, content, terminal widths, and environment values is impossible. Full coverage is therefore defined over explicit semantic equivalence partitions and boundaries. The current executable graph models parameter domains and presentation contracts; future safety-sensitive command parameters should add first-class constraint and exception nodes as their semantics evolve.

Padagonia provides ontology storage, provenance, traversal, and integrity validation. A deterministic constrained covering-array generator is still required for pairwise and 3-way suites. Literal enumeration of arbitrary paths, strings, terminal widths, IDs, and content is impossible; the plan therefore defines full coverage over explicit semantic equivalence partitions and boundaries.

## Reproduction

Run:

```console
cargo test --test output_ontology
```

The test writes its derived Padagonia store to a temporary directory, validates the save/load round trip, and discards it. The binary `.pad` graph is not committed because it is derived; the versioned test generator is reviewed with the source.
