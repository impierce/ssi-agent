use super::error::TemplateError;

fn ob_system_managed_fields() -> &'static [&'static str] {
    &["id", "type"]
}

/// Dynamically builds the path → OB 3.0 `$def` name mapping by following `$ref` links
/// in the OB JSON Schema, starting from `AchievementSubject`.
///
/// Covers all `$ref`-reachable defs at any nesting depth. Cycles are detected via a
/// DFS stack that tracks def names currently being processed, preventing infinite
/// recursion on circular references such as `Profile.parentOrg → Profile`.
fn ob_build_path_to_def(defs: &serde_json::Map<String, serde_json::Value>) -> Vec<(String, String)> {
    let mut result = Vec::new();
    let mut def_stack = std::collections::HashSet::new();
    ob_collect_path_to_def_recursive(defs, "AchievementSubject", "", &mut result, &mut def_stack);
    result
}

fn ob_collect_path_to_def_recursive(
    defs: &serde_json::Map<String, serde_json::Value>,
    def_name: &str,
    path: &str,
    result: &mut Vec<(String, String)>,
    def_stack: &mut std::collections::HashSet<String>,
) {
    if !def_stack.insert(def_name.to_string()) {
        return; // Cycle detected — this def is already being processed in the current DFS path.
    }

    result.push((path.to_string(), def_name.to_string()));

    if let Some(props) = defs
        .get(def_name)
        .and_then(|def| def.get("properties"))
        .and_then(|p| p.as_object())
    {
        for (prop_name, prop_schema) in props {
            if let Some(next_def) = prop_schema
                .get("$ref")
                .and_then(|r| r.as_str())
                .and_then(|r| r.strip_prefix("#/$defs/"))
            {
                let child_path = if path.is_empty() {
                    prop_name.clone()
                } else {
                    format!("{}/{}", path, prop_name)
                };
                ob_collect_path_to_def_recursive(defs, next_def, &child_path, result, def_stack);
            }
        }
    }

    def_stack.remove(def_name);
}

/// Derives the required child keys for each schema path from the OB 3.0 JSON Schema
/// `$defs` `required` arrays, excluding system-managed fields.
/// Returns a map of `schema_path → required_children`.
fn ob_spec_required_by_path() -> std::collections::HashMap<String, Vec<String>> {
    let ob_schema = ob_json_schema();
    let Some(defs) = ob_schema.get("$defs").and_then(|d| d.as_object()) else {
        return Default::default();
    };
    let system_managed = ob_system_managed_fields();
    ob_build_path_to_def(defs)
        .iter()
        .filter_map(|(path, def_name)| {
            let required: Vec<String> = defs
                .get(def_name.as_str())
                .and_then(|def| def.get("required"))
                .and_then(|r| r.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str())
                        .filter(|k| !system_managed.contains(k))
                        .map(|k| k.to_string())
                        .collect()
                })
                .unwrap_or_default();
            if required.is_empty() {
                None
            } else {
                Some((path.clone(), required))
            }
        })
        .collect()
}

/// Additional required child keys that go beyond the OB 3.0 specification.
/// These are UniCore-opinionated requirements that guide API callers toward producing
/// meaningful credentials without needing to know OB specifics themselves.
///
/// Adding an entry here is the single place to extend UniCore's required-field policy
/// without touching validation, schema-injection, or `non_removable` logic.
fn ob_opinionated_required_by_path() -> &'static [(&'static str, &'static [&'static str])] {
    &[
        // `narrative` is not required by the OB 3.0 spec (`Criteria.required = []`)
        // but is required by UniCore so templates always guide callers to provide
        // meaningful criteria text.
        ("achievement/criteria", &["narrative"]),
    ]
}

/// UniCore-opinionated synthetic `$def`s keyed by schema path, for object paths that OB 3.0
/// itself leaves unvalidated.
///
/// `AchievementSubject` declares `"additionalProperties": true`, so a `profile` object is
/// permitted on the subject root to carry the recipient's OB 3.0 `Profile` data - all other
/// homes for the recipient's profile seemed unsuitable. No spec `$def` governs it, so instead
/// of leaving its interior open, UniCore constrains it to exactly these four fields with enforced
/// types: `givenName`/`familyName`/`email`/`dateOfBirth` are all strings, `email` carries
/// `format: "email"` and `dateOfBirth` carries `format: "date"`.
///
/// Each entry is an exact pin, not a lower bound: a field whose def declares no `format` must not
/// carry one either. A caller-supplied format on `givenName`/`familyName` is therefore rejected —
/// UniCore decides what these fields mean, and an unvetted `format` would let callers constrain
/// them in ways UniCore never agreed to (e.g. `format: "email"` on `givenName`).
///
/// This is the single source of truth for both the allowed property set (its presence adds
/// `profile` to the subject root's allowed keys, see [`validate_ob_properties_recursive`]) and
/// the interior name/type validation ([`validate_ob_opinionated_types`]).
fn ob_opinionated_defs() -> &'static std::collections::HashMap<String, serde_json::Value> {
    static DEFS: std::sync::OnceLock<std::collections::HashMap<String, serde_json::Value>> = std::sync::OnceLock::new();
    DEFS.get_or_init(|| {
        let mut defs = std::collections::HashMap::new();
        defs.insert(
            "profile".to_string(),
            serde_json::json!({
                "type": "object",
                "properties": {
                    "givenName": { "type": "string" },
                    "familyName": { "type": "string" },
                    "email": { "type": "string", "format": "email" },
                    "dateOfBirth": { "type": "string", "format": "date" }
                }
            }),
        );
        defs
    })
}

/// Splits a schema path into its parent path and final segment, e.g. `"profile"` → `("", "profile")`
/// and `"profile/address"` → `("profile", "address")`.
fn split_parent_path(path: &str) -> (&str, &str) {
    match path.rfind('/') {
        Some(i) => (&path[..i], &path[i + 1..]),
        None => ("", path),
    }
}

/// Navigates through `properties` segments to the schema node at `path`, i.e. the node that
/// declares the `type` of the field itself, not its children. Returns `None` if the path is
/// absent from the schema.
fn node_at_path<'a>(schema: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let mut current = schema;
    for seg in path.split('/').filter(|s| !s.is_empty()) {
        current = current.get("properties")?.as_object()?.get(seg)?;
    }
    Some(current)
}

/// Navigates through `properties` segments to the properties map at `path`.
fn props_at_path<'a>(
    schema: &'a serde_json::Value,
    path: &str,
) -> Option<&'a serde_json::Map<String, serde_json::Value>> {
    node_at_path(schema, path)?.get("properties")?.as_object()
}

/// Returns the combined required child keys per schema path: OB 3.0 spec + UniCore extras.
fn ob_combined_required_by_path() -> std::collections::HashMap<String, Vec<String>> {
    let mut combined = ob_spec_required_by_path();
    for (path, children) in ob_opinionated_required_by_path() {
        let entry = combined.entry(path.to_string()).or_default();
        for &child in *children {
            let child = child.to_string();
            if !entry.contains(&child) {
                entry.push(child);
            }
        }
    }
    combined
}

/// Returns the set of paths in `combined` that are reachable from the root by following
/// only required-child links. For these paths, absence from the template schema implies
/// that required children are genuinely missing. For all other paths, required-field
/// checking only applies when the path IS explicitly present in the schema.
fn ob_required_reachable_paths(
    combined: &std::collections::HashMap<String, Vec<String>>,
) -> std::collections::HashSet<String> {
    let mut reachable = std::collections::HashSet::new();
    reachable.insert(String::new()); // root is always reachable
    let mut queue = vec![String::new()];
    while let Some(path) = queue.pop() {
        if let Some(children) = combined.get(&path) {
            for child in children {
                let child_path = if path.is_empty() {
                    child.clone()
                } else {
                    format!("{}/{}", path, child)
                };
                // Only follow into a child path if it also has required children itself.
                if combined.contains_key(&child_path) && reachable.insert(child_path.clone()) {
                    queue.push(child_path);
                }
            }
        }
    }
    reachable
}

/// Returns the JSON Pointer paths (RFC 6901) of the required leaf fields for OB 3.0
/// templates, derived from the combined spec + opinionated requirements.
///
/// A required child is a leaf if its property in the corresponding `$def` is not a
/// `$ref` to another `$def` (i.e. it is a scalar or inline schema, not a nested object).
/// Opinionated extras without a `$def` entry are always treated as leaves.
///
/// These paths are used to mark the corresponding `PropertyAttribute` entries as
/// `non_removable = true`.
pub fn open_badges_required_leaf_paths() -> Vec<String> {
    let combined = ob_combined_required_by_path();
    let ob_schema = ob_json_schema();
    let defs = ob_schema.get("$defs").and_then(|d| d.as_object());

    let path_to_def: std::collections::HashMap<String, String> = defs
        .map(|d| ob_build_path_to_def(d).into_iter().collect())
        .unwrap_or_default();

    let mut leaf_paths: Vec<String> = combined
        .iter()
        .flat_map(|(path, children)| {
            let def_name = path_to_def.get(path.as_str()).map(|d| d.as_str());
            let def_props = def_name
                .and_then(|n| defs.and_then(|d| d.get(n)))
                .and_then(|def| def.get("properties"))
                .and_then(|p| p.as_object());

            children.iter().filter_map(move |child| {
                // A property is an intermediate node when the spec $def references it via $ref.
                let is_intermediate = def_props
                    .and_then(|p| p.get(child.as_str()))
                    .map(|prop_schema| prop_schema.get("$ref").is_some())
                    .unwrap_or(false);

                if is_intermediate {
                    None
                } else {
                    let full_path = if path.is_empty() {
                        format!("/{}", child)
                    } else {
                        format!("/{}/{}", path, child)
                    };
                    Some(full_path)
                }
            })
        })
        .collect();

    leaf_paths.sort();
    leaf_paths
}

/// Returns a lazily-initialised reference to the parsed OpenBadges 3.0 JSON Schema.
fn ob_json_schema() -> &'static serde_json::Value {
    static OB_SCHEMA: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    OB_SCHEMA.get_or_init(|| {
        serde_json::from_str(include_str!("../json_schemas/OpenBadgeCredentialV3.json"))
            .expect("OpenBadgeCredentialV3.json must be valid JSON")
    })
}

/// Validates that the nested OB template schema only uses property names that are valid
/// according to the OpenBadges 3.0 JSON Schema specification.
///
/// Starts at `AchievementSubject` and follows `$ref` links to determine the allowed
/// property set at each nesting level. Any property not present in the resolved `$def`
/// is rejected. Unknown nesting levels (no `$ref` in the parent def) are left open.
pub(crate) fn validate_open_badges_schema_properties(schema: &serde_json::Value) -> Result<(), TemplateError> {
    let ob_schema = ob_json_schema();
    let defs = match ob_schema.get("$defs").and_then(|d| d.as_object()) {
        Some(d) => d,
        None => return Ok(()), // Cannot validate without defs; pass through.
    };
    let root_def = defs.get("AchievementSubject");
    validate_ob_properties_recursive(schema, "", root_def, defs)?;
    validate_ob_opinionated_types(schema)
}

/// Reports whether a schema node matches the `type`/`format` a synthetic def declares.
/// Both are compared exactly: a def that declares no `format` requires the node to have none
/// either, so callers cannot attach a format UniCore has not sanctioned.
fn schema_matches_def(actual: &serde_json::Value, expected: &serde_json::Value) -> bool {
    actual.get("type") == expected.get("type") && actual.get("format") == expected.get("format")
}

/// Enforces the declared `type`/`format` of UniCore-opinionated fields (e.g. the recipient
/// `profile` object) that OB 3.0 leaves unvalidated. Each node that is present must match the
/// synthetic def in [`ob_opinionated_defs`]; absent (optional) nodes and fields are skipped.
///
/// The node at an opinionated path is checked before its children, so a `profile` declared as
/// anything other than an object is rejected rather than silently skipped for lack of children.
fn validate_ob_opinionated_types(schema: &serde_json::Value) -> Result<(), TemplateError> {
    let mut mismatched: Vec<String> = Vec::new();

    for (path, def) in ob_opinionated_defs() {
        let Some(node) = node_at_path(schema, path) else {
            continue; // Optional node — absence is fine; only the shape is constrained.
        };

        if !schema_matches_def(node, def) {
            mismatched.push(format!("/{path}"));
            continue; // Not the declared shape — its children are not meaningful to check.
        }

        let (Some(actual), Some(expected)) = (
            node.get("properties").and_then(|p| p.as_object()),
            def.get("properties").and_then(|p| p.as_object()),
        ) else {
            continue;
        };

        for (field, expected_schema) in expected {
            let Some(actual_schema) = actual.get(field) else {
                continue; // Optional field — absence is fine; only the shape is constrained.
            };
            if !schema_matches_def(actual_schema, expected_schema) {
                mismatched.push(format!("/{path}/{field}"));
            }
        }
    }

    if !mismatched.is_empty() {
        mismatched.sort();
        return Err(TemplateError::InvalidOpenBadgesPropertyType(format!(
            "The following fields do not match the required type/format: [{}]",
            mismatched.join(", ")
        )));
    }
    Ok(())
}

fn validate_ob_properties_recursive(
    schema: &serde_json::Value,
    current_path: &str,
    current_def: Option<&serde_json::Value>,
    defs: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), TemplateError> {
    let properties = match schema.get("properties").and_then(|p| p.as_object()) {
        Some(p) => p,
        None => return Ok(()),
    };

    // Determine the set of allowed property names at this level from the current def.
    let mut allowed: Option<std::collections::HashSet<&str>> = current_def
        .and_then(|def| def.get("properties").and_then(|p| p.as_object()))
        .map(|props| props.keys().map(|k| k.as_str()).collect());

    // Merge in UniCore's opinionated synthetic defs that live directly under this path
    // (e.g. `profile` under the root), so they are permitted alongside the spec properties.
    if let Some(ref mut allowed_set) = allowed {
        for opinionated_path in ob_opinionated_defs().keys() {
            let (parent, key) = split_parent_path(opinionated_path);
            if parent == current_path {
                allowed_set.insert(key);
            }
        }
    }

    let mut disallowed: Vec<&str> = Vec::new();

    for (key, value) in properties {
        if let Some(ref allowed_set) = allowed {
            if !allowed_set.contains(key.as_str()) {
                disallowed.push(key.as_str());
            }
        }

        let child_path = if current_path.is_empty() {
            key.to_string()
        } else {
            format!("{}/{}", current_path, key)
        };

        // Resolve the child def by following the $ref in the current def's properties, falling
        // back to a UniCore-opinionated synthetic def (e.g. `profile`) where the spec has none.
        // This restricts the interior of opinionated objects to their declared property set.
        let child_def = current_def
            .and_then(|def| def.get("properties").and_then(|p| p.as_object()))
            .and_then(|props| props.get(key.as_str()))
            .and_then(|prop_schema| prop_schema.get("$ref").and_then(|r| r.as_str()))
            .and_then(|r| r.strip_prefix("#/$defs/"))
            .and_then(|def_name| defs.get(def_name))
            .or_else(|| ob_opinionated_defs().get(&child_path));

        validate_ob_properties_recursive(value, &child_path, child_def, defs)?;
    }

    if !disallowed.is_empty() {
        disallowed.sort();
        return Err(TemplateError::DisallowedOpenBadgesProperties(format!(
            "The following properties are not allowed for OpenBadges 3.0 templates at path `/{current_path}`: [{}]",
            disallowed.join(", ")
        )));
    }

    Ok(())
}

/// Validates that all required OB 3.0 leaf fields (spec-required + UniCore opinionated)
/// are present in the template schema, and that each leaf has `type: "string"` or a `const`.
///
/// Required fields are derived from `ob_combined_required_by_path()` so this function
/// automatically reflects any changes to the spec JSON file or the opinionated extras list.
pub(crate) fn validate_open_badges_required_properties(schema: &serde_json::Value) -> Result<(), TemplateError> {
    let combined = ob_combined_required_by_path();
    let required_reachable = ob_required_reachable_paths(&combined);
    let leaf_paths: std::collections::HashSet<String> = open_badges_required_leaf_paths().into_iter().collect();

    let mut missing: Vec<String> = Vec::new();
    let mut wrong_type: Vec<String> = Vec::new();
    // Tracks missing intermediate nodes so that their children are not cascade-reported.
    let mut missing_nodes: std::collections::HashSet<String> = std::collections::HashSet::new();

    let mut sorted: Vec<(String, Vec<String>)> = combined.into_iter().collect();
    sorted.sort_by_key(|(p, _)| p.clone());

    for (path, mut children) in sorted {
        children.sort();

        // Skip this path if any ancestor was already reported as missing.
        let skip = {
            let mut ancestor = String::new();
            path.split('/').filter(|s| !s.is_empty()).any(|seg| {
                if ancestor.is_empty() {
                    ancestor = seg.to_string();
                } else {
                    ancestor = format!("{ancestor}/{seg}");
                }
                missing_nodes.contains(&ancestor)
            })
        };
        if skip {
            continue;
        }

        match props_at_path(schema, &path) {
            None => {
                if !required_reachable.contains(&path) {
                    continue; // Optional path absent from template schema — skip silently.
                }
                // Required-reachable path absent — report each required child as missing.
                for child in &children {
                    let full = if path.is_empty() {
                        format!("/{child}")
                    } else {
                        format!("/{path}/{child}")
                    };
                    missing.push(full);
                    let node_path = if path.is_empty() {
                        child.to_string()
                    } else {
                        format!("{path}/{child}")
                    };
                    missing_nodes.insert(node_path);
                }
            }
            Some(props) => {
                for child in &children {
                    let full = if path.is_empty() {
                        format!("/{child}")
                    } else {
                        format!("/{path}/{child}")
                    };
                    match props.get(child.as_str()) {
                        None => {
                            missing.push(full);
                            let node_path = if path.is_empty() {
                                child.to_string()
                            } else {
                                format!("{path}/{child}")
                            };
                            missing_nodes.insert(node_path);
                        }
                        Some(field) if leaf_paths.contains(&full) => {
                            let has_string = field.get("type").and_then(|t| t.as_str()) == Some("string");
                            let has_const = field.get("const").is_some();
                            if !has_string && !has_const {
                                wrong_type.push(full);
                            }
                        }
                        _ => {} // Intermediate node — presence is sufficient.
                    }
                }
            }
        }
    }

    if !missing.is_empty() {
        return Err(TemplateError::MissingRequiredOpenBadgesProperties(format!(
            "The following required fields must be present in the schema for OpenBadges 3.0 templates: [{}]",
            missing.join(", ")
        )));
    }
    if !wrong_type.is_empty() {
        return Err(TemplateError::InvalidRequiredPropertyType(format!(
            "The following required fields must have type \"string\" or a \"const\" value: [{}]",
            wrong_type.join(", ")
        )));
    }
    Ok(())
}

/// Injects `required` arrays into the OB 3.0 template schema at every nesting level
/// that has required children (spec-required + UniCore opinionated extras).
///
/// Only injects a key if the corresponding property already exists in the schema at
/// that level — missing optional properties are not forced into `required`.
pub(crate) fn ensure_schema_required_keys(schema: &mut serde_json::Value) {
    fn add_if_absent(arr: &mut serde_json::Value, key: &str) {
        if let Some(arr) = arr.as_array_mut() {
            let v = serde_json::Value::String(key.to_string());
            if !arr.contains(&v) {
                arr.push(v);
            }
        }
    }

    // Navigate to a mutable node by following properties/segment pairs.
    fn node_at_path_mut<'a>(schema: &'a mut serde_json::Value, path: &str) -> Option<&'a mut serde_json::Value> {
        if path.is_empty() {
            return Some(schema);
        }
        let mut current = schema;
        for seg in path.split('/') {
            current = current.get_mut("properties")?.get_mut(seg)?;
        }
        Some(current)
    }

    let combined = ob_combined_required_by_path();
    let mut sorted: Vec<(String, Vec<String>)> = combined.into_iter().collect();
    sorted.sort_by_key(|(p, _)| p.clone());

    for (path, mut required_children) in sorted {
        required_children.sort();
        let Some(node) = node_at_path_mut(schema, &path) else {
            continue;
        };
        // Only mark a child as required when it actually appears in the schema at this level.
        let present: Vec<String> = required_children
            .into_iter()
            .filter(|k| node.get("properties").and_then(|p| p.get(k.as_str())).is_some())
            .collect();
        if present.is_empty() {
            continue;
        }
        let required_entry = node
            .as_object_mut()
            .map(|o| o.entry("required").or_insert(serde_json::json!([])));
        if let Some(r) = required_entry {
            for key in &present {
                add_if_absent(r, key);
            }
        }
    }
}
