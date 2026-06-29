fn main() {
    slint_build::compile("src/ui/app.slint").unwrap();

    #[cfg(target_os = "macos")]
    {
        for lib_dir in ["/opt/homebrew/lib", "/usr/local/lib"] {
            if std::path::Path::new(lib_dir).join("libmpv.dylib").exists() {
                println!("cargo:rustc-link-search=native={lib_dir}");
                break;
            }
        }
    }
}