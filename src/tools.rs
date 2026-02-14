use regex::Regex;
use serde_json::Error;
use std::fs;
use std::{collections::HashMap, process::Command};

pub enum ToolFormat {
    JsonStandard,
    PythonCall,
    Functools,
    ToolCallTags,
    ToolCallXml,
}

#[derive(Debug, Clone)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub properties: HashMap<String, (String, String)>, // (type, description)
    pub required: Vec<String>,
}

impl Tool {
    pub fn to_json(&self) -> serde_json::Value {
        let mut props = serde_json::Map::new();

        let parse_type = |t: &str| -> String {
            match t {
                "str" => "string".into(),
                "int" => "integer".into(),
                "float" => "number".into(),
                "bool" => "boolean".into(),
                "list" => "array".into(),
                "dict" => "object".into(),
                "None" => "null".into(),
                "null" => "null".into(),
                v => v.into(),
            }
        };

        for (name, (prop_type, desc)) in &self.properties {
            props.insert(
                name.clone(),
                serde_json::json!({
                    "type": parse_type(prop_type),
                    "description": desc
                }),
            );
        }

        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": {
                    "type": "object",
                    "properties": props,
                    "required": self.required
                }
            }
        })
    }
}

pub trait ToJson {
    fn to_json(&self) -> Result<String, Error>;
}

impl ToJson for Vec<Tool> {
    fn to_json(&self) -> Result<String, Error> {
        let tools: Vec<serde_json::Value> = self.iter().map(|t| t.to_json()).collect();
        serde_json::to_string(&tools)
    }
}

pub fn run_tool(tool_file_path: &str, command: &str) -> anyhow::Result<String> {
    let mut segments: Vec<&str> = tool_file_path.split("/").collect();
    let file = segments.pop().unwrap_or_default();

    let command = Command::new("python")
        .current_dir(segments.join("/"))
        .args([
            "-c",
            &format!(
                "from {} import *; print({})",
                file.strip_suffix(".py").unwrap_or("tools"),
                command
            ),
        ])
        .output()?;

    Ok(String::from_utf8(command.stdout)?)
}

pub fn parse_tool_call(text: &str, format: ToolFormat) -> Option<String> {
    let trimmed = text.trim();

    match format {
        ToolFormat::JsonStandard => {
            // {"name": "function_name", "parameters": {...}}
            serde_json::from_str::<serde_json::Value>(trimmed)
                .ok()
                .and_then(|json| {
                    let name = json.get("name")?.as_str()?;
                    let params = json.get("parameters")?.as_object()?;

                    let args = params
                        .iter()
                        .map(|(k, v)| format!("{}={}", k, serde_json::to_string(v).unwrap()))
                        .collect::<Vec<_>>()
                        .join(", ");

                    Some(format!("{}({})", name, args))
                })
        }

        ToolFormat::PythonCall => {
            // <|python_tag|>function_name.call(arg1="val1", arg2="val2")
            trimmed.find("<|python_tag|>").and_then(|start| {
                let call_str = &trimmed[start + 14..].trim();
                call_str.rfind(')').map(|end| call_str[..=end].to_string())
            })
        }

        ToolFormat::Functools => {
            // functools[{"name": "...", "arguments": {...}}]
            trimmed.find("functools[").and_then(|start| {
                let json_start = start + 10;
                trimmed.rfind(']').and_then(|end| {
                    let json_str = &trimmed[json_start..end];
                    serde_json::from_str::<serde_json::Value>(json_str)
                        .ok()
                        .and_then(|json| {
                            let name = json.get("name")?.as_str()?;
                            let params = json.get("arguments")?.as_object()?;

                            let args = params
                                .iter()
                                .map(|(k, v)| {
                                    format!("{}={}", k, serde_json::to_string(v).unwrap())
                                })
                                .collect::<Vec<_>>()
                                .join(", ");

                            Some(format!("{}({})", name, args))
                        })
                })
            })
        }

        ToolFormat::ToolCallTags => {
            // <|tool_call_start|>[function_name(args)]<|tool_call_end|>
            trimmed.find("<|tool_call_start|>").and_then(|start| {
                let call_start = start + 19; // length of "<|tool_call_start|>"
                trimmed.find("<|tool_call_end|>").map(|end| {
                    let inner = trimmed[call_start..end].trim();
                    // Strip surrounding brackets if present
                    let inner = inner.strip_prefix('[').unwrap_or(inner);
                    let inner = inner.strip_suffix(']').unwrap_or(inner);
                    inner.to_string()
                })
            })
        }

        ToolFormat::ToolCallXml => {
            // <tool_call>{"name": "...", "arguments": {...}}</tool_call>
            trimmed.find("<tool_call>").and_then(|start| {
                let json_start = start + 11; // length of "<tool_call>"
                trimmed.find("</tool_call>").and_then(|end| {
                    let json_str = trimmed[json_start..end].trim();
                    serde_json::from_str::<serde_json::Value>(json_str)
                        .ok()
                        .and_then(|json| {
                            let name = json.get("name")?.as_str()?;
                            let params = json.get("arguments")?.as_object()?;

                            let args = params
                                .iter()
                                .map(|(k, v)| {
                                    format!("{}={}", k, serde_json::to_string(v).unwrap())
                                })
                                .collect::<Vec<_>>()
                                .join(", ");

                            Some(format!("{}({})", name, args))
                        })
                })
            })
        }
    }
}

// Check if model supports tool calling
pub fn supports_tools(chat_template: &str) -> bool {
    chat_template.contains("tool_calls")
        || chat_template.contains("tools is not")
        || chat_template.contains("tool is not")
        || chat_template.contains("function")
        || chat_template.contains("<tool_call>")
}

// // Detect tool call format from template
// pub fn detect_tool_format(chat_template: &str) -> ToolFormat {
//     if chat_template.contains(r#"{"name":"#) {
//         ToolFormat::JsonStandard // {"name": "...", "parameters": {...}}
//     } else if chat_template.contains("<|python_tag|>") {
//         ToolFormat::PythonCall // builtin tools format
//     } else if chat_template.contains("functools") {
//         ToolFormat::Functools // functools[...]
//     } else if chat_template.contains("<|tool_call_start|>") {
//         ToolFormat::ToolCallTags // <|tool_call_start|>...<|tool_call_end|>
//     } else {

//     }
// }

pub fn parse_python_functions(directory: String) -> Vec<Tool> {
    let content = fs::read_to_string(&directory).expect("Failed to read file");

    // Match: def function_name(args):
    //            """docstring"""
    let func_regex = Regex::new(r#"(?m)^def\s+(\w+)\s*\((.*?)\).*?:\s*\n\s*"""(.*?)""""#).unwrap();

    let mut tools = Vec::new();

    for cap in func_regex.captures_iter(&content) {
        let name = cap[1].to_string();
        let args_str = &cap[2];
        let docstring = cap[3].trim().to_string();

        let (properties, required) = parse_arguments(args_str);

        tools.push(Tool {
            name,
            description: docstring,
            properties,
            required,
        });
    }

    tools
}

/// Tries all tool call formats and returns the parsed command if found
pub fn try_parse_tool_call(text: &str) -> Option<(ToolFormat, String)> {
    if let Some(cmd) = parse_tool_call(text, ToolFormat::JsonStandard) {
        return Some((ToolFormat::JsonStandard, cmd));
    }
    if let Some(cmd) = parse_tool_call(text, ToolFormat::PythonCall) {
        return Some((ToolFormat::PythonCall, cmd));
    }
    if let Some(cmd) = parse_tool_call(text, ToolFormat::Functools) {
        return Some((ToolFormat::Functools, cmd));
    }
    if let Some(cmd) = parse_tool_call(text, ToolFormat::ToolCallTags) {
        return Some((ToolFormat::ToolCallTags, cmd));
    }
    if let Some(cmd) = parse_tool_call(text, ToolFormat::ToolCallXml) {
        return Some((ToolFormat::ToolCallXml, cmd));
    }
    None
}

/// Splits multiple tool calls (e.g., "func1(), func2(arg=1)") into individual calls
pub fn split_tool_calls(calls: &str) -> Vec<String> {
    let mut results = Vec::new();
    let mut current = String::new();
    let mut paren_depth = 0;

    for ch in calls.chars() {
        match ch {
            '(' => {
                paren_depth += 1;
                current.push(ch);
            }
            ')' => {
                paren_depth -= 1;
                current.push(ch);
            }
            ',' if paren_depth == 0 => {
                let trimmed = current.trim().to_string();
                if !trimmed.is_empty() {
                    results.push(trimmed);
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }

    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        results.push(trimmed);
    }

    results
}

fn parse_arguments(args_str: &str) -> (HashMap<String, (String, String)>, Vec<String>) {
    let mut properties = HashMap::new();
    let mut required = Vec::new();

    for arg in args_str.split(',') {
        let arg = arg.trim();
        if arg.is_empty() || arg == "self" {
            continue;
        }

        let has_default = arg.contains('=');

        // Parse: arg_name: type = default or arg_name: type
        let parts: Vec<&str> = arg.split(':').collect();
        if parts.len() == 2 {
            let arg_name = parts[0].trim().to_string();
            let type_part = parts[1].split('=').next().unwrap().trim().to_string();

            properties.insert(arg_name.clone(), (type_part, String::new()));

            if !has_default {
                required.push(arg_name);
            }
        }
    }

    (properties, required)
}
