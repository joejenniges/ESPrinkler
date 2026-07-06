fn main() {
    embuild::espidf::sysenv::output();
    // MELNOR_VALVES is read at compile time via option_env!; force a rebuild
    // when it changes so `./flash.sh <chip> <valves>` takes effect.
    println!("cargo:rerun-if-env-changed=MELNOR_VALVES");
}
