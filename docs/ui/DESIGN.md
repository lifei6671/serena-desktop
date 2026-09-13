# Serena Desktop Design System

> Version: 1.0
> Product: Serena Desktop
> Theme: Light
> Target: Windows/macOS Desktop Application
> Design Mode: Compact Developer Tool
> Status: Canonical Design Contract

---

# 1. Product Design Positioning

Serena Desktop is a desktop developer tool for managing Serena, MCP services, workspaces, remote access, Agent execution, runtime state, logs, and related local development infrastructure.

The interface should feel closer to:

* IDE companion
* Developer console
* Runtime control center
* Agent orchestration workbench
* Infrastructure management tool

rather than:

* Consumer application
* Marketing website
* Generic SaaS dashboard
* Mobile application
* Decorative AI product

The core visual qualities are:

* precise
* technical
* compact
* calm
* restrained
* professional
* operational
* information-dense
* desktop-native

Every screen should immediately look like part of the same Serena Desktop application.

---

# 2. Core Design Philosophy

Serena Desktop follows four principles.

## 2.1 Function Before Decoration

Visual elements exist to communicate:

* structure
* hierarchy
* state
* context
* operation
* feedback

Avoid decorative UI that does not improve understanding or operation.

---

## 2.2 Compact Developer-Tool Density

Serena Desktop intentionally uses higher information density than ordinary SaaS products.

Prefer:

* compact controls
* short vertical spacing
* lightweight cards
* small metadata
* dense lists
* inline actions
* technical labels

Avoid:

* oversized controls
* excessive whitespace
* huge cards
* marketing-style hero areas
* large illustrations

The interface should allow experienced users to scan substantial operational information without excessive scrolling.

---

## 2.3 Neutral Surfaces, Semantic Color

Most of the interface should remain neutral.

Use color primarily for:

* active state
* running state
* success
* warning
* failure
* connection state
* important execution feedback

Do not use large saturated surfaces simply for visual interest.

---

## 2.4 Stable Application Shell

The application shell is a design contract.

Feature pages may change their content architecture.

Feature pages must not redesign:

* title bar
* sidebar
* page header system
* typography
* buttons
* cards
* spacing
* status colors
* bottom status bar
* icon language

---

# 3. Theme

Serena Desktop currently uses one canonical theme:

```text
Light Theme
```

Do not generate a separate dark-theme design.

The native title bar may remain dark because it is part of the desktop application chrome and product identity.

This does not constitute a dark application theme.

The application body must remain light.

---

# 4. Application Shell

The following structure must be reused across all primary screens.

```text
┌──────────────────────────────────────────────────────────────┐
│ Native Title Bar                                             │
├───────────────────┬──────────────────────────────────────────┤
│                   │                                          │
│ Sidebar           │ Main Workspace                           │
│                   │                                          │
│                   │                                          │
│                   │                                          │
├───────────────────┴──────────────────────────────────────────┤
│ Status Bar                                                   │
└──────────────────────────────────────────────────────────────┘
```

---

# 5. Native Title Bar

Height:

```text
36px
```

Background:

```text
#090D16
```

Typical text:

```text
slate-300
```

Bottom border:

```text
slate-800
```

The title bar may contain:

* Serena identity
* application title
* current application context
* MCP / runtime state
* command palette trigger
* version
* native window controls

Example hierarchy:

```text
Serena Desktop
—
Agent 调度中心

MCP Core: ACTIVE
Port 9121
Ctrl+K

v1.x.x
[ minimize ] [ maximize ] [ close ]
```

The title bar must remain compact.

Do not transform it into:

* a web navigation header
* a large toolbar
* a breadcrumb area
* a marketing header

---

# 6. Sidebar

Canonical width:

```text
256px
```

Treat this value as fixed unless a future explicit design decision changes it.

Background:

```text
slate-100
```

Border:

```text
1px solid slate-200
```

Canonical structure:

```text
Product Identity

Primary Navigation

Workspace / Context Navigation

Flexible Space
```

Example:

```text
Serena
Desktop Suite

首页
服务状态
设置
日志终端
Agent 编排
远程访问通道

────────────

工作区项目

serena-desktop
  ├─ active task
  ├─ recent task
  └─ history

veyra
guard-wall
xtunnel
```

The sidebar must remain information-dense.

Do not create large navigation cards.

---

# 7. Navigation Items

## Default

Approximate height:

```text
32px
```

Typography:

```text
13px
500
```

Text:

```text
slate-600
```

Icon:

```text
slate-500
```

Radius:

```text
6px
```

Horizontal padding:

```text
10px
```

Hover:

```text
background: slate-200 / subtle
text: slate-900
```

---

## Active Navigation

Background:

```text
slate-900
```

Text:

```text
white
```

Font weight:

```text
600
```

Active icon may use:

```text
emerald-400
```

Active navigation must have strong visual differentiation.

Do not use only a subtle text-color change for the selected item.

---

# 8. Main Workspace

Default background:

```text
white
```

Canonical content max-width:

```text
1152px
```

Default desktop padding:

```text
32px
```

Compact window padding:

```text
24px
```

Default major section spacing:

```text
24px
```

Standard pages should remain centered within the workspace.

Full-width workspace layouts are permitted for:

* terminal
* logs
* code diff
* large data tables
* runtime timeline
* monitoring views
* network diagnostics

---

# 9. Bottom Status Bar

Height:

```text
24px
```

Background:

```text
slate-100
```

Top border:

```text
slate-200
```

Typography:

```text
10–11px
```

The status bar is for persistent environment information.

Examples:

```text
● Serena: 运行中

内部端口: 9121

PID: 14208
CPU: 1.8%
RAM: 72 MB
```

Good candidates:

* Serena runtime
* MCP state
* internal port
* active workspace
* PID
* CPU
* RAM
* connection status

Do not place important business actions here.

---

# 10. Color System

## Brand

```text
brand.emerald = #10B981
```

Emerald represents:

* Serena identity
* healthy runtime
* success
* connected
* clean state
* completed operation

Do not make emerald the primary button color throughout the application.

---

## Shell

```text
shell.dark    = #090D16
shell.surface = #0F172A
```

---

## Neutral System

Use the Tailwind Slate family as the canonical neutral palette.

Semantic mapping:

```text
app.background        = slate-50
main.background       = white
sidebar.background    = slate-100

surface.default       = white
surface.subtle        = slate-50
surface.hover         = slate-100
surface.active        = slate-900

border.default        = slate-200
border.hover          = slate-300
border.strong         = slate-400

text.primary          = slate-900
text.secondary        = slate-600
text.muted            = slate-500
text.subtle           = slate-400
text.inverse          = white
```

Neutral colors should account for the majority of the interface.

---

# 11. Semantic Status Colors

## Success / Healthy / Completed

Use emerald.

```text
background = emerald-50
text       = emerald-700
border     = emerald-200
indicator  = emerald-500
```

---

## Running / Processing

Use blue.

```text
background = blue-50
text       = blue-700
border     = blue-200
indicator  = blue-500 / blue-600
```

Running state may use a subtle animation.

---

## Warning

Use amber.

Use warning colors only when user attention is genuinely required.

---

## Failure / Destructive

Use red.

Suitable for:

* failure
* disconnected
* fatal error
* deletion
* destructive cancellation

Do not make destructive actions dominant before interaction.

---

## Neutral / Idle

Use slate.

Suitable for:

* stopped
* inactive
* unconfigured
* secondary state
* historical metadata

---

# 12. Typography

## Primary Font

Use native system typography:

```text
"Segoe UI",
-apple-system,
BlinkMacSystemFont,
Roboto,
"Microsoft YaHei",
sans-serif
```

---

## Monospace Font

```text
"Cascadia Code",
Consolas,
"Fira Code",
Menlo,
monospace
```

Use monospace for machine-oriented data.

Examples:

* port
* PID
* version
* path
* task ID
* Git branch
* runtime identifier
* command
* shortcut
* technical timestamps

Normal descriptive text must remain sans-serif.

---

# 13. Typography Scale

## Page Title

```text
24px
700
approximately 32px line-height
slate-900
```

---

## Section Title

```text
14px
700
slate-800 / slate-900
```

---

## Standard Body

```text
14px
400
slate-700 / slate-800
approximately 22px line-height
```

---

## Navigation

```text
13px
500
```

Active:

```text
13px
600
```

---

## Secondary Text

```text
12–13px
slate-500
```

---

## Metadata

```text
11px
slate-500
```

---

## Tiny Technical Label

```text
10–11px
```

Avoid text smaller than 10px.

---

# 14. Spacing System

Use a 4px base grid.

Canonical tokens:

```text
4px
8px
12px
16px
20px
24px
32px
```

Preferred mapping:

```text
page padding        = 24–32px
major section gap   = 24px
component gap       = 16px
card padding        = 14–16px
control gap         = 8px
compact item gap    = 4–6px
```

Avoid arbitrary spacing such as:

```text
17px
19px
23px
27px
```

unless technically necessary.

---

# 15. Radius System

Serena Desktop uses restrained rounding.

```text
radius-xs   = 4px
radius-sm   = 6px
radius-md   = 8px
radius-full = 9999px
```

Typical usage:

```text
button          = 6px
navigation      = 6px
input           = 6–8px
card            = 8px
dialog          = 8px
icon container  = 6px
badge           = 4px / full
```

Do not use 16–24px rounded cards as a default pattern.

---

# 16. Borders

Borders are one of the primary structural mechanisms in Serena Desktop.

Standard:

```text
1px solid slate-200
```

Hover:

```text
slate-300
```

Strong neutral:

```text
slate-400
```

Semantic borders:

```text
running   = blue-200
success   = emerald-200
warning   = amber-200
failure   = red-200
```

Prefer borders over heavy elevation.

---

# 17. Shadows

The UI should remain structurally flat.

Allowed:

```text
shadow-xs
shadow-sm
```

Suitable for:

* active controls
* task composer
* floating menu
* dialog
* selected interactive surface

Avoid:

* large diffuse shadows
* multi-layer Material elevation
* dramatic floating cards
* decorative shadow effects

---

# 18. Page Header Contract

Every primary page must reuse the same page-header grammar.

```text
Page Title            Optional Actions
Optional Badge

Description

──────────────────────────────────────
```

Example:

```text
Agent   [Codex Local Runner v0.153.4]

在当前工作区中创建、编排和管理本地自主 Agent 任务。

──────────────────────────────────────
```

Rules:

* Page title: 24px / bold
* Description: 14px / slate-500
* Technical badges: compact
* Actions: aligned right
* Divider: subtle

Do not invent a different header structure for each feature.

---

# 19. Cards

## Standard Card

```text
background = white
border     = slate-200
radius     = 8px
padding    = 14–16px
shadow     = none / xs
```

Interactive hover:

```text
border = slate-300
```

---

## Context Card

Suitable for:

* current workspace
* runtime context
* remote endpoint
* active service

```text
background = slate-50
border     = slate-200
```

---

## Status Card

May use extremely light semantic tint.

Example running:

```text
border     = blue-200
background = blue-50 at very low intensity
```

Completed state should normally return to neutral white.

---

# 20. Buttons

## Primary

Primary actions use neutral dark styling.

```text
background  = slate-900
text        = white
radius      = 6px
height      = 30–34px
font-size   = 12–13px
font-weight = 600
```

Hover:

```text
slate-800
```

Small icon accents may use emerald.

---

## Secondary

```text
background = white
border     = slate-200
text       = slate-700
```

Hover:

```text
background = slate-100
border     = slate-300
text       = slate-900
```

---

## Ghost

```text
background = transparent
text       = slate-600
```

Hover:

```text
background = slate-100
text       = slate-900
```

---

## Destructive

Use red only for genuinely destructive actions.

Prefer subtle destructive buttons until confirmation becomes necessary.

---

# 21. Inputs

```text
background  = white
border      = slate-200
text        = slate-800
placeholder = slate-400
font-size   = 13–14px
radius      = 6–8px
```

Focus:

```text
border     = slate-400
ring       = slate-900 / 10%
ring-width = 2px
```

Focus should be visible but restrained.

---

# 22. Agent Composer

Agent task creation uses a dedicated compound component.

```text
┌────────────────────────────────────────────┐
│                                            │
│ Describe task                             │
│                                            │
├────────────────────────────────────────────┤
│ Execution Modes        Shortcut    Execute │
└────────────────────────────────────────────┘
```

Rules:

* white surface
* slate-200 outer border
* 8px radius
* textarea integrated into container
* bottom toolbar uses slate-50
* toolbar separated by subtle border
* selected execution mode uses dark neutral state
* secondary modes use white bordered controls
* execution action uses primary button styling

It should feel like:

```text
developer command surface
```

not:

```text
consumer chat input
```

Avoid chat bubbles or social messaging aesthetics.

---

# 23. Task List

Task lists are compact operational cards.

Canonical hierarchy:

```text
Status Marker
Task Title
Workspace · Time · Duration
                                  Status

Current step / execution summary
                                  Actions
```

Semantic states:

```text
queued      → slate
running     → blue
completed   → emerald
warning     → amber
failed      → red
cancelled   → slate / red depending on context
```

Running task:

* blue status marker
* blue badge
* optional pulse
* subtle blue border
* near-neutral background

Completed task:

* emerald status marker
* emerald badge
* white card
* neutral border

Do not keep successful historical items strongly tinted.

---

# 24. Workspace UI

Workspace components may display:

* folder icon
* workspace name
* filesystem path
* Git branch
* Git cleanliness
* version
* recent tasks
* actions

Workspace name is the primary element.

Technical metadata is secondary.

Paths use monospace.

Example:

```text
CURRENT WORKSPACE     git:main (clean)

serena-desktop

E:\...\serena-desktop
```

Long paths must truncate without breaking layout.

---

# 25. Status Indicators

Runtime state is central to Serena Desktop.

Important state must combine at least two signals:

```text
color
indicator
text
optional animation
```

Examples:

```text
● Serena: 运行中

● MCP Core: ACTIVE

● 执行中

● 已完成
```

Never rely on color alone.

---

# 26. Badges

## Neutral

```text
background = slate-100
text       = slate-600
border     = slate-200
```

## Success

```text
background = emerald-50
text       = emerald-700
border     = emerald-200
```

## Running

```text
background = blue-50
text       = blue-700
border     = blue-200
```

## Warning

```text
background = amber-50
text       = amber-700
border     = amber-200
```

## Failure

```text
background = red-50
text       = red-700
border     = red-200
```

Typography:

```text
10–11px
500
```

Technical badges may use monospace.

---

# 27. Icons

Stitch may select an appropriate icon library.

The exact library is not predetermined.

However, once Stitch selects an icon system for Serena Desktop, it must remain consistent throughout the entire application.

Preferred icon characteristics:

* outline style
* geometric
* restrained
* developer-tool oriented
* approximately 1.5–2px stroke
* simple silhouettes
* consistent optical weight

Preferred sizes:

```text
12px
14px
16px
20px
```

Navigation:

```text
16px
```

Rules:

* do not mix multiple visual icon families
* do not mix filled cartoon icons with outline icons
* do not use emoji as application icons
* do not introduce different icon styles per page
* use the same metaphor consistently for the same operation

If Stitch creates the first canonical screen, its chosen icon system becomes the default icon system for subsequent screens.

---

# 28. Tables

Developer-oriented tables should remain compact.

```text
background = white
header     = slate-50
border     = slate-200
font-size  = 12–13px
```

Use subtle separators.

Prefer compact row height.

Suitable information:

* Runtime
* Port
* Process
* Connection
* Agent
* MCP tool
* Remote endpoint
* Audit event

Avoid giant table rows.

---

# 29. Tabs

Default tabs should be flat.

Preferred:

```text
text label
+
bottom indicator / subtle active state
```

Do not default to large rounded pill tabs.

Pill controls are acceptable when selecting mutually exclusive operating modes.

Active state must remain obvious.

---

# 30. Selectors and Dropdowns

Compact trigger:

```text
height      = 28–32px
background  = white
border      = slate-200
font-size   = 12–13px
radius      = 4–6px
```

Popup:

```text
background = white
border     = slate-200
radius     = 8px
shadow     = subtle
```

Menu items:

```text
28–32px height
```

---

# 31. Dialogs

Standard:

```text
width = 400–560px
```

Complex configuration:

```text
width = 560–720px
```

Structure:

```text
Title
Description

Content

──────────────────

Secondary    Primary
```

Do not create fullscreen dialogs for ordinary settings.

---

# 32. Toasts

Toasts are compact operational feedback.

Suitable messages:

```text
工作区已切换

MCP 服务启动成功

连接失败

Agent 任务已创建
```

Semantic indicator:

```text
success → emerald
error   → red
warning → amber
info    → slate / blue
```

Avoid large notification cards.

---

# 33. Interaction States

Every reusable control must support:

```text
default
hover
active
focus
disabled
loading
```

---

## Hover

Prefer:

* subtle neutral background
* subtle border change
* stronger text contrast

---

## Active

Small controls may use:

```text
scale 0.95–0.98
```

Duration:

```text
100–150ms
```

Use sparingly.

---

## Focus

Focus must remain visible and independent from selection.

Preferred:

```text
2px restrained neutral focus ring
```

---

## Disabled

```text
opacity = 0.45–0.55
```

Disabled components should not retain interactive hover behavior.

---

# 34. Motion

Motion communicates operation.

Allowed:

* loading spinner
* running pulse
* connection breathing indicator
* small hover transition
* press feedback
* collapse
* expand
* progress transition

Recommended transition:

```text
100–200ms
```

Continuous runtime animation may be longer.

Avoid:

* decorative entrance animation
* bouncing
* large spring animations
* parallax
* animated gradients
* card floating effects

---

# 35. Scrollbars

Desktop scrollbar:

```text
width = 6px
```

Track:

```text
transparent
```

Thumb:

```text
rgba(148, 163, 184, 0.4)
```

Hover:

```text
rgba(100, 116, 139, 0.7)
```

Radius:

```text
full
```

---

# 36. Responsive Behavior

Serena Desktop is desktop-first.

Primary design widths:

```text
1280
1440
1600
1920
```

The application is not designed as a mobile-responsive product.

When the desktop window narrows:

1. Hide low-priority metadata.
2. Allow page header actions to wrap.
3. Stack horizontal card sections when necessary.
4. Preserve primary operations.
5. Preserve navigation identity.
6. Avoid altering the overall design system.

Do not transform Serena Desktop into a mobile interface.

---

# 37. Information Architecture Principle

Every page should approximately follow:

```text
Application Shell

Page Identity

Current Context

Primary Operation

Primary Information

Secondary Information

Supporting Detail
```

Example Agent page:

```text
Agent

Current Workspace

New Task

Recent Tasks
```

Example Remote Access page:

```text
远程访问

Current Access State

Connection Method

Access Configuration

Endpoint / Security

Diagnostics
```

Example Service page:

```text
服务状态

Runtime Summary

MCP Core

CodeGraph

Registered Services

Diagnostics
```

Different information architecture is allowed.

Different visual systems are not.

---

# 38. Settings Pages

Settings must retain compact developer-tool density.

Avoid generic SaaS settings designs consisting of enormous cards and large empty regions.

Preferred:

```text
Settings

General
────────────────────────────────

Startup
[ ] Launch on startup

Dashboard
[ ] Enable dashboard

MCP Port
[ 9121                      ]

Logs
[ Open logs ]
```

Recommended organization:

* sections
* compact form rows
* labels
* descriptions
* inline controls
* subtle dividers

Complex settings may use secondary navigation or tabs.

---

# 39. Remote Access Pages

Remote Access should visually remain part of Serena Desktop.

It must not become an independent VPN-style consumer product.

Preferred information:

* current active method
* connection state
* public endpoint
* provider
* authentication
* MCP protection
* diagnostics
* connection actions

Important active state must be immediately visible.

Use:

```text
method
+
status
+
endpoint
```

as the top information hierarchy.

Connection options may use cards, but they must remain compact and operational.

---

# 40. Runtime / Service Pages

Runtime views should favor:

* lists
* compact cards
* tables
* status indicators
* technical metadata

Suitable structure:

```text
Service Name          Running

Port                  9121
PID                   14208
Uptime                04:31:17
CPU                   1.8%
RAM                   72 MB

[ Restart ] [ Open Logs ]
```

Technical information is a first-class part of the design.

---

# 41. Logs and Terminal

Logs may use full-width workspace layout.

Recommended:

```text
dark or neutral terminal surface
monospace
compact toolbar
filters
search
follow toggle
clear
export
```

A terminal-like component may use its own dark content surface even though the application theme is light.

The surrounding application shell remains light.

---

# 42. Empty States

Developer tools do not need large illustration-based empty states.

Preferred empty state:

```text
small icon

尚无 Agent 任务

创建任务后，执行记录会显示在这里。

[ 创建任务 ]
```

Use:

* simple icon
* short title
* concise explanation
* optional action

Avoid:

* giant illustration
* mascot
* decorative graphics
* marketing language

---

# 43. Loading States

Prefer localized loading.

Examples:

```text
spinner in button

skeleton rows

status badge

inline progress indicator
```

Avoid blocking the whole application unless application initialization genuinely prevents interaction.

---

# 44. Error States

Errors should answer:

1. What failed?
2. What is affected?
3. What can the user do?

Example:

```text
MCP Core 启动失败

端口 9121 已被其他进程占用。

PID 19842 · node.exe

[ 更改端口 ] [ 重试 ]
```

Prefer actionable technical errors over generic:

```text
Something went wrong.
```

---

# 45. Technical Metadata

Technical information is part of Serena Desktop's identity.

Examples:

* Port
* PID
* Runtime
* Workspace Path
* Git Branch
* Commit
* Agent
* MCP Server
* Version
* Provider
* Endpoint
* Execution Duration

Present technical metadata using:

* monospace
* muted text
* compact badges
* aligned values
* subtle separators

Do not hide useful technical details merely to simplify the interface.

---

# 46. Component Reuse Contract

Before generating a new visual component, Stitch must determine whether the requirement can be represented by an existing component.

Canonical reusable components include:

```text
AppShell
NativeTitleBar
Sidebar
NavigationItem
PageHeader
SectionHeader
Card
ContextCard
StatusCard
Button
IconButton
Input
Textarea
Select
Checkbox
Switch
Tabs
Badge
StatusIndicator
Table
Dialog
Toast
WorkspaceItem
TaskItem
AgentComposer
TechnicalMetadata
EmptyState
LoadingState
ErrorState
```

New components should extend this vocabulary rather than replace it.

---

# 47. Visual Drift Restrictions

Stitch must not introduce the following unless DESIGN.md is explicitly revised.

Do not introduce:

* another application theme
* new application shell
* new sidebar width
* new page header language
* new primary color
* random feature-specific colors
* gradient branding
* glassmorphism
* frosted cards
* large blur
* oversized shadows
* 16–24px default card radius
* giant cards
* mobile bottom navigation
* floating navigation
* huge page titles
* marketing hero sections
* illustration-driven UI
* excessive whitespace
* different button systems
* different icon families
* different typography systems
* arbitrary spacing scales
* feature-specific visual languages

---

# 48. New Screen Generation Contract

When generating any new Serena Desktop page:

## Step 1

Inspect existing Serena Desktop screens.

## Step 2

Preserve:

```text
App Shell
Title Bar
Sidebar
Status Bar
Page Header
Typography
Spacing
Colors
Radius
Buttons
Inputs
Cards
Status Semantics
Icons
Density
```

## Step 3

Design only the information architecture required by the new feature.

## Step 4

Reuse existing components before creating new ones.

## Step 5

If a new component is necessary, create the smallest extension consistent with this document.

---

# 49. Stitch-Specific Instruction

When this document is provided to Stitch, treat it as the authoritative project design specification.

Do not treat every prompt as a request to redesign the application.

A prompt such as:

```text
Design the Remote Access page.
```

means:

```text
Design the information architecture and page content for Remote Access inside the existing Serena Desktop design system.
```

It does not mean:

```text
Create a new visual interpretation of a Remote Access product.
```

Always reuse existing Serena Desktop visual conventions.

---

# 50. Canonical Prompt Prefix

For important screens, prepend the following instruction:

```text
This screen belongs to the existing Serena Desktop application.

Strictly follow the Serena Desktop DESIGN.md.

Preserve the existing application shell, sidebar, title bar,
status bar, typography, spacing, colors, radius, controls,
information density, icon system, status semantics and page hierarchy.

This is a compact professional developer tool.

Do not redesign shared components.

Do not introduce a new visual language.

Only design the information architecture and interactions required
by this feature.
```

---

# 51. Canonical Reference Screen

The Serena Desktop Agent orchestration screen is the initial visual reference screen.

It defines the baseline for:

* App Shell
* Native Title Bar
* Sidebar
* Navigation
* Page Header
* Workspace Context
* Agent Composer
* Task List
* Status Badges
* Runtime Metadata
* Bottom Status Bar
* Information Density
* Interaction Scale

Future screens should visually appear to have been designed by the same product team using the same component library.

---

# 52. Serena Desktop Visual Identity

The Serena Desktop visual identity can be summarized as:

```text
Windows-native shell
+
light neutral workspace
+
compact developer-tool density
+
slate structural palette
+
emerald Serena identity
+
blue execution state
+
monospace technical metadata
+
restrained borders
+
small radius
+
minimal shadow
+
clear operational hierarchy
```

When uncertain, choose the solution that feels more like:

```text
IDE / Developer Console / Infrastructure Tool
```

and less like:

```text
SaaS Dashboard / Consumer App / Marketing Product
```

---

# 53. Final Rule

Feature complexity may increase.

Information density may increase.

Serena Desktop may gain:

* more services
* more MCP integrations
* more Agents
* more remote access providers
* more diagnostics
* more workspace capabilities

The visual language must remain stable.

New functionality should look like an extension of Serena Desktop, not a newly designed application.