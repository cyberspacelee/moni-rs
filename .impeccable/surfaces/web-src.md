---
version: 1
slug: "web-src"
primary_target: "web/src"
related_targets: []
---

## Scope
Whole web app (`web/src`): overview, server detail, login, management. Mode: Operate. Owner-operator and public visitor carry equal weight; phone is first-class.

## Direction contract
FORM: seed:e6a6dded94cadd18 — assigned direction, railway station clock network.
THESIS: The hub is the master clock and every VPS is a slave clock: online servers tick with each 5 s frame, offline ones stop at their last heartbeat. Refuses the grey-card-grid-with-one-accent default and the purple/blue SaaS look.
OWN-WORLD: Enamel white ground, sign-black shell (top bar, primary buttons), clock-black ink, signal red used only on the second hand and the logo. State: green online, amber near limit, crimson alert, always with icon + text. System grotesk, large tabular numerals, hairline rules, square-ish 10px corners, no glow, no glass, no gradients.
STORY: At a glance: which clocks are running, which are strained, which stopped and when. Click through for history; manage behind login.
FIRST VIEWPORT: Black station-sign bar; a quiet KPI strip of plain numerals (not cards); cards whose top-left is a live mini station clock. A stopped clock shows its last-seen time.
SIGNATURE: Live mini station clock per server: the red second hand jumps on each frame and halts when the agent goes silent; offline = stopped clock at last-seen time.
RAISES: offline cards persist and are stamped; one meaning per colour; CPU chart spans full width on detail; hierarchy by numeral scale; ←/→ between servers on detail.
