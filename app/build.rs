fn main() {
    glib_build_tools::compile_resources(&["../data"], "../data/sticky.gresource.xml", "sticky.gresource");
}
