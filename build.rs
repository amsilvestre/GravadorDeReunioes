fn main() {
    slint_build::compile("ui/main.slint").unwrap();

    let mut res = winres::WindowsResource::new();
    res.set_icon("Double-J-Design-Ravenna-3d-Record.ico");
    res.compile().unwrap();

    // Linkar com bibliotecas CUDA no Windows
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rustc-link-lib=cudart");
        println!("cargo:rustc-link-lib=cublas");
        println!("cargo:rustc-link-lib=cublasLt");
    }
}
