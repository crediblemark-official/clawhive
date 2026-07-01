/// Memory distillation prompt, loaded from external ICVS file.
pub const DISTILLATION_SOURCE: &str = include_str!("../../prompts/distillation.icvs");

#[cfg(test)]
#[path = "distillation_test.rs"]
mod tests;
