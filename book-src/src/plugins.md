# Plugins

Smaragd can be extended with small scripts written in [Rhai](https://rhai.rs), an embedded scripting language. A plugin script can:

1. Register a custom `:` command
2. Define an `on_save(text)` hook that transforms a document's text right before an explicit save
