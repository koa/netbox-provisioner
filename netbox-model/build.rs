fn main() {
    cynic_codegen::register_schema("netbox")
        .from_sdl_file("schemas/netbox.graphql")
        .unwrap()
        .as_default()
        .unwrap();
}
