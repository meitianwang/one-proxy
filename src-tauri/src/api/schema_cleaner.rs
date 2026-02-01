use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};

const PLACEHOLDER_REASON_DESCRIPTION: &str = "Brief explanation of why you are calling this tool";

const UNSUPPORTED_CONSTRAINTS: [&str; 10] = [
    "minLength",
    "maxLength",
    "exclusiveMinimum",
    "exclusiveMaximum",
    "pattern",
    "minItems",
    "maxItems",
    "format",
    "default",
    "examples",
];

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
enum PathSegment {
    Key(String),
    Index(usize),
}

pub fn clean_json_schema_for_antigravity(value: &Value) -> Value {
    let mut v = value.clone();
    clean_json_schema(&mut v, true);
    v
}

pub fn clean_json_schema_for_gemini(value: &Value) -> Value {
    let mut v = value.clone();
    clean_json_schema(&mut v, false);
    v
}

fn clean_json_schema(value: &mut Value, add_placeholder: bool) {
    convert_refs_to_hints(value);
    convert_const_to_enum(value);
    convert_enum_values_to_strings(value);
    add_enum_hints(value);
    add_additional_properties_hints(value);
    move_constraints_to_description(value, false);
    merge_all_of(value);
    flatten_anyof_oneof(value);
    flatten_type_arrays(value);
    remove_unsupported_keywords(value, false);
    if !add_placeholder {
        remove_keywords(value, false, &["nullable", "title"]);
        remove_placeholder_fields(value);
    }
    cleanup_required_fields(value);
    if add_placeholder {
        add_empty_schema_placeholder(value, &mut Vec::new());
    }
}

fn convert_refs_to_hints(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if let Some(ref_val) = map.get("$ref").and_then(|v| v.as_str()) {
                let def_name = ref_val
                    .rsplit('/')
                    .next()
                    .unwrap_or(ref_val)
                    .to_string();
                let mut hint = format!("See: {}", def_name);
                if let Some(existing) = map.get("description").and_then(|v| v.as_str()) {
                    if !existing.is_empty() {
                        hint = format!("{} ({})", existing, hint);
                    }
                }
                *value = json!({
                    "type": "object",
                    "description": hint
                });
                return;
            }
            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                if let Some(child) = map.get_mut(&key) {
                    convert_refs_to_hints(child);
                }
            }
        }
        Value::Array(arr) => {
            for item in arr {
                convert_refs_to_hints(item);
            }
        }
        _ => {}
    }
}

fn convert_const_to_enum(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if map.contains_key("const") && !map.contains_key("enum") {
                if let Some(val) = map.get("const").cloned() {
                    map.insert("enum".to_string(), Value::Array(vec![val]));
                }
            }
            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                if let Some(child) = map.get_mut(&key) {
                    convert_const_to_enum(child);
                }
            }
        }
        Value::Array(arr) => {
            for item in arr {
                convert_const_to_enum(item);
            }
        }
        _ => {}
    }
}

fn convert_enum_values_to_strings(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if let Some(Value::Array(enum_arr)) = map.get_mut("enum") {
                let strings: Vec<Value> = enum_arr
                    .iter()
                    .map(|v| Value::String(value_to_string(v)))
                    .collect();
                *enum_arr = strings;
                map.insert("type".to_string(), Value::String("string".to_string()));
            }
            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                if let Some(child) = map.get_mut(&key) {
                    convert_enum_values_to_strings(child);
                }
            }
        }
        Value::Array(arr) => {
            for item in arr {
                convert_enum_values_to_strings(item);
            }
        }
        _ => {}
    }
}

fn add_enum_hints(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if let Some(Value::Array(enum_arr)) = map.get("enum") {
                let len = enum_arr.len();
                if len > 1 && len <= 10 {
                    let vals: Vec<String> = enum_arr.iter().map(value_to_string).collect();
                    append_hint_to_obj(map, &format!("Allowed: {}", vals.join(", ")));
                }
            }
            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                if let Some(child) = map.get_mut(&key) {
                    add_enum_hints(child);
                }
            }
        }
        Value::Array(arr) => {
            for item in arr {
                add_enum_hints(item);
            }
        }
        _ => {}
    }
}

fn add_additional_properties_hints(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if let Some(Value::Bool(false)) = map.get("additionalProperties") {
                append_hint_to_obj(map, "No extra properties allowed");
            }
            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                if let Some(child) = map.get_mut(&key) {
                    add_additional_properties_hints(child);
                }
            }
        }
        Value::Array(arr) => {
            for item in arr {
                add_additional_properties_hints(item);
            }
        }
        _ => {}
    }
}

fn move_constraints_to_description(value: &mut Value, in_properties_map: bool) {
    match value {
        Value::Object(map) => {
            if !in_properties_map {
                for key in UNSUPPORTED_CONSTRAINTS.iter() {
                    if let Some(val) = map.get(*key) {
                        if matches!(val, Value::Object(_) | Value::Array(_)) {
                            continue;
                        }
                        let hint = format!("{}: {}", key, value_to_string(val));
                        append_hint_to_obj(map, &hint);
                    }
                }
            }
            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                if let Some(child) = map.get_mut(&key) {
                    let child_in_props = key == "properties";
                    move_constraints_to_description(child, child_in_props);
                }
            }
        }
        Value::Array(arr) => {
            for item in arr {
                move_constraints_to_description(item, false);
            }
        }
        _ => {}
    }
}

fn merge_all_of(value: &mut Value) {
    match value {
        Value::Object(map) => {
            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                if let Some(child) = map.get_mut(&key) {
                    merge_all_of(child);
                }
            }

            let all_of = map.remove("allOf");
            if let Some(Value::Array(items)) = all_of {
                let mut required: Vec<String> = map
                    .get("required")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();

                if !map.contains_key("properties") {
                    map.insert("properties".to_string(), json!({}));
                }

                let mut props_map_opt = map
                    .get_mut("properties")
                    .and_then(|v| v.as_object_mut());

                for item in items {
                    if let Value::Object(item_map) = item {
                        if let Some(Value::Object(item_props)) = item_map.get("properties") {
                            if let Some(props_map) = props_map_opt.as_mut() {
                                for (k, v) in item_props {
                                    props_map.insert(k.clone(), v.clone());
                                }
                            }
                        }
                        if let Some(Value::Array(req_arr)) = item_map.get("required") {
                            for r in req_arr {
                                if let Some(s) = r.as_str() {
                                    if !required.contains(&s.to_string()) {
                                        required.push(s.to_string());
                                    }
                                }
                            }
                        }
                    }
                }

                if !required.is_empty() {
                    map.insert("required".to_string(), json!(required));
                }
            } else if let Some(other) = all_of {
                map.insert("allOf".to_string(), other);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                merge_all_of(item);
            }
        }
        _ => {}
    }
}

fn flatten_anyof_oneof(value: &mut Value) {
    match value {
        Value::Object(map) => {
            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                if let Some(child) = map.get_mut(&key) {
                    flatten_anyof_oneof(child);
                }
            }

            if let Some(Value::Array(items)) = map.get("anyOf") {
                if !items.is_empty() {
                    let parent_desc = map
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let (best_idx, all_types) = select_best(items);
                    let mut selected = items[best_idx].clone();
                    if !parent_desc.is_empty() {
                        merge_description_in_value(&mut selected, &parent_desc);
                    }
                    if all_types.len() > 1 {
                        append_hint_to_value(
                            &mut selected,
                            &format!("Accepts: {}", all_types.join(" | ")),
                        );
                    }
                    *value = selected;
                    return;
                }
            }

            if let Some(Value::Array(items)) = map.get("oneOf") {
                if !items.is_empty() {
                    let parent_desc = map
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let (best_idx, all_types) = select_best(items);
                    let mut selected = items[best_idx].clone();
                    if !parent_desc.is_empty() {
                        merge_description_in_value(&mut selected, &parent_desc);
                    }
                    if all_types.len() > 1 {
                        append_hint_to_value(
                            &mut selected,
                            &format!("Accepts: {}", all_types.join(" | ")),
                        );
                    }
                    *value = selected;
                    return;
                }
            }
        }
        Value::Array(arr) => {
            for item in arr {
                flatten_anyof_oneof(item);
            }
        }
        _ => {}
    }
}

fn flatten_type_arrays(value: &mut Value) {
    let mut nullable_fields: HashMap<Vec<PathSegment>, HashSet<String>> = HashMap::new();
    let mut path = Vec::new();
    flatten_type_arrays_inner(value, &mut path, &mut nullable_fields);

    for (obj_path, fields) in nullable_fields {
        if let Some(target) = get_mut_at_path(value, &obj_path) {
            if let Value::Object(map) = target {
                if let Some(Value::Array(req_arr)) = map.get_mut("required") {
                    let filtered: Vec<Value> = req_arr
                        .iter()
                        .filter(|v| {
                            v.as_str()
                                .map(|s| !fields.contains(s))
                                .unwrap_or(true)
                        })
                        .cloned()
                        .collect();
                    if filtered.is_empty() {
                        map.remove("required");
                    } else {
                        *req_arr = filtered;
                    }
                }
            }
        }
    }
}

fn flatten_type_arrays_inner(
    value: &mut Value,
    path: &mut Vec<PathSegment>,
    nullable_fields: &mut HashMap<Vec<PathSegment>, HashSet<String>>,
) {
    match value {
        Value::Object(map) => {
            if let Some(Value::Array(types_arr)) = map.get_mut("type") {
                if !types_arr.is_empty() {
                    let mut has_null = false;
                    let mut non_null_types: Vec<String> = Vec::new();
                    for item in types_arr.iter() {
                        let s = value_to_string(item);
                        if s == "null" {
                            has_null = true;
                        } else if !s.is_empty() {
                            non_null_types.push(s);
                        }
                    }
                    let first_type = non_null_types
                        .get(0)
                        .cloned()
                        .unwrap_or_else(|| "string".to_string());
                    *types_arr = vec![Value::String(first_type.clone())];

                    if non_null_types.len() > 1 {
                        append_hint_to_obj(
                            map,
                            &format!("Accepts: {}", non_null_types.join(" | ")),
                        );
                    }

                    if has_null {
                        if let Some((obj_path, field_name)) = property_context(path) {
                            append_hint_to_obj(map, "(nullable)");
                            nullable_fields
                                .entry(obj_path)
                                .or_default()
                                .insert(field_name);
                        }
                    }
                }
            }

            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                if let Some(child) = map.get_mut(&key) {
                    path.push(PathSegment::Key(key.clone()));
                    flatten_type_arrays_inner(child, path, nullable_fields);
                    path.pop();
                }
            }
        }
        Value::Array(arr) => {
            for (idx, item) in arr.iter_mut().enumerate() {
                path.push(PathSegment::Index(idx));
                flatten_type_arrays_inner(item, path, nullable_fields);
                path.pop();
            }
        }
        _ => {}
    }
}

fn remove_unsupported_keywords(value: &mut Value, in_properties_map: bool) {
    let mut keywords: Vec<&str> = Vec::new();
    keywords.extend(UNSUPPORTED_CONSTRAINTS.iter().copied());
    keywords.extend([
        "$schema",
        "$defs",
        "definitions",
        "const",
        "$ref",
        "additionalProperties",
        "propertyNames",
    ]);

    match value {
        Value::Object(map) => {
            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                let is_extension = key.starts_with("x-");
                let should_remove = !in_properties_map
                    && (keywords.contains(&key.as_str()) || is_extension);
                if should_remove {
                    map.remove(&key);
                }
            }

            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                if let Some(child) = map.get_mut(&key) {
                    let child_in_props = key == "properties";
                    remove_unsupported_keywords(child, child_in_props);
                }
            }
        }
        Value::Array(arr) => {
            for item in arr {
                remove_unsupported_keywords(item, false);
            }
        }
        _ => {}
    }
}

fn remove_keywords(value: &mut Value, in_properties_map: bool, keywords: &[&str]) {
    match value {
        Value::Object(map) => {
            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                if !in_properties_map && keywords.contains(&key.as_str()) {
                    map.remove(&key);
                }
            }

            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                if let Some(child) = map.get_mut(&key) {
                    let child_in_props = key == "properties";
                    remove_keywords(child, child_in_props, keywords);
                }
            }
        }
        Value::Array(arr) => {
            for item in arr {
                remove_keywords(item, false, keywords);
            }
        }
        _ => {}
    }
}

fn remove_placeholder_fields(value: &mut Value) {
    match value {
        Value::Object(map) => {
            let mut remove_underscore = false;
            let mut remove_reason = false;
            if let Some(Value::Object(props)) = map.get_mut("properties") {
                if props.contains_key("_") {
                    props.remove("_");
                    remove_underscore = true;
                }

                if props.contains_key("reason") && props.len() == 1 {
                    let should_remove = props
                        .get("reason")
                        .and_then(|v| v.get("description"))
                        .and_then(|v| v.as_str())
                        .map(|d| d == PLACEHOLDER_REASON_DESCRIPTION)
                        .unwrap_or(false);
                    if should_remove {
                        props.remove("reason");
                        remove_reason = true;
                    }
                }
            }
            if remove_underscore {
                remove_required_entry(map, "_");
            }
            if remove_reason {
                remove_required_entry(map, "reason");
            }

            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                if let Some(child) = map.get_mut(&key) {
                    remove_placeholder_fields(child);
                }
            }
        }
        Value::Array(arr) => {
            for item in arr {
                remove_placeholder_fields(item);
            }
        }
        _ => {}
    }
}

fn cleanup_required_fields(value: &mut Value) {
    match value {
        Value::Object(map) => {
            let has_props = map.get("properties").and_then(|v| v.as_object()).cloned();
            if let (Some(props), Some(Value::Array(req_arr))) = (has_props, map.get_mut("required"))
            {
                let valid: Vec<Value> = req_arr
                    .iter()
                    .filter_map(|v| v.as_str())
                    .filter(|key| props.contains_key(*key))
                    .map(|s| Value::String(s.to_string()))
                    .collect();
                if valid.len() != req_arr.len() {
                    if valid.is_empty() {
                        map.remove("required");
                    } else {
                        *req_arr = valid;
                    }
                }
            }

            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                if let Some(child) = map.get_mut(&key) {
                    cleanup_required_fields(child);
                }
            }
        }
        Value::Array(arr) => {
            for item in arr {
                cleanup_required_fields(item);
            }
        }
        _ => {}
    }
}

fn add_empty_schema_placeholder(value: &mut Value, path: &mut Vec<PathSegment>) {
    match value {
        Value::Object(map) => {
            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                if let Some(child) = map.get_mut(&key) {
                    path.push(PathSegment::Key(key.clone()));
                    add_empty_schema_placeholder(child, path);
                    path.pop();
                }
            }

            let is_object = map
                .get("type")
                .and_then(|v| v.as_str())
                .map(|t| t == "object")
                .unwrap_or(false);
            if !is_object {
                return;
            }

            let props_exists = map.get("properties").and_then(|v| v.as_object());
            let props_empty = props_exists.map(|p| p.is_empty()).unwrap_or(true);
            let has_required = map
                .get("required")
                .and_then(|v| v.as_array())
                .map(|a| !a.is_empty())
                .unwrap_or(false);

            if props_exists.is_none() || props_empty {
                let props = map.entry("properties".to_string()).or_insert_with(|| json!({}));
                if let Value::Object(props_map) = props {
                    props_map.insert(
                        "reason".to_string(),
                        json!({
                            "type": "string",
                            "description": PLACEHOLDER_REASON_DESCRIPTION
                        }),
                    );
                }
                map.insert("required".to_string(), json!(vec!["reason"]));
                return;
            }

            if props_exists.is_some() && !has_required && !path.is_empty() {
                if let Some(Value::Object(props_map)) = map.get_mut("properties") {
                    if !props_map.contains_key("_") {
                        props_map.insert("_".to_string(), json!({ "type": "boolean" }));
                    }
                }
                map.insert("required".to_string(), json!(vec!["_"]));
            }
        }
        Value::Array(arr) => {
            for (idx, item) in arr.iter_mut().enumerate() {
                path.push(PathSegment::Index(idx));
                add_empty_schema_placeholder(item, path);
                path.pop();
            }
        }
        _ => {}
    }
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}

fn append_hint_to_obj(map: &mut Map<String, Value>, hint: &str) {
    let existing = map.get("description").and_then(|v| v.as_str()).unwrap_or("");
    let new_desc = if existing.is_empty() {
        hint.to_string()
    } else {
        format!("{} ({})", existing, hint)
    };
    map.insert("description".to_string(), Value::String(new_desc));
}

fn append_hint_to_value(value: &mut Value, hint: &str) {
    if let Value::Object(map) = value {
        append_hint_to_obj(map, hint);
    }
}

fn merge_description_in_value(value: &mut Value, parent_desc: &str) {
    if parent_desc.is_empty() {
        return;
    }
    if let Value::Object(map) = value {
        let child_desc = map.get("description").and_then(|v| v.as_str()).unwrap_or("");
        if child_desc.is_empty() {
            map.insert("description".to_string(), Value::String(parent_desc.to_string()));
        } else if child_desc != parent_desc {
            map.insert(
                "description".to_string(),
                Value::String(format!("{} ({})", parent_desc, child_desc)),
            );
        }
    }
}

fn select_best(items: &[Value]) -> (usize, Vec<String>) {
    let mut best_idx = 0;
    let mut best_score = -1;
    let mut types: Vec<String> = Vec::new();

    for (idx, item) in items.iter().enumerate() {
        let mut t = item
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let has_props = item.get("properties").is_some();
        let has_items = item.get("items").is_some();

        let score = if t == "object" || has_props {
            if t.is_empty() {
                t = "object".to_string();
            }
            3
        } else if t == "array" || has_items {
            if t.is_empty() {
                t = "array".to_string();
            }
            2
        } else if !t.is_empty() && t != "null" {
            1
        } else {
            if t.is_empty() {
                t = "null".to_string();
            }
            0
        };

        if !t.is_empty() {
            types.push(t);
        }

        if score > best_score {
            best_score = score;
            best_idx = idx;
        }
    }

    (best_idx, types)
}

fn property_context(path: &[PathSegment]) -> Option<(Vec<PathSegment>, String)> {
    if path.len() < 2 {
        return None;
    }
    match (&path[path.len() - 2], &path[path.len() - 1]) {
        (PathSegment::Key(props), PathSegment::Key(field)) if props == "properties" => {
            let obj_path = path[..path.len() - 2].to_vec();
            Some((obj_path, field.clone()))
        }
        _ => None,
    }
}

fn get_mut_at_path<'a>(value: &'a mut Value, path: &[PathSegment]) -> Option<&'a mut Value> {
    let mut current = value;
    for seg in path {
        match seg {
            PathSegment::Key(key) => {
                if let Value::Object(map) = current {
                    current = map.get_mut(key)?;
                } else {
                    return None;
                }
            }
            PathSegment::Index(idx) => {
                if let Value::Array(arr) = current {
                    current = arr.get_mut(*idx)?;
                } else {
                    return None;
                }
            }
        }
    }
    Some(current)
}

fn remove_required_entry(map: &mut Map<String, Value>, field: &str) {
    if let Some(Value::Array(req_arr)) = map.get_mut("required") {
        let filtered: Vec<Value> = req_arr
            .iter()
            .filter(|v| v.as_str().map(|s| s != field).unwrap_or(true))
            .cloned()
            .collect();
        if filtered.is_empty() {
            map.remove("required");
        } else {
            *req_arr = filtered;
        }
    }
}
