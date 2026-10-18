fn main() {
    println!("cargo::rerun-if-changed=cpp/stretch_wrapper.cpp");
    println!("cargo::rerun-if-changed=../../third_party/signalsmith-stretch/signalsmith-stretch.h");
    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .file("cpp/stretch_wrapper.cpp")
        .include("../../third_party")
        .flag_if_supported("/EHsc")
        .warnings(false)
        .opt_level(3)
        .compile("vss_stretch");
}
