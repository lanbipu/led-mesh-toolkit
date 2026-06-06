# Product

## Register

product

## Users

Primary users are **on-site AV / LED calibration engineers** working at LED
screen and wall installations. Their context shapes everything:

- They work in the field, often under uncontrolled, mixed lighting (bright
  ambient, glare, dim back-of-house). The screen must stay readable on a laptop
  in those conditions.
- They are time-pressured and standing up, not sitting at a tuned monitor. The
  happy path (measure -> reconstruct -> export) has to be fast and forgiving of
  mis-clicks.
- They are technically fluent. They understand measurement error, coordinate
  frames, and reconstruction; the UI should respect that competence rather than
  hand-hold.

Secondary context: the same projects get opened indoors for reconstruction
review and hand-off into downstream pipelines (disguise, Unreal). The tool also
ships a headless `lmt` CLI that exposes the same service layer, so the GUI is one
of two equal transports, not the only door in.

## Product Purpose

LED Mesh Toolkit reconstructs the **metric 3D geometry of LED screens and walls**
from on-site capture, and exports world-coordinate models plus UVs for downstream
pipelines.

It covers the full field workflow: define the screen, choose a measurement method
(total-station resection via Trimble SX, or visual / structured-light capture),
generate a printable measurement instruction card, import the captured data,
reconstruct, preview the result in 3D, and review reconstruction runs with their
error metrics.

Success looks like: an engineer leaves the site confident the geometry is
correct, with per-point error legible and uncertainty made visible rather than
hidden. The output has to be metrically trustworthy because everything
downstream (pixel mapping, projection, content fit) inherits its accuracy.

## Brand Personality

Three words: **precise, restrained, distinctive.**

- Voice and tone: technical and direct. State facts and numbers, never market.
  Confident expert peer, not a cheerful assistant. No fluff, no exclamation
  energy.
- Honest about uncertainty: the interface should say what it does not know
  (the hatched "unknown" pattern is the philosophy made visual), not paper over
  gaps with confident-looking defaults.
- Distinctive on purpose: the functional core is mission-control precise, but
  the identity is earned through typography and hierarchy. It should not be
  mistaken for a stock shadcn dashboard. Memorable through restraint and craft,
  not decoration.
- Emotional goal: the engineer feels in control and trusts the numbers.

## Anti-references

The user cited no strong dislikes, but the "more distinctive" direction implies
one clear thing to avoid:

- **Generic shadcn / SaaS dashboard sameness.** No big-number hero-metric cards,
  no endless identical icon + heading + text card grids, no gradient accents
  standing in for hierarchy. If a screen could be any B2B SaaS tool, it has
  failed the distinctiveness goal.

Plus the universal bans: side-stripe accent borders, gradient text,
glassmorphism-by-default, modal-as-first-thought.

## Design Principles

1. **Glanceable truth.** A field engineer must read state (what is measured,
   what is missing, whether a run is good) at a glance, in bad lighting, without
   parsing. Status earns prominence; everything else recedes.

2. **Precision is the product.** The output is geometry that has to be metrically
   correct, so the UI must make accuracy legible: errors in real units, measured
   vs. guessed clearly distinguished, never a false sense of certainty.

3. **Honest about uncertainty.** Show what is unknown. Empty, partial, and
   low-confidence states are first-class and visually distinct, not afterthoughts.

4. **Field-first ergonomics.** Optimize the standing-up, time-pressured path.
   Fast, forgiving of mis-clicks, robust under real conditions. The tool should
   never fight the engineer on site.

5. **Distinctive, not default.** Earn a recognizable identity through type and
   hierarchy. Never settle for the generic-dashboard look just because the
   component library makes it easy.

## Accessibility & Inclusion

The user was undecided, so these are reasonable defaults, several of which the
field-lighting context independently demands:

- **WCAG AA contrast** as the floor (the uncontrolled-lighting context makes this
  functional, not just compliance).
- **Status never by color alone.** Reinforce with icon, label, or pattern (the
  hatched fill already does this for unknown states); essential for glare and
  color-vision differences.
- **Keyboard reachable** for all primary actions.
- **Respect `prefers-reduced-motion`**; motion is functional feedback, never
  decoration that can't be turned off.
- Revisit and raise the bar if the tool moves beyond an internal / field-team
  audience.
