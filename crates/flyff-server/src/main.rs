//! The `flyff` executable: a thin `main` over the library (Plan/02 §1.1).

#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
    // Only `main` calls `process::exit` (Plan/10 §2.4).
    let code = flyff_server::cli::run_main(std::env::args_os());
    std::process::exit(code);
}
