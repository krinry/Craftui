# GitHub Copilot Instructions for CraftUI

When generating or editing code in this repository or in any CraftUI application:

- **Language & Framework**:
  - UI screens are written in `.cui` declarative markup.
  - Core framework and backend application logic are written in modern Rust (edition 2024).
- **`.cui` Markup Rules**:
  - Components start with `export component Name { ... }`.
  - Properties: `in-out property <type> name: default;` (`int`, `float`, `string`, `bool`, `color`, `length`).
  - Layout containers: `Column` (vertical), `Row` (horizontal), `Stack` (layered), `Grid`, `ScrollView`, `SafeArea`.
  - Primitives: `Text`, `Button`, `TextInput`, `TextArea`, `Checkbox`, `Switch`, `Slider`, `Select`, `Icon`, `Image`, `Card`, `Modal`.
  - Events: `on_tap => { ... }`, `on_change(val) => { ... }`.
  - Two-way bindings: `value <=> state_variable;`.
  - End every property line with `;`.
- **Colors & Styles**:
  - Use semantic theme tokens: `Theme.background`, `Theme.surface`, `Theme.primary`, `Theme.on_primary`, `Theme.text_primary`, `Theme.text_secondary`, `Theme.border`, `Theme.danger`.
- **Rust Architecture**:
  - Use `craftui::prelude::*`.
  - Reactivity uses `Scope` (`cx`), `Signal<T>`, `Computed<T>`, and `cx.effect(...)`.
  - When wiring UI with Rust, use the generated screen struct (e.g. `AppScreen::new(cx)`) and bind properties with `.bind(signal)`.
- **Testing & Verification**:
  - Always run `cargo check --workspace --all-targets` and `cargo test --workspace` to ensure zero compile warnings and 100% test passing.
