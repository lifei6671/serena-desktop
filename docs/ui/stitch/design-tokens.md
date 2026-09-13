---
> 此文件是 Stitch 导出的参考 token，不是权威视觉规范；[docs/ui/DESIGN.md](../DESIGN.md) 才是 authoritative。

name: Serena Desktop Canonical
colors:
  surface: '#f8f9ff'
  surface-dim: '#cbdbf5'
  surface-bright: '#f8f9ff'
  surface-container-lowest: '#ffffff'
  surface-container-low: '#eff4ff'
  surface-container: '#e5eeff'
  surface-container-high: '#dce9ff'
  surface-container-highest: '#d3e4fe'
  on-surface: '#0b1c30'
  on-surface-variant: '#3c4a42'
  inverse-surface: '#213145'
  inverse-on-surface: '#eaf1ff'
  outline: '#6c7a71'
  outline-variant: '#bbcabf'
  surface-tint: '#006c49'
  primary: '#006c49'
  on-primary: '#ffffff'
  primary-container: '#10b981'
  on-primary-container: '#00422b'
  inverse-primary: '#4edea3'
  secondary: '#565e74'
  on-secondary: '#ffffff'
  secondary-container: '#dae2fd'
  on-secondary-container: '#5c647a'
  tertiary: '#005ac2'
  on-tertiary: '#ffffff'
  tertiary-container: '#71a1ff'
  on-tertiary-container: '#00367a'
  error: '#ba1a1a'
  on-error: '#ffffff'
  error-container: '#ffdad6'
  on-error-container: '#93000a'
  primary-fixed: '#6ffbbe'
  primary-fixed-dim: '#4edea3'
  on-primary-fixed: '#002113'
  on-primary-fixed-variant: '#005236'
  secondary-fixed: '#dae2fd'
  secondary-fixed-dim: '#bec6e0'
  on-secondary-fixed: '#131b2e'
  on-secondary-fixed-variant: '#3f465c'
  tertiary-fixed: '#d8e2ff'
  tertiary-fixed-dim: '#adc6ff'
  on-tertiary-fixed: '#001a42'
  on-tertiary-fixed-variant: '#004395'
  background: '#f8f9ff'
  on-background: '#0b1c30'
  surface-variant: '#d3e4fe'
  shell-dark: '#090D16'
  shell-surface: '#0F172A'
  shell-border: '#1E293B'
  shell-text: '#CBD5E1'
  status-healthy: '#10B981'
  status-running: '#3B82F6'
  status-warning: '#F59E0B'
  status-error: '#EF4444'
  surface-canvas: '#FFFFFF'
  surface-subtle: '#F8FAFC'
  surface-sidebar: '#F1F5F9'
  border-subtle: '#E2E8F0'
  border-strong: '#94A3B8'
typography:
  headline-lg:
    fontFamily: Geist
    fontSize: 24px
    fontWeight: '700'
    lineHeight: 32px
    letterSpacing: -0.02em
  headline-md:
    fontFamily: Geist
    fontSize: 18px
    fontWeight: '600'
    lineHeight: 24px
    letterSpacing: -0.015em
  headline-sm:
    fontFamily: Geist
    fontSize: 14px
    fontWeight: '700'
    lineHeight: 20px
    letterSpacing: -0.01em
  body-lg:
    fontFamily: Geist
    fontSize: 14px
    fontWeight: '400'
    lineHeight: 22px
  body-md:
    fontFamily: Geist
    fontSize: 13px
    fontWeight: '400'
    lineHeight: 18px
  body-sm:
    fontFamily: Geist
    fontSize: 12px
    fontWeight: '400'
    lineHeight: 16px
  nav-active:
    fontFamily: Geist
    fontSize: 13px
    fontWeight: '600'
    lineHeight: 18px
  nav-default:
    fontFamily: Geist
    fontSize: 13px
    fontWeight: '500'
    lineHeight: 18px
  button-label:
    fontFamily: Geist
    fontSize: 12px
    fontWeight: '600'
    lineHeight: 16px
  code-default:
    fontFamily: JetBrains Mono
    fontSize: 12px
    fontWeight: '400'
    lineHeight: 18px
  code-badge:
    fontFamily: JetBrains Mono
    fontSize: 11px
    fontWeight: '500'
    lineHeight: 14px
  tech-micro:
    fontFamily: JetBrains Mono
    fontSize: 10px
    fontWeight: '400'
    lineHeight: 12px
    letterSpacing: 0.02em
  status-label:
    fontFamily: Geist
    fontSize: 11px
    fontWeight: '400'
    lineHeight: 14px
rounded:
  sm: 0.125rem
  DEFAULT: 0.25rem
  md: 0.375rem
  lg: 0.5rem
  xl: 0.75rem
  full: 9999px
spacing:
  gutter: 1rem
  margin: 1.5rem
  space-xs: 0.25rem
  space-sm: 0.5rem
  space-md: 1rem
  space-lg: 1.5rem
  space-xl: 2rem
---

## Brand & Style

This design system establishes a high-density, engineering-grade desktop interface tailored for developers operating Serena runtimes, Model Context Protocol (MCP) services, workspaces, and autonomous agent orchestrations. The aesthetic is utilitarian, calm, and rigorous—rooted in modern developer tooling rather than consumer software paradigms.

### Design Movements & Tone
- **Technical Minimalist / Tooling Utility:** The visual structure prioritizes high information density, structural boundary separation (`1px` borders), and visual hierarchy over gratuitous whitespace or deep drop shadows.
- **Hybrid Desktop Architecture:** A deep, native chrome frame (`#090D16`) locks the window hierarchy, grounding an ultra-crisp, high-readability light workspace canvas (`#FFFFFF` and `#F8FAFC`).
- **Instrument Precision:** Interfaces behave like mission control surfaces. Technical identifiers, process IDs, ports, paths, and execution telemetry are treated as first-class architectural elements.
- **Emotional Intent:** The system instills confidence, deterministic predictability, and absolute operational clarity. State transitions are instantaneous and explicit; ambiguity is eliminated through multi-signal feedback patterns.

## Colors

The system uses a chromatic hierarchy focused strictly on runtime feedback and semantic state delivery. Chromatic colors are intentionally withheld from purely decorative elements.

### Architecture & Workspaces
- **Native Shell Chrome (`#090D16`):** Applied to the window title bar, framing the application with a permanent reference point for window controls, project context, and workspace status.
- **Workspace Canvas (`#FFFFFF`):** High-contrast background for execution workspaces, card surfaces, and core editing environments.
- **Secondary Surfaces (`#F8FAFC` to `#F1F5F9`):** Dedicated to auxiliary sidebars, status bars, and segmented container panels.
- **Structural Borders (`#E2E8F0`):** Crisp 1px structural separation for cards, dividers, and inputs, stepping to `#CBD5E1` on interactive hover.

### Semantic State System
Every state is communicated with high-contrast foregrounds and delicate background tints to keep density clear and scannable:
- **Healthy / Connected / Brand (`#10B981`):** Primary green indicator. Used for active runtime state, healthy MCP connections, and verified execution passes. Accompanied by emerald-50 backgrounds and emerald-700 text.
- **Running / Orchestrating (`#3B82F6`):** Dedicated execution blue. Signifies active sub-agent processes, active compilations, or streaming tasks. Accompanied by blue-50 backgrounds and blue-700 text.
- **Warning / Degraded (`#F59E0B`):** System throttling, unpinned configurations, or pending restarts.
- **Destructive / Error (`#EF4444`):** Fatal process termination, runtime exceptions, disconnected MCP endpoints.

## Typography

The type scale balances structural developer readability with technical data precision. 

### Font Architecture
- **Primary Interface Font (`Geist`):** Delivers clean optical tracking, neutral modern forms, and structural alignment for headers, navigation, form inputs, and descriptive body text.
- **Technical Monospace Font (`JetBrains Mono`):** Standardizes all machine data. Used for PIDs, IP addresses, port numbers, commit hashes, JSON/YAML fragments, CLI parameters, and micro timestamps.

### Composition Guidelines
- **Strict Size Floor:** No element renders below `10px`. Micro labels at `10px` to `11px` must maintain uppercase styling or distinct token backgrounds to preserve immediate legibility.
- **Numeric Precision:** Tabular figures are enforced for numerical logs, resource utilization readouts (CPU/RAM metrics), and runtime timers.

## Layout & Spacing

The layout is built on a strict **4px base rhythm** (4px, 8px, 12px, 16px, 20px, 24px, 32px), engineered to maximize actionable content density and eliminate vertical dead zones.

### Shell Architecture
- **Native Title Bar:** Fixed at `36px` height, spanning 100% of the desktop window width. Background: `#090D16`.
- **Navigation Sidebar:** Fixed width at `256px`. Internal list gaps use `4px` between nav items; padding is `10px` horizontally.
- **Status Bar:** Fixed at `24px` height at the window base. Background: `#F1F5F9`, border-top: `1px solid #E2E8F0`.
- **Main Content Canvas:** Fluid width with a max-width container of `1152px` for standard operational forms and dashboard dashboards. Terminal windows, live telemetry grids, and diff viewers expand to 100% fluid container width.

### Responsive Breakpoints & Viewport Constraints
- Designed strictly for desktop screens: `1280px`, `1440px`, `1600px`, and `1920px`.
- When window width collapses under `1280px`, multi-column cards reflow to single-column vertical stacks, and secondary metadata columns in tables fold into expandable summary views.

## Elevation & Depth

This design system avoids high-blur drop shadows and multi-layered skeuomorphic illusions in favor of **Structural Flat Architecture**. Depth and division are expressed through 1px crisp borders, surface tonal shifts, and minimal micro-shadows.

### Structural Depth Rules
- **Canvas Base:** Standard cards rest flush against the surface canvas using `1px solid #E2E8F0`. No box shadows are applied to idle cards.
- **Micro-Elevation (`shadow-xs`):** `0 1px 2px 0 rgba(0, 0, 0, 0.05)` applied exclusively to standard primary action controls and contextual input focus states.
- **Floating Overlays (`shadow-sm`):** `0 4px 6px -1px rgba(0, 0, 0, 0.07), 0 2px 4px -2px rgba(0, 0, 0, 0.05)` restricted to floating dropdown menus, context menus, command palettes, and modal dialogs.
- **Focus Rings:** Distinctive 2px structural outline using `rgba(15, 23, 42, 0.1)` (`slate-900` at 10%) with zero blur offset to deliver crisp keyboard accessibility.

## Shapes

The geometric silhouette remains sharp, restrained, and engineering-focused. Rounding is kept strictly between **4px** and **8px**.

### Corner Radius System
- **Micro Radius (4px / `radius-xs`):** Applied to technical status badges, monospace tag pills, micro indicators, and tight list selection boxes.
- **Standard Control Radius (6px / `radius-sm`):** Buttons, navigation links, input triggers, and contextual search bars.
- **Container Radius (8px / `radius-md`):** Workspace panels, cards, orchestration step boxes, modal windows, and dialog viewports.
- **Full Radius (`9999px`):** Status indicator dots and scrollbar thumbs only. Large rounded pill buttons are strictly forbidden.

## Components

### Buttons
- **Primary Action:** Solid `#0F172A` (slate-900) background, `#FFFFFF` text, `6px` radius, height `30px` to `34px`, font size `12px` (semi-bold). Hover shifts to `#1E293B`. Active press scales subtly to `0.98`.
- **Secondary Action:** `#FFFFFF` background, `1px solid #E2E8F0` border, `#334155` text. Hover shifts to `#F8FAFC` with border `#CBD5E1`.
- **Ghost / Utility:** Transparent background, `#475569` text. Hover shifts to `#F1F5F9`.

### Badges & Technical Indicators
- **Structure:** `4px` radius, `10px` to `11px` typography, `2px 6px` padding, inline monospace support.
- **Status Indicator Tokens:**
  - *Healthy:* Background `#ECFDF5`, text `#047857`, border `1px solid #A7F3D0`, leading 6px dot `#10B981`.
  - *Running:* Background `#EFF6FF`, text `#1D4ED8`, border `1px solid #BFDBFE`, animated 6px blue pulse dot.
  - *Warning:* Background `#FFFBEB`, text `#B45309`, border `1px solid #FDE68A`.
  - *Error:* Background `#FEF2F2`, text `#B91C1C`, border `1px solid #FECACA`.

### Input Fields & Select Controls
- **Height:** Compact `28px` to `32px`.
- **Surface:** `#FFFFFF` with `1px solid #E2E8F0`, interior padding `6px 10px`, typography `13px` Geist.
- **Focus:** `1px solid #94A3B8` border and `2px solid rgba(15, 23, 42, 0.08)` focus ring.
- **Monospace Code Inputs:** Paths, environment values, and query selectors use `JetBrains Mono` at `12px`.

### Cards & Process Containers
- **Default Card:** `#FFFFFF` background, `1px solid #E2E8F0` border, `8px` corner radius, internal padding `14px` to `16px`. Hover state changes border to `#CBD5E1`.
- **Running Task Card:** Subtle border shift to `1px solid #BFDBFE` with an optional `0.5px` top highlight band in `#3B82F6`.

### Lists & Navigation Trees
- **Sidebar Nav Item:** Height `32px`, horizontal padding `10px`, radius `6px`.
- **Active Nav:** Background `#0F172A`, text `#FFFFFF`, icon accent `#34D399` (`emerald-400`).
- **Inactive Nav:** Background transparent, text `#475569`, icon `#64748B`. Hover shifts background to `#E2E8F0` with text `#0F172A`.

### Status Bar Components
- Bottom container at `24px` height with items aligned horizontally using `8px` gap, `11px` type size, and muted monochrome icons (`12px`). Houses active MCP host indicator, running agent count, workspace path, and connection latency.
