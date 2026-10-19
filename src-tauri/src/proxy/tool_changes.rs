//! Mid-conversation tool changes for Anthropic-compatible upstreams.
//!
//! Claude Code announces tools that appear or disappear during a session
//! (an MCP server connecting, a deferred tool being loaded) with content
//! blocks inside a message:
//!
//! ```json
//! {"type":"tool_addition","tool":{"type":"tool_definition","definition":{"name":..,"input_schema":..}}}
//! {"type":"tool_removal","tool":{"type":"tool_reference","name":".."}}
//! ```
//!
//! Anthropic's API reads them; third-party Anthropic-compatible endpoints do
//! not. DeepSeek answers 422 "unknown variant tool_definition, expected
//! tool_reference", which breaks the whole conversation from that message on.
//! For those upstreams the additions move into the top-level `tools` list,
//! where every compatible endpoint understands them, and both kinds of block
//! become a one-line text note so the model still learns what changed.

use serde_json::{json, Value};

/// Anthropic's own API understands tool changes natively.
pub fn upstream_reads_tool_changes(host: Option<&str>) -> bool {
    host.map(|host| host.split(':').next().unwrap_or(host).to_ascii_lowercase())
        .is_some_and(|host| host == "anthropic.com" || host.ends_with(".anthropic.com"))
}

/// Rewrite `tool_addition` / `tool_removal` blocks in an Anthropic Messages
/// body. Returns whether anything changed.
pub fn inline_tool_changes(body: &mut Value) -> bool {
    let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) else {
        return false;
    };

    let mut added: Vec<Value> = Vec::new();
    let mut changed = false;
    for message in messages.iter_mut() {
        let Some(content) = message.get_mut("content").and_then(Value::as_array_mut) else {
            continue;
        };
        for block in content.iter_mut() {
            let kind = block.get("type").and_then(Value::as_str);
            let note = match kind {
                Some("tool_addition") => {
                    let definition = block
                        .get("tool")
                        .and_then(|tool| tool.get("definition"))
                        .cloned();
                    let name = definition
                        .as_ref()
                        .and_then(|d| d.get("name"))
                        .or_else(|| block.get("tool").and_then(|tool| tool.get("name")))
                        .and_then(Value::as_str)
                        .unwrap_or("a tool")
                        .to_string();
                    if let Some(definition) = definition.filter(Value::is_object) {
                        added.push(definition);
                    }
                    format!("[Tool now available: {name}]")
                }
                Some("tool_removal") => {
                    let name = block
                        .get("tool")
                        .and_then(|tool| tool.get("name").or_else(|| tool.get("tool_name")))
                        .and_then(Value::as_str)
                        .unwrap_or("a tool");
                    format!("[Tool no longer available: {name}]")
                }
                _ => continue,
            };
            *block = json!({ "type": "text", "text": note });
            changed = true;
        }
    }

    if !added.is_empty() {
        add_tools(body, added);
    }
    changed
}

/// Append definitions to `tools`, skipping names already declared. A later
/// addition of the same name replaces nothing: the first declaration is the
/// one the model has been calling.
fn add_tools(body: &mut Value, definitions: Vec<Value>) {
    let Some(object) = body.as_object_mut() else {
        return;
    };
    let tools = object
        .entry("tools")
        .or_insert_with(|| Value::Array(Vec::new()));
    let Some(tools) = tools.as_array_mut() else {
        return;
    };
    for definition in definitions {
        let name = definition.get("name").and_then(Value::as_str);
        let known = name.is_some_and(|name| {
            tools
                .iter()
                .any(|tool| tool.get("name").and_then(Value::as_str) == Some(name))
        });
        if !known {
            tools.push(definition);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body_with(blocks: Value) -> Value {
        json!({
            "model": "deepseek-v4-pro",
            "tools": [{"name": "Read", "input_schema": {"type": "object"}}],
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "hi"}]},
                {"role": "user", "content": blocks},
            ]
        })
    }

    #[test]
    fn additions_move_into_tools_and_leave_a_note() {
        let mut body = body_with(json!([
            {"type": "text", "text": "context"},
            {"type": "tool_addition", "tool": {"type": "tool_definition", "definition": {
                "name": "mcp__docs__batch",
                "description": "Batch",
                "input_schema": {"type": "object"},
                "eager_input_streaming": true
            }}}
        ]));
        assert!(inline_tool_changes(&mut body));

        let tools: Vec<&str> = body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(tools, ["Read", "mcp__docs__batch"]);
        assert_eq!(
            body["messages"][1]["content"][1],
            json!({"type": "text", "text": "[Tool now available: mcp__docs__batch]"})
        );
        assert!(!body.to_string().contains("tool_definition"));
    }

    #[test]
    fn removals_become_notes() {
        let mut body = body_with(json!([
            {"type": "tool_removal", "tool": {"type": "tool_reference", "name": "mcp__docs__batch"}}
        ]));
        assert!(inline_tool_changes(&mut body));
        assert_eq!(
            body["messages"][1]["content"][0]["text"],
            "[Tool no longer available: mcp__docs__batch]"
        );
    }

    #[test]
    fn a_tool_already_declared_is_not_added_twice() {
        let mut body = body_with(json!([
            {"type": "tool_addition", "tool": {"type": "tool_definition", "definition": {
                "name": "Read", "input_schema": {"type": "object"}
            }}}
        ]));
        inline_tool_changes(&mut body);
        assert_eq!(body["tools"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn tools_list_is_created_when_missing() {
        let mut body = json!({"messages": [{"role": "user", "content": [
            {"type": "tool_addition", "tool": {"type": "tool_definition", "definition": {
                "name": "X", "input_schema": {"type": "object"}
            }}}
        ]}]});
        inline_tool_changes(&mut body);
        assert_eq!(body["tools"][0]["name"], "X");
    }

    #[test]
    fn ordinary_bodies_are_untouched() {
        let mut body = body_with(json!([
            {"type": "tool_result", "tool_use_id": "t1", "content": [
                {"type": "tool_reference", "tool_name": "Monitor"}
            ]}
        ]));
        let before = body.clone();
        assert!(!inline_tool_changes(&mut body));
        assert_eq!(body, before);
    }

    #[test]
    fn only_anthropic_reads_tool_changes() {
        assert!(upstream_reads_tool_changes(Some("api.anthropic.com")));
        assert!(upstream_reads_tool_changes(Some("api.anthropic.com:443")));
        for host in ["api.deepseek.com", "api.xiaomimimo.com", "openrouter.ai", "notanthropic.com"] {
            assert!(!upstream_reads_tool_changes(Some(host)), "{host}");
        }
        assert!(!upstream_reads_tool_changes(None));
    }
}
