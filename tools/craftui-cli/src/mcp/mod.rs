pub mod prompts;
pub mod protocol;
pub mod resources;
pub mod server;
pub mod tools;

#[cfg(test)]
mod tests;

use craftui_core::Result;

pub fn run_mcp_server(stdio: bool, _port: Option<u16>) -> Result<()> {
    if stdio {
        server::run_stdio_server()
    } else {
        // Default to stdio for CLI MCP integrations
        server::run_stdio_server()
    }
}
