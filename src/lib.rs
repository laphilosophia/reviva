pub mod backend;
pub mod core;
pub mod export;
pub mod prompts;
pub mod repo;
pub mod storage;

mod cli;

pub fn run() -> Result<(), String> {
    cli::run()
}
