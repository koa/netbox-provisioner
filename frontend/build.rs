use anyhow::Result;
use backend::graphql::{create_anonymous_schema, create_schema};
use std::fs;

fn main() -> Result<()> {
    fs::create_dir_all("target")?;
    write_graphql_schema()?;
    write_anonymous_graphql_schema()?;
    Ok(())
}

fn write_graphql_schema() -> Result<()> {
    let schema = create_schema(Default::default());
    fs::write("target/authenticated_schema.graphql", schema.sdl())?;
    Ok(())
}
fn write_anonymous_graphql_schema() -> Result<()> {
    let schema = create_anonymous_schema();
    fs::write("target/anonymous_schema.graphql", schema.sdl())?;
    Ok(())
}
