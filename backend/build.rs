/// Registers NetBox's schema for cynic (`netbox::fetch_topology`), which checks the query
/// against it.
fn main() {
    cynic_codegen::register_schema("netbox")
        .from_sdl_file("src/netbox/schema.graphqls")
        .expect("NetBox schema")
        .as_default()
        .expect("NetBox schema as default");
}
