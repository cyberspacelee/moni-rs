---
name: moni
description: Self-hosted VPS monitor on Cloudflare's free tier.
colors:
  ivory: "#faf9f5"
  surface: "#ffffff"
  oat: "#f2f0e9"
  line: "#e8e6dc"
  line-strong: "#d6d3c7"
  slate: "#141413"
  slate-2: "#4d4c48"
  slate-3: "#73726c"
  clay: "#b9532f"
  clay-strong: "#a14626"
  clay-soft: "#f7e9e1"
  series-clay: "#c96442"
  series-sky: "#5b8fc4"
  state-good: "#2e7d4f"
  state-warning: "#b07a12"
  state-critical: "#c8372d"
  dark-ground: "#1b1a19"
  dark-surface: "#242322"
  dark-ink: "#f4f3ee"
  dark-clay: "#d97757"
typography:
  headline:
    fontFamily: "-apple-system, BlinkMacSystemFont, SF Pro Text, Segoe UI, PingFang SC, Hiragino Sans GB, Microsoft YaHei, sans-serif"
    fontSize: "1.5rem"
    fontWeight: 600
    lineHeight: "2rem"
    letterSpacing: "-0.015em"
  figure:
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
  control: "8px"
  segment: "6px"
  panel: "16px"
  pill: "9999px"
spacing:
  tight: "6px"
  group: "12px"
  panel: "20px"
  section: "32px"
components:
  button-primary:
    backgroundColor: "{colors.clay}"
    textColor: "#ffffff"
    rounded: "{rounded.control}"
    height: "36px"
    padding: "0 14px"
  button-primary-hover:
    backgroundColor: "{colors.clay-strong}"
  button-secondary:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.slate}"
    rounded: "{rounded.control}"
    height: "36px"
  sign-in:
    backgroundColor: "{colors.slate}"
    textColor: "{colors.ivory}"
    rounded: "{rounded.pill}"
    height: "32px"
  segmented-selected:
    backgroundColor: "{colors.surface}"
    textColor: "{colors.slate}"
    rounded: "{rounded.segment}"
    height: "28px"
  panel:
    backgroundColor: "{colors.surface}"
    rounded: "{rounded.panel}"
  meter:
    backgroundColor: "{colors.oat}"
    rounded: "{rounded.pill}"
    height: "6px"
---

# Design System: moni

## Overview

**Creative North Star: "The Quiet Instrument"**

Clean, restrained and enterprise, in the spirit of Apple and Anthropic product pages: warm ivory and slate neutrals, one clay accent, generous spacing and soft corners. The numbers are the content; the chrome stays out of their way. The owner explicitly rejected purple/blue gradients, glow, glass, stark black-and-white, decorative icons, metaphor devices on server cards, and filler taglines.

**Key Characteristics:**
- Warm neutrals with a single clay accent for actions, selection and the primary data series.
- Text labels instead of decorative icons; icons only where they act (theme, sign-out, search, back/next, alert).
- Readings in hairline-divided strips, server cards with label/value rows.
- Flat surfaces with a whisper of shadow; depth grows on hover.

## Colors

### Primary
- **Clay** (#b9532f, dark #d97757): primary buttons, active tab underline, focus ring, links, checkbox accent. Clay also fills meters at rest and draws the first chart series (#c96442).

### Secondary
- **Sky** (#5b8fc4, dark #7fb0e0): the second chart series only (peak, swap, upload).

### Neutral
- **Ivory** (#faf9f5) page ground, **Surface** (#ffffff) panels, **Oat** (#f2f0e9) tracks and segmented-control ground.
- **Slate / Slate 2 / Slate 3** (#141413 / #4d4c48 / #73726c): text steps; Slate 3 holds ≥4.5:1 on Ivory.
- **Line / Line Strong** (#e8e6dc / #d6d3c7): hairlines and hovered borders.

### Named Rules
**The State Words Rule.** Green, amber and red mean online, near-limit (≥80 %) and alerting, and always travel with a word or an icon. Offline is a hollow ring plus 离线.

## Typography

The Apple system stack with PingFang SC for Chinese; no web fonts. Every figure uses tabular numerals in the sans; monospace is only for server IDs, hostnames, kernels, tokens and commands.

- **Headline** (600, 24px/32px): page titles, server name on detail.
- **Figure** (600, 30px): a card's CPU value; summary values 24px, detail readings 18px.
- **Title** (600, 16px/20px): card names, section headings.
- **Body** (400, 14px/20px). **Label** (400, 12px/16px): field labels, meta lines, card rows.

## Layout

Max width 1280px with 16px / 24px gutters. Overview: a summary strip (2 → 4 columns) over a server grid (1 → 2 → 3). Detail: six readings (2 → 3 → 6), the CPU chart across the full width, then the other charts two-up. Strips draw hairlines with 1px grid gaps over a Line-coloured ground. Phones get a bottom tab bar and the footer pads itself to clear it. The footer holds only © year + author, version, GitHub and the license.

## Elevation & Depth

Panels rest at `0 1px 2px rgb(20 20 19 / 0.04)` with a 1px Line border. Hovered server cards lift to `0 8px 24px -12px rgb(20 20 19 / 0.18)`. The header is Ivory at 90 % with a light backdrop blur so content scrolls under it legibly. Dialogs sit over a 35 % black backdrop.

## Shapes

16px panels, 8px controls, 6px segments, pills for status badges, the sign-in button and meters. Icons are Lucide at a 1.5px absolute stroke.

## Components

- **Server card**: name and alert badge over flag · region · cores · arch, with the status on the right. Below: the CPU figure with a live sparkline, memory and disk meters, then rows for network, monthly traffic and uptime. An offline card says when it was last seen.
- **Segmented control**: Oat ground; the selected segment is a white raised pill.
- **Header**: logo + wordmark, text nav (the active item is Slate and medium weight), alert pill, live dot + 实时, theme and sign-out icons, and a pill sign-in for visitors.

## Do's and Don'ts

- **Do** use words before icons; an icon must perform an action or carry state.
- **Do** keep every number in tabular sans with its unit.
- **Don't** add taglines or self-description to the chrome.
- **Don't** use purple, gradients, glow, glass panels, or stark black-and-white.
- **Don't** put metaphor devices (clocks, gauges, mascots) on server cards.
