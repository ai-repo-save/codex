mod auth;
mod sealed_executable;
mod wire;

pub use sealed_executable::SealedSudoExecutable;

pub fn sudo_once_available() -> bool {
    auth::runtime_capability().is_ok() && sealed_executable::kernel_supports_sealed_executable()
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
