use super::*;
use claw10_icvs::IcvsCompiler;

#[test]
fn test_bootstrap_compiles() {
    let prompts = IcvsCompiler::compile_prompt(BOOTSTRAP_SOURCE, "bootstrap").unwrap();
    assert_eq!(prompts.len(), 2);
    assert!(prompts[0].content.contains("wawancara singkat"));
}
