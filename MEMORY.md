# Brandi memory

## Architecture decisions

- Brandi's identity palette is hot pink (`#FF2DAA`) with dark plum (`#160812`) and soft white (`#FFF7FC`).
- Public promotion is approval-gated. Generated drafts are immutable revisions in an append-only queue; Telegram sends the exact approved text and records success or failure.
- VHS generation separates planning, deterministic compilation, critique, deterministic safety checks, native VHS validation, saving, and rendering. Saving is confined to `demos/`; rendering is always explicit.
- Promotion plans derive from the brand genome and measured Git/GitHub/Telegram evidence. Padagonia is an optional durable sync target backed by a local JSONL outbox.
- Telegram supports one hub bot routing multiple projects or a fleet of project workers in one supervisor process. Q/A uses read-only local evidence and optional Groq Compound Mini; credentials live only in environment variables.
- Kaptaind integration reads release indexes as evidence and never starts, stops, or mutates Kaptaind.
