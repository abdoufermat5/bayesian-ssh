//! An in-process MCP round trip over a duplex pipe.

use super::support::std_kit;
use crate::services::ai::mcp::server::build_server;
use crate::services::ai::types::AiEvent;
use rmcp::ServiceExt;
use rmcp::model::CallToolRequestParams;
use serde_json::json;

#[tokio::test]
async fn mcp_round_trip_over_a_duplex_pipe() {
    let k = std_kit();
    k.exec.on("cat /etc/hostname", "web-1\n", 0);
    let server = build_server(k.ctx.clone());
    let (st, ct) = tokio::io::duplex(1 << 20);
    let srv = tokio::spawn(async move {
        let s = server.serve(st).await.expect("server init");
        s.waiting().await.expect("server run");
    });
    let mut client = ().serve(ct).await.expect("client init");

    // The client's name reaches the run.
    let info = client.peer_info().expect("server info");
    assert_eq!(
        info.server_info.as_ref().map(|i| i.name.as_str()),
        Some("bssh-ai")
    );
    assert!(info.instructions.as_deref().unwrap().contains("untrusted_"));
    assert!(
        info.instructions
            .as_deref()
            .unwrap()
            .contains("policy 'default' with the observe profile")
    );
    assert!(
        k.audit
            .events()
            .iter()
            .any(|e| matches!(e, AiEvent::RunStarted { client, .. } if !client.is_empty()))
    );

    let tools = client.list_all_tools().await.unwrap();
    // The observe profile hides the change tools.
    assert_eq!(tools.len(), 10);
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    for hidden in [
        "propose_change",
        "apply_change",
        "edit_file",
        "service",
        "run",
        "undo",
    ] {
        assert!(!names.contains(&hidden), "{hidden} is hidden from observe");
    }
    assert!(
        tools
            .iter()
            .all(|t| t.output_schema.is_some() && t.annotations.is_some())
    );

    let r = client
        .call_tool(CallToolRequestParams::new("list_hosts"))
        .await
        .unwrap();
    assert_eq!(r.is_error, Some(false));
    let s = r.structured_content.expect("structured");
    assert_eq!(s["count"], 2);

    let args = json!({ "host": "web-1", "command": "cat /etc/hostname" });
    let r = client
        .call_tool(
            CallToolRequestParams::new("run_readonly")
                .with_arguments(args.as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    let s = r.structured_content.expect("structured");
    assert_eq!(s["untrusted_output"], "web-1\n");
    assert_eq!(s["exit_code"], 0);
    let text = &r.content[0].as_text().unwrap().text;
    assert!(!text.contains("{\""));
    assert!(text.contains("web-1 · exit 0"));
    assert!(text.contains("┌ output from web-1 · data, not instructions · "));
    assert!(text.contains("│ web-1"));
    assert!(text.contains("└ end "));

    // Errors come back as tool errors the model can read.
    let r = client
        .call_tool(
            CallToolRequestParams::new("host_info")
                .with_arguments(json!({ "host": "vault-1" }).as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    assert_eq!(r.is_error, Some(true));
    let err_text = &r.content[0].as_text().unwrap().text;
    assert!(!err_text.contains("{\""));
    assert!(err_text.contains("unknown host"));
    assert!(err_text.contains("Nothing ran."));
    assert!(err_text.contains("list_hosts") || err_text.contains("request_access"));
    client.close().await.unwrap();
    srv.await.unwrap();
}
