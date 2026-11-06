use anyhow::Result;
use backend::graphql::{create_anonymous_schema, create_schema};
use std::fs;

fn main() -> Result<()> {
    fs::create_dir_all("target")?;
    write_graphql_schema()?;
    write_anonymous_graphql_schema()?;
    Ok(())
}

/// Also registers it for cynic as "authenticated" (`graphql::authenticated::schema`), which
/// checks every query struct naming it against it.
fn write_graphql_schema() -> Result<()> {
    let schema = create_schema(Default::default());
    fs::write("src/graphql/authenticated/schema.graphql", schema.sdl())?;
    cynic_codegen::register_schema("authenticated")
        .from_sdl_file("src/graphql/authenticated/schema.graphql")?;
    Ok(())
}
fn write_anonymous_graphql_schema() -> Result<()> {
    let schema = create_anonymous_schema();
    fs::write("target/anonymous_schema.graphql", schema.sdl())?;
    Ok(())
}
