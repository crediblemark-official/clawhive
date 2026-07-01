/// Bootstrap prompt, loaded from external ICVS file.
pub const BOOTSTRAP_SOURCE: &str = include_str!("../../prompts/bootstrap.icvs");

#[cfg(test)]
#[path = "bootstrap_test.rs"]
mod tests;
