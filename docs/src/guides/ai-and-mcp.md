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

### 3.1 Recommended MCP Setup for CraftUI
You can expose CraftUI CLI tools to any MCP client (such as Claude Desktop, Cursor Agent, or Antigravity) using the standard command execution bridge.

In your MCP configuration (`claude_desktop_config.json` or your agent's MCP settings):

```json
{
  "mcpServers": {
    "craftui": {
      "command": "cargo",
      "args": ["run", "-p", "craftui-cli", "--"]
    }
  }
}
```

### 3.2 What AI Can Do via MCP
1. **Validate `.cui` syntax on the fly**:
   An AI agent can execute:
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
