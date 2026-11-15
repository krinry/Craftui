# Developing with AI & MCP in CraftUI

CraftUI provides first-class support for AI coding assistants, autonomous agents, and the **Model Context Protocol (MCP)**. This guide explains how to configure and use AI tools like Cursor, Claude, Antigravity, and Copilot with CraftUI.

---

## 1. Why AI Needs Special Context for CraftUI

CraftUI uses a custom declarative DSL (`.cui`) compiled directly to Rust. General-purpose Large Language Models (LLMs) trained on public internet data often default to familiar web frameworks (HTML, JSX, React) or Flutter if not provided with explicit rules.

To prevent hallucinations, CraftUI ships with:
- **`llms.txt`** (Repository root): Compact index for LLMs and web crawlers.
- **`docs/llms-full.txt`**: Complete, dense context specification designed to fit in a single prompt or system instruction.
- **`.cursorrules`**: Automatic rules for Cursor and Windsurf IDEs.
- **`.github/copilot-instructions.md`**: Context for GitHub Copilot.
- **`.agents/skills/craftui/SKILL.md`**: Specialized skill for autonomous agents.

---

## 2. Using AI Assistants

### Cursor & Windsurf
When opening this repository in Cursor or Windsurf, `.cursorrules` is automatically loaded. When working in your own external project, copy `.cursorrules` from the CraftUI repository into your project root.

You can ask:
> "Create a responsive settings screen with a dark mode toggle and volume slider in CraftUI."

The AI will output syntactically valid `.cui` code using `Column`, `Row`, `Switch`, `Slider`, and `Theme` tokens instead of HTML or Flutter tags.

### Claude Desktop, ChatGPT & Gemini
If you are using Claude Projects, Custom GPTs, or Gemini Gems, attach or paste:
1. [`docs/llms-full.txt`](../../llms-full.txt) into your Project Knowledge or Custom Instructions.
2. Prompt:
   > "You are an expert CraftUI developer. When asked for UI code, write .cui components and reactive Rust controllers following the CraftUI specification in docs/llms-full.txt."

---

## 3. Model Context Protocol (MCP) Integration

The **Model Context Protocol (MCP)** allows AI models to interact directly with local developer tools, run validations, and inspect components.

### 3.1 Native MCP Server Configuration
CraftUI CLI ships with a built-in, native MCP server over `stdio` (`craftui mcp --stdio`).

To connect CraftUI to Claude Desktop, Cursor, or any MCP client, add the following to your MCP configuration (e.g., `claude_desktop_config.json` or `.cursor/mcp.json`):

```json
{
  "mcpServers": {
    "craftui": {
      "command": "craftui",
      "args": ["mcp", "--stdio"]
    }
  }
}
```

Or when running from the source repository:

```json
{
  "mcpServers": {
    "craftui": {
      "command": "cargo",
      "args": ["run", "-p", "craftui-cli", "--", "mcp", "--stdio"]
    }
  }
}
```

### 3.2 Available Native MCP Tools

| Tool | Description | Input Parameters |
| :--- | :--- | :--- |
| `craftui_validate` | Real-time parser & validator returning exact line/column coordinates and fix hints | `code` (string), `path` (optional) |
| `craftui_widget_catalog` | Machine-readable schema of all widgets, allowed properties, types, and event callbacks | `widget` (optional filter) |
| `craftui_scaffold` | Generates 100% valid `.cui` templates (`counter`, `login_form`, `settings_card`, `search_filter`) and Rust controllers | `template`, `name` |
| `craftui_render_preview` | Headlessly renders `.cui` to an RGBA PNG snapshot via `vello_cpu` and returns base64 image data | `code`, `width`, `height`, `out_path` |
| `craftui_doctor` | Inspects toolchain readiness (Rust compiler, wasm32, android NDK, wasm-bindgen) | `path` |
| `craftui_explain` | Architectural explanations for reactivity, layout, two-way bindings, and animations | `topic` |

### 3.3 Available MCP Resources & Prompts

- **Resources**:
  - `craftui://docs/cheatsheet`: Complete one-page syntax & property reference.
  - `craftui://docs/llms-full`: Full token-optimized specification for AI context windows.
  - `craftui://templates/counter`: Reactive counter starter component.
  - `craftui://templates/login`: Production login form template.
- **Prompts**:
  - `craftui_new_screen`: Guided workflow for creating new screens.
  - `craftui_debug_component`: Guided debugging workflow for fixing compilation errors.
   ```bash
   craftui check ui/main.cui --json
   ```
   If any syntax errors or missing properties occur, the compiler returns structured JSON errors with line and column numbers. The AI can automatically read and self-correct its code before presenting it to you.

2. **Generate and inspect PNG UI previews**:
   ```bash
   craftui preview ui/main.cui --device
   ```
   AI agents with vision capabilities can inspect the rendered screenshot to verify layout alignments, padding, and theming.

3. **Check platform permissions**:
   ```bash
   craftui permissions check
   ```
   Audits `craftui.toml` to ensure required mobile and web capabilities are properly declared.

---

## 4. Prompting Best Practices for Humans

When prompting any AI to write CraftUI code:
1. **Mention the file extension**: Explicitly ask for a `.cui` component.
2. **Mention Theme tokens**: Ask the model to use `Theme.background`, `Theme.primary`, etc., for seamless dark/light mode support.
3. **Specify the Rust controller**: Ask for both the `.cui` screen and the Rust controller struct with `cx: Scope` and signals.
