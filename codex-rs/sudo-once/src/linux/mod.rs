#[cfg(test)]
mod auth;
mod sealed_executable;
#[cfg(test)]
mod wire;

pub use sealed_executable::SealedSudoExecutable;

pub const fn sudo_once_available() -> bool {
    false
}

pub const fn try_dispatch_helper_from_env() -> Option<i32> {
    None
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
