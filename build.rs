fn main() {
    embuild::espidf::sysenv::output();
    // config.toml is baked in via include_str!; rebuild when it changes.
    println!("cargo:rerun-if-changed=config.toml");
}
