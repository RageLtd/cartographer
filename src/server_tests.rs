use super::*;

#[test]
fn tool_router_lists_exactly_the_code_graph_tools() {
    let mut names: Vec<String> = CartographerServer::tool_router()
        .list_all()
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect();
    names.sort();

    assert_eq!(
        names,
        [
            "cartographer_detect_changes",
            "cartographer_find_cycles",
            "cartographer_get_file_info",
            "cartographer_index_project",
            "cartographer_parse_file",
            "cartographer_query",
            "cartographer_search",
            "cartographer_stats",
        ]
    );
}
