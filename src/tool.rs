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

    /// Which (fake) MCP server this tool belongs to, when
    /// [`Tool::source`] is [`ToolSource::Mcp`]. Defaults to `None`;
    /// MCP facade tools override this with their server's name so the
    /// panel's per-server on/off toggle (POST /api/mcp) can suppress all
    /// of a server's tools at once.
    fn mcp_server(&self) -> Option<&'static str> {
        None
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

    /// Registry-level read-only filter: consume the registry and return one
    /// that exposes ONLY tools whose `Tool::is_read_only()` is `true` (the
    /// shared suppression state is carried over unchanged). The
    /// `--tools-readonly` flag applies this wherever a registry is built —
    /// there is no tool-name special-casing anywhere; a tool is dropped
    /// exactly when it reports `is_read_only() == false`.
    pub fn into_read_only(self) -> Self {
        let tools: HashMap<String, Arc<dyn Tool>> = (*self.tools)
            .clone()
            .into_iter()
            .filter(|(_name, tool)| tool.is_read_only())
            .collect();
        Self {
            tools: Arc::new(tools),
            suppressed: self.suppressed,
        }
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

    /// All registered MCP server names (sorted, deduped) — the servers that
    /// have at least one `ToolSource::Mcp` tool. This is the set of servers
    /// the /api/mcp toggle accepts (an unregistered server is unknown).
    pub fn mcp_servers(&self) -> Vec<String> {
        let mut servers: Vec<String> = self
            .tools
            .values()
            .filter(|tool| tool.source() == ToolSource::Mcp)
            .filter_map(|tool| tool.mcp_server().map(str::to_string))
            .collect();
        servers.sort();
        servers.dedup();
        servers
    }

    /// Enable or disable ALL tools of one MCP server (panel-level toggle).
    /// A disabled server's tools are hidden from the model and error on
    /// execution — per-tool suppression entries for that server's tools, so
    /// the proven registry mechanism carries the state. Unknown servers
    /// have no tools and are a no-op (the HTTP layer 400s first).
    pub fn set_mcp_suppressed(&self, server: &str, enabled: bool) {
        let names: Vec<String> = self
            .tools
            .values()
            .filter(|tool| tool.source() == ToolSource::Mcp && tool.mcp_server() == Some(server))
            .map(|tool| tool.name())
            .collect();
        for name in names {
            self.set_suppressed(&name, enabled);
        }
    }

    /// Whether every tool of the MCP server is currently enabled (not
    /// suppressed). A server with no registered tools reports `true`.
    pub fn mcp_server_enabled(&self, server: &str) -> bool {
        self.tools
            .values()
            .filter(|tool| tool.source() == ToolSource::Mcp && tool.mcp_server() == Some(server))
            .all(|tool| !self.lock_suppressed().contains(&tool.name()))
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
        read_only: bool,
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

        fn is_read_only(&self) -> bool {
            self.read_only
        }

        async fn execute(&self, _input: Value) -> Result<String> {
            Ok("ok".to_string())
        }
    }

    fn dummy(name: &'static str, source: ToolSource) -> Box<dyn Tool> {
        Box::new(DummyTool {
            name,
            source,
            read_only: true,
        })
    }

    fn write_dummy(name: &'static str) -> Box<dyn Tool> {
        Box::new(DummyTool {
            name,
            source: ToolSource::Builtin,
            read_only: false,
        })
    }

    #[test]
    fn into_read_only_keeps_only_is_read_only_tools() {
        let mut registry = ToolRegistry::new();
        registry.register(dummy("Calc", ToolSource::Builtin));
        registry.register(dummy("tavily_search", ToolSource::Mcp));
        registry.register(write_dummy("write_file"));

        assert_eq!(registry.list_tools().len(), 3);

        let read_only = registry.into_read_only();
        let names = read_only.list_tools();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"Calc".to_string()));
        assert!(names.contains(&"tavily_search".to_string()));
        assert!(!names.contains(&"write_file".to_string()));
        assert!(read_only.get("write_file").is_none());
        assert!(read_only.get("Calc").is_some());
    }

    /// The real registry shape (same build as examples/axoner.rs and
    /// examples/axoner-web.rs): with the filter `write_file` is absent and
    /// every read tool (calculator / ReadFile / ListDir / ModelsConfig /
    /// WebSearch / WebFetch / tavily facade / context7 facade) is present;
    /// without it all eleven are present.
    #[test]
    fn into_read_only_filters_the_real_registry() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(crate::tools::Calculator));
        registry.register(Box::new(crate::tools::WriteFile::default()));
        registry.register(Box::new(crate::tools::ReadFile::default()));
        registry.register(Box::new(crate::tools::ListDir::default()));
        registry.register(Box::new(crate::tools::ModelsConfig::new()));
        registry.register(Box::new(crate::tools::WebSearch::new()));
        registry.register(Box::new(crate::tools::WebFetch::new()));
        registry.register(Box::new(crate::tools::TavilyMcpSearch::new()));
        registry.register(Box::new(crate::tools::TavilyMcpExtract::new()));
        registry.register(Box::new(crate::tools::Context7McpResolveLibraryId::new()));
        registry.register(Box::new(crate::tools::Context7McpGetLibraryDocs::new()));

        let all_names: Vec<String> = registry.list_tools();
        assert_eq!(all_names.len(), 11, "precondition: all registered");
        assert!(all_names.contains(&"write_file".to_string()));

        let read_only = registry.into_read_only();
        let names: Vec<String> = read_only.list_tools();
        assert_eq!(names.len(), 10, "only write_file dropped");
        assert!(!names.contains(&"write_file".to_string()));
        for expected in [
            "calculator",
            "ReadFile",
            "ListDir",
            "ModelsConfig",
            "WebSearch",
            "WebFetch",
            "tavily_search",
            "tavily_extract",
            "context7_resolve_library_id",
            "context7_get_library_docs",
        ] {
            assert!(
                names.contains(&expected.to_string()),
                "read tool `{expected}` must survive the filter"
            );
        }

        // The LLM-facing tool list reflects the same filtered set.
        let for_llm = read_only.get_all_for_llm();
        assert_eq!(for_llm.len(), 10);
        assert!(!for_llm.iter().any(|t| t.name == "write_file"));
    }

    #[test]
    fn into_read_only_carries_suppression_state_over() {
        let mut registry = ToolRegistry::new();
        registry.register(dummy("Calc", ToolSource::Builtin));
        registry.register(write_dummy("write_file"));
        registry.set_suppressed("Calc", false);

        let read_only = registry.into_read_only();
        assert!(read_only.is_suppressed("Calc"));
        assert!(read_only.get_all_for_llm().is_empty());
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

    // --- item48: per-MCP-server suppression (POST /api/mcp) -----------------

    fn serverless_dummy(name: &'static str, source: ToolSource) -> Box<dyn Tool> {
        // `dummy` never sets mcp_server, so it reports None even for Mcp
        // tools — exactly the pre-item48 shape. Used to prove serverless Mcp
        // tools are invisible to the per-server toggle.
        dummy(name, source)
    }

    #[test]
    fn mcp_servers_lists_sorted_deduped_builtin_only_servers() {
        let mut registry = ToolRegistry::new();
        registry.register(serverless_dummy("Calc", ToolSource::Builtin));
        registry.register(serverless_dummy("orphan", ToolSource::Mcp));
        assert!(
            registry.mcp_servers().is_empty(),
            "no tool reports a server yet"
        );

        registry.register(Box::new(crate::tools::TavilyMcpSearch::new()));
        registry.register(Box::new(crate::tools::TavilyMcpExtract::new()));
        registry.register(Box::new(crate::tools::Context7McpResolveLibraryId::new()));
        registry.register(Box::new(crate::tools::Context7McpGetLibraryDocs::new()));
        assert_eq!(
            registry.mcp_servers(),
            vec!["context7".to_string(), "tavily".to_string()]
        );
    }

    #[test]
    fn set_mcp_suppressed_disables_only_that_servers_tools() {
        let mut registry = ToolRegistry::new();
        registry.register(dummy("WebSearch", ToolSource::Builtin));
        registry.register(Box::new(crate::tools::TavilyMcpSearch::new()));
        registry.register(Box::new(crate::tools::TavilyMcpExtract::new()));
        registry.register(Box::new(crate::tools::Context7McpResolveLibraryId::new()));

        assert_eq!(registry.get_all_for_llm().len(), 4);

        registry.set_mcp_suppressed("tavily", false);

        // Registry-level: the whole tavily server disappears from the
        // LLM-facing tool list (an agent run would not see it); builtins
        // and the other server stay. (HashMap order → compare as a set.)
        let for_llm: Vec<String> = registry
            .get_all_for_llm()
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(for_llm.len(), 2);
        assert!(for_llm.contains(&"WebSearch".to_string()));
        assert!(for_llm.contains(&"context7_resolve_library_id".to_string()));
        assert!(registry.is_suppressed("tavily_search"));
        assert!(registry.is_suppressed("tavily_extract"));
        assert!(!registry.is_suppressed("context7_resolve_library_id"));

        // Per-server enabled state mirrors the suppression.
        assert!(!registry.mcp_server_enabled("tavily"));
        assert!(registry.mcp_server_enabled("context7"));

        // Re-enable restores every tool of the server.
        registry.set_mcp_suppressed("tavily", true);
        assert_eq!(registry.get_all_for_llm().len(), 4);
        assert!(!registry.is_suppressed("tavily_search"));
        assert!(registry.mcp_server_enabled("tavily"));
    }

    #[test]
    fn set_mcp_suppressed_unknown_server_is_noop() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(crate::tools::TavilyMcpSearch::new()));
        registry.set_mcp_suppressed("not-a-server", false);
        registry.set_mcp_suppressed("", false);
        assert_eq!(registry.get_all_for_llm().len(), 1);
        assert!(registry.mcp_server_enabled("tavily"));
    }

    #[test]
    fn default_mcp_server_is_none() {
        let mut registry = ToolRegistry::new();
        registry.register(serverless_dummy("orphan", ToolSource::Mcp));
        // A serverless Mcp tool (pre-item48 shape) is never matched by the
        // per-server toggle: "tavily" is unknown and suppression is a noop.
        registry.set_mcp_suppressed("tavily", false);
        assert_eq!(registry.get_all_for_llm().len(), 1);
        assert!(registry.mcp_servers().is_empty());
    }
}
