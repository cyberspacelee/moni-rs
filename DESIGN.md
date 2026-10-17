---
name: moni
description: Self-hosted VPS monitor on Cloudflare's free tier, drawn as a network of railway station clocks.
colors:
  enamel: "#f3f3f0"
  surface: "#ffffff"
  surface-2: "#ebebe7"
  line: "#dddcd6"
  line-strong: "#c6c5be"
  ink: "#111111"
  ink-2: "#45443f"
  ink-3: "#6b6a64"
  sign-black: "#111111"
  shell-ink: "#f3f3f0"
  shell-ink-2: "#a7a69f"
  signal-red: "#e63312"
  series-ink: "#1f1f1e"
  series-burnt-orange: "#c2561a"
  state-good: "#13843a"
  state-warning: "#a87200"
  state-critical: "#c81e4a"
  dark-ground: "#0f0f0e"
  dark-surface: "#181817"
  dark-ink: "#ecebe6"
  dark-signal-red: "#ff5533"
typography:
  headline:
    fontFamily: "ui-sans-serif, system-ui, -apple-system, Segoe UI, PingFang SC, Microsoft YaHei, sans-serif"
    fontSize: "1.5rem"
    fontWeight: 600
    lineHeight: "2rem"
    letterSpacing: "-0.015em"
  reading:
    fontFamily: "ui-sans-serif, system-ui, -apple-system, Segoe UI, PingFang SC, Microsoft YaHei, sans-serif"
    fontSize: "1.875rem"
    fontWeight: 600
    lineHeight: "2rem"
    fontFeature: "tnum"
  title:
    fontSize: "1rem"
    fontWeight: 600
    lineHeight: "1.25rem"
  body:
    fontSize: "0.875rem"
    fontWeight: 400
    lineHeight: "1.25rem"
  label:
    fontSize: "0.75rem"
    fontWeight: 400
    lineHeight: "1rem"
  mono:
    fontFamily: "ui-monospace, SF Mono, JetBrains Mono, Cascadia Mono, Menlo, monospace"
    fontSize: "0.8125rem"
rounded:
  tick: "2px"
  control: "6px"
  panel: "10px"
spacing:
  hairline: "1px"
  tight: "6px"
  group: "12px"
  panel: "16px"
  panel-wide: "20px"
  section: "32px"
components:
  shell-bar:
    backgroundColor: "{colors.sign-black}"
    textColor: "{colors.shell-ink}"
    height: "56px"
  button-primary:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.enamel}"
    rounded: "{rounded.control}"
    height: "36px"
    padding: "0 14px"
  button-secondary:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.ink}"
    rounded: "{rounded.control}"
    height: "36px"
  segmented-selected:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.enamel}"
    rounded: "4px"
    height: "28px"
  panel:
    backgroundColor: "{colors.surface}"
    rounded: "{rounded.panel}"
  meter:
    backgroundColor: "{colors.surface-2}"
    rounded: "{rounded.tick}"
    height: "6px"
---

# Design System: moni

## Overview

**Creative North Star: "The Station Clock Network"**

The hub is a master clock; every VPS is a slave clock wired to it. A running server ticks with each 5 s frame; a silent one stops at its last heartbeat and stays on the board, stamped with when it stopped. The world lends the product only type, palette, density and that one signature move; navigation and controls are the web's standard ones.

The look is enamel and sign-black: a black station-sign bar, enamel-white ground, clock-black ink, numerals that carry hierarchy on their own, and a single signal red that belongs to the second hand. Rejected on purpose: purple/blue gradients, glow, glassmorphism, and a plain grey dashboard with one accent.

**Key Characteristics:**
- Mini station clock on every server (overview card, detail header, management row) is the identity.
- Readings sit in hairline-divided strips, not hero-metric cards.
- Flat surfaces; depth only on hover and overlays.
- System sans everywhere, tabular numerals for every number, monospace only for identifiers and commands.

## Colors

Restrained neutrals tinted toward enamel, a committed sign-black shell, one signal colour, and a state vocabulary that never doubles as decoration.

### Primary
- **Sign Black** (#111111): the shell bar, primary buttons, selected segment, logo ink, meter fill at rest. In dark mode the shell goes to #050505 and primary actions invert to Dark Ink.
- **Signal Red** (#e63312 / dark #ff5533): only the clock's second hand, the logo's second hand, the active management tab underline, text caret and selection tint.

### Secondary
- **Burnt Orange** (#c2561a / dark #f0813f): the second chart series (peak, swap, upload) and the upload arrow.

### Neutral
- **Enamel** (#f3f3f0): page ground. **Surface** (#ffffff): panels. **Surface 2** (#ebebe7): meter tracks, hovers, inline code.
- **Ink / Ink 2 / Ink 3** (#111111 / #45443f / #6b6a64): text steps; Ink 3 holds ≥4.5:1 on Enamel.
- **Line / Line Strong** (#dddcd6 / #c6c5be): hairlines and hovered borders.

### Named Rules
**The One Meaning Rule.** Green is online, amber is near a limit (≥80 %), crimson is alerting, signal red is "the clock is running". Crimson is pulled toward pink so it never reads as the signal, and every state colour travels with an icon or a word.

**The Hollow Ring Rule.** Offline is a hollow ring plus the word 离线, never a grey filled dot that reads as "disabled".

## Typography

**Body Font:** the platform sans (ui-sans-serif, system-ui, PingFang SC, Microsoft YaHei). No web fonts are downloaded.
**Mono Font:** ui-monospace stack, for server IDs, hostnames, kernels, tokens and install commands only.

**Character:** a workhorse sans set tight, where the size of a numeral is the hierarchy.

### Hierarchy
- **Headline** (600, 24px/32px, -0.015em): page titles and the server name on detail.
- **Reading** (600, 30px/32px, tabular): a card's CPU numeral; summary-strip values run 24px, detail readings 18px.
- **Title** (600, 16px/20px): card server names, section headings.
- **Body** (400, 14px/20px): rows, forms, prose.
- **Label** (400, 12px/16px): field labels, meta lines, axis-adjacent legends.

### Named Rules
**The Numerals Only Rule.** Every figure uses `font-variant-numeric: tabular-nums` in the sans. Monospace never dresses numbers up as "technical".

## Layout

Max width 1280px (max-w-7xl), 16px gutters on phones and 24px from 640px up. The overview is a summary strip (2 columns, 4 from 1024px) over a server grid (1 → 2 → 3 columns). Detail: a six-reading strip (2 → 3 → 6 columns), then the CPU chart across the full width, then the remaining charts two-up. Strips draw hairlines with 1px grid gaps over a Line-coloured ground. Phones get a fixed bottom tab bar when there is more than one destination, and the footer pads itself to clear it.

## Elevation & Depth

Flat by default. Panels are surface plus a 1px Line border with no resting shadow. Depth appears only as a response: hovered server cards lift with `0 6px 20px -10px rgb(0 0 0 / 0.25)`, chart tooltips use `0 8px 24px -8px rgb(0 0 0 / 0.25)`, dialogs use the platform-scale `shadow-2xl` over a 45 % black backdrop.

## Shapes

Corners are small and engineered: 10px panels, 6px controls, 4px segments, 2px meter tracks, and a full circle only for clocks and status dots. Icons are Lucide at a 1.5px absolute stroke, so a 12px glyph and a 16px glyph match the text stems beside them.

## Components

- **Station clock**: a 40-unit enamel face with a black ring, 12 ticks (longer at quarters), and black hour and minute hands. The signal-red second hand jumps to each frame's second with a 320ms ease-out and parks at 12 in grey when stopped. A face with no hands and a grey hub means never connected. Its accessible name is 运行中, 停于 HH:MM or 从未连接.
- **Shell bar**: 56px sign-black, logo + 总览/管理 nav (active = white 12 % tint), live indicator (dot + 实时/重连中), alert count as a filled crimson chip, then theme and log-out ghost buttons.
- **Server card**: header row (clock 36px · name/badge over flag + region · cores · arch · status). Running: CPU numeral + live sparkline, then a hairline section with memory and disk meters, down/up rates, the month's traffic and uptime. Stopped: a dashed rule, then 停于 time and how long ago.
- **Meter**: 6px track with a fill in Series Ink, amber from 80 % and crimson from 90 %. The printed value always sits beside it.
- **Segmented control**: a bordered surface; the selected segment is filled Sign Black.
- **Footer**: logo, version, tagline; GitHub, MIT license, and © year with the author linking to GitHub.

## Do's and Don'ts

- **Do** keep a stopped server on the board with its stop time; never empty its card.
- **Do** show a state with colour and words or an icon together.
- **Don't** use purple, blue gradients, glow, glass, or blurred translucent bars.
- **Don't** spend Signal Red on anything that is not the running clock, the logo, or the active-tab underline.
- **Don't** put KPIs in big-number cards; use the hairline reading strip.
- **Don't** use monospace for numbers.
