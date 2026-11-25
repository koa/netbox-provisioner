use anyhow::Result;
use backend::graphql::create_schema;
use std::fs;

/// Writes the server's schema and registers it for cynic as the default
/// (`graphql::schema`), which checks every query struct against it.
fn main() -> Result<()> {
    fs::create_dir_all("target")?;
    let schema = create_schema(Default::default());
    fs::write("target/schema.graphql", schema.sdl())?;
    cynic_codegen::register_schema("provisioner")
        .from_sdl_file("target/schema.graphql")?
        .as_default()?;
    Ok(())
}
