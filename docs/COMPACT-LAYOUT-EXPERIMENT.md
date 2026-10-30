# Small-window layout experiment

Branch: `codex/editor-compact-layout`. Current experiment: 0.1.53-layout.1, updated with 0.1.52 and the CI qualification improvements. This is not merged into main.

Save, Reload, profile/JSON/tools actions and draft status stay in a fixed footer. The hierarchy/sidebar and item fields have independently clipped scroll containers. Hide preview frees its column and reduces the minimum content width; Show preview restores it. Layout preferences are session-only and do not rewrite configuration.

Review manually: resize/restore by dragging, scroll one pane without moving the other, reach every item field with Tab, use Save/Undo/shortcuts while text is focused, switch universes, toggle preview with invalid text, and move between DPI scales. Scrolling over the preview must leave both pane positions intact. Nothing should discard drafts or add hover delays. Native interactive checks have not been run locally.
