# Small-window layout experiment

Branch: `codex/editor-compact-layout`. This is not merged into the normal 0.1.49 build.

Save, Reload, profile/JSON/tools actions and draft status stay in a fixed footer. The hierarchy/sidebar and item fields have independently clipped scroll containers. Hide preview frees its column and reduces the minimum content width; Show preview restores it. Layout preferences are session-only and do not rewrite configuration.

Review manually: resize/restore by dragging, scroll one pane without moving the other, reach every item field, use Save/Undo/shortcuts while text is focused, switch universes, toggle preview with invalid text, and move between DPI scales. Nothing should discard drafts or add hover delays. Native interactive checks have not been run locally.
