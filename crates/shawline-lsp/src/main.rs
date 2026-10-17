//! `shawline-lsp`: the Shawline language server over stdio.

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (connection, io_threads) = lsp_server::Connection::stdio();
    shawline_lsp::run(&connection)?;
    drop(connection);
    io_threads.join()?;
    Ok(())
}
