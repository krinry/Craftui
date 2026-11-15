pub mod prompts;
pub mod protocol;
pub mod resources;
pub mod server;
pub mod tools;

#[cfg(test)]
mod tests;

use craftui_core::Result;

pub fn run_mcp_server(stdio: bool) -> Result<()> {
    if stdio {
        server::run_stdio_server()
    } else {
        Err(craftui_core::CraftError::Platform(
            "Only stdio transport is currently supported for CraftUI MCP server. Run with `craftui mcp --stdio`.".into(),
        ))
    }
}
