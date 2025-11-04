# UniCore Procedural Macros

This module provides procedural macros used throughout the UniCore codebase to reduce boilerplate and ensure consistency.

## Macros

### `#[derive(Config)]`

A procedural macro that automatically generates configuration loading code for structs. This macro enables:

- Loading configuration values from provisioned sources
- Applying environment-specific defaults (development, production)
- Struct validation
- Support for custom transformation functions

#### Usage

```rust
#[derive(Config)]
struct MyConfig {
    #[config(default = "42", development_default = "100")]
    value: u32,
    
    #[config(production_default = "https://prod.example.com")]
    url: String,
}
```

#### Attributes

- `default = "value"`: Default value for all environments
- `development_default = "value"`: Default value for development environment
- `production_default = "value"`: Default value for production environment
- `transform_with = "function"`: Custom transformation function to apply

This macro is essential for UniCore's configuration system, providing a declarative way to define environment-specific configuration with sensible defaults.