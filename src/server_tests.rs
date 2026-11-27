use super::*;
use crate::db::client::open;

const PROJECT_RESOURCE: &str = "cartographer://project";

fn tool_names(router: &ToolRouter<CartographerServer>) -> Vec<String> {
    let mut names: Vec<String> = router
        .list_all()
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect();
    names.sort();
    names
}

fn registry() -> Arc<LanguageRegistry> {
    Arc::new(LanguageRegistry::builtin())
}

/// A server with an index, in a temporary directory the caller keeps alive.
fn full_server() -> (CartographerServer, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db = open(&dir.path().join("cartographer.db")).unwrap();
    (CartographerServer::new(db, registry()), dir)
}

// A program driving a parse-only server reads the tool list to learn what it
// can use, so a tool that could only answer "there is no index" is not on it.
#[test]
fn a_parse_only_server_lists_only_the_tools_that_need_no_index() {
    let server = CartographerServer::parse_only(registry());

    assert_eq!(
        tool_names(&server.tool_router),
        ["cartographer_parse_file", "cartographer_uses_in_file"]
    );
}

#[test]
fn a_parse_only_server_offers_no_resources() {
    let server = CartographerServer::parse_only(registry());

    assert!(server.resources().is_empty());
}

#[test]
fn a_server_with_an_index_offers_the_project_resource() {
    let (server, _dir) = full_server();

    let uris: Vec<String> = server
        .resources()
        .iter()
        .map(|resource| resource.uri.clone())
        .collect();

    assert_eq!(uris, [PROJECT_RESOURCE]);
}

#[test]
fn a_server_with_an_index_lists_every_tool() {
    let (server, _dir) = full_server();

    assert_eq!(
        tool_names(&server.tool_router),
        tool_names(&CartographerServer::tool_router())
    );
}

#[test]
fn tool_router_lists_exactly_the_code_graph_tools() {
    let names = tool_names(&CartographerServer::tool_router());

    assert_eq!(
        names,
        [
            "cartographer_detect_changes",
            "cartographer_find_cycles",
            "cartographer_find_uses",
            "cartographer_get_file_info",
            "cartographer_index_project",
            "cartographer_parse_file",
            "cartographer_query",
            "cartographer_search",
            "cartographer_stats",
            "cartographer_uses_in_file",
        ]
    );
}
