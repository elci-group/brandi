//! Cited marketing knowledge compiled into a Padagonia graph.
//!
//! The corpus is a bounded public sample for Jack Appleby (@jappleby). It is
//! not a complete archive of the account. Planning checks below are
//! deterministic projections of that sample.

use crate::error::{BrandiError, Result};
use padagonia::{NamespaceId, Provenance, Scalar, Store, StringTableExt};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

pub const SCHEMA_VERSION: &str = "brandi-marketing-knowledge-v1";
pub const CORPUS_BOUNDARY: &str = "This graph is not a complete archive of @jappleby (X user id 19904963, account created 2009-02-02). It is a cited public sample retrieved 2026-10-06 covering posts and essays from 2022-06 through 2026-10, plus Motion's secondary synthesis of a December 2025 talk.";

const PROFILE: &str = "https://x.com/jappleby";
const LAUNCH: &str = "https://x.com/jappleby/status/1534663345936207872";
const INDEPENDENT: &str = "https://x.com/jappleby/status/1633907126845464576";
const SUBS: &str = "https://x.com/jappleby/status/2075203775011119571";
const HOOP: &str = "https://x.com/jappleby/status/1964012862566703194";
const TEACHABLE: &str = "https://jackappleby.teachable.com/p/personalsocialstrategy";
const FOUR: &str = "https://futuresocial.beehiiv.com/p/full-social-strategy-4-easy-steps";
const WHY: &str = "https://x.com/jappleby/status/2046592318623682615";
const HQ: &str = "https://x.com/jappleby/status/2055284252292354516";
const SPOTLIGHT: &str = "https://www.creatorspotlight.com/p/future-social";
const PITCH: &str = "https://x.com/jappleby/status/1511734519493234698";
const INTERRUPT: &str = "https://x.com/jappleby/status/2095490895705444370";
const AUTHENTIC: &str = "https://x.com/jappleby/status/2095883547768725572";
const PILLARS: &str = "https://futuresocial.beehiiv.com/p/let-s-fix-social-media-content-pillars";
const HOOKS: &str = "https://www.superside.com/blog/social-media-hooks";
const BEST_2025: &str = "https://futuresocial.beehiiv.com/p/best-of-future-social-2025-edition";
const AGORA: &str = "https://www.agorapulse.com/blog/podcasts/social-pulse-podcast/from-no-budget-to-yes-please-how-to-sell-social-strategy-internally/";
const MOTION: &str = "https://motionapp.com/library/expert/jack-appleby/";
const STRATEGY_INDEX: &str = "https://futuresocial.beehiiv.com/t/social-strategy";
const AUTHOR_INDEX: &str = "https://futuresocial.beehiiv.com/authors/jack-appleby";

struct Claim {
    id: &'static str,
    label: &'static str,
    tier: &'static str,
    confidence: f32,
    statement: &'static str,
    evidence: &'static [&'static str],
}

/// A deterministic check applied while planning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanConstraint {
    pub principle_id: &'static str,
    pub scope: &'static str,
    pub check: &'static str,
}

const PLANNING: &[PlanConstraint] = &[
    PlanConstraint {
        principle_id: "four-question-strategy",
        scope: "frame",
        check: "[four-question-strategy] Name a specific goal, the audience, the resources on hand, and the measure of success.",
    },
    PlanConstraint {
        principle_id: "idea-selection",
        scope: "item",
        check: "[idea-selection] Name the idea before the format.",
    },
    PlanConstraint {
        principle_id: "youtube-title-gut-check",
        scope: "item",
        check: "[youtube-title-gut-check] State the concept in five to seven words.",
    },
    PlanConstraint {
        principle_id: "hook-is-a-highlight",
        scope: "item",
        check: "[hook-is-a-highlight] Open on a highlight, with the story before the product.",
    },
    PlanConstraint {
        principle_id: "stance",
        scope: "item",
        check: "[stance] Say why this audience would follow, without requiring a spicy take.",
    },
    PlanConstraint {
        principle_id: "full-idea-on-surface",
        scope: "item",
        check: "[full-idea-on-surface] Put the full idea on this surface instead of a teaser link.",
    },
    PlanConstraint {
        principle_id: "tried-and-true-mix",
        scope: "item",
        check: "[tried-and-true-mix] Mark the item tried-and-true or trying-new. His pitch script keeps experiments inside about 20-40%.",
    },
    PlanConstraint {
        principle_id: "explain-the-why",
        scope: "item",
        check: "[explain-the-why] Stay true to the product, and reject both unrelated virality and branded content with no view path.",
    },
    PlanConstraint {
        principle_id: "brands-are-interruptions",
        scope: "item",
        check: "[brands-are-interruptions] Assume the audience did not open the feed for this brand.",
    },
    PlanConstraint {
        principle_id: "actionable-takeaway",
        scope: "item",
        check: "[actionable-takeaway] Leave a takeaway the reader could save or share.",
    },
    PlanConstraint {
        principle_id: "authenticity",
        scope: "item",
        check: "[authenticity] Do not perform honesty the evidence does not support.",
    },
    PlanConstraint {
        principle_id: "separate-strategy-and-creative",
        scope: "item",
        check: "[separate-strategy-and-creative] Keep strategy agreement separate from the creative execution.",
    },
];

pub fn planning_constraints() -> &'static [PlanConstraint] {
    PLANNING
}

pub fn strategy_questions() -> &'static [&'static str] {
    &[
        "What are you trying to accomplish?",
        "Who are you trying to reach?",
        "What resources do you have?",
        "How will you measure success?",
    ]
}

fn claims() -> &'static [Claim] {
    &[
        Claim { id: "shared-knowledge", label: "SharedKnowledge", tier: "meta", confidence: 1.0, statement: "Shared marketing knowledge, references, and analysis used by Brandi planning.", evidence: &[PROFILE] },
        Claim { id: "corpus-boundary", label: "Analysis", tier: "meta", confidence: 1.0, statement: CORPUS_BOUNDARY, evidence: &[PROFILE] },
        Claim { id: "content-planning", label: "Analysis", tier: "meta", confidence: 1.0, statement: "Brandi content plans and promotion plans consume these checks deterministically.", evidence: &[FOUR] },
        Claim { id: "jack-appleby", label: "Person", tier: "identity", confidence: 1.0, statement: "Jack Appleby (@jappleby) is a marketer and creator. His public bio lists work at Microsoft, Verizon, Beats By Dre, and Community, and names Future Social and How To Hoop Forever.", evidence: &[PROFILE] },
        Claim { id: "future-social", label: "Work", tier: "identity", confidence: 0.95, statement: "Future Social is his newsletter on social, brand, and creator strategy. He launched it with Morning Brew on 8 June 2022, said it was independent on 9 March 2023, and reported 85,000 subscribers on 9 July 2026. The subscriber count is his claim.", evidence: &[LAUNCH, INDEPENDENT, SUBS] },
        Claim { id: "how-to-hoop-forever", label: "Work", tier: "identity", confidence: 0.9, statement: "How To Hoop Forever is a separate basketball creator business, not the marketing newsletter.", evidence: &[HOOP, TEACHABLE] },
        Claim { id: "four-question-strategy", label: "Method", tier: "primary", confidence: 0.95, statement: "A social strategy answers four questions: what you are trying to accomplish, who you are trying to reach, what resources you have, and how you will measure success. Goals should be SMART, and the audience should be named rather than chased as mass virality.", evidence: &[FOUR] },
        Claim { id: "explain-the-why", label: "Principle", tier: "primary", confidence: 0.95, statement: "Strategy shows the product in ways that are true to it and can still earn views and sales, by making the emotional value clear. Two failures are virality unrelated to the product, and heavily branded content with no realistic view path.", evidence: &[WHY] },
        Claim { id: "idea-selection", label: "Principle", tier: "primary", confidence: 0.95, statement: "Choose and package the idea before optimization, formatting, or best-practice theater.", evidence: &[HQ] },
        Claim { id: "stance", label: "Principle", tier: "primary", confidence: 0.95, statement: "There has to be a reason to follow. A stance is enough, and it does not have to be spicy.", evidence: &[HQ] },
        Claim { id: "full-idea-on-surface", label: "Principle", tier: "primary", confidence: 0.95, statement: "Publish the full idea on each surface. Do not replace the idea with a teaser link. Good ideas travel.", evidence: &[HQ] },
        Claim { id: "actionable-takeaway", label: "Principle", tier: "primary", confidence: 0.9, statement: "Good content teaches something the reader would save or share. He described Future Social as an actionable how-to in a 2023 interview.", evidence: &[SPOTLIGHT] },
        Claim { id: "expertise-before-pitch", label: "Principle", tier: "primary", confidence: 0.95, statement: "Show expertise before asking for the decision. One method he named is a weekly Best In Social email. Later steps of that thread are outside this sample.", evidence: &[PITCH] },
        Claim { id: "brands-are-interruptions", label: "Principle", tier: "primary", confidence: 1.0, statement: "People do not open a feed hoping a brand posted. Brand content is an interruption, so it has to be good.", evidence: &[INTERRUPT] },
        Claim { id: "authenticity", label: "Principle", tier: "primary", confidence: 0.7, statement: "In one reply on 4 September 2026 he said he is focused on honesty in influencer and brand content, and that faux lines take advantage of consumers. This sample does not treat that reply as a broader doctrine.", evidence: &[AUTHENTIC] },
        Claim { id: "tried-and-true-mix", label: "Method", tier: "primary", confidence: 0.95, statement: "His pitch script splits pillars into tried-and-true work (about 60-80%) and experiments (about 20-40%), both still on brand. The split is the script he wrote, not a measured optimum.", evidence: &[PILLARS] },
        Claim { id: "hook-is-a-highlight", label: "Method", tier: "secondary", confidence: 0.75, statement: "Superside's writeup of his September 2023 Standout Summit talk summarizes seven hook themes: a highlight rather than a summary, YouTube-style titles, story before product, simple rather than clever, clickbait only when the piece delivers, many headline options, and studying what stopped you. That list is the article's packaging, not a primary transcript.", evidence: &[HOOKS] },
        Claim { id: "fundamentals-persist", label: "Principle", tier: "primary", confidence: 0.95, statement: "Social networks change. The strategy fundamentals do not.", evidence: &[BEST_2025] },
        Claim { id: "separate-strategy-and-creative", label: "Method", tier: "primary", confidence: 0.9, statement: "Get agreement on the strategy in its own conversation before pitching creative. The source is an edited podcast transcript.", evidence: &[AGORA] },
        Claim { id: "two-best-practices", label: "Principle", tier: "secondary", confidence: 0.75, statement: "Motion's synthesis of the December 2025 talk says performance moves mainly with the idea and the hook. Posting time, hashtag count, and pixel size are peripheral. This is not a transcript.", evidence: &[MOTION] },
        Claim { id: "ideas-before-tactics", label: "Principle", tier: "secondary", confidence: 0.75, statement: "Motion's synthesis says to brainstorm the idea before format tricks.", evidence: &[MOTION] },
        Claim { id: "emotional-benefit", label: "Principle", tier: "secondary", confidence: 0.75, statement: "Motion's synthesis argues for emotional benefit over joke-only content.", evidence: &[MOTION] },
        Claim { id: "attention-not-category", label: "Principle", tier: "secondary", confidence: 0.75, statement: "Motion's synthesis says the feed competes with entertainment, so study creators who win attention rather than only category rivals.", evidence: &[MOTION] },
        Claim { id: "story-over-production", label: "Principle", tier: "secondary", confidence: 0.75, statement: "Motion's synthesis says the story, and whether people can decode the concept, outweigh production polish.", evidence: &[MOTION] },
        Claim { id: "paid-to-have-opinion", label: "Principle", tier: "secondary", confidence: 0.75, statement: "Motion's synthesis says to show up with a prepared point of view.", evidence: &[MOTION] },
        Claim { id: "b-sides", label: "Principle", tier: "secondary", confidence: 0.75, statement: "Motion's synthesis says to produce more ideas than the slots on the calendar.", evidence: &[MOTION] },
        Claim { id: "youtube-title-gut-check", label: "Method", tier: "secondary", confidence: 0.75, statement: "Motion's synthesis says that if a post cannot be said in five to seven words, the concept is muddy.", evidence: &[MOTION] },
        Claim { id: "brainstorm-based-on", label: "Method", tier: "secondary", confidence: 0.75, statement: "Motion's synthesis lists four prompts: a belief you hold, a tension the audience feels, a story only you can tell, and a perspective that makes them think.", evidence: &[MOTION] },
        Claim { id: "republish-is-not-cross-post-ban", label: "Analysis", tier: "analysis", confidence: 0.95, statement: "Putting the full idea on each surface means the idea travels. It is not a rule against using that idea in more than one place.", evidence: &[HQ] },
        Claim { id: "naive-never-republish", label: "Analysis", tier: "analysis", confidence: 0.6, statement: "A naive rule would forbid republication. That rule is not his claim. His May 2026 post describes publishing the same essay on the destination surface.", evidence: &[HQ] },
        Claim { id: "ref-x-profile", label: "Reference", tier: "primary", confidence: 1.0, statement: "Public X profile for @jappleby, retrieved 2026-10-06.", evidence: &[PROFILE] },
        Claim { id: "ref-future-social-launch", label: "Reference", tier: "primary", confidence: 0.95, statement: "Post 1534663345936207872, 8 June 2022: Future Social launches with Morning Brew.", evidence: &[LAUNCH] },
        Claim { id: "ref-future-social-independent", label: "Reference", tier: "primary", confidence: 0.95, statement: "Post 1633907126845464576, 9 March 2023: Future Social becomes independent.", evidence: &[INDEPENDENT] },
        Claim { id: "ref-future-social-85k", label: "Reference", tier: "primary", confidence: 0.95, statement: "Post 2075203775011119571, 9 July 2026: he reports 85,000 Future Social subscribers.", evidence: &[SUBS] },
        Claim { id: "ref-hoop-baseline", label: "Reference", tier: "primary", confidence: 0.9, statement: "Post 1964012862566703194, 5 September 2025: How To Hoop Forever is listed as a separate creator business.", evidence: &[HOOP] },
        Claim { id: "ref-teachable", label: "Reference", tier: "primary", confidence: 0.9, statement: "Course page stating two creator businesses, including How To Hoop Forever.", evidence: &[TEACHABLE] },
        Claim { id: "ref-four-questions", label: "Reference", tier: "primary", confidence: 0.95, statement: "Future Social essay, 5 June 2024: a social strategy is four questions.", evidence: &[FOUR] },
        Claim { id: "ref-explain-why", label: "Reference", tier: "primary", confidence: 0.95, statement: "Post 2046592318623682615, 21 April 2026: what he looks for in a strategist.", evidence: &[WHY] },
        Claim { id: "ref-linkedin-hq", label: "Reference", tier: "primary", confidence: 0.95, statement: "Post 2055284252292354516, 15 May 2026: idea selection, stance, and publishing the full essay on the surface.", evidence: &[HQ] },
        Claim { id: "ref-creator-spotlight", label: "Reference", tier: "primary", confidence: 0.9, statement: "Creator Spotlight interview, 30 June 2023, on actionable Future Social issues.", evidence: &[SPOTLIGHT] },
        Claim { id: "ref-best-in-social", label: "Reference", tier: "primary", confidence: 0.95, statement: "Post 1511734519493234698, 6 April 2022: show expertise before the pitch.", evidence: &[PITCH] },
        Claim { id: "ref-interruption", label: "Reference", tier: "primary", confidence: 1.0, statement: "Post 2095490895705444370, 3 September 2026: brands are an interruption.", evidence: &[INTERRUPT] },
        Claim { id: "ref-authenticity", label: "Reference", tier: "primary", confidence: 0.7, statement: "Reply 2095883547768725572, 4 September 2026. One post, not a thread.", evidence: &[AUTHENTIC] },
        Claim { id: "ref-pillars", label: "Reference", tier: "primary", confidence: 0.95, statement: "Future Social essay, 3 June 2026: Tried & True versus Trying New.", evidence: &[PILLARS] },
        Claim { id: "ref-superside-hooks", label: "Reference", tier: "secondary", confidence: 0.75, statement: "Superside, 18 September 2023: seven hook themes attributed to his Standout Summit talk.", evidence: &[HOOKS] },
        Claim { id: "ref-best-of-2025", label: "Reference", tier: "primary", confidence: 0.95, statement: "Future Social recap, 10 December 2025: networks change and fundamentals do not.", evidence: &[BEST_2025] },
        Claim { id: "ref-agorapulse", label: "Reference", tier: "primary", confidence: 0.9, statement: "Agorapulse Social Pulse transcript: sell the strategy before the creative.", evidence: &[AGORA] },
        Claim { id: "ref-motion", label: "Reference", tier: "secondary", confidence: 0.75, statement: "Motion library synthesis of the December 2025 talk Forget Algorithms. Not a transcript.", evidence: &[MOTION] },
        Claim { id: "ref-slow-cadence", label: "Reference", tier: "title", confidence: 0.6, statement: "Title only, 20 November 2025: Social Media shouldn't move fast. Speed literally doesn't matter.", evidence: &[STRATEGY_INDEX] },
        Claim { id: "ref-trend-detox", label: "Reference", tier: "title", confidence: 0.6, statement: "Title only, 5 September 2024: Why you need a Trend Detox.", evidence: &[STRATEGY_INDEX] },
        Claim { id: "ref-trend-break", label: "Reference", tier: "title", confidence: 0.6, statement: "Title only, 13 May 2026: Why Social Media Marketers should take a break from trend content.", evidence: &[AUTHOR_INDEX] },
    ]
}

fn explicit_edges() -> &'static [(&'static str, &'static str, &'static str)] {
    &[
        ("jack-appleby", "authored", "future-social"),
        ("jack-appleby", "authored", "how-to-hoop-forever"),
        ("future-social", "states", "four-question-strategy"),
        ("future-social", "states", "tried-and-true-mix"),
        ("future-social", "states", "fundamentals-persist"),
        ("future-social", "states", "actionable-takeaway"),
        ("jack-appleby", "states", "explain-the-why"),
        ("jack-appleby", "states", "idea-selection"),
        ("jack-appleby", "states", "stance"),
        ("jack-appleby", "states", "full-idea-on-surface"),
        ("jack-appleby", "states", "expertise-before-pitch"),
        ("jack-appleby", "states", "brands-are-interruptions"),
        ("jack-appleby", "states", "authenticity"),
        ("jack-appleby", "states", "hook-is-a-highlight"),
        ("jack-appleby", "states", "separate-strategy-and-creative"),
        ("jack-appleby", "states", "two-best-practices"),
        ("jack-appleby", "states", "ideas-before-tactics"),
        ("jack-appleby", "states", "emotional-benefit"),
        ("jack-appleby", "states", "attention-not-category"),
        ("jack-appleby", "states", "story-over-production"),
        ("jack-appleby", "states", "paid-to-have-opinion"),
        ("jack-appleby", "states", "b-sides"),
        ("jack-appleby", "states", "youtube-title-gut-check"),
        ("jack-appleby", "states", "brainstorm-based-on"),
        (
            "full-idea-on-surface",
            "supports",
            "republish-is-not-cross-post-ban",
        ),
        (
            "republish-is-not-cross-post-ban",
            "tensions_with",
            "naive-never-republish",
        ),
        ("two-best-practices", "supports", "idea-selection"),
        ("two-best-practices", "supports", "hook-is-a-highlight"),
        ("ideas-before-tactics", "supports", "idea-selection"),
        ("emotional-benefit", "supports", "explain-the-why"),
        (
            "attention-not-category",
            "supports",
            "brands-are-interruptions",
        ),
        ("story-over-production", "supports", "hook-is-a-highlight"),
        ("paid-to-have-opinion", "supports", "stance"),
        ("b-sides", "supports", "tried-and-true-mix"),
        ("brainstorm-based-on", "supports", "idea-selection"),
        ("fundamentals-persist", "supports", "four-question-strategy"),
        (
            "expertise-before-pitch",
            "supports",
            "separate-strategy-and-creative",
        ),
        ("youtube-title-gut-check", "supports", "hook-is-a-highlight"),
    ]
}

fn provenance(confidence: f32, evidence: &[&str]) -> Provenance {
    Provenance::new(
        "brandi-marketing-knowledge",
        "deterministic-corpus",
        confidence,
        0.0,
        0,
        evidence.iter().map(|url| (*url).to_string()).collect(),
    )
}

/// Build the marketing ontology. Static ids are unique; a duplicate is a programming error.
pub fn knowledge_store() -> Store {
    let mut store = Store::new();
    let namespace = NamespaceId::default();
    for claim in claims() {
        store
            .add_node_in_namespace(
                namespace.clone(),
                claim.id,
                claim.label,
                vec![
                    ("id", Scalar::String(claim.id.into())),
                    ("statement", Scalar::String(claim.statement.into())),
                    ("tier", Scalar::String(claim.tier.into())),
                    ("confidence", Scalar::F64(f64::from(claim.confidence))),
                ],
                None,
                provenance(claim.confidence, claim.evidence),
            )
            .expect("static marketing node ids are unique");
    }
    for constraint in PLANNING {
        let id = format!("check-{}", constraint.principle_id);
        store
            .add_node_in_namespace(
                namespace.clone(),
                id,
                "PlanningConstraint",
                vec![
                    (
                        "id",
                        Scalar::String(format!("check-{}", constraint.principle_id)),
                    ),
                    ("statement", Scalar::String(constraint.check.into())),
                    ("tier", Scalar::String("planning".into())),
                    ("confidence", Scalar::F64(1.0)),
                    (
                        "principle_id",
                        Scalar::String(constraint.principle_id.into()),
                    ),
                    ("scope", Scalar::String(constraint.scope.into())),
                ],
                None,
                provenance(1.0, &[FOUR]),
            )
            .expect("static planning constraint ids are unique");
    }

    let mut edges: Vec<(String, &str, String)> = Vec::new();
    for claim in claims() {
        if claim.id == "shared-knowledge" {
            continue;
        }
        edges.push(("shared-knowledge".into(), "contains", claim.id.to_string()));
    }
    for constraint in PLANNING {
        let id = format!("check-{}", constraint.principle_id);
        edges.push(("shared-knowledge".into(), "contains", id.clone()));
        edges.push((id, "applies_to", constraint.principle_id.to_string()));
        edges.push((
            format!("check-{}", constraint.principle_id),
            "constrains",
            "content-planning".into(),
        ));
    }
    for (src, label, dst) in explicit_edges() {
        edges.push(((*src).to_string(), label, (*dst).to_string()));
    }
    for claim in claims() {
        if claim.label == "Reference" {
            continue;
        }
        for url in claim.evidence {
            if let Some(reference) = claims().iter().find(|other| {
                other.label == "Reference" && other.evidence.len() == 1 && other.evidence[0] == *url
            }) {
                edges.push((claim.id.into(), "evidenced_by", reference.id.to_string()));
            }
        }
    }

    for (src, label, dst) in &edges {
        let external_id = format!("{src}--{label}--{dst}");
        let src_id = store
            .node_by_external_id(&namespace, src)
            .unwrap_or_else(|| panic!("missing marketing node {src}"));
        let dst_id = store
            .node_by_external_id(&namespace, dst)
            .unwrap_or_else(|| panic!("missing marketing node {dst}"));
        store
            .add_edge_in_namespace(
                namespace.clone(),
                external_id,
                src_id,
                dst_id,
                label,
                vec![],
                None,
                provenance(1.0, &[PROFILE]),
            )
            .expect("static marketing edge ids are unique");
    }
    store
}

struct NodeView {
    id: String,
    label: String,
    statement: String,
    tier: String,
    confidence: f64,
    evidence: Vec<String>,
}

struct EdgeView {
    src: String,
    label: String,
    dst: String,
}

fn string_prop(store: &Store, node: &padagonia::Node, key: &str) -> String {
    let Some(key_id) = store.string_table().key_id(key) else {
        return String::new();
    };
    node.properties
        .iter()
        .find(|(id, _)| *id == key_id)
        .and_then(|(_, value)| match value {
            Scalar::String(text) => Some(text.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

fn number_prop(store: &Store, node: &padagonia::Node, key: &str) -> f64 {
    let Some(key_id) = store.string_table().key_id(key) else {
        return f64::from(node.provenance.confidence);
    };
    node.properties
        .iter()
        .find(|(id, _)| *id == key_id)
        .and_then(|(_, value)| match value {
            Scalar::F64(number) => Some(*number),
            _ => None,
        })
        .unwrap_or_else(|| f64::from(node.provenance.confidence))
}

fn graph_views(store: &Store) -> (Vec<NodeView>, Vec<EdgeView>) {
    let mut nodes: Vec<NodeView> = store
        .nodes()
        .values()
        .map(|node| NodeView {
            id: node.external_id.clone(),
            label: store
                .string_table()
                .resolve_label(node.label)
                .unwrap_or("")
                .to_string(),
            statement: string_prop(store, node, "statement"),
            tier: string_prop(store, node, "tier"),
            confidence: number_prop(store, node, "confidence"),
            evidence: node.provenance.evidence.clone(),
        })
        .collect();
    nodes.sort_by(|left, right| left.id.cmp(&right.id));
    let mut edges: Vec<EdgeView> = store
        .edges()
        .values()
        .map(|edge| EdgeView {
            src: store
                .nodes()
                .get(&edge.src)
                .map(|node| node.external_id.clone())
                .unwrap_or_default(),
            label: store
                .string_table()
                .resolve_relation(edge.label)
                .unwrap_or("")
                .to_string(),
            dst: store
                .nodes()
                .get(&edge.dst)
                .map(|node| node.external_id.clone())
                .unwrap_or_default(),
        })
        .collect();
    edges.sort_by(|left, right| {
        left.src
            .cmp(&right.src)
            .then(left.label.cmp(&right.label))
            .then(left.dst.cmp(&right.dst))
    });
    (nodes, edges)
}

pub fn knowledge_value() -> Value {
    let store = knowledge_store();
    let (nodes, edges) = graph_views(&store);
    json!({
        "schema_version": SCHEMA_VERSION,
        "corpus_boundary": CORPUS_BOUNDARY,
        "node_count": nodes.len(),
        "edge_count": edges.len(),
        "nodes": nodes.iter().map(|node| json!({
            "id": node.id,
            "label": node.label,
            "tier": node.tier,
            "statement": node.statement,
            "confidence": (node.confidence * 100.0).round() / 100.0,
            "evidence": node.evidence,
        })).collect::<Vec<_>>(),
        "edges": edges.iter().map(|edge| json!({
            "src": edge.src,
            "label": edge.label,
            "dst": edge.dst,
        })).collect::<Vec<_>>(),
        "planning_constraints": PLANNING.iter().map(|constraint| json!({
            "principle_id": constraint.principle_id,
            "scope": constraint.scope,
            "check": constraint.check,
        })).collect::<Vec<_>>(),
    })
}

pub fn reasoning_value() -> Value {
    json!({
        "schema_version": SCHEMA_VERSION,
        "corpus_boundary": CORPUS_BOUNDARY,
        "questions": strategy_questions(),
        "constraints": PLANNING.iter().map(|constraint| json!({
            "principle_id": constraint.principle_id,
            "scope": constraint.scope,
            "check": constraint.check,
        })).collect::<Vec<_>>(),
    })
}

pub fn render_knowledge() -> String {
    let value = knowledge_value();
    let nodes = value["nodes"].as_array().cloned().unwrap_or_default();
    let mut out = vec![
        "Marketing knowledge".to_string(),
        CORPUS_BOUNDARY.to_string(),
        format!(
            "Nodes: {}  Edges: {}",
            value["node_count"].as_u64().unwrap_or(0),
            value["edge_count"].as_u64().unwrap_or(0)
        ),
        String::new(),
        "Shared knowledge".to_string(),
    ];
    let mut grouped: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for node in nodes {
        let label = node["label"].as_str().unwrap_or("").to_string();
        grouped.entry(label).or_default().push(node);
    }
    for (label, group) in &grouped {
        out.push(String::new());
        out.push(label.clone());
        for node in group {
            out.push(format!(
                "- {} ({}, {}): {}",
                node["id"].as_str().unwrap_or(""),
                node["tier"].as_str().unwrap_or(""),
                node["confidence"],
                node["statement"].as_str().unwrap_or("")
            ));
            if let Some(evidence) = node["evidence"].as_array() {
                let urls: Vec<&str> = evidence.iter().filter_map(|item| item.as_str()).collect();
                if !urls.is_empty() {
                    out.push(format!("  evidence: {}", urls.join(", ")));
                }
            }
        }
    }
    out.push(String::new());
    out.push("Planning constraints".to_string());
    for constraint in PLANNING {
        out.push(format!("- {}", constraint.check));
    }
    out.join("\n")
}

/// Write the binary Padagonia graph under `.brandi/state/`.
pub fn save_knowledge(root: &Path) -> Result<PathBuf> {
    let brandi = root.join(".brandi");
    if !brandi.is_dir() {
        return Err(BrandiError::Invalid(format!(
            "no .brandi directory in {}",
            root.display()
        )));
    }
    let state = brandi.join("state");
    fs::create_dir_all(&state)?;
    let path = state.join("marketing-knowledge.padagonia");
    knowledge_store()
        .save(&path)
        .map_err(|err| BrandiError::Invalid(format!("padagonia save failed: {err}")))?;
    Ok(path)
}

pub fn marketing_principle_ids() -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut ids = Vec::new();
    for constraint in PLANNING {
        if seen.insert(constraint.principle_id) {
            ids.push(constraint.principle_id.to_string());
        }
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;
    use padagonia::Store;
    use std::collections::{HashSet, VecDeque};

    #[test]
    fn corpus_boundary_says_the_archive_is_incomplete() {
        assert!(CORPUS_BOUNDARY.contains("not a complete archive"));
        assert!(CORPUS_BOUNDARY.contains("2026-10-06"));
        assert!(CORPUS_BOUNDARY.contains("2009-02-02"));
    }

    #[test]
    fn every_principle_and_method_has_evidence_and_known_endpoints() {
        let store = knowledge_store();
        let (nodes, edges) = graph_views(&store);
        let ids: HashSet<&str> = nodes.iter().map(|node| node.id.as_str()).collect();
        for node in &nodes {
            if matches!(node.label.as_str(), "Principle" | "Method") {
                assert!(!node.evidence.is_empty(), "{} has no evidence", node.id);
                assert!(!node.statement.is_empty(), "{} has no statement", node.id);
            }
            let blob = format!("{} {}", node.statement, node.evidence.join(" "));
            assert!(!blob.contains("sk-"), "{}", node.id);
            assert!(!blob.to_ascii_lowercase().contains("api_key"));
        }
        for edge in &edges {
            assert!(ids.contains(edge.src.as_str()), "missing {}", edge.src);
            assert!(ids.contains(edge.dst.as_str()), "missing {}", edge.dst);
        }
        for constraint in PLANNING {
            assert!(
                ids.contains(constraint.principle_id),
                "planning id {}",
                constraint.principle_id
            );
        }
    }

    #[test]
    fn root_reaches_every_node() {
        let store = knowledge_store();
        let root = store
            .node_by_external_id(&NamespaceId::default(), "shared-knowledge")
            .expect("root");
        let mut seen = HashSet::new();
        let mut queue = VecDeque::from([root]);
        while let Some(id) = queue.pop_front() {
            if !seen.insert(id) {
                continue;
            }
            for edge in store.edges().values() {
                if edge.src == id {
                    queue.push_back(edge.dst);
                }
            }
        }
        assert_eq!(seen.len(), store.nodes().len());
    }

    #[test]
    fn save_round_trips_counts_and_rejects_a_missing_brief_dir() {
        let store = knowledge_store();
        let nodes = store.nodes().len();
        let edges = store.edges().len();
        let tmp = tempfile::tempdir().unwrap();
        let missing = save_knowledge(tmp.path()).unwrap_err();
        assert!(missing.to_string().contains(".brandi"), "{missing}");
        fs::create_dir(tmp.path().join(".brandi")).unwrap();
        let path = save_knowledge(tmp.path()).unwrap();
        let loaded = Store::load(&path).unwrap();
        assert_eq!(loaded.nodes().len(), nodes);
        assert_eq!(loaded.edges().len(), edges);
        assert!(path.ends_with("marketing-knowledge.padagonia"));
    }
}
