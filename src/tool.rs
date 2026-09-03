use anyhow::Result;
use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

/// Where a tool comes from: a built-in Rust tool or a (real or facade) MCP
/// server's tool. Serialised lowercase for the control-plane API
/// (`"builtin"` / `"mcp"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolSource {
    Builtin,
    Mcp,
}

/// Registry snapshot of one tool: name, whether it is enabled (not
/// suppressed), and where it comes from.
#[derive(Debug, Clone, Serialize)]
pub struct ToolInfo {
    pub name: String,
    pub enabled: bool,
    pub source: ToolSource,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> String;

    fn description(&self) -> String;

    fn input_schema(&self) -> Value;

    /// Where this tool comes from. Defaults to a built-in tool; MCP facade
    /// tools override this to [`ToolSource::Mcp`].
    fn source(&self) -> ToolSource {
        ToolSource::Builtin
    }

    /// Whether the tool only reads state and never mutates anything outside
    /// the agent scratch area. Defaults to `true`; write tools (currently
    /// only `write_file`) override to `false`. The future `--tools-readonly`
    /// flag indexes this to strip write tools from the registry.
    fn is_read_only(&self) -> bool {
        true
    }

    async fn execute(&self, input: Value) -> Result<String>;
}

/// Registry of available tools plus per-tool suppression state. The
/// suppression set is shared through an `Arc`, so clones of the registry
/// (e.g. one held by the HTTP control plane, one owned by the agent) see and
/// apply the same on/off state.
#[derive(Clone)]
pub struct ToolRegistry {
    tools: Arc<HashMap<String, Arc<dyn Tool>>>,
    suppressed: Arc<RwLock<HashSet<String>>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: Arc::new(HashMap::new()),
            suppressed: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        let name = tool.name();
        let tool: Arc<dyn Tool> = Arc::from(tool);
        let mut map = (*self.tools).clone();
        map.insert(name, tool);
        self.tools = Arc::new(map);
    }

    pub fn get(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.tools.get(name)
    }

    pub fn get_all_for_llm(&self) -> Vec<crate::provider::Tool> {
        let suppressed = self.lock_suppressed();
        self.tools
            .values()
            .filter(|tool| !suppressed.contains(&tool.name()))
            .map(|tool| crate::provider::Tool {
                name: tool.name(),
                description: tool.description(),
                input_schema: tool.input_schema(),
            })
            .collect()
    }

    pub fn list_tools(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    /// Enable or disable a tool by name. A disabled (suppressed) tool is
    /// hidden from the model and errors on execution.
    pub fn set_suppressed(&self, name: &str, enabled: bool) {
        let mut suppressed = self
            .suppressed
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if enabled {
            suppressed.remove(name);
        } else {
            suppressed.insert(name.to_string());
        }
    }

    /// Whether the tool is currently suppressed (disabled).
    pub fn is_suppressed(&self, name: &str) -> bool {
        self.lock_suppressed().contains(name)
    }

    /// All currently suppressed tool names, sorted (for persistence).
    pub fn suppressed_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.lock_suppressed().iter().cloned().collect();
        names.sort();
        names
    }

    /// Snapshot of every registered tool: name, enabled flag and source.
    pub fn list_tools_info(&self) -> Vec<ToolInfo> {
        let suppressed = self.lock_suppressed();
        let mut infos: Vec<ToolInfo> = self
            .tools
            .values()
            .map(|tool| {
                let name = tool.name();
                ToolInfo {
                    enabled: !suppressed.contains(&name),
                    name,
                    source: tool.source(),
                }
            })
            .collect();
        infos.sort_by(|a, b| a.name.cmp(&b.name));
        infos
    }

    fn lock_suppressed(&self) -> std::sync::RwLockReadGuard<'_, HashSet<String>> {
        self.suppressed
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct DummyTool {
        name: &'static str,
        source: ToolSource,
    }

    #[async_trait]
    impl Tool for DummyTool {
        fn name(&self) -> String {
            self.name.to_string()
        }

        fn description(&self) -> String {
            "dummy".to_string()
        }

        fn input_schema(&self) -> Value {
            json!({"type": "object"})
        }

        fn source(&self) -> ToolSource {
            self.source
        }

        async fn execute(&self, _input: Value) -> Result<String> {
            Ok("ok".to_string())
        }
    }

    fn dummy(name: &'static str, source: ToolSource) -> Box<dyn Tool> {
        Box::new(DummyTool { name, source })
    }

    #[test]
    fn default_source_is_builtin() {
        let registry = ToolRegistry::new();
        let mut registry = registry;
        registry.register(dummy("Calc", ToolSource::Builtin));
        let infos = registry.list_tools_info();
        assert_eq!(infos.len(), 1);
        assert_eq!(infos[0].name, "Calc");
        assert!(infos[0].enabled);
        assert_eq!(infos[0].source, ToolSource::Builtin);
    }

    #[test]
    fn get_all_for_llm_skips_suppressed_tools() {
        let mut registry = ToolRegistry::new();
        registry.register(dummy("Calc", ToolSource::Builtin));
        registry.register(dummy("tavily_search", ToolSource::Mcp));

        assert_eq!(registry.get_all_for_llm().len(), 2);

        registry.set_suppressed("Calc", false);
        assert!(registry.is_suppressed("Calc"));
        let for_llm = registry.get_all_for_llm();
        assert_eq!(for_llm.len(), 1);
        assert_eq!(for_llm[0].name, "tavily_search");

        registry.set_suppressed("Calc", true);
        assert!(!registry.is_suppressed("Calc"));
        assert_eq!(registry.get_all_for_llm().len(), 2);
    }

    #[test]
    fn list_tools_info_reports_enabled_state_and_sources() {
        let mut registry = ToolRegistry::new();
        registry.register(dummy("WebSearch", ToolSource::Builtin));
        registry.register(dummy("tavily_search", ToolSource::Mcp));
        registry.register(dummy("tavily_extract", ToolSource::Mcp));
        registry.set_suppressed("tavily_search", false);

        let infos = registry.list_tools_info();
        assert_eq!(infos.len(), 3);
        // Sorted by name for deterministic snapshots.
        let names: Vec<&str> = infos.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, vec!["WebSearch", "tavily_extract", "tavily_search"]);

        let by_name = |name: &str| infos.iter().find(|i| i.name == name).unwrap();
        assert!(by_name("WebSearch").enabled);
        assert_eq!(by_name("WebSearch").source, ToolSource::Builtin);
        assert!(!by_name("tavily_search").enabled);
        assert_eq!(by_name("tavily_search").source, ToolSource::Mcp);
        assert!(by_name("tavily_extract").enabled);
        assert_eq!(by_name("tavily_extract").source, ToolSource::Mcp);
    }

    #[test]
    fn clones_share_suppression_state() {
        let mut registry = ToolRegistry::new();
        registry.register(dummy("WebSearch", ToolSource::Builtin));
        let clone = registry.clone();

        assert!(!clone.is_suppressed("WebSearch"));
        registry.set_suppressed("WebSearch", false);
        assert!(clone.is_suppressed("WebSearch"));
        assert!(clone.get_all_for_llm().is_empty());
        assert_eq!(clone.list_tools_info().len(), 1);
        assert!(!clone.list_tools_info()[0].enabled);

        clone.set_suppressed("WebSearch", true);
        assert!(!registry.is_suppressed("WebSearch"));
    }

    #[test]
    fn suppressed_names_is_sorted_for_persistence() {
        let mut registry = ToolRegistry::new();
        registry.register(dummy("WebSearch", ToolSource::Builtin));
        registry.register(dummy("tavily_search", ToolSource::Mcp));
        registry.set_suppressed("tavily_search", false);
        registry.set_suppressed("WebSearch", false);
        assert_eq!(
            registry.suppressed_names(),
            vec!["WebSearch".to_string(), "tavily_search".to_string()]
        );
    }
}
